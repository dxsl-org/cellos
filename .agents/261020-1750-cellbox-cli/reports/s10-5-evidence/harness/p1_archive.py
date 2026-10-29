#!/usr/bin/env python3
"""S10-5 P1: hash-recorded quarantine archive of the current canonical bytes plus
the pre-edit copies of every file S10-2b edits. Nothing is deleted or modified.
Writes <evidence>/p1-archive.json."""
import hashlib, json, os, shutil, sys, datetime
from pathlib import Path

REPO = Path("/home/dmin/cellos")
EVID = REPO / ".agents/261020-1750-cellbox-cli/reports/s10-5-evidence"

QUARANTINED = [
    "kernel/src/embedded-x86_64/kernel_fs.img",
    "kernel/src/embedded-x86_64/init",
    "kernel/src/embedded-x86_64/boot-config/autoload.toml",
    "kernel/src/embedded-x86_64/boot-config/services.toml",
    "kernel/src/embedded-x86_64/boot-config/system.toml",
    "target/x86_64-unknown-none/release/cellos-kernel",
]
PRE_S10_2B = [
    "scripts/assemble-x86-image.py",
    "scripts/cellos_sign/signing.py",
    "scripts/cellos_sign/cli.py",
    "scripts/package-desktop-x86.py",
    "scripts/x86/make-iso-ci.sh",
    "scripts/build-cloud-images.sh",
    "scripts/test_cellos_sign.py",
    "scripts/test_assemble_x86_image.py",
    ".github/workflows/ci.yml",
]
NOTE = """QUARANTINED Cellbox bytes (Phase 10 S10-5, D10-14 A). NOT a safe boot set.
Containment section 'Recovery': no matched known-safe set exists. These files contain the
quarantined cellbox 64eb28f3... and terminal 636497ac... and start sshd.
Never boot, sign, copy to a canonical path, or distribute these as a release.
They are kept only so nothing is deleted. The only known-qualified set is the retained
candidate 4fae8b20... The directory pre-s10-2b/ holds the pre-edit copies of the files
S10-2b changes (code rollback).
"""


def sha(p: Path) -> str:
    with p.open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def main() -> int:
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    dest = Path.home() / ".cache/cellos-quarantine" / f"cellbox-s10-5-{stamp}"
    dest.mkdir(parents=True, mode=0o700)
    entries = {}
    for group, files in (("quarantined", QUARANTINED), ("pre-s10-2b", PRE_S10_2B)):
        for rel in files:
            src = REPO / rel
            out = dest / group / rel
            out.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, out)
            h = sha(src)
            if sha(out) != h:
                print("STOP: archive copy mismatch", rel)
                return 1
            with open(src, "rb") as a, open(out, "rb") as b:
                if a.read() != b.read():
                    print("STOP: cmp mismatch", rel)
                    return 1
            entries[f"{group}/{rel}"] = {"sha256": h, "bytes": src.stat().st_size}
            os.chmod(out, 0o400)
    (dest / "QUARANTINE.txt").write_text(NOTE)
    (dest / "MANIFEST.sha256").write_text(
        "".join(f"{e['sha256']}  {k}\n" for k, e in sorted(entries.items())))
    for p in (dest / "QUARANTINE.txt", dest / "MANIFEST.sha256"):
        os.chmod(p, 0o400)
    for root, dirs, _ in os.walk(dest):
        for d in dirs:
            os.chmod(Path(root, d), 0o700)
    # re-hash the archive from disk
    for k, e in entries.items():
        if sha(dest / k) != e["sha256"]:
            print("STOP: archive re-hash", k)
            return 1
    expect = {
        "quarantined/kernel/src/embedded-x86_64/kernel_fs.img": "447d0348735a25e8b396c11c0091ff3d214b0c0b392043bb0515d4104a298e2c",
        "quarantined/target/x86_64-unknown-none/release/cellos-kernel": "12cb0bac7849d425b2bacbb5ed503229328c525d71744fcf68d20d5c8a61ac67",
        "quarantined/kernel/src/embedded-x86_64/init": "0c2f607bb29e090b31c7820290aafc1974c818c0610d216f00b5f88a10104104",
    }
    for k, h in expect.items():
        if entries[k]["sha256"] != h:
            print("STOP: unexpected archived hash", k)
            return 1
    (EVID / "p1-archive.json").write_text(json.dumps(
        {"archive": str(dest), "entries": entries}, indent=2, sort_keys=True) + "\n")
    print("P1 OK", dest)
    return 0


if __name__ == "__main__":
    sys.exit(main())
