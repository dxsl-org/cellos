#!/usr/bin/env python3
"""S10-7.4 candidate desktop observed run of the complete original schedule.

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py --preflight   # go/no-go only
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_desktop.py --run         # preflight, claim RUN_CONSUMED-cand-desktop, ONE run
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s107_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s107_run.main('cand-desktop', sys.argv[1:]))
