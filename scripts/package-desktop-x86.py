#!/usr/bin/env python3
"""Assemble the desktop x86_64 image (headless cells plus compositor, fb-console,
desktop, files, terminal, chrome and ocel) from source.

Thin caller of `scripts/assemble-x86-image.py --profile desktop`, run as its own
process (the S10-2a candidate route admits only the assembler as the running
program): every cell and `init` (features `input,ui`) is built, signed and
checked there, the boot config comes from the `config/` templates, and the image
is assembled fresh (no overlay of an existing image). Options: `--out-dir`
(default kernel/src/embedded-x86_64), `--seed-file`, `--objcopy`.
"""

import argparse
from pathlib import Path
import subprocess
import sys

def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out-dir", default=str(root / "kernel/src/embedded-x86_64"))
    parser.add_argument("--seed-file", help="32-byte hex private signing seed (default: dev key)")
    parser.add_argument("--objcopy", help="objcopy for cellos-sign")
    args = parser.parse_args()
    argv = ["--profile", "desktop", "--out-dir", args.out_dir]
    if args.seed_file:
        argv += ["--seed-file", args.seed_file]
    if args.objcopy:
        argv += ["--objcopy", args.objcopy]
    return subprocess.run([sys.executable, str(root / "scripts/assemble-x86-image.py"),
                           *argv]).returncode

if __name__ == "__main__":
    sys.exit(main())
