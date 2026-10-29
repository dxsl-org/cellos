#!/usr/bin/env python3
"""S10-4.11 approved per-event conditional recovery; immutable original schedule."""
import json
from pathlib import Path
import shutil
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
PRIOR = HERE.parent / 'observer-step-amendment-corrected'
sys.path.insert(0, str(PRIOR))
import amendment as A
X = A.X
OUT = Path('/tmp/cellbox-s10-4-observer-recurring-20261029')
PREVIOUS_ARCHIVES = ('observer-diagnostic', 'observer-step-amendment',
                     'observer-step-amendment-corrected')


class RecurringClient(A.AmendedClient):
    def __init__(self, observer, control):
        super().__init__(observer, control)
        self.total_extra_native_packets = 0

    def _packet(self, command, deadline):
        if command == 'c' and self.failed:
            raise X.PolicyError('terminal event forbids further continuation')
        if command == 's' and self.failed:
            X.need(self.extra_used and self._additional_active and self.extra_native_packets == 0,
                   'per-event single additional packet not exclusively admitted')
            self.extra_native_packets += 1
            self.total_extra_native_packets += 1
            self.control.steps[-1]['additional_native_packet_issued'] = True
        return X.RspClient._packet(self, command, deadline)

    def step_over_breakpoint(self, addr):
        if self.failed:
            raise X.PolicyError('terminal event forbids recurrence or a third step')
        lifecycle_before = self.lifecycle()
        try:
            return X.DiagnosticClient.step_over_breakpoint(self, addr)
        except X.RspError as first_error:
            if str(first_error) != X.NO_PROGRESS:
                raise
            first = self.control.steps[-1]
            self.extra_used = False
            self.extra_native_packets = 0
            decision = {'event_id': len(self.decisions) + 1, 'first_ordinal': first['ordinal'],
                        'site': addr, 'start_ns': time.monotonic_ns(), 'extra_steps_used': 0,
                        'recurrence_authorized': True, 'max_additional_steps_per_event': 1}
            self.decisions.append(decision)
            succeeded = False
            try:
                current = self.context()
                qmp_current = self.control.stopped_context()
                lifecycle_after = self.lifecycle()
                decision.update(lifecycle_before=lifecycle_before, lifecycle_after=lifecycle_after)
                proof = A.admit(first, current, qmp_current,
                                self._hardware_breakpoints, self.observer.watches,
                                self._raw_scope(first), self.control.retained_health,
                                self.control.host_verified, lifecycle_before, lifecycle_after)
                # The immutable S10-4.10 pure proof checker contains its old policy metadata.
                # Only this newly approved caller replaces that metadata; its proof is unchanged.
                del proof['max_additional_steps_globally']
                proof.update(max_additional_steps_per_event=1, recurrence_authorized=True)
                decision['proof'] = proof
                self.extra_used = True
                decision['additional_allowance_consumed'] = True
                reply = self._additional_once(addr, first, current, qmp_current, lifecycle_after)
                additional = self.control.steps[-1]
                X.need(additional['after']['rip'] != addr and
                       additional['after']['stop'] == 'T05thread:01;' and
                       additional['qmp_after']['cpus'] == qmp_current['cpus'] and
                       all(additional['after'][key] == current[key]
                           for key in ('cs', 'cr3', 'sstepbits', 'sstep')) and
                       additional['trace']['complete_ordered_scope'] is True and
                       additional['trace']['enabled_before_after_acknowledged'] is True,
                       'additional advancement/context/trace witness missing')
                decision.update(outcome='ADDITIONAL_ADVANCED_CONTINUE',
                                additional_ordinal=additional['ordinal'],
                                after_rip=additional['after']['rip'])
                succeeded = True
            except Exception as error:
                decision['outcome'] = ('ADDITIONAL_FAILED_TERMINAL_STOP' if self.extra_used else
                                       'PROOF_REJECTED_TERMINAL_STOP')
                decision['error'] = f'{type(error).__name__}: {error}'
            finally:
                self.failed = not succeeded
                decision.update(extra_steps_used=self.extra_native_packets,
                                terminal_latch_cleared=succeeded, end_ns=time.monotonic_ns())
                X.append(self.observer.m.work / 'evidence/recurrence-decisions.jsonl', decision)
            if succeeded:
                self.extra_used = False
                return reply
            raise A.BoundaryStop(decision['outcome']) from first_error


def previous_hashes():
    return {name: A.tree_hashes(HERE.parent / name) for name in PREVIOUS_ARCHIVES}


def manifest(root):
    entries = {str(p.relative_to(root)): X.sha(p) for p in root.rglob('*')
               if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts}
    (root / 'evidence.sha256').write_text(''.join(f'{value}  {name}\n'
        for name, value in sorted(entries.items())))


def run_profile(profile, health):
    destination = HERE / profile
    destination.mkdir(mode=0o700, exist_ok=False)
    work = OUT / profile
    work.mkdir(mode=0o700, exist_ok=False)
    machine = observer = surface = control = None
    result = {'profile': profile, 'status': 'STARTING', 'candidate_runs': 0,
              'mode': 'S10-4.11-recurring-conditional-observed', 'start_ns': time.monotonic_ns(),
              'original_window_seconds': 900, 'command_deadlines_unchanged': True}
    try:
        machine = X.Machine(profile, work / 'vm', True)
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
        result.update(qemu_pid=machine.proc.pid, argv=machine.argv)
        control = A.AmendedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = X.Observer(machine)
        observer.client = RecurringClient(observer, control)
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
        manifest(destination)
        print(json.dumps({key: result[key] for key in ('profile', 'status', 'native_scope_count',
              'additional_native_packets', 'workload_complete')}), flush=True)
    return result


def main():
    OUT.mkdir(mode=0o700, exist_ok=False)
    old = previous_hashes()
    before = X.protected_paths()
    compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    source_paths = [Path(__file__), HERE / 'test_recurring.py', PRIOR / 'amendment.py',
                    A.PREVIOUS / 'diagnostic.py', X.RUNTIME / 'rsp.py', X.RUNTIME / 'common.py',
                    X.RUNTIME / 'observer.py', X.R / 'scripts/phase10_canonical_drive.py',
                    X.R / 'scripts/phase04_gui_drive.py', X.R / 'scripts/phase05_vt_hostile.py']
    sources = {str(path): X.sha(path) for path in source_paths}
    host = json.loads((A.PREVIOUS / 'host-provenance.json').read_text())
    health = A.retained_health(host, Path(shutil.which('qemu-system-x86_64')))
    X.need(X.sha(X.C) == X.CAND_SHA, 'qualified candidate identity changed')
    X.dump(HERE / 'preservation-before.json', before)
    X.dump(HERE / 'previous-archives-before.json', old)
    X.dump(HERE / 'old-compressed-trace-before.json', compressed)
    X.dump(HERE / 'host-provenance.json', {'qemu': host, 'executing_host_sources': sources,
           'retained_health': health, 'approval': 'S10-4.11 CP1 user2026-10-29; per-event one additional, successful continuation; one headless and one desktop original schedule/deadlines'})
    docs = X.R / '.agents/261020-1750-cellbox-cli'
    X.dump(HERE / 'approval-docs-before.json', {str(path): X.sha(path) for path in
        (docs / 'phase-10-s0-proposal.md', docs / 'plan.md', docs / 'phase-10-qualification.md', X.R / 'CHANGELOG.md')})
    results = []
    try:
        for profile in ('headless', 'desktop'):
            results.append(run_profile(profile, health))
            X.need(before == X.protected_paths() and old == previous_hashes() and
                   all(X.sha(path) == sources[str(path)] for path in source_paths),
                   'protected/prior/executing source drift; no next profile authorized')
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
        summary = {'status': 'COMPLETE_OBSERVED_SCHEDULES' if len(results) == 2 and
                   all(result['workload_complete'] for result in results) else 'PARTIAL_OBSERVED_RESULTS',
                   'profiles': results, 'preservation': comparison,
                   'candidate_sha256': X.CAND_SHA, 'cbx27_met': False,
                   'phase10_criterion4_met': False, 'historical_runtime_baseline': None,
                   'resident_bytes': None, 'exact_stack_peak_bytes': None, 'kernel_stack_peak_bytes': None}
        X.dump(HERE / 'summary.json', summary)
        manifest(HERE)
        print(json.dumps({'status': summary['status'], 'preservation': comparison}, indent=2), flush=True)
        X.need(comparison['unchanged'] and comparison['previous_archives_unchanged'] and
               comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed'],
               'protected previous/current bytes changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
