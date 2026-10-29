#!/usr/bin/env python3
"""User-approved S10-4.9: one immutable headless diagnostic, no step retry."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import traceback

RUNTIME = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(RUNTIME))
from common import C, CAND_SHA, D, Machine, R, Surface, W, dump, need, sha
from observer import Observer
from rsp import PolicyError, RspClient, RspError

OUT = Path('/tmp/cellbox-s10-4-observer-diagnostic-20261028')
ARCHIVE = Path(__file__).resolve().parent
OLD_TRACE = W / 'runtime/failed-hardware-resume-trace.jsonl.gz'
BASE_EVENTS = ('gdbstub_io_command', 'gdbstub_io_reply', 'gdbstub_hit_watchpoint',
               'gdbstub_hit_break', 'breakpoint_singlestep')
EXEC_EVENTS = ('exec_tb', 'exec_tb_exit')
READ_EXTENSIONS = ('qqemu.sstepbits', 'qqemu.sstep')
REGISTERS = ('rip', 'eflags', 'cs', 'cr3', 'rsp')
NO_PROGRESS = 'single native instruction did not advance current breakpoint'


def append(path, value):
    with Path(path).open('a') as f:
        f.write(json.dumps(value) + '\n')


def trace_interval(raw):
    """Describe dispatch only; TB entry and missing exit are not retirement."""
    lines = raw.decode('utf-8', 'strict').splitlines()
    parsed = []
    for line in lines:
        match = re.fullmatch(r'(\d+)@(\d+\.\d+):([a-z_]+) (.*)', line)
        need(match is not None, 'incomplete/unprefixed trace line')
        parsed.append({'tid': int(match[1]), 'time': match[2],
                       'event': match[3], 'text': match[4]})
    starts = [i for i, x in enumerate(parsed)
              if x['event'] == 'gdbstub_io_command' and x['text'] == 'Received: s']
    need(len(starts) == 1, 'trace scope must contain exactly one native s')
    start = starts[0]
    stops = [i for i, x in enumerate(parsed) if i > start and
             x['event'] == 'gdbstub_io_reply' and
             re.match(r'Sent: [TS][0-9a-fA-F]{2}', x['text'])]
    need(len(stops) == 1, 'trace scope must contain exactly one following stop')
    stop = stops[0]
    between = parsed[start + 1:stop]
    entries = [x for x in between if x['event'] == 'exec_tb']
    exits = [x for x in between if x['event'] == 'exec_tb_exit']
    enabled = [x for x in between if x['event'] == 'breakpoint_singlestep'
               and re.fullmatch(r'cpu=0 enable=7', x['text'])]
    need(len(enabled) == 1, 'native single-step mode7 not witnessed on CPU0')
    disabled = [x for x in parsed[stop + 1:] if x['event'] == 'breakpoint_singlestep'
                and x['text'] == 'cpu=0 enable=0']
    need(len(disabled) == 1, 'single-step disable not witnessed after stop')
    need(parsed[start]['tid'] == parsed[stop]['tid'], 'gdbstub thread changed within scope')
    return {'complete_ordered_scope': True, 'line_count': len(lines),
            'command_tid': parsed[start]['tid'], 'stop': parsed[stop]['text'][6:],
            'tb_entries': entries, 'tb_exits': exits,
            'zero_tb_dispatch_observed': not entries,
            'retirement_proven_by_trace_alone': False,
            'interpretation': ('No TB dispatch between s and stop; zero guest TB execution '
                               'only if source-path/trace-health prerequisites hold.' if not entries else
                               'TB dispatch observed, not affirmative instruction-retirement proof.')}


class TraceControl:
    def __init__(self, machine):
        self.machine = machine
        self.path = machine.work / 'evidence/qemu-trace.log'
        self.log = machine.work / 'evidence/qmp-queries.jsonl'
        self.steps = []
        self.cpu = None

    def check_budget(self):
        need(time.monotonic_ns() - self.machine.start_ns < 900_000_000_000,
             'original900s diagnostic deadline expired')

    def query(self, command, **args):
        allowed = command in ('query-status', 'query-cpus-fast') and not args
        allowed |= (command == 'trace-event-get-state' and set(args) == {'name'}
                    and args['name'] in BASE_EVENTS + EXEC_EVENTS)
        allowed |= (command == 'trace-event-set-state' and set(args) == {'name', 'enable'}
                    and args['name'] in EXEC_EVENTS and type(args['enable']) is bool)
        if not allowed:
            raise PolicyError(f'QMP diagnostic command not allowed: {command} {args!r}')
        self.check_budget()
        request = {'execute': command}
        if args:
            request['arguments'] = args
        append(self.log, {'event': 'request', 'monotonic_ns': time.monotonic_ns(),
                          'message': request})
        # Reuse the connected/capability-negotiated transport. Its cmd(name, **args)
        # signature cannot represent trace commands' own "name" argument.
        self.machine.qmp.file.write(json.dumps(request).encode() + b'\n')
        while True:
            line = self.machine.qmp.file.readline()
            need(bool(line), 'QMP diagnostic transport closed')
            reply = json.loads(line)
            append(self.log, {'event': 'response', 'monotonic_ns': time.monotonic_ns(),
                              'message': reply})
            need('error' not in reply, f'QMP diagnostic error: {reply!r}')
            if 'return' in reply:
                return reply['return']

    def stopped_context(self):
        cpus = self.query('query-cpus-fast')
        status = self.query('query-status')
        need(len(cpus) == 1 and cpus[0]['cpu-index'] == 0 and
             cpus[0]['target'] == 'x86_64' and cpus[0]['thread-id'] > 0,
             'runtime CPU census is not one x86_64 vCPU')
        need(status['running'] is False, 'QMP context is not stopped')
        if self.cpu is None:
            self.cpu = cpus[0]
        need(cpus[0] == self.cpu, 'vCPU identity/topology changed')
        return {'cpus': cpus, 'status': status}

    def state(self, event, expected):
        reply = self.query('trace-event-get-state', name=event)
        need(len(reply) == 1 and reply[0]['name'] == event and
             reply[0]['state'] == expected, f'trace event {event} not {expected}')
        return reply

    def preflight(self):
        result = self.stopped_context()
        for event in BASE_EVENTS:
            self.state(event, 'enabled')
        for event in EXEC_EVENTS:
            self.state(event, 'disabled')
        return result

    def begin(self):
        for event in EXEC_EVENTS:
            self.query('trace-event-set-state', name=event, enable=True)
            self.state(event, 'enabled')
        return self.path.stat().st_size

    def end(self, offset):
        for event in EXEC_EVENTS:
            self.state(event, 'enabled')
        end = self.path.stat().st_size
        for event in EXEC_EVENTS:
            self.query('trace-event-set-state', name=event, enable=False)
            self.state(event, 'disabled')
        with self.path.open('rb') as f:
            f.seek(offset)
            raw = f.read(end - offset)
        result = trace_interval(raw)
        need(self.cpu is not None and all(x['tid'] == self.cpu['thread-id']
                 for x in result['tb_entries'] + result['tb_exits']),
             'TB trace host thread does not match QMP vCPU')
        result.update(offset=offset, end_offset=end,
                      raw_sha256=hashlib.sha256(raw).hexdigest())
        return result


class DiagnosticClient(RspClient):
    def __init__(self, observer, control):
        self.observer = observer
        self.control = control
        self.failed = False
        super().__init__(str(observer.m.gdb_path),
                         str(observer.m.work / 'evidence/rsp-traffic.jsonl'), timeout=5)

    @staticmethod
    def _validate(command):
        if command not in READ_EXTENSIONS:
            RspClient._validate(command)

    def connect(self):
        super().connect()
        need(all(name in self.register_info for name in REGISTERS),
             'required context register absent from target XML')
        for annex, raw in self.target_xml.items():
            (self.observer.m.work / 'evidence' / annex).write_bytes(raw)
        preflight = self.control.preflight()
        preflight['context'] = self.context()
        preflight['register_layout'] = {name: vars(self.register_info[name]) for name in REGISTERS}
        dump(self.observer.m.work / 'evidence/preflight.json', preflight)
        return self

    def context(self):
        need(self._state == 'stopped', 'context requires RSP stop')
        values = {name: self.read_register(name) for name in REGISTERS}
        values['instruction_read_address'] = values['rip']
        values['instruction_bytes'] = self.read_memory(values['rip'], 16).hex()
        values['stop'] = self.last_stop.decode('ascii')
        need(re.search(r'thread:0*1;', values['stop']) is not None,
             'stop packet does not identify the sole vCPU thread1')
        bits = self.request('qqemu.sstepbits').decode('ascii')
        mode = self.request('qqemu.sstep').decode('ascii')
        need(bits == 'ENABLE=1,NOIRQ=2,NOTIMER=4' and mode == '0x7',
             'unsupported or changed native step mode')
        values.update(sstepbits=bits, sstep=mode,
                      tf=bool(values['eflags'] & 0x100), rf=bool(values['eflags'] & 0x10000))
        return values

    def resume(self):
        if self.failed:
            raise PolicyError('diagnostic terminal stop forbids further continuation')
        self.control.check_budget()
        return super().resume()

    def step_over_breakpoint(self, addr):
        if self.failed:
            raise PolicyError('diagnostic terminal stop forbids further stepping')
        record = {'ordinal': len(self.control.steps) + 1, 'addr': addr,
                  'phase': self.observer.phase, 'start_ns': time.monotonic_ns(),
                  'breakpoints_before': sorted(self._hardware_breakpoints),
                  'watches_before': sorted(self.observer.watches)}
        self.control.steps.append(record)
        offset = None
        try:
            record['qmp_before'] = self.control.stopped_context()
            record['before'] = self.context()
            offset = self.control.begin()
            # Original implementation owns the sole s, guard and finally-restoration.
            reply = super().step_over_breakpoint(addr)
            record['returned_stop'] = reply.decode('ascii')
            return reply
        except Exception as e:
            self.failed = True
            record['error'] = f'{type(e).__name__}: {e}'
            raise
        finally:
            try:
                if self._state == 'stopped':
                    record['after'] = self.context()
                    record['qmp_after'] = self.control.stopped_context()
                record['breakpoints_after'] = sorted(self._hardware_breakpoints)
                record['watches_after'] = sorted(self.observer.watches)
                need(record['breakpoints_after'] == record['breakpoints_before'],
                     'execute breakpoint restoration/census mismatch')
                need(record['watches_after'] == record['watches_before'],
                     'write-watch coverage changed during native step')
                if offset is not None:
                    record['trace'] = self.control.end(offset)
            except Exception as e:
                self.failed = True
                record['capture_error'] = f'{type(e).__name__}: {e}'
                raise
            finally:
                record['end_ns'] = time.monotonic_ns()
                append(self.observer.m.work / 'evidence/steps.jsonl', record)


class CheckedShell(D.Shell):
    def __init__(self, machine, health_check):
        super().__init__(machine)
        self.health_check = health_check

    def wait_prompt(self, count, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.health_check()
            if self.idle(self.raw(), count=count):
                return True
            time.sleep(.02)
        return False


def protected_paths():
    prior = json.loads((W / 'canonical/before.json').read_text())
    return {name: {'size': Path(name).stat().st_size, 'sha256': sha(name)} for name in prior}


def analyze(steps, run_status):
    failed = [s for s in steps if NO_PROGRESS in s.get('error', '')]
    healthy = [s for s in steps if 'error' not in s and 'capture_error' not in s and
               s.get('trace', {}).get('tb_entries') and
               s.get('after', {}).get('rip') != s.get('before', {}).get('rip')]
    result = {'native_step_count': len(steps), 'same_pc_failures': len(failed),
              'successful_trace_health_scopes': len(healthy),
              'root_cause': 'UNPROVEN', 'pending_debug_attribution': 'SUSPECTED',
              'retirement_proven_by_TB_entry_alone': False,
              'full_workload_memory_qualification': False}
    if len(failed) == 1:
        last = failed[0]
        result['failure'] = last
        same = all(last.get('before', {}).get(k) == last.get('after', {}).get(k)
                   for k in ('rip', 'cs', 'cr3'))
        complete = ('capture_error' not in last and last.get('trace', {}).get('complete_ordered_scope')
                    and same and healthy and steps[-1] is last)
        result['classification'] = ('ZERO_TB_DISPATCH_AT_SAME_PC' if complete and
                                    last['trace']['zero_tb_dispatch_observed'] else
                                    'INCONCLUSIVE_SAME_PC')
    else:
        result['classification'] = ('INCONCLUSIVE_NON_REPRODUCTION' if run_status == 'COMPLETE'
                                    else 'INCONCLUSIVE_DIAGNOSTIC_STOP')
    return result


def main():
    # Exclusive claim is permanent: a failed attempt never authorizes rerunning this script.
    OUT.mkdir(mode=0o700, parents=False, exist_ok=False)
    before = protected_paths()
    dump(OUT / 'preservation-before.json', before)
    old_trace_before = {'size': OLD_TRACE.stat().st_size, 'sha256': sha(OLD_TRACE)}
    dump(OUT / 'old-compressed-trace-before.json', old_trace_before)
    need(sha(C) == CAND_SHA, 'qualified candidate identity changed')
    source_paths = [Path(__file__), RUNTIME / 'common.py', RUNTIME / 'observer.py',
                    RUNTIME / 'rsp.py', R / 'scripts/phase10_canonical_drive.py',
                    R / 'scripts/phase04_gui_drive.py', R / 'scripts/phase05_vt_hostile.py']
    source_before = {str(p): sha(p) for p in source_paths}
    binary = Path(shutil.which('qemu-system-x86_64'))
    dump(OUT / 'host-provenance.json', {
        'qemu_binary': str(binary), 'qemu_sha256': sha(binary),
        'version': subprocess.run([str(binary), '--version'], capture_output=True,
                                  text=True, check=True).stdout,
        'elf_notes': subprocess.run(['readelf', '-n', str(binary)], capture_output=True,
                                    text=True, check=True).stdout,
        'host_sources': source_before,
        'source_reference_scope': 'upstream v8.2.2 source plus installed event metadata; Ubuntu patched source not independently reconstructed'})
    machine = observer = surface = control = None
    record = {'status': 'STARTING', 'candidate_runs': 0, 'profile': 'headless',
              'mode': 'diagnostic-only', 'start_ns': time.monotonic_ns(),
              'approval': 'user2026-10-28 S10-4.9 exactly one attempt; no extra/drain/retry step'}
    try:
        machine = Machine('headless', OUT / 'vm', True)
        trace_path = machine.work / 'evidence/qemu-trace.log'
        machine.argv += ['-D', str(trace_path), '-msg', 'timestamp=on']
        for event in BASE_EVENTS:
            machine.argv += ['-trace', f'enable={event}']
        provenance = json.loads((machine.work / 'evidence/input-provenance.json').read_text())
        provenance['argv'] = machine.argv
        provenance['mode'] = 'S10-4.9-diagnostic-only'
        dump(machine.work / 'evidence/input-provenance.json', provenance)
        record['argv'] = machine.argv
        record['candidate_runs'] = 1
        machine.start()
        machine.qmp.sock.settimeout(5)
        record['qemu_pid'] = machine.proc.pid
        control = TraceControl(machine)
        observer = Observer(machine)
        observer.client = DiagnosticClient(observer, control)
        observer.start()
        while not observer.ready.wait(.1):
            control.check_budget()
        observer.check()
        surface = Surface(machine, observer)
        surface.sh = CheckedShell(machine, surface.check)
        surface.ready()
        surface.sample_idle('ordinary-prompt-idle')
        surface.streams()
        surface.top_idle()
        surface.phase('complete')
        surface.snapshot('complete')
        record['status'] = 'COMPLETE'
    except Exception as e:
        record.update(status='STOP', error=f'{type(e).__name__}: {e}', traceback=traceback.format_exc())
    finally:
        if surface:
            record['surface_events'] = surface.events
        if observer:
            try:
                observer.shutdown()
            except Exception as e:
                record['observer_error'] = f'{type(e).__name__}: {e}'
                record['status'] = 'STOP'
        if machine:
            try:
                machine.stop()
            except Exception as e:
                record['cleanup_error'] = f'{type(e).__name__}: {e}'
                record['status'] = 'STOP'
        record['end_ns'] = time.monotonic_ns()
        dump(OUT / 'runtime-result.json', record)
        after = protected_paths()
        dump(OUT / 'preservation-after.json', after)
        changed = [name for name in before if before[name] != after.get(name)]
        old_trace_after = {'size': OLD_TRACE.stat().st_size, 'sha256': sha(OLD_TRACE)}
        source_changed = [str(p) for p in source_paths if source_before[str(p)] != sha(p)]
        comparison = {'unchanged': not changed, 'files': len(before), 'canonical_files': 469,
                      'changed': changed, 'old_compressed_trace_unchanged': old_trace_before == old_trace_after,
                      'host_source_changed': source_changed,
                      'old_compressed_trace': old_trace_after}
        dump(OUT / 'preservation-comparison.json', comparison)
        result = analyze(control.steps if control else [], record['status'])
        result.update(runtime=record, preservation=comparison,
                      cbx27_met=False, phase10_criterion4_met=False,
                      candidate_rebuilds=0, desktop_runs=0, extra_steps=0,
                      candidate_sha256=CAND_SHA)
        dump(OUT / 'summary.json', result)
        for name in ('runtime-result.json', 'summary.json', 'preservation-before.json',
                     'preservation-after.json', 'preservation-comparison.json',
                     'old-compressed-trace-before.json', 'host-provenance.json'):
            shutil.copy2(OUT / name, ARCHIVE / name)
        if machine and (machine.work / 'evidence').exists():
            shutil.copytree(machine.work / 'evidence', ARCHIVE / 'evidence', dirs_exist_ok=False)
        # The writable clone is owned solely by this diagnostic; never remove source disks.
        if machine and (machine.work / 'disk.img').exists():
            (machine.work / 'disk.img').unlink()
        checks = {str(p.relative_to(ARCHIVE)): sha(p) for p in ARCHIVE.rglob('*')
                  if p.is_file() and p.name != 'evidence.sha256' and '__pycache__' not in p.parts}
        (ARCHIVE / 'evidence.sha256').write_text(''.join(f'{v}  {k}\n' for k, v in sorted(checks.items())))
        print(json.dumps({'classification': result['classification'], 'steps': result['native_step_count'],
                          'same_pc_failures': result['same_pc_failures'],
                          'trace_health_scopes': result['successful_trace_health_scopes'],
                          'runtime_status': record['status'], 'error': record.get('error'),
                          'preservation': comparison}, indent=2), flush=True)
        need(comparison['unchanged'] and comparison['old_compressed_trace_unchanged']
             and not source_changed, 'protected bytes or executing host source changed')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
