#!/usr/bin/env python3
"""Phase 09 S9-3 fixture disk: the preprovisioned SSH host-trust store (D79-10).

Usage: phase09_trust_disk.py DISK_IMG CONFIG_DIR HOST_KEY_PUB PORT [REKEY_PORT]

Writes a sparse private disk laid out like `api::abi::disk`:
  P1 FAT32 (/mnt/sd)  the lane's own /etc/cellos (P1 is the config override volume).
  P5 CellosFS (/srv)  /.cellos-trust/ssh_known_hosts, seeded offline by
                      tools/cellos-fs-provision --seed, in store format v1
                      (cells/services/vfs/src/trust.rs):
    10.0.2.2 PORT  the fixture's real key   -> KnownExact for `ssh ... 10.0.2.2`
    host     PORT  a different key          -> Mismatch for `ssh ... host`
    10.0.2.2 REKEY_PORT  the same real key (the S9-6 `RekeyLimit 64K` server), if given
  No entry for `gateway` (it resolves to 10.0.2.2 too) -> Unknown.

HOST_KEY_PUB is the fixture's OpenSSH public key line (`ssh-ed25519 BASE64 comment`).
The image is a throw-away fixture for one private QEMU run, never a user disk.
"""
import base64
import os
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FAT_BASE_LBA = 2048
FAT_SECTORS = 524_288
SRV_BASE_LBA = 931_072
SRV_SECTORS = 131_072
DISK_BYTES = 1_127_680 * 512


def run(*cmd, env=None):
    subprocess.run(cmd, check=True, env=env)


def wrong_key() -> str:
    """A well-formed ed25519 blob that is not the fixture's key."""
    blob = len(b"ssh-ed25519").to_bytes(4, "big") + b"ssh-ed25519" + (32).to_bytes(4, "big") + b"\x5a" * 32
    return base64.b64encode(blob).decode()


def main() -> int:
    if len(sys.argv) not in (5, 6):
        print(__doc__, file=sys.stderr)
        return 2
    disk, config, pub, port = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4])
    rekey_port = int(sys.argv[5]) if len(sys.argv) == 6 else None
    algorithm, key = pub.read_text().split()[:2]
    if algorithm != "ssh-ed25519":
        print(f"FAIL: fixture key is {algorithm}", file=sys.stderr)
        return 1
    store = (
        "cellos-ssh-trust v1\n"
        f"10.0.2.2 {port} ssh-ed25519 {key} preprovisioned\n"
        f"host {port} ssh-ed25519 {wrong_key()} preprovisioned\n"
    )
    if rekey_port is not None:
        store += f"10.0.2.2 {rekey_port} ssh-ed25519 {key} preprovisioned\n"
    with open(disk, "wb") as f:
        f.truncate(DISK_BYTES)
    run(sys.executable, str(ROOT / "tools/mkfat32_inplace.py"), "--config-dir", str(config),
        str(disk), str(FAT_SECTORS), str(FAT_BASE_LBA))
    with tempfile.TemporaryDirectory() as tmp:
        srv = Path(tmp) / "srv"
        (srv / ".cellos-trust").mkdir(parents=True)
        (srv / ".cellos-trust/ssh_known_hosts").write_text(store)
        tool = ROOT / "tools/cellos-fs-provision"
        host = subprocess.run(["rustc", "-vV"], check=True, capture_output=True, text=True).stdout
        triple = next(l.split(": ", 1)[1] for l in host.splitlines() if l.startswith("host: "))
        env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS")}
        run("cargo", "run", "--quiet", "--locked", "--offline", "--manifest-path", str(tool / "Cargo.toml"),
            "--target", triple, "--", "--image", str(disk), "--base", str(SRV_BASE_LBA),
            "--sectors", str(SRV_SECTORS), "--intent", "fresh-blank",
            "--receipt", str(disk.with_name("srv-receipt.json")), "--seed", str(srv), env=env)
    rekey = "" if rekey_port is None else f", 10.0.2.2:{rekey_port} exact"
    print(f"trust fixture: {disk} (10.0.2.2:{port} exact, host:{port} mismatched{rekey})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
