"""New integration tests plus input-gate and retained34 host tests; no QEMU."""
from pathlib import Path
import queue
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import coordinated as C
import test_admission as G
import test_serialized as S


class IntegrationTests(unittest.TestCase):
    def gate(self):
        temp = tempfile.TemporaryDirectory(prefix='s10-4-input-integration-')
        self.addCleanup(temp.cleanup)
        machine = SimpleNamespace(qmp=SimpleNamespace(), start_ns=time.monotonic_ns())
        return C.InputGate(machine, Path(temp.name) / 'input.jsonl', source_verified=True)

    def client(self, gate):
        client = object.__new__(C.CoordinatedClient)
        client.gate = gate
        return client

    def test_entire_original_stopped_region_and_native_step_hold_common_gate(self):
        gate = self.gate()
        observer = object.__new__(C.CoordinatedObserver)
        observer.gate = gate
        calls = []
        def stopped(owner, reply):
            self.assertTrue(gate.lock._is_owned())
            self.assertEqual(gate.phase, gate.PAUSED_OR_STEPPING)
            calls.append(('stopped', reply))
            return 'handled'
        with patch.object(C.X.Observer, '_stopped', stopped):
            self.assertEqual(observer._stopped(b'T05thread:01;'), 'handled')
        self.assertFalse(gate.lock._is_owned())
        def step(owner, addr):
            self.assertTrue(gate.lock._is_owned())
            self.assertEqual(gate.phase, gate.PAUSED_OR_STEPPING)
            calls.append(('step', addr))
            return b'T05thread:01;'
        with patch.object(C.R.RecurringClient, 'step_over_breakpoint', step):
            self.assertEqual(self.client(gate).step_over_breakpoint(0x1000), b'T05thread:01;')
        self.assertEqual(calls, [('stopped', b'T05thread:01;'), ('step', 0x1000)])
        self.assertFalse(gate.lock._is_owned())

    def test_resume_invokes_only_original_callback_once(self):
        gate = self.gate()
        client = self.client(gate)
        calls = []
        def original(owner):
            self.assertTrue(gate.lock._is_owned())
            calls.append('original-c')
            return 'sent'
        with patch.object(C.R.RecurringClient, 'resume', original):
            self.assertEqual(client.resume(), 'sent')
        self.assertEqual(calls, ['original-c'])
        self.assertEqual(gate.resume_epoch, 1)
        self.assertEqual(gate.phase, gate.ORDINARY_RESUME_PENDING_OR_RUNNING)
        self.assertFalse(gate.lock._is_owned())
        self.assertEqual(gate.batches, [])

    def test_terminal_gate_never_invokes_original_resume(self):
        gate = self.gate()
        gate.terminate('original proof failed')
        with patch.object(C.R.RecurringClient, 'resume') as original:
            with self.assertRaisesRegex(C.AdmissionError, 'original proof failed'):
                self.client(gate).resume()
            original.assert_not_called()

    def test_failed_original_native_region_is_terminal_no_wrapper_recovery(self):
        gate = self.gate()
        with patch.object(C.R.RecurringClient, 'step_over_breakpoint', side_effect=RuntimeError('native proof failed')) as original:
            with self.assertRaisesRegex(RuntimeError, 'native proof failed'):
                self.client(gate).step_over_breakpoint(0x1000)
            original.assert_called_once_with(0x1000)
        self.assertEqual(gate.phase, gate.TERMINAL)
        self.assertFalse(gate.lock._is_owned())

    def test_original_snapshot_condition_wait_releases_gate(self):
        gate = self.gate()
        observer = object.__new__(C.CoordinatedObserver)
        observer.gate = gate
        observer.error = None
        observer.requests = queue.Queue()
        errors = []
        def service():
            try:
                _, event, result = observer.requests.get(timeout=1)
                acquired = gate.lock.acquire(timeout=.5)
                if not acquired:
                    observer.error = 'snapshot condition retained admission lock'
                else:
                    gate.lock.release()
                    result['serviced'] = True
                event.set()
            except BaseException as error:
                errors.append(error)
                observer.error = str(error)
        thread = threading.Thread(target=service, daemon=True)
        thread.start()
        observer.snapshot('test-mapping')
        thread.join(2)
        self.assertFalse(thread.is_alive())
        self.assertEqual(errors, [])
        self.assertIsNone(observer.error)

    def test_wrapper_run_does_not_lock_ordinary_wait_stop(self):
        gate = self.gate()
        observer = object.__new__(C.CoordinatedObserver)
        observer.gate = gate
        observer.error = None
        def ordinary_run(owner):
            self.assertFalse(gate.lock._is_owned())
            return 'ordinary-wait-stop'
        with patch.object(C.X.Observer, 'run', ordinary_run):
            self.assertEqual(observer.run(), 'ordinary-wait-stop')
        self.assertEqual(gate.phase, gate.TERMINAL)

    def test_original_surface_caller_budgets_are_not_enlarged(self):
        gate = self.gate()
        fixed_time = gate.clock()
        gate.clock = lambda: fixed_time
        surface = object.__new__(C.CoordinatedSurface)
        surface.gate = gate
        observed = []
        def original_command(owner, name, cmd, timeout):
            observed.append((timeout, gate._caller.deadline - gate.clock()))
            return ['st=0']
        with patch.object(C.X.Surface, 'command', original_command):
            self.assertEqual(surface.command('status', 'echo st=$?', 60), ['st=0'])
        self.assertEqual(observed[0][0], 60)
        self.assertEqual(observed[0][1], 60)
        self.assertFalse(hasattr(gate._caller, 'deadline'))
        def original_submit(owner, cmd):
            self.assertLessEqual(gate._caller.deadline - gate.clock(), 120)
            return 'original-input'
        with patch.object(C.X.Surface, 'submit', original_submit):
            self.assertEqual(surface.submit('top'), 'original-input')
        self.assertFalse(hasattr(gate._caller, 'deadline'))


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([tests, loader.loadTestsFromModule(G), loader.loadTestsFromModule(S)])


if __name__ == '__main__':
    unittest.main()
