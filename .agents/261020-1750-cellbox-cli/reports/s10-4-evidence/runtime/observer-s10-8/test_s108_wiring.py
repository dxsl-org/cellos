"""S10-8.0 offline tests: the `_stop` collision fix.  No VM.

* every S10-7 mixin test (entry, snapshot, fail-closed, write_results) re-runs against the S10-8 `MemoryMixin`;
* a real observer THREAD (the S10-7 defect surface) starts, finishes and joins without error;
* every wiring stop still raises `common.Stop`;
* negative control: the retained S10-7 mixin reproduces the TypeError, so the regression tests can detect the defect.
"""
import json
import sys
import tempfile
import threading
import types
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_env as ENV  # noqa: E402
import s108_memory as MEM8  # noqa: E402
import s108_run as RUN8  # noqa: E402
import s107_env as E7  # noqa: E402
import s107_run as RUN7  # noqa: E402
import test_s107_wiring as W7  # noqa: E402  (module import: its tests are NOT collected here)
import common as COMMON  # noqa: E402


class Observer8(MEM8.MemoryMixin, W7.FakeObserver):
    SKIP_SNAPSHOT_NAMES = ('\0disarm',)


class Mixin8(W7.Mixin):
    """All S10-7 mixin tests against the fixed mixin (inherited test methods, `observer()` swapped)."""

    def observer(self, guest=None, **kwargs):
        guest = guest or W7.build()
        guest.map_identity_range(self.PC, self.PC + 3 * W7.PAGE, fill=0)
        return Observer8(self.machine, guest, self.PC, memory=W7.context(**kwargs))

    def test_write_results_summarises(self):
        observer = self.observer()
        observer._entry(self.PC)
        observer._snapshot('complete')
        observer.write_results()
        self.assertTrue(observer.written)
        summary = json.loads((self.work / 'evidence/memory-summary.json').read_text())
        self.assertEqual(summary['schema'], 'cellos.s10-8.memory-run-summary/1')
        self.assertEqual(len(summary['snapshots']), 1)


def fake_machine(tmp, profile):
    work = Path(tmp)
    (work / 'evidence').mkdir()
    (work / 'sock').mkdir()
    _record, files = E7.candidate_files(profile)
    return types.SimpleNamespace(work=work, gdb_path=work / 'sock/gdb.sock', a=types.SimpleNamespace(profile=profile),
                                 inputs={'files': {'/bin/cellbox': {'file': files['/bin/cellbox']['file']}}})


class NoCollision(unittest.TestCase):
    def test_no_stop_name_in_the_mixin_and_thread_stop_is_inherited_unchanged(self):
        self.assertNotIn('_stop', vars(MEM8.MemoryMixin))
        self.assertIn('_wiring_stop', vars(MEM8.MemoryMixin))
        self.assertIs(RUN8.MemoryPlainObserver._stop, threading.Thread._stop)
        self.assertIs(RUN8.MemoryHookObserver._stop, threading.Thread._stop)
        self.assertNotIn('_stop', vars(RUN8.MemoryPlainObserver))
        self.assertNotIn('_stop', vars(RUN8.MemoryHookObserver))

    def test_source_raises_only_through_the_renamed_helper(self):
        text = (HERE / 's108_memory.py').read_text()
        self.assertNotIn('def _stop(', text)
        self.assertNotIn('self._stop(', text)
        self.assertEqual(text.count('raise self._wiring_stop('), 3)

    def test_wiring_stop_is_a_retained_stop(self):
        observer = Observer8.__new__(Observer8)
        error = observer._wiring_stop('boom')
        self.assertIsInstance(error, COMMON.Stop)
        self.assertEqual(str(error), 'boom')
        with self.assertRaises(COMMON.Stop):
            raise observer._wiring_stop('x')


class ThreadLifecycle(unittest.TestCase):
    """The retained Observer is a threading.Thread: CPython calls `_stop()` with no arguments when it finishes."""

    def test_plain_observer_thread_starts_finishes_and_joins(self):
        with tempfile.TemporaryDirectory() as tmp:
            observer = RUN8.MemoryPlainObserver(fake_machine(tmp, 'headless'), memory=RUN8.memory_context(ENV.SETS['cand-headless']))
            observer.start()
            observer.join(10)                       # S10-7: TypeError('_stop() missing 1 required positional argument') raised here
            self.assertFalse(observer.is_alive())
            self.assertTrue(observer.ready.is_set())
            self.assertIsNotNone(observer.error)    # no gdb stub exists: the retained run() records its own connection failure
            self.assertNotIn('TypeError', observer.error)
            self.assertTrue((Path(tmp) / 'evidence/observation.json').is_file())
            self.assertTrue((Path(tmp) / 'evidence/memory-summary.json').is_file())
            with self.assertRaises(COMMON.Stop) as raised:      # retained shutdown(): join, then the recorded observation blocker
                observer.shutdown()
            self.assertIn('observation blocker', str(raised.exception))

    def test_hook_observer_thread_starts_finishes_and_joins(self):
        with tempfile.TemporaryDirectory() as tmp:
            terminated = []
            gate = types.SimpleNamespace(terminate=terminated.append)       # retained CoordinatedObserver.run() calls gate.terminate() on exit
            observer = RUN8.MemoryHookObserver(fake_machine(tmp, 'desktop'), gate, RUN8.HO.load_static(), Path(tmp) / 'ledger.jsonl',
                                               memory=RUN8.memory_context(ENV.SETS['cand-desktop']))
            observer.start()
            observer.join(10)
            self.assertFalse(observer.is_alive())
            self.assertIsNotNone(observer.error)
            self.assertNotIn('TypeError', observer.error)
            with self.assertRaises(COMMON.Stop) as raised:
                observer.shutdown()
            self.assertIn('observation blocker', str(raised.exception))

    def test_negative_control_the_retained_s10_7_mixin_reproduces_the_defect(self):
        with tempfile.TemporaryDirectory() as tmp:
            observer = RUN7.MemoryPlainObserver(fake_machine(tmp, 'headless'), memory=RUN7.memory_context(E7.SETS['cand-headless']))
            observer.start()
            with self.assertRaises(TypeError) as raised:
                for _ in range(100):
                    observer.join(.1)
                    if not observer.is_alive():
                        break
            self.assertIn('_stop', str(raised.exception))
            self.assertIn('message', str(raised.exception))


class WiringStopsStillRaiseStop(unittest.TestCase):
    """Each of the three wiring failure sites raises common.Stop (through `_wiring_stop`)."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.work = Path(self.tmp.name)
        (self.work / 'evidence').mkdir()
        self.machine = types.SimpleNamespace(work=self.work)

    def observer(self, guest=None):
        guest = guest or W7.build()
        guest.map_identity_range(W7.PC0, W7.PC0 + 3 * W7.PAGE, fill=0)
        return Observer8(self.machine, guest, W7.PC0, memory=W7.context())

    def test_entry_range_failure(self):
        observer = self.observer(W7.build(kernel_rsp=0, tss=0))
        with self.assertRaises(COMMON.Stop) as raised:
            observer._entry(W7.PC0)
        self.assertIn('cannot establish stack ranges', str(raised.exception))

    def test_snapshot_without_entry_record(self):
        observer = self.observer()
        observer.focus = W7.PC0
        observer.instances[W7.PC0] = {}
        with self.assertRaises(COMMON.Stop) as raised:
            observer._snapshot('complete')
        self.assertIn('no focus instance', str(raised.exception))

    def test_snapshot_failure_closed(self):
        guest = W7.build()
        observer = self.observer(guest)
        observer._entry(W7.PC0)
        guest.unreadable.add(W7.UTOP - 8 * W7.PAGE)
        with self.assertRaises(COMMON.Stop) as raised:
            observer._snapshot('finite-stream')
        self.assertIn('failed closed', str(raised.exception))


class RealClassesChain(unittest.TestCase):
    def test_mro_and_retained_methods_unchanged(self):
        self.assertEqual(RUN8.MemoryHookObserver.__mro__[1], MEM8.MemoryMixin)
        self.assertIs(RUN8.MemoryHookObserver._handle, RUN8.HO.HookObserver._handle)
        self.assertIs(RUN8.MemoryHookObserver._stopped, RUN8.C.CoordinatedObserver._stopped)
        self.assertEqual(RUN8.MemoryHookObserver.SKIP_SNAPSHOT_NAMES, (RUN8.HO.DISARM,))
        self.assertIs(RUN8.MemoryPlainObserver._entry, MEM8.MemoryMixin._entry)

    def test_shared_helpers_are_the_unchanged_s10_7_objects(self):
        for name in ('DiscoveryError', 'KernelSymbols', 'MemoryContext', 'entry_record', 'discover_kernel_stack', 'discover_user_stack'):
            self.assertIs(getattr(MEM8, name), getattr(RUN7.MEM, name), name)


if __name__ == '__main__':
    unittest.main()
