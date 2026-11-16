"""Tier-1 closure ratchet: the `unsafe` linked into Tier-1 cells, beyond `cells/`.

`#![forbid(unsafe_code)]` is per crate, so a forbid-clean cell still links every
`unsafe` line of `libs/ostd`, `libs/wire`, the vendored `virtio-drivers` and the
registry crates it depends on. A memory bug there defeats language isolation
for every Tier-1 cell. This module holds that code to reviewed ceilings.

Inputs, both committed:
  * `scripts/tier1-closure.generated.toml` — written by
    `scripts/gen-tier1-closure.py` from `cargo metadata --locked --offline
    --all-features` (three bare-metal targets, normal edges). It lists the
    Tier-1 roots, the in-tree packages they link (in-tree proc-macros among
    them), every third-party package pinned by name, version and full source
    string with its counts, every third-party proc-macro pinned the same way,
    and the `[[bin]]` targets of every `cells/` workspace member. CI regenerates
    it and fails on any drift, so a new dependency, a version or git-revision
    bump or a changed count of a third-party crate needs a reviewed diff. The
    signer reads only the committed file and so needs no cargo, build or
    network.
  * `scripts/tcb-unsafe-baseline.toml` — one `[[site]]` row per in-tree closure
    file that uses `unsafe`, an unsafe attribute or `global_asm!`, with two
    ceilings (`unsafe` tokens, `attrs`), and a `[total]` ceiling over the
    Tier-1 cells plus the closure.

Rules (same lexer and token rule as the F1 cell scan):
  * a closure file above either ceiling, or with a count and no row: violation;
  * a combined total above its ceiling: violation. The total counts the Tier-1
    cells too, so `unsafe` moved from cells into a library passes only when the
    sum does not grow;
  * tightening findings — a closure file or the total below its ceiling, a row
    for a file that is gone or outside the closure, a Tier-1 root that is no
    longer one, and a Tier-1 cell missing from the closure file's roots (a new
    cell crate, until the file is regenerated): NOTE, and a violation under
    `--check --strict` (CI), a NOTE on `--sign`. A drop never lets more
    `unsafe` into a signed cell than the ceilings admit. A missing root is the
    one gap: the new cell's own files count in the Tier-1 total at once, but an
    in-tree crate only it links is unceilinged until the closure file is
    regenerated, which CI enforces before merge. `cellos-sign --write-baseline`
    lowers the ceilings to the tree (never raises one).

Counting: every git-tracked `.rs` file the package owns (deepest package
directory wins), except the package's top-level `tests/`, `benches/`,
`examples/` and `build.rs`, which never link into a cell. `#[cfg(test)]`
modules inside `src/` are counted: the rule cannot see `cfg`. Code a build
script writes to `OUT_DIR` is not on disk to count; the generator fails if a
closure crate `include!`s `OUT_DIR` output while its build script (or an
in-tree build-dependency) spells any counted token.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from . import scan

CLOSURE_PATH = "scripts/tier1-closure.generated.toml"
BASELINE_PATH = "scripts/tcb-unsafe-baseline.toml"
_UNLINKED_DIRS = frozenset({"tests", "benches", "examples", "target"})


class TcbError(ValueError):
    """The closure or baseline file is missing or malformed. Fail the run."""


@dataclass(frozen=True)
class Counts:
    unsafe: int = 0
    attrs: int = 0

    def __add__(self, other: "Counts") -> "Counts":
        return Counts(self.unsafe + other.unsafe, self.attrs + other.attrs)

    def __bool__(self) -> bool:
        return bool(self.unsafe or self.attrs)


@dataclass
class Closure:
    tier1_roots: set[str]
    packages: dict[str, str]  # in-tree package name → repo-relative directory
    unresolved: dict[str, str]  # repo-relative cell directory → reason
    thirdparty: list[dict]
    # Repo-relative `cells/` crate directory → {bin target name: repo-relative root}.
    bins: dict[str, dict[str, str]] = field(default_factory=dict)
    proc_macros: list[dict] = field(default_factory=list)


@dataclass
class Baseline:
    sites: dict[str, Counts]
    total: Counts


@dataclass(frozen=True)
class Finding:
    """`strict_only`: a NOTE that becomes a violation under `--strict`."""

    crate: str
    path: str
    detail: str
    strict_only: bool = False


@dataclass
class Report:
    findings: list[Finding] = field(default_factory=list)
    per_package: dict[str, Counts] = field(default_factory=dict)
    per_file: dict[str, Counts] = field(default_factory=dict)
    closure: Counts = Counts()
    cells: Counts = Counts()
    thirdparty_pins: int = 0
    thirdparty: Counts = Counts()

    @property
    def total(self) -> Counts:
        return self.closure + self.cells


def _read_toml(repo: Path, relative: str) -> dict:
    try:
        return tomllib.loads((repo / relative).read_text(encoding="utf-8"))
    except OSError as exc:
        raise TcbError(f"cannot read {relative}: {exc}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise TcbError(f"{relative} is not valid TOML: {exc}") from exc


def _int(value: object, where: str) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0:
        raise TcbError(f"{where}: expected a non-negative integer, got {value!r}")
    return value


def _keys(raw: dict, required: set[str], where: str, optional: frozenset[str] = frozenset()) -> None:
    """`raw` has every `required` key and nothing outside `required | optional`."""
    missing = required - set(raw)
    unknown = set(raw) - required - optional
    if missing or unknown:
        raise TcbError(f"{where}: missing {sorted(missing)} / unknown {sorted(unknown)} keys")


def load_closure(repo: Path, relative: str = CLOSURE_PATH) -> Closure:
    data = _read_toml(repo, relative)
    if data.get("version") != 1:
        raise TcbError(f"{relative}: unsupported `version` {data.get('version')!r}")
    _keys(data, {"version", "targets", "tier1_roots"}, relative,
          optional=frozenset({"package", "thirdparty", "proc_macro", "bin", "unresolved"}))
    packages: dict[str, str] = {}
    for raw in data.get("package", []):
        _keys(raw, {"name", "path"}, f"{relative} [[package]]", optional=frozenset({"kind"}))
        if raw["name"] in packages:
            raise TcbError(f"{relative}: duplicate [[package]] {raw['name']}")
        packages[raw["name"]] = raw["path"]
    unresolved: dict[str, str] = {}
    for raw in data.get("unresolved", []):
        _keys(raw, {"path", "reason"}, f"{relative} [[unresolved]]")
        unresolved[raw["path"]] = raw["reason"]
    bins: dict[str, dict[str, str]] = {}
    for raw in data.get("bin", []):
        where = f"{relative} [[bin]] {raw.get('name')}"
        _keys(raw, {"name", "package", "dir", "src"}, where)
        if raw["name"] in bins.setdefault(raw["dir"], {}):
            raise TcbError(f"{where}: duplicate bin in {raw['dir']}")
        bins[raw["dir"]][raw["name"]] = raw["src"]
    pins: set[tuple[str, str, str]] = set()
    for table, counted in (("thirdparty", True), ("proc_macro", False)):
        for raw in data.get(table, []):
            where = f"{relative} [[{table}]] {raw.get('name')}@{raw.get('version')}"
            required = {"name", "version", "source"} | ({"unsafe", "attrs"} if counted else set())
            _keys(raw, required, where)
            if counted:
                _int(raw["unsafe"], where)
                _int(raw["attrs"], where)
            pin = (raw["name"], raw["version"], raw["source"])
            if pin in pins:
                raise TcbError(f"{where}: duplicate pin")
            pins.add(pin)
    return Closure(
        tier1_roots=set(data["tier1_roots"]),
        packages=packages,
        unresolved=unresolved,
        thirdparty=list(data.get("thirdparty", [])),
        bins=bins,
        proc_macros=list(data.get("proc_macro", [])),
    )


def load_baseline(repo: Path, relative: str = BASELINE_PATH) -> Baseline:
    data = _read_toml(repo, relative)
    if data.get("version") != 1:
        raise TcbError(f"{relative}: unsupported `version` {data.get('version')!r}")
    _keys(data, {"version", "total"}, relative, optional=frozenset({"site"}))
    _keys(data["total"], {"unsafe", "attrs"}, f"{relative} [total]")
    total = Counts(_int(data["total"]["unsafe"], "[total]"), _int(data["total"]["attrs"], "[total]"))
    sites: dict[str, Counts] = {}
    for raw in data.get("site", []):
        where = f"{relative} [[site]] {raw.get('path')}"
        _keys(raw, {"path", "unsafe", "attrs"}, where)
        if raw["path"] in sites:
            raise TcbError(f"{where}: duplicate row")
        counts = Counts(_int(raw["unsafe"], where), _int(raw["attrs"], where))
        if not counts:
            raise TcbError(f"{where}: a row with no count is not a ceiling — delete it")
        sites[raw["path"]] = counts
    return Baseline(sites=sites, total=total)


def count(code: str) -> Counts:
    """Counts for one file's reduced source."""
    return Counts(scan.count_unsafe(code), scan.count_unsafe_attrs(code))


def linked_sources(package_dir: Path, files: list[Path]) -> list[Path]:
    """The files of `package_dir` that can link into a cell.

    Drops files owned by a nested package (a directory below `package_dir` with
    its own `Cargo.toml`), the package's top-level `tests/`, `benches/`,
    `examples/` and `build.rs`, and anything under `target/`.
    """
    nested: dict[Path, bool] = {}

    def is_nested(directory: Path) -> bool:
        if directory not in nested:
            nested[directory] = (directory / "Cargo.toml").is_file()
        return nested[directory]

    kept = []
    for path in files:
        rel = path.relative_to(package_dir)
        if rel.parts[0] in _UNLINKED_DIRS or rel.as_posix() == "build.rs":
            continue
        if "target" in rel.parts:
            continue
        parents = [package_dir.joinpath(*rel.parts[:i]) for i in range(1, len(rel.parts))]
        if any(is_nested(p) for p in parents):
            continue
        kept.append(path)
    return sorted(kept)


def _package_name(manifest: Path) -> str | None:
    try:
        return tomllib.loads(manifest.read_text(encoding="utf-8")).get("package", {}).get("name")
    except (OSError, tomllib.TOMLDecodeError):
        return None


def check(
    repo: Path,
    closure: Closure,
    baseline: Baseline,
    tier1_cells: dict[str, str],
    cells: Counts,
    extra: dict[str, str] | None = None,
) -> Report:
    """Hold the in-tree closure to the baseline. Pure source parsing.

    `tier1_cells` maps each Tier-1 cell crate under `cells/` to its directory;
    `cells` is their combined count, from the F1 token pass. `extra` appends
    reduced code to repo-relative files (an existing or a new one), so a
    negative control can add `unsafe` without writing to the tree.
    """
    repo = repo.resolve()
    extra = extra or {}
    report = Report(cells=cells)

    # ── The closure file must cover every Tier-1 cell ─────────────────────────
    for name, directory in sorted(tier1_cells.items()):
        if name not in closure.tier1_roots and directory not in closure.unresolved:
            report.findings.append(Finding(
                name, directory,
                f"Tier-1 cell is not a root in {CLOSURE_PATH}; in-tree crates only it "
                "links are unceilinged — run scripts/gen-tier1-closure.py",
                strict_only=True,
            ))
    for name in sorted(closure.tier1_roots - set(tier1_cells)):
        report.findings.append(Finding(
            name, CLOSURE_PATH,
            "listed as a Tier-1 root but no longer a Tier-1 cell — run "
            "scripts/gen-tier1-closure.py",
            strict_only=True,
        ))

    # ── In-tree closure packages, per file ────────────────────────────────────
    for name, directory in sorted(closure.packages.items()):
        package_dir = repo / directory
        if _package_name(package_dir / "Cargo.toml") != name:
            report.findings.append(Finding(
                name, directory,
                f"{CLOSURE_PATH} names package `{name}` here, but no such package "
                "exists — run scripts/gen-tier1-closure.py",
            ))
            continue
        files = {repo / rel for rel in scan.tracked_sources(repo, [directory])}
        files |= {repo / rel for rel in extra if rel.startswith(directory + "/")}
        total = Counts()
        for path in linked_sources(package_dir, sorted(files)):
            rel = path.relative_to(repo).as_posix()
            code = scan.read_code(path) if path.is_file() else ""
            counts = count(code + "\n" + extra.get(rel, ""))
            if not counts:
                continue
            report.per_file[rel] = counts
            total = total + counts
        report.per_package[name] = total
        report.closure = report.closure + total

    for rel, counts in sorted(report.per_file.items()):
        crate = next(n for n, d in closure.packages.items() if rel.startswith(d + "/"))
        ceiling = baseline.sites.get(rel)
        if ceiling is None:
            report.findings.append(Finding(
                crate, rel,
                f"Tier-1 closure file has {counts.unsafe} `unsafe` and {counts.attrs} unsafe "
                f"attribute(s) and no row in {BASELINE_PATH}",
            ))
            continue
        for label, now, cap in (("`unsafe`", counts.unsafe, ceiling.unsafe),
                                ("unsafe attribute", counts.attrs, ceiling.attrs)):
            if now > cap:
                report.findings.append(Finding(
                    crate, rel, f"{now} {label} site(s) exceed the ceiling of {cap}"))
            elif now < cap:
                report.findings.append(Finding(
                    crate, rel,
                    f"{now} {label} site(s) are below the ceiling of {cap} — lower it",
                    strict_only=True,
                ))
    for rel in sorted(set(baseline.sites) - set(report.per_file)):
        report.findings.append(Finding(
            "<baseline>", rel,
            "row names a file that is gone, outside the closure or free of unsafe — delete it",
            strict_only=True,
        ))

    # ── Combined total: Tier-1 cells plus the closure ─────────────────────────
    where = f"{BASELINE_PATH} [total]"
    for label, attr in (("`unsafe`", "unsafe"), ("unsafe attribute", "attrs")):
        now, cap = getattr(report.total, attr), getattr(baseline.total, attr)
        split = f"cells {getattr(report.cells, attr)} + closure {getattr(report.closure, attr)}"
        if now > cap:
            report.findings.append(Finding(
                "<total>", where, f"Tier-1 {label} total {now} ({split}) exceeds {cap}"))
        elif now < cap:
            report.findings.append(Finding(
                "<total>", where,
                f"Tier-1 {label} total {now} ({split}) is below {cap} — lower it",
                strict_only=True,
            ))

    report.thirdparty_pins = len(closure.thirdparty)
    for raw in closure.thirdparty:
        report.thirdparty = report.thirdparty + Counts(raw["unsafe"], raw["attrs"])
    return report


def lowered_baseline(text: str, report: Report) -> str:
    """`text` (the baseline file) with every ceiling lowered to the tree.

    Mechanical and one-way: each `[[site]]` ceiling and each `[total]` ceiling
    becomes min(ceiling, current count); a row whose file now counts 0/0 (gone,
    clean or outside the closure) is deleted with the blank line after it. A
    count above its ceiling keeps the ceiling, and a file without a row gets
    none, so the check still fails for those: raising a ceiling stays a
    reviewed hand edit. Comments and layout are kept.
    """
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    table, current = None, Counts()
    i = 0
    while i < len(lines):
        line = lines[i]
        stripped = line.strip()
        if stripped == "[[site]]":
            block = [line]
            j = i + 1
            while j < len(lines) and lines[j].strip() and not lines[j].lstrip().startswith("["):
                block.append(lines[j])
                j += 1
            fields = {ln.split("=", 1)[0].strip(): ln.split("=", 1)[1].split("#", 1)[0].strip()
                      for ln in block[1:] if "=" in ln}
            path = fields.get("path", "").strip('"')
            current = report.per_file.get(path, Counts())
            cap = Counts(int(fields.get("unsafe", 0)), int(fields.get("attrs", 0)))
            if not Counts(min(cap.unsafe, current.unsafe), min(cap.attrs, current.attrs)):
                if j < len(lines) and not lines[j].strip():
                    j += 1  # the separating blank line
                i = j
                continue
            table = "site"
            out.append(line)
            i += 1
            continue
        if stripped.startswith("["):
            table = "total" if stripped == "[total]" else None
            current = report.total
        key, sep, value = line.partition("=")
        if sep and table and key.strip() in ("unsafe", "attrs"):
            cap = int(value.split("#", 1)[0])
            lowered = min(cap, getattr(current, key.strip()))
            line = f"{key}= {lowered}\n"
        out.append(line)
        i += 1
    return "".join(out)
