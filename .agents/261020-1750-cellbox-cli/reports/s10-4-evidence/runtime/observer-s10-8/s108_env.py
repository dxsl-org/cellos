"""S10-8 environment: the two candidate measurement sets, new run tokens and the new evidence root.

Everything else (original deadlines, measurement-kernel manifest, ELF helpers, set specs) is the retained S10-7 module
`s107_env`, imported unchanged.  S10-7's `RUN_CONSUMED-*` tokens and `reports/s10-7-evidence/` are never touched: S10-8 uses
`RUN_CONSUMED-s108-<set>` next to this file and `reports/s10-8-evidence/`.
"""
import json
import os
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s107_env as E7  # noqa: E402

RUNTIME = E7.RUNTIME
REPORTS = E7.REPORTS
PLAN = E7.PLAN
REPO = E7.REPO
EVIDENCE = REPORTS / 's10-8-evidence'
HARNESS_EVIDENCE = EVIDENCE / 'harness'
RUN_EVIDENCE = EVIDENCE / 'measurement-runs'

# Approved run order (D8-2): candidate headless, then candidate desktop; b2-headless is NOT rerun, b2-desktop stays null (D8-4).
SET_NAMES = ('cand-headless', 'cand-desktop')
SETS = {name: E7.SETS[name] for name in SET_NAMES}
NOT_RUN_SETS = {
    'b2-headless': 'class B COMPLETE in S10-7.4; not rerun (D8-2)',
    'b2-desktop': 'stays null (D8-4): image defect, no repair authorized',
}
DEADLINES = E7.DEADLINES
sha256_file = E7.sha256_file
dump = E7.dump


def token(name):
    """Run token of the one S10-8 run of `name` (evaluated at call time: tests redirect HERE)."""
    if name not in SET_NAMES:
        raise KeyError(name)
    return HERE / f'RUN_CONSUMED-s108-{name}'


def claim_run(name, path=None):
    """Atomically claim the one approved S10-8 run of `name`; a second claim raises FileExistsError."""
    target = Path(path) if path is not None else token(name)
    descriptor = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(descriptor, json.dumps({'set': name, 'claimed_ns': time.monotonic_ns(), 'wall': time.time(), 'pid': os.getpid(),
                                     'meaning': f'S10-8 ONE run of set {name}; never remove, never rerun (D8-3)'}).encode())
    os.close(descriptor)
    return target


def run_consumed(name, path=None):
    return (Path(path) if path is not None else token(name)).exists()
