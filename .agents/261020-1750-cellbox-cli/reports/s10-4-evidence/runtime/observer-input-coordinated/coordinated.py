#!/usr/bin/env python3
"""S10-4.13 approved source-backed input admission; one desktop, no extra execution."""
import json
from pathlib import Path
import shutil
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parent / 'observer-qmp-serialized'
ANALYSIS = HERE.parent / 'observer-input-admission-analysis'
sys.path.insert(0, str(PREVIOUS))
import serialized as S
from admission import InputGate, AdmissionError
R, A, X = S.R, S.A, S.X
OUT = Path('/tmp/cellbox-s10-4-input-coordinated-20261029')
PREVIOUS_ARCHIVES = S.PREVIOUS_ARCHIVES + ('observer-qmp-serialized', 'observer-input-admission-analysis')


class CoordinatedClient(R.RecurringClient):
    def __init__(self, observer, control, gate):
        self.gate = gate
        super().__init__(observer, control)

    def resume(self):
        return self.gate.ordinary_resume(lambda: super(CoordinatedClient, self).resume())

    def step_over_breakpoint(self, addr):
        with self.gate.paused():
            return super().step_over_breakpoint(addr)


class CoordinatedObserver(X.Observer):
    def __init__(self, machine, gate):
        self.gate = gate
        super().__init__(machine)

    def _stopped(self, reply):
        with self.gate.paused():
            return super()._stopped(reply)

    def run(self):
        try:
            return super().run()
        finally:
            self.gate.terminate(self.error or 'observer stopped')


class CoordinatedSurface(X.Surface):
    def __init__(self, machine, observer, gate):
        self.gate = gate
        super().__init__(machine, observer)

    def check(self):
        super().check()
        if self.gate.phase == 'TERMINAL':
            raise AdmissionError(self.gate.terminal_reason)

    def ready(self):
        with self.gate.caller_scope(900):
            return super().ready()

    def command(self, name, cmd, timeout=120):
        with self.gate.caller_scope(timeout):
            return super().command(name, cmd, timeout)

    def submit(self, cmd):
        with self.gate.caller_scope(120):
            return super().submit(cmd)


def previous_hashes():
    return {name: A.tree_hashes(HERE.parent / name) for name in PREVIOUS_ARCHIVES}


def verify_input_source(binary):
    proof = json.loads((ANALYSIS / 'rejection-proof.json').read_text())
    identity = proof['installed_identity']
    publication = proof['published_source']
    snapshots = json.loads((ANALYSIS / 'source-snapshots.json').read_text())
    census = json.loads((ANALYSIS / 'patch-census.json').read_text())
    X.need(X.sha(binary) == identity['sha256'] == 'd306a5fcc9e390c2c9faeb58944fada0e4b47780154d02b5465ed4ff9879c972',
           'installed source-bound QEMU binary changed')
    X.need(identity['version'] == '1:8.2.2+ds-0ubuntu1.17' and identity['dpkg_md5_match'] is True and
           publication['active_patch_count'] == census['active_patch_count'] == len(census['patches']) == 128 and
           not publication['input_file_patch_hits'], 'exact package input rejection proof incomplete')
    X.need(all(X.sha(ANALYSIS / row['file']) == row['sha256'] and not row['active_patches_touching_file']
               for row in snapshots), 'source/patch binding changed')
    X.need(X.sha(ANALYSIS / 'source/ui/input.c') == proof['control_flow']['file_sha256'] and
           X.sha(ANALYSIS / 'qemu_8.2.2+ds-0ubuntu1.17.debian.tar.xz') == publication['debian_archive_sha256'],
           'published input source bytes changed')
    return {'verified': True, 'binary_sha256': identity['sha256'], 'version': identity['version'],
            'proof_sha256': X.sha(ANALYSIS / 'rejection-proof.json'),
            'next_protocol_sha256': X.sha(ANALYSIS / 'next-protocol.json'),
            'zero_prefix_scope': 'Only events of the exact structured VM-not-running rejected batch; not earlier accepted batches'}


def run_desktop(health, source_binding):
    destination = HERE / 'desktop'
    destination.mkdir(mode=0o700, exist_ok=False)
    work = OUT / 'desktop'
    work.mkdir(mode=0o700, exist_ok=False)
    machine = observer = surface = control = gate = None
    result = {'profile': 'desktop', 'status': 'STARTING', 'candidate_runs': 0,
              'mode': 'S10-4.13-coordinated-exact-batch-input', 'start_ns': time.monotonic_ns(),
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
        gate = InputGate(machine, machine.work / 'evidence/gui-input-journal.jsonl',
                         source_verified=source_binding['verified'])
        gate.install()
        X.dump(machine.work / 'evidence/qmp-lock-installation.json',
               {'actual_qmp_object_id': id(machine.qmp), 'shared_lock_id': id(gate.lock),
                'lock': 'threading.RLock', 'GUI_cmd_API': 'cmd(name, **args)',
                'direct_trace_query_same_lock': True, 'paused_native_proof_same_lock': True,
                'installed_before_observer_GUI': True, 'source_binding': source_binding,
                'input_budget_seconds': 5, 'max_input_attempts_per_batch': 64})
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv, vm_start_ns=machine.start_ns)
        control = S.SerializedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = CoordinatedObserver(machine, gate)
        observer.client = CoordinatedClient(observer, control, gate)
        gate.health = observer.check
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = CoordinatedSurface(machine, observer, gate)
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
        print(json.dumps({key: result[key] for key in ('profile', 'status', 'error',
              'native_scope_count', 'additional_native_packets', 'workload_complete') if key in result}), flush=True)
    return result


def main():
    OUT.mkdir(mode=0o700, exist_ok=False)
    old = previous_hashes()
    before = X.protected_paths()
    compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    source_paths = [HERE / name for name in ('coordinated.py', 'admission.py', 'test_admission.py', 'test_coordinated.py')]
    source_paths += [PREVIOUS / 'serialized.py', S.PREVIOUS / 'recurring.py', R.PRIOR / 'amendment.py',
                     A.PREVIOUS / 'diagnostic.py', X.RUNTIME / 'rsp.py', X.RUNTIME / 'common.py',
                     X.RUNTIME / 'observer.py', X.R / 'scripts/phase10_canonical_drive.py',
                     X.R / 'scripts/phase04_gui_drive.py', X.R / 'scripts/phase05_vt_hostile.py']
    sources = {str(path): X.sha(path) for path in source_paths}
    host = json.loads((A.PREVIOUS / 'host-provenance.json').read_text())
    binary = Path(shutil.which('qemu-system-x86_64'))
    health = A.retained_health(host, binary)
    binding = verify_input_source(binary)
    X.need(X.sha(X.C) == X.CAND_SHA, 'qualified candidate identity changed')
    X.dump(HERE / 'preservation-before.json', before)
    X.dump(HERE / 'previous-archives-before.json', old)
    X.dump(HERE / 'old-compressed-trace-before.json', compressed)
    X.dump(HERE / 'source-binding-verification.json', binding)
    X.dump(HERE / 'host-provenance.json', {'qemu': host, 'executing_host_sources': sources,
           'retained_health': health, 'input_source_binding': binding,
           'approval': 'S10-4.13 CP1 user2026-10-29; coordinated exact-unaccepted-batch admission; max64 in same5s/caller/900 deadline; one fresh desktop only; no extra guest execution'})
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
                      'previous_archives_unchanged': old == previous_hashes(),
                      'old_compressed_trace_unchanged': compressed ==
                          {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)},
                      'host_source_changed': [str(path) for path in source_paths
                                              if sources[str(path)] != X.sha(path)]}
        X.dump(HERE / 'preservation-after.json', after)
        X.dump(HERE / 'preservation-comparison.json', comparison)
        summary = {'status': 'COMPLETE_DESKTOP_OBSERVED_SCHEDULE' if result and result['workload_complete']
                   else 'PARTIAL_DESKTOP_OBSERVED_RESULT', 'desktop': result,
                   'preservation': comparison, 'candidate_sha256': X.CAND_SHA, 'headless_runs': 0,
                   'cbx27_met': False, 'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
                   'resident_bytes': None, 'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None}
        X.dump(HERE / 'summary.json', summary)
        R.manifest(HERE)
        print(json.dumps({'status': summary['status'], 'preservation': comparison}, indent=2), flush=True)
        X.need(comparison['unchanged'] and comparison['previous_archives_unchanged'] and
               comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed'],
               'protected previous/current bytes changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
