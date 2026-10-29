#!/usr/bin/env python3
"""S10-8.3 candidate desktop observed run (original schedule, pause-credit recovery waits, fixed memory mixin).

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py --preflight   # go/no-go only
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py --run         # preflight, claim RUN_CONSUMED-s108-cand-desktop, ONE run
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s108_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s108_run.main('cand-desktop', sys.argv[1:]))
