#!/usr/bin/env python3
"""S10-9.4 candidate headless observed run (original schedule, latency-recording recovery waits, fixed memory mixin, atomic RSP client).

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --preflight   # go/no-go only
    PYTHONDONTWRITEBYTECODE=1 python3 -B run_cand_headless.py --run         # preflight, claim RUN_CONSUMED-s109-cand-headless, ONE run
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import s109_run  # noqa: E402

if __name__ == '__main__':
    raise SystemExit(s109_run.main('cand-headless', sys.argv[1:]))
