"""Deterministic, host-only contract tests; no VM, external package or guest control."""
from collections import Counter
import json
from pathlib import Path
import socket
import tempfile
import threading
from types import SimpleNamespace
import unittest

import admission as G


PAUSED = 'PAUSED_OR_STEPPING'
RUNNING = 'ORDINARY_RESUME_PENDING_OR_RUNNING'
TERMINAL = 'TERMINAL'
RUN_STATUS = {'return': {'running': True, 'status': 'running'}}
DEBUG_STATUS = {'return': {'running': False, 'status': 'debug'}}
REJECT = {'error': {'class': 'GenericError', 'desc': 'VM not running'}}
ACCEPT = {'return': {}}


def key_events(pairs):
    return [{'type': 'key', 'data': {'down': down,
             'key': {'type': 'qcode', 'data': key}}} for key, down in pairs]


class Clock:
    def __init__(self, now=0.0):
        self.now = now
        self.mutex = threading.Lock()
        self.called = None

    def __call__(self):
        with self.mutex:
            value = self.now
            event, self.called = self.called, None
        if event is not None:
            event.set()
        return value

    def advance(self, seconds):
        with self.mutex:
            self.now += seconds


class FakeSocket:
    def __init__(self, clock):
        self.clock = clock
        self.timeout = 11.0
        self.timeouts = []

    def gettimeout(self):
        return self.timeout

    def settimeout(self, seconds):
        self.timeout = seconds
        self.timeouts.append((self.clock(), seconds))


class Reply:
    def __init__(self, *lines, accepted=False, elapsed=0.0):
        self.lines = lines
        self.accepted = accepted
        self.elapsed = elapsed


class FakeFile:
    """One complete byte transaction; overlapping request owners fail immediately."""
    def __init__(self, clock):
        self.clock = clock
        self.requests = []
        self.raw_requests = []
        self.raw_replies = []
        self.accepted = []
        self.active = None
        self.responses = []
        self.handler = None
        self.gate = None
        self.read_hook = None
        self.block_command = None
        self.entered = threading.Event()
        self.release = threading.Event()
        self.write_result = None

    def write(self, payload):
        assert isinstance(payload, bytes), 'QMP request must remain bytes'
        assert self.active is None, 'another owner interleaved a QMP transaction'
        if self.gate is not None:
            assert self.gate.lock._is_owned(), 'transaction escaped the common RLock'
        request = json.loads(payload)
        assert request['execute'] not in (
            'cont', 'stop', 'system_reset', 'human-monitor-command'), 'input added guest controls'
        self.requests.append(request)
        self.raw_requests.append(payload)
        self.active = request
        response = self.handler(request) if self.handler is not None else None
        if response is None:
            if request['execute'] == 'query-status':
                response = Reply(RUN_STATUS)
            elif request['execute'] == 'trace-event-get-state':
                response = Reply({'return': [{'name': request['arguments']['name'],
                                             'state': 'enabled', 'vcpu': False}]})
            else:
                response = Reply(ACCEPT, accepted=request['execute'] == 'input-send-event')
        self.clock.advance(response.elapsed)
        self.responses = list(response.lines)
        if response.accepted:
            self.accepted.append(payload)
        if self.write_result is not None:
            return self.write_result
        return len(payload)

    def flush(self):
        pass

    def readline(self):
        assert self.active is not None, 'reply read without an owning request'
        if self.active['execute'] == self.block_command:
            self.block_command = None
            self.entered.set()
            assert self.release.wait(2), 'test did not release the scripted transaction'
        assert self.responses, 'gate consumed another transaction response'
        value = self.responses.pop(0)
        if isinstance(value, BaseException):
            self.active = None
            self.responses = []
            raise value
        raw = value if isinstance(value, bytes) else json.dumps(value).encode() + b'\n'
        self.raw_replies.append(raw)
        if self.read_hook is not None:
            self.read_hook(self.active, raw)
        if not self.responses:
            self.active = None
        return raw

    @property
    def inputs(self):
        return [r for r in self.requests if r['execute'] == 'input-send-event']

    @property
    def input_bytes(self):
        return [raw for raw, request in zip(self.raw_requests, self.requests)
                if request['execute'] == 'input-send-event']


class FakeQmp:
    """The original cmd/keys/abs_move/button API, including the name keyword."""
    def __init__(self, clock):
        self.file = FakeFile(clock)
        self.sock = FakeSocket(clock)
        self.original_calls = []

    def cmd(self, name, **args):
        self.original_calls.append((name, args))
        message = {'execute': name}
        if args:
            message['arguments'] = args
        self.file.write(json.dumps(message).encode() + b'\n')
        while True:
            line = self.file.readline()
            if not line:
                raise RuntimeError('QMP closed')
            reply = json.loads(line)
            if 'return' in reply:
                return reply['return']
            if 'error' in reply:
                raise RuntimeError(f"QMP {name}: {reply['error']}")

    def keys(self, events):
        self.cmd('input-send-event', events=key_events(events))

    def abs_move(self, x, y):
        self.cmd('input-send-event', events=[
            {'type': 'abs', 'data': {'axis': 'x', 'value': x}},
            {'type': 'abs', 'data': {'axis': 'y', 'value': y}}])

    def button(self, down):
        self.cmd('input-send-event', events=[
            {'type': 'btn', 'data': {'down': down, 'button': 'left'}}])


class AdmissionTests(unittest.TestCase):
    def make_gate(self, *, now=0.0, start=0.0, running=True, **options):
        temp = tempfile.TemporaryDirectory(prefix='s10-4-admission-unit-')
        self.addCleanup(temp.cleanup)
        clock = Clock(now)
        qmp = FakeQmp(clock)
        journal = Path(temp.name) / 'admission.jsonl'
        machine = SimpleNamespace(qmp=qmp, start_ns=int(start * 1_000_000_000))
        options.setdefault('source_verified', True)
        gate = G.InputGate(machine, journal, clock=clock, **options)
        gate.install()
        qmp.file.gate = gate
        resumes = []
        if running:
            gate.ordinary_resume(lambda: resumes.append('original-c'))
        return SimpleNamespace(gate=gate, qmp=qmp, file=qmp.file, clock=clock,
                               journal=journal, resumes=resumes, machine=machine)

    def spawn(self, callback):
        outcome = {}
        def run():
            try:
                outcome['value'] = callback()
            except BaseException as error:
                outcome['error'] = error
        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        return thread, outcome

    def joined(self, thread, outcome):
        thread.join(2)
        self.assertFalse(thread.is_alive(), 'admission/observer transaction deadlock')
        if 'error' in outcome:
            raise outcome['error']
        return outcome.get('value')

    def wake_cleanup(self, state):
        self.addCleanup(lambda: state.gate.terminate('test cleanup'))

    def input(self, state, events=None, **routing):
        if events is None:
            events = key_events([
                ('shift', True), ('a', True), ('a', False), ('shift', False)])
        return state.qmp.cmd('input-send-event', events=events, **routing)

    def trace_query(self, state, name):
        # TraceControl deliberately bypasses cmd(name, **args): the QAPI name
        # argument collides with that original public method's first parameter.
        with state.qmp._observer_transaction_lock:
            state.qmp.file.write(json.dumps({
                'execute': 'trace-event-get-state',
                'arguments': {'name': name}}).encode() + b'\n')
            while True:
                reply = json.loads(state.qmp.file.readline())
                if 'return' in reply:
                    return reply['return']
                if 'error' in reply:
                    raise RuntimeError(reply['error'])

    def natural_resume(self, state):
        with state.gate.paused():
            self.assertEqual(state.gate.phase, PAUSED)
        state.gate.ordinary_resume(lambda: state.resumes.append('original-c'))

    def scripted_wait(self, state, callback):
        """Advance fake time/actions only after completely releasing recursive ownership."""
        condition = state.gate.condition
        calls = []
        def wait(timeout=None):
            calls.append(timeout)
            self.assertIsNotNone(timeout, 'admission waits must be bounded')
            self.assertGreater(timeout, 0)
            saved = condition._release_save()
            try:
                self.assertFalse(state.gate.lock._is_owned())
                callback(len(calls), timeout)
            finally:
                condition._acquire_restore(saved)
            return True
        condition.wait = wait
        return calls

    def observed_wait(self, state):
        event = threading.Event()
        original = state.gate.condition.wait
        def wait(timeout=None):
            event.set()
            return original(timeout)
        state.gate.condition.wait = wait
        return event

    def assert_stopped(self, state, count=0):
        self.assertEqual(state.gate.phase, TERMINAL)
        self.assertEqual(len(state.file.inputs), count)
        self.assertEqual(sum(b['commit_count'] for b in state.gate.batches), 0)
        before = len(state.file.requests)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assertEqual(len(state.file.requests), before, 'terminal gate performed more I/O')

    def test_frozen_api_initial_state_and_common_reentrant_lock(self):
        state = self.make_gate(running=False)
        gate = state.gate
        self.assertIsInstance(G.AdmissionError('failure'), RuntimeError)
        self.assertEqual(gate.phase, PAUSED)
        self.assertEqual(gate.resume_epoch, 0)
        self.assertIsNone(gate.pending)
        self.assertEqual(gate.batches, [])
        self.assertIs(state.qmp._observer_transaction_lock, gate.lock)
        self.assertIs(gate.condition._lock, gate.lock)
        with gate.paused():
            with gate.lock:
                result = self.trace_query(state, 'guest_*')
        self.assertEqual(result[0]['name'], 'guest_*')
        state.qmp.cmd('screendump', filename='/host-only/screen.ppm', format='ppm')
        self.assertEqual(state.qmp.original_calls,
                         [('screendump', {'filename': '/host-only/screen.ppm', 'format': 'ppm'})])
        self.assertEqual(gate.phase, PAUSED)

    def test_direct_input_returns_original_cmd_value_and_commits_once(self):
        state = self.make_gate()
        events = key_events([('ret', True), ('ret', False)])
        self.assertEqual(self.input(state, events), {})
        self.assertEqual(state.file.inputs, [{'execute': 'input-send-event',
                                            'arguments': {'events': events}}])
        self.assertEqual(state.file.accepted, state.file.input_bytes)
        self.assertFalse(any(name == 'input-send-event' for name, _ in state.qmp.original_calls),
                         'input must inspect structured replies, not formatted cmd exceptions')
        self.assertEqual(len(state.gate.batches), 1)
        batch = state.gate.batches[0]
        self.assertEqual(batch['commit_count'], 1)
        self.assertEqual(len(batch['attempts']), 1)
        self.assertEqual(bytes.fromhex(batch['payload_hex']), state.file.input_bytes[0])
        self.assertLessEqual(batch['start'], batch['finish'])
        self.assertEqual(batch['deadline'], 5.0)
        self.assertIsNone(state.gate.pending)
        self.assertEqual(state.resumes, ['original-c'])
        json.dumps(state.gate.batches)

    def test_all_original_gui_forms_and_generic_routing_keep_exact_event_order(self):
        state = self.make_gate()
        pairs = [('ret', True), ('ret', False), ('shift', True), ('a', True),
                 ('a', False), ('shift', False), ('ctrl', True), ('c', True),
                 ('c', False), ('ctrl', False)]
        self.assertIsNone(state.qmp.keys(pairs))
        self.assertIsNone(state.qmp.abs_move(12345, 32767))
        self.assertIsNone(state.qmp.button(True))
        self.assertIsNone(state.qmp.button(False))
        generic = [
            {'type': 'rel', 'data': {'axis': 'x', 'value': -7}},
            {'type': 'key', 'data': {'down': True, 'key': {'type': 'number', 'data': 30}}},
            {'type': 'key', 'data': {'down': False, 'key': {'type': 'number', 'data': 30}}},
            {'type': 'btn', 'data': {'down': True, 'button': 'wheel-up'}}]
        self.assertEqual(self.input(state, generic, device='video0', head=0), {})
        arguments = [request['arguments'] for request in state.file.inputs]
        self.assertEqual(arguments[0]['events'], key_events(pairs))
        self.assertEqual(arguments[1]['events'], [
            {'type': 'abs', 'data': {'axis': 'x', 'value': 12345}},
            {'type': 'abs', 'data': {'axis': 'y', 'value': 32767}}])
        self.assertEqual([args['events'][0]['data']['down'] for args in arguments[2:4]],
                         [True, False])
        self.assertEqual(arguments[4], {'events': generic, 'device': 'video0', 'head': 0})
        self.assertEqual([b['commit_count'] for b in state.gate.batches], [1] * 5)
        self.assertEqual(len(state.file.accepted), 5)
        self.assertEqual(state.resumes, ['original-c'])

    def test_stop_after_running_precheck_retries_identical_zero_prefix_batch(self):
        state = self.make_gate()
        events = key_events([('ctrl', True), ('c', True), ('c', False), ('ctrl', False)])
        original = json.loads(json.dumps(events))
        input_count = 0
        def handler(request):
            nonlocal input_count
            if request['execute'] == 'input-send-event':
                input_count += 1
                if input_count == 1:
                    self.assertEqual(state.file.requests[-2]['execute'], 'query-status')
                    return Reply({'event': 'STOP'}, REJECT)
                return Reply(ACCEPT, accepted=True)
        state.file.handler = handler
        def resume(_count, _timeout):
            self.assertEqual(state.file.accepted, [], 'rejected batch dispatched a prefix')
            self.assertEqual(len(state.file.inputs), 1)
            events[0]['data']['down'] = False
            events.append({'type': 'rel', 'data': {'axis': 'x', 'value': 999}})
            self.natural_resume(state)
        waits = self.scripted_wait(state, resume)
        self.assertEqual(self.input(state, events, device='video0', head=1), {})
        self.assertTrue(waits, 'exact rejection must release the gate, not spin input')
        self.assertEqual(len(state.file.inputs), 2)
        self.assertEqual(state.file.input_bytes[0], state.file.input_bytes[1])
        self.assertEqual(state.file.inputs[1]['arguments']['events'], original)
        self.assertEqual(state.file.accepted, [state.file.input_bytes[1]])
        self.assertEqual(state.gate.batches[0]['commit_count'], 1)
        self.assertEqual(len(state.gate.batches[0]['attempts']), 2)
        self.assertEqual(state.resumes, ['original-c', 'original-c'])

    def test_accepted_previous_batch_is_never_replayed_with_rejected_next_batch(self):
        state = self.make_gate()
        prefix = key_events([('a', True), ('a', False)])
        suffix = key_events([('b', True), ('b', False)])
        self.input(state, prefix)
        counter = 0
        def handler(request):
            nonlocal counter
            if request['execute'] == 'input-send-event':
                counter += 1
                return Reply(REJECT) if counter == 1 else Reply(ACCEPT, accepted=True)
        state.file.handler = handler
        self.scripted_wait(state, lambda *_: self.natural_resume(state))
        self.input(state, suffix)
        self.assertEqual([r['arguments']['events'] for r in state.file.inputs],
                         [prefix, suffix, suffix])
        self.assertEqual([json.loads(raw)['arguments']['events'] for raw in state.file.accepted],
                         [prefix, suffix])
        self.assertEqual([b['commit_count'] for b in state.gate.batches], [1, 1])
        self.assertNotEqual(state.gate.batches[0]['id'], state.gate.batches[1]['id'])

    def test_paused_input_wait_releases_recursive_lock_for_snapshot_and_natural_resume(self):
        state = self.make_gate(running=False)
        self.wake_cleanup(state)
        waiting = self.observed_wait(state)
        def gui():
            with state.gate.lock:
                return self.input(state)
        thread, outcome = self.spawn(gui)
        self.assertTrue(waiting.wait(2), 'input did not wait while observer paused')
        self.assertIsNotNone(state.gate.pending)
        self.assertEqual(len(state.gate.batches), 1)
        snapshot_serviced = threading.Event()
        def observer():
            with state.gate.paused():
                state.qmp.cmd('query-cpus-fast')
                snapshot_serviced.set()
            state.gate.ordinary_resume(lambda: state.resumes.append('original-c'))
        observer_thread, observer_outcome = self.spawn(observer)
        self.joined(observer_thread, observer_outcome)
        self.assertTrue(snapshot_serviced.is_set())
        self.assertEqual(self.joined(thread, outcome), {})
        self.assertEqual(state.resumes, ['original-c'])
        self.assertEqual(len(state.file.accepted), 1)

    def test_native_proof_restoration_critical_section_excludes_input(self):
        state = self.make_gate()
        self.wake_cleanup(state)
        entered = threading.Event()
        state.clock.called = entered
        waiting = self.observed_wait(state)
        with state.gate.paused():
            thread, outcome = self.spawn(lambda: self.input(state))
            self.assertTrue(entered.wait(2), 'input deadline was not established before lock wait')
            for stage in ('stopped', 'native-step', 'proof', 'restoration'):
                self.trace_query(state, stage)
                self.assertEqual(state.file.inputs, [])
            self.assertEqual(state.gate.phase, PAUSED)
        self.assertTrue(waiting.wait(2))
        self.assertEqual(state.file.inputs, [])
        state.gate.ordinary_resume(lambda: state.resumes.append('original-c'))
        self.assertEqual(self.joined(thread, outcome), {})
        self.assertEqual(state.resumes, ['original-c', 'original-c'])

    def test_ordinary_resume_does_not_hold_gate_during_external_wait_stop(self):
        state = self.make_gate(running=False)
        resumed = threading.Event()
        release_wait_stop = threading.Event()
        self.addCleanup(release_wait_stop.set)
        def observer():
            state.gate.ordinary_resume(lambda: state.resumes.append('original-c'))
            resumed.set()
            self.assertTrue(release_wait_stop.wait(2), 'test did not finish ordinary wait_stop')
        thread, outcome = self.spawn(observer)
        self.assertTrue(resumed.wait(2))
        self.assertEqual(self.input(state), {})
        release_wait_stop.set()
        self.joined(thread, outcome)
        self.assertEqual(state.resumes, ['original-c'])

    def test_termination_wakes_paused_input_without_any_resume(self):
        state = self.make_gate(running=False)
        waiting = self.observed_wait(state)
        thread, outcome = self.spawn(lambda: self.input(state))
        self.assertTrue(waiting.wait(2))
        state.gate.terminate('observer shutdown')
        thread.join(2)
        self.assertFalse(thread.is_alive(), 'terminal notification did not wake input')
        self.assertIsInstance(outcome.get('error'), G.AdmissionError)
        self.assertEqual(state.resumes, [])
        self.assert_stopped(state)

    def test_paused_body_and_original_resume_failures_are_terminal(self):
        for failure in ('paused-body', 'ordinary-resume'):
            with self.subTest(failure=failure):
                state = self.make_gate(running=False)
                def bad_resume():
                    state.resumes.append('failed-original-c')
                    raise RuntimeError('original observer failure')
                with self.assertRaises(RuntimeError):
                    if failure == 'paused-body':
                        with state.gate.paused():
                            raise RuntimeError('proof failure')
                    else:
                        state.gate.ordinary_resume(bad_resume)
                self.assert_stopped(state)
                self.assertEqual(state.resumes,
                                 ['failed-original-c'] if failure == 'ordinary-resume' else [])

    def test_both_qmp_owners_keep_entire_request_reply_and_async_event_ownership(self):
        for first in ('observer', 'GUI'):
            with self.subTest(first=first):
                state = self.make_gate()
                state.file.handler = lambda request: Reply(
                    {'event': 'STOP'}, {'event': 'RESUME'},
                    {'return': [{'name': 'guest_*'}]} if request['execute'] == 'trace-event-get-state'
                    else RUN_STATUS if request['execute'] == 'query-status' else ACCEPT,
                    accepted=request['execute'] == 'input-send-event')
                state.file.block_command = ('trace-event-get-state' if first == 'observer'
                                            else 'input-send-event')
                observer = lambda: self.trace_query(state, 'guest_*')
                gui = lambda: self.input(state)
                a = self.spawn(observer if first == 'observer' else gui)
                self.assertTrue(state.file.entered.wait(2))
                b = self.spawn(gui if first == 'observer' else observer)
                state.file.release.set()
                first_value = self.joined(*a)
                second_value = self.joined(*b)
                self.assertEqual(first_value if first == 'GUI' else second_value, {})
                self.assertEqual(second_value if first == 'GUI' else first_value,
                                 [{'name': 'guest_*'}])
                self.assertIsNone(state.file.active)
                self.assertEqual(len(state.file.inputs), 1)

    def test_ambiguous_or_nonexact_reply_is_terminal_even_if_target_accepted(self):
        bad_replies = [
            socket.timeout('accepted reply lost'), b'', b'{malformed\n',
            OSError('transport disconnected'), [], None,
            {'error': {'class': 'GenericError', 'desc': 'VM not running!'}},
            {'error': {'class': 'OtherError', 'desc': 'VM not running'}},
            {'error': {'class': 'GenericError', 'desc': 'VM not running', 'extra': True}},
            {'error': {'class': 'GenericError', 'desc': 'other failure'}},
            {'return': {}, 'id': 'some-other-request'},
            {'return': {'unexpected': True}}, {'return': {}, 'error': REJECT['error']},
            {'unexpected': 'message'}, b'"not a reply object"\n',
            b'{"return":{},"return":{}}\n', b'{"return":NaN}\n',
            {'event': 7}, {'event': 'STOP', 'return': {}}]
        for reply in bad_replies:
            with self.subTest(reply=repr(reply)):
                state = self.make_gate()
                def handler(request):
                    if request['execute'] == 'input-send-event':
                        return Reply(reply, accepted=True)
                state.file.handler = handler
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assertEqual(len(state.file.accepted), 1)
                self.assert_stopped(state, count=1)
                self.assertEqual(state.qmp.sock.gettimeout(), 11.0)
                def observer_can_acquire():
                    with state.gate.lock:
                        return True
                self.assertTrue(self.joined(*self.spawn(observer_can_acquire)))

    def test_event_is_not_commit_and_formatted_error_text_is_not_rejection_proof(self):
        for reply in (b'', socket.timeout('VM not running'),
                      RuntimeError("QMP input-send-event: {'class': 'GenericError', 'desc': 'VM not running'}")):
            with self.subTest(reply=repr(reply)):
                state = self.make_gate()
                state.file.handler = lambda request: (Reply({'event': 'RESUME'}, reply, accepted=True)
                    if request['execute'] == 'input-send-event' else None)
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state, count=1)

    def test_missing_source_health_and_ownership_fail_closed(self):
        state = self.make_gate(running=False, source_verified=False)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assertEqual(state.file.requests, [])
        self.assert_stopped(state)
        for reason in ('unowned stop', 'source drift', 'coverage drift', 'observer error'):
            with self.subTest(reason=reason):
                state = self.make_gate()
                def unhealthy():
                    raise RuntimeError(reason)
                state.gate.health = unhealthy
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state)

    def test_health_is_rechecked_after_rejection_before_resubmission(self):
        state = self.make_gate()
        state.file.handler = lambda request: (Reply(REJECT)
            if request['execute'] == 'input-send-event' else None)
        def lose_ownership(_count, _timeout):
            def unowned():
                raise RuntimeError('stop is not observer owned')
            state.gate.health = unowned
        self.scripted_wait(state, lose_ownership)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=1)
        self.assertEqual(state.resumes, ['original-c'])

    def test_only_running_or_debug_readiness_states_are_permitted(self):
        states = [('paused', False), ('shutdown', False), ('suspended', False),
                  ('prelaunch', False), ('inmigrate', False), ('internal-error', False),
                  ('unknown', False), ('running', False), ('debug', True)]
        for name, running in states:
            with self.subTest(status=name, running=running):
                state = self.make_gate()
                state.file.handler = lambda request: Reply(
                    {'return': {'status': name, 'running': running}})
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state)

    def test_debug_before_owned_stop_defers_until_observer_naturally_resumes(self):
        state = self.make_gate()
        statuses = 0
        def handler(request):
            nonlocal statuses
            if request['execute'] == 'query-status':
                statuses += 1
                return Reply(DEBUG_STATUS if statuses == 1 else RUN_STATUS)
        state.file.handler = handler
        def observer(_count, _timeout):
            self.assertEqual(state.file.inputs, [])
            self.natural_resume(state)
        waits = self.scripted_wait(state, observer)
        self.assertEqual(self.input(state), {})
        self.assertTrue(waits)
        self.assertEqual(len(state.file.inputs), 1)
        self.assertEqual(state.resumes, ['original-c', 'original-c'])

    def test_exact_rejection_and_stale_notifications_do_not_replace_readiness(self):
        state = self.make_gate()
        def handler(request):
            if request['execute'] == 'input-send-event':
                return Reply(REJECT)
            return Reply(RUN_STATUS if not state.file.inputs else DEBUG_STATUS)
        state.file.handler = handler
        def stale_notification(_count, _timeout):
            state.clock.advance(1.0)
            with state.gate.condition:
                state.gate.condition.notify_all()
        self.scripted_wait(state, stale_notification)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=1)
        self.assertEqual(state.resumes, ['original-c'])

    def test_fresh_running_observation_can_admit_in_same_natural_resume_epoch(self):
        for first_response in ('debug', 'rejection'):
            with self.subTest(first_response=first_response):
                state = self.make_gate()
                statuses = 0
                inputs = 0
                def handler(request):
                    nonlocal statuses, inputs
                    if request['execute'] == 'query-status':
                        statuses += 1
                        return Reply(DEBUG_STATUS if first_response == 'debug' and statuses == 1
                                     else RUN_STATUS)
                    inputs += 1
                    return (Reply(REJECT) if first_response == 'rejection' and inputs == 1
                            else Reply(ACCEPT, accepted=True))
                state.file.handler = handler
                waits = self.scripted_wait(state, lambda *_: state.clock.advance(0.01))
                self.assertEqual(self.input(state), {})
                self.assertTrue(waits)
                self.assertEqual(state.gate.resume_epoch, 1)
                self.assertEqual(state.resumes, ['original-c'])
                self.assertEqual(len(state.file.accepted), 1)
                self.assertEqual(state.gate.batches[0]['commit_count'], 1)

    def test_max64_includes_first_attempt_and_last_attempt_can_commit(self):
        for final_acceptance in (False, True):
            with self.subTest(final_acceptance=final_acceptance):
                state = self.make_gate()
                count = 0
                def handler(request):
                    nonlocal count
                    if request['execute'] == 'input-send-event':
                        count += 1
                        if final_acceptance and count == 64:
                            return Reply(ACCEPT, accepted=True)
                        return Reply(REJECT)
                state.file.handler = handler
                waits = self.scripted_wait(state, lambda *_: self.natural_resume(state))
                if final_acceptance:
                    self.assertEqual(self.input(state), {})
                    self.assertEqual(state.gate.batches[0]['commit_count'], 1)
                    self.assertEqual(len(state.file.accepted), 1)
                else:
                    with self.assertRaises(G.AdmissionError):
                        self.input(state)
                    self.assert_stopped(state, count=64)
                self.assertEqual(count, 64)
                self.assertEqual(len(state.gate.batches[0]['attempts']), 64)
                self.assertGreaterEqual(len(waits), 63)
                self.assertEqual(len(set(state.file.input_bytes)), 1)

    def test_custom_lower_attempt_cap_is_not_reset_by_resumes(self):
        state = self.make_gate(max_attempts=3)
        state.file.handler = lambda request: (Reply(REJECT)
            if request['execute'] == 'input-send-event' else None)
        self.scripted_wait(state, lambda *_: self.natural_resume(state))
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=3)

    def test_one_deadline_covers_status_input_wait_and_subsequent_readiness(self):
        state = self.make_gate()
        def handler(request):
            if request['execute'] == 'query-status':
                return Reply(RUN_STATUS, elapsed=1.0)
            return Reply(REJECT, elapsed=1.0)
        state.file.handler = handler
        def wait(_count, timeout):
            self.assertLessEqual(timeout, 3.0)
            state.clock.advance(2.0)
            self.natural_resume(state)
        self.scripted_wait(state, wait)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=1)
        self.assertEqual(state.gate.batches[0]['deadline'], 5.0)
        self.assertEqual(state.qmp.sock.gettimeout(), 11.0)
        for now, timeout in state.qmp.sock.timeouts:
            if timeout != 11.0:
                self.assertGreater(timeout, 0)
                self.assertLessEqual(timeout, 5.0 - now)

    def test_total_deadline_is_established_before_contended_lock_acquisition(self):
        state = self.make_gate()
        computed = threading.Event()
        state.clock.called = computed
        with state.gate.lock:
            thread, outcome = self.spawn(lambda: self.input(state))
            self.assertTrue(computed.wait(2), 'deadline calculation waited for gate acquisition')
            state.clock.advance(5.001)
        thread.join(2)
        self.assertFalse(thread.is_alive())
        self.assertIsInstance(outcome.get('error'), G.AdmissionError)
        self.assertEqual(state.file.requests, [], 'expired lock wait obtained a fresh input budget')
        self.assertEqual(state.gate.phase, TERMINAL)

    def test_five_second_budget_and_original900_window_are_never_enlarged(self):
        for now, budget, deadline in ((0.0, 0.125, 0.125), (0.0, 20.0, 5.0),
                                      (899.75, 5.0, 900.0)):
            with self.subTest(now=now, budget=budget):
                state = self.make_gate(now=now, input_budget=budget, running=False)
                def expire(_count, timeout):
                    self.assertLessEqual(timeout, deadline - state.clock())
                    state.clock.advance(deadline - state.clock() + 0.001)
                self.scripted_wait(state, expire)
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state)
                self.assertEqual(state.gate.batches[0]['deadline'], deadline)
        state = self.make_gate(now=900.0, running=False)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assertEqual(state.file.requests, [])

    def test_nested_caller_scopes_use_absolute_minimum_and_clear_on_exit(self):
        state = self.make_gate()
        with state.gate.caller_scope(2.0):
            state.clock.advance(0.5)
            with state.gate.caller_scope(20.0):
                self.input(state)
            with state.gate.caller_scope(0.75):
                self.input(state)
            state.clock.advance(1.0)
            self.input(state)
        self.input(state)
        self.assertEqual([b['deadline'] for b in state.gate.batches], [2.0, 1.25, 2.0, 6.5])

    def test_caller_budget_includes_paused_wait_and_is_not_restarted(self):
        state = self.make_gate(running=False)
        def expire(_count, timeout):
            self.assertLessEqual(timeout, 0.5)
            state.clock.advance(0.5)
        self.scripted_wait(state, expire)
        with state.gate.caller_scope(1.0):
            state.clock.advance(0.5)
            with self.assertRaises(G.AdmissionError):
                self.input(state)
        self.assert_stopped(state)
        self.assertEqual(state.gate.batches[0]['deadline'], 1.0)

    def test_caller_scope_is_thread_local(self):
        state = self.make_gate()
        scoped = threading.Event()
        release = threading.Event()
        self.addCleanup(release.set)
        def other_caller():
            with state.gate.caller_scope(0.01):
                scoped.set()
                self.assertTrue(release.wait(2))
        thread, outcome = self.spawn(other_caller)
        self.assertTrue(scoped.wait(2))
        self.input(state)
        self.assertEqual(state.gate.batches[0]['deadline'], 5.0)
        release.set()
        self.joined(thread, outcome)

    def test_status_transport_failure_is_terminal_before_input(self):
        for reply in (socket.timeout('status timeout'), b'', b'bad json\n',
                      {'error': {'class': 'GenericError', 'desc': 'VM not running'}}):
            with self.subTest(reply=repr(reply)):
                state = self.make_gate()
                state.file.handler = lambda _request: Reply(reply)
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state)
                self.assertEqual(state.qmp.sock.gettimeout(), 11.0)

    def test_short_request_write_is_ambiguous_terminal_not_retryable(self):
        state = self.make_gate()
        def handler(request):
            if request['execute'] == 'input-send-event':
                state.file.write_result = 1
                return Reply(ACCEPT, accepted=True)
        state.file.handler = handler
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=1)
        self.assertEqual(len(state.file.accepted), 1)

    def test_raw_journal_preserves_every_request_reply_and_batch_identity(self):
        state = self.make_gate()
        count = 0
        def handler(request):
            nonlocal count
            if request['execute'] == 'input-send-event':
                count += 1
                return Reply({'event': 'STOP'}, REJECT) if count == 1 else Reply(ACCEPT, accepted=True)
        state.file.handler = handler
        self.scripted_wait(state, lambda *_: self.natural_resume(state))
        self.input(state, device='video0', head=1)
        records = [json.loads(line) for line in state.journal.read_text().splitlines()]
        self.assertTrue(records)
        raw_records = [record for record in records if 'raw_hex' in record]
        raw_values = [bytes.fromhex(record['raw_hex']) for record in raw_records]
        self.assertEqual(Counter(raw_values),
                         Counter(state.file.raw_requests + state.file.raw_replies),
                         'wire requests/replies must be journaled exactly once per occurrence')
        for record in raw_records:
            self.assertEqual(record['batch_id'], state.gate.batches[0]['id'])
            self.assertIn('attempt', record)
            self.assertIn('epoch', record)
            self.assertIn('time', record)
        replies = [record['reply'] for record in records if 'reply' in record]
        self.assertIn(REJECT, replies)
        self.assertIn(ACCEPT, replies)
        self.assertIn({'event': 'STOP'}, replies)
        batch = state.gate.batches[0]
        for field in ('id', 'payload_hex', 'deadline', 'start', 'finish', 'attempts',
                      'outcome', 'commit_count'):
            self.assertIn(field, batch)
        self.assertEqual(batch['commit_count'], 1)

    def test_ambiguous_failure_retains_raw_reply_and_real_stop_summary(self):
        state = self.make_gate()
        malformed = b'{accepted-but-truncated\n'
        state.file.handler = lambda request: (Reply(malformed, accepted=True)
            if request['execute'] == 'input-send-event' else None)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        records = [json.loads(line) for line in state.journal.read_text().splitlines()]
        self.assertIn(malformed.hex(), [record.get('raw_hex') for record in records])
        batch = state.gate.batches[0]
        self.assertEqual(batch['commit_count'], 0)
        self.assertTrue(batch.get('error'))
        self.assertIsNotNone(batch['finish'])
        self.assert_stopped(state, count=1)

    def test_late_success_is_terminal_but_known_commit_is_never_erased_or_replayed(self):
        state = self.make_gate()
        def late_read(request, _raw):
            if request['execute'] == 'input-send-event':
                state.clock.advance(5.0)
        state.file.read_hook = late_read
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assertEqual(state.gate.phase, TERMINAL)
        self.assertEqual(len(state.file.inputs), 1)
        self.assertEqual(len(state.file.accepted), 1)
        self.assertEqual(state.gate.batches[0]['commit_count'], 1)
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assertEqual(len(state.file.inputs), 1)
        self.assertEqual(sum(batch['commit_count'] for batch in state.gate.batches), 1)
        self.assertEqual(state.qmp.sock.gettimeout(), 11.0)

    def test_missing_health_unowned_phase_and_source_drift_during_wait_fail_closed(self):
        for failure in ('missing-health', 'unowned-phase', 'source-drift'):
            with self.subTest(failure=failure):
                state = self.make_gate()
                if failure == 'missing-health':
                    state.gate.health = lambda: False
                elif failure == 'unowned-phase':
                    state.gate.phase = 'UNOWNED_RUNNING'
                else:
                    state.file.handler = lambda request: (Reply(REJECT)
                        if request['execute'] == 'input-send-event' else None)
                    self.scripted_wait(state, lambda *_: setattr(state.gate, 'source_verified', False))
                with self.assertRaises(G.AdmissionError):
                    self.input(state)
                self.assert_stopped(state, count=1 if failure == 'source-drift' else 0)
                self.assertEqual(state.resumes, ['original-c'])

    def test_second_producer_cannot_replace_the_one_pending_batch(self):
        state = self.make_gate(running=False)
        self.wake_cleanup(state)
        waiting = self.observed_wait(state)
        thread, outcome = self.spawn(lambda: self.input(state))
        self.assertTrue(waiting.wait(2))
        pending = state.gate.pending
        self.assertIsNotNone(pending)
        with self.assertRaises(G.AdmissionError):
            self.input(state, key_events([('b', True), ('b', False)]))
        thread.join(2)
        self.assertFalse(thread.is_alive(), 'second-producer STOP did not wake first producer')
        self.assertIsInstance(outcome.get('error'), G.AdmissionError)
        self.assert_stopped(state)
        self.assertEqual(state.file.requests, [])
        self.assertEqual(state.resumes, [])

    def test_configured_large_attempt_count_cannot_raise_approved64_cap(self):
        state = self.make_gate(max_attempts=1000)
        state.file.handler = lambda request: (Reply(REJECT)
            if request['execute'] == 'input-send-event' else None)
        self.scripted_wait(state, lambda *_: state.clock.advance(0.001))
        with self.assertRaises(G.AdmissionError):
            self.input(state)
        self.assert_stopped(state, count=64)

    def test_caller_scope_clears_even_when_original_caller_raises(self):
        state = self.make_gate()
        with self.assertRaisesRegex(RuntimeError, 'caller failed'):
            with state.gate.caller_scope(0.1):
                raise RuntimeError('caller failed')
        self.input(state)
        self.assertEqual(state.gate.batches[0]['deadline'], 5.0)

    def test_benign_async_events_are_journaled_not_mistaken_for_acceptance(self):
        state = self.make_gate()
        event = {'event': 'RTC_CHANGE', 'data': {'offset': 1},
                 'timestamp': {'seconds': 12, 'microseconds': 34}}
        state.file.handler = lambda request: Reply(
            event, RUN_STATUS if request['execute'] == 'query-status' else ACCEPT,
            accepted=request['execute'] == 'input-send-event')
        self.assertEqual(self.input(state), {})
        self.assertEqual(state.gate.phase, RUNNING)
        self.assertEqual(state.gate.resume_epoch, 1)
        self.assertEqual(len(state.file.accepted), 1)
        records = [json.loads(line) for line in state.journal.read_text().splitlines()]
        self.assertEqual(sum(record.get('reply') == event for record in records), 2)
        self.assertEqual(state.resumes, ['original-c'])

    def test_noninput_error_eof_and_timeout_release_original_transaction_owner(self):
        failures = [b'', socket.timeout('observer timeout'),
                    {'error': {'class': 'GenericError', 'desc': 'observer query failed'}}]
        for running in (False, True):
            for failure in failures:
                with self.subTest(running=running, failure=repr(failure)):
                    state = self.make_gate(running=running)
                    failed = False
                    def handler(request):
                        nonlocal failed
                        if request['execute'] == 'query-cpus-fast' and not failed:
                            failed = True
                            return Reply(failure)
                    state.file.handler = handler
                    with self.assertRaises((RuntimeError, socket.timeout)):
                        state.qmp.cmd('query-cpus-fast')
                    if not running:
                        self.natural_resume(state)
                    self.assertEqual(self.joined(*self.spawn(lambda: self.input(state))), {})
                    self.assertEqual(state.qmp.sock.gettimeout(), 11.0)
                    self.assertEqual(len(state.file.accepted), 1)


if __name__ == '__main__':
    unittest.main()
