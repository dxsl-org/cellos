"""S10-8.1 offline tests: pause-credit recovery-wait deadlines (D8-1 = B).  No VM; a fake clock drives every deadline."""
import inspect
import json
import queue
import re
import sys
import tempfile
import threading
import time
import types
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_credit as CREDIT  # noqa: E402
import s108_env as ENV  # noqa: E402
import s108_run as RUN8  # noqa: E402
import s107_retained as RET  # noqa: E402
import common as COMMON  # noqa: E402

NS = 1_000_000_000
TICK = 20_000_000          # the retained Surface.wait poll interval (.02 s)


class Clock:
    def __init__(self):
        self.t = time.monotonic_ns()
        self.t0 = self.t
        self.on_sleep = []

    def now(self):
        return self.t

    def sleep(self, seconds):
        self.t += int(round(seconds * NS))
        for hook in self.on_sleep:
            hook(self)


class Observer:
    def __init__(self, pause_ns=0, phase='endless-terminal-stream'):
        self.pause_ns, self.phase = pause_ns, phase

    def check(self):
        pass


class Trap:
    """pause_ns must never be read."""

    phase = 'p'

    def check(self):
        pass

    @property
    def pause_ns(self):
        raise AssertionError('pause accounting read by a wait that must not be credited')


class Base(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        for name, replacement in (('now_ns', self.clock.now), ('sleep', self.clock.sleep)):
            patcher = mock.patch.object(CREDIT, name, replacement)
            patcher.start()
            self.addCleanup(patcher.stop)

    def machine(self, global_remaining_s=None):
        """Machine whose 900 s window ends `global_remaining_s` after the clock origin (default: far away)."""
        remaining = 800 if global_remaining_s is None else global_remaining_s
        # Surface.check() reads the REAL monotonic clock for the 900 s window; the credit module reads the fake one (same origin).
        return types.SimpleNamespace(alive=lambda: True, start_ns=self.clock.t0 - (900 - remaining) * NS)

    def headless(self, observer, **kwargs):
        return RUN8.CreditedHeadlessSurface(self.machine(**kwargs), observer)

    def pause_during_first(self, observer, wall_s):
        """Observer paused 100 % of the next `wall_s` seconds of wall time (the pause counter grows with the fake clock)."""
        end = self.clock.t + int(wall_s * NS)
        state = {'prev': self.clock.t}

        def hook(clock):
            upto = min(clock.t, end)
            if upto > state['prev']:
                observer.pause_ns += upto - state['prev']
            state['prev'] = clock.t
        self.clock.on_sleep.append(hook)


class Deadline(Base):
    def test_unpaused_wait_expires_exactly_at_the_original_10s(self):
        surface = self.headless(Observer())
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(str(raised.exception), 'timeout: new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['outcome'], 'timeout')
        self.assertEqual(record['pause_credited_ns'], 0)
        self.assertEqual(record['effective_timeout_s'], 10.0)
        self.assertAlmostEqual(record['waited_s'], 10.0, delta=.021)
        self.assertIsNone(record['capped_by'])
        self.assertEqual(record['original_timeout_s'], 10)
        self.assertEqual(record['phase'], 'endless-terminal-stream')

    def test_boundary_strict_less_than_like_the_retained_loop(self):
        start = self.clock.t
        surface = self.headless(Observer())
        with self.assertRaises(COMMON.Stop):                       # true only AT the deadline: never evaluated (retained `<`)
            surface.wait(lambda: self.clock.t >= start + 10 * NS, 10, 'new interrupt prompt')
        self.assertEqual(surface.pause_credits[0]['outcome'], 'timeout')
        start = self.clock.t
        surface = self.headless(Observer())
        value = surface.wait(lambda: 'ok' if self.clock.t >= start + 10 * NS - TICK else None, 10, 'new interrupt prompt')
        self.assertEqual(value, 'ok')                              # true one tick before the deadline: satisfied
        self.assertEqual(surface.pause_credits[0]['outcome'], 'satisfied')

    def test_extension_equals_the_pause_accumulated_inside_the_wait_exactly(self):
        observer = Observer()
        surface = self.headless(observer)
        self.pause_during_first(observer, 4)
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['pause_measured_in_wait_ns'], 4 * NS)
        self.assertEqual(record['pause_credited_ns'], 4 * NS)
        self.assertEqual(record['effective_timeout_s'], 14.0)
        self.assertAlmostEqual(record['waited_s'], 14.0, delta=.021)
        self.assertEqual(record['effective_deadline_ns'] - record['original_deadline_ns'], record['pause_credited_ns'])

    def test_pause_before_the_wait_is_not_credited(self):
        surface = self.headless(Observer(pause_ns=50 * NS))        # accumulated by earlier phases
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual((record['pause_at_start_ns'], record['pause_credited_ns']), (50 * NS, 0))
        self.assertAlmostEqual(record['waited_s'], 10.0, delta=.021)

    def test_credit_lets_a_late_prompt_succeed(self):
        observer = Observer()
        start = self.clock.t
        surface = self.headless(observer)
        self.pause_during_first(observer, 4)
        value = surface.wait(lambda: 'prompt' if self.clock.t >= start + 12 * NS else None, 10, 'new interrupt prompt')
        self.assertEqual(value, 'prompt')                          # plain wall 10 s would have stopped at 10.0 s
        record = surface.pause_credits[0]
        self.assertEqual(record['outcome'], 'satisfied')
        self.assertEqual(record['pause_credited_ns'], 4 * NS)

    def test_never_beyond_the_phase_maximum(self):
        observer = Observer()
        surface = self.headless(observer)
        self.pause_during_first(observer, 10_000)                  # observer paused 100 % of the time
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['effective_timeout_s'], CREDIT.PHASE_MAX_S)
        self.assertEqual(record['capped_by'], 'phase_max')
        self.assertLessEqual(record['waited_s'], CREDIT.PHASE_MAX_S + .021)
        self.assertEqual(record['pause_credited_ns'], (CREDIT.PHASE_MAX_S - 10) * NS)
        self.assertGreater(record['pause_measured_in_wait_ns'], record['pause_credited_ns'])

    def test_never_beyond_the_900s_global_window(self):
        observer = Observer()
        surface = self.headless(observer, global_remaining_s=30)
        self.pause_during_first(observer, 10_000)
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['effective_timeout_s'], 30.0)
        self.assertEqual(record['capped_by'], 'global_window')
        self.assertAlmostEqual((self.clock.t - self.clock.t0) / NS, 30, delta=.021)

    def test_global_window_shorter_than_the_original_wait_still_wins(self):
        surface = self.headless(Observer(), global_remaining_s=5)
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(surface.pause_credits[0]['effective_timeout_s'], 5.0)
        self.assertEqual(surface.pause_credits[0]['pause_credited_ns'], 0)

    def test_constants_are_the_retained_deadlines(self):
        self.assertEqual((CREDIT.ORIGINAL_RECOVERY_S, CREDIT.PHASE_MAX_S, CREDIT.GLOBAL_WINDOW_S), (10, 120, 900))
        self.assertEqual(ENV.DEADLINES, {'command_s': 120, 'status_s': 60, 'total_window_s': 900})


class Isolation(Base):
    """No effect on any other timeout: delegation to the retained method, pause accounting never read."""

    def test_other_headless_waits_are_untouched(self):
        surface = self.headless(Trap())
        self.assertEqual(surface.wait(lambda: 'now', 120, 'top real cell row'), 'now')
        self.assertEqual(surface.wait(lambda: 'now', 10, 'some other 10 s wait'), 'now')
        self.assertEqual(surface.wait(lambda: 'now', 11, 'new interrupt prompt'), 'now')
        self.assertEqual(surface.wait(lambda: 'now', 10, 'exact initial idle prompt prefix'), 'now')
        with self.assertRaises(COMMON.Stop) as raised:             # a timeout of another wait is the retained real-time one
            surface.wait(lambda: False, .05, 'new interrupt prompt', interval=.01)
        self.assertEqual(str(raised.exception), 'timeout: new interrupt prompt')
        self.assertEqual(surface.__dict__.get('pause_credits', []), [])

    def test_other_headless_waits_call_the_retained_method(self):
        surface = self.headless(Trap())
        with mock.patch.object(COMMON.Surface, 'wait', return_value='retained') as retained:
            self.assertEqual(surface.wait('pred', 120, 'x', .5), 'retained')
            retained.assert_called_once_with('pred', 120, 'x', .5)

    def test_status_and_command_deadlines_are_not_credited(self):
        for klass in (RUN8.CreditedHeadlessSurface, RUN8.CreditedDesktopSurface):
            self.assertNotIn('status', vars(CREDIT.CreditedRecoveryMixin))
            self.assertNotIn('command', vars(CREDIT.CreditedRecoveryMixin))
            self.assertNotIn('pause', vars(CREDIT.CreditedRecoveryMixin))
            self.assertIs(klass.pause, COMMON.Surface.pause)
        self.assertEqual(ENV.DEADLINES['status_s'], 60)
        self.assertEqual(ENV.DEADLINES['command_s'], 120)

    def test_only_two_methods_are_overridden(self):
        self.assertEqual({n for n in vars(CREDIT.CreditedRecoveryMixin) if not n.startswith('__')},
                         {'wait', 'gui_fresh', '_credit_ledger', '_credit_start'})


class FailClosed(Base):
    def run_wait(self, observer, **kwargs):
        calls = []
        surface = self.headless(observer, **kwargs)

        def predicate():
            calls.append(1)
            return False
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(predicate, 10, 'new interrupt prompt')
        self.assertEqual(calls, [], 'the predicate must not run when the accounting is unavailable')
        self.assertEqual(self.clock.t, self.clock.t0, 'no time may pass')
        return str(raised.exception), surface

    def test_missing_pause_counter(self):
        text, surface = self.run_wait(types.SimpleNamespace(check=lambda: None, phase='p'))
        self.assertIn('pause_ns', text)
        self.assertEqual(surface.__dict__.get('pause_credits', []), [])

    def test_invalid_pause_counters(self):
        for value in (None, 'x', -1, 1.5, True):
            with self.subTest(value=value):
                self.clock.t = self.clock.t0
                text, _surface = self.run_wait(Observer(pause_ns=value))
                self.assertIn('pause_ns', text)

    def test_no_observer(self):
        surface = RUN8.CreditedHeadlessSurface(self.machine(), None)
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: True, 10, 'new interrupt prompt')
        self.assertIn('no observer', str(raised.exception))

    def test_machine_start_unavailable(self):
        surface = RUN8.CreditedHeadlessSurface(types.SimpleNamespace(alive=lambda: True, start_ns=None), Observer())
        surface.check = lambda: None
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: True, 10, 'new interrupt prompt')
        self.assertIn('machine start time unavailable', str(raised.exception))

    def test_counter_going_backwards_stops_the_wait(self):
        observer = Observer(pause_ns=5 * NS)
        surface = self.headless(observer)
        self.clock.on_sleep.append(lambda clock: setattr(observer, 'pause_ns', 4 * NS))
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertIn('backwards', str(raised.exception))
        self.assertTrue(surface.pause_credits[0]['outcome'].startswith('stopped: Stop'))

    def test_stop_inside_the_wait_is_recorded_and_propagates(self):
        class Dying(Observer):
            def check(self):
                raise COMMON.Stop('observation blocker: x')
        surface = self.headless(Dying())
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(len(surface.pause_credits), 1)
        self.assertTrue(surface.pause_credits[0]['outcome'].startswith('stopped: Stop: observation blocker'))


class Accounting(unittest.TestCase):
    def test_source_of_the_retained_counter_is_the_expected_method(self):
        run = inspect.getsource(RET.OBS.Observer.run)
        self.assertIn('stop_ns=time.monotonic_ns()', run)
        self.assertIn('self._stopped(reply)', run)
        self.assertIn('self.pause_ns+=time.monotonic_ns()-stop_ns', run)
        self.assertEqual(run.count('pause_ns'), 1)
        self.assertIn("'observer_pause_ns':self.pause_ns", inspect.getsource(RET.OBS.Observer.write_results))
        self.assertIn('self.pause_ns=0', inspect.getsource(RET.OBS.Observer.__init__))

    def test_pause_source_reads_exactly_the_retained_counter_of_a_real_observer_run(self):
        """The retained Observer.run/write_results execute on a stand-in; PauseSource equals observer.pause_ns and observation.json."""
        Observer = RET.OBS.Observer
        with tempfile.TemporaryDirectory() as tmp:
            work = Path(tmp)
            (work / 'evidence').mkdir()
            stops = []

            class Client:
                _state = 'stopped'

                def connect(self):
                    pass

                def resume(self):
                    pass

                def wait_stop(self, timeout=None):
                    return b'T05thread:1;'

                def read_register(self, name):
                    return 0

                def close(self):
                    pass

            fake = types.SimpleNamespace(m=types.SimpleNamespace(work=work), client=Client(), done=threading.Event(), ready=threading.Event(),
                                         requests=queue.Queue(), watches=set(), breaks=set(), error=None, pause_ns=0, instances={},
                                         profile='headless', focus=None, counters=0, stack_events=[], snapshots=[], draw_events=[],
                                         events=(work / 'e').open('w'), coverage=(work / 'c').open('w'), add_bp=lambda a: None)

            def stopped(reply):
                stops.append(time.monotonic_ns())
                time.sleep(.02)
                if len(stops) == 3:
                    fake.done.set()
            fake._stopped = stopped
            fake.write_results = lambda: Observer.write_results(fake)
            Observer.run(fake)
            self.assertEqual(len(stops), 3)
            self.assertGreaterEqual(fake.pause_ns, 3 * 20_000_000)
            self.assertLess(fake.pause_ns, 3 * 200_000_000)
            source = CREDIT.PauseSource(fake)
            self.assertEqual(source.read(), fake.pause_ns)
            observation = json.loads((work / 'evidence/observation.json').read_text())
            self.assertEqual(observation['observer_pause_ns'], fake.pause_ns)
            self.assertEqual(observation['status'], 'COMPLETE')

    def test_credit_is_the_counter_difference(self):
        clock = Clock()
        with mock.patch.object(CREDIT, 'now_ns', clock.now):
            observer = Observer(pause_ns=7 * NS)
            wait = CREDIT.CreditedWait(label='l', phase='p', pause=CREDIT.PauseSource(observer), machine_start_ns=clock.t0)
            observer.pause_ns += 1_234_567_891
            self.assertTrue(wait.active())
            self.assertEqual(wait.credited_ns(), 1_234_567_891)
            self.assertEqual(wait.effective_deadline_ns - wait.base_deadline_ns, observer.pause_ns - 7 * NS)


class DeskPath(Base):
    """Desktop: the credited loop replaces Desk.wait_for ONLY for ('runtime new GUI prompt', 10) and only inside gui_fresh."""

    PROMPT = COMMON.D.PROMPT

    class Desk:
        def __init__(self, ready_at, clock):
            self.clock, self.ready_at = clock, ready_at
            self.model, self.state, self.calls, self.retained_calls, self.failures, self.failure_records = [], {}, [], [], [], []
            self.shots = []

        def wait_for(self, pred, what, timeout, interval=1.0):
            self.retained_calls.append((what, timeout))
            return 'retained', what

        def health_check(self):
            self.calls.append('health')

        def grab(self):
            self.calls.append('grab')
            return self.clock.t

        def read_screen(self, image):
            if image >= self.ready_at:
                rows = ['', 'banner', COMMON.D.PROMPT]
                return [r.rstrip() if i == len(rows) - 1 else r for i, r in enumerate(rows)], (len(COMMON.D.PROMPT), 2)
            return ['', 'banner', 'top output'], (0, 2)

        def shot(self, name, image):
            self.shots.append((name, image))

        def retain_failure(self, error, context):
            self.failures.append((error, context))

    def surface(self, desk, observer, **kwargs):
        surface = RUN8.CreditedDesktopSurface(self.machine(**kwargs), observer, types.SimpleNamespace(phase='RUN', terminal_reason=None))
        surface.d = desk
        return surface

    def test_credited_gui_fresh_succeeds_after_the_original_10s(self):
        observer = Observer(phase='endless-collected-cat')
        desk = self.Desk(self.clock.t + 12 * NS, self.clock)
        surface = self.surface(desk, observer)
        self.pause_during_first(observer, 4)
        rows = surface.gui_fresh()
        self.assertEqual(rows[-1], self.PROMPT.rstrip())
        self.assertEqual(desk.model[-1], self.PROMPT)
        self.assertEqual(desk.shots[0][0], 'recovered')
        record = surface.pause_credits[0]
        self.assertEqual((record['label'], record['outcome'], record['pause_credited_ns']), ('runtime new GUI prompt', 'satisfied', 4 * NS))
        self.assertEqual(record['phase'], 'endless-collected-cat')
        self.assertEqual(desk.retained_calls, [])
        self.assertNotIn('wait_for', desk.__dict__, 'the instance patch must be removed after gui_fresh')
        self.assertEqual(desk.calls[:2], ['health', 'grab'])

    def test_credited_gui_fresh_timeout_retains_the_failure_and_restores_the_desk(self):
        observer = Observer()
        desk = self.Desk(self.clock.t + 10_000 * NS, self.clock)
        surface = self.surface(desk, observer)
        self.pause_during_first(observer, 3)
        with self.assertRaises(COMMON.Stop) as raised:
            surface.gui_fresh()
        self.assertEqual(str(raised.exception), 'timeout waiting for runtime new GUI prompt')
        self.assertEqual(len(desk.failures), 1)
        context = desk.failures[0][1]
        self.assertEqual((context['wait_for'], context['timeout_seconds']), ('runtime new GUI prompt', 10))
        self.assertEqual(context['pause_credit']['effective_timeout_s'], 13.0)
        self.assertNotIn('wait_for', desk.__dict__)
        self.assertEqual(surface.pause_credits[0]['outcome'], 'timeout')

    def test_other_desk_waits_inside_gui_fresh_use_the_retained_method(self):
        desk = self.Desk(0, self.clock)
        surface = self.surface(desk, Trap())

        def fake_gui_fresh(self):
            self.d.wait_for(lambda image: 1, 'runtime top title bar', 180, interval=.2)
            self.d.wait_for(lambda image: 1, 'runtime new GUI prompt', 11, interval=.1)
            return 'done'
        with mock.patch.object(COMMON.Surface, 'gui_fresh', fake_gui_fresh):
            self.assertEqual(surface.gui_fresh(), 'done')
        self.assertEqual(desk.retained_calls, [('runtime top title bar', 180), ('runtime new GUI prompt', 11)])
        self.assertEqual(surface.__dict__.get('pause_credits', []), [])

    def test_health_check_stop_propagates_and_is_recorded(self):
        observer = Observer()
        desk = self.Desk(0, self.clock)

        def dying():
            raise COMMON.Stop('owned QEMU exited')
        desk.health_check = dying
        surface = self.surface(desk, observer)
        with self.assertRaises(COMMON.Stop) as raised:
            surface.gui_fresh()
        self.assertEqual(str(raised.exception), 'owned QEMU exited')
        self.assertTrue(surface.pause_credits[0]['outcome'].startswith('stopped: Stop: owned QEMU exited'))
        self.assertEqual(len(desk.failures), 1)

    def test_fail_closed_without_pause_accounting(self):
        desk = self.Desk(0, self.clock)
        surface = self.surface(desk, types.SimpleNamespace(check=lambda: None, phase='p'))
        with self.assertRaises(COMMON.Stop):
            surface.gui_fresh()
        self.assertEqual(desk.calls, [])
        self.assertNotIn('wait_for', desk.__dict__)


class Interrupt(Base):
    """The retained `Surface.interrupt` itself runs through the credited wait (headless) and gui_fresh (desktop)."""

    def test_headless_interrupt_uses_the_credited_wait(self):
        observer = Observer()
        surface = self.headless(observer)
        sent, state = [], {'idle_at': self.clock.t + 13 * NS}
        surface.sh = types.SimpleNamespace(send=sent.append, idle=lambda count: self.clock.t >= state['idle_at'])
        surface.status = lambda: 130
        self.pause_during_first(observer, 4)
        result = surface.interrupt((3, 0))
        self.assertEqual(sent, [b'\x03'])
        self.assertEqual(result['status'], 130)
        self.assertEqual(len(surface.pause_credits), 1)
        self.assertEqual(surface.pause_credits[0]['outcome'], 'satisfied')
        self.assertEqual(surface.pause_credits[0]['label'], 'new interrupt prompt')

    def test_desktop_interrupt_uses_gui_fresh(self):
        observer = Observer()
        desk = DeskPath.Desk(self.clock.t + 12 * NS, self.clock)
        desk.ctrl_c = lambda: desk.calls.append('ctrl_c')
        surface = RUN8.CreditedDesktopSurface(self.machine(), observer, types.SimpleNamespace(phase='RUN', terminal_reason=None))
        surface.d = desk
        surface.status = lambda: 130
        self.pause_during_first(observer, 4)
        result = surface.interrupt(None)
        self.assertEqual(result['status'], 130)
        self.assertEqual(surface.pause_credits[0]['label'], 'runtime new GUI prompt')
        self.assertEqual(desk.calls[0], 'ctrl_c')


class RetainedEquivalence(unittest.TestCase):
    """With zero pause the credited loops behave like the retained loops (same call pattern, same Stop text, same result)."""

    class Desk:
        def __init__(self):
            self.log, self.failure_records = [], []

        def health_check(self):
            self.log.append('h')

        def grab(self):
            self.log.append('g')
            return 'img'

    def test_desk_loop_matches_checkeddesk_wait_for(self):
        for succeed_on in (1, 3):
            retained, credited = self.Desk(), self.Desk()
            counts = {'r': 0, 'c': 0}

            def pred_for(key):
                def pred(image):
                    counts[key] += 1
                    return 'got' if counts[key] >= succeed_on else None
                return pred
            first = COMMON.CheckedDesk.wait_for(retained, pred_for('r'), 'what', 5, interval=0)
            wait = CREDIT.CreditedWait(label='what', phase='p', pause=CREDIT.PauseSource(Observer()), machine_start_ns=time.monotonic_ns(),
                                       original_s=5)
            second = CREDIT.desk_wait_for(credited, wait, pred_for('c'), 'what', 5, 0)
            self.assertEqual(first, second)
            self.assertEqual(retained.log, credited.log)
        retained, credited = self.Desk(), self.Desk()
        with self.assertRaises(COMMON.Stop) as a:
            COMMON.CheckedDesk.wait_for(retained, lambda image: None, 'what', .05, interval=.01)
        wait = CREDIT.CreditedWait(label='what', phase='p', pause=CREDIT.PauseSource(Observer()), machine_start_ns=time.monotonic_ns(), original_s=.05)
        with self.assertRaises(COMMON.Stop) as b:
            CREDIT.desk_wait_for(credited, wait, lambda image: None, 'what', .05, .01)
        self.assertEqual(str(a.exception), str(b.exception))

    def test_surface_loop_matches_surface_wait(self):
        class S:
            def __init__(self):
                self.log = []

            def check(self):
                self.log.append('c')
        for succeed_on in (1, 4):
            retained, credited = S(), S()
            counts = {'r': 0, 'c': 0}

            def pred_for(key, log):
                def pred():
                    counts[key] += 1
                    log.append('p')
                    return 'got' if counts[key] >= succeed_on else None
                return pred
            first = COMMON.Surface.wait(retained, pred_for('r', retained.log), 5, 'lbl', 0)
            wait = CREDIT.CreditedWait(label='lbl', phase='p', pause=CREDIT.PauseSource(Observer()), machine_start_ns=time.monotonic_ns(), original_s=5)
            second = CREDIT.surface_wait(credited, wait, pred_for('c', credited.log), 'lbl', 0)
            self.assertEqual(first, second)
            self.assertEqual(retained.log, credited.log)
        with self.assertRaises(COMMON.Stop) as a:
            COMMON.Surface.wait(S(), lambda: None, .05, 'lbl', .01)
        wait = CREDIT.CreditedWait(label='lbl', phase='p', pause=CREDIT.PauseSource(Observer()), machine_start_ns=time.monotonic_ns(), original_s=.05)
        with self.assertRaises(COMMON.Stop) as b:
            CREDIT.surface_wait(S(), wait, lambda: None, 'lbl', .01)
        self.assertEqual(str(a.exception), str(b.exception))


class Survey(unittest.TestCase):
    """Drift guard: the retained source has exactly two 10 s recovery waits and everything else keeps its own deadline."""

    def sources(self):
        return {'common.Surface': inspect.getsource(COMMON.Surface),
                'HookSurface': inspect.getsource(RET.HS.HookSurface), 'CoordinatedSurface': inspect.getsource(RET.C.CoordinatedSurface),
                'TemporalSurface': inspect.getsource(RET.G.TemporalSurface)}

    def waits(self, text):
        found = {(label, int(timeout)) for timeout, label in re.findall(r"self\.wait\(.*?,\s*(\d+)\s*,\s*'([^']+)'", text)}
        found |= {(label, int(timeout)) for label, timeout in re.findall(r"wait_for\(\w+,\s*'([^']+)',\s*(\d+)", text)}
        return found

    def test_the_ten_second_waits_are_exactly_the_two_credited_ones(self):
        found = set()
        for text in self.sources().values():
            found |= {item for item in self.waits(text) if item[1] == 10}
        self.assertEqual(found, {CREDIT.HEADLESS_KEY, CREDIT.GUI_KEY})

    def test_full_wait_table_of_the_retained_surface(self):
        table = self.waits(self.sources()['common.Surface'])
        self.assertEqual(table, {('exact initial idle prompt prefix', 900), ('desktop ready', 900), ('Terminal surface', 300),
                                 ('top real cell row', 120), ('live JSON terminal stream', 120), ('new interrupt prompt', 10),
                                 ('runtime top title bar', 180), ('runtime new GUI prompt', 10), ('runtime live JSON terminal stream', 120)})
        self.assertIn("self.command('status','echo st=$?',60)", self.sources()['common.Surface'])
        self.assertIn('def command(self,name,cmd,timeout=120)', self.sources()['common.Surface'])

    def test_fixed_pause_durations_are_not_deadlines(self):
        durations = sorted(set(int(n) for n in re.findall(r'self\.pause\((\d+)\)', self.sources()['common.Surface'])))
        self.assertEqual(durations, [2, 3, 4, 10])                  # observation durations (sleeps), never wait-for-condition deadlines
        self.assertIn('def pause(self,seconds)', self.sources()['common.Surface'])

    def test_interrupt_flows_call_exactly_the_credited_entry_points(self):
        text = inspect.getsource(COMMON.Surface.interrupt)
        self.assertIn("self.wait(lambda:self.sh.idle(count=before[0]+1),10,'new interrupt prompt')", text)
        self.assertIn('self.gui_fresh()', text)
        self.assertIn("d.wait_for(pred,'runtime new GUI prompt',10,interval=.1)", inspect.getsource(COMMON.Surface.gui_fresh))
        for later in ('self.status()',):
            self.assertIn(later, text)                              # status after the recovery keeps its 60 s deadline


if __name__ == '__main__':
    unittest.main()
