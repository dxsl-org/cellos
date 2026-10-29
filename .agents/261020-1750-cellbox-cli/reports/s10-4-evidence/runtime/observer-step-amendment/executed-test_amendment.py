"""Synthetic host-only proof/control tests; no QEMU, candidate or guest execution."""
import copy
import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import amendment as A
from test_rsp import CORE, CONTROL, FakeSocket

ADDR = 0x1000
BYTES = '58c3' + '90' * 14


def witness(flag=3, stop='T05thread:01;'):
    lines = [('gdbstub_io_command', 'Received: p2', 11),
             ('gdbstub_io_reply', 'Sent: 0010000000000000', 11),
             ('gdbstub_io_command', 'Received: z1,1000,1', 11),
             ('gdbstub_io_reply', 'Sent: OK', 11),
             ('gdbstub_io_command', 'Received: s', 11),
             ('breakpoint_singlestep', 'cpu=0 enable=7', 11),
             ('exec_tb', 'tb:0xab00 pc=0x1000', 22),
             ('exec_tb_exit', f'tb:0xab00 flags=0x{flag:x}', 22),
             ('gdbstub_hit_break', 'RUN_STATE_DEBUG', 11),
             ('gdbstub_io_reply', f'Sent: {stop}', 11),
             ('breakpoint_singlestep', 'cpu=0 enable=0', 11),
             ('gdbstub_io_command', 'Received: p2', 11),
             ('gdbstub_io_reply', 'Sent: 0010000000000000', 11),
             ('gdbstub_io_command', 'Received: Z1,1000,1', 11),
             ('gdbstub_io_reply', 'Sent: OK', 11)]
    return ''.join(f'{tid}@123.{n:06d}:{event} {text}\n' for n, (event, text, tid) in enumerate(lines, 1)).encode()


def context(pc=ADDR, sp=0x2000, stop='T05thread:01;'):
    return {'rip': pc, 'eflags': 0x202, 'cs': 0x23, 'cr3': 0x3000, 'rsp': sp,
            'instruction_read_address': pc, 'instruction_bytes': BYTES,
            'sstepbits': 'ENABLE=1,NOIRQ=2,NOTIMER=4', 'sstep': '0x7',
            'tf': False, 'rf': False, 'stop': stop}


def qmp():
    return {'cpus': [{'cpu-index': 0, 'target': 'x86_64', 'thread-id': 22}],
            'status': {'running': False, 'status': 'debug', 'singlestep': False}}


def proof_fixture(raw=None):
    raw = witness() if raw is None else raw
    parsed = A.X.trace_interval(raw)
    parsed.update(raw_sha256=hashlib.sha256(raw).hexdigest(), offset=0, end_offset=len(raw))
    first = {'ordinal': 2, 'addr': ADDR, 'before': context(stop='T05thread:01;watch:3000;'),
             'after': context(), 'qmp_before': qmp(), 'qmp_after': qmp(),
             'breakpoints_before': [(ADDR, 1), (0x4000, 1)],
             'breakpoints_after': [(ADDR, 1), (0x4000, 1)],
             'watches_before': [(0x3000, 8)], 'watches_after': [(0x3000, 8)],
             'error': f'RspError: {A.X.NO_PROGRESS}', 'trace': parsed}
    healthy = {'ordinal': 1, 'addr': ADDR, 'before': context(),
               'after': context(ADDR + 1, 0x2008), 'trace': A.X.trace_interval(witness(0))}
    return first, context(), qmp(), {(ADDR, 1), (0x4000, 1)}, {(0x3000, 8)}, raw, [healthy], True


class FakeControl:
    def __init__(self, path):
        self.path = path
        self.path.write_bytes(b'')
        self.steps = []
        self.host_verified = True

    def check_budget(self):
        pass

    def preflight(self):
        return qmp()

    def stopped_context(self):
        return qmp()

    def begin(self):
        return self.path.stat().st_size

    def end(self, offset):
        raw = self.path.read_bytes()[offset:]
        result = A.X.trace_interval(raw)
        result.update(offset=offset, end_offset=self.path.stat().st_size,
                      raw_sha256=hashlib.sha256(raw).hexdigest())
        return result


class AmendmentTests(unittest.TestCase):
    def client(self, outcomes, *, healthy=True, first_raw=None, second_stop='T05thread:01;'):
        temp = tempfile.TemporaryDirectory(prefix='s10-4-amendment-unit-')
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        (root / 'evidence').mkdir()
        observer = SimpleNamespace(m=SimpleNamespace(work=root, gdb_path=root / 'gdb.sock'),
                                   phase='synthetic', watches={(0x3000, 8)})
        control = FakeControl(root / 'trace.log')
        state = {'pc': ADDR, 'sp': 0x2000, 'calls': 0}
        def native(_):
            index = state['calls']
            state['calls'] += 1
            advance = outcomes[index]
            stop = 'T05thread:01;' if index == 0 else second_stop
            raw = (first_raw if index == 0 and first_raw is not None else
                   witness(0 if advance else 3, stop))
            with control.path.open('ab') as f:
                f.write(raw)
            if advance:
                state['pc'] += 1
                state['sp'] += 8
            return stop.encode()
        overrides = {'p1': lambda _: state['sp'].to_bytes(8, 'little').hex().encode(),
                     'p2': lambda _: state['pc'].to_bytes(8, 'little').hex().encode(),
                     'p3': b'23000000', 'p4': b'0030000000000000', 'p5': b'02020000',
                     'm1000,10': BYTES.encode(), 'm1001,10': BYTES.encode(),
                     'qqemu.sstepbits': b'ENABLE=1,NOIRQ=2,NOTIMER=4', 'qqemu.sstep': b'0x7', 's': native}
        core = CORE.replace(b'</feature>', b'<reg name="eflags" bitsize="32" regnum="5"/></feature>')
        peer = FakeSocket(overrides, {'core.xml': core,
                                     'control.xml': CONTROL.replace(b'name="cr3"', b'name="cr3" regnum="4"')})
        client = A.AmendedClient(observer, control)
        self.addCleanup(client.close)
        with patch('rsp.socket.socket', return_value=peer):
            client.connect()
        client.add_breakpoint(ADDR)
        client.add_breakpoint(0x4000)
        client.add_watchpoint(0x3000, 8)
        if healthy:
            control.steps.append(proof_fixture()[-2][0])
        return client, peer, control

    def test_complete_ordered_same_TB_flag3_proof_admitted(self):
        result = A.admit(*proof_fixture())
        self.assertTrue(result['admitted'])
        self.assertEqual(result['health_ordinal'], 1)
        self.assertFalse(result['recurrence_authorized'])

    def test_entry_exit_order_pointer_PC_count_and_flag_are_required(self):
        good = witness()
        lines = good.splitlines(keepends=True)
        reordered = list(lines)
        reordered[6], reordered[7] = reordered[7], reordered[6]
        cases = [good.replace(b'tb:0xab00 flags=0x3', b'tb:0xac00 flags=0x3'),
                 good.replace(b'pc=0x1000', b'pc=0x1001'), witness(0),
                 b''.join(reordered), good.replace(lines[7], b''),
                 good.replace(lines[6], lines[6] + lines[6])]
        for raw in cases:
            with self.subTest(raw=raw), self.assertRaises(RuntimeError):
                A.admit(*proof_fixture(raw))

    def test_missing_restore_remove_OK_disable_or_native_mode_rejected(self):
        raw = witness()
        for old, new in ((b'Received: Z1,1000,1', b'Received: p2'),
                         (b'Sent: OK', b'Sent: E22'),
                         (b'cpu=0 enable=7', b'cpu=0 enable=1'),
                         (b'cpu=0 enable=0', b'cpu=1 enable=0')):
            with self.subTest(old=old), self.assertRaises(RuntimeError):
                A.admit(*proof_fixture(raw.replace(old, new)))

    def test_interleaved_continue_extra_step_or_watch_removal_rejected(self):
        raw = witness()
        insertion = b'11@123.000004:gdbstub_io_command Received: '
        for command in (b'c', b's', b'z2,3000,8', b'M1000,1:90', b'P0=00', b'Z0,1000,1',
                        b'Qqemu.sstep:7'):
            changed = raw.replace(b'11@123.000005:gdbstub_io_command Received: s',
                                  insertion + command + b'\n11@123.000005:gdbstub_io_command Received: s')
            with self.subTest(command=command), self.assertRaises(RuntimeError):
                A.admit(*proof_fixture(changed))

    def test_each_stable_context_field_checked_on_after_and_fresh_read(self):
        for key in A.STABLE_FIELDS:
            for index in (0, 1):
                args = list(proof_fixture())
                target = args[0]['after'] if index == 0 else args[1]
                target[key] = 'changed' if isinstance(target[key], str) else target[key] + 1
                with self.subTest(key=key, target=index), self.assertRaises(RuntimeError):
                    A.admit(*args)

    def test_vcpu_running_state_and_trace_TID_mismatch_rejected(self):
        for target in ('after_cpu', 'fresh_cpu', 'running', 'host_semantics', 'trace_tid'):
            args = list(proof_fixture())
            if target == 'after_cpu':
                args[0]['qmp_after']['cpus'][0]['thread-id'] = 23
            elif target == 'fresh_cpu':
                args[2]['cpus'][0]['thread-id'] = 23
            elif target == 'running':
                args[2]['status']['running'] = True
            elif target == 'host_semantics':
                args[7] = False
            else:
                args = list(proof_fixture(witness().replace(b'22@', b'23@')))
            with self.subTest(target=target), self.assertRaises(RuntimeError):
                A.admit(*args)

    def test_full_coverage_enrollment_and_duplicate_census_rejected(self):
        for target in ('bp_after', 'bp_current', 'wp_after', 'wp_current', 'duplicate'):
            args = list(proof_fixture())
            if target == 'bp_after':
                args[0]['breakpoints_after'] = [(0x4000, 1)]
            elif target == 'bp_current':
                args[3] = {(0x4000, 1)}
            elif target == 'wp_after':
                args[0]['watches_after'] = []
            elif target == 'wp_current':
                args[4] = set()
            else:
                args[0]['breakpoints_before'].append((ADDR, 1))
            with self.subTest(target=target), self.assertRaises(RuntimeError):
                A.admit(*args)

    def test_missing_capture_digest_health_or_unexpected_stop_rejected(self):
        for target in ('capture', 'digest', 'health', 'stop'):
            args = list(proof_fixture())
            if target == 'capture':
                args[0]['capture_error'] = 'missing log'
            elif target == 'digest':
                args[0]['trace']['raw_sha256'] = '0' * 64
            elif target == 'health':
                args[6] = []
            else:
                args[0]['after']['stop'] = 'T05thread:01;watch:3000;'
            with self.subTest(target=target), self.assertRaises(RuntimeError):
                A.admit(*args)

    def test_original_success_uses_one_step_and_does_not_consume_allowance(self):
        client, peer, _ = self.client([True])
        self.assertEqual(client.step_over_breakpoint(ADDR), b'T05thread:01;')
        self.assertEqual(peer.commands.count('s'), 1)
        self.assertFalse(client.extra_used)
        self.assertEqual(client.decisions, [])

    def test_additional_success_stops_after_two_total_no_continue_or_recurrence(self):
        client, peer, control = self.client([False, True])
        with self.assertRaisesRegex(A.BoundaryStop, 'ADDITIONAL_ADVANCED_BOUNDARY_STOP'):
            client.step_over_breakpoint(ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertEqual(client.extra_native_packets, 1)
        self.assertEqual(client.decisions[-1]['extra_steps_used'], 1)
        self.assertNotIn('c', peer.commands)
        self.assertEqual(client._hardware_breakpoints, {(ADDR, 1), (0x4000, 1)})
        self.assertTrue(all(step.get('watches_before') == step.get('watches_after')
                            for step in control.steps[1:]))
        before = len(peer.commands)
        with self.assertRaises(A.X.PolicyError):
            client.resume()
        with self.assertRaises(A.X.PolicyError):
            client.step_over_breakpoint(ADDR)
        self.assertEqual(len(peer.commands), before)

    def test_second_no_progress_stops_no_third_step_and_restores(self):
        client, peer, _ = self.client([False, False])
        with self.assertRaisesRegex(A.BoundaryStop, 'ADDITIONAL_FAILED_BOUNDARY_STOP'):
            client.step_over_breakpoint(ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertIn((ADDR, 1), client._hardware_breakpoints)
        with self.assertRaises(A.X.PolicyError):
            client.step_over_breakpoint(ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertNotIn('c', peer.commands)

    def test_missing_health_or_pairing_proof_never_sends_additional_step(self):
        for kwargs in ({'healthy': False}, {'first_raw': witness(0)}):
            client, peer, _ = self.client([False], **kwargs)
            with self.subTest(kwargs=kwargs), self.assertRaisesRegex(A.BoundaryStop, 'PROOF_REJECTED'):
                client.step_over_breakpoint(ADDR)
            self.assertEqual(peer.commands.count('s'), 1)
            self.assertEqual(client.extra_native_packets, 0)

    def test_additional_unexpected_watch_stop_is_terminal_no_followup(self):
        client, peer, _ = self.client([False, True], second_stop='T05thread:01;watch:3000;')
        with self.assertRaisesRegex(A.BoundaryStop, 'ADDITIONAL_FAILED_BOUNDARY_STOP'):
            client.step_over_breakpoint(ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertNotIn('c', peer.commands)
        self.assertNotIn('z2,3000,8', peer.commands)
        self.assertIn((ADDR, 1), client._hardware_breakpoints)

    def test_enrollment_guard_rejects_unenrolled_site_before_any_native_packet(self):
        client, peer, _ = self.client([False])
        with self.assertRaises(RuntimeError):
            client.step_over_breakpoint(0x5000)
        self.assertEqual(peer.commands.count('s'), 0)
        self.assertFalse(client.extra_used)

    def test_packet_policy_still_rejects_raw_step_writes_Z0_and_mode_setters(self):
        client, peer, _ = self.client([False])
        before = list(peer.commands)
        for command in ('s', 's1000', 'vCont;s', 'M1000,1:90', 'P0=00', 'Z0,1000,1', 'Qqemu.sstep:7'):
            with self.subTest(command=command), self.assertRaises(A.X.PolicyError):
                client.request(command)
        self.assertEqual(peer.commands, before)

    def test_fresh_context_mutation_after_admission_prevents_additional_packet(self):
        client, peer, _ = self.client([False])
        original = client._additional_once
        def changed(addr, first, admitted_context, admitted_qmp):
            bad = copy.deepcopy(admitted_context)
            bad['rsp'] += 8
            return original(addr, first, bad, admitted_qmp)
        with patch.object(client, '_additional_once', changed):
            with self.assertRaisesRegex(A.BoundaryStop, 'ADDITIONAL_FAILED_BOUNDARY_STOP'):
                client.step_over_breakpoint(ADDR)
        self.assertEqual(peer.commands.count('s'), 1)
        self.assertEqual(client.decisions[-1]['extra_steps_used'], 0)
        self.assertNotIn('c', peer.commands)


if __name__ == '__main__':
    unittest.main()
