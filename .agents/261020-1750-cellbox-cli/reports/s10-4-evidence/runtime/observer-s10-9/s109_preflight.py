#!/usr/bin/env python3
"""S10-9.2 go/no-go (offline; never starts QEMU, never claims a run token).

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109_preflight.py [--write] [SET ...]

Reuses the retained S10-7 checks unchanged (frozen retained sources, prior archives, 546-path protected census, per-set
kernel/ISO/init hashes, test-hooks provenance, private-I/O argv, inputs, environment, QEMU identity) and replaces only what is
S10-9 specific:

  frozen_s107_sources   every executing source frozen by the S10-7 gate is byte-identical
  frozen_s108_sources   every executing source frozen by the S10-8 gate (observer-s10-8 + s10-7 + memory + retained) is byte-identical
  prior_run_evidence    the S10-7/S10-8 run and harness evidence manifests (evidence.sha256) still verify, no unlisted file
  run_token             RUN_CONSUMED-s109-<set> absent and no s10-9-evidence/measurement-runs/<set> dir
  gate                  the S10-9 single clean invocation record is PASS and the S10-9 sources are frozen
  collision_census      s109_census over the new classes is empty
  kernel_hashes         the measurement kernels/ISOs hash-match the S10-7.2 manifest (per set: `kernels_isos`)
"""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_env as ENV  # noqa: E402
import s107_env as E7  # noqa: E402
import s107_preflight as PF7  # noqa: E402

GATE_SUMMARY = HERE / 'verification-summary-s10-9.json'
NEW_FROZEN = HERE / 'frozen-sources-s10-9.sha256'
S107_FROZEN = E7.HERE / 'frozen-sources-s10-7.sha256'
S108_FROZEN = E7.HERE.parent / 'observer-s10-8' / 'frozen-sources-s10-8.sha256'
PRIOR_EVIDENCE_MANIFESTS = (
    'harness', 'measurement-runs/b2-headless', 'measurement-runs/cand-desktop', 'measurement-runs/cand-headless',
)
PRIOR_EVIDENCE_MANIFESTS_S108 = ('harness', 'measurement-runs/cand-desktop', 'measurement-runs/cand-headless')

# Shared with S10-7 unchanged.
tree_hashes = PF7.tree_hashes
check_frozen_sources = PF7.check_frozen_sources
check_prior_archives = PF7.check_prior_archives
census_now = PF7.census_now
compare_census = PF7.compare_census
check_census = PF7.check_census
check_set_files = PF7.check_set_files
check_hooks_provenance = PF7.check_hooks_provenance
check_private_io = PF7.check_private_io
check_inputs = PF7.check_inputs
check_environment = PF7.check_environment
check_qemu_identity = PF7.check_qemu_identity
own_qemu_note = PF7.own_qemu_note
PRIOR_CENSUS = PF7.PRIOR_CENSUS


def check_frozen_s107():
    if not S107_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': f'{S107_FROZEN} missing'}}
    return check_frozen_sources(S107_FROZEN)


def check_frozen_s108():
    if not S108_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': f'{S108_FROZEN} missing'}}
    return check_frozen_sources(S108_FROZEN)


def verify_manifest(root):
    """evidence.sha256 (`<sha256>  <relative path>` lines): every listed file matches, no file is unlisted."""
    root = Path(root)
    manifest = root / 'evidence.sha256'
    if not manifest.is_file():
        return {'root': str(root), 'ok': False, 'reason': 'evidence.sha256 missing'}
    listed, bad = {}, []
    for line in manifest.read_text().splitlines():
        digest, name = line.split('  ', 1)
        listed[name] = digest
        path = root / name
        if not path.is_file() or ENV.sha256_file(path) != digest:
            bad.append(name)
    unlisted = sorted(str(p.relative_to(root)) for p in root.rglob('*')
                      if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts and str(p.relative_to(root)) not in listed)
    return {'root': str(root), 'ok': not bad and not unlisted, 'listed': len(listed), 'modified_or_missing': bad[:5], 'unlisted': unlisted[:5]}


def check_prior_run_evidence():
    results = [verify_manifest(ENV.REPORTS / 's10-7-evidence' / name) for name in PRIOR_EVIDENCE_MANIFESTS]
    results += [verify_manifest(ENV.REPORTS / 's10-8-evidence' / name) for name in PRIOR_EVIDENCE_MANIFESTS_S108]
    return {'ok': all(r['ok'] for r in results), 'detail': {'manifests': len(results), 'results': results}}


def check_run_token(name):
    evidence = ENV.RUN_EVIDENCE / name
    token = ENV.token(name)
    return {'ok': not token.exists() and not evidence.exists(),
            'detail': {'token': str(token), 'consumed': token.exists(), 'evidence_dir_exists': evidence.exists()}}


def check_gate():
    if not GATE_SUMMARY.exists() or not NEW_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': 'no clean S10-9.2 gate record yet'}}
    summary = json.loads(GATE_SUMMARY.read_text())
    frozen = json.loads(NEW_FROZEN.read_text())
    drift = sorted(p for p, h in frozen.items() if not Path(p).is_file() or ENV.sha256_file(p) != h)
    clean = (summary.get('status') == 'PASS' and summary.get('single_invocation') is True and summary.get('failures') == 0
             and summary.get('errors') == 0 and summary.get('skipped') == 0 and summary.get('expected_failures') == 0
             and summary.get('unexpected_successes') == 0)
    return {'ok': clean and not drift, 'detail': {'tests_run': summary.get('tests_run'), 'status': summary.get('status'),
                                                  'components': summary.get('components'), 'frozen_files': len(frozen), 'drift': drift}}


def check_collision_census():
    try:
        import s109_census as CENSUS
        census = CENSUS.build_census()
        return {'ok': bool(census['empty']), 'detail': {'unintended_new': census['unintended_new'],
                                                        'surface_chain_unintended': census['surface_chain_unintended'],
                                                        'new_classes': sorted(census['new_classes'])}}
    except Exception as error:  # noqa: BLE001 - recorded as the no-go reason
        return {'ok': False, 'detail': {'error': f'{type(error).__name__}: {error}'}}


def preflight(names=ENV.SET_NAMES):
    checks = {'frozen_sources': check_frozen_sources(), 'frozen_s107_sources': check_frozen_s107(), 'frozen_s108_sources': check_frozen_s108(),
              'prior_archives': check_prior_archives(), 'prior_run_evidence': check_prior_run_evidence(),
              'protected_census': check_census(), 'gate': check_gate(), 'collision_census': check_collision_census(),
              'environment': check_environment()}
    checks['qemu_identity'], health, binding = check_qemu_identity()
    sets = {}
    for name in names:
        sets[name] = {'kernels_isos': check_set_files(name), 'hooks_provenance': check_hooks_provenance(name),
                      'run_token': check_run_token(name), 'private_io': check_private_io(name), 'inputs': check_inputs(name)}
    global_ok = all(item['ok'] for item in checks.values())
    result = {'schema': 'cellos.s10-9.go-no-go/1', 'checks': checks, 'sets': sets,
              'go_by_set': {name: global_ok and all(item['ok'] for item in group.values()) for name, group in sets.items()},
              'informational': own_qemu_note(), 'not_run_sets': ENV.NOT_RUN_SETS}
    result['go'] = all(result['go_by_set'].values())
    return result, health, binding


if __name__ == '__main__':
    names = tuple(a for a in sys.argv[1:] if a in ENV.SET_NAMES) or ENV.SET_NAMES
    outcome, _health, _binding = preflight(names)
    if '--write' in sys.argv:
        (HERE / 'go-no-go.json').write_text(json.dumps(outcome, indent=2) + '\n')
    print(json.dumps({'go': outcome['go'], 'go_by_set': outcome['go_by_set'],
                      'failed': {k: v['detail'] for k, v in outcome['checks'].items() if not v['ok']},
                      'failed_sets': {n: {k: v['detail'] for k, v in group.items() if not v['ok']} for n, group in outcome['sets'].items()}}, indent=1)[:6000])
    raise SystemExit(0 if outcome['go'] else 2)
