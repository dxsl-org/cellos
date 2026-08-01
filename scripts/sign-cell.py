#!/usr/bin/env python3
"""Ed25519-sign a Cellos cell ELF binary and embed the signature as __ViCell_sig.

The DEV key is derived from a fixed seed (bytes [0x43]*32), reproducible —
matches CELL_SIGNER_PUBKEY in kernel/src/signing.rs (`dev-signing-key` feature).
Production signing uses --seed-hex or a KMS; never the hardcoded dev seed.

Signed payload (MUST match kernel/src/signing.rs::verify_cell_with_key):
  1. PT_LOAD segments sorted by (p_offset, p_filesz, phdr_index)
  2. __ViCell_manifest section bytes (if present)

  NOTE: The ELF header is NOT signed — objcopy mutates e_shnum/e_shoff when
  embedding __ViCell_sig, so including the header would break verification.

The signature is embedded as a non-loadable ELF section `__ViCell_sig` (64 bytes)
via `objcopy`. The section must not have the ALLOC flag — it must never be in PT_LOAD.

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


def _pt_load_payload(elf: bytes) -> bytes:
    """Return the signed payload: sorted PT_LOAD data || manifest section.

    The ELF header is NOT signed: objcopy mutates e_shnum/e_shoff when adding
    __ViCell_sig, which would invalidate the signature at verify time. PT_LOAD
    covers all executable code; the manifest covers all capability claims.
    """
    assert elf[:4] == ELF_MAGIC, f"Not an ELF file (magic={elf[:4].hex()})"
    ei_class = elf[4]   # 1=32-bit, 2=64-bit
    ei_data  = elf[5]   # 1=LE, 2=BE
    assert ei_class in (1, 2), f"Unsupported ELF class: {ei_class}"
    assert ei_data == 1,       f"Only little-endian ELF supported (ei_data={ei_data})"

    bits = 64 if ei_class == 2 else 32

    if bits == 64:
        e_phoff     = _read_u64le(elf, 32)
        e_phentsize = _read_u16le(elf, 54)
        e_phnum     = _read_u16le(elf, 56)

        def read_phdr(i: int):
            base = e_phoff + i * e_phentsize
            p_type   = _read_u32le(elf, base)
            p_offset = _read_u64le(elf, base + 8)
            p_filesz = _read_u64le(elf, base + 32)
            return p_type, p_offset, p_filesz

        e_shoff     = _read_u64le(elf, 40)
        e_shentsize = _read_u16le(elf, 58)
        e_shnum     = _read_u16le(elf, 60)
        e_shstrndx  = _read_u16le(elf, 62)

        def read_shdr(i: int):
            base = e_shoff + i * e_shentsize
            sh_name   = _read_u32le(elf, base)
            sh_offset = _read_u64le(elf, base + 24)
            sh_size   = _read_u64le(elf, base + 32)
            return sh_name, sh_offset, sh_size
    else:
        e_phoff     = _read_u32le(elf, 28)
        e_phentsize = _read_u16le(elf, 42)
        e_phnum     = _read_u16le(elf, 44)

        def read_phdr(i: int):
            base = e_phoff + i * e_phentsize
            p_type   = _read_u32le(elf, base)
            p_offset = _read_u32le(elf, base + 4)
            p_filesz = _read_u32le(elf, base + 16)
            return p_type, p_offset, p_filesz

        e_shoff     = _read_u32le(elf, 32)
        e_shentsize = _read_u16le(elf, 46)
        e_shnum     = _read_u16le(elf, 48)
        e_shstrndx  = _read_u16le(elf, 50)

        def read_shdr(i: int):
            base = e_shoff + i * e_shentsize
            sh_name   = _read_u32le(elf, base)
            sh_offset = _read_u32le(elf, base + 16)
            sh_size   = _read_u32le(elf, base + 20)
            return sh_name, sh_offset, sh_size

    PT_LOAD = 1

    # 1. PT_LOAD segments sorted by (p_offset, p_filesz, phdr_index)
    segments = []
    for i in range(e_phnum):
        p_type, p_offset, p_filesz = read_phdr(i)
        if p_type == PT_LOAD and p_filesz > 0:
            segments.append((p_offset, p_filesz, i))
    segments.sort()   # lexicographic on (p_offset, p_filesz, phdr_index)

    payload = bytearray()
    for p_offset, p_filesz, _ in segments:
        payload += elf[p_offset : p_offset + p_filesz]

    # 2. __ViCell_manifest section bytes (if present)
    manifest = _find_section(elf, MANIFEST_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits)
    if manifest is not None:
        payload += manifest

    return bytes(payload)


def _find_section(elf: bytes, name: str, e_shoff: int, e_shentsize: int,
                  e_shnum: int, e_shstrndx: int, bits: int) -> bytes | None:
    """Return raw bytes of the named ELF section, or None if not found."""
    if e_shnum == 0 or e_shoff == 0:
        return None

    if bits == 64:
        strtab_sh_name, strtab_offset, strtab_size = _read_shdr64(elf, e_shoff, e_shentsize, e_shstrndx)
    else:
        strtab_sh_name, strtab_offset, strtab_size = _read_shdr32(elf, e_shoff, e_shentsize, e_shstrndx)

    def read_name(sh_name: int) -> str:
        end = elf.index(b'\x00', strtab_offset + sh_name)
        return elf[strtab_offset + sh_name : end].decode('ascii', errors='replace')

    for i in range(e_shnum):
        if bits == 64:
            sh_name, sh_offset, sh_size = _read_shdr64(elf, e_shoff, e_shentsize, i)
        else:
            sh_name, sh_offset, sh_size = _read_shdr32(elf, e_shoff, e_shentsize, i)
        if sh_size == 0:
            continue
        try:
            if read_name(sh_name) == name:
                return elf[sh_offset : sh_offset + sh_size]
        except (ValueError, IndexError):
            continue
    return None


def _read_shdr64(elf: bytes, e_shoff: int, e_shentsize: int, i: int):
    base = e_shoff + i * e_shentsize
    sh_name   = _read_u32le(elf, base)
    sh_offset = _read_u64le(elf, base + 24)
    sh_size   = _read_u64le(elf, base + 32)
    return sh_name, sh_offset, sh_size

def _read_shdr32(elf: bytes, e_shoff: int, e_shentsize: int, i: int):
    base = e_shoff + i * e_shentsize
    sh_name   = _read_u32le(elf, base)
    sh_offset = _read_u32le(elf, base + 16)
    sh_size   = _read_u32le(elf, base + 20)
    return sh_name, sh_offset, sh_size


# ── Key helpers ───────────────────────────────────────────────────────────────

def _priv_from_seed(seed: bytes) -> Ed25519PrivateKey:
    assert len(seed) == 32, f"Seed must be 32 bytes, got {len(seed)}"
    return Ed25519PrivateKey.from_private_bytes(seed)

def _pub_bytes(priv: Ed25519PrivateKey) -> bytes:
    return priv.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


# ── Embed signature via objcopy ───────────────────────────────────────────────

def _embed_sig(elf_path: str, sig: bytes, out_path: str, objcopy: str) -> None:
    assert len(sig) == 64
    with tempfile.NamedTemporaryFile(suffix=".sig", delete=False) as f:
        f.write(sig)
        sig_file = f.name
    try:
        subprocess.run(
            [
                objcopy,
                f"--remove-section={SIG_SECTION}",   # strip stale sig (no-op if absent)
                f"--add-section={SIG_SECTION}={sig_file}",
                f"--set-section-flags={SIG_SECTION}=noload,readonly",
                elf_path,
                out_path,
            ],
            check=True,
        )
    finally:
        os.unlink(sig_file)


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

    payload = _pt_load_payload(elf)

    if args.verify:
        # Extract __ViCell_sig from the ELF
        ei_class = elf[4]
        bits = 64 if ei_class == 2 else 32
        if bits == 64:
            e_shoff     = _read_u64le(elf, 40)
            e_shentsize = _read_u16le(elf, 58)
            e_shnum     = _read_u16le(elf, 60)
            e_shstrndx  = _read_u16le(elf, 62)
        else:
            e_shoff     = _read_u32le(elf, 32)
            e_shentsize = _read_u16le(elf, 46)
            e_shnum     = _read_u16le(elf, 48)
            e_shstrndx  = _read_u16le(elf, 50)
        sig_bytes = _find_section(elf, SIG_SECTION, e_shoff, e_shentsize, e_shnum, e_shstrndx, bits)
        if sig_bytes is None or len(sig_bytes) != 64:
            print(f"FAIL: no valid {SIG_SECTION} section in {args.inp}", file=sys.stderr)
            sys.exit(3)
        pubkey_obj = Ed25519PublicKey.from_public_bytes(pub)
        try:
            pubkey_obj.verify(sig_bytes, payload)
            print(f"OK: signature valid ({args.inp})")
        except Exception as e:
            print(f"FAIL: signature invalid — {e}", file=sys.stderr)
            sys.exit(3)
        return

    # Sign mode
    sig = priv.sign(payload)
    assert len(sig) == 64

    out_path = args.out or args.inp
    # If signing in-place, write to a temp then replace.
    if out_path == args.inp:
        with tempfile.NamedTemporaryFile(suffix=".elf", delete=False, dir=os.path.dirname(args.inp) or ".") as tf:
            tf_path = tf.name
        try:
            _embed_sig(args.inp, sig, tf_path, args.objcopy)
            os.replace(tf_path, out_path)
        except Exception:
            try: os.unlink(tf_path)
            except OSError: pass
            raise
    else:
        _embed_sig(args.inp, sig, out_path, args.objcopy)

    print(f"OK: signed -> {out_path} ({len(elf)} + 64 B sig)")


if __name__ == "__main__":
    main()
