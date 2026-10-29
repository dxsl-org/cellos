#!/usr/bin/env python3
"""S10-9b: the ONE B2 desktop (v2 image) measurement run.  `--preflight` (offline go/no-go) or `--run` (once; never repeated)."""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s109b_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s109b_run.main(sys.argv[1:]))
