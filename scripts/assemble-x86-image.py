#!/usr/bin/env python3
"""Assemble the x86_64 embedded VIFS1 image (`kernel_fs.img`) and `init` from source.

The single source of the x86 image recipe (Phase 10 S10-1, D11-1). Callers:
`scripts/build-x86_64-cells.ps1` (headless), the CI x86 job (headless) and
`scripts/package-desktop-x86.py` (desktop). Usage:

    assemble-x86-image.py --profile headless|desktop --out-dir DIR
                          [--seed-file PATH] [--objcopy BIN]

Steps, stopping at the first failure:

 1. resolve the cargo target directory (`CARGO_TARGET_DIR`, default `target`); a
    dev-key run refuses an out-dir or target directory inside the repository,
    except the canonical route (S10-2b): out-dir `kernel/src/embedded-x86_64` with
    the cargo target directory `<repo>/target` or one outside the repository;
 2. delete every manifest binary and `app-init` in the release directory and
    verify they are gone, so a failed build cannot leave stale bytes to package;
 3. build the profile packages, then `app-init` with the profile features; a
    non-zero exit, a missing artifact, an artifact this cargo run did not report
    or one placed before the build started aborts;
 4. copy the inputs into the work directory, then sign the copies of every
    signed cell and `app-init` in one batch: the dev key through the named
    route `x86-candidate-<profile>` (S10-2a; out-dir outside the repository) or
    `--seed-file` through the `cellos-sign` CLI; the signing guards decide, and
    the cargo target binaries stay unsigned;
 5. sign the operator policy (`sign-policy.py`, dev fleet key) as `/POLICY.BIN`;
 6. check the inputs before any image byte exists (quarantined hashes, alias
    source, exact destination set);
 7. write the image with `tools/mkfat32.py` and template-generated boot config
    into a work directory inside `--out-dir`;
 8. read the image back and check it against the manifest;
 9. publish `kernel_fs.img`, `init`, `boot-config/` and `manifest.json` with
    `os.replace`, the manifest last.

Any failure exits non-zero and leaves `--out-dir` as it was. The assembler never
sets `CELLBOX_PRIVATE_FIXTURE_SIGNING` or `CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI`;
toolchain variables (`CC_*`, `CFLAGS_*`, `BINDGEN_*`) are the caller's.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TARGET = "x86_64-unknown-none"
CELL_RUSTFLAGS = "-C relocation-model=pic"
# The canonical kernel profile (Phase 10 S10-3a, decision b): the fleet profile, i.e. the
# default features plus `policy-required`, so Tier 2 admission is DISABLED. The kernel is
# built outside this script (CI x86 kernel and PC steps, the S10-3 candidate builder, S10-5)
# with `--features <these>`; the manifest records them so every builder reads one source.
CANONICAL_KERNEL_FEATURES = ("policy-required",)

sys.path.insert(0, str(REPO / "scripts"))
from cellos_sign import cli as signing_cli, signing  # noqa: E402  (the blocked set is imported, never copied)

CONFIG_PATHS = frozenset(
    "/etc/cellos/" + name + ".toml" for name in ("system", "services", "autoload")
)
POLICY_PATH = "/POLICY.BIN"
CELLBOX_PATH = "/bin/cellbox"
ALIAS_PATH = "/bin/cellbox-ssh"
FORBIDDEN_PATHS = frozenset({"/bin/shell", "/bin/sshd"})
INIT_PACKAGE = "app-init"
INIT_BINARY = "app-init"


@dataclass(frozen=True)
class Cell:
    package: str
    binary: str
    paths: tuple[str, ...]
    signed: bool = True


@dataclass(frozen=True)
class Profile:
    name: str
    cells: tuple[Cell, ...]
    init_features: tuple[str, ...]

    def packages(self) -> list[str]:
        """Cargo `-p` list in manifest order, each package once."""
        return list(dict.fromkeys(cell.package for cell in self.cells))

    def image_paths(self) -> set[str]:
        """Every image path the profile packages, without the generated config."""
        return {path for cell in self.cells for path in cell.paths} | {POLICY_PATH}


# D10-6: the retained workers in canonical images. app-sys-tools builds 11 binaries.
WORKERS = ("ls", "cat", "echo", "ps", "kill", "shutdown")

HEADLESS_CELLS = (
    Cell("app-cellbox", "cellbox", (CELLBOX_PATH, ALIAS_PATH)),  # D10-5: one file, two entries
    Cell("service-vfs", "service-vfs", ("/bin/vfs",)),
    Cell("service-config", "service-config", ("/bin/config",)),
    Cell("service-input", "service-input", ("/bin/input",)),
    Cell("service-net", "service-net", ("/bin/net",)),
    Cell("service-platform", "platform", ("/bin/platform",)),
    Cell("driver-nvme", "driver-nvme", ("/bin/nvme",)),
    Cell("driver-e1000", "driver-e1000", ("/bin/e1000",)),
    Cell("driver-igb", "driver-igb", ("/bin/igb",)),
    Cell("driver-ahci", "driver-ahci", ("/bin/ahci",)),
    Cell("driver-xhci", "driver-xhci", ("/bin/xhci",)),
    Cell("driver-serial-x86", "driver-serial-x86", ("/bin/serial",)),
    *(Cell("app-sys-tools", worker, ("/bin/" + worker,)) for worker in WORKERS),
    # Tier-2 fixtures stay unsigned: they exercise the untrusted-cell wall.
    Cell("tier2-smoke", "tier2-smoke", ("/bin/tier2-smoke",), signed=False),
    Cell("tier2-exploit", "tier2-exploit", ("/bin/tier2-exploit",), signed=False),
)

DESKTOP_CELLS = HEADLESS_CELLS + (
    Cell("service-compositor", "service-compositor", ("/bin/compositor",)),
    Cell("fb-console", "fb-console", ("/bin/fb-console",)),
    Cell("desktop", "desktop", ("/bin/desktop",)),
    Cell("files", "files", ("/bin/files",)),
    Cell("terminal", "terminal", ("/bin/terminal",)),
    Cell("app-chrome", "chrome", ("/bin/chrome",)),
    Cell("ocel", "ocel", ("/bin/ocel",)),
)

PROFILES = {
    "headless": Profile("headless", HEADLESS_CELLS, ("input",)),
    "desktop": Profile("desktop", DESKTOP_CELLS, ("input", "ui")),
}


class AssemblyError(RuntimeError):
    """A step failed; nothing was published."""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def read_seed_file(path: Path) -> str:
    """The private seed as 64 lowercase hex digits; never the dev seed."""
    seed = path.read_text().strip().lower()
    if len(seed) != 64 or any(c not in "0123456789abcdef" for c in seed):
        raise AssemblyError(f"--seed-file {path}: expected 64 hex digits (a 32-byte seed)")
    if seed == signing.DEV_SEED_HEX:
        raise AssemblyError("--seed-file holds the dev seed; omit --seed-file for the dev key")
    return seed


def parse_toml_rows(raw: bytes, source: str) -> list[dict]:
    try:
        return tomllib.loads(raw.decode("utf-8")).get("cells", [])
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as exc:
        raise AssemblyError(f"image {source} is not valid TOML: {exc}") from exc


def check_inputs(pairs: list[tuple[Path, str]], init: Path, profile: Profile,
                 blocked: frozenset[str]) -> None:
    """Step 6: refuse before any image byte exists."""
    for source in [src for src, _ in pairs] + [init]:
        if sha256_file(source) in blocked:
            raise AssemblyError(f"quarantined bytes refused as image input: {source}")
    destinations = [dst for _, dst in pairs]
    duplicates = sorted({dst for dst in destinations if destinations.count(dst) > 1})
    if duplicates:
        raise AssemblyError(f"duplicate image destinations: {duplicates}")
    forbidden = sorted(FORBIDDEN_PATHS & set(destinations))
    if forbidden:
        raise AssemblyError(f"retired or forbidden image paths refused: {forbidden}")
    sources = dict((dst, src) for src, dst in pairs)
    if CELLBOX_PATH not in sources or ALIAS_PATH not in sources:
        raise AssemblyError(f"{CELLBOX_PATH} and {ALIAS_PATH} are both required")
    if sources[CELLBOX_PATH] != sources[ALIAS_PATH]:
        raise AssemblyError(f"{ALIAS_PATH} must come from the same host file as {CELLBOX_PATH}")
    expected = profile.image_paths()
    if set(destinations) != expected:
        raise AssemblyError(
            "image destinations differ from the profile manifest: "
            f"extra {sorted(set(destinations) - expected)}, "
            f"missing {sorted(expected - set(destinations))}"
        )


def check_image(files: dict[str, bytes], expected: dict[str, bytes], profile: Profile,
                blocked: frozenset[str]) -> None:
    """Step 8: the read-back image equals the manifest and boots `cellbox sh` only.

    `files` is the image as read back; `expected` maps every manifest path and the
    three generated config files to the bytes that were handed to the writer.
    """
    wanted = profile.image_paths() | CONFIG_PATHS
    if set(expected) != wanted:
        raise AssemblyError(f"internal: expected bytes do not cover the manifest: "
                            f"{sorted(set(expected) ^ wanted)}")
    present = set(files)
    forbidden = sorted(FORBIDDEN_PATHS & present)
    if forbidden:
        raise AssemblyError(f"retired or forbidden paths in the image: {forbidden}")
    for path in sorted(files):
        if sha256_bytes(files[path]) in blocked:
            raise AssemblyError(f"quarantined bytes in the image: {path}")
    if present != wanted:
        raise AssemblyError(
            "image path set differs from the manifest: "
            f"extra {sorted(present - wanted)}, missing {sorted(wanted - present)}"
        )
    for path in sorted(wanted):
        if files[path] != expected[path]:
            raise AssemblyError(f"image file differs from its input: {path}")
    if files[CELLBOX_PATH] != files[ALIAS_PATH]:
        raise AssemblyError(f"{ALIAS_PATH} is not byte-equal to {CELLBOX_PATH}")

    autoload = parse_toml_rows(files["/etc/cellos/autoload.toml"], "autoload.toml")
    services = parse_toml_rows(files["/etc/cellos/services.toml"], "services.toml")
    shells = [row for row in autoload if row.get("path") == CELLBOX_PATH]
    if len(shells) != 1 or shells[0].get("args") != ["sh"]:
        raise AssemblyError(
            f"autoload must start exactly one {CELLBOX_PATH} with args [\"sh\"]: {shells}")
    for row in autoload:
        if row.get("path") in FORBIDDEN_PATHS | {ALIAS_PATH}:
            raise AssemblyError(f"autoload starts a refused path: {row.get('path')}")
    # The inbound sshd (CBX-25) first, then /bin/config, which stays `ai`-only (D10-4 A).
    for refused in (("sshd", "/bin/sshd"), ("shell", "/bin/shell"), ("config", "/bin/config")):
        for row in services:
            if row.get("name") == refused[0] or row.get("path") == refused[1]:
                raise AssemblyError(f"services.toml starts a refused service: {row.get('name')}")
    if "ui" in profile.init_features:
        service_names = {row.get("name") for row in services}
        for name in ("compositor", "input"):
            if name not in service_names:
                raise AssemblyError(f"desktop services.toml does not start {name}")
        if not any(row.get("name") == "desktop" for row in autoload):
            raise AssemblyError("desktop autoload.toml does not start desktop")


class Assembler:
    """One image assembly. Tests subclass the build/sign/policy/provenance hooks."""

    def __init__(self, profile: Profile, out_dir: Path, *, seed_hex: str | None = None,
                 objcopy: str = "objcopy", repo: Path = REPO,
                 extra_pairs: tuple[tuple[Path, str], ...] = (),
                 extra_blocked: frozenset[str] = frozenset()):
        self.profile = profile
        self.out_dir = Path(out_dir)
        self.seed_hex = seed_hex
        self.objcopy = objcopy
        self.repo = repo
        self.extra_pairs = extra_pairs
        self.blocked = frozenset(signing.BLOCKED_ARTIFACT_SHA256) | extra_blocked
        target_dir = Path(os.environ.get("CARGO_TARGET_DIR") or "target")
        if not target_dir.is_absolute():
            target_dir = repo / target_dir
        self.rel = target_dir / TARGET / "release"

    # ---- hooks -------------------------------------------------------------------

    def cargo_build(self, args: list[str]) -> set[Path]:
        """Run one cargo build; return the executables this run reported."""
        env = dict(os.environ)
        env["RUSTFLAGS"] = CELL_RUSTFLAGS
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)  # it would take precedence over RUSTFLAGS
        command = ["cargo", "build", "--release", "--locked", "--target", TARGET,
                   "-Z", "build-std=core,alloc", "--message-format=json-render-diagnostics",
                   *args]
        print("+ " + " ".join(command), flush=True)
        process = subprocess.Popen(command, cwd=self.repo, env=env, stdout=subprocess.PIPE,
                                   text=True)
        executables = set()
        for line in process.stdout:
            if not line.startswith("{"):
                continue
            message = json.loads(line)
            if message.get("reason") == "compiler-artifact" and message.get("executable"):
                executables.add(Path(message["executable"]).resolve())
        if process.wait() != 0:
            raise AssemblyError(f"cargo build failed (exit {process.returncode}): {' '.join(args)}")
        return executables

    def is_canonical_out_dir(self) -> bool:
        """True when this run publishes into the repository's canonical directory."""
        return self.out_dir.resolve() == (self.repo / signing.X86_CANONICAL_ROOT).resolve()

    def route_name(self) -> str:
        """The dev-key route for this run: canonical (S10-2b) or candidate (S10-2a)."""
        kind = "canonical" if self.is_canonical_out_dir() else "candidate"
        return f"x86-{kind}-{self.profile.name}"

    def sign(self, targets: list[Path], candidate_root: Path) -> None:
        """Sign the input copies in place. A private seed takes the `cellos-sign` CLI
        (private-fixture path); the dev key takes the named candidate route, which
        only this program, at its pinned digest, can pass."""
        if self.seed_hex is not None:
            command = [sys.executable, str(self.repo / "scripts" / "cellos-sign"), "--quiet",
                       "--objcopy", self.objcopy, "--seed-hex", self.seed_hex,
                       "--sign", *[str(target) for target in targets]]
            print(f"+ cellos-sign --objcopy {self.objcopy} (private key) "
                  f"--sign {len(targets)} cell(s)", flush=True)
            code = subprocess.run(command, cwd=self.repo).returncode
        else:
            print(f"+ cellos-sign route {self.route_name()} (dev key) "
                  f"--sign {len(targets)} cell(s)", flush=True)
            code = signing_cli.sign_route(self.repo, self.route_name(), candidate_root,
                                          targets, self.objcopy)
        if code != 0:
            raise AssemblyError("cellos-sign refused or failed; nothing was packaged")

    def sign_policy(self, out: Path) -> None:
        command = [sys.executable, str(self.repo / "scripts" / "sign-policy.py"), "--out", str(out)]
        if subprocess.run(command, cwd=self.repo).returncode != 0 or not out.is_file():
            raise AssemblyError("sign-policy.py failed (needs `pip install cryptography`)")

    def write_image(self, image: Path, config_dir: Path, pairs: list[tuple[Path, str]]) -> None:
        env = dict(os.environ)
        # Template-generated boot config (D11-5): no operator override directory.
        env.pop("CELLOS_CONFIG_DIR", None)
        env.pop("CELLOS_INIT_FEATURES", None)
        command = [sys.executable, str(self.repo / "tools" / "mkfat32.py"),
                   "--config-features", ",".join(self.profile.init_features),
                   "--config-output-dir", str(config_dir), str(image)]
        for source, destination in pairs:
            command += [str(source), destination]
        if subprocess.run(command, cwd=self.repo, env=env).returncode != 0 or not image.is_file():
            raise AssemblyError("mkfat32.py failed")

    def placed_ns(self, path: Path) -> int | None:
        """When the directory entry was placed: POSIX st_ctime, which also moves when
        cargo re-links an unchanged artifact by hard link (its mtime stays old).
        Windows exposes no change time (None): there the verified invalidation and
        cargo's own artifact report carry freshness."""
        return path.stat().st_ctime_ns if os.name == "posix" else None

    def provenance(self) -> dict:
        def run(*command: str) -> str:
            return subprocess.run(command, cwd=self.repo, capture_output=True, text=True,
                                  check=True).stdout.strip()

        status = subprocess.run(
            ["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"],
            cwd=self.repo, capture_output=True, check=True).stdout.split(b"\0")
        digest, count, index = hashlib.sha256(), 0, 0
        while index < len(status):
            entry = status[index]
            index += 1
            if not entry:
                continue
            code, path = entry[:2], entry[3:]
            if code[:1] in (b"R", b"C"):
                index += 1  # the original path follows a rename/copy entry
            file = self.repo / os.fsdecode(path)
            content = sha256_file(file).encode() if file.is_file() else b"-"
            digest.update(code + b"\0" + path + b"\0" + content + b"\n")
            count += 1
        toolchain_env = {name: os.environ.get(name) for name in (
            "CC_x86_64_unknown_none", "CFLAGS_x86_64_unknown_none",
            "BINDGEN_EXTRA_CLANG_ARGS_x86_64_unknown_none")}
        return {
            "git_head": run("git", "rev-parse", "HEAD"),
            "dirty_files": count,
            "dirty_digest_sha256": digest.hexdigest(),
            "cargo": run("cargo", "-V"),
            "rustc": run("rustc", "-V"),
            "rustflags": CELL_RUSTFLAGS,
            "toolchain_env": toolchain_env,
        }

    # ---- steps -------------------------------------------------------------------

    def binary(self, name: str) -> Path:
        return self.rel / name

    def invalidate(self) -> None:
        stale = [self.binary(cell.binary) for cell in self.profile.cells]
        stale.append(self.binary(INIT_BINARY))
        for path in dict.fromkeys(stale):
            try:
                path.unlink()
            except FileNotFoundError:
                pass
            if path.exists() or path.is_symlink():
                raise AssemblyError(f"failed to invalidate a stale input: {path}")

    def build(self, marker: Path) -> None:
        marker.write_bytes(b"")
        started = self.placed_ns(marker)
        cells = self.cargo_build([arg for package in self.profile.packages()
                                  for arg in ("-p", package)])
        init = self.cargo_build(["-p", INIT_PACKAGE, "--features",
                                 ",".join(self.profile.init_features)])
        reported = cells | init
        for name in dict.fromkeys([cell.binary for cell in self.profile.cells] + [INIT_BINARY]):
            artifact = self.binary(name)
            if not artifact.is_file():
                raise AssemblyError(f"build produced no artifact: {artifact}")
            if artifact.resolve() not in reported:
                raise AssemblyError(f"artifact not reported by this cargo run: {artifact}")
            placed = self.placed_ns(artifact)
            if started is not None and placed is not None and placed < started:
                raise AssemblyError(f"artifact older than the build start: {artifact}")

    def check_isolation(self) -> None:
        """Dev-key runs write outside the repository, except the canonical route (S10-2b):
        out-dir `kernel/src/embedded-x86_64` with the cargo target directory `<repo>/target`
        or outside the repository. Refused before invalidation or build, symlinks resolved."""
        if self.seed_hex is not None:
            return
        repo = self.repo.resolve()
        canonical = self.is_canonical_out_dir()
        target_dir = self.rel.parent.parent
        for what, path, admitted in (
                ("--out-dir", self.out_dir, canonical),
                ("CARGO_TARGET_DIR", target_dir, canonical and target_dir.resolve() == (repo / "target").resolve())):
            if path.resolve().is_relative_to(repo) and not admitted:
                raise AssemblyError(
                    f"dev-key run refused: {what} {path} is inside the repository; "
                    f"only the canonical route ({signing.X86_CANONICAL_ROOT} with the cargo "
                    f"target directory <repo>/target) is admitted")

    def assemble(self) -> dict:
        built_at = _dt.datetime.now(_dt.timezone.utc)
        self.check_isolation()
        created_out_dir = False
        if not self.out_dir.is_dir():
            self.out_dir.mkdir()
            created_out_dir = True
        try:
            with tempfile.TemporaryDirectory(prefix=".assemble-", dir=self.out_dir) as tmp:
                return self._assemble(Path(tmp), built_at)
        except BaseException:
            if created_out_dir:
                shutil.rmtree(self.out_dir, ignore_errors=True)
            raise

    def _assemble(self, work: Path, built_at: _dt.datetime) -> dict:
        print(f"=== x86_64 image: profile {self.profile.name}, release dir {self.rel} ===",
              flush=True)
        self.invalidate()
        self.build(work / "build-start")

        # Copy, then sign (D2a-3): the cargo target binaries stay unsigned and every
        # signed byte lives in the work directory. What is signed, hashed, checked and
        # packaged is one set of bytes.
        inputs = work / "inputs"
        inputs.mkdir()
        snapshots: dict[str, Path] = {}
        for name in dict.fromkeys(cell.binary for cell in self.profile.cells):
            snapshots[name] = inputs / name
            shutil.copyfile(self.binary(name), snapshots[name])
        init = inputs / INIT_BINARY
        shutil.copyfile(self.binary(INIT_BINARY), init)
        signed = [snapshots[name] for name in dict.fromkeys(
            cell.binary for cell in self.profile.cells if cell.signed)]
        self.sign(signed + [init], work)

        policy = work / "POLICY.BIN"
        self.sign_policy(policy)

        pairs = [(snapshots[cell.binary], path) for cell in self.profile.cells for path in cell.paths]
        pairs.append((policy, POLICY_PATH))
        pairs.extend(self.extra_pairs)

        check_inputs(pairs, init, self.profile, self.blocked)

        image = work / "kernel_fs.img"
        config_dir = work / "boot-config"
        self.write_image(image, config_dir, pairs)

        reader = load_module("cellos_boot_config_reader", self.repo / "scripts" / "generate-boot-config.py")
        files = reader.image_files(image)
        expected = {destination: source.read_bytes() for source, destination in pairs}
        for path in CONFIG_PATHS:
            expected[path] = (config_dir / Path(path).name).read_bytes()
        check_image(files, expected, self.profile, self.blocked)

        packages = {path: {"package": cell.package, "binary": cell.binary, "signed": cell.signed}
                    for cell in self.profile.cells for path in cell.paths}
        manifest = {
            "schema": "cellos-x86-image-manifest/1",
            "profile": self.profile.name,
            "init_features": list(self.profile.init_features),
            "kernel_features": list(CANONICAL_KERNEL_FEATURES),
            "key_class": "private" if self.seed_hex else "dev",
            "signing_route": None if self.seed_hex else self.route_name(),
            "objcopy": self.objcopy,
            "files": {path: {"sha256": sha256_bytes(files[path]), "size": len(files[path]),
                             **packages.get(path, {})}
                      for path in sorted(files)},
            "kernel_fs_img": {"sha256": sha256_file(image), "size": image.stat().st_size},
            "init": {"sha256": sha256_file(init), "size": init.stat().st_size,
                     "package": INIT_PACKAGE},
            "source": self.provenance(),
            "build_started_utc": built_at.isoformat(timespec="seconds"),
            "build_finished_utc": _dt.datetime.now(_dt.timezone.utc).isoformat(timespec="seconds"),
        }
        (work / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")

        # Publish: image and init first, the manifest last (S10-3/S10-5 check the
        # image hash against it before any kernel build).
        os.replace(image, self.out_dir / "kernel_fs.img")
        os.replace(init, self.out_dir / "init")
        published_config = self.out_dir / "boot-config"
        if published_config.exists():
            os.replace(published_config, work / "previous-boot-config")
        os.replace(config_dir, published_config)
        os.replace(work / "manifest.json", self.out_dir / "manifest.json")
        print(f"published {self.out_dir}/kernel_fs.img ({manifest['kernel_fs_img']['size']} B, "
              f"sha256 {manifest['kernel_fs_img']['sha256']}), {len(files)} files", flush=True)
        return manifest


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--profile", required=True, choices=sorted(PROFILES))
    parser.add_argument("--out-dir", required=True, type=Path,
                        help="where kernel_fs.img, init, boot-config/ and manifest.json go")
    parser.add_argument("--seed-file", type=Path,
                        help="32-byte hex private signing seed (default: the dev key)")
    parser.add_argument("--objcopy", default=os.environ.get("OBJCOPY", "objcopy"),
                        help="objcopy for cellos-sign ($OBJCOPY, default objcopy)")
    parser.add_argument("--test-inject", action="append", default=[], metavar="HOST=/DEST",
                        help="test hook, refusal only: add an extra input pair outside the "
                             "manifest (always refused before the image is written)")
    parser.add_argument("--test-block-sha256", action="append", default=[], metavar="HEX",
                        help="test hook, refusal only: add a SHA-256 to the quarantined set")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    profile = PROFILES[args.profile]
    extra_pairs = []
    for item in args.test_inject:
        source, separator, destination = item.partition("=")
        if not separator or not destination.startswith("/"):
            raise SystemExit(f"--test-inject must be HOST=/DEST: {item}")
        if destination in profile.image_paths() | CONFIG_PATHS:
            raise SystemExit(f"--test-inject may only add paths outside the manifest: {destination}")
        extra_pairs.append((Path(source).resolve(), destination))
    extra_blocked = frozenset(value.lower() for value in args.test_block_sha256)
    try:
        seed_hex = read_seed_file(args.seed_file) if args.seed_file else None
        Assembler(profile, args.out_dir.resolve(), seed_hex=seed_hex, objcopy=args.objcopy,
                  extra_pairs=tuple(extra_pairs), extra_blocked=extra_blocked).assemble()
    except (AssemblyError, signing.SigningRefused) as exc:
        print(f"FAIL: {exc}", file=sys.stderr)
        print("nothing was published; the out-dir is unchanged", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
