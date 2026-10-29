#!/usr/bin/env python3
"""S10-4.17 go/no-go preflight and the ONE approved desktop run.

  python3 -B run_hook.py --preflight   # go/no-go only; never starts a VM, never claims the run
  python3 -B run_hook.py --run         # preflight again, then claim RUN_CONSUMED and run once

RUN_CONSUMED is created with O_EXCL before any VM starts and is never removed: a second
invocation, whatever the first outcome, refuses to run.  No retry, headless run or rerun
logic exists in this file by design.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import time
import traceback

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import classify as K
import feed_static as FS
import hook_ledger as HL
import hook_observer as HO
import hook_surface as HS

G, C, F, X, M = HO.G, HO.C, HO.F, HO.X, HO.M
R, A = F.R, F.A
RUNTIME = HERE.parent
OUT = Path('/tmp/cellbox-s10-4-feed-hook-20261029')
DOCS = X.R / '.agents/261020-1750-cellbox-cli'
RUN_CONSUMED = HERE / 'RUN_CONSUMED'
FROZEN = HERE / 'frozen-sources.sha256'
VERIFICATION = HERE / 'verification-summary.json'


def own_sources():
    names = ['feed_static.py', 'hook_ledger.py', 'hook_observer.py', 'hook_surface.py', 'hookfix.py', 'classify.py',
             'run_hook.py', 'run_gate.py', 'static-identity.json']
    names += sorted(p.name for p in HERE.glob('test_*.py'))
    names += sorted(f'fixtures/{p.name}' for p in (HERE / 'fixtures').glob('*') if p.is_file())
    return [HERE / name for name in names]


def retained_sources():
    paths = [G.HERE / name for name in ('run_temporal.py', 'gui_temporal.py', 'temporal.py', 'test_temporal.py',
                                        'test_all.py', 'rendered_proof.py')]
    paths += [F.HERE / name for name in ('run_atomic.py', 'atomic_rsp.py', 'test_atomic.py', 'test_all.py',
                                         'reproduce_before.py')]
    paths += [C.HERE / name for name in ('coordinated.py', 'admission.py', 'test_admission.py', 'test_coordinated.py')]
    paths += [C.PREVIOUS / 'serialized.py', C.PREVIOUS / 'test_serialized.py', C.S.PREVIOUS / 'recurring.py',
              C.S.PREVIOUS / 'test_recurring.py', R.PRIOR / 'amendment.py', A.PREVIOUS / 'diagnostic.py',
              X.RUNTIME / 'rsp.py', X.RUNTIME / 'common.py', X.RUNTIME / 'observer.py',
              X.R / 'scripts/phase10_canonical_drive.py', X.R / 'scripts/phase04_gui_drive.py',
              X.R / 'scripts/phase05_vt_hostile.py']
    return [p for p in paths if p.exists()]


def frozen_sources():
    return {str(p): X.sha(p) for p in own_sources() + retained_sources()}


def prior_archives():
    return sorted(p.name for p in RUNTIME.iterdir() if p.is_dir() and p.name not in ('observer-feed-hook', '__pycache__'))


def archive_hashes():
    return {name: A.tree_hashes(RUNTIME / name) for name in prior_archives()}


def parse_manifest(root):
    entries = {}
    for line in (root / 'evidence.sha256').read_text().splitlines():
        digest, name = line.split('  ', 1)
        entries[name] = digest
    return entries


def stray_qemu():
    found = []
    for proc in Path('/proc').iterdir():
        if proc.name.isdigit():
            try:
                comm = (proc / 'comm').read_text().strip()
            except OSError:
                continue
            if comm.startswith('qemu-system'):
                found.append((proc.name, comm))
    return found


def preflight():
    checks = {}

    def check(name, ok, detail=None):
        checks[name] = {'ok': bool(ok), 'detail': detail}

    proposal = (DOCS / 'phase-10-s0-proposal.md').read_text()
    check('G1_cp1_approval_recorded', 'CP1 APPROVED by user 2026-10-29 exactly as written' in proposal)
    summary = json.loads(VERIFICATION.read_text()) if VERIFICATION.exists() else {}
    check('G2_single_clean_invocation', summary.get('status') == 'PASS' and summary.get('failures') == 0 and
          summary.get('errors') == 0 and summary.get('skipped') == 0 and summary.get('expected_failures') == 0 and
          summary.get('unexpected_successes') == 0 and summary.get('retained_tests') == 144 and
          summary.get('prior_current_tests') == 47 and
          summary.get('tests_run') == 191 + summary.get('new_tests', -1) and summary.get('new_tests', 0) > 0 and
          summary.get('single_invocation') is True,
          {k: summary.get(k) for k in ('status', 'tests_run', 'failures', 'errors', 'skipped')})
    frozen = json.loads(FROZEN.read_text()) if FROZEN.exists() else {}
    current = frozen_sources() if frozen else {}
    drift = sorted(k for k in set(frozen) | set(current) if frozen.get(k) != current.get(k))
    check('G2_frozen_sources_match', bool(frozen) and not drift, {'files': len(frozen), 'drift': drift[:10]})
    saved = json.loads((HERE / 'static-identity.json').read_text())
    fresh = json.loads(json.dumps(FS.derive(write=False)))
    saved.pop('receipts', None)
    fresh.pop('receipts', None)
    check('G3_static_identity_matches_candidate_elf', saved == fresh)
    try:
        profile = M.verify_inputs('desktop')
        inputs_ok = profile['files']['/bin/terminal']['sha256'] == FS.TERMINAL_SHA
    except Exception as error:  # noqa: BLE001 - recorded as the no-go reason
        inputs_ok, profile = False, str(error)
    check('G4_candidate_inputs_and_terminal_sha', X.sha(X.C) == X.CAND_SHA and inputs_ok)
    before = X.protected_paths()
    prior = json.loads((G.HERE / 'preservation-after.json').read_text())
    check('G4_protected_paths_stable_since_s10_4_15', before == prior, {'files': len(before)})
    s15 = parse_manifest(G.HERE)
    # The retained manifest generator skips every file named evidence.sha256 (nested manifests included).
    tree = {k: v for k, v in A.tree_hashes(G.HERE).items() if Path(k).name != 'evidence.sha256'}
    check('G4_prior_archive_manifest_intact', s15 == tree)
    check('G4_compressed_trace_present', X.OLD_TRACE.exists())
    try:
        host = json.loads((A.PREVIOUS / 'host-provenance.json').read_text())
        binary = Path(shutil.which('qemu-system-x86_64'))
        health = A.retained_health(host, binary)
        binding = C.verify_input_source(binary)
        check('G4_qemu_health_and_input_source_binding', health['same_binary'] and binding['verified'])
    except Exception as error:  # noqa: BLE001
        health = binding = None
        check('G4_qemu_health_and_input_source_binding', False, str(error))
    check('G5_run_not_consumed', not RUN_CONSUMED.exists() and not (HERE / 'desktop').exists() and not OUT.exists())
    check('G6_no_stray_qemu', not stray_qemu(), stray_qemu())
    usage = os.statvfs('/tmp')
    check('G6_tmp_space', usage.f_bavail * usage.f_frsize > 4 << 30, usage.f_bavail * usage.f_frsize)
    check('G7_docs_record_approval_before_execution', all(
        'S10-4.17' in (DOCS / name).read_text() and 'APPROVED' in (DOCS / name).read_text()
        for name in ('phase-10-qualification.md', 'plan.md')) and 'S10-4.17 **CP1 APPROVED' in (X.R / 'CHANGELOG.md').read_text())
    result = {'go': all(item['ok'] for item in checks.values()), 'checks': checks, 'ns': time.monotonic_ns(),
              'wall_time': time.time()}
    return result, health, binding, before


def hook_facts(observer, desk):
    hits = observer.ledger.snapshot()
    stdout = sum(h['length'] for h in hits if h['stream'] == 'stdout')
    stderr = sum(h['length'] for h in hits if h['stream'] == 'stderr')
    return {'hits': len(hits), 'bytes': observer.ledger.total_bytes(), 'stdout_bytes': stdout, 'stderr_bytes': stderr,
            'ledger_sha256': hashlib.sha256(b''.join(bytes.fromhex(h['data_hex']) for h in hits)).hexdigest(),
            'violation': observer.ledger.violation, 'identity_gate': observer.identity_gate,
            'disarm': observer.disarm_record, 'hook_failure': observer.hook_failure,
            'terminal_entries': observer.terminal_entries,
            'metric': {k: {kk: vv for kk, vv in v.items() if kk != 'oracle'} | {
                'oracle': {kk: vv for kk, vv in v['oracle'].items() if kk not in ('output_text', 'expected_rows')}}
                for k, v in (desk.metric if desk else {}).items()},
            'metric_complete': bool(desk and desk.metric_complete)}


def chain(error):
    seen = []
    while error is not None and error not in seen:
        seen.append(error)
        error = error.__cause__ or error.__context__
    return seen


def run_desktop(health, binding, static):
    destination = HERE / 'desktop'
    destination.mkdir(mode=0o700, exist_ok=False)
    work = OUT / 'desktop'
    work.mkdir(mode=0o700, exist_ok=False)
    machine = observer = surface = control = gate = None
    result = {'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0, 'mode': 'S10-4.17-feed-hook-temporal-GUI',
              'start_ns': time.monotonic_ns(), 'original_window_seconds': 900, 'command_deadlines_unchanged': True}
    try:
        machine = X.Machine('desktop', work / 'vm', True)
        machine.argv += ['-D', str(machine.work / 'evidence/qemu-trace.log'), '-msg', 'timestamp=on']
        for event in X.BASE_EVENTS:
            machine.argv += ['-trace', f'enable={event}']
        provenance_path = machine.work / 'evidence/input-provenance.json'
        provenance = json.loads(provenance_path.read_text())
        provenance.update(argv=machine.argv, mode=result['mode'])
        X.dump(provenance_path, provenance)
        result['candidate_runs'] = 1
        machine.start()
        machine.qmp.sock.settimeout(5)
        gate = C.InputGate(machine, machine.work / 'evidence/gui-input-journal.jsonl', source_verified=binding['verified'])
        gate.install()
        X.dump(machine.work / 'evidence/qmp-lock-installation.json',
               {'actual_qmp_object_id': id(machine.qmp), 'shared_lock_id': id(gate.lock), 'lock': 'threading.RLock',
                'GUI_cmd_API': 'cmd(name, **args)', 'direct_trace_query_same_lock': True,
                'paused_native_proof_same_lock': True, 'installed_before_observer_GUI': True,
                'source_binding': binding, 'input_budget_seconds': 5, 'max_input_attempts_per_batch': 64,
                'receive': 'unchanged retained AtomicReceiveMixin', 'GUI': 'feed-hook ledger oracle for finite/status only'})
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv, vm_start_ns=machine.start_ns)
        control = C.S.SerializedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = HO.HookObserver(machine, gate, static, machine.work / 'evidence/feed-hook-ledger.jsonl')
        observer.client = F.AtomicClient(observer, control, gate)
        gate.health = observer.check
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = HS.HookSurface(machine, observer, gate)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['streams'] = surface.streams()
        result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:  # noqa: BLE001 - every exit is classified, never retried
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
        for item in chain(error):
            if isinstance(item, HS.HookOracleFailure):
                result['oracle_failure'] = {'cls': item.cls, 'code': item.code, 'reason': item.reason}
                break
        if surface and surface.d:
            try:
                surface.d.retain_failure(error, {'runner': 'single desktop stopped before cleanup'})
            except Exception as retention_error:  # noqa: BLE001
                result['retention_error'] = str(retention_error)
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps)
            if surface.d:
                result.update(gui_temporal_epochs=surface.d.temporal_epochs, gui_failures=surface.d.failure_records)
        if observer:
            try:
                observer.shutdown()
            except Exception as error:  # noqa: BLE001
                result['observer_error'] = f'{type(error).__name__}: {error}'
                result['status'] = 'STOP'
        if gate:
            gate.terminate(result.get('error', 'owned desktop attempt ended'))
            result.update(gui_batches=gate.batches, gate_terminal_reason=gate.terminal_reason,
                          natural_resume_epoch=gate.resume_epoch)
            X.dump(machine.work / 'evidence/gui-input-batches.json', gate.batches)
        if machine:
            try:
                machine.stop()
            except Exception as error:  # noqa: BLE001
                result.update(status='STOP', cleanup_error=f'{type(error).__name__}: {error}')
        desk = surface.d if surface else None
        facts = hook_facts(observer, desk) if observer else {}
        result.update(feed_hook=facts, end_ns=time.monotonic_ns(), workload_complete=result['status'] == 'COMPLETE',
                      decisions=observer.client.decisions if observer else [],
                      native_scope_count=len(control.steps) if control else 0,
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0)
        if machine:
            X.dump(machine.work / 'evidence/feed-hook-summary.json', facts)
        X.dump(work / 'runtime-result.json', result)
        for path in work.iterdir():
            if path.is_file():
                shutil.copy2(path, destination / path.name)
        if machine and (machine.work / 'evidence').exists():
            shutil.copytree(machine.work / 'evidence', destination / 'evidence', dirs_exist_ok=False)
        if machine and (machine.work / 'disk.img').exists():
            (machine.work / 'disk.img').unlink()
        R.manifest(destination)
        print(json.dumps({key: result[key] for key in ('status', 'error', 'native_scope_count',
              'additional_native_packets', 'workload_complete') if key in result}), flush=True)
    return result


def claim_run(path=RUN_CONSUMED):
    """Atomically claim the single approved run; a second claim raises FileExistsError."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    os.write(fd, json.dumps({'claimed_ns': time.monotonic_ns(), 'wall': time.time(), 'pid': os.getpid(),
                             'meaning': 'S10-4.17 approved ONE desktop run; never remove, never rerun'}).encode())
    os.close(fd)


def main(argv):
    if '--preflight' in argv:
        result, _health, _binding, _before = preflight()
        (HERE / 'go-no-go-preflight.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({'go': result['go'], 'failed': [k for k, v in result['checks'].items() if not v['ok']]}))
        return 0 if result['go'] else 2
    if '--run' not in argv:
        print(__doc__)
        return 64
    go, health, binding, before = preflight()
    (HERE / 'go-no-go.json').write_text(json.dumps(go, indent=2) + '\n')
    if not go['go']:
        print(json.dumps({'NO_GO': [k for k, v in go['checks'].items() if not v['ok']]}))
        return 2
    claim_run()  # permanent, BEFORE any VM exists
    OUT.mkdir(mode=0o700, exist_ok=False)
    static = HO.load_static()
    archives = archive_hashes()
    compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    sources = frozen_sources()
    X.dump(HERE / 'preservation-before.json', before)
    X.dump(HERE / 'previous-archives-before.json', archives)
    X.dump(HERE / 'old-compressed-trace-before.json', compressed)
    X.dump(HERE / 'source-binding-verification.json', binding)
    X.dump(HERE / 'host-provenance.json', {'executing_host_sources': sources, 'retained_health': health,
           'input_source_binding': binding, 'static_identity_sha256': X.sha(HERE / 'static-identity.json'),
           'approval': 'S10-4.17 CP1 user2026-10-29; feed-hook ledger oracle for finite/status; ONE desktop; no rerun'})
    X.dump(HERE / 'approval-docs-before.json', {str(path): X.sha(path) for path in
        (DOCS / 'phase-10-s0-proposal.md', DOCS / 'plan.md', DOCS / 'phase-10-qualification.md', X.R / 'CHANGELOG.md')})
    result = None
    try:
        result = run_desktop(health, binding, static)
    finally:
        after = X.protected_paths()
        post_archives = archive_hashes()
        comparison = {'unchanged': before == after, 'files': len(before), 'canonical_files': 469,
                      'changed': [p for p in before if before[p] != after.get(p)],
                      'previous_archives_unchanged': archives == post_archives,
                      'previous_archive_count': len(archives),
                      'old_compressed_trace_unchanged': compressed == {'size': X.OLD_TRACE.stat().st_size,
                                                                       'sha256': X.sha(X.OLD_TRACE)},
                      'host_source_changed': [p for p in sources if sources[p] != X.sha(p)]}
        X.dump(HERE / 'preservation-after.json', after)
        X.dump(HERE / 'preservation-comparison.json', comparison)
        facts = dict((result or {}).get('feed_hook', {}))
        run_facts = {
            'status': (result or {}).get('status'), 'error': (result or {}).get('error'),
            'observer_error': (result or {}).get('observer_error'), 'cleanup_error': (result or {}).get('cleanup_error'),
            'oracle_failure': (result or {}).get('oracle_failure'), 'hook_failure': facts.get('hook_failure'),
            'metric_complete': facts.get('metric_complete', False),
            'preservation_ok': comparison['unchanged'] and comparison['previous_archives_unchanged'] and
            comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed']}
        verdict = K.classify(run_facts)
        summary = {'classification': verdict, 'facts': run_facts, 'desktop': result, 'preservation': comparison,
                   'candidate_sha256': X.CAND_SHA, 'headless_runs': 0, 'desktop_runs': 1, 'rerun_authorized': False,
                   'cbx27_met': False, 'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
                   'resident_bytes': None, 'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None}
        X.dump(HERE / 'summary.json', summary)
        R.manifest(HERE)
        print(json.dumps({'class': verdict, 'preservation': comparison}, indent=2), flush=True)
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
