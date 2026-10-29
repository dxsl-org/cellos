#!/usr/bin/env python3
"""S10-4.12: shared full-transaction host QMP lock; one desktop replacement."""
import json
from pathlib import Path
import shutil
import sys
import threading
import time
import traceback

HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parent / 'observer-recurring'
sys.path.insert(0, str(PREVIOUS))
import recurring as R
A, X = R.A, R.X
OUT = Path('/tmp/cellbox-s10-4-qmp-serialized-20261029')
PREVIOUS_ARCHIVES = R.PREVIOUS_ARCHIVES + ('observer-recurring',)


def install_serialization(qmp):
    """Keep the actual owner/API so bound keys/buttons/grab also use this cmd."""
    X.need(not hasattr(qmp, '_observer_transaction_lock'), 'QMP serialization already installed')
    lock = threading.RLock()
    original = qmp.cmd

    def cmd(name, **args):
        with lock:
            return original(name, **args)

    qmp._observer_transaction_lock = lock
    qmp.cmd = cmd
    return lock


class SerializedTraceControl(A.AmendedTraceControl):
    def query(self, command, **args):
        # Same lock as the actual GUI owner's cmd, not a separate reader lock.
        # Keep the original direct-query body: trace args contain name, which
        # cannot be forwarded to existing Qmp.cmd(name, **args).
        with self.machine.qmp._observer_transaction_lock:
            return super().query(command, **args)


def previous_hashes():
    return {name: A.tree_hashes(HERE.parent / name) for name in PREVIOUS_ARCHIVES}


def run_desktop(health):
    destination = HERE / 'desktop'
    destination.mkdir(mode=0o700, exist_ok=False)
    work = OUT / 'desktop'
    work.mkdir(mode=0o700, exist_ok=False)
    machine = observer = surface = control = None
    result = {'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0,
              'mode': 'S10-4.12-serialized-QMP-desktop-replacement', 'start_ns': time.monotonic_ns(),
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
        lock = install_serialization(machine.qmp)
        X.dump(machine.work / 'evidence/qmp-lock-installation.json',
               {'actual_qmp_object_id': id(machine.qmp), 'shared_lock_id': id(lock),
                'lock': 'threading.RLock', 'GUI_cmd_API': 'cmd(name, **args)',
                'direct_trace_query_same_lock': True, 'installed_before_observer_GUI': True})
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv)
        control = SerializedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = X.Observer(machine)
        observer.client = R.RecurringClient(observer, control)
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = X.Surface(machine, observer)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        result['ordinary_idle'] = surface.sample_idle('ordinary-prompt-idle')
        result['streams'] = surface.streams()
        result['top_idle'] = surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        result['status'] = 'COMPLETE'
    except Exception as error:
        result.update(status='STOP', error=f'{type(error).__name__}: {error}',
                      traceback=traceback.format_exc())
    finally:
        if surface:
            result.update(surface_events=surface.events, timestamps=surface.timestamps)
        if observer:
            try:
                observer.shutdown()
            except Exception as error:
                result.update(status='STOP', observer_error=f'{type(error).__name__}: {error}')
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
        print(json.dumps({key: result[key] for key in ('profile', 'status', 'error',
              'native_scope_count', 'additional_native_packets', 'workload_complete') if key in result}), flush=True)
    return result


def main():
    OUT.mkdir(mode=0o700, exist_ok=False)
    old = previous_hashes()
    before = X.protected_paths()
    compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    source_paths = [Path(__file__), HERE / 'test_serialized.py', PREVIOUS / 'recurring.py',
                    R.PRIOR / 'amendment.py', A.PREVIOUS / 'diagnostic.py',
                    X.RUNTIME / 'rsp.py', X.RUNTIME / 'common.py', X.RUNTIME / 'observer.py',
                    X.R / 'scripts/phase10_canonical_drive.py', X.R / 'scripts/phase04_gui_drive.py',
                    X.R / 'scripts/phase05_vt_hostile.py']
    sources = {str(path): X.sha(path) for path in source_paths}
    host = json.loads((A.PREVIOUS / 'host-provenance.json').read_text())
    health = A.retained_health(host, Path(shutil.which('qemu-system-x86_64')))
    X.need(X.sha(X.C) == X.CAND_SHA, 'qualified candidate identity changed')
    X.dump(HERE / 'preservation-before.json', before)
    X.dump(HERE / 'previous-archives-before.json', old)
    X.dump(HERE / 'old-compressed-trace-before.json', compressed)
    X.dump(HERE / 'host-provenance.json', {'qemu': host, 'executing_host_sources': sources,
           'retained_health': health, 'approval': 'S10-4.12 CP1 user2026-10-29; shared QMP lock plus exactly one desktop replacement; existing recurring conditional recovery; no headless rerun/deadline increase'})
    docs = X.R / '.agents/261020-1750-cellbox-cli'
    X.dump(HERE / 'approval-docs-before.json', {str(path): X.sha(path) for path in
        (docs / 'phase-10-s0-proposal.md', docs / 'plan.md', docs / 'phase-10-qualification.md', X.R / 'CHANGELOG.md')})
    result = None
    try:
        result = run_desktop(health)
    finally:
        after = X.protected_paths()
        comparison = {'unchanged': before == after, 'files': len(before), 'canonical_files': 469,
                      'changed': [path for path in before if before[path] != after.get(path)],
                      'previous_archives_unchanged': old == previous_hashes(),
                      'old_compressed_trace_unchanged': compressed ==
                          {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)},
                      'host_source_changed': [str(path) for path in source_paths
                                              if sources[str(path)] != X.sha(path)]}
        X.dump(HERE / 'preservation-after.json', after)
        X.dump(HERE / 'preservation-comparison.json', comparison)
        summary = {'status': 'COMPLETE_DESKTOP_OBSERVED_SCHEDULE' if result and result['workload_complete']
                   else 'PARTIAL_DESKTOP_OBSERVED_RESULT', 'desktop': result,
                   'preservation': comparison, 'candidate_sha256': X.CAND_SHA,
                   'headless_runs': 0, 'cbx27_met': False, 'phase10_criterion4_met': False,
                   'historical_runtime_baseline': None, 'resident_bytes': None,
                   'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None}
        X.dump(HERE / 'summary.json', summary)
        R.manifest(HERE)
        print(json.dumps({'status': summary['status'], 'preservation': comparison}, indent=2), flush=True)
        X.need(comparison['unchanged'] and comparison['previous_archives_unchanged'] and
               comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed'],
               'protected previous/current bytes changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
