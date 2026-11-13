#!/usr/bin/env python3
"""Phase 03 machine-pin fixture disk (ADR-0029 §3): an x86 NVMe/SATA disk with a /srv.

Usage: machine_pin_disk.py DISK_IMG [--corrupt-pin]

Writes a sparse private disk laid out like `abi::disk`:
  P1 FAT32 (/mnt/sd)  empty: the image's own /etc/cellos plan applies.
  P5 CellosFS (/srv)  provisioned offline by tools/cellos-fs-provision, blank, so
                      init's first boot records /srv/.cellos-machine/pin.
  --corrupt-pin       seeds /srv/.cellos-machine/pin with 64 bytes that are not a
                      ViMachineFactsV1 record (version 0x5a5a), for the
                      "corrupt record enters safe mode" lane.

The receipt is written beside the image as DISK_IMG.srv-receipt.json and must not
exist yet. The image is a throw-away fixture for private QEMU runs, never a user disk.
"""
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
FAT_BASE_LBA = 2048
FAT_SECTORS = 524_288
SRV_BASE_LBA = 931_072
SRV_SECTORS = 131_072
DISK_BYTES = 1_127_680 * 512


def run(*cmd, env=None):
    subprocess.run(cmd, check=True, env=env)


def main() -> int:
    args = sys.argv[1:]
    corrupt = "--corrupt-pin" in args
    paths = [arg for arg in args if arg != "--corrupt-pin"]
    if len(paths) != 1 or len(args) - len(paths) > 1:
        print(__doc__, file=sys.stderr)
        return 2
    disk = Path(paths[0])
    receipt = Path(str(disk) + ".srv-receipt.json")
    with open(disk, "wb") as f:
        f.truncate(DISK_BYTES)
    run(sys.executable, str(ROOT / "tools/mkfat32_inplace.py"), str(disk), str(FAT_SECTORS), str(FAT_BASE_LBA))
    host = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
    triple = next(l.split(": ", 1)[1] for l in host.splitlines() if l.startswith("host: "))
    env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")}
    tool = ROOT / "tools/cellos-fs-provision"
    with tempfile.TemporaryDirectory() as tmp:
        provision = ["cargo", "run", "--quiet", "--locked", "--offline", "--manifest-path",
                     str(tool / "Cargo.toml"), "--target", triple, "--", "--image", str(disk),
                     "--base", str(SRV_BASE_LBA), "--sectors", str(SRV_SECTORS),
                     "--intent", "fresh-blank", "--receipt", str(receipt)]
        if corrupt:
            srv = Path(tmp) / "srv"
            (srv / ".cellos-machine").mkdir(parents=True)
            (srv / ".cellos-machine/pin").write_bytes(b"\x5a" * 64)
            provision += ["--seed", str(srv)]
        run(*provision, env=env)
    state = "corrupt pin seeded" if corrupt else "blank /srv"
    print(f"machine pin fixture: {disk} ({state})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
