#!/usr/bin/env python3
"""S10-7.4 offline gate: ONE invocation runs the retained S10-4.17 gate (269 tests), the 35 observer-memory tests and the new
S10-7.4 tests together.  Never starts a VM, QEMU or any guest operation.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s107_gate.py

Every invocation (pass or fail) archives its raw output under gate-attempts/attempt-NN/ and a receipt; only a fully clean
invocation (zero failures/errors/skipped/expected failures/unexpected successes and the exact expected component counts)
writes verification-summary-s10-7.json and freezes the executing-source hashes (frozen-sources-s10-7.sha256).  Nothing is
written into the retained archives: the retained gate's own side effects are redirected into this attempt directory.
"""
import contextlib
import hashlib
import json
import os
import platform
import shutil
import sys
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402
import s107_retained as RET  # noqa: E402  (puts every retained archive on sys.path)
import s107_memory  # noqa: E402,F401  (puts observer-memory on sys.path for its tests)
import run_gate as RG  # noqa: E402  (retained gate: Recorder/count/retained_suite/NEW_MODULES; importing runs nothing)
import rendered_proof  # noqa: E402  (retained module)

G = RET.G
ATTEMPTS = HERE / 'gate-attempts'
# ONE retained test cannot hold in today's environment and is excluded (recorded, never silently): it re-runs the S10-4.17 PRE-RUN
# go/no-go (run_hook.preflight) and asserts G5 (RUN_CONSUMED absent / desktop dir absent / run output absent), G4 compressed trace present
# (/tmp/cellbox-s10-4/runtime/*.gz, 431,927,301 B, vanished), G4 candidate inputs at /tmp/cellbox-s10-3d-resume (vanished) and G4 protected
# census == the S10-4.17 record (rewritten by the approved S10-5 promotion).  Current-state equivalents live in s107_preflight.
EXCLUDED_RETAINED = ('test_t7_t11_flow.', '.test_preflight_defines_every_go_no_go_criterion')
MEMORY_MODULES = ['test_memory', 'test_status_select']
OWN_MODULES = ['test_s107_wiring', 'test_s107_surface', 'test_s107_gonogo']
EXPECT = {'retained_original': 144, 'retained_prior_current': 47, 'feed_hook_new': 78, 'memory': 35}
SUMMARY = HERE / 'verification-summary-s10-7.json'
FROZEN = HERE / 'frozen-sources-s10-7.sha256'


def own_sources():
    names = sorted(p.name for p in HERE.glob('*.py')) + ['schedule-b2.json']
    return [HERE / name for name in names]


def memory_sources():
    return sorted(p for p in ENV.MEMORY.glob('*.py'))


def frozen_sources():
    """Retained + feed-hook + memory + new executing sources (every file the gate and the runners execute)."""
    import run_hook as RH
    sources = {str(p): ENV.sha256_file(p) for p in own_sources() + memory_sources()}
    sources.update(RH.frozen_sources())
    return sources


def is_excluded(test):
    ident = test.id()
    return ident.startswith(EXCLUDED_RETAINED[0]) and ident.endswith(EXCLUDED_RETAINED[1])


def without_excluded(suite):
    """(suite without the excluded retained test, [excluded ids]) preserving structure."""
    kept, excluded = unittest.TestSuite(), []
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            inner, gone = without_excluded(item)
            kept.addTest(inner)
            excluded += gone
        elif is_excluded(item):
            excluded.append(item.id())
        else:
            kept.addTest(item)
    return kept, excluded


RETAINED_GLYPH = Path('/tmp/cellbox-phase10-a3hg7abb/host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs')
RETAINED_CAND = Path(ENV.CAND_RETAINED_ROOT)


@contextlib.contextmanager
def retained_paths():
    """Recreate, for the duration of the gate only, the two vanished /tmp paths the retained tests read (symlinks to the sha-verified
    byte-identical copies; nothing is faked, nothing existing is touched; every created path is removed on exit and recorded).

      /tmp/cellbox-s10-3d-resume/cand          -> ~/.cache/cellos-candidates/4fae8b20... (candidate.json sha == CAND_SHA)
      /tmp/cellbox-phase10-a3hg7abb/.../phase04-gui-glyphs -> the offline-rebuilt glyph oracle (sha == retained GLYPH_SHA)
    """
    import s107_prepare as PREP
    created, record = [], {}
    try:
        if ENV.sha256_file(ENV.CAND_CACHE / 'candidate.json') != ENV.CAND_SHA:
            raise ENV.EnvError('candidate cache copy changed')
        glyph = ENV.SETS['cand-desktop'].glyph_bin
        if ENV.sha256_file(glyph) != PREP.GLYPH_SHA:
            raise ENV.EnvError('prepared glyph oracle is not the retained GLYPH_SHA binary')
        for link, target in ((RETAINED_CAND, ENV.CAND_CACHE), (RETAINED_GLYPH, glyph)):
            if link.exists() or link.is_symlink():
                raise ENV.EnvError(f'{link} already exists; refusing to touch it')
            top = link
            while not top.parent.exists():
                top = top.parent
            link.parent.mkdir(parents=True, exist_ok=True)
            link.symlink_to(target)
            created.append(top)
            record[str(link)] = str(target)
        yield record
    finally:
        for top in created:
            if top.is_symlink() or top.is_file():
                top.unlink()
            else:
                shutil.rmtree(top)


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
    link.symlink_to(ENV.RUNTIME / 'observer-recurring')
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
    with retained_paths() as shims:
        def suite(names):
            built = unittest.TestSuite([loader.loadTestsFromName(name) for name in names])
            return built, RG.count(built)

        feed_hook_all, feed_total = suite(RG.NEW_MODULES)
        feed_hook, excluded = without_excluded(feed_hook_all)
        memory, memory_count = suite(MEMORY_MODULES)
        own, own_count = suite(OWN_MODULES)
        retained_kept, retained_excluded = without_excluded(retained)
        excluded += retained_excluded
        feed_count = feed_total - len(excluded)
        whole = unittest.TestSuite([retained_kept, feed_hook, memory, own])
        with raw.open('w') as stream, patch.object(module.OLD.P, 'exercise', side_effect=redirected), patch.object(rendered_proof, 'HERE', attempt):
            runner = unittest.TextTestRunner(stream=stream, verbosity=2, resultclass=RG.Recorder)
            result = runner.run(whole)
    finished = time.time()
    components = {'retained_original': old_count, 'retained_prior_current': retained_count - old_count, 'feed_hook_new': feed_total,
                  'feed_hook_new_executed': feed_count, 'memory': memory_count, 'own_s10_7_4': own_count}
    expected_total = retained_count + feed_count + memory_count + own_count
    clean = (result.wasSuccessful() and not result.skipped and not result.expectedFailures and not result.unexpectedSuccesses
             and result.testsRun == expected_total and all(components[k] == v for k, v in EXPECT.items()) and own_count > 0
             and len(excluded) == 1 and excluded[0].endswith(EXCLUDED_RETAINED[1]))
    receipt = {
        'attempt': number, 'status': 'PASS' if clean else 'FAIL', 'single_invocation': True, 'tests_run': result.testsRun,
        'components': components, 'retained_gate_total': components['retained_original'] + components['retained_prior_current'] + components['feed_hook_new'],
        'retained_gate_executed': components['retained_original'] + components['retained_prior_current'] + components['feed_hook_new_executed'],
        'excluded_retained_tests': excluded, 'excluded_reason': 'state-dependent S10-4.17 pre-run preflight test; see EXCLUDED_RETAINED in s107_gate.py',
        'temporary_retained_path_links': shims,
        'failures': len(result.failures), 'errors': len(result.errors), 'skipped': len(result.skipped),
        'expected_failures': len(result.expectedFailures), 'unexpected_successes': len(result.unexpectedSuccesses),
        'argv': sys.argv, 'cwd': os.getcwd(), 'python': platform.python_version(),
        'env_PYTHONDONTWRITEBYTECODE': os.environ.get('PYTHONDONTWRITEBYTECODE'),
        'env_sha256': hashlib.sha256(json.dumps(sorted(os.environ.items())).encode()).hexdigest(),
        'start': started, 'end': finished, 'duration_seconds': finished - started,
        'modules': {'feed_hook_new': RG.NEW_MODULES, 'memory': MEMORY_MODULES, 'own': OWN_MODULES},
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
        summary['method'] = ('ONE unittest invocation: retained S10-4.17 gate (144 + 47 + 78 = 269) + 35 observer-memory tests + new S10-7.4 tests, '
                             'zero failures/errors/skips; retained synthetic outputs redirected to this attempt directory, prior archives untouched')
        SUMMARY.write_text(json.dumps(summary, indent=2) + '\n')
        FROZEN.write_text(json.dumps(summary['sources_sha256'], indent=2, sort_keys=True) + '\n')
        return 0
    return 1


if __name__ == '__main__':
    raise SystemExit(main())
