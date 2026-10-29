"""Signing side of `cellos-sign`: the prod-key gate and the sign/verify calls.

The Ed25519 payload rules, the `__ViCell_sig` section layout and the objcopy
invocation all stay in `scripts/sign-cell.py` and are called from here, not
reimplemented: the kernel verifier (`kernel/src/signing.rs`) is byte-compatible
with exactly one producer, and a second copy of those rules is a second thing
that can drift out of agreement with it.
"""

from __future__ import annotations

import importlib.util
import hashlib
import os
import stat
import sys
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType

# Environment markers that mean "this is a hosted CI runner". The production key
# is only ever released to one of these.
CI_MARKERS = ("GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE", "CIRCLECI", "CELLOS_SIGN_CI")

# Escape hatch for a self-hosted KMS signer that sets none of the above. Named
# so that it cannot be enabled by accident and shows up in a grep of any script.
PROD_OVERRIDE_ENV = "CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI"

# Phase 04 S1/S3-gui: the serial shell ships as `cellbox sh` and the GUI Terminal
# hosts it, so a private fixture has to be able to sign ELFs named `cellbox` and
# `terminal`. The allowance is deliberately narrow: exactly this variable set to
# "1", only for the names below, and only with an explicit private seed that is NOT
# the reproducible dev seed the canonical builds sign with. It never admits the
# quarantined bytes (see BLOCKED_ARTIFACT_SHA256). It is independent of the rollout
# guards of `package-desktop-x86.py` and `make-iso-ci.sh` (reopened by S10-2b on their own rules).
CELLBOX_FIXTURE_ENV = "CELLBOX_PRIVATE_FIXTURE_SIGNING"
PRIVATE_FIXTURE_NAMES = frozenset({"cellbox", "app-cellbox", "terminal"})
BLOCKED_ARTIFACT_SHA256 = frozenset({
    "64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39",
    "636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2",
})
# `scripts/sign-cell.py` DEV_SEED (`[0x43] * 32`); kept literal so the preflight
# needs no signer import, and re-checked against the signer's own constant below.
DEV_SEED_HEX = "43" * 32

# The repository this reviewed module belongs to. Route recipes are resolved here,
# never against a caller-supplied root.
REPO_ROOT = Path(__file__).resolve().parent.parent.parent


@dataclass(frozen=True)
class QualifiedRoute:
    """A reviewed route (Phase 10 S10-2a candidate routes, S10-2b canonical routes)."""

    recipe: str                     # repository-relative path of the only admitted program
    recipe_sha256: str              # pinned digest of that program; any edit needs a re-review
    fixture_names: frozenset[str]   # the PRIVATE_FIXTURE_NAMES this route may sign
    binaries: frozenset[str]        # every basename a batch on this route may contain
    in_repo_root: str | None = None  # None: the root must be outside the repository (candidate
                                     # routes); else the root must be exactly this repo-relative path


# The x86 routes: the DEV cell key may sign `cellbox`/`terminal` only here, only from the
# pinned assembler running as the program itself, never the quarantined hashes. Candidate
# routes sign into a root outside the repository (S10-2a); canonical routes (S10-2b) sign
# only into exactly `X86_CANONICAL_ROOT`. Adding a route or a name, or editing the
# assembler, is a reviewed source change of this table.
X86_CANONICAL_ROOT = "kernel/src/embedded-x86_64"
X86_CANDIDATE_RECIPE = "scripts/assemble-x86-image.py"
X86_CANDIDATE_RECIPE_SHA256 = "b74cc99787ab69dde04dac5c517acb6f771016043c23841bc7b40ab0b3bec7ed"
_X86_HEADLESS_SIGNED = frozenset({
    "cellbox", "service-vfs", "service-config", "service-input", "service-net", "platform",
    "driver-nvme", "driver-e1000", "driver-igb", "driver-ahci", "driver-xhci",
    "driver-serial-x86", "ls", "cat", "echo", "ps", "kill", "shutdown", "app-init",
})
_X86_DESKTOP_SIGNED = _X86_HEADLESS_SIGNED | frozenset({
    "service-compositor", "fb-console", "desktop", "files", "terminal", "chrome", "ocel",
})
QUALIFIED_ROUTES = {
    "x86-candidate-headless": QualifiedRoute(
        X86_CANDIDATE_RECIPE, X86_CANDIDATE_RECIPE_SHA256, frozenset({"cellbox"}),
        _X86_HEADLESS_SIGNED),
    "x86-candidate-desktop": QualifiedRoute(
        X86_CANDIDATE_RECIPE, X86_CANDIDATE_RECIPE_SHA256, frozenset({"cellbox", "terminal"}),
        _X86_DESKTOP_SIGNED),
    "x86-canonical-headless": QualifiedRoute(
        X86_CANDIDATE_RECIPE, X86_CANDIDATE_RECIPE_SHA256, frozenset({"cellbox"}),
        _X86_HEADLESS_SIGNED, X86_CANONICAL_ROOT),
    "x86-canonical-desktop": QualifiedRoute(
        X86_CANDIDATE_RECIPE, X86_CANDIDATE_RECIPE_SHA256, frozenset({"cellbox", "terminal"}),
        _X86_DESKTOP_SIGNED, X86_CANONICAL_ROOT),
}


@dataclass(frozen=True)
class Route:
    """A signing request on a named route: the route name and its candidate root."""

    name: str
    candidate_root: Path


class SigningRefused(RuntimeError):
    """A signing precondition failed. The caller must not fall back to signing."""


def is_ci() -> bool:
    """True when a recognised CI marker is set to a non-empty, non-`false` value."""
    for name in CI_MARKERS:
        value = os.environ.get(name, "").strip().lower()
        if value and value not in ("0", "false", "no"):
            return True
    return False


def guard_prod_key(seed_hex: str | None) -> None:
    """Refuse a non-dev key outside CI.

    The production key must exist only in CI/KMS; this check is what makes a
    developer's habit of "just sign it locally" fail loudly instead of minting a
    production signature from a laptop. It is an *accident* guard, not a security
    boundary — environment variables are forgeable, and anyone holding the key
    material has already passed the boundary that matters (see package docstring).
    """
    if seed_hex is None:
        return  # dev key: reproducible, matches the `dev-signing-key` feature
    if is_ci() or os.environ.get(PROD_OVERRIDE_ENV):
        return
    raise SigningRefused(
        "refusing to sign with a non-dev key outside CI. The production key lives "
        f"in CI/KMS only (Spec 18 §2.1). Set one of {', '.join(CI_MARKERS)} on a "
        f"real runner, or {PROD_OVERRIDE_ENV}=1 on an audited KMS signer."
    )


def load_signer(repo: Path) -> ModuleType:
    """Import `scripts/sign-cell.py` as a module (its name is not an identifier)."""
    path = repo / "scripts" / "sign-cell.py"
    spec = importlib.util.spec_from_file_location("cellos_sign_cell", path)
    if spec is None or spec.loader is None:
        raise SigningRefused(f"cannot load the signer at {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def private_fixture_allowed(seed_hex: str | None) -> bool:
    """True only for an explicit private-fixture signing request.

    Needs `CELLBOX_PRIVATE_FIXTURE_SIGNING=1` *and* an explicit seed that is not
    the reproducible dev seed: the default (no `--seed-hex`) is the dev key every
    canonical build signs with, so a fixture can never ride on it.
    """
    if os.environ.get(CELLBOX_FIXTURE_ENV) != "1":
        return False
    return seed_hex is not None and seed_hex.strip().lower() != DEV_SEED_HEX


def _route_caller() -> Path | None:
    """The real path of the running program (`__main__`), or None (REPL, `-m`, embed)."""
    main = sys.modules.get("__main__")
    file = getattr(main, "__file__", None)
    return Path(os.path.realpath(file)) if file else None


def route_admits(route: Route, targets: list[Path], seed_hex: str | None) -> None:
    """Admit a dev-key batch on a named route, or raise `SigningRefused`.

    Every condition must hold: the route is in `QUALIFIED_ROUTES`; the running
    program is that route's recipe in this repository with the pinned SHA-256; the
    key is the dev key (no seed); every target name is one of the route's binaries
    and its fixture names are the route's; the root is an existing directory that
    is outside the repository (candidate routes) or exactly the route's
    `in_repo_root` (canonical routes); and every target is a regular, unlinked
    (`st_nlink == 1`) file inside that root. No environment variable is read.
    """
    def refuse(reason: str) -> None:
        raise SigningRefused(f"ROLLOUT BLOCKED: route {route.name!r}: {reason}")

    spec = QUALIFIED_ROUTES.get(route.name)
    if spec is None:
        refuse("not a qualified route")
    repo = Path(os.path.realpath(REPO_ROOT))
    recipe = Path(os.path.realpath(repo / spec.recipe))
    if _route_caller() != recipe:
        refuse(f"the running program is not the recipe {spec.recipe}")
    with recipe.open("rb") as program:
        if hashlib.file_digest(program, "sha256").hexdigest() != spec.recipe_sha256:
            refuse(f"{spec.recipe} differs from its reviewed SHA-256 pin")
    if seed_hex is not None:
        refuse("routes sign with the dev key only (no seed)")
    root = Path(os.path.realpath(route.candidate_root))
    if not root.is_dir():
        refuse(f"candidate root is not a directory: {route.candidate_root}")
    if spec.in_repo_root is None:
        if root.is_relative_to(repo):
            refuse(f"candidate root is inside the repository: {route.candidate_root}")
    elif root != Path(os.path.realpath(repo / spec.in_repo_root)):
        refuse(f"canonical root must be exactly {spec.in_repo_root}: {route.candidate_root}")
    for target in targets:
        real = Path(os.path.realpath(target))
        names = {target.name, real.name}
        if names & (PRIVATE_FIXTURE_NAMES - spec.fixture_names):
            refuse(f"fixture name not on this route: {target}")
        if not names <= spec.binaries:
            refuse(f"not a binary of this route: {target}")
        try:
            info = os.lstat(target)
        except FileNotFoundError:
            refuse(f"missing target: {target}")
        if not stat.S_ISREG(info.st_mode):
            refuse(f"not a regular file (symlink or special): {target}")
        if info.st_nlink != 1:
            refuse(f"hard-linked target ({info.st_nlink} links): {target}")
        if real == root or not real.is_relative_to(root):
            refuse(f"target outside the candidate root: {target}")


def _refuse_blocked_hashes(targets: list[Path]) -> None:
    for target in targets:
        if target.is_file():
            with target.open("rb") as artifact:
                digest = hashlib.file_digest(artifact, "sha256").hexdigest()
            if digest in BLOCKED_ARTIFACT_SHA256:
                raise SigningRefused(
                    f"ROLLOUT BLOCKED: quarantined Cellbox/terminal bytes: {target}"
                )


def preflight_blocked_artifacts(targets: list[Path], seed_hex: str | None, *,
                                route: Route | None = None) -> None:
    """Refuse the whole batch before any key is loaded or target is written.

    First, unconditionally, the quarantined byte hashes, whatever the name, env,
    seed or route. Then the names in `PRIVATE_FIXTURE_NAMES` (`cellbox`,
    `app-cellbox`, `terminal`) are refused unless [`private_fixture_allowed`] holds
    or, for a `route` request, [`route_admits`] admits the whole batch.
    """
    _refuse_blocked_hashes(targets)
    if route is not None:
        route_admits(route, targets, seed_hex)
        return
    allowed = private_fixture_allowed(seed_hex)
    for target in targets:
        names = {target.name, target.resolve().name}
        if names & PRIVATE_FIXTURE_NAMES and not allowed:
            raise SigningRefused(
                f"ROLLOUT BLOCKED: unqualified Cellbox/terminal artifact: {target}"
            )


def sign_and_verify(
    repo: Path, targets: list[Path], objcopy: str, seed_hex: str | None, *,
    route: Route | None = None,
) -> None:
    """Sign each ELF in place, then re-verify it.

    The signer first creates a final, zero-signature ELF layout. Only then can
    it sign the canonical payload and replace the excluded signature bytes;
    successful objcopy alone never proves the kernel-compatible result.
    """
    # Preflight the entire batch before loading keys or mutating any target.
    # Names cover rebuilt candidates; hashes also catch renamed quarantined ELFs.
    preflight_blocked_artifacts(targets, seed_hex, route=route)
    guard_prod_key(seed_hex)
    signer = load_signer(repo)
    # The low-level signer refuses to produce a signature unless this sentinel
    # says a policy check already passed. This is the one place that sets it,
    # and it is only reachable from `cli.run_sign`/`cli.sign_route` after
    # `run_check` returned OK.
    signer._CHECKED = True
    seed = bytes.fromhex(seed_hex) if seed_hex else signer.DEV_SEED
    # Re-checked after the key is loaded, not trusted from the preflight.
    _refuse_blocked_hashes(targets)
    if route is not None:
        route_admits(route, targets, seed_hex)
    elif seed == signer.DEV_SEED and any(
        {t.name, t.resolve().name} & PRIVATE_FIXTURE_NAMES for t in targets
    ):
        raise SigningRefused("ROLLOUT BLOCKED: private fixtures never sign with the dev key")
    if len(seed) != 32:
        raise SigningRefused(f"seed must be 32 bytes (64 hex chars), got {len(seed)}")
    priv = signer._priv_from_seed(seed)
    pub = signer._pub_bytes(priv)

    for target in targets:
        if not target.is_file():
            raise SigningRefused(f"cannot sign missing binary: {target}")
        temporary = Path(str(target) + ".signed")
        signer._add_signature_placeholder(str(target), str(temporary), objcopy)
        signature = priv.sign(signer._signed_payload(temporary.read_bytes()))
        signer._write_signature(str(temporary), signature)
        os.replace(temporary, target)
        _verify(signer, target, pub)


def _verify(signer: ModuleType, target: Path, pub: bytes) -> None:
    """Re-read the written ELF and check the embedded signature against `pub`."""
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

    elf = target.read_bytes()
    payload = signer._signed_payload(elf)
    bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = signer._section_table(elf)
    sig = signer._find_section(
        elf,
        signer.SIG_SECTION,
        e_shoff,
        e_shentsize,
        e_shnum,
        e_shstrndx,
        bits,
    )
    if sig is None or len(sig) != 64:
        raise SigningRefused(f"{target}: no valid {signer.SIG_SECTION} section after signing")
    try:
        Ed25519PublicKey.from_public_bytes(pub).verify(sig, payload)
    except Exception as exc:  # cryptography raises InvalidSignature
        raise SigningRefused(f"{target}: signature does not verify after embed — {exc}") from exc
