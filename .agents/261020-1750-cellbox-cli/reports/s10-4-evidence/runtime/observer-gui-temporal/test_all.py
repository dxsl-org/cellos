"""One offline post-edit suite: retained144 + temporal, renderer and retention proofs."""
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from PIL import Image
import gui_temporal as G
import test_temporal as T

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('retained_atomic_tests', G.PREVIOUS / 'test_all.py')
OLD = importlib.util.module_from_spec(spec)
spec.loader.exec_module(OLD)


class TimeoutRetention(unittest.TestCase):
    def desk(self, directory):
        desk = object.__new__(G.TemporalDesk)
        desk.a = SimpleNamespace(evidence=directory)
        desk.last_frame = Image.new('RGB', (16, 12), (5, 6, 7))
        desk.last_frame_ns = 101
        desk.last_read = None
        desk.active_epoch = {'id': 1, 'state': 'ARMED', 'ledger': {'complete': False}}
        desk.failure_records = []
        desk._retained_errors = set()
        desk.gate = SimpleNamespace(phase='ordinary', resume_epoch=4, batches=[])
        desk.read_screen = lambda image: (['actual retained row'], (3, 0))
        return desk

    def test_all_wait_error_types_rethrow_and_keep_last_already_observed_frame_read_epoch(self):
        for error in (G.M.Stop('timeout'), G.X.D.StepFailed('screen'), RuntimeError('observer'), TimeoutError('input')):
            with self.subTest(error=type(error).__name__), tempfile.TemporaryDirectory() as directory:
                desk = self.desk(directory)
                with patch.object(G.M.CheckedDesk, 'wait_for', side_effect=error) as original:
                    with self.assertRaises(type(error)) as raised:
                        desk.wait_for(lambda image: False, 'original condition', 120, interval=1.5)
                self.assertIs(raised.exception, error)
                self.assertEqual(original.call_args.args[2:], (120, 1.5))
                result = json.loads((Path(directory) / 'gui-failure-01.json').read_text())
                self.assertEqual(result['last_exact_read'], [['actual retained row'], [3, 0]])
                self.assertEqual(result['epoch']['state'], 'ARMED')
                self.assertEqual(result['last_frame_ns'], 101)
                self.assertTrue(Path(result['last_frame']).exists())
                self.assertEqual(len(desk.failure_records), 1)
                desk.retain_failure(error, {'outer': True})
                self.assertEqual(len(desk.failure_records), 1)

    def test_retention_decodes_only_last_frame_without_guest_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            desk = self.desk(directory)
            desk.grab = lambda: self.fail('extra guest framebuffer request forbidden on timeout')
            desk.retain_failure(G.M.Stop('deadline'), {'timeout': 120})
            self.assertEqual(desk.last_read, (['actual retained row'], (3, 0)))

    def test_actual_client_keeps_atomic_receive_and_original_guard_resume(self):
        self.assertIs(G.F.AtomicClient._event, G.F.AtomicReceiveMixin._event)
        self.assertIs(G.F.AtomicClient.step_over_breakpoint, G.C.CoordinatedClient.step_over_breakpoint)
        self.assertIs(G.F.AtomicClient.resume, G.C.CoordinatedClient.resume)
        self.assertTrue(issubclass(G.TemporalSurface, G.C.CoordinatedSurface))

    def test_nonfinite_status_preserves_original_nonzero_status_path(self):
        desk = object.__new__(G.TemporalDesk)
        desk.finite_status_pending = False
        with patch.object(G.M.CheckedDesk, 'command_read', return_value=['st=130']) as original:
            result = desk.command_read('status', G.STATUS, 60)
        self.assertEqual(result, ['st=130'])
        self.assertEqual(original.call_args.args, ('status', G.STATUS, 60, None))

    def test_only_qualified_known_banner_is_opaque_preoutput_history(self):
        line = T.PROMPT + T.COMMAND
        ledger = G.TemporalLedger(79, 26, ['', G.X.D.BANNER, line], (len(line), 2), T.COMMAND)
        self.assertFalse(ledger.snapshot()['complete'])
        self.assertEqual(ledger.snapshot()['typed_rows'][1], G.X.D.BANNER)

    def test_current_prefix_alone_never_bridges_lost_stable_row_coverage(self):
        grid = T.ASCIIGrid()
        ledger = T.ledger_for(grid)
        grid.feed('\n' * 25)
        with self.assertRaises(G.OracleIncomplete):
            ledger.observe(*grid.capture(), T.FIRST_NS)
        self.assertFalse(ledger.snapshot()['complete'])


class RenderedTemporalProof(unittest.TestCase):
    def test_source_geometry_complete_ordered_vs_missed_output(self):
        from rendered_proof import prove
        proof = prove()
        self.assertTrue(proof['complete_ordered_trace_accepts'])
        self.assertTrue(proof['coalesced_or_dropped_trace_rejects'])
        self.assertTrue(proof['all_exact_pixel_samples_read'])
        self.assertFalse(proof['historical_desktop_completion_proved'])


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([tests, loader.loadTestsFromModule(OLD), loader.loadTestsFromModule(T)])


if __name__ == '__main__':
    original_exercise = OLD.P.exercise
    offline = HERE / 'retained-test-reproduction'
    offline.mkdir(mode=0o700, exist_ok=False)
    def retained_exercise(client_type, directory, label):
        # Retained regression is unchanged; only its synthetic output destination moves.
        target = offline if Path(directory) == G.PREVIOUS else directory
        return original_exercise(client_type, target, label)
    with patch.object(OLD.P, 'exercise', side_effect=retained_exercise):
        unittest.main(verbosity=2)
