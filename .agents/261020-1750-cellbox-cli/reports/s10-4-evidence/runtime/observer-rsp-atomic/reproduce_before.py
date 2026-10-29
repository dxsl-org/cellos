#!/usr/bin/env python3
"""Deterministic synthetic old-host failure; no network, VM or guest controls."""
import hashlib
import json
from pathlib import Path
import sys
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
RUNTIME = HERE.parent
sys.path.insert(0, str(RUNTIME))
import rsp as B

FRAME = b'$T05thread:01;#07'
STOP = b'T05thread:01;'


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


class BoundaryPeer:
    """The first complete frame arrives exactly when that .1s poll expires."""
    def __init__(self, clock):
        self.clock = clock
        self.timeout = 0.1
        self.receives = 0
        self.sent = []
        self.closed = False

    def settimeout(self, value):
        self.timeout = value

    def recv(self, size):
        self.receives += 1
        self.clock.now += self.timeout
        if self.receives == 1:
            return FRAME
        raise TimeoutError('synthetic empty receive poll')

    def sendall(self, payload):
        self.sent.append(bytes(payload))

    def close(self):
        self.closed = True


def exercise(client_type, directory, label):
    directory = Path(directory)
    clock = Clock()
    peer = BoundaryPeer(clock)
    log_path = directory / f'{label}-rsp.jsonl'
    client = client_type('/synthetic/no-connect.sock', log_path, timeout=5)
    client._socket = peer
    client._state = 'running'
    client._continue_ack = False
    client._log = log_path.open('w')
    polls = []
    with patch.object(B.time, 'monotonic', clock):
        try:
            for ordinal in (1, 2):
                poll = {'ordinal': ordinal, 'start': clock.now, 'timeout': 0.1}
                try:
                    reply = client.wait_stop(timeout=0.1)
                    poll['returned_stop'] = reply.decode()
                except Exception as error:
                    poll['error'] = f'{type(error).__name__}: {error}'
                poll.update(end=clock.now, buffer_hex=client._buffer.hex(), state=client._state,
                            ACK_count=peer.sent.count(b'+'))
                polls.append(poll)
                if 'returned_stop' in poll:
                    break
            state = client._state
            last_stop = client.last_stop.decode() if client.last_stop else None
            buffer_hex = client._buffer.hex()
        finally:
            client.close()
    journal = [json.loads(line) for line in log_path.read_text().splitlines()]
    delivered = sum(row['event'] == 'packet_rx' and row['payload_hex'] == STOP.hex() for row in journal)
    result = {'scope': 'Synthetic deterministic fake clock/transport; no candidate execution',
              'client_type': client_type.__name__, 'original_rsp_sha256': hashlib.sha256((RUNTIME / 'rsp.py').read_bytes()).hexdigest(),
              'wire_frame_hex': FRAME.hex(), 'valid_checksum': sum(STOP) % 256 == 7,
              'polls': polls, 'receive_calls': peer.receives, 'sent_raw_hex': [data.hex() for data in peer.sent],
              'final_state_before_close': state, 'last_stop': last_stop, 'final_buffer_hex': buffer_hex,
              'positive_ACK_count': peer.sent.count(b'+'), 'decoded_stop_dispatch_count': delivered,
              'stop_received_across_original_polls': last_stop == STOP.decode() and state == 'stopped',
              'no_guest_control_packet': all(data in (b'+', b'-') for data in peer.sent),
              'old_loss_signature': bool(polls and 'RSP deadline expired' in polls[0].get('error', '') and
                  polls[0]['buffer_hex'] == '' and polls[0]['ACK_count'] == 0 and last_stop is None),
              'no_timeout_increase': all(poll['timeout'] == 0.1 for poll in polls)}
    (directory / f'{label}.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def assert_delivery(result):
    assert result['stop_received_across_original_polls'], 'valid stop was silently lost across original .1s polls'
    assert result['positive_ACK_count'] == result['decoded_stop_dispatch_count'] == 1
    assert result['no_guest_control_packet'] and result['no_timeout_increase']


def main():
    result = exercise(B.RspClient, HERE, 'reproduction-before')
    print(json.dumps(result, indent=2), flush=True)
    # This unchanged-host regression assertion must fail before any fix is authored.
    assert_delivery(result)


if __name__ == '__main__':
    main()
