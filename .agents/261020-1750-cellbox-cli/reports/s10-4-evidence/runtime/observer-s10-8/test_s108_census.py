"""S10-8.0 offline tests: the static collision census (empty for the new classes, able to detect the S10-7 defect)."""
import json
import sys
import threading
import unittest
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / 'observer-s10-7'))
import s108_census as CENSUS  # noqa: E402


class Census(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.census = CENSUS.build_census()
        cls.registry = CENSUS.base_registry()

    def test_the_census_of_the_new_classes_is_empty(self):
        self.assertTrue(self.census['empty'])
        self.assertEqual(self.census['unintended_new'], {})
        self.assertEqual(self.census['surface_chain_unintended'], {})
        self.assertEqual(set(self.census['new_classes']), {
            's108_memory.MemoryMixin', 's108_run.MemoryHookObserver', 's108_run.MemoryPlainObserver', 's108_credit.PauseSource',
            's108_credit.CreditedWait', 's108_credit.CreditedRecoveryMixin', 's108_run.CreditedDesktopSurface', 's108_run.CreditedHeadlessSurface'})
        json.dumps(self.census)

    def test_the_only_overlaps_are_declared_super_chained_overrides(self):
        overlaps = {name: [item['name'] for item in data['intentional_overrides']] for name, data in self.census['new_classes'].items()
                    if data['intentional_overrides']}
        self.assertEqual(overlaps, {'s108_memory.MemoryMixin': ['__init__', '_entry', '_snapshot', 'write_results']})
        for data in self.census['new_classes'].values():
            self.assertTrue(all(item['chains_to_super'] for item in data['intentional_overrides']))
        chained = {name: sorted(item['name'] for item in items) for name, items in self.census['surface_chain_overrides'].items()}
        self.assertEqual(chained, {'s108_run.CreditedDesktopSurface': ['gui_fresh', 'wait'], 's108_run.CreditedHeadlessSurface': ['gui_fresh', 'wait']})
        for items in self.census['surface_chain_overrides'].values():
            self.assertTrue(all(item['chains_to_super'] and item['declared'] for item in items))

    def test_no_new_class_defines_any_thread_internal_name(self):
        thread = self.registry['threading.Thread']
        for required in ('_stop', '_started', '_is_stopped', '_tstate_lock', 'join', 'is_alive', 'name', 'daemon', 'ident'):
            self.assertIn(required, thread, required)
        for name, data in self.census['new_classes'].items():
            defined = set(data['defined']['members']) | set(data['defined']['instance_attributes'])
            self.assertFalse(defined & {n for n in thread if not n.startswith('__')} - {'run', 'start'}, name)

    def test_negative_control_the_census_reproduces_the_s10_7_defect(self):
        found = self.census['s107_collisions_found']['s107_memory.MemoryMixin']
        self.assertEqual([item['name'] for item in found], ['_stop'])
        self.assertIn('threading.Thread', found[0]['bases'])

    def test_synthetic_collisions_are_detected(self):
        class BadStop(threading.Thread):
            def _stop(self, message):
                return message

        class BadAttr:
            def go(self):
                self._started = 1

        class Check:
            def check(self):
                return 1

        class MemoryMixin:                                           # declared name, but no super() chain: not an accepted override
            def _entry(self, pc):
                return pc

        for cls in (BadStop, BadAttr, Check, MemoryMixin):
            with self.subTest(cls.__name__):
                self.assertTrue(CENSUS.census_class(cls, self.registry)['unintended'])

    def test_standalone_helper_dunder_init_is_not_an_override_but_a_mixin_init_must_chain(self):
        class Helper:
            def __init__(self):
                self.fine = 1

        result = CENSUS.census_class(Helper, self.registry)
        self.assertEqual(result['unintended'], [])
        self.assertEqual(result['standalone_helper_dunders_not_overrides'], ['__init__'])

        class MemoryMixin:
            def __init__(self):
                self.fine = 1                                        # mixin __init__ without super(): flagged

        self.assertEqual([i['name'] for i in CENSUS.census_class(MemoryMixin, self.registry)['unintended']], ['__init__'])

    def test_machine_names_are_included(self):
        self.assertIn('start_ns', self.registry['common.Machine'])
        self.assertIn('stop', self.registry['phase10_canonical_drive.Machine'])
        self.assertIn('_snapshot', self.registry['observer.Observer'])
        self.assertIn('_handle', self.registry['hook_observer.HookObserver'])
        self.assertIn('_stopped', self.registry['coordinated.CoordinatedObserver'])


if __name__ == '__main__':
    unittest.main()
