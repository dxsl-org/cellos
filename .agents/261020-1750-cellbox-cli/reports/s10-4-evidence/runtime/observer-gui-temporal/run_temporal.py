#!/usr/bin/env python3
"""S10-4.15 one original desktop schedule with conservative temporal GUI proof."""
import json
from pathlib import Path
import shutil
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import gui_temporal as G
F, C, X = G.F, G.C, G.X
R, A = F.R, F.A
OUT = Path('/tmp/cellbox-s10-4-gui-temporal-20261029')
ARCHIVES = F.ARCHIVES + ('observer-rsp-atomic', 'observer-finite-oracle-analysis')


def hashes():
    return {name: A.tree_hashes(HERE.parent / name) for name in ARCHIVES}


def run_desktop(health, binding):
    destination = HERE / 'desktop'
    destination.mkdir(mode=0o700, exist_ok=False)
    work = OUT / 'desktop'
    work.mkdir(mode=0o700, exist_ok=False)
    machine = observer = surface = control = gate = None
    result = {'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0,
              'mode': 'S10-4.15-temporal-GUI-atomic-RSP', 'start_ns': time.monotonic_ns(),
              'original_window_seconds': 900, 'command_deadlines_unchanged': True}
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
               {'actual_qmp_object_id': id(machine.qmp), 'shared_lock_id': id(gate.lock),
                'lock': 'threading.RLock', 'GUI_cmd_API': 'cmd(name, **args)',
                'direct_trace_query_same_lock': True, 'paused_native_proof_same_lock': True,
                'installed_before_observer_GUI': True, 'source_binding': binding,
                'input_budget_seconds': 5, 'max_input_attempts_per_batch': 64,
                'receive': 'unchanged retained AtomicReceiveMixin', 'GUI': 'TemporalLedger exact output, no hidden route'})
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv, vm_start_ns=machine.start_ns)
        control = C.S.SerializedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = C.CoordinatedObserver(machine, gate)
        observer.client = F.AtomicClient(observer, control, gate)
        gate.health = observer.check
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = G.TemporalSurface(machine, observer, gate)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['streams'] = surface.streams()
        result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:
        result.update(status='STOP', error=f'{type(error).__name__}: {error}', traceback=traceback.format_exc())
        if surface and surface.d:
            try:
                surface.d.retain_failure(error, {'runner': 'single desktop stopped before cleanup'})
            except Exception as retention_error:
                result['retention_error'] = str(retention_error)
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps)
            if surface.d:
                result.update(gui_temporal_epochs=surface.d.temporal_epochs, gui_failures=surface.d.failure_records)
        if observer:
            try:
                observer.shutdown()
            except Exception as error:
                result.update(status='STOP', observer_error=f'{type(error).__name__}: {error}')
        if gate:
            gate.terminate(result.get('error', 'owned desktop attempt ended'))
            result.update(gui_batches=gate.batches, gate_terminal_reason=gate.terminal_reason,
                          natural_resume_epoch=gate.resume_epoch)
            X.dump(machine.work / 'evidence/gui-input-batches.json', gate.batches)
        if machine:
            try:
                machine.stop()
            except Exception as error:
                result.update(status='STOP', cleanup_error=f'{type(error).__name__}: {error}')
        result.update(end_ns=time.monotonic_ns(), workload_complete=result['status'] == 'COMPLETE',
                      decisions=observer.client.decisions if observer else [],
                      native_scope_count=len(control.steps) if control else 0,
                      additional_native_packets=observer.client.total_extra_native_packets if observer else 0)
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


def main():
    tests = json.loads((HERE / 'verification-summary.json').read_text())
    proof = json.loads((HERE / 'rendered-temporal-proof.json').read_text())
    X.need(tests['status'] == 'PASS' and tests['retained_tests_passed'] == 144 and
           tests['new_tests_passed'] == 47 and proof['complete_ordered_trace_accepts'] and
           proof['coalesced_or_dropped_trace_rejects'], 'offline full/missed-output observability gate not proved')
    OUT.mkdir(mode=0o700, exist_ok=False)
    previous = hashes()
    before = X.protected_paths()
    compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    paths = [HERE / name for name in ('run_temporal.py', 'gui_temporal.py', 'temporal.py', 'test_temporal.py', 'test_all.py', 'rendered_proof.py')]
    paths += [F.HERE / name for name in ('run_atomic.py', 'atomic_rsp.py')]
    paths += [C.HERE / name for name in ('coordinated.py', 'admission.py')]
    paths += [C.PREVIOUS / 'serialized.py', C.S.PREVIOUS / 'recurring.py', R.PRIOR / 'amendment.py',
              A.PREVIOUS / 'diagnostic.py', X.RUNTIME / 'rsp.py', X.RUNTIME / 'common.py', X.RUNTIME / 'observer.py',
              X.R / 'scripts/phase10_canonical_drive.py', X.R / 'scripts/phase04_gui_drive.py', X.R / 'scripts/phase05_vt_hostile.py']
    sources = {str(path): X.sha(path) for path in paths}
    host = json.loads((A.PREVIOUS / 'host-provenance.json').read_text())
    binary = Path(shutil.which('qemu-system-x86_64'))
    health = A.retained_health(host, binary)
    binding = C.verify_input_source(binary)
    X.dump(HERE / 'preservation-before.json', before)
    X.dump(HERE / 'previous-archives-before.json', previous)
    X.dump(HERE / 'old-compressed-trace-before.json', compressed)
    X.dump(HERE / 'source-binding-verification.json', binding)
    X.dump(HERE / 'host-provenance.json', {'qemu': host, 'executing_host_sources': sources,
           'retained_health': health, 'input_source_binding': binding,
           'approval': 'S10-4.15 CP1 user2026-10-29; temporal2frame/status ledger and all-path timeout retention; one desktop original controls/deadlines only'})
    docs = X.R / '.agents/261020-1750-cellbox-cli'
    X.dump(HERE / 'approval-docs-before.json', {str(path): X.sha(path) for path in
        (docs / 'phase-10-s0-proposal.md', docs / 'plan.md', docs / 'phase-10-qualification.md', X.R / 'CHANGELOG.md')})
    result = None
    try:
        result = run_desktop(health, binding)
    finally:
        after = X.protected_paths()
        comparison = {'unchanged': before == after, 'files': len(before), 'canonical_files': 469,
                      'changed': [path for path in before if before[path] != after.get(path)],
                      'previous_archives_unchanged': previous == hashes(),
                      'old_compressed_trace_unchanged': compressed == {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)},
                      'host_source_changed': [str(path) for path in paths if sources[str(path)] != X.sha(path)]}
        X.dump(HERE / 'preservation-after.json', after)
        X.dump(HERE / 'preservation-comparison.json', comparison)
        summary = {'status': 'COMPLETE_DESKTOP_OBSERVED_SCHEDULE' if result and result['workload_complete'] else
                   'PARTIAL_DESKTOP_OBSERVED_RESULT', 'desktop': result, 'preservation': comparison,
                   'candidate_sha256': X.CAND_SHA, 'headless_runs': 0, 'cbx27_met': False,
                   'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
                   'resident_bytes': None, 'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None}
        X.dump(HERE / 'summary.json', summary)
        R.manifest(HERE)
        print(json.dumps({'status': summary['status'], 'preservation': comparison}, indent=2), flush=True)
        X.need(comparison['unchanged'] and comparison['previous_archives_unchanged'] and
               comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed'], 'protected bytes changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
