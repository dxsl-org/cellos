"""S10-4.14 one post-implementation host verification; no VM/guest execution."""
import json
import unittest
from unittest.mock import patch

import run_atomic as F
import reproduce_before as P
from atomic_rsp import AtomicRspClient
import test_atomic as A
import test_coordinated as C


class FixedBoundaryProof(unittest.TestCase):
    def test_exact_unchanged_deadline_reproduction_now_preserves_dispatches_once(self):
        before = json.loads((F.HERE / 'reproduction-before.json').read_text())
        self.assertTrue(before['old_loss_signature'])
        self.assertTrue(before['valid_checksum'])
        self.assertFalse(before['stop_received_across_original_polls'])
        after = P.exercise(AtomicRspClient, F.HERE, 'reproduction-after')
        P.assert_delivery(after)
        self.assertEqual(after['polls'][0]['buffer_hex'], P.FRAME.hex())
        self.assertEqual(after['polls'][0]['ACK_count'], 0)
        self.assertEqual(after['receive_calls'], 1)

    def test_actual_observer_client_inherits_fixed_receive_and_original_native_guard(self):
        self.assertIs(F.AtomicClient._event, F.AtomicReceiveMixin._event)
        self.assertIs(F.AtomicClient.step_over_breakpoint, F.C.CoordinatedClient.step_over_breakpoint)
        self.assertIs(F.AtomicClient.resume, F.C.CoordinatedClient.resume)
        self.assertIs(F.AtomicClient._validate, F.C.CoordinatedClient._validate)


def load_tests(loader, tests, pattern):
    return unittest.TestSuite([tests, loader.loadTestsFromModule(A), loader.loadTestsFromModule(C)])


if __name__ == '__main__':
    unittest.main()
