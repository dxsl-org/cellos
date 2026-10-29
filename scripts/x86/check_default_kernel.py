#!/usr/bin/env python3
"""G3 content rule for the default x86 kernel path (Phase 10 S10-2b).

Usage: check_default_kernel.py KERNEL EMBEDDED_DIR

Admits KERNEL only if both hold, else exits 1 with `ROLLOUT BLOCKED: ...`:

1. the ramdisk `EMBEDDED_DIR/kernel_fs.img` (read with the repository reader,
   `generate-boot-config.py image_files`) has no file whose SHA-256 is in
   `signing.BLOCKED_ARTIFACT_SHA256` (imported, never copied), no `/bin/shell`
   or `/bin/sshd`, and `/bin/cellbox` is byte-equal to `/bin/cellbox-ssh`;
2. the kernel ELF contains the exact bytes of `kernel_fs.img` and of `init`
   from `EMBEDDED_DIR`, so the file checked in (1) is the one that boots.

An explicitly selected non-default kernel is never passed to this script.
"""
import hashlib
import importlib.util
import mmap
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "scripts"))

FORBIDDEN = ("/bin/shell", "/bin/sshd")
CELLBOX, ALIAS = "/bin/cellbox", "/bin/cellbox-ssh"


def refuse(reason: str) -> None:
    print(f"ROLLOUT BLOCKED: default x86 kernel refused: {reason}; see "
          ".agents/261020-1750-cellbox-cli/reports/rollout-containment.md", file=sys.stderr)
    sys.exit(1)


def load_image_files(image: Path) -> dict:
    spec = importlib.util.spec_from_file_location(
        "generate_boot_config", REPO / "scripts" / "generate-boot-config.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.image_files(str(image))


def check(kernel: Path, embedded: Path) -> None:
    from cellos_sign import signing

    image, init = embedded / "kernel_fs.img", embedded / "init"
    for path in (kernel, image, init):
        if not path.is_file():
            refuse(f"missing {path}")
    if image.stat().st_size == 0 or init.stat().st_size == 0:
        refuse("empty embedded input")
    try:
        files = load_image_files(image)
    except Exception as exc:  # unreadable or not a FAT volume
        refuse(f"cannot read {image}: {exc}")
    for path in sorted(files):
        if hashlib.sha256(files[path]).hexdigest() in signing.BLOCKED_ARTIFACT_SHA256:
            refuse(f"quarantined bytes in the image: {path}")
    present = [path for path in FORBIDDEN if path in files]
    if present:
        refuse(f"retired paths in the image: {present}")
    if CELLBOX not in files or ALIAS not in files or files[CELLBOX] != files[ALIAS]:
        refuse(f"{ALIAS} is missing or not byte-equal to {CELLBOX}")
    with kernel.open("rb") as handle, mmap.mmap(handle.fileno(), 0, access=mmap.ACCESS_READ) as elf:
        for name, path in (("kernel_fs.img", image), ("init", init)):
            data = path.read_bytes()
            if elf.find(data) < 0:
                refuse(f"the kernel does not contain the exact {name} bytes of {embedded}")


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    check(Path(argv[1]), Path(argv[2]))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
