"""Source-bound, observer-coordinated QMP input admission (host side only)."""

from contextlib import contextmanager
import itertools
import json
import math
from pathlib import Path
import threading
import time


class AdmissionError(RuntimeError):
    """Admission stopped; this batch must not be submitted by another route."""


class InputGate:
    PAUSED_OR_STEPPING = 'PAUSED_OR_STEPPING'
    ORDINARY_RESUME_PENDING_OR_RUNNING = 'ORDINARY_RESUME_PENDING_OR_RUNNING'
    TERMINAL = 'TERMINAL'
    _WAIT_SECONDS = 0.05

    def __init__(self, machine, journal_path, *, source_verified,
                 clock=time.monotonic, input_budget=5.0, max_attempts=64):
        budget = float(input_budget)
        if not math.isfinite(budget) or budget <= 0:
            raise ValueError('input_budget must be positive and finite')
        if type(max_attempts) is not int or max_attempts <= 0:
            raise ValueError('max_attempts must be a positive integer')
        self.machine = machine
        self.journal_path = Path(journal_path)
        self.source_verified = source_verified
        self.clock = clock
        self.input_budget = min(budget, 5.0)
        self.max_attempts = min(max_attempts, 64)
        self.lock = threading.RLock()
        self.condition = threading.Condition(self.lock)
        self.phase = self.PAUSED_OR_STEPPING
        self.resume_epoch = 0
        self.health = lambda: None
        self.batches = []
        self.pending = None
        self.terminal_reason = None
        self._caller = threading.local()
        self._ids = itertools.count(1)
        self._original_cmd = None

    def install(self):
        """Wrap the actual GUI QMP owner; trace queries use this same lock."""
        qmp = self.machine.qmp
        with self.lock:
            if self._original_cmd is not None or hasattr(qmp, '_observer_transaction_lock'):
                raise AdmissionError('QMP admission/serialization already installed')
            original = qmp.cmd

            def cmd(name, **args):
                if name == 'input-send-event':
                    return self._input(args)
                with self.lock:
                    return original(name, **args)

            self._original_cmd = original
            qmp._observer_transaction_lock = self.lock
            qmp.cmd = cmd
        return self.lock

    def _stop_locked(self, reason):
        if self.phase != self.TERMINAL:
            self.terminal_reason = str(reason)
            self.phase = self.TERMINAL
        self.condition.notify_all()

    def terminate(self, reason):
        with self.lock:
            self._stop_locked(reason)

    def _check(self):
        if self.phase == self.TERMINAL:
            raise AdmissionError(self.terminal_reason or 'admission terminated')
        if self.phase not in (self.PAUSED_OR_STEPPING,
                              self.ORDINARY_RESUME_PENDING_OR_RUNNING):
            raise AdmissionError('unowned observer phase')
        if self.source_verified is not True:
            raise AdmissionError('input source identity is not verified')
        if self.clock() >= self.machine.start_ns / 1_000_000_000 + 900:
            raise AdmissionError('original 900s VM deadline expired')
        if self.health() is False:
            raise AdmissionError('observer health/ownership check failed')
        # A health callback may itself terminate the gate.
        if self.phase == self.TERMINAL:
            raise AdmissionError(self.terminal_reason or 'admission terminated')

    @contextmanager
    def paused(self):
        """Hold the common lock through all stopped/native/proof restoration."""
        with self.lock:
            try:
                self._check()
                self.phase = self.PAUSED_OR_STEPPING
                self.condition.notify_all()
                yield
            except BaseException as error:
                self._stop_locked(f'{type(error).__name__}: {error}')
                raise
            finally:
                self.condition.notify_all()

    def ordinary_resume(self, callback):
        """Invoke only the observer's original, already-required resume once."""
        with self.lock:
            try:
                self._check()
                result = callback()
                self._check()
                self.resume_epoch += 1
                self.phase = self.ORDINARY_RESUME_PENDING_OR_RUNNING
                self.condition.notify_all()
                return result
            except BaseException as error:
                self._stop_locked(f'{type(error).__name__}: {error}')
                raise

    @contextmanager
    def caller_scope(self, seconds):
        """Nest original absolute caller deadlines without extending an outer one."""
        seconds = float(seconds)
        if not math.isfinite(seconds) or seconds < 0:
            raise ValueError('caller deadline must be finite and nonnegative')
        previous = getattr(self._caller, 'deadline', None)
        deadline = self.clock() + seconds
        self._caller.deadline = min(previous, deadline) if previous is not None else deadline
        try:
            yield
        finally:
            if previous is None:
                del self._caller.deadline
            else:
                self._caller.deadline = previous

    def _remaining(self, batch):
        remaining = batch['deadline'] - self.clock()
        if remaining <= 0:
            raise AdmissionError('input admission total deadline expired')
        return remaining

    def _journal(self, kind, batch, **fields):
        record = {'kind': kind, 'batch_id': batch['id'], 'epoch': self.resume_epoch,
                  'time': self.clock(), **fields, 'phase': self.phase}
        # Open per record: no persistent file descriptor and no writer left buffered
        # across a condition wait or terminal path. Journal failures are terminal.
        with self.journal_path.open('a', encoding='utf-8') as stream:
            stream.write(json.dumps(record, allow_nan=False) + '\n')
            stream.flush()

    @staticmethod
    def _pairs(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError('duplicate QMP JSON member')
            result[key] = value
        return result

    @staticmethod
    def _constant(value):
        raise ValueError(f'non-JSON constant {value}')

    def _transaction(self, payload, command, batch, attempt):
        """Own an entire request/reply, including asynchronous event lines."""
        qmp = self.machine.qmp
        original_timeout = qmp.sock.gettimeout()
        try:
            self._check()
            self._journal('request', batch, command=command, attempt=attempt,
                          raw_hex=payload.hex())
            qmp.sock.settimeout(self._remaining(batch))
            count = qmp.file.write(payload)
            if count is not None and count != len(payload):
                raise AdmissionError('short QMP request write; acceptance is ambiguous')
            qmp.sock.settimeout(self._remaining(batch))
            qmp.file.flush()
            while True:
                self._check()
                qmp.sock.settimeout(self._remaining(batch))
                raw = qmp.file.readline()
                reply = None
                parse_error = None
                try:
                    if raw:
                        reply = json.loads(raw, object_pairs_hook=self._pairs,
                                           parse_constant=self._constant)
                except (ValueError, UnicodeError) as error:
                    parse_error = str(error)
                self._journal('reply', batch, command=command, attempt=attempt,
                              raw_hex=raw.hex(), reply=reply, parse_error=parse_error)
                if not raw:
                    raise AdmissionError('QMP EOF; acceptance is ambiguous')
                if parse_error is not None or not isinstance(reply, dict):
                    raise AdmissionError('malformed QMP reply; acceptance is ambiguous')
                if 'id' in reply:
                    raise AdmissionError('unexpected QMP reply id; transaction ownership lost')
                has_return, has_error = 'return' in reply, 'error' in reply
                if has_return != has_error and 'event' not in reply:
                    return reply
                if not has_return and not has_error and isinstance(reply.get('event'), str):
                    continue
                raise AdmissionError('unmatched QMP reply; transaction ownership lost')
        finally:
            # Do not lend a shrinking GUI timeout to the observer while waiting.
            qmp.sock.settimeout(original_timeout)

    def _wait(self, batch):
        self._check()
        self.condition.wait(timeout=min(self._WAIT_SECONDS, self._remaining(batch)))
        self._remaining(batch)
        self._check()

    def _admit(self, batch, payload):
        while True:
            self._check()
            self._remaining(batch)
            if (self.phase != self.ORDINARY_RESUME_PENDING_OR_RUNNING or
                    self.resume_epoch < 1):
                self._wait(batch)
                continue
            attempt = len(batch['attempts']) + 1
            status_reply = self._transaction(b'{"execute": "query-status"}\n',
                                             'query-status', batch, attempt)
            status = status_reply.get('return')
            if not isinstance(status, dict):
                raise AdmissionError(f'QMP readiness failed: {status_reply!r}')
            if status.get('status') == 'debug' and status.get('running') is False:
                # Give the observer complete lock ownership to classify/process
                # its stop. A subsequent fresh RUN observation can establish
                # readiness in this epoch too (original resume may still be
                # transitioning); it is never acceptance proof.
                self._wait(batch)
                continue
            if status.get('status') != 'running' or status.get('running') is not True:
                raise AdmissionError(f'unowned QMP readiness state: {status!r}')
            self._check()
            self._remaining(batch)
            entry = {'attempt': attempt, 'epoch': self.resume_epoch,
                     'start': self.clock(), 'outcome': 'PENDING'}
            batch['attempts'].append(entry)
            reply = self._transaction(payload, 'input-send-event', batch, attempt)
            entry['finish'] = self.clock()
            entry['reply'] = reply
            if set(reply) == {'return'} and type(reply['return']) is dict and not reply['return']:
                entry['outcome'] = 'COMMITTED'
                batch['commit_count'] = 1
                batch['outcome'] = 'COMMITTED'
                self._remaining(batch)
                self._check()
                return {}
            if (set(reply) == {'error'} and reply['error'] ==
                    {'class': 'GenericError', 'desc': 'VM not running'}):
                entry['outcome'] = 'RETAIN_UNACCEPTED'
                if attempt >= self.max_attempts:
                    raise AdmissionError('input admission attempt cap exhausted')
                self._wait(batch)
                continue
            entry['outcome'] = 'TERMINAL'
            raise AdmissionError(f'input acceptance not proved: {reply!r}')

    def _input(self, args):
        # Freeze before even attempting lock acquisition. Retries never encode the
        # caller's mutable arguments again, nor include an earlier committed batch.
        start = self.clock()
        deadline = min(start + self.input_budget,
                       self.machine.start_ns / 1_000_000_000 + 900,
                       getattr(self._caller, 'deadline', math.inf))
        batch = {'id': next(self._ids), 'payload_hex': '', 'deadline': deadline,
                 'start': start, 'finish': None, 'attempts': [],
                 'phase': self.phase, 'epoch': self.resume_epoch,
                 'caller': threading.current_thread().name,
                 'caller_deadline': getattr(self._caller, 'deadline', None),
                 'outcome': 'PENDING', 'commit_count': 0}
        self.batches.append(batch)
        acquired = False
        registered = False
        try:
            request = {'execute': 'input-send-event'}
            if args:
                request['arguments'] = args
            payload = json.dumps(request, allow_nan=False).encode() + b'\n'
            batch['payload_hex'] = payload.hex()
            self._journal('batch', batch, payload_hex=payload.hex(), deadline=deadline,
                          start=start, attempt=0, phase=batch['phase'],
                          caller=batch['caller'], caller_deadline=batch['caller_deadline'])
            acquired = self.lock.acquire(timeout=self._remaining(batch))
            if not acquired:
                raise AdmissionError('input admission lock deadline expired')
            self._remaining(batch)
            self._check()
            if self.pending is not None:
                raise AdmissionError('more than one pending GUI batch')
            self.pending = batch
            registered = True
            return self._admit(batch, payload)
        except BaseException as error:
            batch['error'] = f'{type(error).__name__}: {error}'
            batch['outcome'] = 'TERMINAL'
            for entry in batch['attempts']:
                if entry['outcome'] == 'PENDING':
                    entry.update(outcome='TERMINAL', finish=self.clock())
            if acquired:
                self._stop_locked(batch['error'])
            else:
                # Acquiring an already expired lock again would extend the very
                # deadline being enforced. Its current owner observes terminal
                # on its next health check; paused()/resume also notify on exit.
                if self.phase != self.TERMINAL:
                    self.terminal_reason = batch['error']
                    self.phase = self.TERMINAL
                if self.lock.acquire(blocking=False):
                    try:
                        self.condition.notify_all()
                    finally:
                        self.lock.release()
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                raise
            raise AdmissionError(batch['error']) from error
        finally:
            batch['finish'] = self.clock()
            if registered:
                self.pending = None
            try:
                self._journal('finish', batch, attempt=len(batch['attempts']),
                              outcome=batch['outcome'], commit_count=batch['commit_count'],
                              finish=batch['finish'], error=batch.get('error'))
            except Exception as error:
                # A committed request remains committed even if its final audit
                # record fails: terminate, but never authorize its replay.
                if acquired:
                    self._stop_locked(f'input journal failure: {error}')
                else:
                    self.terminal_reason = self.terminal_reason or f'input journal failure: {error}'
                    self.phase = self.TERMINAL
                raise AdmissionError(f'input journal failure: {error}') from error
            finally:
                if acquired:
                    self.lock.release()
