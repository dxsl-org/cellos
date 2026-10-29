"""Host-only synthetic protocol tests; no VM/guest evidence or dependencies.

Run from this directory: python3 -m unittest -v test_rsp
All socket traffic is an in-process scripted fake, including deadlines. No QEMU,
network listener, candidate file, or real guest memory/register access occurs.
"""

from dataclasses import dataclass
import json
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import patch

from rsp import PolicyError, RegisterUnavailable, RspClient, RspError, RspTimeout


TARGET = b'''<?xml version="1.0"?>
<!DOCTYPE target SYSTEM "gdb-target.dtd">
<target xmlns:xi="http://www.w3.org/2001/XInclude">
<architecture>i386:x86-64</architecture>
<xi:include href="core.xml"/><xi:include href="control.xml"/>
</target>'''
CORE = b'''<feature name="synthetic.core">
<reg name="rax" bitsize="64" regnum="0"/>
<reg name="rsp" bitsize="64"/><reg name="rip" bitsize="64"/>
<reg name="cs" bitsize="32"/>
</feature>'''
CONTROL = b'<feature name="synthetic.control"><reg name="cr3" bitsize="64"/></feature>'
VALUES = {'rax': 0x0102030405060708, 'rsp': 0x12345678, 'rip': 0x10000000, 'cs': 0x23, 'cr3': 0x1000}
SIZES = {'rax': 8, 'rsp': 8, 'rip': 8, 'cs': 4, 'cr3': 8}
PACKED = b''.join(VALUES[name].to_bytes(SIZES[name], 'little').hex().encode() for name in VALUES)


def frame(payload, *, encoded=False):
    if not encoded:
        payload = b''.join(b'}' + bytes([byte ^ 0x20]) if byte in b'#$}*' else bytes([byte]) for byte in payload)
    return b'$' + payload + b'#' + f'{sum(payload) % 256:02x}'.encode()


@dataclass
class Wire:
    raw: bytes


class FakeSocket:
    """Scripted byte transport; recv on empty input times out immediately."""
    def __init__(self, overrides=None, annexes=None, fragment=65536):
        self.overrides = overrides or {}
        self.annexes = {'target.xml': TARGET, 'core.xml': CORE, 'control.xml': CONTROL}
        if annexes:
            self.annexes.update(annexes)
        self.fragment = fragment
        self.incoming = bytearray()
        self.sent = []
        self.commands = []
        self.recv_calls = 0
        self.closed = False
        self.connected = None
        self.timeouts = []

    def connect(self, path):
        self.connected = path

    def settimeout(self, duration):
        self.timeouts.append(duration)

    def close(self):
        self.closed = True

    def sendall(self, data):
        self.sent.append(data)
        if data in (b'+', b'-'):
            return
        if data == b'\x03':
            result = self.overrides.get('interrupt', b'T02thread:1;')
            self.incoming.extend(result.raw if isinstance(result, Wire) else frame(result))
            return
        assert data.startswith(b'$') and data[-3:-2] == b'#', data
        raw = data[1:-3]
        assert int(data[-2:], 16) == sum(raw) % 256
        command = raw.decode('ascii')
        self.commands.append(command)
        if command in self.overrides:
            result = self.overrides[command]
            if callable(result):
                result = result(command)
        elif command == 'qSupported:qXfer:features:read+':
            result = b'PacketSize=1000;qXfer:features:read+;hwbreak+'
        elif command == '?':
            result = b'T05thread:1;'
        elif command.startswith('qXfer:features:read:'):
            _, _, _, annex, bounds = command.split(':')
            offset, length = (int(value, 16) for value in bounds.split(','))
            content = self.annexes[annex]
            result = (b'l' if offset + length >= len(content) else b'm') + content[offset:offset + length]
        elif command == 'g':
            result = PACKED
        elif command.startswith(('Z', 'z', 'H', 'Qqemu.PhyMemMode:')):
            result = b'OK'
        elif command == 'qqemu.PhyMemMode':
            result = b'0'
        elif command == 'c':
            result = None
        else:
            raise AssertionError(f'unscripted command {command!r}')
        if isinstance(result, Wire):
            self.incoming.extend(result.raw)
        else:
            self.incoming.extend(b'+')
            if result is not None:
                self.incoming.extend(frame(result))

    def recv(self, count):
        self.recv_calls += 1
        if not self.incoming:
            raise TimeoutError('synthetic empty transport')
        count = min(count, self.fragment, len(self.incoming))
        raw = bytes(self.incoming[:count])
        del self.incoming[:count]
        return raw


class ClientTests(unittest.TestCase):
    def client(self, overrides=None, annexes=None, fragment=65536, connect=True):
        directory = tempfile.TemporaryDirectory(prefix='rsp-unit-')
        self.addCleanup(directory.cleanup)
        log = Path(directory.name) / 'packets.jsonl'
        peer = FakeSocket(overrides, annexes, fragment)
        instance = RspClient(str(Path(directory.name) / 'gdb.sock'), str(log), timeout=0.25)
        self.addCleanup(instance.close)
        if connect:
            with patch('rsp.socket.socket', return_value=peer) as factory:
                self.assertIs(instance.connect(), instance)
            factory.assert_called_once_with(socket.AF_UNIX, socket.SOCK_STREAM)
        return instance, peer, log

    def test_connect_unix_xml_includes_and_little_endian_registers(self):
        client, peer, _ = self.client()
        self.assertTrue(peer.connected.endswith('/gdb.sock'))
        self.assertEqual(client.last_stop, b'T05thread:1;')
        self.assertEqual(set(client.target_xml), {'target.xml', 'core.xml', 'control.xml'})
        self.assertEqual(client.register_info['cr3'].number, 4)
        self.assertEqual(client.register_info['cs'].bitsize, 32)
        self.assertEqual(client.registers(), VALUES)
        self.assertEqual(peer.commands[-1], 'g')
        self.assertNotIn('c', peer.commands)

    def test_fragmented_and_coalesced_frames(self):
        client, _, _ = self.client({'qC': Wire(b'+' + frame(b'O6869') + frame(b'QC1'))}, fragment=1)
        self.assertEqual(client.request('qC'), b'QC1')
        self.assertEqual(client.console, [b'hi'])

    def test_binary_escapes_and_run_length(self):
        client, _, _ = self.client({'qC': Wire(b'+' + frame(b'}\x03}\x04}]}' + bytes([ord('*') ^ 0x20]) + b'A* ', encoded=True))})
        self.assertEqual(client.request('qC'), b'#$}*AAAA')

    def test_run_length_invalid_forms(self):
        for raw in (b'* ', b'A*', b'A*\x1f', b'A*\x7f', b'A*#', b'}'):
            with self.subTest(raw=raw), self.assertRaises(RspError):
                RspClient._decode(raw)

    def test_escaped_star_not_run_length(self):
        self.assertEqual(RspClient._decode(b'A}\x0a '), b'A* ')

    def test_checksum_mismatch_sends_nack_no_retry(self):
        client, peer, _ = self.client({'qC': Wire(b'+$QC1#00')})
        with self.assertRaisesRegex(RspError, 'checksum mismatch'):
            client.request('qC')
        self.assertEqual(peer.sent[-1], b'-')
        self.assertEqual(peer.commands.count('qC'), 1)

    def test_target_nack_no_retry(self):
        client, peer, _ = self.client({'qC': Wire(b'-')})
        with self.assertRaisesRegex(RspError, 'rejected'):
            client.request('qC')
        self.assertEqual(peer.commands.count('qC'), 1)

    def test_nonhex_checksum_and_invalid_framing(self):
        for raw in (b'+$QC1#zz', b'+garbage', b'+%Stop:T05#00'):
            with self.subTest(raw=raw):
                client, _, _ = self.client({'qC': Wire(raw)})
                with self.assertRaises(RspError):
                    client.request('qC')

    def test_unsupported_and_target_error_are_explicit(self):
        for payload in (b'', b'E22'):
            with self.subTest(payload=payload):
                client, _, _ = self.client({'qC': payload})
                with self.assertRaises(RspError):
                    client.request('qC')

    def test_console_malformed_fails(self):
        client, _, _ = self.client({'qC': Wire(b'+' + frame(b'O123'))})
        with self.assertRaisesRegex(RspError, 'console'):
            client.request('qC')

    def test_allow_policy(self):
        commands = ('?', 'qSupported', 'qSupported:qXfer:features:read+',
                    'qXfer:features:read:target.xml:0,100', 'qAttached',
                    'qAttached:1', 'qC', 'Hg0', 'Hc-1', 'Hgp1.1',
                    'm1000,10', 'p1a', 'g', 'c', 'vCont?',
                    'Z1,1000,1', 'z1,1000,1', 'Z2,2000,1000', 'z2,2000,1000',
                    'qqemu.PhyMemMode', 'Qqemu.PhyMemMode:0', 'Qqemu.PhyMemMode:1')
        for command in commands:
            with self.subTest(command=command):
                RspClient._validate(command)

    def test_deny_policy_never_sends(self):
        client, peer, _ = self.client()
        before = list(peer.sent)
        commands = ('M1000,1:00', 'X1000,1:a', 'G00', 'P0=00',
                    'Z0,1000,1', 'z0,1000,1', 'Z3,1000,1', 'Z4,1000,1',
                    'Z2,1000,1;X1', 'Z1,1000,0', 'm1000,0',
                    'qRcmd,68656c70', 'qXfer:exec-file:read::0,10',
                    'qXfer:features:write:target.xml:0,1:a',
                    'qXfer:features:read:target.xml:0,0', 'QStartNoAckMode',
                    'Qqemu.PhyMemMode:2', 'Qqemu.sstep=7', 'qqemu.sstep',
                    'vCont;c', 'vCont;s', 's', 'C05', 'c1000', 'vRun', 'vAttach;1',
                    'D', 'k', '!', '\x03', 'g\n', 'g\x00', 'g#67', '$g#67',
                    'Hg1;M0,1:00', 'qSupported:qRcmd+', 'm1,1\nc', '')
        for command in commands:
            with self.subTest(command=command), self.assertRaises(PolicyError):
                client.request(command)
        self.assertEqual(peer.sent, before)

    def test_hardware_operations_no_software_fallback(self):
        client, peer, _ = self.client()
        client.add_breakpoint(0x1000)
        client.remove_breakpoint(0x1000)
        client.add_watchpoint(0x2000, 4096)
        client.remove_watchpoint(0x2000, 4096)
        self.assertEqual(peer.commands[-4:], ['Z1,1000,1', 'z1,1000,1', 'Z2,2000,1000', 'z2,2000,1000'])
        for response in (b'', b'E22', b'unsupported'):
            peer.overrides['Z1,1000,1'] = response
            with self.subTest(response=response), self.assertRaises(RspError):
                client.add_breakpoint(0x1000)
        self.assertFalse(any(command.startswith(('Z0', 'M', 'X')) for command in peer.commands))

    def test_read_exact_memory_and_chunking(self):
        client, peer, _ = self.client({'m1000,4': b'0001abff', 'm1004,2': b'0506'})
        client._packet_size = 8
        # The wire request itself must still fit PacketSize: m1000,4 is 7 bytes.
        self.assertEqual(client.read_memory(0x1000, 6), b'\x00\x01\xab\xff\x05\x06')
        self.assertEqual(peer.commands[-2:], ['m1000,4', 'm1004,2'])

    def test_short_or_nonhex_memory_fails(self):
        for response in (b'00', b'000z', b'xxxx'):
            client, _, _ = self.client({'m1000,2': response})
            with self.subTest(response=response), self.assertRaises(RspError):
                client.read_memory(0x1000, 2)

    def test_address_length_validation(self):
        client, peer, _ = self.client()
        before = list(peer.sent)
        for addr, length in ((-1, 1), (2**64, 1), (0, 0), (0, -1), (2**64 - 1, 2), (True, 1), (0, False)):
            with self.subTest(addr=addr, length=length), self.assertRaises(ValueError):
                client.read_memory(addr, length)
        self.assertEqual(peer.sent, before)

    def test_native_step_only_current_breakpoint_restored_before_return(self):
        addr=0x1000;values=iter([addr,addr+4]);client,peer,_=self.client({'p2':lambda _:next(values).to_bytes(8,'little').hex().encode(),'s':b'T05thread:1;'})
        client.add_breakpoint(addr);client.add_breakpoint(0x2000);client.add_watchpoint(0x3000,8);at=len(peer.commands)
        self.assertEqual(client.step_over_breakpoint(addr),b'T05thread:1;')
        self.assertEqual(peer.commands[at:],['p2','z1,1000,1','s','p2','Z1,1000,1'])
        self.assertEqual(client._hardware_breakpoints,{(addr,1),(0x2000,1)})
        client.resume();self.assertEqual(peer.commands[-2:],['Z1,1000,1','c'])

    def test_native_step_rejects_unenrolled_noncurrent_and_raw_stepping(self):
        client,peer,_=self.client({'p2':b'0010000000000000'})
        with self.assertRaises(PolicyError):client.step_over_breakpoint(0x1000)
        client.add_breakpoint(0x2000)
        with self.assertRaises(PolicyError):client.step_over_breakpoint(0x2000)
        for packet in ['s','s1000','S05','vCont;s','M1000,1:90','P10=00','Z0,1000,1']:
            with self.subTest(packet=packet),self.assertRaises(PolicyError):client.request(packet)
        self.assertFalse(any(c=='s' or c.startswith('z1') for c in peer.commands))

    def test_native_step_no_progress_fails_closed_restoring_breakpoint(self):
        client,peer,_=self.client({'p2':b'0010000000000000','s':b'T05thread:1;'});client.add_breakpoint(0x1000)
        with self.assertRaisesRegex(RspError,'did not advance'):client.step_over_breakpoint(0x1000)
        self.assertEqual(peer.commands[-1],'Z1,1000,1');self.assertEqual(client._state,'stopped')
        self.assertEqual(peer.commands.count('s'),1);self.assertNotIn('c',peer.commands)

    def test_native_step_returns_interleaved_watch_with_guard_untouched(self):
        values=iter([0x1000,0x1004]);client,peer,_=self.client({'p2':lambda _:next(values).to_bytes(8,'little').hex().encode(),'s':b'T05watch:3000;thread:1;'})
        client.add_breakpoint(0x1000);client.add_watchpoint(0x3000,8)
        self.assertIn(b'watch:3000',client.step_over_breakpoint(0x1000))
        self.assertFalse(any(c.startswith('z2') for c in peer.commands));self.assertEqual(peer.commands[-1],'Z1,1000,1')

    def test_resume_does_not_receive_and_wait_stop_drains_console(self):
        client, peer, _ = self.client({'c': Wire(b'+' + frame(b'O6869') + frame(b'T05watch:2000;thread:1;'))})
        before = peer.recv_calls
        self.assertIsNone(client.resume())
        self.assertEqual(peer.recv_calls, before)
        with self.assertRaisesRegex(RspError, 'stopped target'):
            client.read_memory(0, 1)
        self.assertEqual(client.wait_stop(timeout=0.1), b'T05watch:2000;thread:1;')
        self.assertEqual(client.console, [b'hi'])
        with self.assertRaises(RspError):
            client.wait_stop()

    def test_request_c_is_asynchronous(self):
        client, peer, _ = self.client({'c': Wire(b'+' + frame(b'S05'))})
        before = peer.recv_calls
        self.assertEqual(client.request('c'), b'')
        self.assertEqual(before, peer.recv_calls)
        self.assertEqual(client.wait_stop(), b'S05')

    def test_wait_timeout_then_interrupt_no_guest_write(self):
        client, peer, _ = self.client()
        client.resume()
        with self.assertRaises(RspTimeout):
            client.wait_stop(timeout=0.01)
        self.assertEqual(client.interrupt(), b'T02thread:1;')
        self.assertIn(b'\x03', peer.sent)
        self.assertEqual(peer.commands[-1], 'c')
        with self.assertRaises(RspError):
            client.interrupt()

    def test_partial_stop_survives_wait_timeout(self):
        client, peer, _ = self.client({'c': Wire(b'+$T05thread')})
        client.resume()
        with self.assertRaises(RspTimeout):
            client.wait_stop()
        full = frame(b'T05thread:1;')
        peer.incoming.extend(full[len(b'$T05thread'):])
        self.assertEqual(client.wait_stop(), b'T05thread:1;')

    def test_stop_before_continue_ack_is_retained(self):
        client, _, _ = self.client({'c': Wire(frame(b'T05thread:1;') + b'+')})
        client.resume()
        self.assertEqual(client.wait_stop(), b'T05thread:1;')

    def test_exit_and_unexpected_stop_fail_explicitly(self):
        client, _, _ = self.client({'c': b'W00'})
        client.resume()
        with self.assertRaisesRegex(RspError, 'exited'):
            client.wait_stop()
        client, _, _ = self.client({'qC': b'S05'})
        with self.assertRaisesRegex(RspError, 'asynchronous stop'):
            client.request('qC')

    def test_g_prefix_uses_xml_p_for_tail(self):
        prefix = PACKED[:-16]
        client, peer, _ = self.client({'g': prefix, 'p4': b'0010000000000000'})
        self.assertEqual(client.registers(), VALUES)
        self.assertEqual(peer.commands[-2:], ['g', 'p4'])

    def test_register_number_gap_uses_explicit_p_only(self):
        control = CONTROL.replace(b'name="cr3"', b'name="cr3" regnum="9"')
        overrides = {}
        for number, name in enumerate(VALUES):
            overrides[f'p{9 if name == "cr3" else number:x}'] = VALUES[name].to_bytes(SIZES[name], 'little').hex().encode()
        client, peer, _ = self.client(overrides, {'control.xml': control})
        self.assertEqual(client.registers(), VALUES)
        self.assertNotIn('g', peer.commands)
        self.assertEqual(peer.commands[-1], 'p9')

    def test_single_register_uses_cached_xml_number(self):
        client, peer, _ = self.client({'p2': b'0000001000000000'})
        self.assertEqual(client.read_register('rip'), VALUES['rip'])
        self.assertEqual(peer.commands[-1], 'p2')
        self.assertNotIn('g', peer.commands)
        before = list(peer.sent)
        with self.assertRaises(RegisterUnavailable):
            client.read_register('not_advertised')
        self.assertEqual(peer.sent, before)

    def test_single_register_unavailable_and_running_are_errors(self):
        client, peer, _ = self.client({'p2': b'xxxxxxxxxxxxxxxx'})
        with self.assertRaises(RegisterUnavailable):
            client.read_register('rip')
        client.resume()
        before = list(peer.sent)
        with self.assertRaisesRegex(RspError, 'stopped target'):
            client.read_register('rip')
        self.assertEqual(peer.sent, before)

    def test_register_unavailable_and_malformed_sizes(self):
        for packet in (b'x' * 16 + PACKED[16:], PACKED[:15], PACKED + b'00'):
            client, _, _ = self.client({'g': packet})
            with self.subTest(packet=packet), self.assertRaises(RspError):
                client.registers()

    def test_unsupported_g_never_silently_falls_back(self):
        client, peer, _ = self.client({'g': b''})
        with self.assertRaisesRegex(RspError, 'unsupported'):
            client.registers()
        self.assertEqual(peer.commands[-1], 'g')

    def test_missing_register_connect_fails_without_guessed_offset(self):
        client, peer, _ = self.client(annexes={'control.xml': b'<feature/>'}, connect=False)
        with patch('rsp.socket.socket', return_value=peer):
            with self.assertRaisesRegex(RegisterUnavailable, 'cr3'):
                client.connect()
        self.assertTrue(peer.closed)
        self.assertNotIn('g', peer.commands)

    def test_xml_chunks_and_escaping(self):
        core = CORE.replace(b'<feature name=', b'<!-- #$}* -->\n<feature name=') + b' ' * 2200
        client, peer, _ = self.client(annexes={'core.xml': core})
        self.assertEqual(client.target_xml['core.xml'], core)
        self.assertGreater(sum(command.startswith('qXfer:features:read:core.xml:') for command in peer.commands), 1)

    def test_captured_qemu_target_without_xi_namespace(self):
        target = b'<?xml version="1.0"?><!DOCTYPE target SYSTEM "gdb-target.dtd"><target><architecture>i386:x86-64</architecture><xi:include href="i386-64bit.xml"/></target>'
        client, _, _ = self.client(annexes={'target.xml': target, 'i386-64bit.xml': CORE.replace(b'</feature>', CONTROL.split(b'>', 1)[1])})
        self.assertEqual(client.register_info['cr3'].number, 4)
        self.assertEqual(client.registers(), VALUES)
        self.assertEqual(client.target_xml['target.xml'], target)

    def test_unbound_xi_does_not_allow_unknown_prefix_or_bad_include(self):
        for changed in (TARGET.replace(b' xmlns:xi="http://www.w3.org/2001/XInclude"', b'').replace(b'core.xml', b'../core.xml'),
                        TARGET.replace(b' xmlns:xi="http://www.w3.org/2001/XInclude"', b'').replace(b'<architecture>', b'<bad:architecture>').replace(b'</architecture>', b'</bad:architecture>')):
            with self.subTest(target=changed), self.assertRaises(RspError):
                self.client(annexes={'target.xml': changed})

    def test_xml_cycles_entities_bad_registers_and_wrong_architecture(self):
        bad_cases = (
            {'control.xml': b'<feature xmlns:xi="http://www.w3.org/2001/XInclude"><xi:include href="target.xml"/></feature>'},
            {'control.xml': b'<!DOCTYPE feature [<!ENTITY x "a">]><feature/>'},
            {'control.xml': b'<feature><reg name="cr3" bitsize="7"/></feature>'},
            {'control.xml': b'<feature><reg name="cr3" bitsize="64" regnum="0"/></feature>'},
            {'control.xml': b'<feature><reg name="rax" bitsize="64"/></feature>'},
            {'control.xml': b'<feature'},
            {'target.xml': TARGET.replace(b'i386:x86-64', b'aarch64')},
            {'target.xml': TARGET.replace(b'core.xml', b'../core.xml')},
        )
        for annexes in bad_cases:
            with self.subTest(annexes=annexes):
                client, peer, _ = self.client(annexes=annexes, connect=False)
                with patch('rsp.socket.socket', return_value=peer), self.assertRaises(RspError):
                    client.connect()
                self.assertTrue(peer.closed)

    def test_missing_feature_capability_and_invalid_packet_size(self):
        for response in (b'PacketSize=1000', b'PacketSize=zz;qXfer:features:read+', b'PacketSize=1;qXfer:features:read+'):
            client, peer, _ = self.client({'qSupported:qXfer:features:read+': response}, connect=False)
            with self.subTest(response=response), patch('rsp.socket.socket', return_value=peer), self.assertRaises(RspError):
                client.connect()
            self.assertTrue(peer.closed)

    def test_physical_read_restores_virtual_view(self):
        client, peer, _ = self.client({'m1000,2': b'abcd'})
        self.assertEqual(client.read_physical_memory(0x1000, 2), b'\xab\xcd')
        self.assertEqual(peer.commands[-4:], ['qqemu.PhyMemMode', 'Qqemu.PhyMemMode:1', 'm1000,2', 'Qqemu.PhyMemMode:0'])

    def test_physical_read_error_still_restores_virtual_view(self):
        client, peer, _ = self.client({'m1000,2': b'E14'})
        with self.assertRaises(RspError):
            client.read_physical_memory(0x1000, 2)
        self.assertEqual(peer.commands[-1], 'Qqemu.PhyMemMode:0')

    def test_physical_mode_unsupported_and_restore_errors_are_fatal(self):
        client, peer, _ = self.client({'qqemu.PhyMemMode': b''})
        with self.assertRaises(RspError):
            client.read_physical_memory(0, 1)
        self.assertEqual(peer.commands[-1], 'qqemu.PhyMemMode')
        client, peer, _ = self.client({'m0,1': b'00', 'Qqemu.PhyMemMode:0': b'E22'})
        with self.assertRaises(RspError):
            client.read_physical_memory(0, 1)
        self.assertEqual(peer.commands[-1], 'Qqemu.PhyMemMode:0')

    def test_raw_logs_append_hex_monotonic_and_close_is_idempotent(self):
        client, peer, log = self.client()
        records = [json.loads(line) for line in log.read_text().splitlines()]
        self.assertTrue(all(isinstance(record['monotonic_ns'], int) for record in records))
        self.assertEqual([record['monotonic_ns'] for record in records], sorted(record['monotonic_ns'] for record in records))
        self.assertTrue(any(record['event'] == 'tx' and record['raw_hex'].startswith('24') for record in records))
        self.assertTrue(any(record['event'] == 'rx' for record in records))
        size = log.stat().st_size
        client.close()
        client.close()
        self.assertTrue(peer.closed)
        self.assertGreater(log.stat().st_size, size)
        with self.assertRaises(RspError):
            client.request('g')
        # A new client appends, preserving the previous transcript.
        another = RspClient(peer.connected, str(log))
        self.addCleanup(another.close)
        with patch('rsp.socket.socket', return_value=FakeSocket()):
            another.connect()
        self.assertGreater(log.stat().st_size, size)

    def test_invalid_socket_path_and_timeout(self):
        for path in ('', '\x00abstract', b'/tmp/bytes'):
            with self.subTest(path=path), self.assertRaises(ValueError):
                RspClient(path, '/unused')
        for timeout in (0, -1, float('inf'), float('nan')):
            with self.subTest(timeout=timeout), self.assertRaises(ValueError):
                RspClient('/unused', '/unused', timeout)


if __name__ == '__main__':
    unittest.main()
