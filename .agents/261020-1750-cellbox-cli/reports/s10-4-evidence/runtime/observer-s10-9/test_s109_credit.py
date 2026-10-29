"""S10-9.1 offline tests: latency-recording recovery waits (D9-1 = B).  No VM; a fake clock drives every deadline.

Contract under test: the two original 10 s recovery waits (headless `new interrupt prompt`, desktop `runtime new GUI prompt`) wait up to
120 s of observer-pause-credited wall time (S10-8 pause accounting), capped by the unchanged 900 s window, and RECORD time-to-prompt,
credited pause, effective deadline and outcome; the product 10 s criterion is `not_evaluated` (never met/failed); a prompt that does not
return within the budget stops the run exactly as before; every other deadline is untouched.
"""
import inspect
import json
import re
import sys
import time
import types
import unittest
from pathlib import Path
from unittest import mock

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_credit as CREDIT  # noqa: E402
import s109_env as ENV  # noqa: E402
import s109_run as RUN9  # noqa: E402
import s108_credit as CREDIT8  # noqa: E402
import s107_retained as RET  # noqa: E402
import common as COMMON  # noqa: E402

NS = 1_000_000_000
TICK = 20_000_000          # the retained Surface.wait poll interval (.02 s)
VERDICT_WORDS = re.compile(r'(^|_)(met|failed|passed|pass|fail|within|ok|success|verdict)(_|$)')


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
    def __init__(self, pause_ns=0, phase='endless-collected-cat'):
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
        raise AssertionError('pause accounting read by a wait that must not be latency-recorded')


class Base(unittest.TestCase):
    def setUp(self):
        self.clock = Clock()
        for name, replacement in (('now_ns', self.clock.now), ('sleep', self.clock.sleep)):
            patcher = mock.patch.object(CREDIT8, name, replacement)      # the S10-8 module owns the clock both classes read
            patcher.start()
            self.addCleanup(patcher.stop)

    def machine(self, global_remaining_s=None):
        remaining = 800 if global_remaining_s is None else global_remaining_s
        return types.SimpleNamespace(alive=lambda: True, start_ns=self.clock.t0 - (900 - remaining) * NS)

    def headless(self, observer, **kwargs):
        return RUN9.LatencyHeadlessSurface(self.machine(**kwargs), observer)

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


class Budget(Base):
    def test_unpaused_wait_runs_to_120s_not_10s_then_stops_as_before(self):
        surface = self.headless(Observer())
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(str(raised.exception), 'timeout: new interrupt prompt')       # the retained stop text
        record = surface.pause_credits[0]
        self.assertEqual(record['outcome'], 'timeout')
        self.assertEqual(record['effective_timeout_s'], 120.0)
        self.assertAlmostEqual(record['waited_s'], 120.0, delta=.021)
        self.assertEqual(record['pause_credited_ns'], 0)
        self.assertIsNone(record['capped_by'])
        self.assertIsNone(record['time_to_prompt_s'])
        self.assertEqual((record['original_timeout_s'], record['budget_s'], record['global_window_s']), (10, 120, 900))

    def test_a_prompt_after_the_original_10s_is_recorded_not_judged(self):
        start = self.clock.t
        surface = self.headless(Observer(phase='endless-terminal-stream'))
        value = surface.wait(lambda: 'prompt' if self.clock.t >= start + 14 * NS else None, 10, 'new interrupt prompt')
        self.assertEqual(value, 'prompt')                                              # the S10-8 10 s wait would have stopped the run at 10 s
        record = surface.pause_credits[0]
        self.assertEqual(record['outcome'], 'satisfied')
        self.assertAlmostEqual(record['time_to_prompt_s'], 14.0, delta=.021)
        self.assertEqual(record['pause_measured_in_wait_ns'], 0)
        self.assertAlmostEqual(record['time_to_prompt_minus_measured_pause_s'], 14.0, delta=.021)
        self.assertEqual(record['phase'], 'endless-terminal-stream')
        self.assertEqual(record['recovery_10s_criterion'], 'not_evaluated')

    def test_the_budget_is_pause_credited_and_not_capped_at_120s_of_wall_time(self):
        observer = Observer()
        start = self.clock.t
        surface = self.headless(observer)
        self.pause_during_first(observer, 85)                                          # S10-8 headless: observer paused 85 s of one wait
        value = surface.wait(lambda: 'prompt' if self.clock.t >= start + 150 * NS else None, 10, 'new interrupt prompt')
        self.assertEqual(value, 'prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['pause_measured_in_wait_ns'], 85 * NS)
        self.assertEqual(record['pause_credited_ns'], 85 * NS)
        self.assertAlmostEqual(record['time_to_prompt_s'], 150.0, delta=.021)           # wall time beyond 120 s is allowed: only pause-credited
        self.assertAlmostEqual(record['time_to_prompt_minus_measured_pause_s'], 65.0, delta=.021)
        self.assertEqual(record['effective_deadline_ns'] - record['base_deadline_ns'], record['pause_credited_ns'])
        self.assertEqual(record['effective_deadline_ns'] - record['original_10s_deadline_ns'], 110 * NS + 85 * NS)

    def test_no_prompt_stops_after_120s_of_unpaused_time_plus_the_credit(self):
        observer = Observer()
        surface = self.headless(observer)
        self.pause_during_first(observer, 30)
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['effective_timeout_s'], 150.0)
        self.assertAlmostEqual(record['waited_s'], 150.0, delta=.021)
        self.assertEqual(record['outcome'], 'timeout')
        self.assertAlmostEqual(record['waited_s'] - record['pause_measured_in_wait_ns'] / NS, 120.0, delta=.021)

    def test_boundary_strict_less_than_like_the_retained_loop(self):
        start = self.clock.t
        surface = self.headless(Observer())
        with self.assertRaises(COMMON.Stop):                       # true only AT the deadline: never evaluated (retained `<`)
            surface.wait(lambda: self.clock.t >= start + 120 * NS, 10, 'new interrupt prompt')
        self.assertEqual(surface.pause_credits[0]['outcome'], 'timeout')
        start = self.clock.t
        surface = self.headless(Observer())
        value = surface.wait(lambda: 'ok' if self.clock.t >= start + 120 * NS - TICK else None, 10, 'new interrupt prompt')
        self.assertEqual(value, 'ok')
        self.assertEqual(surface.pause_credits[0]['outcome'], 'satisfied')

    def test_pause_before_the_wait_is_not_credited(self):
        surface = self.headless(Observer(pause_ns=50 * NS))
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual((record['pause_at_start_ns'], record['pause_credited_ns']), (50 * NS, 0))
        self.assertAlmostEqual(record['waited_s'], 120.0, delta=.021)

    def test_never_beyond_the_900s_global_window(self):
        observer = Observer()
        surface = self.headless(observer, global_remaining_s=300)
        self.pause_during_first(observer, 10_000)                  # observer paused 100 % of the time: the credit alone would never end
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        record = surface.pause_credits[0]
        self.assertEqual(record['effective_timeout_s'], 300.0)
        self.assertEqual(record['capped_by'], 'global_window')
        self.assertAlmostEqual((self.clock.t - self.clock.t0) / NS, 300, delta=.021)
        self.assertGreater(record['pause_measured_in_wait_ns'], record['pause_credited_ns'])

    def test_global_window_shorter_than_the_budget_still_wins(self):
        surface = self.headless(Observer(), global_remaining_s=45)
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(surface.pause_credits[0]['effective_timeout_s'], 45.0)
        self.assertEqual(surface.pause_credits[0]['pause_credited_ns'], 0)

    def test_constants_are_the_retained_deadlines(self):
        self.assertEqual((CREDIT.ORIGINAL_RECOVERY_S, CREDIT.RECOVERY_BUDGET_S, CREDIT.GLOBAL_WINDOW_S), (10, 120, 900))
        self.assertEqual(ENV.DEADLINES, {'command_s': 120, 'status_s': 60, 'total_window_s': 900})
        self.assertEqual(CREDIT.RECOVERY_BUDGET_S, ENV.DEADLINES['command_s'])
        self.assertEqual(CREDIT.GLOBAL_WINDOW_S, CREDIT8.GLOBAL_WINDOW_S)
        self.assertEqual(CREDIT.HEADLESS_KEY, ('new interrupt prompt', 10))
        self.assertEqual(CREDIT.GUI_KEY, ('runtime new GUI prompt', 10))


class NotEvaluated(Base):
    """The product 10 s criterion is never judged: every record says `not_evaluated` and carries no met/failed/pass/fail field."""

    def records(self):
        out = []
        for prompt_after in (3, 14, None):
            self.clock.t = self.clock.t0
            start = self.clock.t
            surface = self.headless(Observer())
            predicate = (lambda s=start, p=prompt_after: 'x' if p is not None and self.clock.t >= s + p * NS else None)
            try:
                surface.wait(predicate, 10, 'new interrupt prompt')
            except COMMON.Stop:
                pass
            out.append(surface.pause_credits[0])
        return out

    def test_every_record_is_not_evaluated_and_has_no_verdict_field(self):
        for record in self.records():
            self.assertEqual(record['recovery_10s_criterion'], 'not_evaluated')
            self.assertIn('NOT EVALUATED', record['criterion_note'])
            for key, value in record.items():
                self.assertFalse(VERDICT_WORDS.search(key), key)
                self.assertNotIsInstance(value, bool, key)               # no boolean verdict-like field either

    def test_a_prompt_inside_and_outside_10s_get_the_same_classification(self):
        under, over, never = self.records()
        self.assertEqual(under['recovery_10s_criterion'], over['recovery_10s_criterion'])
        self.assertEqual(under['outcome'], over['outcome'])
        self.assertEqual(never['outcome'], 'timeout')
        self.assertEqual({k for k in under}, {k for k in over})

    def test_source_uses_the_original_10s_constant_only_to_identify_the_wait_and_label_the_record(self):
        code = [line.strip() for line in inspect.getsource(CREDIT.LatencyWait).splitlines() if 'ORIGINAL_RECOVERY_S' in line]
        self.assertEqual(len(code), 2, code)
        self.assertTrue(any('original_10s_deadline_ns = self.began_ns + ORIGINAL_RECOVERY_S * NS' in line for line in code))   # identification only
        self.assertTrue(any("'original_timeout_s': ORIGINAL_RECOVERY_S" in line for line in code))
        finish = inspect.getsource(CREDIT.LatencyWait.finish)
        self.assertNotIn('original_10s_deadline_ns <', finish)
        self.assertNotRegex(finish, r'[<>]=?\s*(self\.)?original_10s')
        self.assertEqual(CREDIT.CRITERION, 'not_evaluated')

    def test_run_level_policy_and_runner_sources_state_not_evaluated(self):
        self.assertEqual(RUN9.RECOVERY_WAIT_POLICY['recovery_10s_criterion'], 'not_evaluated')
        self.assertIn('NOT EVALUATED', RUN9.RECOVERY_WAIT_POLICY['ceiling'])
        for function in (RUN9.run_headless, RUN9.run_desktop, RUN9.finish):
            self.assertIn('CREDIT.CRITERION', inspect.getsource(function))
        self.assertIn("'recovery_waits'", inspect.getsource(RUN9.finish))
        json.dumps(RUN9.RECOVERY_WAIT_POLICY)


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

    def test_status_and_command_deadlines_are_not_touched(self):
        for klass in (RUN9.LatencyHeadlessSurface, RUN9.LatencyDesktopSurface):
            for name in ('status', 'command', 'pause', 'ready', 'sample_idle', 'streams', 'top_idle', 'interrupt'):
                self.assertNotIn(name, vars(CREDIT.LatencyRecoveryMixin))
            self.assertIs(klass.pause, COMMON.Surface.pause)
        self.assertEqual(ENV.DEADLINES['status_s'], 60)
        self.assertEqual(ENV.DEADLINES['command_s'], 120)

    def test_the_latency_mixin_replaces_only_the_wait_object_the_two_entry_points_are_the_s10_8_ones(self):
        self.assertEqual({n for n in vars(CREDIT.LatencyRecoveryMixin) if not n.startswith('__')}, {'_credit_start'})
        self.assertIs(CREDIT.LatencyRecoveryMixin.wait, CREDIT8.CreditedRecoveryMixin.wait)
        self.assertIs(CREDIT.LatencyRecoveryMixin.gui_fresh, CREDIT8.CreditedRecoveryMixin.gui_fresh)
        for klass in (RUN9.LatencyHeadlessSurface, RUN9.LatencyDesktopSurface):
            self.assertIs(klass.__mro__[1], CREDIT.LatencyRecoveryMixin)
            self.assertIs(klass.__mro__[2], CREDIT8.CreditedRecoveryMixin)


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
        surface = RUN9.LatencyHeadlessSurface(self.machine(), None)
        with self.assertRaises(COMMON.Stop) as raised:
            surface.wait(lambda: True, 10, 'new interrupt prompt')
        self.assertIn('no observer', str(raised.exception))

    def test_machine_start_unavailable(self):
        surface = RUN9.LatencyHeadlessSurface(types.SimpleNamespace(alive=lambda: True, start_ns=None), Observer())
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
        self.assertIsNone(surface.pause_credits[0]['time_to_prompt_s'])

    def test_stop_inside_the_wait_is_recorded_and_propagates(self):
        class Dying(Observer):
            def check(self):
                raise COMMON.Stop('observation blocker: x')
        surface = self.headless(Dying())
        with self.assertRaises(COMMON.Stop):
            surface.wait(lambda: False, 10, 'new interrupt prompt')
        self.assertEqual(len(surface.pause_credits), 1)
        self.assertTrue(surface.pause_credits[0]['outcome'].startswith('stopped: Stop: observation blocker'))


class DeskPath(Base):
    """Desktop: the latency loop replaces Desk.wait_for ONLY for ('runtime new GUI prompt', 10) and only inside gui_fresh."""

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
        surface = RUN9.LatencyDesktopSurface(self.machine(**kwargs), observer, types.SimpleNamespace(phase='RUN', terminal_reason=None))
        surface.d = desk
        return surface

    def test_gui_fresh_records_a_prompt_returned_after_the_original_10s(self):
        observer = Observer(phase='endless-collected-cat')
        desk = self.Desk(self.clock.t + 14 * NS, self.clock)            # S10-8 rg-large: 14.22 s wall, 3.99 s credited pause
        surface = self.surface(desk, observer)
        self.pause_during_first(observer, 4)
        rows = surface.gui_fresh()
        self.assertEqual(rows[-1], self.PROMPT.rstrip())
        self.assertEqual(desk.model[-1], self.PROMPT)
        self.assertEqual(desk.shots[0][0], 'recovered')
        record = surface.pause_credits[0]
        self.assertEqual((record['label'], record['outcome'], record['pause_credited_ns']), ('runtime new GUI prompt', 'satisfied', 4 * NS))
        self.assertEqual(record['phase'], 'endless-collected-cat')
        self.assertAlmostEqual(record['time_to_prompt_s'], 14.0, delta=1.01)        # the desk poll is 0.1 s; fake grabs advance 0.1 s each
        self.assertEqual(record['recovery_10s_criterion'], 'not_evaluated')
        self.assertEqual(desk.retained_calls, [])
        self.assertNotIn('wait_for', desk.__dict__, 'the instance patch must be removed after gui_fresh')
        self.assertEqual(desk.calls[:2], ['health', 'grab'])

    def test_gui_fresh_without_a_prompt_stops_at_the_credited_120s_retains_the_failure_and_restores_the_desk(self):
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
        self.assertEqual(context['pause_credit']['effective_timeout_s'], 123.0)
        self.assertEqual(context['pause_credit']['recovery_10s_criterion'], 'not_evaluated')
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
        desk = self.Desk(0, self.clock)

        def dying():
            raise COMMON.Stop('owned QEMU exited')
        desk.health_check = dying
        surface = self.surface(desk, Observer())
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
    """The retained `Surface.interrupt` itself runs through the latency wait (headless) and gui_fresh (desktop); status keeps 60 s."""

    def test_headless_interrupt_records_a_late_recovery_and_then_runs_the_unchanged_status(self):
        observer = Observer()
        surface = self.headless(observer)
        sent, state = [], {'idle_at': self.clock.t + 40 * NS}
        surface.sh = types.SimpleNamespace(send=sent.append, idle=lambda count: self.clock.t >= state['idle_at'])
        surface.status = lambda: 130
        self.pause_during_first(observer, 25)
        result = surface.interrupt((3, 0))
        self.assertEqual(sent, [b'\x03'])
        self.assertEqual(result['status'], 130)
        self.assertEqual(len(surface.pause_credits), 1)
        record = surface.pause_credits[0]
        self.assertEqual((record['outcome'], record['label']), ('satisfied', 'new interrupt prompt'))
        self.assertAlmostEqual(record['time_to_prompt_s'], 40.0, delta=.021)
        self.assertEqual(record['pause_measured_in_wait_ns'], 25 * NS)

    def test_desktop_interrupt_uses_gui_fresh(self):
        observer = Observer()
        desk = DeskPath.Desk(self.clock.t + 12 * NS, self.clock)
        desk.ctrl_c = lambda: desk.calls.append('ctrl_c')
        surface = RUN9.LatencyDesktopSurface(self.machine(), observer, types.SimpleNamespace(phase='RUN', terminal_reason=None))
        surface.d = desk
        surface.status = lambda: 130
        self.pause_during_first(observer, 4)
        result = surface.interrupt(None)
        self.assertEqual(result['status'], 130)
        self.assertEqual(surface.pause_credits[0]['label'], 'runtime new GUI prompt')
        self.assertEqual(desk.calls[0], 'ctrl_c')


class Survey(unittest.TestCase):
    """Drift guard for the survey of the original recovery waits: the retained source has exactly two 10 s waits and every Ctrl-C
    recovery (collected-cat, terminal-stream, rg-large, interactive top; headless and desktop) goes through them."""

    def sources(self):
        return {'common.Surface': inspect.getsource(COMMON.Surface), 'HookSurface': inspect.getsource(RET.HS.HookSurface),
                'CoordinatedSurface': inspect.getsource(RET.C.CoordinatedSurface), 'TemporalSurface': inspect.getsource(RET.G.TemporalSurface)}

    def waits(self, text):
        found = {(label, int(timeout)) for timeout, label in re.findall(r"self\.wait\(.*?,\s*(\d+)\s*,\s*'([^']+)'", text)}
        found |= {(label, int(timeout)) for label, timeout in re.findall(r"wait_for\(\w+,\s*'([^']+)',\s*(\d+)", text)}
        return found

    def test_the_ten_second_waits_are_exactly_the_two_latency_recorded_ones(self):
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

    def test_fixed_pause_durations_are_observation_sleeps_not_recovery_deadlines(self):
        durations = sorted(set(int(n) for n in re.findall(r'self\.pause\((\d+)\)', self.sources()['common.Surface'])))
        self.assertEqual(durations, [2, 3, 4, 10])
        self.assertIn('def pause(self,seconds)', self.sources()['common.Surface'])

    def test_every_ctrl_c_recovery_site_goes_through_interrupt(self):
        text = inspect.getsource(COMMON.Surface)
        self.assertEqual(text.count('self.interrupt(before)'), 4, 'collected-cat, terminal-stream, rg-large, interactive-top')
        for phase in ("snapshot('collected-cat-buffered');cat_cancel=self.interrupt(before)", "cancel=self.interrupt(before)", "rg=self.interrupt(before)",
                      "result['interrupt']=self.interrupt(before)"):
            self.assertIn(phase, text)
        self.assertEqual(len([name for name in dir(COMMON.Surface) if 'interrupt' in name]), 1)
        interrupt = inspect.getsource(COMMON.Surface.interrupt)
        self.assertIn("self.wait(lambda:self.sh.idle(count=before[0]+1),10,'new interrupt prompt')", interrupt)
        self.assertIn('self.gui_fresh()', interrupt)
        self.assertIn("d.wait_for(pred,'runtime new GUI prompt',10,interval=.1)", inspect.getsource(COMMON.Surface.gui_fresh))
        self.assertIn('self.status()', interrupt)                  # status after the recovery keeps its 60 s deadline


if __name__ == '__main__':
    unittest.main()
