#!/usr/bin/env python3
"""S10-7.4 candidate headless observed run (original headless schedule from run.py).

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --preflight   # go/no-go only
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --run         # preflight, claim RUN_CONSUMED-cand-headless, ONE run
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s107_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s107_run.main('cand-headless', sys.argv[1:]))
