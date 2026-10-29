"""S10-4.17 desktop surface: the finite and status epochs are gated by the feed-hook ledger.

Everything outside those two original commands is the retained S10-4.15 desktop code.
Intermediate GUI samples are retained evidence only and never gate (the lost-physical-row
gate that stopped S10-4.15 is not used for these two epochs).
"""
import json
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_ledger as HL
import hook_observer as HO

G, X, M = HO.G, HO.X, HO.M
FINITE, STATUS = HL.FINITE, HL.STATUS
STABLE_NS = 1_500_000_000


class HookOracleFailure(M.Stop):
    def __init__(self, cls, code, reason):
        super().__init__(f'feed-hook oracle {cls}/{code}: {reason}')
        self.cls, self.code, self.reason = cls, code, reason


class HookDesk(G.TemporalDesk):
    def __init__(self, args, machine, verdict, check, gate, hook_observer):
        super().__init__(args, machine, verdict, check, gate)
        self.hook_observer = hook_observer
        self.metric = {}
        self.metric_complete = False
        self.disarm = None

    def _evidence(self, name, value):
        X.dump(Path(self.a.evidence) / name, value)

    def command_read(self, name, text, timeout=180, done=None):
        if (text not in (FINITE, STATUS) or done is not None or
                (text == STATUS and not self.finite_status_pending)):
            return super().command_read(name, text, timeout, done)
        kind = 'finite' if text == FINITE else 'status'
        epoch = {'id': len(self.temporal_epochs) + 1, 'name': name, 'command': text, 'kind': kind, 'oracle': 'feed-hook',
                 'timeout_seconds': timeout, 'start_ns': time.monotonic_ns(), 'state': 'TYPING', 'samples': [],
                 'ledger': None}
        self.temporal_epochs.append(epoch)
        self.active_epoch = epoch
        ledger = self.hook_observer.ledger
        try:
            before = self.state['img']
            typed_rows = self.model[:-1] + [self.model[-1] + text]
            self.type_text(text)
            typed = self.expect_screen(typed_rows, f'{name}: typed line', 120)
            grab_ns = self.last_frame_ns
            self.chk(f'{name}-typed-line-exact', True, 'diff=0')
            self.shot(f'{name}-typed-exact', typed)
            got = self.read_screen(typed)
            X.need(got is not None, 'exact typed-line re-read missing')
            line = HL.PROMPT + text
            X.need(got[0] and got[0][-1] == line and tuple(got[1]) == (len(line), len(got[0]) - 1),
                   'pre-Enter typed line or cursor is not exact')
            hits = ledger.snapshot()
            pre = [h for h in hits if h['ns'] <= grab_ns]
            try:
                HL.check_typed(pre, text)
            except HL.OracleFail as error:
                raise HookOracleFailure(error.cls, error.code, error.reason) from error
            epoch.update(state='TYPED_EXACT', typed_rows=got[0], typed_cursor=got[1], typed_grab_ns=grab_ns,
                         typed_exact_ns=time.monotonic_ns(), pre_hits=len(pre))
            prior = len(self.gate.batches)
            self.tap('ret')
            X.need(len(self.gate.batches) == prior + 1, 'Enter must be exactly one original GUI batch')
            commit = self.gate.batches[-1]
            X.need(commit['outcome'] == 'COMMITTED' and commit['commit_count'] == 1 and
                   commit['attempts'][-1]['outcome'] == 'COMMITTED', 'Enter batch not committed exactly once')
            commit_ns = int(commit['attempts'][-1]['finish'] * 1e9)
            epoch.update(state='ARMED', Enter_batch_id=commit['id'], Enter_commit=commit, commit_finish_ns=commit_ns,
                         armed_ns=time.monotonic_ns())
            sample_dir = Path(self.a.evidence) / f'gui-epoch-{epoch["id"]:02d}'
            sample_dir.mkdir(mode=0o700, exist_ok=False)
            stable = {}

            def observe(image):
                stamp = time.monotonic_ns()
                index = len(epoch['samples'])
                frame_path = sample_dir / f'{index:04d}.png'
                image.save(frame_path)
                read = self.read_screen(image)
                self.last_read = read
                sample = {'index': index, 'monotonic_ns': stamp, 'frame': str(frame_path), 'exact_read': read,
                          'gating': False, 'accepted': False}
                epoch['samples'].append(sample)
                try:
                    if read and any(row.startswith(('[terminal', '[shell')) for row in read[0]):
                        raise HookOracleFailure('C', 'notice-row', 'Terminal notice written outside feed is visible')
                    verdict = HL.evaluate(kind, text, ledger.snapshot(), grab_ns, commit_ns, stamp)
                    summary = verdict.as_dict()
                    summary.pop('output_text', None)
                    summary.pop('expected_rows', None)
                    sample['oracle'] = summary
                    epoch['oracle'] = summary
                    if verdict.status == 'FAIL':
                        raise HookOracleFailure(verdict.cls, verdict.code, verdict.reason)
                    if verdict.status != 'READY':
                        stable.clear()
                        return None
                    if read is None or not HL.screen_matches(read[0], read[1], verdict.extra['expected_rows']):
                        sample['screen_match'] = False
                        stable.clear()
                        return None
                    sample['screen_match'] = True
                    rows, cursor = read
                    if stable and stable['rows'] == rows and tuple(stable['cursor']) == tuple(cursor) \
                            and stamp - stable['ns'] >= STABLE_NS:
                        sample['accepted'] = True
                        epoch['verdict_ready'] = verdict
                        return read
                    if not stable or stable['rows'] != rows or tuple(stable['cursor']) != tuple(cursor):
                        stable.clear()
                        stable.update(rows=rows, cursor=cursor, ns=stamp)
                    return None
                finally:
                    X.dump(sample_dir / f'{index:04d}.json', sample)

            image, result = self.wait_for(observe, f'{name}: feed-hook oracle', timeout, interval=1.5)
            rows, cursor = result
            ready = epoch.pop('verdict_ready')
            self.shot(f'{name}-feed-hook-complete', image)
            self.chk(f'{name}-feed-hook-twoframes' if kind == 'finite' else f'{name}-feed-hook-status0', True,
                     'ledger F1-F4 and exact final-screen F5; no intermediate-sample gate')
            final = rows if rows[-1] != X.D.PROMPT.rstrip() else rows[:-1] + [X.D.PROMPT]
            self.controls(name, before, image, final, max(1, len(final) - 2))
            self.model[:] = final
            self.state['img'] = image
            record = {'kind': kind, 'command': text, 'typed_grab_ns': grab_ns, 'commit_finish_ns': commit_ns,
                      'oracle': ready.as_dict(), 'hits_total': ledger.count(), 'frames': ready.extra['frames']}
            self.metric[kind] = record
            self._evidence(f'feed-hook-epoch-{kind}.json', record)
            epoch.update(state='COMPLETE', end_ns=time.monotonic_ns(), oracle_final=record['oracle'] and {
                k: v for k, v in record['oracle'].items() if k != 'output_text'})
            self.finite_status_pending = kind == 'finite'
            if kind == 'status':
                self.metric_complete = True
                self.disarm = self.hook_observer.disarm_hook()
            return rows
        except BaseException as error:
            epoch.update(state='STOP', error=f'{type(error).__name__}: {error}', end_ns=time.monotonic_ns())
            try:
                self.retain_failure(error, {'command': name, 'timeout_seconds': timeout, 'oracle': 'feed-hook'})
            except Exception as retention_error:
                epoch['retention_error'] = str(retention_error)
            raise
        finally:
            X.dump(Path(self.a.evidence) / 'gui-temporal-epochs.json', self.temporal_epochs)


class HookSurface(G.TemporalSurface):
    def ready(self):
        # Retained S10-4.15 desktop launch/oracle sequence, replacing only the Desk class.
        with self.gate.caller_scope(900):
            self.wait(lambda: '[desktop] Ready. Entering event loop.' in self.m.text(), 900, 'desktop ready')
            self.timestamps['desktop_ready_ns'] = time.monotonic_ns()
            self.pause(3)
            self.d = HookDesk(self.m.a, self.m, X.D.Verdict([]), self.check, self.gate, self.observer)
            d = self.d
            pre = d.shot('desktop-before-terminal')
            X.need(pre.size == (1280, 800), 'desktop resolution')
            X.need(d.locate(pre) is None, 'Terminal already present')
            self.phase('terminal-launch')
            self.timestamps['terminal_click_ns'] = time.monotonic_ns()
            d.click_px(X.D.TASKBAR['terminal'], X.D.TASKBAR_Y)
            d.park()
            self.wait(lambda: '[terminal] Terminal surface ready. Entering event loop.' in self.m.text(), 300, 'Terminal surface')
            rows = ['', X.D.BANNER, X.D.PROMPT]
            image = d.expect_screen(rows, 'runtime initial exact prompt', 240)
            self.timestamps['exact_prompt_ns'] = time.monotonic_ns()
            d.shot('prompt', image)
            org = d.org
            region = pre.crop((org[0], org[1], org[0] + X.D.WIN_W, org[1] + X.D.WIN_H))
            X.need(d.diff(region, d.pad(rows), 0, X.D.ROWS - 1, d.mcursor(rows))[0] > 1000,
                   'desktop prompt negative control')
            _, window = d.window(image)
            X.need(d.diff(window, d.pad(rows[:-1] + ['Cellos # ']), 0, X.D.ROWS - 1,
                          (len(X.D.PROMPT), 2))[0] >= X.D.MIN_NEGATIVE_DIFF, 'wrong-prompt negative control')
            X.need(d.cursor_of(window) == (len(X.D.PROMPT), 2), 'initial GUI cursor')
            d.click_px(org[0] + 360, org[1] + 300)
            d.park()
            self.pause(2)
            d.model[:] = rows
            d.state['img'] = image
            self.phase('initial-prompt')
            self.snapshot('initial-prompt')
