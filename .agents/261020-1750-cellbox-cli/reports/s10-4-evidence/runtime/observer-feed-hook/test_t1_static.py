"""T1 static ELF identity: archived facts are re-derived live from the immutable candidate Terminal."""
import json
from pathlib import Path
import re
import sys
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import feed_static as FS

ROUTE = HERE.parent / 'observer-output-route-analysis'
SAVED = json.loads((HERE / 'static-identity.json').read_text())


def normalized(value):
    return json.loads(json.dumps(value))


class StaticIdentity(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fresh = normalized(FS.derive(write=False))

    def test_archived_static_identity_equals_live_rederivation(self):
        saved = dict(SAVED)
        fresh = dict(self.fresh)
        saved.pop('receipts')
        fresh.pop('receipts')
        self.assertEqual(saved, fresh)

    def test_elf_identity_and_load_layout(self):
        terminal = self.fresh['terminal']
        self.assertEqual(terminal['sha256'], FS.TERMINAL_SHA)
        self.assertEqual(FS.sha(terminal['path']), FS.TERMINAL_SHA)
        self.assertEqual(terminal['size'], 950600)
        self.assertEqual(terminal['etype'], 3)
        self.assertEqual(terminal['loads'][0]['va'], 0)
        self.assertEqual(terminal['text_load_flags'], 5)
        self.assertTrue(self.fresh['entry_fingerprints_differ'])
        self.assertEqual(self.fresh['terminal_first48_hex'],
                         '4883e4f0488d1d05a40d004c8d25fea30d004c39e37408ff134883c308ebf3e86cee010048c7c03c0000004831ff0f05')

    def test_feed_symbol_prologue_and_single_ret(self):
        feed = self.fresh['feed']
        self.assertEqual((feed['va'], feed['symbol']), (0x1da80, FS.FEED_SYMBOL))
        self.assertEqual(feed['first_instruction'], 'push   %rbp')
        self.assertEqual(feed['first_instruction_bytes_hex'], '55')
        self.assertEqual(feed['ret_addresses'], [0x1e5f3])
        self.assertEqual([a for a, _t in feed['panic_calls']], [0x1e600, 0x1e60c])
        self.assertTrue(all('panic_bounds_check' in t for _a, t in feed['panic_calls']))
        self.assertEqual(feed['register_prologue'][4:10], [
            'push   %r12', 'push   %rbx', 'sub    $0xd8,%rsp', 'mov    %rcx,%rbx', 'mov    %rdx,%r14', 'mov    %rdi,%r12'])

    def test_exactly_four_callers_inside_drain_and_no_other_reference(self):
        callers = self.fresh['callers']
        self.assertEqual(callers['call_count'], 4)
        self.assertEqual([c['address'] for c in callers['all_direct_references']], list(FS.CALL_SITES))
        self.assertTrue(all(c['kind'] == 'call' for c in callers['all_direct_references']))
        self.assertTrue(callers['all_inside_drain'])
        self.assertEqual(callers['return_offsets'], sorted(FS.RET_SITES))
        self.assertEqual(callers['pointer_or_lea_references'], [])
        self.assertEqual(callers['relocation_references'], [])
        feed_text = (HERE / 'fixtures/terminal-feed-full.asm').read_text()
        self.assertNotRegex(feed_text, rf'(call|jmp)\s+{FS.FEED_VA:x} ')

    def test_call_site_prelude_proves_register_contract(self):
        for site in self.fresh['call_site_prelude']:
            with self.subTest(call=site['call_address']):
                self.assertTrue(site['rdi_from_r14'] and site['rsi_from_rbx'])
                self.assertIn(site['rdx_move'], ('mov    %r13,%rdx', 'mov    %r12,%rdx'))
                self.assertTrue(site['rdx_equals_drain_rsp_plus_0x18'])
                self.assertTrue(site['length_bound_checked_before_call'])
                self.assertTrue(site['rcx_not_modified_between_zero_test_and_call'])
                self.assertEqual(site['compare_instruction'], 'cmp    $0x200,%rcx')
        drain = self.fresh['drain']
        self.assertEqual(drain['frame_adjust'], 'sub    $0x218,%rsp')
        # The only rsp writes are the frame setup/teardown pair and an unrelated inner pair after the feed calls.
        self.assertEqual(drain['rsp_writes'][0], [0x4a, 'sub    $0x218,%rsp'])
        self.assertEqual(self.fresh['entry_invariants'], {
            'rdx_equals_rsp_plus': 0x20, 'rcx_min': 1, 'rcx_max': 0x200,
            'derivation': 'drain_rsp+0x18 buffer; call pushes 8 bytes => RDX==RSP_entry+0x20'})

    def test_stream_mapping_is_program_order_proof_not_field_layout(self):
        mapping = self.fresh['stream_mapping']
        self.assertEqual(mapping['return_offset_to_stream_stage'], {'0x14b': 'first', '0x1a5': 'first', '0x1e5': 'second', '0x22d': 'second'})
        evidence = mapping['asm_evidence']
        self.assertEqual(evidence['first_endpoint_lea'], 'lea    0xc8(%r15),%rsi')
        self.assertEqual(evidence['second_endpoint_adjust'], 'add    $0xe8,%r15')
        self.assertEqual(evidence['loop_counters'], ['mov    $0x1f,%r12d', 'mov    $0x1f,%r13d'])
        drain = FS.parse_asm((HERE / 'fixtures/terminal-drain-full.asm').read_text())
        targets = {}
        for address, text in drain:
            match = re.match(r'(?:j[a-z]+|jmp)\s+([0-9a-f]+) ', text)
            if match:
                targets.setdefault(int(match[1], 16), []).append(address)
        # Stage 2 (host+0xe8) is entered only through 0xd1, which only stage-1 exits reach.
        self.assertEqual(sorted(targets[0xd1]), [0x191, 0x1aa, 0x258])
        self.assertEqual(targets[0xcf], [0x136])
        self.assertEqual([t for a, t in drain if a in (0xcf, 0xd1)], ['xor    %ebp,%ebp', 'add    $0xe8,%r15'])
        self.assertFalse(any(0xd1 < t <= 0xe5 for t in targets))
        order = [a for a, t in drain if t.startswith('call') and '26e60' in t]
        self.assertEqual(order[:2], [0xae, 0xe5])
        self.assertIn('stdout first', 'stdout first (session.rs for stderr in [false,true])')
        source = (HERE.parents[5] / 'cells/apps/terminal/src/session.rs').read_text()
        self.assertRegex(source, r'for stderr in \[false, true\]')
        self.assertIn('DRAIN_FRAMES: usize = 32', source)

    def test_windows_are_relocation_free_and_match_elf_bytes(self):
        self.assertTrue(self.fresh['window_relocation_free'])
        data = Path(self.fresh['terminal']['path']).read_bytes()
        labels = [w['label'] for w in self.fresh['windows']]
        self.assertEqual(labels[:3], ['entry', 'feed-prologue', 'feed-ret-epilogue'])
        self.assertEqual(len(labels), 7)
        for window in self.fresh['windows']:
            offset = 4096 + window['va']
            self.assertEqual(data[offset:offset + window['length']].hex(), window['hex'])
        self.assertEqual(self.fresh['windows'][1]['hex'][:2], '55')

    def test_retained_assembly_matches_fresh_disassembly(self):
        retained_drain = FS.parse_asm((ROUTE / 'terminal-drain.asm').read_text())
        fresh_drain = FS.parse_asm((HERE / 'fixtures/terminal-drain-full.asm').read_text())
        self.assertEqual(retained_drain, [row for row in fresh_drain if row[0] <= retained_drain[-1][0]])
        retained_feed = FS.parse_asm((ROUTE / 'terminal-feed-entry.asm').read_text())
        fresh_feed = FS.parse_asm((HERE / 'fixtures/terminal-feed-full.asm').read_text())
        # The retained excerpt stops mid-instruction at 0x1db1f (a truncated, mis-decoded final row).
        self.assertEqual(retained_feed[:-1], fresh_feed[:len(retained_feed) - 1])
        self.assertEqual(retained_feed[-1][0], 0x1db1f)

    def test_fixture_receipts_describe_read_only_commands_only(self):
        for receipt in SAVED['receipts']:
            self.assertIn(receipt['argv'][0], ('objdump',))
            self.assertNotIn('-w', receipt['argv'])


if __name__ == '__main__':
    unittest.main()
