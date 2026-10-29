"""S10-9 offline tests: static name-collision census of the new classes (the S10-7 defect was `MemoryMixin._stop` vs `threading.Thread._stop`).
No VM."""
import sys
import threading
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-8'))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s109_census as CENSUS  # noqa: E402
import s109_credit as CREDIT  # noqa: E402
import s109_run as RUN9  # noqa: E402
import s108_census as C8  # noqa: E402


class CensusIsEmpty(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.census = CENSUS.build_census()

    def test_no_unintended_collision_in_any_new_class(self):
        self.assertTrue(self.census['empty'])
        self.assertEqual(self.census['unintended_new'], {})
        self.assertEqual(self.census['surface_chain_unintended'], {})

    def test_the_new_classes_are_the_expected_ones(self):
        self.assertEqual(sorted(self.census['new_classes']), ['s109_credit.LatencyRecoveryMixin', 's109_credit.LatencyWait', 's109_run.AtomicHeadlessClient',
                                                              's109_run.LatencyDesktopSurface', 's109_run.LatencyHeadlessSurface'])
        defined = self.census['new_classes']['s109_credit.LatencyRecoveryMixin']['defined']
        self.assertEqual(defined['members'], ['_credit_start'])

    def test_the_atomic_headless_client_defines_nothing_of_its_own(self):
        found = C8.own_names(RUN9.AtomicHeadlessClient)
        self.assertEqual(found['members'], set())
        self.assertEqual(found['instance'], set())

    def test_wait_factory_is_replaced_not_shadowed_elsewhere(self):
        self.assertTrue(self.census['latency_wait_factory_replaces_s108_factory_only'])
        for klass in (RUN9.LatencyDesktopSurface, RUN9.LatencyHeadlessSurface):
            self.assertEqual(CENSUS.surface_overrides(klass), [])

    def test_registry_covers_threads_observers_machines_and_the_rsp_clients(self):
        registry = CENSUS.base_registry()
        for label in ('threading.Thread', 'observer.Observer', 'coordinated.CoordinatedObserver', 'hook_observer.HookObserver', 'common.Machine',
                      'phase10_canonical_drive.Machine', 'rsp.RspClient', 'diagnostic.DiagnosticClient', 'recurring.RecurringClient', 'run_atomic.AtomicClient'):
            self.assertIn(label, registry)
        self.assertIn('_stop', registry['threading.Thread'])
        self.assertIn('_event', registry['rsp.RspClient'])
        self.assertIn('_pending_wire_frame', registry['run_atomic.AtomicClient'])

    def test_the_s10_8_classes_reused_unchanged_are_still_clean(self):
        for name, item in self.census['s108_classes_informational'].items():
            if name != 's108_credit.PauseSource':
                self.assertEqual(item['unintended'], [], name)
        self.assertEqual(sorted(self.census['s108_collisions_found']), ['s108_credit.PauseSource'])          # informational, standalone helper attribute


class CensusDetectsCollisions(unittest.TestCase):
    """Negative controls: the census must reproduce the defect class it exists to catch."""

    @classmethod
    def setUpClass(cls):
        cls.registry = CENSUS.base_registry()

    def test_a_mixin_defining_thread_stop_is_unintended(self):
        class MemoryMixin:
            def _stop(self, message):
                return message
        result = CENSUS.census_class(MemoryMixin, self.registry)
        self.assertEqual([item['name'] for item in result['unintended']], ['_stop'])
        self.assertIn('threading.Thread', result['unintended'][0]['bases'])

    def test_an_override_without_super_chain_is_unintended(self):
        class MemoryMixin:
            def _entry(self, pc):
                return pc
        result = CENSUS.census_class(MemoryMixin, self.registry)
        self.assertEqual([item['name'] for item in result['unintended']], ['_entry'])
        class MemoryMixin:                                                # same name, chained: accepted only because it is declared
            def _entry(self, pc):
                return super()._entry(pc)
        result = CENSUS.census_class(MemoryMixin, self.registry)
        self.assertEqual([item['name'] for item in result['intentional_overrides']], ['_entry'])
        self.assertEqual(result['unintended'], [])

    def test_a_client_subclass_overriding_an_rsp_method_is_unintended(self):
        class Client(RUN9.AtomicHeadlessClient):
            def _receive(self, deadline):
                return None
        result = CENSUS.census_class(Client, self.registry)
        self.assertEqual([item['name'] for item in result['unintended']], ['_receive'])

    def test_instance_attribute_collisions_are_found_by_ast(self):
        class Helper(threading.Thread):
            def __init__(self):
                super().__init__()
                self.pause_ns = 0
                self._started = None            # collides with Thread._started
        result = CENSUS.census_class(Helper, self.registry)
        self.assertIn('_started', [item['name'] for item in result['unintended']])

    def test_standalone_helper_dunder_is_not_a_collision(self):
        result = CENSUS.census_class(CREDIT.LatencyWait, self.registry)
        self.assertEqual(result['unintended'], [])
        self.assertIn('__init__', result['standalone_helper_dunders_not_overrides'])


if __name__ == '__main__':
    unittest.main()
