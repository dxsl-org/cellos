#!/usr/bin/env python3
"""Ed25519-sign a Cellos cell ELF binary and embed the signature as __ViCell_sig.

The DEV key is derived from a fixed seed (bytes [0x43]*32), reproducible —
matches CELL_SIGNER_PUBKEY in kernel/src/signing.rs (`dev-signing-key` feature).
Production signing uses --seed-hex or a KMS; never the hardcoded dev seed.

Signed payload (MUST match kernel/src/signing.rs::verify_cell_with_key):
  every byte of the final ELF container except the 64-byte `__ViCell_sig`
  payload itself. The signature section header stays covered, which authenticates
  ELF/program/section headers, all section names and offsets, and `.rela.dyn`
  metadata before the loader can use it.

Signing first adds a zero-filled signature section, signs that stable container,
then replaces only the excluded 64-byte payload. The signature section must not
have the ALLOC flag — it must never be in PT_LOAD.

Usage:
    python scripts/cellos-sign --sign cell.elf                        (the only signing route)
    python scripts/sign-cell.py --verify --in cell-signed.elf         (check signature)
    python scripts/sign-cell.py --emit-test-vector                    (print Rust consts)
    python scripts/sign-cell.py --emit-pubkey                         (print Rust const)

    --seed-hex HEX    32-byte hex seed for a custom/prod key (default: dev seed)
    --objcopy PATH    path to riscv64/aarch64 objcopy (default: $OBJCOPY env or "objcopy")

NOTE: this is the low-level signer and performs NO F1/F5 policy check, so it
refuses to sign on its own. A cell signature attests that the pipeline enforced
F1 and F5 (Spec 18 §2.1); this entry point cannot attest that, and a dev-key
signature is just as load-bearing as a production one because every local and
QEMU image is a dev-key build. Signing therefore requires either the `_CHECKED`
sentinel — set only by `cellos_sign.signing`, i.e. only after a passing check —
or the explicit `--unchecked-dev-signature` opt-in, which exists for signer
round-trip tests and produces a binary that must never reach an image. The
production-key guard is enforced here as well as in the wrapper, so no route
through this file can mint a production signature outside CI.
"""

import argparse
import os
import struct
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from cellos_sign.signing import SigningRefused, guard_prod_key  # noqa: E402

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import (
        Ed25519PrivateKey,
        Ed25519PublicKey,
    )
    from cryptography.hazmat.primitives import serialization
    _CRYPTO_VERSION = tuple(int(x) for x in __import__('cryptography').__version__.split('.')[:2])
    if _CRYPTO_VERSION < (2, 6):
        sys.exit("error: cryptography >= 2.6 required (pip install --upgrade cryptography)")
except ImportError:
    sys.exit("error: pip install cryptography")

# Fixed dev seed — deterministic, matches kernel's CELL_SIGNER_PUBKEY when
# the `dev-signing-key` feature is enabled. NEVER use in production.
DEV_SEED: bytes = bytes([0x43] * 32)

# Set to True on *this module object* by `cellos_sign.signing.sign_and_verify`,
# and nowhere else, once the F1/F5 check has passed. A direct `python3
# scripts/sign-cell.py` runs a fresh module whose sentinel is False, so the
# no-check route cannot mint a signature by accident. It is a wiring assertion,
# not a security boundary: anyone who can run the script can also edit it (see
# the threat model in scripts/cellos_sign/__init__.py).
_CHECKED = False

ELF_MAGIC = b'\x7fELF'
SIG_SECTION = "__ViCell_sig"
MANIFEST_SECTION = "__ViCell_manifest"


# ── ELF helpers ───────────────────────────────────────────────────────────────

def _read_u16le(data: bytes, offset: int) -> int:
    return struct.unpack_from("<H", data, offset)[0]

def _read_u32le(data: bytes, offset: int) -> int:
    return struct.unpack_from("<I", data, offset)[0]

def _read_u64le(data: bytes, offset: int) -> int:
    return struct.unpack_from("<Q", data, offset)[0]


def _section_table(elf: bytes) -> tuple[int, int, int, int, int]:
    """Return (bits, section-table offset, entry size, count, string-table index)."""
    assert elf[:4] == ELF_MAGIC, f"Not an ELF file (magic={elf[:4].hex()})"
    assert len(elf) >= 52, f"ELF header truncated ({len(elf)} bytes)"
    bits = 64 if elf[4] == 2 else 32 if elf[4] == 1 else 0
    assert bits, f"Unsupported ELF class: {elf[4]}"
    assert elf[5] == 1, f"Only little-endian ELF supported (ei_data={elf[5]})"
    if bits == 64:
        assert len(elf) >= 64, f"ELF64 header truncated ({len(elf)} bytes)"
        return (
            bits,
            _read_u64le(elf, 40),
            _read_u16le(elf, 58),
            _read_u16le(elf, 60),
            _read_u16le(elf, 62),
        )
    return (
        bits,
        _read_u32le(elf, 32),
        _read_u16le(elf, 46),
        _read_u16le(elf, 48),
        _read_u16le(elf, 50),
    )


def _find_section_range(
    elf: bytes,
    name: str,
    e_shoff: int,
    e_shentsize: int,
    e_shnum: int,
    e_shstrndx: int,
    bits: int,
) -> tuple[int, int] | None:
    """Return a uniquely named in-bounds section's (offset, size), or None."""
    if not e_shnum or not e_shoff or e_shentsize == 0:
        return None
    table_end = e_shoff + e_shentsize * e_shnum
    if table_end > len(elf) or e_shstrndx >= e_shnum:
        return None
    reader = _read_shdr64 if bits == 64 else _read_shdr32
    _, strtab_kind, strtab_offset, strtab_size = reader(
        elf, e_shoff, e_shentsize, e_shstrndx
    )
    if strtab_kind != 3 or strtab_offset + strtab_size > len(elf):
        return None
    wanted = name.encode("ascii")

    def read_name(sh_name: int) -> bytes | None:
        if sh_name >= strtab_size:
            return None
        start = strtab_offset + sh_name
        end = elf.find(b"\x00", start, strtab_offset + strtab_size)
        if end < 0:
            return None
        return elf[start:end]

    found = None
    for i in range(e_shnum):
        sh_name, sh_kind, sh_offset, sh_size = reader(elf, e_shoff, e_shentsize, i)
        if read_name(sh_name) != wanted:
            continue
        if sh_kind != 1 or sh_offset + sh_size > len(elf):
            return None
        if found is not None:
            return None
        found = (sh_offset, sh_size)
    return found


def _find_section(
    elf: bytes,
    name: str,
    e_shoff: int,
    e_shentsize: int,
    e_shnum: int,
    e_shstrndx: int,
    bits: int,
) -> bytes | None:
    section = _find_section_range(elf, name, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits)
    if section is None:
        return None
    offset, size = section
    return elf[offset : offset + size]


def _signed_payload(elf: bytes) -> bytes:
    """Return every final ELF byte except the fixed signature payload."""
    bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = _section_table(elf)
    signature = _find_section_range(
        elf, SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits
    )
    assert signature is not None, f"Missing {SIG_SECTION} section"
    offset, size = signature
    assert size == 64, f"{SIG_SECTION} must be 64 bytes, got {size}"
    return elf[:offset] + elf[offset + size :]

def _read_shdr64(
    elf: bytes, e_shoff: int, e_shentsize: int, index: int
) -> tuple[int, int, int, int]:
    base = e_shoff + index * e_shentsize
    return (
        _read_u32le(elf, base),
        _read_u32le(elf, base + 4),
        _read_u64le(elf, base + 24),
        _read_u64le(elf, base + 32),
    )


def _read_shdr32(
    elf: bytes, e_shoff: int, e_shentsize: int, index: int
) -> tuple[int, int, int, int]:
    base = e_shoff + index * e_shentsize
    return (
        _read_u32le(elf, base),
        _read_u32le(elf, base + 4),
        _read_u32le(elf, base + 16),
        _read_u32le(elf, base + 20),
    )


# ── Key helpers ───────────────────────────────────────────────────────────────

def _priv_from_seed(seed: bytes) -> Ed25519PrivateKey:
    assert len(seed) == 32, f"Seed must be 32 bytes, got {len(seed)}"
    return Ed25519PrivateKey.from_private_bytes(seed)

def _pub_bytes(priv: Ed25519PrivateKey) -> bytes:
    return priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


# ── Embed signature via objcopy ───────────────────────────────────────────────

def _add_signature_placeholder(elf_path: str, out_path: str, objcopy: str) -> None:
    """Create the stable final ELF layout with an excluded zero signature."""
    with tempfile.NamedTemporaryFile(suffix=".sig", delete=False) as f:
        f.write(bytes(64))
        sig_file = f.name
    try:
        subprocess.run(
            [
                objcopy,
                f"--remove-section={SIG_SECTION}",
                f"--add-section={SIG_SECTION}={sig_file}",
                f"--set-section-flags={SIG_SECTION}=noload,readonly",
                elf_path,
                out_path,
            ],
            check=True,
        )
    finally:
        os.unlink(sig_file)


def _write_signature(elf_path: str, sig: bytes) -> None:
    """Replace exactly the canonical payload's excluded signature bytes."""
    assert len(sig) == 64
    with open(elf_path, "rb") as f:
        elf = bytearray(f.read())
    bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = _section_table(elf)
    signature = _find_section_range(
        elf, SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits
    )
    assert signature is not None, f"Missing {SIG_SECTION} after objcopy"
    offset, size = signature
    assert size == len(sig), f"{SIG_SECTION} has invalid size {size}"
    elf[offset : offset + size] = sig
    with open(elf_path, "wb") as f:
        f.write(elf)


# ── Rust array literal helper ─────────────────────────────────────────────────

def _rust_array(name: str, data: bytes) -> str:
    body = ", ".join(f"0x{b:02x}" for b in data)
    return f"const {name}: [u8; {len(data)}] = [{body}];"


# ── Admission ─────────────────────────────────────────────────────────────────

def _guard_admission(args) -> None:
    """Refuse to mint a signature that no F1/F5 check stands behind.

    Raises SigningRefused unless the signature is either backed by a passing
    check (`_CHECKED`) or explicitly declared unchecked by the caller. The
    opt-in is dev-key only: a production signature is never allowed to skip the
    check, whatever the operator asks for.
    """
    if _CHECKED:
        return
    if not args.unchecked_dev_signature:
        raise SigningRefused(
            "sign-cell.py runs no F1/F5 policy check, so a signature minted here "
            "would attest nothing. Sign with `python3 scripts/cellos-sign --sign "
            "ELF...`, which checks first. For a signer round-trip test only, pass "
            "--unchecked-dev-signature."
        )
    if args.seed_hex:
        raise SigningRefused(
            "--unchecked-dev-signature is dev-key only; a production key must "
            "never sign without a passing F1/F5 check."
        )
    print("WARNING: minting an UNCHECKED dev signature — no F1/F5 check ran. "
          "This binary must never be shipped in an image.", file=sys.stderr)


# ── Main ──────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--in",   dest="inp",    help="Input ELF file path")
    ap.add_argument("--out",  dest="out",    help="Output signed ELF file path")
    ap.add_argument("--verify", action="store_true", help="Verify mode: check existing signature")
    ap.add_argument("--emit-pubkey", action="store_true", help="Print CELL_SIGNER_PUBKEY Rust const and exit")
    ap.add_argument("--emit-test-vector", action="store_true", help="Print self_test() Rust consts and exit")
    ap.add_argument("--seed-hex", default=None, help="32-byte hex seed (default: dev seed)")
    ap.add_argument("--objcopy", default=os.environ.get("OBJCOPY", "objcopy"), help="objcopy binary")
    ap.add_argument("--unchecked-dev-signature", action="store_true",
                    help="mint a dev signature with NO F1/F5 check — signer "
                         "round-trip tests only; the result attests nothing")
    args = ap.parse_args()

    # --verify and the --emit-* modes never produce a signature, so they may use
    # any key anywhere; only the signing path is gated.
    if not (args.verify or args.emit_pubkey or args.emit_test_vector):
        try:
            guard_prod_key(args.seed_hex)
            _guard_admission(args)
        except SigningRefused as exc:
            sys.exit(f"REFUSED: {exc}")

    seed = bytes.fromhex(args.seed_hex) if args.seed_hex else DEV_SEED
    priv = _priv_from_seed(seed)
    pub  = _pub_bytes(priv)

    if args.emit_pubkey:
        print(_rust_array("DEV_CELL_SIGNER_PUBKEY", pub))
        return

    if args.emit_test_vector:
        test_payload = b"CellosSigningTest"
        test_sig = priv.sign(test_payload)
        print("// Paste into kernel/src/signing.rs self_test() constants:")
        print(_rust_array("TEST_PUBKEY", pub))
        print(f'const TEST_PAYLOAD: &[u8] = b"CellosSigningTest";')
        print(_rust_array("TEST_SIG", test_sig))
        return

    if not args.inp:
        ap.error("--in is required")

    with open(args.inp, "rb") as f:
        elf = f.read()

    if args.verify:
        try:
            bits, e_shoff, e_shentsize, e_shnum, e_shstrndx = _section_table(elf)
            sig_bytes = _find_section(
                elf, SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits
            )
            if sig_bytes is None or len(sig_bytes) != 64:
                raise ValueError(f"no valid {SIG_SECTION} section")
            Ed25519PublicKey.from_public_bytes(pub).verify(sig_bytes, _signed_payload(elf))
            print(f"OK: signature valid ({args.inp})")
        except Exception as exc:
            print(f"FAIL: signature invalid — {exc}", file=sys.stderr)
            sys.exit(3)
        return

    out_path = args.out or args.inp
    if out_path == args.inp:
        with tempfile.NamedTemporaryFile(
            suffix=".elf", delete=False, dir=os.path.dirname(args.inp) or "."
        ) as temporary:
            target_path = temporary.name
    else:
        target_path = out_path

    try:
        _add_signature_placeholder(args.inp, target_path, args.objcopy)
        with open(target_path, "rb") as f:
            stable_elf = f.read()
        _write_signature(target_path, priv.sign(_signed_payload(stable_elf)))
        if target_path != out_path:
            os.replace(target_path, out_path)
    except Exception:
        if target_path != out_path:
            try:
                os.unlink(target_path)
            except OSError:
                pass
        raise

    print(f"OK: signed -> {out_path} ({len(elf)} source bytes)")


if __name__ == "__main__":
    main()
