"""Deterministic host-only atomic receive tests; no network, guest or sleeps.

Run alongside the parent's combined suite. The inherited ClientTests use their
unchanged scripted transport, with only the constructed client class replaced.
"""
from collections import deque
from dataclasses import dataclass
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))

import rsp as B
import test_rsp as original_tests
import reproduce_before
from atomic_rsp import AtomicReceiveMixin, AtomicRspClient


POLL = 0.1
STOP = reproduce_before.STOP
FRAME = reproduce_before.FRAME
frame = original_tests.frame


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now

    def monotonic_ns(self):
        return int(self.now * 1_000_000_000)


@dataclass
class Receive:
    value: object
    elapsed: float = 0.0


class ByteTransport:
    """Only scripted bytes; exhaustion spends the current receive budget."""
    def __init__(self, clock, receives=(), send_result=None, on_send=None):
        self.clock = clock
        self.receives = deque(receives)
        self.send_result = send_result
        self.on_send = on_send
        self.timeout = None
        self.timeouts = []
        self.recv_calls = 0
        self.sent = []
        self.possibly_accepted = []
        self.closed = False

    def settimeout(self, value):
        self.timeout = value
        self.timeouts.append(value)

    def recv(self, count):
        self.recv_calls += 1
        if not self.receives:
            self.clock.now += self.timeout
            raise TimeoutError('synthetic empty receive')
        step = self.receives.popleft()
        if step.elapsed > self.timeout + 1e-12:
            raise AssertionError('script exceeds the original receive budget')
        self.clock.now += step.elapsed
        if isinstance(step.value, BaseException):
            raise step.value
        if len(step.value) > count:
            raise AssertionError('script exceeds recv byte count')
        return step.value

    def sendall(self, data):
        data = bytes(data)
        self.sent.append(data)
        # Even a raised error may follow peer acceptance: the client must not
        # infer non-delivery and retry this ACK.
        self.possibly_accepted.append(data)
        if self.on_send is not None:
            self.on_send(data)
        if isinstance(self.send_result, BaseException):
            raise self.send_result
        return self.send_result

    def close(self):
        self.closed = True


class AtomicBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        for target, replacement in (
            ('monotonic', self.clock),
            ('monotonic_ns', self.clock.monotonic_ns),
            ('sleep', lambda *_: self.fail('receive polling must not sleep')),
        ):
            patcher = patch.object(B.time, target, replacement)
            patcher.start()
            self.addCleanup(patcher.stop)

    def client(self, receives=(), *, send_result=None, on_send=None):
        peer = ByteTransport(self.clock, receives, send_result, on_send)
        client = AtomicRspClient('/synthetic/no-connect.sock', '/unused', timeout=5.0)
        self.assertIsInstance(client, AtomicReceiveMixin)
        client._socket = peer
        client._state = 'running'
        client._continue_ack = False
        client._log = io.StringIO()
        self.addCleanup(client.close)
        self.addCleanup(self.assert_host_only, client, peer)
        return client, peer

    def assert_host_only(self, client, peer):
        self.assertEqual(client.timeout, 5.0)
        self.assertTrue(all(data in (b'+', b'-') for data in peer.sent), peer.sent)
        self.assertTrue(all(0 < timeout <= POLL + 1e-12 for timeout in peer.timeouts), peer.timeouts)

    def records(self, client, event=None):
        records = [json.loads(line) for line in client._log.getvalue().splitlines()]
        return records if event is None else [row for row in records if row['event'] == event]

    def assert_dispatches(self, client, payloads):
        self.assertEqual([bytes.fromhex(row['payload_hex']) for row in self.records(client, 'packet_rx')], payloads)

    def assert_pending(self, client, wire=FRAME, payload=STOP):
        self.assertEqual(bytes(client._buffer), wire)
        self.assertEqual(client._pending_wire_frame, {
            'wire_length': len(frame(payload)), 'payload': payload, 'ack_state': 'PENDING',
        })
        self.assertIsNone(client._wire_failure)
        self.assertIsNone(client.last_stop)
        self.assertEqual(client._state, 'running')
        self.assert_dispatches(client, [])

    def assert_fatal(self, operation):
        with self.assertRaises(B.RspError) as caught:
            operation()
        self.assertNotIsInstance(caught.exception, B.RspTimeout)
        return caught.exception

    def assert_terminal_no_retry(self, client, peer):
        self.assertIsInstance(client._wire_failure, str)
        self.assertTrue(client._wire_failure)
        failure = client._wire_failure
        sent = list(peer.sent)
        receives = peer.recv_calls
        buffered = bytes(client._buffer)
        for operation in (
            client._require_socket,
            lambda: client._event(client._deadline(POLL)),
            lambda: client.wait_stop(timeout=POLL),
            lambda: client.request('qC'),
            client.interrupt,
        ):
            self.assert_fatal(operation)
        self.assertEqual(client._wire_failure, failure)
        self.assertEqual(peer.sent, sent)
        self.assertEqual(peer.recv_calls, receives)
        self.assertEqual(bytes(client._buffer), buffered)
        self.assert_dispatches(client, [])
        self.assertIsNone(client.last_stop)

    def test_full_and_every_fragmentation_deliver_once(self):
        splits = [(FRAME,)]
        splits.extend((FRAME[:cut], FRAME[cut:]) for cut in range(1, len(FRAME)))
        splits.append(tuple(bytes([byte]) for byte in FRAME))
        for chunks in splits:
            with self.subTest(chunks=chunks):
                client, peer = self.client([Receive(chunk) for chunk in chunks])
                self.assertEqual(client.wait_stop(timeout=POLL), STOP)
                self.assertEqual(peer.sent, [b'+'])
                self.assertEqual(peer.recv_calls, len(chunks))
                self.assertEqual(client.last_stop, STOP)
                self.assertEqual(client._state, 'stopped')
                self.assertEqual(client._buffer, b'')
                self.assertIsNone(client._pending_wire_frame)
                self.assert_dispatches(client, [STOP])
                self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
                with self.assertRaises(B.RspTimeout):
                    client._event(client._deadline(POLL))
                self.assertEqual(peer.sent, [b'+'])
                self.assert_dispatches(client, [STOP])

    def test_complete_frame_at_deadline_retains_until_next_original_poll(self):
        client, peer = self.client([Receive(FRAME, POLL)])
        deadline = client._deadline(POLL)
        with self.assertRaises(B.RspTimeout):
            client.wait_stop(timeout=POLL)
        self.assertEqual(self.clock.now, deadline)
        self.assert_pending(client)
        pending = client._pending_wire_frame
        self.assertEqual(peer.sent, [])
        self.assertEqual(self.records(client, 'tx'), [])
        self.assertEqual(client.wait_stop(timeout=POLL), STOP)
        self.assertEqual(peer.sent, [b'+'])
        self.assertEqual(peer.recv_calls, 1)
        self.assertEqual(pending['ack_state'], 'ACKNOWLEDGED')
        self.assertIsNone(client._pending_wire_frame)
        self.assertEqual(client._buffer, b'')
        self.assert_dispatches(client, [STOP])

    def test_every_partial_frame_survives_poll_expiry(self):
        for cut in range(1, len(FRAME)):
            with self.subTest(cut=cut):
                client, peer = self.client([Receive(FRAME[:cut], POLL), Receive(FRAME[cut:])])
                with self.assertRaises(B.RspTimeout):
                    client.wait_stop(timeout=POLL)
                self.assertEqual(bytes(client._buffer), FRAME[:cut])
                self.assertIsNone(client._pending_wire_frame)
                self.assertIsNone(client._wire_failure)
                self.assertEqual(peer.sent, [])
                self.assert_dispatches(client, [])
                self.assertEqual(client.wait_stop(timeout=POLL), STOP)
                self.assertEqual(peer.sent, [b'+'])
                self.assertEqual(peer.recv_calls, 2)
                self.assert_dispatches(client, [STOP])

    def test_repeated_expired_preflight_preserves_pending_identity_and_tail(self):
        tail = frame(b'O6869') + frame(b'S02')
        client, peer = self.client([Receive(FRAME + tail, POLL)])
        deadline = client._deadline(POLL)
        with self.assertRaises(B.RspTimeout):
            client._event(deadline)
        self.assert_pending(client, FRAME + tail)
        pending = client._pending_wire_frame
        for _ in range(3):
            with self.assertRaises(B.RspTimeout):
                client._event(deadline)
            self.assertIs(client._pending_wire_frame, pending)
            self.assert_pending(client, FRAME + tail)
        self.assertEqual(peer.sent, [])
        self.assertEqual(peer.recv_calls, 1)
        self.assertEqual(client.wait_stop(timeout=POLL), STOP)
        self.assertEqual(bytes(client._buffer), tail)
        self.assertEqual(client._reply(client._deadline(POLL)), b'S02')
        self.assertEqual(client.console, [b'hi'])
        self.assertEqual(peer.sent, [b'+', b'+', b'+'])
        self.assertEqual(peer.recv_calls, 1)
        self.assertEqual(client._buffer, b'')
        self.assert_dispatches(client, [STOP, b'O6869', b'S02'])

    def test_expiry_after_decode_before_ack_does_not_redecode(self):
        client, peer = self.client()
        client._buffer.extend(FRAME)
        deadline = client._deadline(POLL)

        def decode_then_expire(encoded):
            payload = B.RspClient._decode(encoded)
            self.clock.now = deadline
            return payload

        with patch.object(client, '_decode', side_effect=decode_then_expire) as decoder:
            with self.assertRaises(B.RspTimeout):
                client._event(deadline)
            self.assert_pending(client)
            self.assertEqual(peer.sent, [])
            self.assertEqual(client.wait_stop(timeout=POLL), STOP)
            self.assertEqual(decoder.call_count, 1)
        self.assertEqual(peer.recv_calls, 0)
        self.assertEqual(peer.sent, [b'+'])
        self.assert_dispatches(client, [STOP])

    def test_successful_ack_crossing_deadline_commits_without_second_check(self):
        snapshots = []
        client, peer = self.client([Receive(FRAME)])
        deadline = client._deadline(POLL)

        def ack_at_boundary(raw):
            self.assertEqual(raw, b'+')
            self.assert_pending(client)
            snapshots.append(client._pending_wire_frame)
            self.assertEqual(self.records(client)[-1]['event'], 'tx')
            self.assertEqual(self.records(client)[-1]['raw_hex'], '2b')
            self.clock.now = deadline + POLL

        peer.on_send = ack_at_boundary
        self.assertEqual(client.wait_stop(timeout=POLL), STOP)
        self.assertGreater(self.clock.now, deadline)
        self.assertEqual(snapshots[0]['ack_state'], 'ACKNOWLEDGED')
        self.assertEqual(peer.sent, [b'+'])
        self.assertEqual(client._buffer, b'')
        self.assertIsNone(client._pending_wire_frame)
        self.assertEqual(client._state, 'stopped')
        self.assert_dispatches(client, [STOP])

    def test_coalesced_continue_ack_console_and_stop(self):
        for chunks in ((b'+' + frame(b'O6869') + FRAME,),
                       (b'+', frame(b'O6869'), FRAME)):
            with self.subTest(chunks=chunks):
                client, peer = self.client([Receive(chunk) for chunk in chunks])
                client._continue_ack = True
                self.assertEqual(client.wait_stop(timeout=POLL), STOP)
                self.assertFalse(client._continue_ack)
                self.assertEqual(client.console, [b'hi'])
                self.assertEqual(peer.sent, [b'+', b'+'])
                self.assertEqual(client._buffer, b'')
                self.assert_dispatches(client, [b'O6869', STOP])

    def test_coalesced_ack_stop_console_and_second_packet_consumed_once(self):
        tail = frame(b'O6869') + frame(b'S02')
        client, peer = self.client([Receive(b'+' + FRAME + tail)])
        client._continue_ack = True
        self.assertEqual(client.wait_stop(timeout=POLL), STOP)
        self.assertEqual(bytes(client._buffer), tail)
        self.assertEqual(client._reply(client._deadline(POLL)), b'S02')
        self.assertEqual(client.console, [b'hi'])
        self.assertEqual(peer.sent, [b'+', b'+', b'+'])
        self.assertEqual(peer.recv_calls, 1)
        self.assertEqual(client._buffer, b'')
        self.assert_dispatches(client, [STOP, b'O6869', b'S02'])
        with self.assertRaises(B.RspTimeout):
            client._event(client._deadline(POLL))
        self.assertEqual(peer.sent, [b'+', b'+', b'+'])
        self.assert_dispatches(client, [STOP, b'O6869', b'S02'])

    def test_standalone_ack_events_are_consumed_without_acknowledging_them(self):
        client, peer = self.client([Receive(b'+-+')])
        for expected in (b'+', b'-', b'+'):
            self.assertEqual(client._event(client._deadline(POLL)), expected)
        self.assertEqual(peer.sent, [])
        self.assertEqual(peer.recv_calls, 1)
        self.assertEqual(client._buffer, b'')
        self.assertIsNone(client._pending_wire_frame)
        self.assert_dispatches(client, [])

    def test_empty_receive_poll_is_benign_and_does_not_send(self):
        client, peer = self.client()
        for _ in range(3):
            start = self.clock.now
            with self.assertRaises(B.RspTimeout):
                client.wait_stop(timeout=POLL)
            self.assertAlmostEqual(self.clock.now - start, POLL)
            self.assertEqual(client._buffer, b'')
            self.assertIsNone(client._pending_wire_frame)
            self.assertIsNone(client._wire_failure)
            self.assertEqual(client._state, 'running')
        self.assertEqual(peer.sent, [])
        self.assertEqual(peer.recv_calls, 3)
        self.assert_dispatches(client, [])

    def test_bad_checksum_naks_once_then_fails_closed(self):
        client, peer = self.client([Receive(b'$T05thread:01;#00')])
        error = self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
        self.assertIn('checksum mismatch', str(error))
        self.assertEqual(peer.sent, [b'-'])
        self.assert_terminal_no_retry(client, peer)

    def test_bad_checksum_at_deadline_is_fatal_without_nak_retry(self):
        client, peer = self.client([Receive(b'$T05thread:01;#00', POLL)])
        self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
        self.assertEqual(peer.sent, [])
        self.assert_terminal_no_retry(client, peer)

    def test_malformed_framing_and_nonhex_checksum_fail_closed(self):
        for raw in (b'garbage', b'%Stop:T05#00', b'\x00', b'$T05#zz', b'$T05#0g'):
            for elapsed in (0.0, POLL):
                with self.subTest(raw=raw, elapsed=elapsed):
                    client, peer = self.client([Receive(raw, elapsed)])
                    self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
                    expected = [b'-'] if raw.startswith(b'$') and elapsed == 0 else []
                    self.assertEqual(peer.sent, expected)
                    self.assert_terminal_no_retry(client, peer)

    def test_decode_errors_never_positive_ack_even_at_deadline(self):
        for encoded in (b'}', b'* ', b'A*', b'A*\x1f', b'A*\x7f', b'A*$'):
            for elapsed in (0.0, POLL):
                with self.subTest(encoded=encoded, elapsed=elapsed):
                    client, peer = self.client([Receive(frame(encoded, encoded=True), elapsed)])
                    self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
                    self.assertEqual(peer.sent, [])
                    self.assertIsNone(client._pending_wire_frame)
                    self.assert_terminal_no_retry(client, peer)

    def test_valid_escaped_and_run_length_payload_retained_decoded(self):
        encoded = b'}\x03}\x04}]}' + bytes([ord('*') ^ 0x20]) + b'A* '
        payload = b'#$}*AAAA'
        wire = frame(encoded, encoded=True)
        client, peer = self.client([Receive(wire, POLL)])
        with self.assertRaises(B.RspTimeout):
            client._event(client._deadline(POLL))
        self.assertEqual(bytes(client._buffer), wire)
        self.assertEqual(client._pending_wire_frame, {
            'wire_length': len(wire), 'payload': payload, 'ack_state': 'PENDING',
        })
        self.assertEqual(peer.sent, [])
        self.assert_dispatches(client, [])
        self.assertEqual(client._event(client._deadline(POLL)), payload)
        self.assertEqual(peer.sent, [b'+'])
        self.assertEqual(peer.recv_calls, 1)
        self.assertIsNone(client._pending_wire_frame)
        self.assert_dispatches(client, [payload])

    def test_eof_empty_or_mid_frame_is_fatal_not_a_poll_timeout(self):
        for prefix in (b'', FRAME[:1], FRAME[:-1]):
            for elapsed in (0.0, POLL):
                with self.subTest(prefix=prefix, elapsed=elapsed):
                    receives = ([Receive(prefix)] if prefix else []) + [Receive(b'', elapsed)]
                    client, peer = self.client(receives)
                    self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
                    self.assertEqual(peer.sent, [])
                    self.assert_terminal_no_retry(client, peer)

    def test_receive_oserror_fails_closed_without_ack(self):
        client, peer = self.client([Receive(OSError('synthetic receive failure'))])
        self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
        self.assertEqual(peer.sent, [])
        self.assert_terminal_no_retry(client, peer)

    def test_ambiguous_ack_failure_never_retries_or_dispatches(self):
        outcomes = (
            TimeoutError('synthetic ACK timeout after possible acceptance'),
            OSError('synthetic ACK transport failure after possible acceptance'),
            1, 0, False, b'+',
        )
        for outcome in outcomes:
            with self.subTest(outcome=repr(outcome)):
                client, peer = self.client([Receive(FRAME), Receive(FRAME)], send_result=outcome)

                def attempted_ack(raw):
                    self.assertEqual(raw, b'+')
                    self.assert_pending(client)
                    self.assertEqual(self.records(client)[-1]['event'], 'tx')
                    self.assertEqual(self.records(client)[-1]['raw_hex'], '2b')

                peer.on_send = attempted_ack
                self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
                self.assertEqual(peer.sent, [b'+'])
                self.assertEqual(peer.possibly_accepted, [b'+'])
                self.assertEqual(bytes(client._buffer), FRAME)
                self.assertEqual(client._pending_wire_frame['ack_state'], 'PENDING')
                self.assertEqual(peer.recv_calls, 1)
                self.assertEqual(len(peer.receives), 1)
                self.assert_terminal_no_retry(client, peer)
                self.assertEqual(len(self.records(client, 'tx')), 1)

    def test_ambiguous_nak_timeout_is_terminal_without_retry(self):
        client, peer = self.client([Receive(b'$T05thread:01;#00')],
                                   send_result=TimeoutError('synthetic NAK delivery uncertainty'))
        self.assert_fatal(lambda: client.wait_stop(timeout=POLL))
        self.assertEqual(peer.sent, [b'-'])
        self.assert_terminal_no_retry(client, peer)

    def test_exact_retained_old_reproducer_now_requires_delivery(self):
        with tempfile.TemporaryDirectory(prefix='rsp-atomic-reproduction-') as directory:
            result = reproduce_before.exercise(AtomicRspClient, directory, 'reproduction-after-unit')
        reproduce_before.assert_delivery(result)
        self.assertEqual(result['client_type'], 'AtomicRspClient')
        self.assertEqual(result['wire_frame_hex'], FRAME.hex())
        self.assertTrue(result['valid_checksum'])
        self.assertFalse(result['old_loss_signature'])
        self.assertEqual(len(result['polls']), 2)
        first, second = result['polls']
        self.assertTrue(first['error'].startswith('RspTimeout:'), first)
        self.assertEqual(first['buffer_hex'], FRAME.hex())
        self.assertEqual(first['ACK_count'], 0)
        self.assertEqual(first['state'], 'running')
        self.assertEqual(second['returned_stop'], STOP.decode())
        self.assertEqual(second['ACK_count'], 1)
        self.assertEqual(second['buffer_hex'], '')
        self.assertEqual([poll['timeout'] for poll in result['polls']], [POLL, POLL])
        self.assertEqual(result['receive_calls'], 1)
        self.assertEqual(result['sent_raw_hex'], ['2b'])
        self.assertEqual(result['positive_ACK_count'], 1)
        self.assertEqual(result['decoded_stop_dispatch_count'], 1)
        self.assertTrue(result['no_guest_control_packet'])
        self.assertTrue(result['no_timeout_increase'])


class AtomicOriginalClientTests(original_tests.ClientTests):
    """Every existing ClientTests method, constructing the new concrete class."""
    def setUp(self):
        super().setUp()
        patcher = patch.object(original_tests, 'RspClient', AtomicRspClient)
        patcher.start()
        self.addCleanup(patcher.stop)

    def client(self, *args, **kwargs):
        client, peer, log = super().client(*args, **kwargs)
        self.assertIs(type(client), AtomicRspClient)
        return client, peer, log


if __name__ == '__main__':
    unittest.main()
