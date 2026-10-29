"""Deterministic host-only S10-4.11 tests; no candidate/QEMU execution."""
import copy
import unittest
from unittest.mock import patch

import recurring as R
import test_amendment as T


class ProofTests(unittest.TestCase):
    # Reuse unchanged proof assertions against the exact checker used by the new caller.
    test_ordered_same_TB = T.AmendmentTests.test_complete_ordered_same_TB_flag3_proof_admitted
    test_pairing_order_flags = T.AmendmentTests.test_entry_exit_order_pointer_PC_count_and_flag_are_required
    test_restoration_mode = T.AmendmentTests.test_missing_restore_remove_OK_disable_or_native_mode_rejected
    test_interleaved_controls = T.AmendmentTests.test_interleaved_continue_extra_step_or_watch_removal_rejected
    test_every_context_field = T.AmendmentTests.test_each_stable_context_field_checked_on_after_and_fresh_read
    test_vcpu_identity = T.AmendmentTests.test_vcpu_running_state_and_trace_TID_mismatch_rejected
    test_complete_census = T.AmendmentTests.test_full_coverage_enrollment_and_duplicate_census_rejected
    test_capture_hash_health = T.AmendmentTests.test_missing_capture_digest_health_or_unexpected_stop_rejected
    test_expected_empty_lifecycle = T.AmendmentTests.test_empty_watch_census_requires_complete_stable_boot_lifecycle
    test_enrolled_heap_not_empty = T.AmendmentTests.test_equal_but_missing_enrolled_heap_watch_is_not_expected_emptiness
    test_enabled_retained_health = T.AmendmentTests.test_retained_health_and_actual_logging_acknowledgment_are_required


class RecurringTests(unittest.TestCase):
    def client(self, outcomes, **kwargs):
        with patch.object(R.A, 'AmendedClient', R.RecurringClient):
            return T.AmendmentTests.client(self, outcomes, **kwargs)

    def revisit(self, client, peer):
        addr = T.ADDR + 1
        client.add_breakpoint(addr)
        peer.overrides['m1002,10'] = T.BYTES.encode()
        original = T.witness
        return addr, patch.object(T, 'witness',
            lambda flag=3, stop='T05thread:01;': original(flag, stop).replace(b'1000', b'1001'))

    def assert_terminal(self, client, peer):
        before = list(peer.commands)
        with self.assertRaises(R.X.PolicyError):
            client.resume()
        with self.assertRaises(R.X.PolicyError):
            client.step_over_breakpoint(T.ADDR)
        with self.assertRaises(R.X.PolicyError):
            client.request('c')
        self.assertEqual(peer.commands, before)
        self.assertTrue(client.failed)

    def test_normal_success_needs_no_extra(self):
        client, peer, _ = self.client([True])
        self.assertEqual(client.step_over_breakpoint(T.ADDR), b'T05thread:01;')
        self.assertEqual(peer.commands.count('s'), 1)
        self.assertEqual(client.total_extra_native_packets, 0)
        self.assertFalse(client.failed)
        self.assertEqual(client.decisions, [])

    def test_successful_extra_returns_only_after_full_capture_and_can_resume(self):
        client, peer, control = self.client([False, True])
        self.assertEqual(client.step_over_breakpoint(T.ADDR), b'T05thread:01;')
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertFalse(client.failed)
        self.assertNotIn('c', peer.commands)
        decision = client.decisions[0]
        self.assertEqual(decision['outcome'], 'ADDITIONAL_ADVANCED_CONTINUE')
        self.assertEqual(decision['extra_steps_used'], 1)
        self.assertTrue(decision['proof']['recurrence_authorized'])
        self.assertNotIn('max_additional_steps_globally', decision['proof'])
        self.assertEqual(decision['proof']['max_additional_steps_per_event'], 1)
        self.assertTrue(all(step['breakpoints_before'] == step['breakpoints_after'] and
                            step['watches_before'] == step['watches_after'] for step in control.steps))
        self.assertIn(R.X.NO_PROGRESS, control.steps[0]['error'])
        client.resume()
        self.assertEqual(peer.commands[-1], 'c')

    def test_two_independent_events_each_receive_exactly_one_extra(self):
        client, peer, _ = self.client([False, True, False, True])
        client.step_over_breakpoint(T.ADDR)
        addr, changed_site = self.revisit(client, peer)
        with changed_site:
            client.step_over_breakpoint(addr)
        self.assertEqual(peer.commands.count('s'), 4)
        self.assertEqual(client.total_extra_native_packets, 2)
        self.assertEqual([d['first_ordinal'] for d in client.decisions], [1, 3])
        self.assertEqual([d['additional_ordinal'] for d in client.decisions], [2, 4])
        self.assertTrue(all(d['extra_steps_used'] == 1 and d['terminal_latch_cleared']
                            for d in client.decisions))
        self.assertFalse(client.failed)

    def test_later_second_no_progress_is_terminal_no_fifth_packet(self):
        client, peer, _ = self.client([False, True, False, False])
        client.step_over_breakpoint(T.ADDR)
        addr, changed_site = self.revisit(client, peer)
        with changed_site, self.assertRaisesRegex(R.A.BoundaryStop, 'ADDITIONAL_FAILED_TERMINAL_STOP'):
            client.step_over_breakpoint(addr)
        self.assertEqual(peer.commands.count('s'), 4)
        self.assertEqual(client.total_extra_native_packets, 2)
        self.assertEqual(client.decisions[-1]['extra_steps_used'], 1)
        self.assertIn((addr, 1), client._hardware_breakpoints)
        self.assert_terminal(client, peer)

    def test_first_second_no_progress_is_terminal_no_third_packet(self):
        client, peer, _ = self.client([False, False])
        with self.assertRaisesRegex(R.A.BoundaryStop, 'ADDITIONAL_FAILED_TERMINAL_STOP'):
            client.step_over_breakpoint(T.ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertIn((T.ADDR, 1), client._hardware_breakpoints)
        self.assert_terminal(client, peer)

    def test_later_event_requires_its_own_health_proof(self):
        client, peer, control = self.client([False, True, False])
        client.step_over_breakpoint(T.ADDR)
        addr, changed_site = self.revisit(client, peer)
        control.retained_health = {}
        with changed_site, self.assertRaisesRegex(R.A.BoundaryStop, 'PROOF_REJECTED_TERMINAL_STOP'):
            client.step_over_breakpoint(addr)
        self.assertEqual(peer.commands.count('s'), 3)
        self.assertEqual(client.total_extra_native_packets, 1)
        self.assertEqual(client.decisions[-1]['extra_steps_used'], 0)
        self.assert_terminal(client, peer)

    def test_later_event_census_drift_stops_before_extra(self):
        client, peer, control = self.client([False, True, False])
        client.step_over_breakpoint(T.ADDR)
        addr, changed_site = self.revisit(client, peer)
        original = control.end
        def changed(offset):
            result = original(offset)
            client.observer.watches.clear()
            return result
        with changed_site, patch.object(control, 'end', changed):
            with self.assertRaisesRegex(R.A.BoundaryStop, 'PROOF_REJECTED_TERMINAL_STOP'):
                client.step_over_breakpoint(addr)
        self.assertEqual(peer.commands.count('s'), 3)
        self.assert_terminal(client, peer)

    def test_unexpected_additional_watch_stop_terminal_despite_advancement(self):
        client, peer, _ = self.client([False, True], second_stop='T05thread:01;watch:3000;')
        with self.assertRaisesRegex(R.A.BoundaryStop, 'ADDITIONAL_FAILED_TERMINAL_STOP'):
            client.step_over_breakpoint(T.ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assertNotIn('z2,3000,8', peer.commands)
        self.assert_terminal(client, peer)

    def test_missing_witness_never_spends_extra(self):
        for kwargs in ({'healthy': False}, {'first_raw': T.witness(0)}):
            client, peer, _ = self.client([False], **kwargs)
            with self.subTest(kwargs=kwargs), self.assertRaisesRegex(R.A.BoundaryStop, 'PROOF_REJECTED'):
                client.step_over_breakpoint(T.ADDR)
            self.assertEqual(peer.commands.count('s'), 1)
            self.assertEqual(client.total_extra_native_packets, 0)
            self.assert_terminal(client, peer)

    def test_fresh_context_and_lifecycle_recheck_prevents_extra(self):
        for field in ('context', 'lifecycle'):
            client, peer, _ = self.client([False])
            original = client._additional_once
            def changed(addr, first, context, qmp, lifecycle):
                if field == 'context':
                    context = copy.deepcopy(context)
                    context['rsp'] += 8
                else:
                    lifecycle = copy.deepcopy(lifecycle)
                    lifecycle['phase'] = 'changed'
                return original(addr, first, context, qmp, lifecycle)
            with self.subTest(field=field), patch.object(client, '_additional_once', changed):
                with self.assertRaisesRegex(R.A.BoundaryStop, 'ADDITIONAL_FAILED_TERMINAL_STOP'):
                    client.step_over_breakpoint(T.ADDR)
            self.assertEqual(peer.commands.count('s'), 1)
            self.assert_terminal(client, peer)

    def test_expected_empty_boot_can_recover_with_entry_coverage(self):
        client, peer, _ = self.client([False, True], empty=True)
        client.step_over_breakpoint(T.ADDR)
        self.assertTrue(client.decisions[0]['proof']['empty_watch_expected'])
        self.assertEqual(client.decisions[0]['proof']['lifecycle']['verified_instance_count'], 0)
        self.assertFalse(any(command.startswith('z2') for command in peer.commands))
        self.assertFalse(client.failed)

    def test_missing_additional_trace_ack_cannot_clear_terminal_latch(self):
        client, peer, control = self.client([False, True])
        original = control.end
        def missing(offset):
            result = original(offset)
            if len(control.steps) == 2:
                result['enabled_before_after_acknowledged'] = False
            return result
        with patch.object(control, 'end', missing):
            with self.assertRaisesRegex(R.A.BoundaryStop, 'ADDITIONAL_FAILED_TERMINAL_STOP'):
                client.step_over_breakpoint(T.ADDR)
        self.assertEqual(peer.commands.count('s'), 2)
        self.assert_terminal(client, peer)

    def test_packet_allowance_cannot_issue_second_extra_for_same_event(self):
        client, peer, _ = self.client([False, True])
        client.step_over_breakpoint(T.ADDR)
        client.failed = client.extra_used = client._additional_active = True
        before = list(peer.commands)
        with self.assertRaisesRegex(RuntimeError, 'per-event single additional packet'):
            client._packet('s', client._deadline())
        self.assertEqual(peer.commands, before)
        self.assertEqual(client.total_extra_native_packets, 1)

    def test_raw_step_writes_Z0_and_mode_setters_remain_denied(self):
        T.AmendmentTests.test_packet_policy_still_rejects_raw_step_writes_Z0_and_mode_setters(self)

    def test_unenrolled_step_never_sends_native_packet(self):
        T.AmendmentTests.test_enrollment_guard_rejects_unenrolled_site_before_any_native_packet(self)

    def test_original_budget_failure_prevents_resume(self):
        client, peer, control = self.client([True])
        before = list(peer.commands)
        with patch.object(control, 'check_budget', side_effect=RuntimeError('original900s expired')):
            with self.assertRaisesRegex(RuntimeError, 'original900s expired'):
                client.resume()
        self.assertEqual(peer.commands, before)


if __name__ == '__main__':
    unittest.main()
