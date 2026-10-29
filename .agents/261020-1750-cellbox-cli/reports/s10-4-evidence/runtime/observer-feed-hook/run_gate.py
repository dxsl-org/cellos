#!/usr/bin/env python3
"""S10-4.17 offline gate: ONE invocation runs T1-T11 plus the retained 144 and 47 tests together.

    PYTHONDONTWRITEBYTECODE=1 python3 -B run_gate.py

Every invocation (pass or fail) archives its own raw output under gate-attempts/attempt-NN/ and a
receipt; only a fully clean invocation (zero failures/errors/skipped/expected failures/unexpected
successes) writes verification-summary.json and freezes the executing-source hashes.  This file
never starts a VM, QEMU or any guest operation.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import sys
import time
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_observer as HO  # noqa: E402  (also puts every retained archive on sys.path)
import run_hook as RH  # noqa: E402

G = HO.G
NEW_MODULES = ['test_t1_static', 'test_t2_t3_observer', 'test_t4_t5_ledger_oracle', 'test_t6_screen', 'test_t7_t11_flow']
ATTEMPTS = HERE / 'gate-attempts'


class Recorder(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.outcomes = {}

    def addSuccess(self, test):
        super().addSuccess(test)
        self.outcomes[test.id()] = 'ok'

    def addFailure(self, test, err):
        super().addFailure(test, err)
        self.outcomes[test.id()] = 'FAIL'

    def addError(self, test, err):
        super().addError(test, err)
        self.outcomes[test.id()] = 'ERROR'

    def addSkip(self, test, reason):
        super().addSkip(test, reason)
        self.outcomes[test.id()] = 'SKIP'

    def addExpectedFailure(self, test, err):
        super().addExpectedFailure(test, err)
        self.outcomes[test.id()] = 'XFAIL'

    def addUnexpectedSuccess(self, test):
        super().addUnexpectedSuccess(test)
        self.outcomes[test.id()] = 'XPASS'

    def addSubTest(self, test, subtest, err):
        super().addSubTest(test, subtest, err)
        if err is not None:
            self.outcomes[subtest.id()] = 'SUBFAIL'


def count(suite):
    return sum(count(item) if isinstance(item, unittest.TestSuite) else 1 for item in suite)


def retained_suite():
    spec = importlib.util.spec_from_file_location('retained_temporal_all', G.HERE / 'test_all.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    loader = unittest.TestLoader()
    old = loader.loadTestsFromModule(module.OLD)
    whole = loader.loadTestsFromModule(module)
    return module, whole, count(old), count(whole)


def main():
    if not sys.flags.dont_write_bytecode or os.environ.get('PYTHONDONTWRITEBYTECODE') != '1':
        print('refusing: run with PYTHONDONTWRITEBYTECODE=1 python3 -B (no bytecode in prior archives)', file=sys.stderr)
        return 64
    ATTEMPTS.mkdir(exist_ok=True)
    number = 1
    while True:
        attempt = ATTEMPTS / f'attempt-{number:02d}'
        try:
            attempt.mkdir(mode=0o700)
            break
        except FileExistsError:
            number += 1
    link = ATTEMPTS / 'observer-recurring'  # lets the retained rendered proof resolve its HERE.parent fixture path
    link.symlink_to(HERE.parent / 'observer-recurring')
    try:
        return run_attempt(attempt, number)
    finally:
        link.unlink()


def run_attempt(attempt, number):
    output = attempt / 'retained-output'
    output.mkdir()
    reproduction = attempt / 'retained-test-reproduction'
    reproduction.mkdir(mode=0o700)
    module, retained, old_count, retained_count = retained_suite()
    import rendered_proof  # retained module (already on sys.path through gui_temporal)
    original_exercise = module.OLD.P.exercise

    def redirected(client_type, directory, label):
        # Retained regression unchanged; only its synthetic output destination leaves the prior archive.
        target = reproduction if Path(directory) == G.PREVIOUS else directory
        return original_exercise(client_type, target, label)

    loader = unittest.TestLoader()
    new = unittest.TestSuite([loader.loadTestsFromName(name) for name in NEW_MODULES])
    new_count = count(new)
    suite = unittest.TestSuite([retained, new])
    started = time.time()
    raw = attempt / 'stdout-stderr.txt'
    with raw.open('w') as stream, patch.object(module.OLD.P, 'exercise', side_effect=redirected), \
            patch.object(rendered_proof, 'HERE', attempt):
        runner = unittest.TextTestRunner(stream=stream, verbosity=2, resultclass=Recorder)
        result = runner.run(suite)
    finished = time.time()
    clean = (result.wasSuccessful() and not result.skipped and not result.expectedFailures
             and not result.unexpectedSuccesses and result.testsRun == retained_count + new_count
             and old_count == 144 and retained_count == 191)
    receipt = {
        'attempt': number, 'status': 'PASS' if clean else 'FAIL', 'single_invocation': True,
        'tests_run': result.testsRun, 'retained_tests': old_count, 'prior_current_tests': retained_count - old_count,
        'new_tests': new_count, 'failures': len(result.failures), 'errors': len(result.errors),
        'skipped': len(result.skipped), 'expected_failures': len(result.expectedFailures),
        'unexpected_successes': len(result.unexpectedSuccesses),
        'argv': sys.argv, 'cwd': os.getcwd(), 'python': platform.python_version(),
        'env_PYTHONDONTWRITEBYTECODE': os.environ.get('PYTHONDONTWRITEBYTECODE'),
        'env_sha256': hashlib.sha256(json.dumps(sorted(os.environ.items())).encode()).hexdigest(),
        'start': started, 'end': finished, 'duration_seconds': finished - started,
        'new_modules': NEW_MODULES, 'raw_output': str(raw.relative_to(HERE)),
        'raw_output_sha256': hashlib.sha256(raw.read_bytes()).hexdigest(),
        'failed_tests': sorted(k for k, v in result.outcomes.items() if v != 'ok'),
        'outcomes': result.outcomes,
        'retained_redirected_to': str(reproduction.relative_to(HERE)),
    }
    (attempt / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({k: receipt[k] for k in ('attempt', 'status', 'tests_run', 'retained_tests', 'prior_current_tests',
                                               'new_tests', 'failures', 'errors', 'skipped', 'failed_tests')}))
    if clean:
        summary = {k: v for k, v in receipt.items() if k not in ('outcomes',)}
        summary['sources_sha256'] = RH.frozen_sources()
        summary['method'] = ('ONE unittest invocation: retained 144 (atomic/coordinated/etc) + 47 S10-4.15 temporal tests '
                             '+ T1-T11 new tests, zero failures/errors/skips; retained synthetic outputs redirected to '
                             'this attempt directory, prior archives untouched')
        (HERE / 'verification-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
        (HERE / 'frozen-sources.sha256').write_text(json.dumps(summary['sources_sha256'], indent=2, sort_keys=True) + '\n')
        return 0
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
