"""S10-8.1 pause-credit deadline wrapper (D8-1 = B).  New sibling code only; nothing retained is edited.

Rule (applies ONLY to the two original 10 s recovery waits of the retained `common.Surface`):

    effective_deadline = min( start + 10 s + P,                     P = observer pause accumulated inside THIS wait
                              start + PHASE_MAX_S,                  the phase's own maximum (see below)
                              machine.start_ns + 900 s )            the original global measurement window

`P` uses the retained accounting method unchanged: `Observer.pause_ns` is the running sum of COMPLETED
`_stopped`-handling intervals (`pause_ns += monotonic_ns() - stop_ns` in `Observer.run`), so P is the difference of that
counter between the end and the start of the wait.  An interval still being handled when the deadline is evaluated is not
credited until it completes (identical to the retained counter; nothing is estimated).  If the counter is missing, not an
int, negative or goes backwards the wait fails closed with `common.Stop` (never a silent fall back to the plain 10 s).

Every other timeout is delegated untouched to the retained method: the 120 s finite/command wait, the 60 s status wait,
the 180/240/300/900 s boot and drawing waits, and every fixed `pause(N)` observation duration.  Matching is by the exact
`(label, 10)` pair of the two recovery waits; the survey of the retained source lives in `test_s108_credit.py`.

"The phase's own maximum": the retained schedule has no per-phase maximum table.  Every workload phase that contains a
recovery wait (endless-collected-cat, endless-terminal-stream, rg-large, interactive-top-idle) has the retained 120 s
`command_s`/stream-flow deadline as its largest single wait, so a credited recovery wait never lasts longer than 120 s of
wall time in total, even when the observer is paused for all of it.  This interpretation is disclosed in the run records.

Deviation disclosure: a run using this wrapper measures the product in "10 s of unpaused guest time" for these waits; no
product timing claim may be derived from it.
"""
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s108_env as ENV  # noqa: E402
if str(ENV.RUNTIME) not in sys.path:
    sys.path.insert(0, str(ENV.RUNTIME))
from common import Stop  # noqa: E402  (retained exception type)

NS = 1_000_000_000
ORIGINAL_RECOVERY_S = 10
PHASE_MAX_S = ENV.DEADLINES['command_s']             # 120
GLOBAL_WINDOW_S = ENV.DEADLINES['total_window_s']    # 900

HEADLESS_KEY = ('new interrupt prompt', ORIGINAL_RECOVERY_S)          # Surface.interrupt -> Surface.wait
GUI_KEY = ('runtime new GUI prompt', ORIGINAL_RECOVERY_S)             # Surface.gui_fresh -> Desk.wait_for

# Replaceable in tests (resolved at call time).
now_ns = time.monotonic_ns
sleep = time.sleep


class PauseSource:
    """Reads the retained `Observer.pause_ns`; fails closed when it is not usable."""

    def __init__(self, observer):
        self.observer = observer
        self.last = None

    def read(self):
        if self.observer is None:
            raise Stop('pause-credit: no observer, observer pause accounting unavailable')
        value = getattr(self.observer, 'pause_ns', None)
        if type(value) is not int or value < 0:
            raise Stop(f'pause-credit: observer.pause_ns unavailable or invalid ({value!r})')
        if self.last is not None and value < self.last:
            raise Stop(f'pause-credit: observer.pause_ns went backwards ({self.last} -> {value})')
        self.last = value
        return value


class CreditedWait:
    """One credited recovery wait: evaluates the effective deadline and keeps the accounting record."""

    def __init__(self, *, label, phase, pause, machine_start_ns, original_s=ORIGINAL_RECOVERY_S, phase_max_s=PHASE_MAX_S,
                 global_s=GLOBAL_WINDOW_S):
        if machine_start_ns is None:
            raise Stop('pause-credit: machine start time unavailable, global window cap cannot be applied')
        self.pause = pause
        self.began_ns = now_ns()
        self.pause_start_ns = pause.read()                  # fail closed before the wait begins
        self.base_deadline_ns = self.began_ns + original_s * NS
        self.phase_deadline_ns = self.began_ns + phase_max_s * NS
        self.global_deadline_ns = machine_start_ns + global_s * NS
        self.effective_deadline_ns = min(self.base_deadline_ns, self.global_deadline_ns)
        self.capped_by = None
        self.pause_now_ns = self.evaluated_pause_ns = self.pause_start_ns
        self.record = {'label': label, 'phase': phase, 'original_timeout_s': original_s, 'phase_max_s': phase_max_s, 'global_window_s': global_s,
                       'start_ns': self.began_ns, 'pause_at_start_ns': self.pause_start_ns, 'outcome': None}

    def credited_ns(self):
        return max(0, self.effective_deadline_ns - self.base_deadline_ns)

    def active(self):
        """True while the wait may continue; the effective deadline is re-evaluated from the live pause counter every call."""
        self.pause_now_ns = self.evaluated_pause_ns = self.pause.read()
        measured = self.pause_now_ns - self.pause_start_ns
        wanted = self.base_deadline_ns + measured
        self.effective_deadline_ns = min(wanted, self.phase_deadline_ns, self.global_deadline_ns)
        if wanted <= self.effective_deadline_ns:
            self.capped_by = None
        else:
            self.capped_by = 'phase_max' if self.phase_deadline_ns <= self.global_deadline_ns else 'global_window'
        return now_ns() < self.effective_deadline_ns

    def finish(self, outcome):
        if self.record['outcome'] is not None:
            return self.record
        end = now_ns()
        try:
            self.pause_now_ns = self.pause.read()
        except Stop:
            pass
        measured = self.pause_now_ns - self.pause_start_ns
        self.record.update(outcome=outcome, end_ns=end, waited_s=(end - self.began_ns) / NS, pause_at_end_ns=self.pause_now_ns,
                           pause_at_last_evaluation_ns=self.evaluated_pause_ns,
                           pause_measured_in_wait_ns=measured, pause_credited_ns=self.credited_ns(),
                           original_deadline_ns=self.base_deadline_ns, effective_deadline_ns=self.effective_deadline_ns,
                           effective_timeout_s=(self.effective_deadline_ns - self.began_ns) / NS, capped_by=self.capped_by,
                           method='Observer.pause_ns difference (retained: sum of completed _stopped intervals)')
        return self.record


def surface_wait(surface, credit, predicate, label, interval):
    """`common.Surface.wait` loop with the credited deadline (check, predicate, sleep; `timeout: <label>` on expiry)."""
    try:
        while credit.active():
            surface.check()
            value = predicate()
            if value:
                credit.finish('satisfied')
                return value
            sleep(interval)
        credit.finish('timeout')
        raise Stop(f'timeout: {label}')
    except BaseException as error:
        credit.finish(f'stopped: {type(error).__name__}: {error}')
        raise


def desk_wait_for(desk, credit, pred, what, timeout, interval):
    """`common.CheckedDesk.wait_for` loop (health check, grab, predicate, sleep) with the credited deadline plus the retained
    `TemporalDesk.wait_for` failure retention (context identical, extended with the credit record)."""
    start = now_ns()
    try:
        try:
            while credit.active():
                desk.health_check()
                image = desk.grab()
                got = pred(image)
                if got:
                    credit.finish('satisfied')
                    return image, got
                sleep(interval)
            credit.finish('timeout')
            raise Stop(f'timeout waiting for {what}')
        except BaseException as error:
            credit.finish(f'stopped: {type(error).__name__}: {error}')
            raise
    except BaseException as error:
        retain = getattr(desk, 'retain_failure', None)
        if retain is not None:
            try:
                retain(error, {'wait_for': what, 'timeout_seconds': timeout, 'start_ns': start, 'end_ns': now_ns(),
                               'pause_credit': dict(credit.record)})
            except Exception as retention_error:  # noqa: BLE001 - same handling as the retained wrapper
                desk.failure_records.append({'original_error': str(error), 'retention_error': str(retention_error)})
        raise


class CreditedRecoveryMixin:
    """`wait`/`gui_fresh` replacements for the two original 10 s recovery waits; everything else is the retained code.

    Place before the retained surface in the MRO.  `pause_credits` (list of per-wait records) is created lazily so no
    retained constructor is touched.
    """

    def _credit_ledger(self):
        return self.__dict__.setdefault('pause_credits', [])

    def _credit_start(self, label):
        phase = getattr(self.observer, 'phase', None)
        credit = CreditedWait(label=label, phase=phase, pause=PauseSource(self.observer), machine_start_ns=getattr(self.m, 'start_ns', None))
        self._credit_ledger().append(credit.record)
        return credit

    def wait(self, predicate, timeout, label, interval=.02):
        if (label, timeout) != HEADLESS_KEY:
            return super().wait(predicate, timeout, label, interval)
        return surface_wait(self, self._credit_start(label), predicate, label, interval)

    def gui_fresh(self):
        desk = self.d
        retained = desk.wait_for

        def wait_for(pred, what, timeout, interval=1.0):
            if (what, timeout) != GUI_KEY:
                return retained(pred, what, timeout, interval)
            return desk_wait_for(desk, self._credit_start(what), pred, what, timeout, interval)

        desk.wait_for = wait_for
        try:
            return super().gui_fresh()
        finally:
            desk.__dict__.pop('wait_for', None)
