"""S10-9.0a offline tests: classification of the S10-8 headless stop by replaying the retained `rsp-traffic.jsonl` through the retained
non-atomic and the atomic RSP clients.  No VM, no QEMU, no guest.  Reads retained evidence read-only.
"""
import inspect
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_rsp_replay as RR  # noqa: E402
import s109_env as ENV  # noqa: E402
import rsp as B  # noqa: E402

FRAME = b'$T05thread:01;#07'
SMALL_CONTROLS = (
    ENV.RUNTIME / 'headless-observed-1/rsp-traffic.jsonl',
    ENV.RUNTIME / 'observer-recurring/desktop/evidence/rsp-traffic.jsonl',
    ENV.RUNTIME / 'observer-qmp-serialized/desktop/evidence/rsp-traffic.jsonl',
    ENV.REPORTS / 's10-7-evidence/measurement-runs/b2-headless/evidence/rsp-traffic.jsonl',
    ENV.RUNTIME / 'observer-step-amendment/evidence/rsp-traffic.jsonl',
)


def row(event, ns, **fields):
    return {'monotonic_ns': ns, 'event': event, **fields}


class Scanner(unittest.TestCase):
    def test_non_atomic_pattern_ack_then_dispatch(self):
        rows = [row('rx', 1, raw_hex=FRAME.hex()), row('tx', 2, raw_hex='2b'), row('packet_rx', 3, payload_hex=b'T05thread:01;'.hex())]
        report = RR.analyze_journal(rows)
        self.assertEqual((report['frames'], report['dispatched'], report['undispatched_count']), (1, 1, 0))

    def test_atomic_pattern_pending_ack_dispatch(self):
        rows = [row('rx', 1, raw_hex=FRAME.hex()), row('wire_frame_pending', 2, payload_hex=b'T05thread:01;'.hex()), row('tx', 3, raw_hex='2b'),
                row('packet_rx', 4, payload_hex=b'T05thread:01;'.hex())]
        report = RR.analyze_journal(rows)
        self.assertEqual((report['frames'], report['dispatched'], report['undispatched_count'], report['atomic_wire_events']), (1, 1, 0, 1))

    def test_a_frame_without_ack_or_dispatch_is_reported_with_the_following_rows(self):
        rows = [row('rx', 1, raw_hex='2b'), row('rx', 2, raw_hex=FRAME.hex()), row('tx', 3, raw_hex='03'), row('state', 4, state='closed')]
        report = RR.analyze_journal(rows)
        self.assertEqual(report['undispatched_count'], 1)
        frame = report['undispatched_frames'][0]
        self.assertEqual((frame['wire'], frame['checksum_valid'], frame['ack_in_window'], frame['rows_after_count']), (FRAME.decode(), True, False, 2))
        self.assertEqual([x['event'] for x in frame['rows_after']], [('tx', '03'), ('state', 'closed')])
        self.assertTrue(RR.stop_signature(report)['signature'])

    def test_signature_requires_the_exact_tail(self):
        base = [row('rx', 1, raw_hex=FRAME.hex()), row('tx', 3, raw_hex='03'), row('state', 4, state='closed')]
        self.assertTrue(RR.stop_signature(base)['signature'])
        self.assertFalse(RR.stop_signature(base + [row('state', 5, state='x')])['signature'])        # more rows after: not the tail signature
        self.assertFalse(RR.stop_signature(base[:2])['signature'])                                   # no closed row
        bad = [row('rx', 1, raw_hex=b'$T05thread:01;#08'.hex())] + base[1:]
        self.assertFalse(RR.stop_signature(bad)['signature'])                                       # invalid checksum is NOT the race signature
        acked = [row('rx', 1, raw_hex=FRAME.hex()), row('tx', 2, raw_hex='2b'), row('tx', 3, raw_hex='03'), row('state', 4, state='closed')]
        self.assertFalse(RR.stop_signature(acked)['signature'])                                     # an ACK after the frame excludes the race

    def test_frames_split_across_chunks_and_several_frames_per_chunk(self):
        half = FRAME[:7].hex(), FRAME[7:].hex()
        rows = [row('rx', 1, raw_hex=half[0]), row('rx', 2, raw_hex=half[1]), row('tx', 3, raw_hex='2b'),
                row('packet_rx', 4, payload_hex=b'T05thread:01;'.hex())]
        self.assertEqual(RR.analyze_journal(rows)['undispatched_count'], 0)
        two = [row('rx', 1, raw_hex=(b'$OK#9a' + b'$OK#9a').hex()), row('tx', 2, raw_hex='2b'), row('packet_rx', 3, payload_hex=b'OK'.hex())]
        report = RR.analyze_journal(two)
        self.assertEqual((report['frames'], report['dispatched'], report['undispatched_count']), (2, 1, 1))        # one payload dispatched, one lost

    def test_stream_scanner_equals_list_analysis(self):
        rows = [row('rx', 1, raw_hex=FRAME.hex()), row('tx', 2, raw_hex='2b'), row('packet_rx', 3, payload_hex=b'T05thread:01;'.hex()),
                row('rx', 4, raw_hex=FRAME.hex())]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'j.jsonl'
            path.write_text(''.join(json.dumps(r) + '\n' for r in rows) + 'not json\n')
            streamed = RR.analyze_journal(RR.iter_rows(path))
        self.assertEqual((streamed['frames'], streamed['dispatched'], streamed['undispatched_count']), (2, 1, 1))


class RetainedJournals(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.s108 = RR.load_rows(RR.S108_HEADLESS_JOURNAL)
        cls.precedent = RR.load_rows(RR.S1013_COORDINATED_JOURNAL)

    def test_s10_8_headless_journal_has_the_lost_stop_signature(self):
        report = RR.analyze_journal(self.s108)
        self.assertEqual((report['rows'], report['frames'], report['dispatched'], report['undispatched_count']), (3871, 740, 739, 1))
        self.assertEqual(report['atomic_wire_events'], 0)                          # the S10-8 headless client was the non-atomic one
        signature = RR.stop_signature(report)
        self.assertTrue(signature['signature'])
        self.assertEqual(signature['frame'], '$T05thread:01;#07')
        self.assertTrue(signature['valid_checksum'])
        self.assertAlmostEqual(signature['seconds_interrupt_to_closed'], 5.02, delta=.01)       # the 5 s default wait of the shutdown interrupt
        self.assertGreater(signature['seconds_frame_to_shutdown_interrupt'], 889)

    def test_the_s10_4_13_precedent_has_the_same_signature(self):
        signature = RR.stop_signature(self.precedent)
        self.assertTrue(signature['signature'])
        self.assertEqual(signature['frame'], '$T05thread:01;#07')

    def test_last_resume_segment_of_the_s10_8_journal(self):
        segment = RR.final_segment(self.s108)
        self.assertEqual([r['event'] for r in segment['rows']], ['packet_tx', 'tx', 'state', 'rx', 'rx', 'tx', 'state'])
        self.assertEqual(segment['chunks'][0]['bytes'], b'+')
        self.assertEqual(segment['chunks'][1]['bytes'], FRAME)
        self.assertAlmostEqual(segment['chunks'][1]['at_s_after_tx'], 0.200206884, places=9)
        self.assertAlmostEqual((segment['running_ns'] - segment['tx_ns']) / 1e9, 12.336e-6, places=9)
        arrival = segment['chunks'][1]['at_s_after_tx'] - (segment['running_ns'] - segment['tx_ns']) / 1e9
        estimate = RR.poll_boundary_estimate(arrival)
        self.assertEqual(estimate['poll_n'], 2)
        self.assertAlmostEqual(estimate['implied_overhead_ms'], 0.1945, delta=.001)

    def test_the_precedent_implies_the_same_per_poll_overhead(self):
        segment = RR.final_segment(self.precedent)
        arrival = segment['chunks'][-1]['at_s_after_tx'] - (segment['running_ns'] - segment['tx_ns']) / 1e9
        estimate = RR.poll_boundary_estimate(arrival)
        self.assertEqual(estimate['poll_n'], 53)
        self.assertAlmostEqual(estimate['implied_overhead_ms'], 0.189, delta=.002)

    def test_poll_boundary_estimate_arithmetic(self):
        self.assertEqual(RR.poll_boundary_estimate(0.05), {'poll_n': 0, 'implied_overhead_ms': None})
        self.assertAlmostEqual(RR.poll_boundary_estimate(0.2002)['implied_overhead_ms'], 0.2, places=9)
        self.assertAlmostEqual(RR.poll_boundary_estimate(5.3 + 52 * 0.0002)['implied_overhead_ms'], 0.2, places=6)

    def test_control_journals_have_no_lost_frame(self):
        for path in SMALL_CONTROLS:
            with self.subTest(path=path.name):
                report = RR.analyze_journal(RR.iter_rows(path))
                self.assertGreater(report['frames'], 100)
                self.assertEqual(report['undispatched_count'], 0)
                self.assertFalse(RR.stop_signature(report)['signature'])

    def test_synthetic_fixture_journals_classify_as_designed(self):
        before = RR.analyze_journal(RR.iter_rows(ENV.RUNTIME / 'observer-rsp-atomic/reproduction-before-rsp.jsonl'))
        after = RR.analyze_journal(RR.iter_rows(ENV.RUNTIME / 'observer-rsp-atomic/reproduction-after-rsp.jsonl'))
        self.assertEqual((before['frames'], before['undispatched_count'], before['atomic_wire_events']), (1, 1, 0))      # old client: frame lost
        self.assertEqual((after['frames'], after['undispatched_count']), (1, 0))                                         # atomic client: delivered
        self.assertGreaterEqual(after['atomic_wire_events'], 1)


class PollPeerSemantics(unittest.TestCase):
    def peer(self, chunks, late=0.0):
        clock = RR.Clock()
        return clock, RR.PollPeer(clock, chunks, late_s=late)

    def test_timeout_is_rounded_up_to_whole_milliseconds(self):
        clock, peer = self.peer([])
        peer.settimeout(0.0999877)
        with self.assertRaises(TimeoutError):
            peer.recv(1)
        self.assertAlmostEqual(clock.now, 0.100, places=9)
        peer.settimeout(0.0001)
        with self.assertRaises(TimeoutError):
            peer.recv(1)
        self.assertAlmostEqual(clock.now, 0.101, places=9)

    def test_chunk_before_the_wake_is_returned_at_its_arrival_time(self):
        clock, peer = self.peer([(0.05, b'a')])
        peer.settimeout(0.1)
        self.assertEqual(peer.recv(1), b'a')
        self.assertAlmostEqual(clock.now, 0.05, places=9)

    def test_chunk_within_the_wake_latency_is_still_returned_after_the_deadline(self):
        clock, peer = self.peer([(0.1001, b'a')], late=0.0002)
        peer.settimeout(0.1)
        self.assertEqual(peer.recv(1), b'a')
        self.assertAlmostEqual(clock.now, 0.1001, places=9)
        clock, peer = self.peer([(0.1003, b'a')], late=0.0002)
        peer.settimeout(0.1)
        with self.assertRaises(TimeoutError):
            peer.recv(1)
        self.assertAlmostEqual(clock.now, 0.1002, places=9)

    def test_already_arrived_chunk_returns_immediately(self):
        clock, peer = self.peer([(0.0, b'a')])
        clock.now = 1.0
        peer.settimeout(0.1)
        self.assertEqual(peer.recv(1), b'a')
        self.assertEqual(clock.now, 1.0)


class ReplayOfTheRetainedSegment(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.segment = RR.final_segment(RR.load_rows(RR.S108_HEADLESS_JOURNAL))
        cls.types = RR.client_types()

    def run_replay(self, label, late_us, overhead_us=20, segment=None):
        return RR.replay_segment(segment or self.segment, self.types[label], late_s=late_us * 1e-6, overhead_s=overhead_us * 1e-6)

    def test_old_client_loses_the_frame_in_the_boundary_window_with_the_retained_journal_tail(self):
        outcome = self.run_replay('old_non_atomic_RecurringClient', 120)
        self.assertIsNone(outcome['delivered_stop'])
        self.assertEqual(outcome['acks_sent'], 0)
        self.assertEqual(outcome['polls'][1]['error'], 'RspTimeout: RSP deadline expired')          # the swallowed exception of the S10-4.14 mechanism
        self.assertEqual(outcome['buffer_hex'], '')
        retained = [RR.event_tuple(r) for r in self.segment['rows']]
        self.assertEqual([tuple(e) for e in outcome['journal_events']], retained[:-1])             # identical up to `state closed` written by close()
        self.assertEqual(retained[-1], ('state', 'closed'))
        self.assertEqual(outcome['shutdown_interrupt']['result'], 'RspTimeout: RSP receive timed out')   # the retained observation blocker text
        self.assertTrue(outcome['shutdown_interrupt']['sent_03'])
        self.assertEqual(outcome['client_state'], 'running')                                      # host believes the guest runs; QEMU is already stopped

    def test_atomic_client_delivers_the_same_frame_on_the_next_poll(self):
        outcome = self.run_replay('new_AtomicHeadlessClient', 120)
        self.assertEqual(outcome['delivered_stop'], 'T05thread:01;')
        self.assertEqual(outcome['acks_sent'], 1)
        self.assertEqual(outcome['polls'][1]['error'], 'RspTimeout: RSP deadline expired')          # same expiry as the old client ...
        self.assertEqual(outcome['polls'][2]['returned'], 'T05thread:01;')                          # ... but the frame survives it
        events = [e[0] for e in outcome['journal_events']]
        self.assertEqual(events[events.index('wire_frame_pending'):], ['wire_frame_pending', 'tx', 'packet_rx', 'state'])

    def test_window_edges_and_only_the_old_client_loses(self):
        old = RR.sweep(self.segment, self.types['old_non_atomic_RecurringClient'], late_us=range(0, 401))
        new = RR.sweep(self.segment, self.types['new_AtomicHeadlessClient'], late_us=range(0, 401))
        self.assertEqual(old['loss_intervals_late_us'], [[82, 163]])
        self.assertEqual(old['lost_cases'], 82)
        self.assertTrue(new['all_delivered'])
        self.assertEqual(new['lost_cases'], 0)
        self.assertTrue(all(row['acks'] == 1 for row in new['table'] if row['delivered']))
        self.assertTrue(all(row['acks'] == 1 for row in old['table'] if row['delivered']))

    def test_overhead_shifts_the_window_but_the_atomic_client_never_loses(self):
        for overhead in (0, 20, 100, 150):
            with self.subTest(overhead_us=overhead):
                old = RR.sweep(self.segment, self.types['old_non_atomic_RecurringClient'], late_us=range(0, 401), overhead_us=overhead)
                new = RR.sweep(self.segment, self.types['new_AtomicHeadlessClient'], late_us=range(0, 401), overhead_us=overhead)
                self.assertGreater(old['lost_cases'], 0)
                self.assertEqual(len(old['loss_intervals_late_us']), 1)
                self.assertTrue(new['all_delivered'])
                low, high = old['loss_intervals_late_us'][0]
                self.assertEqual(high, 183 - overhead)                                           # upper edge: arrival - nominal second deadline
                self.assertLess(low, high)

    def test_arrival_away_from_the_poll_boundary_is_delivered_by_both_clients(self):
        for arrival in (0.150, 0.2500, 0.3):
            segment = dict(self.segment, chunks=[self.segment['chunks'][0], {'at_s_after_tx': arrival, 'bytes': FRAME}])
            for label in self.types:
                with self.subTest(arrival=arrival, client=label):
                    outcome = self.run_replay(label, 120, segment=segment)
                    self.assertEqual(outcome['delivered_stop'], 'T05thread:01;')
                    self.assertEqual(outcome['acks_sent'], 1)

    def test_the_replay_sends_no_guest_control_packet(self):
        for label in self.types:
            outcome = self.run_replay(label, 120)
            self.assertTrue(set(outcome['sent']) <= {'2463233633', '2b', '03'}, outcome['sent'])      # `$c#63`, ACK, the shutdown interrupt of the retained run

    def test_no_timeout_is_lengthened(self):
        for label in self.types:
            outcome = self.run_replay(label, 120)
            for poll in outcome['polls']:
                if 'ended_s' in poll and 'error' in poll:
                    self.assertAlmostEqual(poll['ended_s'] - poll['began_s'], 0.1, delta=0.0003)


class Verdict(unittest.TestCase):
    def test_classification_is_confirmed_on_the_retained_evidence(self):
        result = RR.classify()
        self.assertEqual(result['verdict'], 'CONFIRMED')
        self.assertTrue(result['journal_signature']['signature'])
        self.assertTrue(result['precedent_s10_4_13_journal']['signature']['signature'])
        sweep = result['replay_sweep']
        self.assertGreater(sweep['old_non_atomic_RecurringClient']['lost_cases'], 0)
        self.assertTrue(sweep['new_AtomicHeadlessClient']['all_delivered'])
        self.assertTrue(all(v['new_AtomicHeadlessClient']['all_delivered'] for v in sweep['overhead_variants'].values()))
        example = result['replay_example_instance']
        self.assertTrue(example['old_replay_journal_equals_retained_tail'])
        self.assertTrue(example['old_replay_shutdown_interrupt_error_equals_retained'])
        self.assertEqual(example['new_AtomicHeadlessClient']['delivered_stop'], 'T05thread:01;')

    def test_verdict_is_not_confirmed_when_the_journal_has_no_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'j.jsonl'
            rows = [row('packet_tx', 1, payload_hex='63'), row('tx', 2, raw_hex='2463233633'), row('state', 3, state='running'),
                    row('rx', 4, raw_hex='2b'), row('rx', 5, raw_hex=FRAME.hex()), row('tx', 6, raw_hex='2b'),
                    row('packet_rx', 7, payload_hex=b'T05thread:01;'.hex()), row('state', 8, state='closed')]
            path.write_text(''.join(json.dumps(r) + '\n' for r in rows))
            self.assertEqual(RR.classify(journal=path)['verdict'], 'NOT_CONFIRMED')

    def test_census_flags_exactly_the_non_atomic_lost_stop_journals_among_the_small_ones(self):
        report = RR.census(list(SMALL_CONTROLS) + [RR.S108_HEADLESS_JOURNAL, RR.S1013_COORDINATED_JOURNAL])
        flagged = [item['journal'] for item in report if item['ends_with_lost_stop_signature']]
        self.assertEqual(sorted(flagged), sorted([str(RR.S108_HEADLESS_JOURNAL.relative_to(ENV.REPORTS)), str(RR.S1013_COORDINATED_JOURNAL.relative_to(ENV.REPORTS))]))
        self.assertTrue(all(item['client_class_by_journal'] == 'non-atomic' for item in report))

    def test_host_calibration_is_a_real_socketpair_measurement_without_any_vm(self):
        calibration = RR.calibrate_host_poll(polls=3)
        self.assertEqual(calibration['polls'], 3)
        self.assertGreater(calibration['overhead_ms']['min'], 0)
        self.assertLess(calibration['overhead_ms']['max'], 50)
        source = inspect.getsource(RR.calibrate_host_poll)
        self.assertIn('socketpair', source)
        self.assertNotIn('subprocess', source)


class RetainedFixtures(unittest.TestCase):
    def test_reproduce_before_after_fixtures_for_all_four_clients(self):
        result = RR.fixtures()
        old_labels = ('rsp.RspClient (non-atomic, retained)', 'old_non_atomic_RecurringClient')
        new_labels = ('atomic_rsp.AtomicRspClient', 'new_AtomicHeadlessClient')
        for label in old_labels:
            self.assertFalse(result[label]['stop_received_across_original_polls'], label)
            self.assertTrue(result[label]['old_loss_signature'], label)
            self.assertEqual(result[label]['positive_ACK_count'], 0, label)
        for label in new_labels:
            self.assertTrue(result[label]['stop_received_across_original_polls'], label)
            self.assertEqual((result[label]['positive_ACK_count'], result[label]['decoded_stop_dispatch_count']), (1, 1), label)
            self.assertTrue(result[label]['no_guest_control_packet'] and result[label]['no_timeout_increase'], label)

    def test_the_retained_reproduce_script_is_never_executed_and_nothing_is_written_into_the_retained_archive(self):
        before = sorted(p.name for p in (ENV.RUNTIME / 'observer-rsp-atomic').iterdir())
        RR.fixtures()
        self.assertEqual(sorted(p.name for p in (ENV.RUNTIME / 'observer-rsp-atomic').iterdir()), before)
        source = inspect.getsource(RR)
        self.assertNotIn('REPRO.main(', source)
        self.assertNotIn('REPRO.HERE', source)


class Hygiene(unittest.TestCase):
    def test_replay_writes_only_under_the_new_evidence_directory(self):
        source = inspect.getsource(RR.main)
        self.assertEqual(source.count('destination.mkdir('), 1)
        self.assertIn('ENV.RSP_CLASSIFICATION_EVIDENCE', source)
        self.assertEqual(ENV.RSP_CLASSIFICATION_EVIDENCE, ENV.REPORTS / 's10-9-evidence' / 's10-9.0a-rsp-classification')
        full = (HERE / 's109_rsp_replay.py').read_text()
        for forbidden in ('write_memory', 'write_register', 'run-desktop.sh', 'kernel/src/main.rs', 'qemu-system', 'Popen', 'os.kill', 'connect('):
            self.assertNotIn(forbidden, full, forbidden)


if __name__ == '__main__':
    unittest.main()
