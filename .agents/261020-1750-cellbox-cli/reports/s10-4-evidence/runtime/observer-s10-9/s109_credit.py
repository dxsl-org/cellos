"""S10-9.1 latency-recording recovery waits (D9-1 = B).  New sibling code only; nothing retained is edited.

Rule (applies ONLY to the two original 10 s recovery waits of the retained `common.Surface`, i.e. every Ctrl-C recovery:
headless `Surface.wait(label='new interrupt prompt', 10)`, desktop `Desk.wait_for(what='runtime new GUI prompt', 10)`):

    effective_deadline = min( start + 120 s + P,                    120 s = retained phase cap (`command_s`), P = observer
                                                                    pause accumulated inside THIS wait (S10-8 accounting)
                              machine.start_ns + 900 s )            the original global measurement window, unchanged

i.e. the prompt may take up to 120 s of OBSERVER-PAUSE-CREDITED wall time (wall time minus the time the observer held the guest
stopped).  `P` is read exactly as in S10-8: `Observer.pause_ns` is the running sum of COMPLETED `_stopped`-handling intervals
(retained `Observer.run`: `pause_ns += monotonic_ns() - stop_ns`), so P is the difference of that counter between the end and the
start of the wait; an interval still being handled is not credited until it completes (nothing is estimated).  A missing,
non-int, negative or backwards counter fails closed with `common.Stop`.

The wait RECORDS instead of judging: time-to-prompt (wall), credited pause, time-to-prompt minus credited pause (an UPPER bound of
the unpaused guest time, because an in-progress pause interval is not subtracted), effective deadline and outcome.  The product
10 s recovery criterion is reported as `not_evaluated` in every record; no `met`/`failed`/boolean-vs-10 s field exists.  A prompt
that does not return within the credited 120 s (or the 900 s window) stops the run exactly as before (`Stop('timeout: <label>')`,
or `timeout waiting for runtime new GUI prompt` on the desktop).

Unchanged and delegated to the retained/S10-8 code: the 120 s finite/command wait, the 60 s status wait (also after a recovery),
the 180/240/300/900 s boot and drawing waits, every `pause(N)` observation duration.  Matching is the S10-8 exact
`(label, 10)` pair; the survey of the retained source lives in `test_s109_credit.py`.

Ceiling: the numbers are QEMU/TCG, test-hooks kernel, observer-throttled guest numbers; NO product-latency claim may be derived.
"""
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
import s109_env as ENV  # noqa: E402
import s108_credit as C8  # noqa: E402

NS = C8.NS
ORIGINAL_RECOVERY_S = C8.ORIGINAL_RECOVERY_S          # 10: the original product recovery wait (identification key only)
RECOVERY_BUDGET_S = ENV.DEADLINES['command_s']        # 120: retained phase cap, measured as observer-pause-credited wall time
GLOBAL_WINDOW_S = ENV.DEADLINES['total_window_s']     # 900
HEADLESS_KEY = C8.HEADLESS_KEY
GUI_KEY = C8.GUI_KEY
CRITERION = 'not_evaluated'
CRITERION_NOTE = ('the product 10 s recovery criterion is NOT EVALUATED by these runs (never met, never failed); recorded numbers are '
                  'observer-throttled QEMU/TCG test-hooks guest numbers and support no product-latency claim')

PauseSource = C8.PauseSource
Stop = C8.Stop


class LatencyWait:
    """One latency-recording recovery wait; same call surface as `s108_credit.CreditedWait` (`active`, `finish`, `record`)."""

    def __init__(self, *, label, phase, pause, machine_start_ns, budget_s=RECOVERY_BUDGET_S, global_s=GLOBAL_WINDOW_S):
        if machine_start_ns is None:
            raise Stop('latency-wait: machine start time unavailable, global window cap cannot be applied')
        self.pause = pause
        self.began_ns = C8.now_ns()
        self.pause_start_ns = pause.read()                  # fail closed before the wait begins
        self.budget_ns = budget_s * NS
        self.base_deadline_ns = self.began_ns + self.budget_ns          # 120 s of wall time, before any pause credit
        self.original_10s_deadline_ns = self.began_ns + ORIGINAL_RECOVERY_S * NS   # identification only, never a verdict
        self.global_deadline_ns = machine_start_ns + global_s * NS
        self.effective_deadline_ns = min(self.base_deadline_ns, self.global_deadline_ns)
        self.capped_by = None
        self.pause_now_ns = self.evaluated_pause_ns = self.pause_start_ns
        self.record = {'label': label, 'phase': phase, 'mode': 'latency-recording (D9-1=B)', 'original_timeout_s': ORIGINAL_RECOVERY_S,
                       'budget_s': budget_s, 'global_window_s': global_s, 'start_ns': self.began_ns,
                       'pause_at_start_ns': self.pause_start_ns, 'outcome': None, 'recovery_10s_criterion': CRITERION}

    def credited_ns(self):
        return max(0, self.effective_deadline_ns - self.base_deadline_ns)

    def active(self):
        """True while the wait may continue; the effective deadline is re-evaluated from the live pause counter every call."""
        self.pause_now_ns = self.evaluated_pause_ns = self.pause.read()
        measured = self.pause_now_ns - self.pause_start_ns
        wanted = self.base_deadline_ns + measured
        self.effective_deadline_ns = min(wanted, self.global_deadline_ns)
        self.capped_by = None if wanted <= self.global_deadline_ns else 'global_window'
        return C8.now_ns() < self.effective_deadline_ns

    def finish(self, outcome):
        if self.record['outcome'] is not None:
            return self.record
        end = C8.now_ns()
        try:
            self.pause_now_ns = self.pause.read()
        except Stop:
            pass
        measured = self.pause_now_ns - self.pause_start_ns
        waited_ns = end - self.began_ns
        returned = outcome == 'satisfied'
        self.record.update(
            outcome=outcome, end_ns=end, waited_s=waited_ns / NS,
            time_to_prompt_s=(waited_ns / NS) if returned else None,
            pause_at_end_ns=self.pause_now_ns, pause_at_last_evaluation_ns=self.evaluated_pause_ns,
            pause_measured_in_wait_ns=measured, pause_credited_ns=self.credited_ns(),
            time_to_prompt_minus_measured_pause_s=((waited_ns - measured) / NS) if returned else None,
            unpaused_time_bound='upper bound: an observer pause interval still in progress when the prompt returned is not subtracted',
            original_10s_deadline_ns=self.original_10s_deadline_ns, base_deadline_ns=self.base_deadline_ns,
            effective_deadline_ns=self.effective_deadline_ns, effective_timeout_s=(self.effective_deadline_ns - self.began_ns) / NS,
            capped_by=self.capped_by, recovery_10s_criterion=CRITERION, criterion_note=CRITERION_NOTE,
            method='Observer.pause_ns difference (retained: sum of completed _stopped intervals), same accounting as S10-8')
        return self.record


class LatencyRecoveryMixin(C8.CreditedRecoveryMixin):
    """The S10-8 `wait`/`gui_fresh` replacements with only the wait object swapped for `LatencyWait` (all matching, delegation,
    desk-restoration and failure-retention code is the unchanged S10-8 code inherited from `CreditedRecoveryMixin`)."""

    def _credit_start(self, label):
        phase = getattr(self.observer, 'phase', None)
        wait = LatencyWait(label=label, phase=phase, pause=PauseSource(self.observer), machine_start_ns=getattr(self.m, 'start_ns', None))
        self._credit_ledger().append(wait.record)
        return wait
