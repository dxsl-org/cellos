"""Host-only full-transaction concurrency tests plus retained27 recurrence tests."""
import inspect
import json
from pathlib import Path
import socket
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import serialized as S
import test_recurring as T


class ObservedRLock:
    """Real RLock with an event proving another thread reached acquire."""
    def __init__(self):
        self.lock = threading.RLock()
        self.owner = None
        self.depth = 0
        self.contender = threading.Event()

    def __enter__(self):
        ident = threading.get_ident()
        if self.owner is not None and self.owner != ident:
            self.contender.set()
        self.lock.acquire()
        self.owner = ident
        self.depth += 1
        return self

    def __exit__(self, *unused):
        self.depth -= 1
        if not self.depth:
            self.owner = None
        self.lock.release()


class TransactionFile:
    """Scripted QMP transport: overlapping writers are assertion failures."""
    def __init__(self):
        self.requests = []
        self.active = None
        self.responses = []
        self.entered = threading.Event()
        self.release = threading.Event()
        self.block_next = False
        self.failure = None

    def write(self, payload):
        assert self.active is None, 'request interleaved before previous reply was consumed'
        request = json.loads(payload)
        self.requests.append(request)
        self.active = request
        command = request['execute']
        if command == 'query-cpus-fast':
            result = [{'cpu-index': 0, 'target': 'x86_64', 'thread-id': 22}]
        elif command == 'query-status':
            result = {'running': False, 'status': 'debug'}
        elif command == 'trace-event-get-state':
            result = [{'name': request['arguments']['name'], 'state': 'enabled', 'vcpu': False}]
        else:
            result = {'command': command}
        self.responses = [{'event': 'STOP'}, {'return': result}]
        return len(payload)

    def readline(self):
        if self.block_next:
            self.block_next = False
            self.entered.set()
            assert self.release.wait(2), 'test did not release blocked QMP reply'
        if self.failure:
            failure, self.failure = self.failure, None
            self.active = None
            self.responses = []
            if failure == 'EOF':
                return b''
            if failure == 'timeout':
                raise socket.timeout('synthetic transport timeout')
            return b'{"error":{"class":"GenericError","desc":"synthetic failure"}}\n'
        value = self.responses.pop(0)
        if 'return' in value:
            self.active = None
        return json.dumps(value).encode() + b'\n'


class SerializedTests(unittest.TestCase):
    def owners(self, observed=False):
        temp = tempfile.TemporaryDirectory(prefix='s10-4-qmp-lock-unit-')
        self.addCleanup(temp.cleanup)
        work = Path(temp.name)
        (work / 'evidence').mkdir()
        qmp = S.X.D.Qmp.__new__(S.X.D.Qmp)
        qmp.file = TransactionFile()
        if observed:
            lock = ObservedRLock()
            with patch.object(S.threading, 'RLock', return_value=lock):
                self.assertIs(S.install_serialization(qmp), lock)
        else:
            lock = S.install_serialization(qmp)
        control = S.SerializedTraceControl(SimpleNamespace(qmp=qmp, work=work, start_ns=time.monotonic_ns()))
        return qmp, control, lock

    def spawn(self, function):
        outcome = {}
        def run():
            try:
                outcome['value'] = function()
            except BaseException as error:
                outcome['error'] = error
        thread = threading.Thread(target=run, daemon=True)
        thread.start()
        return thread, outcome

    def joined(self, thread, outcome):
        thread.join(2)
        self.assertFalse(thread.is_alive(), 'QMP transaction/snapshot deadlock')
        if 'error' in outcome:
            raise outcome['error']
        return outcome.get('value')

    def test_full_transactions_serialize_both_owners_and_async_events(self):
        for gui_command in ('input-send-event', 'screendump'):
            for observer_command in ('query-cpus-fast', 'query-status', 'trace-event-get-state'):
                for first_owner in ('GUI', 'observer'):
                    with self.subTest(gui=gui_command, observer=observer_command, first=first_owner):
                        qmp, control, lock = self.owners(observed=True)
                        qmp.file.block_next = True
                        def gui():
                            if gui_command == 'input-send-event':
                                return qmp.keys([('ret', True), ('ret', False)])
                            return qmp.cmd('screendump', filename='/synthetic/screen.ppm', format='ppm')
                        def observer():
                            args = {'name': 'exec_tb'} if observer_command == 'trace-event-get-state' else {}
                            return control.query(observer_command, **args)
                        first, second = (gui, observer) if first_owner == 'GUI' else (observer, gui)
                        one, result_one = self.spawn(first)
                        try:
                            self.assertTrue(qmp.file.entered.wait(1), 'first owner did not reach reply read')
                            two, result_two = self.spawn(second)
                            self.assertTrue(lock.contender.wait(1), 'second owner bypassed shared transaction lock')
                            self.assertEqual(len(qmp.file.requests), 1)
                        finally:
                            qmp.file.release.set()
                        value_one = self.joined(one, result_one)
                        value_two = self.joined(two, result_two)
                        self.assertEqual(len(qmp.file.requests), 2)
                        observer_value = value_two if first_owner == 'GUI' else value_one
                        if observer_command == 'query-status':
                            self.assertEqual(observer_value, {'running': False, 'status': 'debug'})
                        elif observer_command == 'query-cpus-fast':
                            self.assertEqual(observer_value[0]['thread-id'], 22)
                        else:
                            self.assertEqual(observer_value[0]['name'], 'exec_tb')
                        self.assertIsNone(qmp.file.active)

    def test_QMP_error_EOF_timeout_release_lock_for_other_thread(self):
        for owner in ('GUI', 'observer'):
            for failure in ('error', 'EOF', 'timeout'):
                with self.subTest(owner=owner, failure=failure):
                    qmp, control, lock = self.owners()
                    qmp.file.failure = failure
                    with self.assertRaises(Exception):
                        if owner == 'GUI':
                            qmp.cmd('screendump', filename='/synthetic/frame.ppm')
                        else:
                            control.query('query-status')
                    acquired = threading.Event()
                    def other_thread():
                        with lock:
                            acquired.set()
                    thread, outcome = self.spawn(other_thread)
                    self.joined(thread, outcome)
                    self.assertTrue(acquired.is_set())
                    # Lock release is tested without retrying a failed/closed transport.
                    self.assertEqual(len(qmp.file.requests), 1)

    def test_cmd_API_and_direct_trace_name_argument_remain_exact(self):
        qmp, control, _ = self.owners()
        self.assertEqual(str(inspect.signature(qmp.cmd)), '(name, **args)')
        self.assertEqual(control.query('trace-event-get-state', name='exec_tb')[0]['name'], 'exec_tb')
        self.assertEqual(qmp.file.requests[-1], {'execute': 'trace-event-get-state', 'arguments': {'name': 'exec_tb'}})
        with self.assertRaises(TypeError):
            qmp.cmd('trace-event-get-state', name='exec_tb')
        self.assertEqual(len(qmp.file.requests), 1)

    def test_no_lock_held_across_GUI_snapshot_queue_wait(self):
        qmp, control, _ = self.owners()
        request = threading.Event()
        complete = threading.Event()
        def gui():
            qmp.keys([('ret', True), ('ret', False)])
            request.set()
            self.assertTrue(complete.wait(1), 'snapshot service blocked by GUI-held QMP lock')
        def service():
            self.assertTrue(request.wait(1))
            value = control.query('query-status')
            complete.set()
            return value
        observer, observer_result = self.spawn(service)
        desktop, desktop_result = self.spawn(gui)
        self.joined(desktop, desktop_result)
        self.assertEqual(self.joined(observer, observer_result)['status'], 'debug')

    def test_lock_is_reentrant_for_same_owner(self):
        qmp, _, lock = self.owners()
        def nested():
            with lock:
                return qmp.cmd('input-send-event', events=[])
        thread, result = self.spawn(nested)
        self.assertEqual(self.joined(thread, result), {'command': 'input-send-event'})

    def test_second_install_is_rejected_without_changing_owner(self):
        qmp, _, lock = self.owners()
        with self.assertRaisesRegex(Exception, 'already installed'):
            S.install_serialization(qmp)
        self.assertIs(qmp._observer_transaction_lock, lock)

    def test_trace_policy_failure_also_releases_same_lock(self):
        qmp, control, lock = self.owners()
        with self.assertRaises(S.X.PolicyError):
            control.query('human-monitor-command', command_line='info registers')
        def acquire():
            with lock:
                return True
        thread, result = self.spawn(acquire)
        self.assertTrue(self.joined(thread, result))
        self.assertEqual(qmp.file.requests, [])


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([tests, loader.loadTestsFromModule(T)])


if __name__ == '__main__':
    unittest.main()
