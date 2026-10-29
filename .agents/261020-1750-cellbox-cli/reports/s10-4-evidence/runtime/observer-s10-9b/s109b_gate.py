#!/usr/bin/env python3
"""S10-9b offline gate: ONE invocation runs the retained S10-4.17 gate (269 tests, one stale pre-run preflight test excluded and
disclosed), the 35 observer-memory tests, the 91 S10-7.4 tests, the 85 S10-8 tests, the 142 S10-9 tests and the new S10-9b tests
together.  Never starts a VM, QEMU or any guest operation.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109b_gate.py

Composition = the S10-9 gate (`s109_gate`, imported unchanged: module lists, retained-path shims, excluded-test handling, frozen-source
collection) + the S10-9b modules.  Every invocation (pass or fail) archives its raw output under gate-attempts/attempt-NN/ and a receipt;
only a fully clean invocation (zero failures/errors/skipped/expected failures/unexpected successes and the exact expected component
counts) writes verification-summary-s10-9b.json and freezes the executing-source hashes (frozen-sources-s10-9b.sha256).  Nothing is written
into the retained archives or into observer-s10-7/-8/-9: the retained gate's own side effects are redirected into this attempt directory.
"""
import hashlib
import json
import os
import platform
import sys
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s109_gate as G9  # noqa: E402  (S10-9 gate: unchanged; importing runs nothing)
import s107_env as E7  # noqa: E402
import s107_retained as RET  # noqa: E402
import rendered_proof  # noqa: E402  (retained module)

RG, G = G9.RG, G9.G
ATTEMPTS = HERE / 'gate-attempts'
EXCLUDED_RETAINED = G9.EXCLUDED_RETAINED
S109B_MODULES = ['test_s109b_wiring', 'test_s109b_oracle', 'test_s109b_gonogo']
EXPECT = dict(G9.EXPECT, s10_9=142)
SUMMARY = HERE / 'verification-summary-s10-9b.json'
FROZEN = HERE / 'frozen-sources-s10-9b.sha256'


def own_sources():
    return sorted(HERE.glob('*.py')) + [ENV.SCHEDULE]


def frozen_sources():
    """Everything the S10-9 gate froze (retained + feed-hook + memory + S10-7 + S10-8 + S10-9) plus the S10-9b executing sources/schedule."""
    sources = dict(G9.frozen_sources())
    sources.update({str(path): ENV.sha256_file(path) for path in own_sources()})
    return sources


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
    link.symlink_to(E7.RUNTIME / 'observer-recurring')
    try:
        return run_attempt(attempt, number)
    finally:
        link.unlink()


def run_attempt(attempt, number):
    reproduction = attempt / 'retained-test-reproduction'
    reproduction.mkdir(mode=0o700)
    module, retained, old_count, retained_count = RG.retained_suite()
    original_exercise = module.OLD.P.exercise

    def redirected(client_type, directory, label):
        target = reproduction if Path(directory) == G.PREVIOUS else directory
        return original_exercise(client_type, target, label)

    loader = unittest.TestLoader()
    started = time.time()
    raw = attempt / 'stdout-stderr.txt'
    with G9.retained_paths() as shims:
        def suite(names):
            built = unittest.TestSuite([loader.loadTestsFromName(name) for name in names])
            return built, RG.count(built)

        feed_hook_all, feed_total = suite(RG.NEW_MODULES)
        feed_hook, excluded = G9.without_excluded(feed_hook_all)
        memory, memory_count = suite(G9.MEMORY_MODULES)
        s107, s107_count = suite(G9.S107_MODULES)
        s108, s108_count = suite(G9.S108_MODULES)
        s109, s109_count = suite(G9.S109_MODULES)
        s109b, s109b_count = suite(S109B_MODULES)
        retained_kept, retained_excluded = G9.without_excluded(retained)
        excluded += retained_excluded
        feed_count = feed_total - len(excluded)
        whole = unittest.TestSuite([retained_kept, feed_hook, memory, s107, s108, s109, s109b])
        with raw.open('w') as stream, patch.object(module.OLD.P, 'exercise', side_effect=redirected), patch.object(rendered_proof, 'HERE', attempt):
            runner = unittest.TextTestRunner(stream=stream, verbosity=2, resultclass=RG.Recorder)
            result = runner.run(whole)
    finished = time.time()
    components = {'retained_original': old_count, 'retained_prior_current': retained_count - old_count, 'feed_hook_new': feed_total,
                  'feed_hook_new_executed': feed_count, 'memory': memory_count, 's10_7': s107_count, 's10_8': s108_count, 's10_9': s109_count,
                  'new_s10_9b': s109b_count}
    expected_total = retained_count + feed_count + memory_count + s107_count + s108_count + s109_count + s109b_count
    clean = (result.wasSuccessful() and not result.skipped and not result.expectedFailures and not result.unexpectedSuccesses
             and result.testsRun == expected_total and all(components[k] == v for k, v in EXPECT.items()) and s109b_count > 0
             and len(excluded) == 1 and excluded[0].endswith(EXCLUDED_RETAINED[1]))
    receipt = {
        'attempt': number, 'status': 'PASS' if clean else 'FAIL', 'single_invocation': True, 'tests_run': result.testsRun,
        'components': components, 'retained_gate_total': components['retained_original'] + components['retained_prior_current'] + components['feed_hook_new'],
        'retained_gate_executed': components['retained_original'] + components['retained_prior_current'] + components['feed_hook_new_executed'],
        'excluded_retained_tests': excluded, 'excluded_reason': 'state-dependent S10-4.17 pre-run preflight test; see EXCLUDED_RETAINED in s108_gate.py / s109_gate.py',
        'temporary_retained_path_links': shims,
        'failures': len(result.failures), 'errors': len(result.errors), 'skipped': len(result.skipped),
        'expected_failures': len(result.expectedFailures), 'unexpected_successes': len(result.unexpectedSuccesses),
        'argv': sys.argv, 'cwd': os.getcwd(), 'python': platform.python_version(),
        'env_PYTHONDONTWRITEBYTECODE': os.environ.get('PYTHONDONTWRITEBYTECODE'),
        'env_sha256': hashlib.sha256(json.dumps(sorted(os.environ.items())).encode()).hexdigest(),
        'start': started, 'end': finished, 'duration_seconds': finished - started,
        'modules': {'feed_hook_new': RG.NEW_MODULES, 'memory': G9.MEMORY_MODULES, 's10_7': G9.S107_MODULES, 's10_8': G9.S108_MODULES,
                    's10_9': G9.S109_MODULES, 'new_s10_9b': S109B_MODULES},
        'raw_output': str(raw.relative_to(HERE)), 'raw_output_sha256': hashlib.sha256(raw.read_bytes()).hexdigest(),
        'failed_tests': sorted(k for k, v in result.outcomes.items() if v != 'ok'), 'outcomes': result.outcomes,
        'retained_redirected_to': str(reproduction.relative_to(HERE)),
    }
    (attempt / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({k: receipt[k] for k in ('attempt', 'status', 'tests_run', 'components', 'retained_gate_total', 'retained_gate_executed',
                                               'excluded_retained_tests', 'failures', 'errors', 'skipped', 'failed_tests')}))
    if clean:
        summary = {k: v for k, v in receipt.items() if k != 'outcomes'}
        summary['sources_sha256'] = frozen_sources()
        summary['method'] = ('ONE unittest invocation: retained S10-4.17 gate (144 + 47 + 78 = 269, one stale preflight test excluded) + 35 observer-memory '
                             'tests + 91 S10-7.4 tests + 85 S10-8 tests + 142 S10-9 tests + new S10-9b tests, zero failures/errors/skips; retained synthetic '
                             'outputs redirected to this attempt directory, prior archives and observer-s10-7/-8/-9 untouched')
        SUMMARY.write_text(json.dumps(summary, indent=2) + '\n')
        FROZEN.write_text(json.dumps(summary['sources_sha256'], indent=2, sort_keys=True) + '\n')
        return 0
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
