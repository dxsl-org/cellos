#!/usr/bin/env python3
"""S10-7.4 go/no-go (offline; never starts QEMU, never claims a run token).

    PYTHONDONTWRITEBYTECODE=1 python3 -B s107_preflight.py [--write] [SET ...]

Checks (each `{ok, detail}`):
  frozen_sources           every existing runtime file in the S10-4.17 `frozen-sources.sha256` record is byte-identical
  prior_archives           every prior runtime archive tree hash equals `previous-archives-before.json`
  protected_census         the 546-path census (same method as `X.protected_paths`: {path: {size, sha256}}) vs the
                           S10-4.17 `preservation-after.json`.  Differences are accepted ONLY when each is explained by the
                           approved S10-5 promotion record or by the vanished `/tmp/cellbox-s10-3d-resume/cand` copy whose
                           byte-identical twin sits in `~/.cache/cellos-candidates/<sha>`; `exact_unchanged` is reported
  <set>: kernels_isos      kernel/control kernel/ISO/init/kernel_fs.img sha256 == test-hooks manifest
  <set>: hooks_provenance  provenance.kernel_features contains test-hooks, kernel sha256 == manifest, kernel .symtab carries
                           test_hook_prime_watermark and the per-CPU symbols, control kernel does not
  <set>: run_token         RUN_CONSUMED-<set> absent, no measurement-runs/<set> evidence dir
  <set>: private_io        argv: unix sockets only under the work dir, no hostfwd/-nic/-net/-vnc/tcp, user netdev restrict=on
  <set>: inputs            ELFs/fixture disk/glyph oracle/B2 tool windows present and hash-checked
  gate                     the S10-7.4 single clean invocation record is PASS and the new sources are frozen
"""
import json
import os
import shutil
import sys
import tempfile
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s107_env as ENV  # noqa: E402

PRIOR_FROZEN = ENV.FEED_HOOK / 'frozen-sources.sha256'
PRIOR_ARCHIVES = ENV.FEED_HOOK / 'previous-archives-before.json'
PRIOR_CENSUS = ENV.FEED_HOOK / 'preservation-after.json'
PROMOTION = ENV.REPORTS / 's10-5-evidence/promotion.json'
GATE_SUMMARY = HERE / 'verification-summary-s10-7.json'
NEW_FROZEN = HERE / 'frozen-sources-s10-7.sha256'


def tree_hashes(root):
    """Retained `A.tree_hashes` semantics (all files, relative path -> sha256), computed without importing the stack."""
    root = Path(root)
    return {str(p.relative_to(root)): ENV.sha256_file(p) for p in sorted(root.rglob('*'))
            if p.is_file() and '__pycache__' not in p.parts}


def check_frozen_sources(record_path=PRIOR_FROZEN):
    frozen = json.loads(Path(record_path).read_text())
    current = {path: (ENV.sha256_file(path) if Path(path).is_file() else None) for path in frozen}
    drift = sorted(path for path in frozen if frozen[path] != current[path])
    return {'ok': bool(frozen) and not drift, 'detail': {'files': len(frozen), 'drift': drift}}


def check_prior_archives(record_path=PRIOR_ARCHIVES):
    before = json.loads(Path(record_path).read_text())
    changed = []
    for name, tree in before.items():
        now = tree_hashes(ENV.RUNTIME / name)
        if now != tree:
            changed.append({'archive': name, 'added': sorted(set(now) - set(tree))[:5], 'removed': sorted(set(tree) - set(now))[:5],
                            'modified': sorted(k for k in tree if k in now and now[k] != tree[k])[:5]})
    return {'ok': not changed, 'detail': {'archives': len(before), 'changed': changed}}


def census_now(paths):
    """`X.protected_paths()` method: {path: {size, sha256}}; absent paths are returned separately (the retained one would raise)."""
    present, missing = {}, []
    for name in paths:
        path = Path(name)
        if path.is_file():
            present[name] = {'size': path.stat().st_size, 'sha256': ENV.sha256_file(path)}
        else:
            missing.append(name)
    return present, missing


def compare_census(prior, now, missing, promotion, cache_root=ENV.CAND_CACHE):
    """Classify every difference vs the S10-4.17 record; only declared S10-5/relocation differences are explained."""
    promoted = {str(ENV.REPO / rel): item for rel, item in promotion['promoted'].items()}
    result = {'exact_unchanged': now == prior and not missing, 'paths': len(prior), 'promoted_expected': [], 'relocated': [],
              'unexplained_changed': [], 'unexplained_missing': []}
    for name, record in prior.items():
        if name in missing:
            twin = cache_root / name[len(ENV.CAND_RETAINED_ROOT) + 1:] if name.startswith(ENV.CAND_RETAINED_ROOT + '/') else None
            if twin is not None and twin.is_file() and ENV.sha256_file(twin) == record['sha256'] and twin.stat().st_size == record['size']:
                result['relocated'].append(name)
            else:
                result['unexplained_missing'].append(name)
        elif now[name] != record:
            if name in promoted and now[name]['sha256'] == promoted[name]['sha256'] and now[name]['size'] == promoted[name]['bytes']:
                result['promoted_expected'].append(name)
            else:
                result['unexplained_changed'].append(name)
    result['explained_only'] = not result['unexplained_changed'] and not result['unexplained_missing']
    return result


def check_census():
    prior = json.loads(PRIOR_CENSUS.read_text())
    now, missing = census_now(prior)
    comparison = compare_census(prior, now, missing, json.loads(PROMOTION.read_text()))
    ok = comparison['explained_only'] and len(prior) == 546
    comparison['note'] = ('exact_unchanged is false because the approved S10-5 promotion (after the S10-4.17 record) rewrote the '
                          'listed canonical paths and the /tmp candidate copy no longer exists; every difference is matched to '
                          'promotion.json / the byte-identical ~/.cache candidate twin' if not comparison['exact_unchanged'] else None)
    return {'ok': ok, 'detail': comparison}


# ------------------------------------------------------------------ per-set checks
def check_set_files(name):
    record = ENV.verify_set_files(name)
    return {'ok': record['all_match'], 'detail': record}


def check_hooks_provenance(name):
    spec = ENV.SETS[name]
    detail = {}
    try:
        provenance = ENV.provenance(name)
        detail['kernel_features'] = provenance['kernel_features']
        detail['kernel_sha256'] = provenance['kernel_sha256']
        detail['features_contain_test_hooks'] = 'test-hooks' in provenance['kernel_features']
        detail['kernel_sha_matches_file'] = ENV.sha256_file(spec.kernel) == provenance['kernel_sha256']
        detail['kernel_has_test_hook_symbol'] = ENV.kernel_has_test_hooks(spec.kernel)
        detail['control_kernel_has_test_hook_symbol'] = ENV.kernel_has_test_hooks(spec.control_kernel)
        import s107_memory as MEM  # noqa: E402
        symbols = MEM.KernelSymbols(spec.kernel)
        detail['cpu_locals_va'], detail['tss_tables_va'] = hex(symbols.cpu_locals), hex(symbols.tables)
        ok = (detail['features_contain_test_hooks'] and detail['kernel_sha_matches_file'] and detail['kernel_has_test_hook_symbol']
              and not detail['control_kernel_has_test_hook_symbol'])
    except Exception as error:  # noqa: BLE001 - recorded as the no-go reason
        detail['error'] = f'{type(error).__name__}: {error}'
        ok = False
    return {'ok': ok, 'detail': detail}


def check_run_token(name):
    evidence = ENV.RUN_EVIDENCE / name
    token = ENV.SETS[name].token
    return {'ok': not token.exists() and not evidence.exists(),
            'detail': {'token': str(token), 'consumed': token.exists(), 'evidence_dir_exists': evidence.exists()}}


FORBIDDEN_ARGV = ('hostfwd', '-nic', '-net', '-vnc', 'tcp:', 'tcp,', 'telnet', 'websocket', '-enable-kvm', '-smp', '-snapshot')


def argv_private_violations(argv, work):
    """Violations of the private-I/O rules in an argv; `work` is the run directory all sockets must live under."""
    bad = []
    text = ' '.join(argv)
    for item in FORBIDDEN_ARGV:
        if item in text.split() or (item in ('hostfwd', 'tcp:', 'tcp,', 'telnet', 'websocket') and item in text):
            bad.append(f'forbidden argv token {item!r}')
    for index, item in enumerate(argv):
        if 'path=' in item or item.startswith('unix:'):
            for part in item.replace('unix:', 'path=').split(','):
                if part.startswith('path=') and not part[5:].startswith(str(work) + '/sock/'):
                    bad.append(f'socket outside {work}/sock: {part}')
        if item == '-display' and argv[index + 1] != 'none':
            bad.append('display is not none')
        if item == '-netdev' and (not argv[index + 1].startswith('user,') or 'restrict=on' not in argv[index + 1]):
            bad.append(f'netdev not a restricted user netdev: {argv[index + 1]}')
    if '-gdb' in argv and argv[argv.index('-gdb') + 1] != 'chardev:gdb0':
        bad.append('gdb stub is not the private unix chardev')
    if sum(1 for x in argv if x == '-netdev') != 1:
        bad.append('expected exactly one -netdev')
    return bad


def check_private_io(name):
    import s107_machine as MACH  # noqa: E402
    spec = ENV.SETS[name]
    with tempfile.TemporaryDirectory(prefix='s107-argv-') as scratch:
        argv = MACH.dry_argv(spec, scratch, observed=True, with_data_disk=spec.kind != 'b2')
        argv += ['-D', f'{scratch}/evidence/qemu-trace.log', '-msg', 'timestamp=on']
        bad = argv_private_violations(argv, scratch)
        mode = oct(Path(scratch, 'sock').stat().st_mode & 0o777)
    return {'ok': not bad and mode == '0o700', 'detail': {'violations': bad, 'sock_dir_mode': mode,
                                                        'argv_template': [a.replace(scratch, '$WORK') for a in argv]}}


def check_inputs(name):
    spec = ENV.SETS[name]
    detail, ok = {}, True
    try:
        if spec.kind == 'candidate':
            record, files = ENV.candidate_files(spec.profile)
            detail['candidate_files_checked'] = 0
            for path, item in files.items():
                if ENV.sha256_file(item['file']) != item['sha256']:
                    raise ENV.EnvError(f'candidate file {path} differs from candidate.json')
                detail['candidate_files_checked'] += 1
            detail['fixture_disk'] = {'path': str(spec.fixture_disk), 'exists': spec.fixture_disk.is_file(),
                                      'sha256': ENV.sha256_file(spec.fixture_disk) if spec.fixture_disk.is_file() else None}
            ok &= detail['fixture_disk']['exists']
            import s107_memory as MEM
            detail['heap_arena'] = list(MEM.heap_arena_symbol(files['/bin/cellbox']['file'], kind='cellbox'))
            if spec.profile == 'desktop':
                import s107_prepare as PREP
                glyph = spec.glyph_bin
                detail['glyph_oracle'] = {'path': str(glyph), 'exists': glyph.is_file(),
                                          'sha256': ENV.sha256_file(glyph) if glyph.is_file() else None, 'expected': PREP.GLYPH_SHA}
                ok &= bool(detail['glyph_oracle']['exists'] and detail['glyph_oracle']['sha256'] == PREP.GLYPH_SHA)
        else:
            import s107_b2 as B2
            targets = B2.B2Targets()
            detail['b2_tools'] = {t['name']: {'sha256': t['sha256'][:16], 'windows': len(t['windows']), 'heap_arena': t['heap_arena']}
                                  for t in targets.targets}
            detail['schedule_b2'] = str(HERE / 'schedule-b2.json')
            ok &= (HERE / 'schedule-b2.json').is_file()
    except Exception as error:  # noqa: BLE001
        detail['error'] = f'{type(error).__name__}: {error}'
        ok = False
    return {'ok': bool(ok), 'detail': detail}


def check_gate():
    if not GATE_SUMMARY.exists() or not NEW_FROZEN.exists():
        return {'ok': False, 'detail': {'reason': 'no clean S10-7.4 gate record yet'}}
    summary = json.loads(GATE_SUMMARY.read_text())
    frozen = json.loads(NEW_FROZEN.read_text())
    drift = sorted(p for p, h in frozen.items() if not Path(p).is_file() or ENV.sha256_file(p) != h)
    clean = (summary.get('status') == 'PASS' and summary.get('single_invocation') is True and summary.get('failures') == 0
             and summary.get('errors') == 0 and summary.get('skipped') == 0 and summary.get('expected_failures') == 0
             and summary.get('unexpected_successes') == 0)
    return {'ok': clean and not drift, 'detail': {'tests_run': summary.get('tests_run'), 'status': summary.get('status'),
                                                  'components': summary.get('components'), 'frozen_files': len(frozen), 'drift': drift}}


def check_environment():
    usage = os.statvfs('/tmp')
    free = usage.f_bavail * usage.f_frsize
    return {'ok': free > 4 << 30 and shutil.which('qemu-system-x86_64') is not None,
            'detail': {'tmp_free_bytes': free, 'qemu': shutil.which('qemu-system-x86_64')}}


def check_qemu_identity():
    """Retained same-binary/same-source health (A.retained_health) and input-source binding (C.verify_input_source)."""
    try:
        import s107_retained as RET
        host = json.loads((RET.A.PREVIOUS / 'host-provenance.json').read_text())
        binary = Path(shutil.which('qemu-system-x86_64'))
        health = RET.A.retained_health(host, binary)
        binding = RET.C.verify_input_source(binary)
        return {'ok': bool(health['same_binary'] and binding['verified']), 'detail': {'qemu_sha256': health['qemu_sha256'], 'binding': binding['verified']}}, health, binding
    except Exception as error:  # noqa: BLE001
        return {'ok': False, 'detail': {'error': f'{type(error).__name__}: {error}'}}, None, None


def own_qemu_note():
    """Informational only: other agents may own QEMU processes; this harness only ever kills its own PID."""
    found = []
    for proc in Path('/proc').iterdir():
        if proc.name.isdigit():
            try:
                if (proc / 'comm').read_text().strip().startswith('qemu-system'):
                    found.append(int(proc.name))
            except OSError:
                continue
    return {'foreign_qemu_pids_present': found, 'policy': 'never signalled; a run kills only machine.proc.pid it started'}


def preflight(names=ENV.SET_NAMES):
    checks = {'frozen_sources': check_frozen_sources(), 'prior_archives': check_prior_archives(),
              'protected_census': check_census(), 'gate': check_gate(), 'environment': check_environment()}
    checks['qemu_identity'], health, binding = check_qemu_identity()
    sets = {}
    for name in names:
        sets[name] = {'kernels_isos': check_set_files(name), 'hooks_provenance': check_hooks_provenance(name),
                      'run_token': check_run_token(name), 'private_io': check_private_io(name), 'inputs': check_inputs(name)}
    global_ok = all(item['ok'] for item in checks.values())
    result = {'schema': 'cellos.s10-7.4.go-no-go/1', 'checks': checks, 'sets': sets,
              'go_by_set': {name: global_ok and all(item['ok'] for item in group.values()) for name, group in sets.items()},
              'informational': own_qemu_note(), 'unrunnable_sets': ENV.UNRUNNABLE_SETS}
    result['go'] = all(result['go_by_set'].values())
    return result, health, binding


if __name__ == '__main__':
    names = tuple(a for a in sys.argv[1:] if a in ENV.SET_NAMES) or ENV.SET_NAMES
    outcome, _health, _binding = preflight(names)
    if '--write' in sys.argv:
        (HERE / 'go-no-go.json').write_text(json.dumps(outcome, indent=2) + '\n')
    print(json.dumps({'go': outcome['go'], 'go_by_set': outcome['go_by_set'],
                      'failed': {k: v['detail'] for k, v in outcome['checks'].items() if not v['ok']}}, indent=1)[:4000])
    raise SystemExit(0 if outcome['go'] else 2)
