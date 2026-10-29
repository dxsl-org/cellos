#!/usr/bin/env python3
"""S10-4.10: one global additional native step, then unconditional diagnostic stop."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parent / 'observer-diagnostic'
sys.path.insert(0, str(PREVIOUS))
import diagnostic as X

OUT = Path('/tmp/cellbox-s10-4-step-amendment-corrected-20261029')
ARCHIVE = HERE.parent / 'observer-step-amendment-corrected'
STABLE_FIELDS = ('rip', 'eflags', 'cs', 'cr3', 'rsp', 'instruction_read_address',
                 'instruction_bytes', 'sstepbits', 'sstep', 'tf', 'rf')


class BoundaryStop(RuntimeError):
    """Terminal result, including additional-step success; never a resume request."""


def census(items):
    result = tuple(sorted(tuple(item) for item in items))
    X.need(len(result) == len(set(result)), 'duplicate coverage census entries')
    return result


def events(raw):
    result = []
    for line in raw.decode('ascii', 'strict').splitlines():
        match = re.fullmatch(r'(\d+)@(\d+\.\d+):([a-z_]+) (.*)', line)
        X.need(match is not None, 'incomplete ordered trace')
        result.append({'tid': int(match[1]), 'event': match[3], 'text': match[4]})
    return result


def validate_lifecycle(before, after, bp, wp):
    X.need(isinstance(before, dict) and before == after, 'observer lifecycle proof missing or changed')
    count = before['verified_instance_count']
    X.need(count == len(before['instances']) == len(before['heap_watches']) and
           before['entry_ordinal'] == count, 'incomplete verified-instance lifecycle')
    if not wp:
        X.need(before['phase'] == 'boot' and count == 0 and before['focus'] is None,
               'empty watches not justified by pre-Cellbox lifecycle')
        entries = {(0x100000000 + slot * 32 * 1024 * 1024, 1) for slot in range(512)}
        X.need(entries.issubset(set(bp)), 'pre-Cellbox finite entry coverage missing')
    else:
        X.need(count > 0 and before['focus'] in before['instances'] and
               set(census(before['heap_watches'])).issubset(set(wp)),
               'verified Cellbox heap-watch lifecycle missing')
    return not wp


def admit(first, current, qmp_current, breakpoints, watches, raw, health, host_verified,
          lifecycle_before, lifecycle_after):
    """Pure fail-closed gate; no state mutation, packet send or recovery loop."""
    X.need(host_verified is True, 'installed host semantics identity not verified')
    X.need(first.get('error') == f'RspError: {X.NO_PROGRESS}' and 'capture_error' not in first,
           'not a completely captured original no-progress failure')
    trace = first['trace']
    X.need(trace['complete_ordered_scope'] is True and
           trace.get('enabled_before_after_acknowledged') is True and
           hashlib.sha256(raw).hexdigest() == trace['raw_sha256'], 'trace proof/enable acknowledgment missing or changed')
    before, after = first['before'], first['after']
    X.need(before['rip'] == after['rip'] == current['rip'] == first['addr'], 'RIP changed at admission')
    X.need(all(before[key] == after[key] == current[key] for key in STABLE_FIELDS),
           'context/instruction changed before additional step')
    X.need(not before['tf'] and not before['rf'] and before['sstep'] == '0x7',
           'TF/RF/native-step-mode premise not met')
    X.need(after['stop'] == current['stop'] == 'T05thread:01;' and trace['stop'] == after['stop'],
           'unexpected original step stop')
    cpus = first['qmp_before']['cpus']
    X.need(len(cpus) == 1 and cpus[0]['cpu-index'] == 0 and cpus[0]['target'] == 'x86_64',
           'sole-vCPU premise missing')
    X.need(cpus == first['qmp_after']['cpus'] == qmp_current['cpus'], 'vCPU identity changed')
    X.need(all(value['status']['running'] is False and value['status']['status'] == 'debug'
               for value in (first['qmp_before'], first['qmp_after'], qmp_current)),
           'QMP not stopped at the admitted debug boundary')
    bp = census(first['breakpoints_before'])
    wp = census(first['watches_before'])
    X.need(bp == census(first['breakpoints_after']) == census(breakpoints) and (first['addr'], 1) in bp,
           'current enrollment/execute restoration changed')
    X.need(wp == census(first['watches_after']) == census(watches),
           'write-watch coverage changed')
    empty_expected = validate_lifecycle(lifecycle_before, lifecycle_after, bp, wp)
    X.need(isinstance(health, dict) and health.get('same_binary') is True and
           health.get('source_exact') is True and health.get('successful_scopes', 0) > 0 and
           bool(health.get('evidence_sha256')), 'retained same-binary/source-exact trace health missing')
    parsed = events(raw)
    # Reuse the original framing/mode/thread-stop checker, then add strict proof ordering.
    checked = X.trace_interval(raw)
    X.need(len(checked['tb_entries']) == len(checked['tb_exits']) == 1,
           'proof requires exactly one TB entry and exit')
    command_rows = [(i, row['text'][10:]) for i, row in enumerate(parsed)
                    if row['event'] == 'gdbstub_io_command' and row['text'].startswith('Received: ')]
    controls = [(i, text) for i, text in command_rows if text.startswith(('Z', 'z', 's', 'c'))]
    expected = [f'z1,{first["addr"]:x},1', 's', f'Z1,{first["addr"]:x},1']
    X.need([text for _, text in controls] == expected, 'unexpected/missing controls or guard removal')
    X.need(all(text in expected or re.fullmatch(
        r'p[0-9a-f]+|m[0-9a-f]+,[0-9a-f]+|qqemu\.sstep(?:bits)?', text)
        for _, text in command_rows), 'unexpected non-read packet in proof scope')
    remove_i, step_i, restore_i = [i for i, _ in controls]
    entry_i = next(i for i, row in enumerate(parsed) if row['event'] == 'exec_tb')
    exit_i = next(i for i, row in enumerate(parsed) if row['event'] == 'exec_tb_exit')
    enable_i = next(i for i, row in enumerate(parsed) if row['event'] == 'breakpoint_singlestep'
                    and row['text'] == 'cpu=0 enable=7')
    stop_i = next(i for i, row in enumerate(parsed) if i > step_i and row['event'] == 'gdbstub_io_reply'
                  and row['text'] == 'Sent: T05thread:01;')
    disable_i = next(i for i, row in enumerate(parsed) if row['event'] == 'breakpoint_singlestep'
                     and row['text'] == 'cpu=0 enable=0')
    X.need(remove_i < step_i < enable_i < entry_i < exit_i < stop_i < disable_i < restore_i,
           'unstarted-TB witness order invalid')
    entry = re.fullmatch(r'tb:(0x[0-9a-f]+) pc=(0x[0-9a-f]+)', parsed[entry_i]['text'])
    exited = re.fullmatch(r'tb:(0x[0-9a-f]+) flags=0x3', parsed[exit_i]['text'])
    X.need(entry is not None and exited is not None and entry[1] == exited[1] and
           int(entry[2], 16) == first['addr'], 'same-TB requested-exit proof missing')
    X.need(parsed[entry_i]['tid'] == parsed[exit_i]['tid'] == cpus[0]['thread-id'],
           'execution trace vCPU/TID mismatch')
    command_tid = parsed[step_i]['tid']
    X.need(all(parsed[i]['tid'] == command_tid for i in (remove_i, enable_i, stop_i, disable_i, restore_i)),
           'debugger/control thread mismatch')
    for at, until in ((remove_i, step_i), (restore_i, len(parsed))):
        replies = [row for row in parsed[at + 1:until] if row['event'] == 'gdbstub_io_reply']
        X.need(replies and replies[0]['text'] == 'Sent: OK' and replies[0]['tid'] == command_tid,
               'current breakpoint remove/restore acknowledgment missing')
    return {'admitted': True, 'first_ordinal': first['ordinal'], 'retained_health': health,
            'lifecycle': lifecycle_before, 'empty_watch_expected': empty_expected,
            'raw_sha256': trace['raw_sha256'], 'site': first['addr'],
            'TB': entry[1], 'exit_flag': 3, 'vcpu_tid': cpus[0]['thread-id'],
            'max_additional_steps_globally': 1, 'recurrence_authorized': False}


class AmendedTraceControl(X.TraceControl):
    def end(self, offset):
        result = super().end(offset)
        # The reused begin/end both queried exact enabled states successfully.
        result['enabled_before_after_acknowledged'] = True
        return result


class AmendedClient(X.DiagnosticClient):
    def __init__(self, observer, control):
        super().__init__(observer, control)
        self.extra_used = False
        self.extra_native_packets = 0
        self._additional_active = False
        self.decisions = []
    def _packet(self, command, deadline):
        if command == 's' and self.failed:
            X.need(self.extra_used and self._additional_active and self.extra_native_packets == 0,
                   'additional native packet allowance exhausted or not admitted')
            self.extra_native_packets += 1
            self.control.steps[-1]['additional_native_packet_issued'] = True
        return super()._packet(command, deadline)

    def lifecycle(self):
        instances = self.observer.instances
        return {'phase': self.observer.phase, 'verified_instance_count': len(instances),
                'instances': sorted(instances), 'entry_ordinal': self.observer.entry_ordinal,
                'focus': self.observer.focus,
                'heap_watches': sorted((inst['heap'], 8) for inst in instances.values())}


    def _raw_scope(self, first):
        trace = first['trace']
        with self.control.path.open('rb') as f:
            f.seek(trace['offset'])
            return f.read(trace['end_offset'] - trace['offset'])

    def _additional_once(self, addr, first, admitted_context, admitted_qmp, admitted_lifecycle):
        X.need(self.extra_used is True and self.failed is True, 'additional operation not exclusively admitted')
        record = {'ordinal': len(self.control.steps) + 1, 'addr': addr,
                  'phase': self.observer.phase, 'kind': 'single-approved-additional-step',
                  'first_ordinal': first['ordinal'], 'start_ns': time.monotonic_ns(),
                  'breakpoints_before': sorted(self._hardware_breakpoints),
                  'watches_before': sorted(self.observer.watches)}
        self.control.steps.append(record)
        offset = None
        try:
            record['before'] = self.context()
            record['qmp_before'] = self.control.stopped_context()
            X.need(all(record['before'][key] == admitted_context[key] for key in STABLE_FIELDS)
                   and record['before']['stop'] == admitted_context['stop'] and
                   record['qmp_before'] == admitted_qmp,
                   'fresh additional-step context differs from admitted context')
            X.need(self.lifecycle() == admitted_lifecycle, 'lifecycle changed after admission')
            X.need(census(record['breakpoints_before']) == census(first['breakpoints_after']) and
                   census(record['watches_before']) == census(first['watches_after']),
                   'coverage changed after proof admission')
            offset = self.control.begin()
            # failed remains true: ordinary continuation/stepping stays prohibited.
            # Only this private, single-use call enters the unchanged native guard.
            self._additional_active = True
            try:
                reply = X.RspClient.step_over_breakpoint(self, addr)
            finally:
                self._additional_active = False
            record['returned_stop'] = reply.decode('ascii')
            X.need(reply == b'T05thread:01;', 'unexpected additional-step stop')
            return reply
        except Exception as e:
            record['error'] = f'{type(e).__name__}: {e}'
            raise
        finally:
            try:
                if self._state == 'stopped':
                    record['after'] = self.context()
                    record['qmp_after'] = self.control.stopped_context()
                record['breakpoints_after'] = sorted(self._hardware_breakpoints)
                record['watches_after'] = sorted(self.observer.watches)
                X.need(census(record['breakpoints_before']) == census(record['breakpoints_after']) and
                       census(record['watches_before']) == census(record['watches_after']),
                       'additional-step coverage/restoration failed')
                if offset is not None:
                    record['trace'] = self.control.end(offset)
            except Exception as e:
                record['capture_error'] = f'{type(e).__name__}: {e}'
                raise
            finally:
                record['end_ns'] = time.monotonic_ns()
                X.append(self.observer.m.work / 'evidence/steps.jsonl', record)

    def step_over_breakpoint(self, addr):
        if self.failed or self.extra_used:
            raise X.PolicyError('amendment terminal boundary forbids recurrence or a third step')
        lifecycle_before = self.lifecycle()
        try:
            return super().step_over_breakpoint(addr)
        except X.RspError as first_error:
            if str(first_error) != X.NO_PROGRESS:
                raise
            first = self.control.steps[-1]
            decision = {'first_ordinal': first['ordinal'], 'site': addr,
                        'start_ns': time.monotonic_ns(), 'extra_steps_used': 0}
            self.decisions.append(decision)
            try:
                current = self.context()
                qmp_current = self.control.stopped_context()
                lifecycle_after = self.lifecycle()
                decision.update(lifecycle_before=lifecycle_before, lifecycle_after=lifecycle_after)
                decision['proof'] = admit(first, current, qmp_current,
                                          self._hardware_breakpoints, self.observer.watches,
                                          self._raw_scope(first), self.control.retained_health,
                                          self.control.host_verified, lifecycle_before, lifecycle_after)
                X.need(not self.extra_used, 'global additional-step allowance already consumed')
                self.extra_used = True
                decision['additional_allowance_consumed'] = True
                self._additional_once(addr, first, current, qmp_current, lifecycle_after)
                decision['outcome'] = 'ADDITIONAL_ADVANCED_BOUNDARY_STOP'
            except Exception as e:
                decision['outcome'] = ('ADDITIONAL_FAILED_BOUNDARY_STOP' if self.extra_used else
                                       'PROOF_REJECTED_BOUNDARY_STOP')
                decision['error'] = f'{type(e).__name__}: {e}'
            finally:
                self.failed = True
                decision['extra_steps_used'] = self.extra_native_packets
                decision['end_ns'] = time.monotonic_ns()
                X.append(self.observer.m.work / 'evidence/amendment-decisions.jsonl', decision)
            raise BoundaryStop(decision['outcome']) from first_error


def tree_hashes(root):
    return {str(p.relative_to(root)): X.sha(p) for p in root.rglob('*')
            if p.is_file() and '__pycache__' not in p.parts}


def initial_attempt_hashes():
    result = {'evidence/' + name: value for name, value in tree_hashes(HERE / 'evidence').items()}
    for name in ('summary.json', 'runtime-result.json', 'interpretation.json',
                 'preservation-before.json', 'preservation-after.json', 'preservation-comparison.json',
                 'host-provenance.json', 'initial-host-tests.json',
                 'executed-amendment.py', 'executed-test_amendment.py'):
        result[name] = X.sha(HERE / name)
    return result


def retained_health(host, binary):
    X.need(X.sha(binary) == host['qemu_sha256'], 'known installed QEMU semantic identity changed')
    X.need(all(X.sha(path) == value for path, value in host['host_sources'].items()),
           'retained trace collector source identity changed')
    prior = json.loads((PREVIOUS / 'interpretation.json').read_text())
    X.need(prior['trace_health_successes'] > 0 and all(prior['proof_conditions'].values()),
           'retained positive trace health incomplete')
    hashes = prior['evidence_hashes']
    X.need(all(X.sha(PREVIOUS / path) == value for path, value in hashes.items()),
           'retained trace-health evidence changed')
    return {'same_binary': True, 'source_exact': True,
            'successful_scopes': prior['trace_health_successes'],
            'qemu_sha256': host['qemu_sha256'], 'source_archive': str(PREVIOUS),
            'evidence_sha256': hashes, 'same_site_in_current_boot_required': False}


def main():
    OUT.mkdir(mode=0o700, exist_ok=False)
    ARCHIVE.mkdir(mode=0o700, exist_ok=True)
    initial = initial_attempt_hashes()
    X.dump(OUT / 'initial-failed-admission-before.json', initial)
    for name in ('amendment.py', 'test_amendment.py'):
        X.need(not (ARCHIVE / name).exists(), 'refuse overwriting replacement helper snapshot')
        shutil.copy2(HERE / name, ARCHIVE / name)
    before = X.protected_paths()
    old = tree_hashes(PREVIOUS)
    old_compressed = {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)}
    X.dump(OUT / 'preservation-before.json', before)
    X.dump(OUT / 'previous-diagnostic-before.json', old)
    X.dump(OUT / 'old-compressed-trace-before.json', old_compressed)
    source_paths = [Path(__file__), PREVIOUS / 'diagnostic.py', X.RUNTIME / 'rsp.py',
                    X.RUNTIME / 'common.py', X.RUNTIME / 'observer.py',
                    X.R / 'scripts/phase10_canonical_drive.py', X.R / 'scripts/phase04_gui_drive.py',
                    X.R / 'scripts/phase05_vt_hostile.py']
    source_before = {str(p): X.sha(p) for p in source_paths}
    host = json.loads((PREVIOUS / 'host-provenance.json').read_text())
    binary = Path(shutil.which('qemu-system-x86_64'))
    health = retained_health(host, binary)
    original_sources = json.loads((HERE / 'host-provenance.json').read_text())['executing_host_sources']
    X.need(X.sha(HERE / 'executed-amendment.py') == original_sources[str(Path(__file__))],
           'initial executed helper snapshot identity mismatch')
    X.dump(OUT / 'retained-health-verification.json', health)
    X.need(X.sha(X.C) == X.CAND_SHA, 'qualified candidate identity changed')
    X.dump(OUT / 'host-provenance.json', {'qemu': host, 'executing_host_sources': source_before,
                                        'retained_health': health,
                                        'approval': 'S10-4.10 CP1; Main/user2026-10-29 host-admission correction and one replacement; single global extra step, no recurrence'})
    machine = observer = surface = control = None
    record = {'status': 'STARTING', 'candidate_runs': 0, 'profile': 'headless',
              'mode': 'S10-4.10-corrected-replacement-diagnostic', 'start_ns': time.monotonic_ns(),
              'global_additional_step_limit': 1, 'recurrence_authorized': False}
    try:
        machine = X.Machine('headless', OUT / 'vm', True)
        machine.argv += ['-D', str(machine.work / 'evidence/qemu-trace.log'), '-msg', 'timestamp=on']
        for event in X.BASE_EVENTS:
            machine.argv += ['-trace', f'enable={event}']
        provenance = json.loads((machine.work / 'evidence/input-provenance.json').read_text())
        provenance.update(argv=machine.argv, mode=record['mode'])
        X.dump(machine.work / 'evidence/input-provenance.json', provenance)
        record['candidate_runs'] = 1
        machine.start()
        machine.qmp.sock.settimeout(5)
        record.update(qemu_pid=machine.proc.pid, argv=machine.argv)
        control = AmendedTraceControl(machine)
        control.host_verified = True
        control.retained_health = health
        observer = X.Observer(machine)
        observer.client = AmendedClient(observer, control)
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = X.Surface(machine, observer)
        surface.sh = X.CheckedShell(machine, surface.check)
        surface.ready()
        surface.sample_idle('ordinary-prompt-idle')
        surface.streams()
        surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        record['status'] = 'COMPLETE_WITHOUT_AMENDMENT'
    except Exception as e:
        record.update(status='STOP', error=f'{type(e).__name__}: {e}', traceback=traceback.format_exc())
    finally:
        if surface:
            record['surface_events'] = surface.events
        if observer:
            try:
                observer.shutdown()
            except Exception as e:
                record.update(status='STOP', observer_error=f'{type(e).__name__}: {e}')
        if machine:
            try:
                machine.stop()
            except Exception as e:
                record.update(status='STOP', cleanup_error=f'{type(e).__name__}: {e}')
        record['end_ns'] = time.monotonic_ns()
        X.dump(OUT / 'runtime-result.json', record)
        after = X.protected_paths()
        X.dump(OUT / 'preservation-after.json', after)
        comparison = {'unchanged': before == after, 'files': len(before), 'canonical_files': 469,
                      'changed': [name for name in before if before[name] != after.get(name)],
                      'previous_diagnostic_unchanged': old == tree_hashes(PREVIOUS),
                      'initial_failed_admission_unchanged': initial == initial_attempt_hashes(),
                      'old_compressed_trace_unchanged': old_compressed ==
                          {'size': X.OLD_TRACE.stat().st_size, 'sha256': X.sha(X.OLD_TRACE)},
                      'host_source_changed': [str(p) for p in source_paths if source_before[str(p)] != X.sha(p)]}
        X.dump(OUT / 'preservation-comparison.json', comparison)
        decisions = observer.client.decisions if observer else []
        result = {'status': decisions[-1]['outcome'] if decisions else 'INCONCLUSIVE_NO_ADMITTED_BOUNDARY',
                  'runtime': record, 'decisions': decisions, 'native_scope_count': len(control.steps) if control else 0,
                  'extra_steps_used': sum(d['extra_steps_used'] for d in decisions),
                  'recurrence_authorized': False, 'full_workload_qualification': False,
                  'cbx27_met': False, 'phase10_criterion4_met': False,
                  'preservation': comparison, 'candidate_sha256': X.CAND_SHA}
        X.dump(OUT / 'summary.json', result)
        for p in OUT.iterdir():
            if p.is_file():
                shutil.copy2(p, ARCHIVE / p.name)
        if machine and (machine.work / 'evidence').exists():
            shutil.copytree(machine.work / 'evidence', ARCHIVE / 'evidence', dirs_exist_ok=False)
        if machine and (machine.work / 'disk.img').exists():
            (machine.work / 'disk.img').unlink()
        hashes = {str(p.relative_to(ARCHIVE)): X.sha(p) for p in ARCHIVE.rglob('*')
                  if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts}
        (ARCHIVE / 'evidence.sha256').write_text(''.join(f'{value}  {name}\n' for name, value in sorted(hashes.items())))
        print(json.dumps({key: result[key] for key in ('status', 'native_scope_count', 'extra_steps_used', 'preservation')}, indent=2), flush=True)
        X.need(comparison['unchanged'] and comparison['previous_diagnostic_unchanged'] and
               comparison['initial_failed_admission_unchanged'] and
               comparison['old_compressed_trace_unchanged'] and not comparison['host_source_changed'],
               'protected previous/current bytes changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
