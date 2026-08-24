"""Signing side of `cellos-sign`: the prod-key gate and the sign/verify calls.

The Ed25519 payload rules, the `__ViCell_sig` section layout and the objcopy
invocation all stay in `scripts/sign-cell.py` and are called from here, not
reimplemented: the kernel verifier (`kernel/src/signing.rs`) is byte-compatible
with exactly one producer, and a second copy of those rules is a second thing
that can drift out of agreement with it.
"""

from __future__ import annotations

import importlib.util
import os
import sys
from pathlib import Path
from types import ModuleType

# Environment markers that mean "this is a hosted CI runner". The production key
# is only ever released to one of these.
CI_MARKERS = ("GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE", "CIRCLECI", "CELLOS_SIGN_CI")

# Escape hatch for a self-hosted KMS signer that sets none of the above. Named
# so that it cannot be enabled by accident and shows up in a grep of any script.
PROD_OVERRIDE_ENV = "CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI"


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


def sign_and_verify(
    repo: Path, targets: list[Path], objcopy: str, seed_hex: str | None
) -> None:
    """Sign each ELF in place, then re-verify it.

    The signer first creates a final, zero-signature ELF layout. Only then can
    it sign the canonical payload and replace the excluded signature bytes;
    successful objcopy alone never proves the kernel-compatible result.
    """
    guard_prod_key(seed_hex)
    signer = load_signer(repo)
    # The low-level signer refuses to produce a signature unless this sentinel
    # says a policy check already passed. This is the one place that sets it,
    # and it is only reachable from `cli.run_sign` after `run_check` returned OK.
    signer._CHECKED = True
    seed = bytes.fromhex(seed_hex) if seed_hex else signer.DEV_SEED
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
