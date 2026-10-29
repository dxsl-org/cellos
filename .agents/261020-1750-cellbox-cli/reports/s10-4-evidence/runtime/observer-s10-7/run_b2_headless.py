#!/usr/bin/env python3
"""S10-7.4 B2 headless shared-subset run (schedule-b2.json).

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_b2_headless.py --preflight   # go/no-go only
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_b2_headless.py --run         # preflight, claim RUN_CONSUMED-b2-headless, ONE run
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s107_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s107_run.main('b2-headless', sys.argv[1:]))
