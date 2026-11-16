#!/usr/bin/env python3
"""Per-architecture implementation status, generated from the tree.

Writes docs/generated/arch-status.md: one row per kernel architecture
(x86_64, riscv64, aarch64) plus the 32-bit legacy set. Every cell is read from
a source file, never from prose:

    kernel module      kernel/src/arch/mod.rs (`mod <arch>` and its cfg),
                       file count under kernel/src/arch/<dir>/
    64-bit set         kernel/src/posture.rs::wide (build.rs emits cellos_wide)
    Tier-2 admission   kernel/src/loader/domain_admission.rs::switch_ordering_qualified
    domain grants      kernel/src/posture.rs::domain_grants (cfg cellos_domain_grants)
    Tier-3 hypervisor  kernel/src/arch/<dir>/hypervisor/ exists
    MAX_HARTS          kernel/src/sched/smp.rs::MAX_HARTS (cfg per arch)
    toolchain target   rust-toolchain.toml `targets`
    workflow lines     .github/workflows/*.yml lines naming the arch's target
    lanes              scripts/lanes.toml rows per arch and tier; `default`
                       rows count for the arch of scripts/cargo-config-linux.toml
                       `[build] target`
    boards             boards/**/board.rs `architecture:` and `slug:`

A probe that no longer matches its source is a hard error (exit 2), so the
table cannot silently describe code that has moved.

Modes:
  (default)  rewrite docs/generated/arch-status.md
  --check    do not write; exit 1 when the committed file is stale
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "docs" / "generated" / "arch-status.md"

ARCH_MOD = "kernel/src/arch/mod.rs"
POSTURE = "kernel/src/posture.rs"
ADMISSION = "kernel/src/loader/domain_admission.rs"
SMP = "kernel/src/sched/smp.rs"
TOOLCHAIN = "rust-toolchain.toml"
CARGO_CONFIG = "scripts/cargo-config-linux.toml"
LANES = "scripts/lanes.toml"
WORKFLOWS = ".github/workflows"
BOARDS = "boards"

# Row key, kernel `target_arch` values, arch module directory, workflow regex
# for the kernel target triple(s), board `Architecture` variant.
ROWS = (
    ("x86_64", ("x86_64",), "x86_64", r"\bx86_64-unknown-none\b", "X86_64"),
    ("riscv64", ("riscv64",), "riscv64", r"\briscv64gc-unknown-none-elf\b", "Riscv64"),
    ("aarch64", ("aarch64",), "aarch64", r"\baarch64-unknown-none(?:-softfloat)?\b", "Aarch64"),
    (
        "32-bit legacy",
        ("riscv32", "x86", "arm"),
        "legacy",
        r"\briscv32\w*-unknown-none\w*|\bx86_32-unknown-none\b|\bi[3-6]86-unknown-none\b"
        r"|\barmv7\w*-none\w*|\bthumbv\w+-none\w*",
        None,
    ),
)

TIERS = ("pr", "nightly", "manual", "retired")


class ProbeError(Exception):
    pass


def text(rel: str) -> str:
    path = ROOT / rel
    if not path.is_file():
        raise ProbeError(f"{rel}: missing")
    return path.read_text(encoding="utf-8")


def fn_body(rel: str, name: str) -> str:
    """Body of the first `fn <name>` in `rel`, up to its closing brace."""
    src = text(rel)
    m = re.search(rf"\bfn\s+{name}\b[^{{]*\{{", src)
    if not m:
        raise ProbeError(f"{rel}: fn {name} not found")
    depth, i = 1, m.end()
    while depth and i < len(src):
        depth += {"{": 1, "}": -1}.get(src[i], 0)
        i += 1
    return src[m.end() : i - 1]


def probe_arch_modules() -> dict[str, str]:
    """`mod <dir>;` in kernel/src/arch/mod.rs and the cfg line above it."""
    lines = text(ARCH_MOD).splitlines()
    found = {}
    for i, line in enumerate(lines):
        m = re.match(r"\s*mod\s+(\w+)\s*;", line)
        if m and i and lines[i - 1].strip().startswith("#[cfg("):
            found[m.group(1)] = lines[i - 1].strip()[len("#[cfg(") : -2]
    for _, _, d, _, _ in ROWS:
        if d not in found:
            raise ProbeError(f"{ARCH_MOD}: no cfg-gated `mod {d};`")
    return found


def probe_wide() -> set[str]:
    m = re.search(r"matches!\(\s*arch\s*,\s*([^)]*)\)", fn_body(POSTURE, "wide"))
    if not m:
        raise ProbeError(f"{POSTURE}: wide() no longer a matches! over arch names")
    return set(re.findall(r'"(\w+)"', m.group(1)))


def probe_domain_grants() -> tuple[str, dict[str, str]]:
    """Feature that compiles the grant lifecycle, and per-arch extra features."""
    body = fn_body(POSTURE, "domain_grants")
    base = re.search(r'wide\(arch\)\s*&&\s*on\("([\w-]+)"\)', body)
    if not base:
        raise ProbeError(f"{POSTURE}: domain_grants() lost `wide(arch) && on(..)`")
    extra = dict(re.findall(r'arch\s*!=\s*"(\w+)"\s*\|\|\s*on\("([\w-]+)"\)', body))
    return base.group(1), extra


def probe_admission() -> dict[str, str | None]:
    """Arch → None (always qualified) or the feature it needs."""
    body = fn_body(ADMISSION, "switch_ordering_qualified")
    out: dict[str, str | None] = {}
    for arch, feat in re.findall(
        r'cfg!\(all\(target_arch\s*=\s*"(\w+)"\s*,\s*feature\s*=\s*"([\w-]+)"\)\)', body
    ):
        out[arch] = feat
    for arch in re.findall(r'cfg!\(target_arch\s*=\s*"(\w+)"\)', body):
        out[arch] = None
    if not out:
        raise ProbeError(f"{ADMISSION}: switch_ordering_qualified() has no target_arch cfg")
    return out


def resolve_const(expr: str) -> str:
    expr = expr.strip()
    if re.fullmatch(r"\d+", expr):
        return expr
    name = expr.split("::")[-1]
    for path in sorted((ROOT / "hal").rglob("*.rs")) + sorted((ROOT / "kernel/src").rglob("*.rs")):
        m = re.search(rf"\bconst\s+{name}\s*:\s*usize\s*=\s*(\d+)\s*;", path.read_text(encoding="utf-8"))
        if m:
            return m.group(1)
    raise ProbeError(f"MAX_HARTS: cannot resolve `{expr}`")


def probe_max_harts() -> tuple[dict[str, str], str]:
    """Per-arch MAX_HARTS (the build without optional features) and the fallback."""
    src = text(SMP)
    per_arch: dict[str, str] = {}
    default = None
    for m in re.finditer(
        r"#\[cfg\(((?:[^\[\]]|\[[^\]]*\])*?)\)\]\s*pub\s+const\s+MAX_HARTS\s*:\s*usize\s*=\s*([^;]+);",
        src,
        re.S,
    ):
        cfg, value = m.group(1), resolve_const(m.group(2))
        if cfg.lstrip().startswith("not("):
            default = value
            continue
        for arch in re.findall(r'target_arch\s*=\s*"(\w+)"', cfg):
            per_arch[arch] = value
    if default is None or not per_arch:
        raise ProbeError(f"{SMP}: MAX_HARTS cfg blocks not recognised")
    return per_arch, default


def probe_toolchain() -> list[str]:
    data = tomllib.loads(text(TOOLCHAIN))
    targets = data.get("toolchain", {}).get("targets")
    if not isinstance(targets, list):
        raise ProbeError(f"{TOOLCHAIN}: no [toolchain] targets list")
    return targets


def probe_default_arch() -> str:
    target = tomllib.loads(text(CARGO_CONFIG)).get("build", {}).get("target")
    if not target:
        raise ProbeError(f"{CARGO_CONFIG}: no [build] target")
    return target, target.split("-")[0].replace("riscv64gc", "riscv64")


def probe_workflows(pattern: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    rx = re.compile(pattern)
    wf_dir = ROOT / WORKFLOWS
    if not wf_dir.is_dir():
        raise ProbeError(f"{WORKFLOWS}: missing")
    for wf in sorted(wf_dir.glob("*.yml")):
        n = sum(1 for line in wf.read_text(encoding="utf-8").splitlines() if rx.search(line))
        if n:
            counts[wf.name] = n
    return counts


def probe_lanes() -> list[dict]:
    rows = tomllib.loads(text(LANES))
    out = rows.get("lane", []) + rows.get("test", [])
    if not out:
        raise ProbeError(f"{LANES}: no [[lane]] or [[test]] rows")
    return out


def probe_boards() -> dict[str, list[str]]:
    by_arch: dict[str, list[str]] = {}
    for path in sorted((ROOT / BOARDS).rglob("board.rs")):
        src = path.read_text(encoding="utf-8")
        slug = re.search(r'\bslug:\s*"([^"]+)"', src)
        arch = re.search(r"\barchitecture:\s*Architecture::(\w+)", src)
        if slug and arch:
            by_arch.setdefault(arch.group(1), []).append(slug.group(1))
    if not by_arch:
        raise ProbeError(f"{BOARDS}: no board.rs with slug and architecture")
    return by_arch


def descriptorless_board_dirs() -> list[str]:
    """Board directories with no board.rs (the 32-bit QEMU machines today)."""
    out = []
    for vendor in sorted((ROOT / BOARDS).iterdir()):
        if not vendor.is_dir() or vendor.name == "src":
            continue
        for board in sorted(vendor.iterdir()):
            if board.is_dir() and not (board / "board.rs").exists():
                out.append(f"{vendor.name}/{board.name}")
    return out


def code(s: str) -> str:
    return f"`{s}`"


def render() -> str:
    mods = probe_arch_modules()
    wide = probe_wide()
    grant_feature, grant_extra = probe_domain_grants()
    admission = probe_admission()
    harts, harts_default = probe_max_harts()
    toolchain = probe_toolchain()
    default_target, default_arch = probe_default_arch()
    lanes = probe_lanes()
    boards = probe_boards()
    bare_boards = descriptorless_board_dirs()

    lines = [
        "<!-- Generated by scripts/generate-arch-status.py. Do not edit; run the script. -->",
        "# Per-architecture implementation status",
        "",
        "Each cell is read from the tree by a probe named in [Sources](#sources); the specs",
        "reference this table instead of restating per-architecture status. `--check` fails",
        "when this file no longer matches the tree.",
        "",
        "| Architecture | Kernel arch module | 64-bit set (`cellos_wide`) | Tier-2 admission "
        "(`switch_ordering_qualified`) | Private-root domain grants (`cellos_domain_grants`) "
        "| Tier-3 hypervisor module | `MAX_HARTS` | Toolchain target | Workflow lines naming "
        "the target | Lane rows pr / nightly / manual | Board descriptors |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
    ]

    for name, arches, mod_dir, wf_pattern, board_arch in ROWS:
        arch_dir = ROOT / "kernel/src/arch" / mod_dir
        files = sum(1 for _ in arch_dir.rglob("*.rs"))
        module = f"{code(f'kernel/src/arch/{mod_dir}/')} ({files} files) under {code(mods[mod_dir])}"

        is_wide = any(a in wide for a in arches)
        wide_cell = "yes" if is_wide else "no"

        if not is_wide:
            adm = "no (not in the 64-bit set)"
            grants = "no (not in the 64-bit set)"
        else:
            arch = arches[0]
            if arch not in admission:
                adm = "no"
            elif admission[arch] is None:
                adm = "yes"
            else:
                adm = f"only with {code(admission[arch])}"
            extra = grant_extra.get(arch)
            grants = f"with {code(grant_feature)}" + (f" and {code(extra)}" if extra else "")

        hv = "yes" if (arch_dir / "hypervisor").is_dir() else "no"

        if is_wide:
            hart_cell = harts.get(arches[0], harts_default)
        else:
            hart_cell = harts_default

        wf_rx = re.compile(wf_pattern)
        targets = [t for t in toolchain if wf_rx.search(t)]
        tc = ", ".join(code(t) for t in targets) if targets else "none"

        wf = probe_workflows(wf_pattern)
        wf_cell = ", ".join(f"{k} {v}" for k, v in wf.items()) if wf else "0"

        counts = {t: 0 for t in TIERS}
        defaults = 0
        for row in lanes:
            arch = row.get("arch")
            if arch == "default" and arches[0] == default_arch:
                defaults += 1
                counts[row.get("tier", "manual")] += 1
            elif arch in arches:
                counts[row.get("tier", "manual")] += 1
        lane_cell = f"{counts['pr']} / {counts['nightly']} / {counts['manual']}"
        if defaults:
            lane_cell += f" (incl. {defaults} `default`)"
        if counts["retired"]:
            lane_cell += f"; {counts['retired']} retired"

        if board_arch:
            slugs = boards.get(board_arch, [])
            board_cell = ", ".join(code(s) for s in slugs) if slugs else "none"
        else:
            legacy = [b for b in bare_boards if re.search(r"32", b)]
            board_cell = (
                "none; directories without a descriptor: " + ", ".join(code(b) for b in legacy)
                if legacy
                else "none"
            )

        lines.append(
            f"| {name} | {module} | {wide_cell} | {adm} | {grants} | {hv} | {hart_cell} "
            f"| {tc} | {wf_cell} | {lane_cell} | {board_cell} |"
        )

    lines += [
        "",
        "Reading the table:",
        "",
        "- **Tier-2 admission** is the build-time qualification of the private-root switch. "
        "A boot still enables admission only in a development profile; fleet profiles "
        "(`policy-required`, `production-relay-image`) keep it closed unless the x86 browser "
        "profile is selected (`kernel/src/main.rs`, \"Tier 2 admission\" log line).",
        "- **Lane rows** count `scripts/lanes.toml` rows; `default` rows run on the workspace "
        f"default target, {code(default_target)} ({CARGO_CONFIG}). A `manual` row is not a "
        "verdict on whether the lane passes.",
        "- **`MAX_HARTS`** is the value without optional features (`g1-cpu-accounting` "
        "lowers x86_64 to 2).",
        "",
        "## Sources",
        "",
        "| Column | Probe |",
        "|---|---|",
        f"| Kernel arch module | {code(ARCH_MOD)}: cfg on each `mod <arch>;` |",
        f"| 64-bit set | {code(POSTURE + '::wide')} |",
        f"| Tier-2 admission | {code(ADMISSION + '::switch_ordering_qualified')} |",
        f"| Domain grants | {code(POSTURE + '::domain_grants')} |",
        "| Tier-3 hypervisor module | `kernel/src/arch/<arch>/hypervisor/` exists |",
        f"| `MAX_HARTS` | {code(SMP + '::MAX_HARTS')} |",
        f"| Toolchain target | {code(TOOLCHAIN)} `targets` |",
        f"| Workflow lines | {code(WORKFLOWS + '/*.yml')} lines naming the kernel target |",
        f"| Lane rows | {code(LANES)} `arch` and `tier` |",
        f"| Board descriptors | {code(BOARDS + '/**/board.rs')} `slug` and `architecture` |",
        "",
    ]
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true", help="exit 1 when the committed table is stale")
    args = parser.parse_args()
    try:
        content = render()
    except ProbeError as err:
        print(f"generate-arch-status: probe failed: {err}", file=sys.stderr)
        return 2
    rel = OUT.relative_to(ROOT)
    if args.check:
        current = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if current != content:
            print(f"{rel} is stale; run python3 scripts/generate-arch-status.py", file=sys.stderr)
            return 1
        print(f"{rel} is current")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(content, encoding="utf-8")
    print(f"wrote {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
