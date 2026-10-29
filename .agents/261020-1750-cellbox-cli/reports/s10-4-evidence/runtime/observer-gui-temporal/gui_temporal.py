"""S10-4.15 temporal exact GUI evidence; no output route or guest command change."""
import json
from pathlib import Path
import sys
import time

HERE = Path(__file__).resolve().parent
PREVIOUS = HERE.parent / 'observer-rsp-atomic'
sys.path.insert(0, str(PREVIOUS))
import run_atomic as F
from temporal import TemporalLedger, OracleIncomplete
C, X = F.C, F.X
import common as M
FINITE = 'top --records -d 1 --frames 2 | to jsonl | cat'
STATUS = 'echo st=$?'


class TemporalDesk(M.CheckedDesk):
    def __init__(self, args, machine, verdict, check, gate):
        super().__init__(args, machine, verdict, check)
        self.gate = gate
        self.last_frame = None
        self.last_frame_ns = None
        self.last_read = None
        self.active_epoch = None
        self.temporal_epochs = []
        self.failure_records = []
        self._retained_errors = set()
        self.finite_status_pending = False

    def grab(self):
        image = super().grab()
        self.last_frame = image
        self.last_frame_ns = time.monotonic_ns()
        return image

    def retain_failure(self, error, context):
        identity = id(error)
        if identity in self._retained_errors:
            return
        self._retained_errors.add(identity)
        number = len(self.failure_records) + 1
        path = Path(self.a.evidence) / f'gui-failure-{number:02d}'
        image_path = None
        if self.last_frame is not None:
            image_path = str(path.with_suffix('.png'))
            self.last_frame.save(image_path)
            # Host-only decode of the already-retained framebuffer, no QMP/guest action.
            try:
                self.last_read = self.read_screen(self.last_frame)
            except Exception as decode_error:
                self.last_read = {'decode_error': str(decode_error)}
        record = {'error': f'{type(error).__name__}: {error}', 'context': context,
                  'retained_ns': time.monotonic_ns(), 'last_frame_ns': self.last_frame_ns,
                  'last_frame': image_path, 'last_exact_read': self.last_read,
                  'epoch': self.active_epoch, 'gate_phase': self.gate.phase,
                  'natural_resume_epoch': self.gate.resume_epoch,
                  'last_input_batch': self.gate.batches[-1] if self.gate.batches else None,
                  'scope': 'Last already-observed framebuffer/read; no extra grab/resume/input at timeout'}
        self.failure_records.append(record)
        X.dump(path.with_suffix('.json'), record)

    def wait_for(self, pred, what, timeout, interval=1.0):
        start = time.monotonic_ns()
        try:
            return super().wait_for(pred, what, timeout, interval)
        except BaseException as error:
            try:
                self.retain_failure(error, {'wait_for': what, 'timeout_seconds': timeout,
                                            'start_ns': start, 'end_ns': time.monotonic_ns()})
            except Exception as retention_error:
                self.failure_records.append({'original_error': str(error), 'retention_error': str(retention_error)})
            raise

    def command_read(self, name, text, timeout=180, done=None):
        if (text not in (FINITE, STATUS) or done is not None or
                (text == STATUS and not self.finite_status_pending)):
            return super().command_read(name, text, timeout, done)
        epoch = {'id': len(self.temporal_epochs) + 1, 'name': name, 'command': text,
                 'timeout_seconds': timeout, 'start_ns': time.monotonic_ns(), 'state': 'TYPING',
                 'samples': [], 'ledger': None}
        self.temporal_epochs.append(epoch)
        self.active_epoch = epoch
        ledger = None
        try:
            before = self.state['img']
            typed_rows = self.model[:-1] + [self.model[-1] + text]
            self.type_text(text)
            typed = self.expect_screen(typed_rows, f'{name}: typed line', 120)
            self.chk(f'{name}-typed-line-exact', True, 'diff=0')
            self.shot(f'{name}-typed-exact', typed)
            got = self.read_screen(typed)
            X.need(got is not None, 'exact typed-line re-read missing')
            ledger = TemporalLedger(X.D.COLS, X.D.ROWS, got[0], got[1], text,
                                    kind='finite' if text == FINITE else 'status', expected_status=0)
            epoch.update(state='TYPED_EXACT', typed_rows=got[0], typed_cursor=got[1],
                         typed_exact_ns=time.monotonic_ns())
            prior_batches = len(self.gate.batches)
            self.tap('ret')
            X.need(len(self.gate.batches) == prior_batches + 1, 'Enter must be exactly one original GUI batch')
            commit = self.gate.batches[-1]
            ledger.arm(commit)
            epoch.update(state='ARMED', Enter_batch_id=commit['id'], Enter_commit=commit,
                         armed_ns=time.monotonic_ns(), ledger=ledger.snapshot())
            sample_dir = Path(self.a.evidence) / f'gui-epoch-{epoch["id"]:02d}'
            sample_dir.mkdir(mode=0o700, exist_ok=False)
            def observe(image):
                stamp = time.monotonic_ns()
                sample_index = len(epoch['samples'])
                frame_path = sample_dir / f'{sample_index:04d}.png'
                image.save(frame_path)
                read = self.read_screen(image)
                self.last_read = read
                sample = {'index': sample_index, 'monotonic_ns': stamp, 'frame': str(frame_path),
                          'exact_read': read, 'accepted': False}
                epoch['samples'].append(sample)
                try:
                    accepted = ledger.observe(read[0] if read else None, read[1] if read else None, stamp)
                    sample['accepted'] = accepted
                    return read if accepted else None
                finally:
                    epoch['ledger'] = ledger.snapshot()
                    sample['ledger'] = epoch['ledger']
                    X.dump(sample_dir / f'{sample_index:04d}.json', sample)
            image, result = self.wait_for(observe, f'{name}: temporal exact output', timeout, interval=1.5)
            rows, cursor = result
            self.shot(f'{name}-temporal-complete', image)
            self.chk(f'{name}-temporal-twoframes' if text == FINITE else f'{name}-temporal-status0', True,
                     'ordered complete output and post-submit exact prompt; final echo not required')
            final = rows if rows[-1] != X.D.PROMPT.rstrip() else rows[:-1] + [X.D.PROMPT]
            self.controls(name, before, image, final, max(1, len(final) - 2))
            self.model[:] = final
            self.state['img'] = image
            epoch.update(state='COMPLETE', end_ns=time.monotonic_ns(), ledger=ledger.snapshot())
            self.finite_status_pending = text == FINITE
            return rows
        except BaseException as error:
            if ledger:
                epoch['ledger'] = ledger.snapshot()
            epoch.update(state='STOP', error=f'{type(error).__name__}: {error}', end_ns=time.monotonic_ns())
            try:
                self.retain_failure(error, {'command': name, 'timeout_seconds': timeout})
            except Exception as retention_error:
                epoch['retention_error'] = str(retention_error)
            raise
        finally:
            X.dump(Path(self.a.evidence) / 'gui-temporal-epochs.json', self.temporal_epochs)


class TemporalSurface(C.CoordinatedSurface):
    def ready(self):
        # Exact original desktop launch/oracle sequence, replacing only Desk class.
        with self.gate.caller_scope(900):
            self.wait(lambda: '[desktop] Ready. Entering event loop.' in self.m.text(), 900, 'desktop ready')
            self.timestamps['desktop_ready_ns'] = time.monotonic_ns()
            self.pause(3)
            self.d = TemporalDesk(self.m.a, self.m, X.D.Verdict([]), self.check, self.gate)
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
