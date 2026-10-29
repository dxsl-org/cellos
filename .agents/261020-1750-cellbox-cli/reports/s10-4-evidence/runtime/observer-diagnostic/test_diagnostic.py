"""Synthetic host-only tests; never launches QEMU or accesses candidate bytes."""
import json
from pathlib import Path
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import diagnostic as X
from test_rsp import CORE, CONTROL, FakeSocket


def trace(*middle, pc=0x1004, mode=7, stop='T05thread:01;'):
    lines = ['11@123.000001:gdbstub_io_command Received: p2',
             '11@123.000002:gdbstub_io_reply Sent: 0010000000000000',
             '11@123.000003:gdbstub_io_command Received: s',
             f'11@123.000004:breakpoint_singlestep cpu=0 enable={mode}']
    lines.extend(f'22@123.000005:{line}' for line in middle)
    lines.extend(['11@123.000006:gdbstub_hit_break RUN_STATE_DEBUG',
                  f'11@123.000007:gdbstub_io_reply Sent: {stop}',
                  '11@123.000008:breakpoint_singlestep cpu=0 enable=0'])
    return ('\n'.join(lines) + '\n').encode()


class FakeControl:
    def __init__(self, payload=None):
        self.steps = []
        self.payload = trace('exec_tb tb:0xab pc=0x1000') if payload is None else payload
        self.calls = []

    def check_budget(self):
        self.calls.append('budget')

    def preflight(self):
        self.calls.append('preflight')
        return {}

    def stopped_context(self):
        self.calls.append('stopped')
        return {'cpus': [{'cpu-index': 0, 'target': 'x86_64', 'thread-id': 22}],
                'status': {'running': False}}

    def begin(self):
        self.calls.append('enable')
        return 0

    def end(self, offset):
        self.calls.append('disable')
        return X.trace_interval(self.payload)


class QmpWire:
    def __init__(self, overrides=None):
        self.requests = []
        self.pending = []
        self.states = {e: ('enabled' if e in X.BASE_EVENTS else 'disabled')
                       for e in X.BASE_EVENTS + X.EXEC_EVENTS}
        self.overrides = overrides or {}

    def write(self, data):
        request = json.loads(data)
        self.requests.append(request)
        name = request['execute']
        args = request.get('arguments', {})
        if name in self.overrides:
            result = self.overrides[name]
        elif name == 'query-cpus-fast':
            result = [{'cpu-index': 0, 'target': 'x86_64', 'thread-id': 22}]
        elif name == 'query-status':
            result = {'running': False, 'status': 'debug'}
        elif name == 'trace-event-get-state':
            result = [{'name': args['name'], 'state': self.states[args['name']]}]
        elif name == 'trace-event-set-state':
            self.states[args['name']] = 'enabled' if args['enable'] else 'disabled'
            result = {}
        else:
            raise AssertionError(request)
        self.pending.extend([json.dumps({'event': 'STOP'}).encode() + b'\n',
                             json.dumps({'return': result}).encode() + b'\n'])

    def readline(self):
        return self.pending.pop(0) if self.pending else b''


class DiagnosticTests(unittest.TestCase):
    def directory(self):
        temp = tempfile.TemporaryDirectory(prefix='s10-4-diagnostic-unit-')
        self.addCleanup(temp.cleanup)
        directory = Path(temp.name)
        (directory / 'evidence').mkdir()
        return directory

    def client(self, advance=True, payload=None):
        directory = self.directory()
        observer = SimpleNamespace(m=SimpleNamespace(work=directory, gdb_path=directory / 'gdb.sock'),
                                   phase='synthetic', watches={(0x3000, 8), (0x4000, 4096)})
        control = FakeControl(payload)
        pc = {'value': 0x1000}
        def stepped(_):
            if advance:
                pc['value'] += 4
            return b'T05thread:01;'
        overrides = {'p1': b'0020000000000000',
                     'p2': lambda _: pc['value'].to_bytes(8, 'little').hex().encode(),
                     'p3': b'23000000', 'p4': b'0030000000000000', 'p5': b'02020000',
                     'm1000,10': b'90' * 16, 'm1004,10': b'90' * 16,
                     'qqemu.sstepbits': b'ENABLE=1,NOIRQ=2,NOTIMER=4',
                     'qqemu.sstep': b'0x7', 's': stepped}
        core = CORE.replace(b'</feature>', b'<reg name="eflags" bitsize="32" regnum="5"/></feature>')
        peer = FakeSocket(overrides, {'core.xml': core, 'control.xml': CONTROL.replace(b'name="cr3"', b'name="cr3" regnum="4"')})
        client = X.DiagnosticClient(observer, control)
        self.addCleanup(client.close)
        with patch('rsp.socket.socket', return_value=peer):
            client.connect()
        return client, peer, control, observer

    def controller(self, overrides=None):
        directory = self.directory()
        wire = QmpWire(overrides)
        machine = SimpleNamespace(work=directory, start_ns=time.monotonic_ns(),
                                  qmp=SimpleNamespace(file=wire))
        return X.TraceControl(machine), wire

    def test_read_extensions_exactly_scoped_and_setters_steps_writes_denied(self):
        client, peer, _, _ = self.client()
        self.assertEqual(client.request('qqemu.sstep'), b'0x7')
        before = list(peer.commands)
        for command in ('Qqemu.sstep:7', 'Qqemu.sstep=7', 'qqemu.sstep=7',
                        'qqemu.sstepbits;c', 's', 's1000', 'vCont;s', 'P0=00',
                        'M1000,1:90', 'G00', 'Z0,1000,1', 'qRcmd,00'):
            with self.subTest(command=command), self.assertRaises(X.PolicyError):
                client.request(command)
        self.assertEqual(before, peer.commands)

    def test_native_one_step_restore_and_all_other_guards_unchanged(self):
        client, peer, control, observer = self.client()
        client.add_breakpoint(0x1000)
        client.add_breakpoint(0x2000)
        for addr, size in observer.watches:
            client.add_watchpoint(addr, size)
        at = len(peer.commands)
        client.step_over_breakpoint(0x1000)
        commands = peer.commands[at:]
        self.assertEqual(commands.count('s'), 1)
        self.assertEqual([c for c in commands if c.startswith(('Z', 'z'))],
                         ['z1,1000,1', 'Z1,1000,1'])
        self.assertEqual(client._hardware_breakpoints, {(0x1000, 1), (0x2000, 1)})
        self.assertEqual(control.calls[-1], 'disable')
        self.assertEqual(control.steps[0]['before']['eflags'], 0x202)
        self.assertEqual(control.steps[0]['after']['rip'], 0x1004)
        self.assertFalse(control.steps[0]['trace']['retirement_proven_by_trace_alone'])

    def test_same_pc_original_guard_stops_once_and_forbids_resume_or_step(self):
        client, peer, control, _ = self.client(False, trace())
        client.add_breakpoint(0x1000)
        client.add_breakpoint(0x2000)
        with self.assertRaisesRegex(X.RspError, 'did not advance'):
            client.step_over_breakpoint(0x1000)
        self.assertIn('Z1,1000,1', peer.commands)
        self.assertEqual(peer.commands.count('s'), 1)
        self.assertNotIn('c', peer.commands)
        self.assertTrue(control.steps[-1]['trace']['zero_tb_dispatch_observed'])
        count = len(peer.commands)
        with self.assertRaises(X.PolicyError):
            client.resume()
        with self.assertRaises(X.PolicyError):
            client.step_over_breakpoint(0x1000)
        self.assertEqual(count, len(peer.commands))

    def test_capture_failure_is_terminal_after_restoration_without_fallback(self):
        client, peer, control, _ = self.client(payload=b'incomplete\n')
        client.add_breakpoint(0x1000)
        with self.assertRaisesRegex(RuntimeError, 'incomplete'):
            client.step_over_breakpoint(0x1000)
        self.assertTrue(client.failed)
        self.assertIn((0x1000, 1), client._hardware_breakpoints)
        self.assertEqual(peer.commands.count('s'), 1)
        self.assertIn('capture_error', control.steps[-1])
        with self.assertRaises(X.PolicyError):
            client.resume()

    def test_qmp_preflight_records_raw_messages_and_only_expected_names(self):
        control, wire = self.controller()
        control.preflight()
        control.query('trace-event-set-state', name='exec_tb', enable=True)
        self.assertEqual(wire.states['exec_tb'], 'enabled')
        rows = [json.loads(x) for x in control.log.read_text().splitlines()]
        self.assertTrue(any(r.get('message', {}).get('event') == 'STOP' for r in rows))
        self.assertIn({'execute': 'trace-event-set-state',
                       'arguments': {'name': 'exec_tb', 'enable': True}}, wire.requests)

    def test_qmp_control_policy_never_sends_forbidden_commands(self):
        control, wire = self.controller()
        forbidden = [('cont', {}), ('stop', {}), ('human-monitor-command', {'command-line': 'info registers'}),
                     ('trace-event-set-state', {'name': 'gdbstub_io_command', 'enable': False}),
                     ('trace-event-set-state', {'name': 'exec*', 'enable': True}),
                     ('trace-event-set-state', {'name': 'exec_tb', 'enable': 1}),
                     ('trace-event-get-state', {'name': '*'})]
        for name, args in forbidden:
            with self.subTest(name=name, args=args), self.assertRaises(X.PolicyError):
                control.query(name, **args)
        self.assertEqual(wire.requests, [])

    def test_qmp_multiple_cpu_running_and_unavailable_event_are_stops(self):
        for overrides in ({'query-cpus-fast': []}, {'query-status': {'running': True}},
                          {'trace-event-get-state': [{'name': 'gdbstub_io_command', 'state': 'unavailable'}]}):
            control, _ = self.controller(overrides)
            with self.subTest(overrides=overrides), self.assertRaises(RuntimeError):
                control.preflight()

    def test_scoped_trace_enable_query_before_disable_and_read_complete_interval(self):
        control, wire = self.controller()
        control.preflight()
        control.path.write_bytes(b'')
        offset = control.begin()
        control.path.write_bytes(trace('exec_tb tb:0xab pc=0x1000', 'exec_tb_exit tb:0xab flags=0x3'))
        result = control.end(offset)
        self.assertTrue(result['complete_ordered_scope'])
        self.assertEqual(len(result['tb_entries']), 1)
        self.assertFalse(result['zero_tb_dispatch_observed'])
        self.assertEqual(wire.states['exec_tb'], 'disabled')
        self.assertEqual(wire.states['exec_tb_exit'], 'disabled')

    def test_trace_zero_dispatch_not_pending_cause_or_retirement_claim(self):
        result = X.trace_interval(trace())
        self.assertTrue(result['zero_tb_dispatch_observed'])
        self.assertFalse(result['retirement_proven_by_trace_alone'])
        self.assertNotIn('pending', result)

    def test_trace_missing_exit_with_entry_never_proves_retirement(self):
        result = X.trace_interval(trace('exec_tb tb:0xab pc=0x1000'))
        self.assertEqual(result['tb_exits'], [])
        self.assertFalse(result['zero_tb_dispatch_observed'])
        self.assertFalse(result['retirement_proven_by_trace_alone'])

    def test_trace_incomplete_duplicate_step_wrong_mode_or_thread_rejected(self):
        valid = trace()
        cases = [b'partial', valid.replace(b'Received: s\n', b'Received: c\n'),
                 valid + valid, trace(mode=1),
                 valid.replace(b'11@123.000007', b'33@123.000007'),
                 valid.replace(b'11@123.000008:breakpoint_singlestep cpu=0 enable=0\n', b'')]
        for raw in cases:
            with self.subTest(raw=raw), self.assertRaises(RuntimeError):
                X.trace_interval(raw)

    def test_deadline_cannot_be_extended_by_qmp_query(self):
        control, wire = self.controller()
        control.machine.start_ns -= 901_000_000_000
        with self.assertRaisesRegex(RuntimeError, '900s'):
            control.query('query-status')
        self.assertEqual(wire.requests, [])

    def test_prompt_wait_checks_observer_before_any_more_input_or_progress(self):
        machine = SimpleNamespace()
        def blocked():
            raise RuntimeError('observer stopped')
        shell = X.CheckedShell(machine, blocked)
        with self.assertRaisesRegex(RuntimeError, 'observer stopped'):
            shell.wait_prompt(1, 120)

    def test_analyze_requires_prior_health_complete_context_and_terminal_failure(self):
        before = {'rip': 0x1000, 'cs': 0x23, 'cr3': 0x3000}
        failed = {'before': before, 'after': dict(before), 'error': X.NO_PROGRESS,
                  'trace': X.trace_interval(trace())}
        healthy = {'before': before, 'after': {**before, 'rip': 0x1004},
                   'trace': X.trace_interval(trace('exec_tb tb:0xab pc=0x1000'))}
        self.assertEqual(X.analyze([failed], 'STOP')['classification'], 'INCONCLUSIVE_SAME_PC')
        self.assertEqual(X.analyze([healthy, failed], 'STOP')['classification'], 'ZERO_TB_DISPATCH_AT_SAME_PC')
        for broken in ({**failed, 'capture_error': 'lost log'},
                       {**failed, 'after': {**before, 'cr3': 0x4000}}):
            self.assertEqual(X.analyze([healthy, broken], 'STOP')['classification'], 'INCONCLUSIVE_SAME_PC')
        self.assertEqual(X.analyze([healthy], 'COMPLETE')['classification'], 'INCONCLUSIVE_NON_REPRODUCTION')
        self.assertFalse(X.analyze([healthy], 'COMPLETE')['full_workload_memory_qualification'])


if __name__ == '__main__':
    unittest.main()
