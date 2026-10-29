"""S10-9.1 offline tests: the headless runner now uses the ATOMIC RSP client (same `AtomicReceiveMixin` the desktop client uses), the T05
boundary race fixture passes on that path and fails on the retained non-atomic headless client of S10-8; surfaces/observers are wired
as designed.  No VM, no QEMU, no network.
"""
import inspect
import json
import shutil
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_run as RUN9  # noqa: E402
import s109_env as ENV  # noqa: E402
import s108_run as RUN8  # noqa: E402
import s107_retained as RET  # noqa: E402
import test_s108_wiring as W8  # noqa: E402  (module import only: its tests are NOT collected here)
import reproduce_before as REPRO  # noqa: E402  (retained T05 boundary fixture; main is guarded)
import atomic_rsp  # noqa: E402
import rsp as B  # noqa: E402
import common as COMMON  # noqa: E402

R = RET.R
_SCRATCH = []


def scratch(prefix='s109-wiring-'):
    path = tempfile.mkdtemp(prefix=prefix)
    _SCRATCH.append(path)
    return path


def tearDownModule():
    for path in _SCRATCH:
        shutil.rmtree(path, ignore_errors=True)


class Machine:
    def __init__(self, work):
        self.work = Path(work)
        self.gdb_path = self.work / 'sock/gdb.sock'


class FakeObserver:
    def __init__(self, work):
        self.m = Machine(work)
        self.instances, self.phase, self.watches, self.entry_ordinal, self.focus = {}, 'boot', set(), 0, None


class FakeControl:
    steps = []
    retained_health = {}
    host_verified = True

    def check_budget(self):
        pass


def fixture_class(make_client):
    """`reproduce_before.exercise` builds `client_type(socket_path, log_path, timeout=5)`: wrap the REAL client class `make_client` builds."""
    probe = make_client(FakeObserver(scratch('s109-wiring-probe-')), FakeControl())
    base = type(probe)

    class Fixture(base):
        def __init__(self, socket_path, log_path, timeout=5):
            super().__init__(FakeObserver(scratch()), FakeControl())
    Fixture.__name__ = base.__name__
    return Fixture


class AtomicHeadlessWiring(unittest.TestCase):
    def test_headless_client_is_the_atomic_mixin_over_the_retained_recurring_client(self):
        client = RUN9.headless_client(FakeObserver(scratch()), FakeControl())
        mro = type(client).__mro__
        self.assertIs(mro[0], RUN9.AtomicHeadlessClient)
        self.assertIs(mro[1], atomic_rsp.AtomicReceiveMixin)
        self.assertIs(mro[2], R.RecurringClient)
        self.assertIn(B.RspClient, mro)
        self.assertTrue(client._pending_wire_frame is None and client._wire_failure is None)

    def test_the_mixin_is_the_same_object_the_desktop_client_uses(self):
        self.assertIs(RET.F.AtomicReceiveMixin, atomic_rsp.AtomicReceiveMixin)
        self.assertIs(RET.F.AtomicClient.__mro__[1], RUN9.AtomicHeadlessClient.__mro__[1])
        self.assertEqual([c.__name__ for c in RET.F.AtomicClient.__mro__[:3]], ['AtomicClient', 'AtomicReceiveMixin', 'CoordinatedClient'])
        self.assertIs(RET.C.CoordinatedClient.__mro__[1], R.RecurringClient)       # desktop = atomic over coordinated over the SAME recurring client

    def test_every_receive_framing_method_of_the_headless_client_is_the_atomic_one(self):
        for name in ('_event', '_ack_frame', '_fatal_wire', '_require_socket'):
            self.assertIs(getattr(RUN9.AtomicHeadlessClient, name), getattr(atomic_rsp.AtomicReceiveMixin, name), name)
            self.assertIsNot(getattr(RUN9.AtomicHeadlessClient, name), getattr(B.RspClient, name, None), name)
        for name in ('_validate', '_packet', 'step_over_breakpoint', 'resume', 'connect', 'wait_stop', 'interrupt'):    # policy/native-step layers unchanged
            self.assertIs(getattr(RUN9.AtomicHeadlessClient, name), getattr(R.RecurringClient, name), name)

    def test_the_runner_creates_its_client_only_through_the_atomic_factory(self):
        source = inspect.getsource(RUN9.run_headless)
        self.assertIn('observer.client = headless_client(observer, control)', source)
        self.assertNotIn('R.RecurringClient(', source)
        self.assertNotIn('RecurringClient(observer', source)
        self.assertLess(source.index('observer.client = headless_client(observer, control)'), source.index('observer.start()'))
        self.assertEqual(source.count('headless_client('), 1)
        self.assertEqual(inspect.getsource(RUN9.headless_client).count('AtomicHeadlessClient(observer, control)'), 1)
        self.assertIn("'rsp_client': 'AtomicHeadlessClient (AtomicReceiveMixin over RecurringClient)'", source)

    def test_negative_control_the_s10_8_headless_runner_used_the_non_atomic_client(self):
        source = inspect.getsource(RUN8.run_headless)
        self.assertIn('observer.client = R.RecurringClient(observer, control)', source)
        plain = R.RecurringClient(FakeObserver(scratch()), FakeControl())
        self.assertFalse(hasattr(plain, '_pending_wire_frame'))
        self.assertIs(type(plain)._event, B.RspClient._event)

    def test_desktop_wiring_is_unchanged(self):
        source = inspect.getsource(RUN9.run_desktop)
        self.assertIn('observer.client = RET.F.AtomicClient(observer, control, gate)', source)
        self.assertIn('LatencyDesktopSurface(machine, observer, gate)', source)
        self.assertIn("'rsp_client': 'run_atomic.AtomicClient (AtomicReceiveMixin over CoordinatedClient)'", source)
        self.assertIn('LatencyHeadlessSurface(machine, observer)', inspect.getsource(RUN9.run_headless))

    def test_the_client_built_on_a_real_observer_has_the_same_endpoint_log_and_timeout_as_the_retained_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            observer = RUN9.MemoryPlainObserver(W8.fake_machine(tmp, 'headless'), memory=RUN9.memory_context(ENV.SETS['cand-headless']))
            retained = observer.client
            client = RUN9.headless_client(observer, FakeControl())
            self.assertEqual((client.socket_path, client.log_path, client.timeout), (retained.socket_path, retained.log_path, retained.timeout))
            self.assertEqual(client.timeout, 5.0)
            self.assertTrue(client.log_path.endswith('evidence/rsp-traffic.jsonl'))
            self.assertIs(client.observer, observer)
            observer.events.close()                 # the retained constructor opens these two journals
            observer.coverage.close()

    def test_policy_allowlist_is_identical_to_the_non_atomic_client(self):
        old = R.RecurringClient(FakeObserver(scratch()), FakeControl())
        new = RUN9.headless_client(FakeObserver(scratch()), FakeControl())
        for command in ('?', 'c', 'g', 'm1000,10', 'Z1,100000000,1', 'z2,100000000,8', 'qqemu.PhyMemMode', 'Qqemu.PhyMemMode:1', 'qqemu.sstepbits', 'qqemu.sstep'):
            self.assertEqual(self._allowed(old, command), self._allowed(new, command), command)
        for command in ('M1000,1:00', 'P0=00', 'Z0,1000,1', 'vCont;c', 'X1000,1:00', 'D', 'k'):
            self.assertFalse(self._allowed(new, command), command)

    @staticmethod
    def _allowed(client, command):
        try:
            client._validate(command)
            return True
        except B.PolicyError:
            return False


class T05RaceFixtureOnTheHeadlessPath(unittest.TestCase):
    """The retained S10-4.14 boundary fixture (`reproduce_before.exercise`): the first complete `$T05thread:01;#07` frame arrives exactly when the
    first .1 s poll expires.  Old headless client (S10-8) loses it; the atomic headless client (S10-9) delivers it."""

    def exercise(self, make_client, label):
        with tempfile.TemporaryDirectory() as directory:
            result = REPRO.exercise(fixture_class(make_client), directory, label)
            journal = [json.loads(line) for line in (Path(directory) / f'{label}-rsp.jsonl').read_text().splitlines()]
        return result, journal

    def test_the_old_non_atomic_headless_client_loses_the_stop_with_the_retained_signature(self):
        result, journal = self.exercise(lambda observer, control: R.RecurringClient(observer, control), 'old')
        self.assertFalse(result['stop_received_across_original_polls'])
        self.assertTrue(result['old_loss_signature'])
        self.assertEqual((result['positive_ACK_count'], result['decoded_stop_dispatch_count']), (0, 0))
        self.assertEqual(result['final_buffer_hex'], '')
        with self.assertRaises(AssertionError):
            REPRO.assert_delivery(result)
        self.assertEqual([row['event'] for row in journal if row['event'] != 'state'], ['rx'])   # complete frame received, no ACK, no dispatch: the S10-8 journal tail

    def test_the_atomic_headless_client_delivers_the_stop_through_the_same_fixture(self):
        result, journal = self.exercise(RUN9.headless_client, 'new')
        REPRO.assert_delivery(result)                                          # the retained assertion itself
        self.assertTrue(result['stop_received_across_original_polls'])
        self.assertEqual((result['positive_ACK_count'], result['decoded_stop_dispatch_count']), (1, 1))
        self.assertTrue(result['no_guest_control_packet'])
        self.assertTrue(result['no_timeout_increase'])
        self.assertEqual([row['event'] for row in journal if row['event'] != 'state'], ['rx', 'wire_frame_pending', 'tx', 'packet_rx'])
        self.assertEqual(result['polls'][0]['error'], 'RspTimeout: RSP deadline expired')
        self.assertEqual(result['polls'][0]['buffer_hex'], '24' + '54303574687265616' + '43a30313b' + '23' + '3037')   # frame kept whole across the expiry
        self.assertEqual(result['last_stop'], 'T05thread:01;')

    def test_the_atomic_headless_client_sends_only_acks_in_the_fixture(self):
        result, _journal = self.exercise(RUN9.headless_client, 'acks')
        self.assertEqual(result['sent_raw_hex'], ['2b'])
        self.assertEqual(result['receive_calls'], 1)

    def test_same_result_as_the_retained_atomic_base_client_on_the_fixture(self):
        with tempfile.TemporaryDirectory() as directory:
            base = REPRO.exercise(atomic_rsp.AtomicRspClient, directory, 'base')
        headless, _ = self.exercise(RUN9.headless_client, 'headless')
        for key in ('stop_received_across_original_polls', 'positive_ACK_count', 'decoded_stop_dispatch_count', 'receive_calls',
                    'no_guest_control_packet', 'no_timeout_increase', 'last_stop', 'final_state_before_close'):
            self.assertEqual(base[key], headless[key], key)

    def test_a_terminal_wire_failure_after_a_lost_ack_is_not_a_benign_poll_timeout(self):
        """Retained S10-4.14 semantics carry over: an ACK send error is fatal and poisons the client (no retry)."""
        client = RUN9.headless_client(FakeObserver(scratch()), FakeControl())

        class Peer:
            timeout = None

            def settimeout(self, value):
                self.timeout = value

            def recv(self, size):
                return b'$T05thread:01;#07'

            def sendall(self, payload):
                raise OSError('broken pipe')

            def close(self):
                pass
        client._socket, client._state, client._continue_ack = Peer(), 'running', False
        client._log = types.SimpleNamespace(write=lambda text: None, flush=lambda: None, close=lambda: None)
        with self.assertRaises(B.RspError) as first:
            client.wait_stop(timeout=5)
        self.assertNotIsInstance(first.exception, B.RspTimeout)
        with self.assertRaises(B.RspError) as second:
            client.wait_stop(timeout=5)
        self.assertIn('delivery uncertain', str(second.exception))


class ObserverAndSurfaceClasses(unittest.TestCase):
    def test_observer_classes_are_the_unchanged_s10_8_ones(self):
        self.assertIs(RUN9.MemoryHookObserver, RUN8.MemoryHookObserver)
        self.assertIs(RUN9.MemoryPlainObserver, RUN8.MemoryPlainObserver)
        for name in ('classify_run', 'summarize_memory', 'facts_from_table', 'chain', 'trace_argv', 'memory_context', 'archive', 'write_manifest',
                     'census_file', 'CANDIDATE_SNAPSHOTS'):
            self.assertIs(getattr(RUN9, name), getattr(RUN8, name), name)
        self.assertEqual(RUN9.MemoryPlainObserver.__mro__[1].__name__, 'MemoryMixin')
        self.assertIn('_wiring_stop', vars(RUN9.MemoryPlainObserver.__mro__[1]))
        self.assertNotIn('_stop', vars(RUN9.MemoryPlainObserver.__mro__[1]))

    def test_surface_classes_chain_the_latency_mixin_in_front_of_the_s10_7_status_surfaces(self):
        import s107_machine as MACH7
        desktop, headless = RUN9.LatencyDesktopSurface, RUN9.LatencyHeadlessSurface
        self.assertTrue(issubclass(desktop, MACH7.DesktopStatusMixin) and issubclass(desktop, RUN9.HS.HookSurface))
        self.assertTrue(issubclass(headless, MACH7.HeadlessStatusMixin) and issubclass(headless, COMMON.Surface))
        self.assertIs(headless.status, MACH7.HeadlessStatusMixin.status)
        self.assertIs(desktop.status, MACH7.DesktopStatusMixin.status)

    def test_pause_counter_stays_the_retained_observer_counter(self):
        run = inspect.getsource(RET.OBS.Observer.run)
        self.assertIn('self.pause_ns+=time.monotonic_ns()-stop_ns', run)

    def test_recovery_wait_policy_text(self):
        policy = RUN9.RECOVERY_WAIT_POLICY
        self.assertIn('D9-1=B', policy['decision'])
        self.assertEqual(policy['effective_deadline'], 'min(start + 120 s + pause_in_wait, machine_start + 900 s)')
        self.assertEqual(policy['global_window_s'], 900)
        self.assertEqual(len(policy['covered_recoveries']), 4)
        self.assertIn('60 s status wait', policy['unchanged'])

    def test_rsp_wire_facts_counts_the_atomic_journal_events(self):
        with tempfile.TemporaryDirectory() as tmp:
            rows = [{'event': 'rx', 'raw_hex': '2b'}, {'event': 'wire_frame_pending', 'payload_hex': '00'}, {'event': 'tx', 'raw_hex': '2b'},
                    {'event': 'packet_rx', 'payload_hex': '00'}]
            (Path(tmp) / 'rsp-traffic.jsonl').write_text(''.join(json.dumps(r) + '\n' for r in rows))
            facts = RUN9.rsp_wire_facts(tmp)
            self.assertEqual((facts['wire_frame_pending'], facts['packet_rx'], facts['wire_terminal'], facts['tx_ack_plus'], facts['rows']), (1, 1, 0, 1, 4))
            self.assertIn('error', RUN9.rsp_wire_facts(Path(tmp) / 'missing'))


if __name__ == '__main__':
    unittest.main()
