#!/usr/bin/env python3
"""S10-9.0a offline classification of the S10-8 headless stop against the retained RSP clients (no VM, no QEMU, no guest, no network).

Question (S10-8 note, marked [INFERENCE]): was the cand-headless E stop caused by the retained NON-atomic RSP client losing a complete
stop packet at a poll boundary (the S10-4.14 failure mode), i.e. is the receive-race attribution confirmed or withdrawn?

Method (everything is replayed from retained bytes; no new guest input exists):

  1. `analyze_journal`    stream-reassembles every `rx` chunk of a retained `rsp-traffic.jsonl` into RSP frames and decides, per frame, whether
                          it was dispatched (a `packet_rx` of the same payload follows before the next `rx` chunk) and ACKed (`tx '+'`).
  2. `census`             the same analysis over EVERY retained RSP journal: where does the signature "complete checksum-valid stop frame,
                          no ACK, no dispatch, silence until a shutdown interrupt" occur, and with which client class.
  3. `final_segment`      the last resume segment of the S10-8 headless journal (tx `$c#63`, `state running`, rx `+`, rx frame, silence).
  4. `replay_segment`     that segment is replayed through the retained non-atomic headless client (`RecurringClient`, what S10-8 used) and
                          the atomic one (`AtomicHeadlessClient`, what S10-9 uses) over a deterministic fake clock/socket that reproduces
                          CPython's `socket.settimeout` poll semantics (millisecond-ceiling `poll`, a final scan after the timeout wake)
                          and the observer's `wait_stop(timeout=.1)` loop, swept over the two host-timing parameters the journal cannot
                          record (timeout-wake latency L, inter-poll overhead e).  Loss/delivery, the journal tail and the shutdown
                          interrupt outcome are compared with the retained S10-8 journal.
  5. `calibrate_host_poll` (CLI only) measures the real per-poll cycle of `RspClient.wait_stop(timeout=.1)` on this host over a socketpair
                          (60 polls, no VM) to bound the plausible overhead and compare it with the overhead the journals imply.
  6. `fixtures`           the retained `observer-rsp-atomic/reproduce_before.py` before/after fixtures (`exercise`, redirected to a temporary
                          directory: the retained script's `main` writes into its own archive and is never run) against `rsp.RspClient`,
                          `AtomicRspClient` and the two headless clients.

    PYTHONDONTWRITEBYTECODE=1 python3 -B s109_rsp_replay.py [--write]

`--write` stores the evidence under reports/s10-9-evidence/s10-9.0a-rsp-classification/ (new directory; fails if it already exists).
"""
import io
import json
import math
import statistics
import sys
import tempfile
import time
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_env as ENV  # noqa: E402
import s107_retained as RET  # noqa: E402  (puts every retained archive on sys.path)
import rsp as B  # noqa: E402  (retained runtime/rsp.py)
import atomic_rsp  # noqa: E402  (retained observer-rsp-atomic/atomic_rsp.py)
import reproduce_before as REPRO  # noqa: E402  (retained fixture module; importing defines functions/classes only, main is guarded)

S108_HEADLESS_JOURNAL = ENV.REPORTS / 's10-8-evidence/measurement-runs/cand-headless/evidence/rsp-traffic.jsonl'
S1013_COORDINATED_JOURNAL = ENV.RUNTIME / 'observer-input-coordinated/desktop/evidence/rsp-traffic.jsonl'
POLL_S = 0.1                        # Observer.run: self.client.wait_stop(timeout=.1)
NS = 1_000_000_000


# ------------------------------------------------------------------ 1. journal analysis
def iter_rows(path):
    with Path(path).open() as handle:
        for line in handle:
            line = line.strip()
            if line:
                try:
                    yield json.loads(line)
                except ValueError:
                    yield {'event': 'unparseable'}


def load_rows(path):
    return list(iter_rows(path))


def checksum_ok(payload, checksum):
    try:
        return sum(payload) % 256 == int(checksum, 16)
    except ValueError:
        return False


def event_tuple(row):
    return (row.get('event'), row.get('raw_hex') or row.get('payload_hex') or row.get('state'))


class _Scanner:
    """Streaming frame reassembly over `rx` chunks (constant memory: the retained headless journals have >10^6 rows).

    A frame in the chunk at row i is *dispatched* when a `packet_rx` with the same decoded payload is journalled before the next `rx`
    chunk (its ACK is a `tx '+'` in that window).  Works for the non-atomic client (rx, tx +, packet_rx) and the atomic client
    (rx, wire_frame_pending, tx +, packet_rx).
    """
    KEEP = 5

    def __init__(self):
        self.buffer = bytearray()
        self.rows = self.chunks = self.frames = self.dispatched = self.atomic_wire_events = 0
        self.window_frames, self.window_dispatched, self.window_acks = [], [], 0
        self.window_rows, self.window_len = [], 0      # rows that follow the current rx chunk (first 4 kept), for undispatched frames
        self.undispatched = []
        self.undispatched_total = 0

    def _close_window(self):
        for frame in self.window_frames:
            if frame['payload_hex'] in self.window_dispatched:
                self.window_dispatched.remove(frame['payload_hex'])
                self.dispatched += 1
                continue
            frame['dispatched'] = False
            frame['ack_in_window'] = self.window_acks > 0
            frame['rows_after'] = list(self.window_rows)
            frame['rows_after_count'] = self.window_len
            self.undispatched_total += 1
            if len(self.undispatched) < self.KEEP:
                self.undispatched.append(frame)
        self.window_frames, self.window_dispatched, self.window_acks = [], [], 0
        self.window_rows, self.window_len = [], 0

    def feed(self, row):
        index = self.rows
        self.rows += 1
        event = row.get('event')
        if event != 'rx' and self.chunks:
            self.window_len += 1
            if len(self.window_rows) < 4:
                self.window_rows.append({'event': event_tuple(row), 'monotonic_ns': row.get('monotonic_ns')})
        if event in ('wire_frame_pending', 'wire_terminal'):
            self.atomic_wire_events += 1
        if event == 'rx':
            self._close_window()
            self.chunks += 1
            self.buffer += bytes.fromhex(row['raw_hex'])
            while self.buffer:
                if self.buffer[0] in b'+-':
                    del self.buffer[0]
                    continue
                if self.buffer[0] != ord('$'):
                    break
                end = self.buffer.find(b'#', 1)
                if end == -1 or len(self.buffer) < end + 3:
                    break
                body, checksum = bytes(self.buffer[1:end]), bytes(self.buffer[end + 1:end + 3])
                valid = checksum_ok(body, checksum)
                try:
                    decoded = B.RspClient._decode(body) if valid else b''
                except B.RspError:
                    decoded = b''
                self.frames += 1
                self.window_frames.append({'row': index, 'monotonic_ns': row['monotonic_ns'], 'wire': bytes(self.buffer[:end + 3]).decode('latin1'),
                                           'checksum_valid': valid, 'payload_hex': decoded.hex(), 'payload': decoded.decode('latin1')})
                del self.buffer[:end + 3]
        elif event == 'packet_rx':
            self.window_dispatched.append(row.get('payload_hex'))
        elif event == 'tx' and row.get('raw_hex') == '2b':
            self.window_acks += 1

    def report(self):
        self._close_window()
        return {'rows': self.rows, 'chunks': self.chunks, 'frames': self.frames, 'dispatched': self.dispatched,
                'undispatched_count': self.undispatched_total, 'undispatched_frames': self.undispatched,
                'atomic_wire_events': self.atomic_wire_events, 'trailing_partial_bytes': len(self.buffer)}


def analyze_journal(rows):
    scanner = _Scanner()
    for row in rows:
        scanner.feed(row)
    return scanner.report()


def stop_signature(rows_or_report):
    """True when the journal ends with: ONE valid, undispatched stop frame (no ACK after it), then only a shutdown `\x03` and `closed`."""
    report = rows_or_report if isinstance(rows_or_report, dict) else analyze_journal(rows_or_report)
    if report['undispatched_count'] != 1:
        return {'signature': False, 'reason': f"{report['undispatched_count']} undispatched frames"}
    frame = report['undispatched_frames'][0]
    after = frame['rows_after']
    events = [item['event'] for item in after]
    ok = (frame['checksum_valid'] and frame['payload'].startswith(('T05', 'S05', 'T02', 'S02')) and not frame['ack_in_window']
          and frame['rows_after_count'] == 2 and events == [('tx', '03'), ('state', 'closed')])
    return {'signature': bool(ok), 'frame': frame['wire'], 'rows_after_frame': events, 'valid_checksum': frame['checksum_valid'],
            'seconds_frame_to_shutdown_interrupt': (after[0]['monotonic_ns'] - frame['monotonic_ns']) / NS if len(after) > 0 else None,
            'seconds_interrupt_to_closed': (after[1]['monotonic_ns'] - after[0]['monotonic_ns']) / NS if len(after) > 1 else None}


# ------------------------------------------------------------------ 3. last resume segment
def final_segment(rows):
    """The rows of the last `c` resume: tx `$c#63`, `state running`, rx `+` (continue ack), rx stop frame; plus relative arrival times."""
    last = max(i for i, row in enumerate(rows) if row.get('event') == 'packet_tx' and row.get('payload_hex') == '63')
    segment = rows[last:]
    tx = next(r for r in segment if r.get('event') == 'tx' and r.get('raw_hex') == '2463233633')
    running = next(r for r in segment if r.get('event') == 'state' and r.get('state') == 'running')
    chunks = [r for r in segment if r.get('event') == 'rx']
    return {'start_index': last, 'rows': segment, 'tx_ns': tx['monotonic_ns'], 'running_ns': running['monotonic_ns'],
            'chunks': [{'at_s_after_tx': (r['monotonic_ns'] - tx['monotonic_ns']) / NS, 'bytes': bytes.fromhex(r['raw_hex'])} for r in chunks]}


def poll_boundary_estimate(arrival_s_after_running):
    """Per-poll cycle overhead implied by an arrival exactly at the end of poll n (n = floor(arrival / 0.1 s)):
    arrival = n * 0.1 + (n - 1) * overhead  =>  overhead = (arrival - n * 0.1) / (n - 1)  (n >= 2)."""
    n = int(arrival_s_after_running // POLL_S)
    if n < 2:
        return {'poll_n': n, 'implied_overhead_ms': None}
    return {'poll_n': n, 'implied_overhead_ms': (arrival_s_after_running - n * POLL_S) / (n - 1) * 1e3}


# ------------------------------------------------------------------ 4. replay on a fake clock / socket
class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


class PollPeer:
    """Fake unix socket with CPython `settimeout`/`recv` semantics on a fake clock.

    `recv`: the timeout is rounded UP to whole milliseconds (`poll(2)` argument), a chunk that has arrived is returned at once,
    one arriving before the (rounded) wake-up is returned when it arrives, and one arriving up to `late_s` after it (the wake-up
    latency of the timer: Linux `do_poll` does a final scan after the timeout) is still returned; otherwise the call raises
    `TimeoutError` at wake-up time `wait_end + late_s`.
    """

    def __init__(self, clock, chunks, late_s=0.0):
        self.clock, self.chunks, self.late_s = clock, list(chunks), late_s
        self.timeout = None
        self.sent = []
        self.closed = False
        self.receives = 0

    def settimeout(self, value):
        self.timeout = value

    def recv(self, size):
        self.receives += 1
        wait_end = self.clock.now + math.ceil(self.timeout * 1000 - 1e-9) / 1000
        if self.chunks:
            arrival, data = self.chunks[0]
            if arrival <= self.clock.now:
                self.chunks.pop(0)
                return data
            if arrival <= wait_end + self.late_s:
                self.chunks.pop(0)
                self.clock.now = max(arrival, self.clock.now)
                return data
        self.clock.now = wait_end + self.late_s
        raise TimeoutError('synthetic empty receive poll')

    def sendall(self, payload):
        self.sent.append((self.clock.now, bytes(payload)))

    def close(self):
        self.closed = True


class _Journal:
    def __init__(self):
        self.lines = []

    def write(self, text):
        self.lines.extend(t for t in text.splitlines() if t)

    def flush(self):
        pass

    def close(self):
        pass


class _Machine:
    def __init__(self, work):
        self.gdb_path = Path(work) / 'sock/gdb.sock'
        self.work = Path(work)


class _Observer:
    def __init__(self, work):
        self.m = _Machine(work)
        self.instances, self.phase, self.watches, self.entry_ordinal, self.focus = {}, 'boot', set(), 0, None


class _Control:
    steps = []
    retained_health = {}
    host_verified = True

    def check_budget(self):
        pass


def client_types():
    """(label, factory(work) -> client): the client S10-8 headless used, and the client S10-9 headless uses."""
    import s109_run as RUN9
    return {'old_non_atomic_RecurringClient': lambda work: RET.R.RecurringClient(_Observer(work), _Control()),
            'new_AtomicHeadlessClient': lambda work: RUN9.headless_client(_Observer(work), _Control())}


def replay_segment(segment, factory, *, late_s, overhead_s, start_offset_s=None, max_polls=8, interrupt_after=True):
    """Replay the retained resume segment through `factory(work)` with timing parameters `late_s` (timer wake latency) and
    `overhead_s` (python loop cost between two polls).  Mirrors `Observer.run`: `resume`, then `wait_stop(timeout=.1)` repeated while it
    raises `RspTimeout`.  Returns the observable outcome."""
    clock = Clock()
    running_offset = (segment['running_ns'] - segment['tx_ns']) / NS
    start = running_offset if start_offset_s is None else start_offset_s
    chunks = [(item['at_s_after_tx'], item['bytes']) for item in segment['chunks']]
    peer = PollPeer(clock, chunks, late_s=late_s)
    journal = _Journal()
    with tempfile.TemporaryDirectory(prefix='s109-replay-') as work, mock.patch.object(B.time, 'monotonic', clock):
        client = factory(work)
        client._socket, client._state, client._log = peer, 'stopped', journal
        client.resume()                                   # sends `$c#63`, state -> running, expects a '+' ack next
        clock.now = start
        polls, delivered, error = [], None, None
        for ordinal in range(1, max_polls + 1):
            began = clock.now
            try:
                reply = client.wait_stop(timeout=POLL_S)
                polls.append({'poll': ordinal, 'began_s': began, 'ended_s': clock.now, 'returned': reply.decode()})
                delivered = reply.decode()
                break
            except B.RspTimeout as exc:
                polls.append({'poll': ordinal, 'began_s': began, 'ended_s': clock.now, 'error': f'RspTimeout: {exc}'})
                clock.now += overhead_s
            except Exception as exc:  # noqa: BLE001 - anything else would end the observer thread; record it
                error = f'{type(exc).__name__}: {exc}'
                polls.append({'poll': ordinal, 'error': error})
                break
        interrupt = None
        if interrupt_after and delivered is None and error is None:
            interrupt_peer_time = clock.now
            try:
                client._state = 'running'
                client.interrupt()
                interrupt = 'returned'
            except B.RspError as exc:
                interrupt = f'{type(exc).__name__}: {exc}'
            interrupt = {'result': interrupt, 'started_s': interrupt_peer_time, 'sent_03': any(data == b'\x03' for _, data in peer.sent)}
        events = [json.loads(line) for line in journal.lines]
    return {'delivered_stop': delivered, 'polls': polls, 'error': error, 'sent': [d.hex() for _, d in peer.sent],
            'acks_sent': sum(1 for _, d in peer.sent if d == b'+'), 'client_state': client._state,
            'journal_events': [(e['event'], e.get('raw_hex') or e.get('payload_hex') or e.get('state')) for e in events],
            'buffer_hex': bytes(client._buffer).hex(), 'shutdown_interrupt': interrupt}


def sweep(segment, factory, *, late_us, overhead_us=20, start_offset_us=None):
    """Loss/delivery over the timer wake latency range; returns the table and the contiguous loss intervals."""
    table = []
    for late in late_us:
        outcome = replay_segment(segment, factory, late_s=late * 1e-6, overhead_s=overhead_us * 1e-6, interrupt_after=False,
                                 start_offset_s=None if start_offset_us is None else start_offset_us * 1e-6)
        table.append({'late_us': late, 'delivered': outcome['delivered_stop'] is not None, 'acks': outcome['acks_sent'],
                      'first_poll_error': outcome['polls'][0].get('error') if outcome['polls'] else None, 'n_polls': len(outcome['polls'])})
    lost = [row['late_us'] for row in table if not row['delivered']]
    intervals, start, previous = [], None, None
    for row in table:
        if not row['delivered'] and start is None:
            start = row['late_us']
        if row['delivered'] and start is not None:
            intervals.append([start, previous])
            start = None
        previous = row['late_us']
    if start is not None:
        intervals.append([start, table[-1]['late_us']])
    return {'cases': len(table), 'lost_cases': len(lost), 'loss_intervals_late_us': intervals, 'all_delivered': not lost, 'table': table}


# ------------------------------------------------------------------ 2. census over every retained journal
def census(paths):
    result = []
    for path in sorted(paths):
        report = analyze_journal(iter_rows(path))
        result.append({'journal': str(Path(path).relative_to(ENV.REPORTS)), 'rows': report['rows'], 'frames': report['frames'],
                       'undispatched': report['undispatched_count'], 'atomic_wire_events': report['atomic_wire_events'],
                       'client_class_by_journal': 'atomic (wire_* events present)' if report['atomic_wire_events'] else 'non-atomic',
                       'undispatched_frames': [{'row': f['row'], 'wire': f['wire'], 'payload': f['payload'], 'rows_after': [x['event'] for x in f['rows_after']]}
                                               for f in report['undispatched_frames'][:3]],
                       'ends_with_lost_stop_signature': stop_signature(report)['signature']})
    return result


# ------------------------------------------------------------------ 5. host calibration (CLI only; real sockets, no VM)
def calibrate_host_poll(polls=60):
    import socket
    left, right = socket.socketpair()
    try:
        client = B.RspClient('/synthetic/none', '/dev/null', timeout=5)
        client._socket, client._state, client._continue_ack = left, 'running', False
        durations = []
        for _ in range(polls):
            began = time.monotonic()
            try:
                client.wait_stop(timeout=POLL_S)
            except B.RspTimeout:
                pass
            durations.append(time.monotonic() - began)
    finally:
        left.close()
        right.close()
    overhead = [(d - POLL_S) * 1e3 for d in durations]
    return {'polls': polls, 'method': 'real socketpair, idle peer, RspClient.wait_stop(timeout=.1) repeated; no QEMU/VM/guest',
            'overhead_ms': {'min': min(overhead), 'mean': statistics.mean(overhead), 'median': statistics.median(overhead), 'max': max(overhead)}}


# ------------------------------------------------------------------ 6. retained before/after fixtures
def fixtures():
    """`reproduce_before.exercise` (T05 boundary fixture) for the retained non-atomic/atomic base clients and the two headless clients."""
    types = {'rsp.RspClient (non-atomic, retained)': B.RspClient, 'atomic_rsp.AtomicRspClient': atomic_rsp.AtomicRspClient}
    for label, factory in client_types().items():
        types[label] = fixture_class(factory)
    out = {}
    with tempfile.TemporaryDirectory(prefix='s109-fixture-') as directory:
        for label, client_type in types.items():
            result = REPRO.exercise(client_type, directory, ''.join(c if c.isalnum() else '_' for c in label))
            out[label] = {key: result[key] for key in ('client_type', 'stop_received_across_original_polls', 'positive_ACK_count',
                                                       'decoded_stop_dispatch_count', 'old_loss_signature', 'no_guest_control_packet',
                                                       'no_timeout_increase', 'receive_calls', 'final_buffer_hex')}
            out[label]['first_poll'] = result['polls'][0]
    return out


def fixture_class(factory):
    """Class with the `(socket_path, log_path, timeout)` constructor `reproduce_before.exercise` expects, building the headless client
    that `factory(work)` builds (the real S10-8 / S10-9 headless client classes, not look-alikes)."""
    base = type(factory(tempfile.gettempdir()))

    class Fixture(base):
        def __init__(self, socket_path, log_path, timeout=5):
            super().__init__(_Observer(Path(tempfile.mkdtemp(prefix='s109-fixture-client-'))), _Control())
    Fixture.__name__ = base.__name__
    return Fixture


# ------------------------------------------------------------------ verdict
def classify(journal=S108_HEADLESS_JOURNAL, precedent=S1013_COORDINATED_JOURNAL, *, calibration=None):
    rows = load_rows(journal)
    signature = stop_signature(rows)
    segment = final_segment(rows)
    arrival = segment['chunks'][-1]['at_s_after_tx'] - (segment['running_ns'] - segment['tx_ns']) / NS
    boundary = poll_boundary_estimate(arrival)
    precedent_rows = load_rows(precedent)
    precedent_signature = stop_signature(precedent_rows)
    precedent_segment = final_segment(precedent_rows)
    precedent_arrival = precedent_segment['chunks'][-1]['at_s_after_tx'] - (precedent_segment['running_ns'] - precedent_segment['tx_ns']) / NS
    types = client_types()
    late_range = list(range(0, 401, 1))
    replays = {label: sweep(segment, factory, late_us=late_range) for label, factory in types.items()}
    overhead_variants = {}
    for overhead in (0, 100, 150):
        overhead_variants[f'overhead_{overhead}_us'] = {label: {k: v for k, v in sweep(segment, factory, late_us=late_range, overhead_us=overhead).items() if k != 'table'}
                                                        for label, factory in types.items()}
    old = replays['old_non_atomic_RecurringClient']
    new = replays['new_AtomicHeadlessClient']
    # one illustrative loss instance (smallest losing wake latency of the old client) with full outcomes for both clients
    example_late = old['loss_intervals_late_us'][0][0] if old['loss_intervals_late_us'] else None
    example = None
    if example_late is not None:
        example = {'late_us': example_late}
        for label, factory in types.items():
            example[label] = replay_segment(segment, factory, late_s=example_late * 1e-6, overhead_s=20e-6)
        retained_events = [event_tuple(r) for r in segment['rows']]
        example['retained_s10_8_events'] = retained_events
        lost = example['old_non_atomic_RecurringClient']['journal_events']
        # the replay journal of the old client equals the retained journal tail up to (excluding) the final `state closed` of close()
        example['old_replay_journal_equals_retained_tail'] = [tuple(e) for e in lost] == retained_events[:-1] and retained_events[-1] == ('state', 'closed')
        example['old_replay_shutdown_interrupt_error_equals_retained'] = (example['old_non_atomic_RecurringClient']['shutdown_interrupt'] or {}).get('result') \
            == 'RspTimeout: RSP receive timed out'
    ok_old = bool(signature['signature'] and old['lost_cases'] > 0)
    ok_new = bool(new['all_delivered'] and new['lost_cases'] == 0 and all(v['new_AtomicHeadlessClient']['all_delivered'] for v in overhead_variants.values()))
    verdict = 'CONFIRMED' if (ok_old and ok_new and precedent_signature['signature']) else 'NOT_CONFIRMED'
    return {
        'question': 'S10-8 cand-headless class-E stop: is the retained non-atomic RSP receive race (S10-4.14 mode) the cause?',
        'verdict': verdict,
        'journal': str(journal),
        'journal_signature': signature,
        'last_segment': {'arrival_s_after_running': arrival, 'arrival_s_after_c_tx': segment['chunks'][-1]['at_s_after_tx'],
                         'continue_ack_s_after_c_tx': segment['chunks'][0]['at_s_after_tx'], 'poll_boundary_estimate': boundary},
        'precedent_s10_4_13_journal': {'journal': str(precedent), 'signature': precedent_signature, 'arrival_s_after_running': precedent_arrival,
                                       'poll_boundary_estimate': poll_boundary_estimate(precedent_arrival),
                                       'note': 'the S10-4.13 desktop stop whose cause S10-4.14 CONFIRMED by deterministic reproduction (fixture before/after)'},
        'host_poll_calibration': calibration,
        'replay_sweep': {'timing_parameters': {'late_us': 'timer wake latency 0..400 us step 1 (journal cannot record it)', 'overhead_us': 20,
                                               'start_offset_s': 'journal: state running recorded 12.3 us after tx $c#63'},
                         'old_non_atomic_RecurringClient': {k: v for k, v in old.items() if k != 'table'},
                         'new_AtomicHeadlessClient': {k: v for k, v in new.items() if k != 'table'},
                         'overhead_variants': overhead_variants},
        'replay_example_instance': example,
    }


def main(argv):
    calibration = calibrate_host_poll()
    result = classify(calibration=calibration)
    paths = [p for p in (ENV.REPORTS).rglob('*rsp*.jsonl') if p.is_file()] + list((ENV.REPORTS).rglob('rsp-traffic.jsonl'))
    paths = sorted(set(paths))
    result['journal_census'] = census(paths)
    result['fixtures_reproduce_before'] = fixtures()
    print(json.dumps({k: result[k] for k in ('verdict', 'journal_signature', 'last_segment')}, indent=1, default=str)[:3000])
    if '--write' in argv:
        destination = ENV.RSP_CLASSIFICATION_EVIDENCE
        destination.mkdir(mode=0o700, parents=True, exist_ok=False)
        ENV.dump(destination / 'classification.json', result)
    return 0 if result['verdict'] == 'CONFIRMED' else 2


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
