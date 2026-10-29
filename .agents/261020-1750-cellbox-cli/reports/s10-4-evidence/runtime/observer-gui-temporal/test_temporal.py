"""Pure ASCII-grid temporal-oracle contracts; no VM, renderer, clock or input I/O."""
from copy import deepcopy
import json
from pathlib import Path
import unittest

from temporal import OracleIncomplete, TemporalLedger


COLS, ROWS = 79, 26
PROMPT = 'Cellos > '
COMMAND = 'top --records -d 1 --frames 2 | to jsonl | cat'
STATUS_COMMAND = 'echo st=$?'
FIRST_NS = 1_000_000_001
RETAINED = (Path(__file__).resolve().parent.parent /
            'observer-recurring/headless/runtime-result.json')


def retained_lines():
    """The unchanged actual two-frame fixture used by retained prove_scroll.py."""
    evidence = json.loads(RETAINED.read_text())
    event = next(item for item in evidence['surface_events']
                 if item.get('name') == 'finite-stream')
    if event['command'] != COMMAND:
        raise AssertionError('retained finite command changed')
    return list(event['rows'])


def encode(records):
    return [json.dumps(record, ensure_ascii=True, separators=(',', ':'))
            for record in records]


def enter_commit(batch_id=17, finish=1.0, events=None):
    if events is None:
        events = [{'type': 'key', 'data': {
            'down': down, 'key': {'type': 'qcode', 'data': 'ret'}}}
            for down in (True, False)]
    request = {'execute': 'input-send-event', 'arguments': {'events': events}}
    payload = json.dumps(request, allow_nan=False).encode('ascii') + b'\n'
    return {'id': batch_id, 'payload_hex': payload.hex(),
            'outcome': 'COMMITTED', 'commit_count': 1, 'finish': finish}


class ASCIIGrid:
    """Independent printable-ASCII model of delayed autowrap, LF and scrolling.

    A full physical row leaves x=78 and wrap pending, as TerminalGrid does;
    LF cancels pending wrap rather than introducing an extra empty row.
    Captures have exactly Desk.read_screen's trimmed strings and cursor shape.
    """
    def __init__(self, command=COMMAND, history=None):
        self.rows = [' ' * COLS for _ in range(ROWS)]
        self.x = self.y = self.scrolls = 0
        self.wrap_pending = False
        if history is None:
            history = [f'retained anchor {index:02d}' for index in range(25)]
        self.feed(''.join(row + '\n' for row in history) + PROMPT + command)

    def newline(self):
        self.x = 0
        self.wrap_pending = False
        if self.y == ROWS - 1:
            self.rows.pop(0)
            self.rows.append(' ' * COLS)
            self.scrolls += 1
        else:
            self.y += 1

    def feed(self, text):
        for char in text:
            if char == '\n':
                self.newline()
                continue
            if not (' ' <= char <= '~'):
                raise AssertionError('fixture must be width-one printable ASCII/LF')
            if self.wrap_pending:
                self.newline()
            row = self.rows[self.y]
            self.rows[self.y] = row[:self.x] + char + row[self.x + 1:]
            if self.x == COLS - 1:
                self.wrap_pending = True
            else:
                self.x += 1

    def capture(self):
        rows = [row.rstrip(' ') for row in self.rows]
        while rows and not rows[-1]:
            rows.pop()
        return rows, (self.x, self.y)


def snapshots(grid, lines, *, chunk=17, prompt=True):
    """Capture frequently enough to retain every physical-row transition."""
    text = '\n' + '\n'.join(lines) + ('\n' if lines else '')
    captures = []
    for offset in range(0, len(text), chunk):
        grid.feed(text[offset:offset + chunk])
        captures.append(grid.capture())
    if prompt:
        grid.feed(PROMPT)
        captures.append(grid.capture())
    return captures


def ledger_for(grid, command=COMMAND, *, kind='finite', arm=True, commit=None):
    rows, cursor = grid.capture()
    ledger = TemporalLedger(COLS, ROWS, rows, cursor, command, kind=kind,
                            expected_status=0)
    if arm:
        ledger.arm(enter_commit() if commit is None else commit)
    return ledger


class TemporalLedgerTests(unittest.TestCase):
    def replay(self, ledger, captures, *, start=FIRST_NS, repeated=False):
        results = []
        for index, (rows, cursor) in enumerate(captures):
            stamp = start + index * 2
            results.append(ledger.observe(rows, cursor, stamp))
            if repeated:
                results.append(ledger.observe(rows, cursor, stamp + 1))
        return results

    def assert_unproved(self, ledger, captures, *, start=FIRST_NS):
        """Either terminal incompleteness or continued waiting must stay false."""
        try:
            for index, (rows, cursor) in enumerate(captures):
                self.assertFalse(ledger.observe(rows, cursor, start + index),
                                 'missing evidence must never become completion')
        except OracleIncomplete:
            pass
        self.assertIs(ledger.snapshot()['complete'], False)

    def assert_valid_lines(self, lines, *, chunk=17):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, lines, chunk=chunk)
        results = self.replay(ledger, captures)
        self.assertTrue(results[-1])
        self.assertFalse(any(results[:-1]), 'new prompt must follow both actual frames')
        self.assertIs(ledger.snapshot()['complete'], True)
        self.assertEqual(ledger.snapshot()['records'], [json.loads(line) for line in lines])
        return grid, ledger, captures

    def assert_invalid_records(self, records):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, encode(records)))

    def test_full_retained_two_frame_62_physical_row_trace_then_own_status_epoch(self):
        lines = retained_lines()
        records = [json.loads(line) for line in lines]
        self.assertEqual(len(lines), 20)
        self.assertEqual(sum((len(line) + COLS - 1) // COLS for line in lines), 62)
        self.assertEqual([item['sample_id'] for item in records
                          if item['frame'] == 'begin'], [2, 3])
        grid, finite, captures = self.assert_valid_lines(lines)
        self.assertGreater(grid.scrolls, ROWS)
        self.assertNotIn(PROMPT + COMMAND, captures[-1][0])
        self.assertEqual(captures[-1][0][-1], PROMPT.rstrip())
        self.assertEqual(captures[-1][1], (len(PROMPT), ROWS - 1))
        self.assertEqual(len(finite.snapshot()['frames']), 2)

        grid.feed(STATUS_COMMAND)
        status_commit = enter_commit(batch_id=18, finish=2.0)
        status = ledger_for(grid, STATUS_COMMAND, kind='status', commit=status_commit)
        self.assertIs(status.snapshot()['complete'], False)
        status_results = self.replay(status, snapshots(grid, ['st=0']),
                                     start=2_000_000_001)
        self.assertFalse(any(status_results[:-1]))
        self.assertTrue(status_results[-1])
        self.assertNotEqual(finite.snapshot()['commit']['id'],
                            status.snapshot()['commit']['id'])
        self.assertEqual(status.snapshot()['commit']['id'], 18)

    def test_frequent_partial_rows_and_repeated_captures_do_not_duplicate_records(self):
        lines = retained_lines()
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        results = self.replay(ledger, snapshots(grid, lines, chunk=7), repeated=True)
        self.assertTrue(results[-1])
        self.assertEqual(ledger.snapshot()['records'], [json.loads(line) for line in lines])
        self.assertEqual(len(ledger.snapshot()['frames']), 2)

    def test_complete_output_without_new_prompt_is_not_completion(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, retained_lines(), prompt=False))

    def test_unarmed_even_complete_ordered_trace_never_accepts(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid, arm=False)
        self.assert_unproved(ledger, snapshots(grid, retained_lines()))

    def test_precommit_timestamp_does_not_attribute_complete_output(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, retained_lines()), start=1)

    def test_identical_typed_snapshot_never_appends_or_accepts(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, [grid.capture()] * 30)
        self.assertEqual(ledger.snapshot()['records'], [])

    def test_stale_old_prompt_screen_never_accepts(self):
        stale = ASCIIGrid(command='').capture()
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, [stale, stale])

    def test_fresh_bare_prompt_has_no_two_frame_evidence(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, []))

    def test_single_coalesced_62_row_jump_is_observability_incomplete(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, retained_lines())
        self.assert_unproved(ledger, [captures[-1]])

    def test_dropped_middle_captures_cannot_reconstruct_hidden_records(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, retained_lines(), chunk=17)
        self.assert_unproved(ledger, captures[:5] + captures[-5:])

    def test_only_second_frame_and_prompt_do_not_prove_first(self):
        records = [json.loads(line) for line in retained_lines()]
        self.assert_invalid_records(records[10:])

    def test_one_frame_with_command_frames_two_is_still_incomplete(self):
        self.assert_invalid_records([json.loads(line) for line in retained_lines()[:10]])

    def test_duplicate_complete_frame_is_not_two_distinct_samples(self):
        records = [json.loads(line) for line in retained_lines()[:10]]
        self.assert_invalid_records(records + deepcopy(records))

    def test_second_frame_reusing_sample_identity_rejects(self):
        records = [json.loads(line) for line in retained_lines()]
        records[10]['sample_id'] = records[0]['sample_id']
        self.assert_invalid_records(records)

    def test_missing_row_is_detected_by_actual_total_cells(self):
        records = [json.loads(line) for line in retained_lines()]
        del records[4]
        self.assert_invalid_records(records)

    def test_duplicate_cell_generation_row_cannot_replace_missing_row(self):
        records = [json.loads(line) for line in retained_lines()]
        records[4] = deepcopy(records[3])
        self.assert_invalid_records(records)

    def test_missing_end_and_out_of_order_begin_row_end_reject(self):
        original = [json.loads(line) for line in retained_lines()]
        variants = [original[:9] + original[10:], original[1:2] + original,
                    original[:9] + [original[10]] + original[9:10] + original[11:],
                    original + [deepcopy(original[-1])]]
        for records in variants:
            with self.subTest(order=[item['frame'] for item in records]):
                self.assert_invalid_records(records)

    def test_incomplete_or_dropped_end_is_not_success(self):
        for change in ({'complete': False}, {'dropped': 1},
                       {'complete': 1}, {'dropped': False}):
            with self.subTest(change=change):
                records = [json.loads(line) for line in retained_lines()]
                records[-1].update(change)
                self.assert_invalid_records(records)

    def test_malformed_json_even_with_final_prompt_cannot_accept(self):
        for bad in ('{not-json}', '{"frame":"begin",}', 'null', '[]'):
            with self.subTest(bad=bad):
                lines = retained_lines()
                lines[3] = bad
                grid = ASCIIGrid()
                ledger = ledger_for(grid)
                self.assert_unproved(ledger, snapshots(grid, lines))

    def test_required_begin_row_end_fields_cannot_be_inferred(self):
        for index, field in ((0, 'sample_id'), (1, 'value'),
                             (19, 'complete'), (19, 'dropped')):
            with self.subTest(index=index, field=field):
                records = [json.loads(line) for line in retained_lines()]
                del records[index][field]
                self.assert_invalid_records(records)

    def test_wrong_cursor_rejects_even_when_visible_final_prompt_is_exact(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, retained_lines())
        self.assertFalse(any(self.replay(ledger, captures[:-1])))
        rows, (x, y) = captures[-1]
        self.assert_unproved(ledger, [(rows, (x - 1, y))],
                             start=FIRST_NS + len(captures) * 2)

    def test_cursor_retreat_without_scroll_cannot_append(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        rows, (x, y) = grid.capture()
        self.assert_unproved(ledger, [(rows, (x - 1, y))])

    def test_rewriting_a_stable_row_is_unknown_alignment_not_output(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        grid.feed('\n{"frame":"begin"')
        rows, cursor = grid.capture()
        rows[0] = 'unrelated replacement desktop'
        self.assert_unproved(ledger, [(rows, cursor)])

    def test_repeated_row_alignment_with_multiple_valid_scrolls_is_unproved(self):
        # This is valid JSON, but a long repeated string loses the unique row
        # anchors. Several shifts below26 fit the same observed physical rows.
        records = [json.loads(line) for line in retained_lines()]
        records[1]['value']['name'] = 'R' * (COLS * 40)
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, encode(records), prompt=False)
        with self.assertRaises(OracleIncomplete):
            self.replay(ledger, captures)
        self.assertIs(ledger.snapshot()['complete'], False)

    def test_unreadable_capture_recovers_only_with_retained_unique_overlap(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, retained_lines())
        split = 10
        self.assertFalse(any(self.replay(ledger, captures[:split])))
        self.assertFalse(ledger.observe(None, None, FIRST_NS + split * 2))
        results = self.replay(ledger, captures[split:], start=FIRST_NS + split * 2 + 1)
        self.assertTrue(results[-1])
        self.assertEqual(ledger.snapshot()['records'],
                         [json.loads(line) for line in retained_lines()])

    def test_unreadable_then_nonoverlapping_jump_never_recovers_missing_data(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        captures = snapshots(grid, retained_lines())
        self.assertFalse(ledger.observe(None, None, FIRST_NS))
        self.assert_unproved(ledger, [captures[-1]], start=FIRST_NS + 1)

    def test_exact_multiple_of_79_record_boundaries_preserve_json_identity(self):
        records = [json.loads(line) for line in retained_lines()]
        for index, width in ((0, COLS * 6), (1, COLS * 4), (10, COLS * 7)):
            records[index]['boundary_padding'] = ''
            current = len(encode([records[index]])[0])
            self.assertLessEqual(current, width)
            records[index]['boundary_padding'] = 'X' * (width - current)
        lines = encode(records)
        self.assertEqual([len(lines[index]) % COLS for index in (0, 1, 10)], [0, 0, 0])
        self.assert_valid_lines(lines, chunk=COLS)

    def test_narrow_79_column_boundaries_preserve_end_and_next_begin(self):
        # At exactly79 or158, source LF and softwrap are visually inseparable;
        # raw_decode must still preserve both adjacent complete JSON objects.
        for width in (78, 79, 80, 157, 158, 159):
            with self.subTest(width=width):
                records = [json.loads(line) for line in retained_lines()]
                end = records[9]
                end['pad'] = ''
                padding = width - len(encode([end])[0])
                self.assertGreaterEqual(padding, 0)
                end['pad'] = 'E' * padding
                self.assertEqual(len(encode([end])[0]), width)
                self.assert_valid_lines(encode(records), chunk=13)

    def dynamic_records(self, *, repeated_note=False, copied_rows=False):
        original = [json.loads(line) for line in retained_lines()]
        records = []
        for start, count in ((0, 3), (10, 11)):
            begin = deepcopy(original[start])
            begin['meta']['total_cells'] = count
            note = ('long-metadata-' * 41 if repeated_note else
                    ''.join(f'frame{start}-segment{part:03d}-' for part in range(40)))
            begin['meta']['arbitrary_retained_extension'] = {'note': note, 'optional': None}
            records.append(begin)
            for index in range(count):
                row = deepcopy(original[start + 1 + index % 8])
                row['value'].update(cell_id=100 + index, generation=1000 + index)
                if not copied_rows:
                    row['value'].update(cpu_ns=8_000_000_000 + index * 1_000_000,
                                        cpu_tenths=1000 + index)
                records.append(row)
            records.append(deepcopy(original[start + 9]))
        return records

    def test_dynamic_per_frame_counts_and_long_metadata_are_not_fixed_eight(self):
        self.assert_valid_lines(encode(self.dynamic_records()), chunk=23)

    def test_repeated_long_metadata_with_two_plausible_scrolls_fails_closed(self):
        # The originally failing fixture genuinely admits shifts1 and22; it
        # cannot serve as a success oracle without violating unique coverage.
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, encode(self.dynamic_records(repeated_note=True, copied_rows=True)), chunk=23))
        self.assertIn('ambiguous row overlap', ledger.snapshot()['reasons'])

    def test_copied_row_bodies_with_two_plausible_scrolls_fail_closed(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, encode(self.dynamic_records(copied_rows=True)), chunk=23))
        self.assertIn('ambiguous row overlap', ledger.snapshot()['reasons'])

    def test_dynamic_count_mismatch_rejects_even_when_both_frames_have_rows(self):
        records = [json.loads(line) for line in retained_lines()]
        records[10]['meta']['total_cells'] = 9
        self.assert_invalid_records(records)

    def test_status_needs_own_exact_zero_and_subsequent_prompt(self):
        for lines in ([], ['st=1'], ['st=00'], ['xst=0'], ['st=0 extra'],
                      ['st=0', 'st=1']):
            with self.subTest(lines=lines):
                grid = ASCIIGrid(STATUS_COMMAND)
                ledger = ledger_for(grid, STATUS_COMMAND, kind='status')
                self.assert_unproved(ledger, snapshots(grid, lines))
        grid = ASCIIGrid(STATUS_COMMAND)
        ledger = ledger_for(grid, STATUS_COMMAND, kind='status')
        self.assert_unproved(ledger, snapshots(grid, ['st=0'], prompt=False))

    def test_status_zero_in_prior_history_is_not_own_epoch_output(self):
        grid = ASCIIGrid(STATUS_COMMAND, history=['st=0'] +
                         [f'old history {index:02d}' for index in range(24)])
        ledger = ledger_for(grid, STATUS_COMMAND, kind='status')
        self.assert_unproved(ledger, snapshots(grid, []))

    def test_status_zero_and_prompt_do_not_replace_finite_frame_evidence(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        self.assert_unproved(ledger, snapshots(grid, ['st=0']))

    def test_wrong_typed_command_cursor_and_wrapped_command_cannot_arm(self):
        grid = ASCIIGrid()
        rows, cursor = grid.capture()
        bad_rows = list(rows)
        bad_rows[-1] += 'x'
        for actual_rows, actual_cursor, command in (
                (bad_rows, cursor, COMMAND),
                (rows, (cursor[0] - 1, cursor[1]), COMMAND),
                (rows, cursor, COMMAND + 'x'),
                (rows, cursor, 'x' * COLS)):
            with self.subTest(cursor=actual_cursor, command=command):
                with self.assertRaises(OracleIncomplete):
                    TemporalLedger(COLS, ROWS, actual_rows, actual_cursor, command)

    def test_non_ascii_or_overwide_typed_rows_are_not_exact_screen_evidence(self):
        grid = ASCIIGrid()
        rows, cursor = grid.capture()
        for replacement in ('non-ASCII \u00e9', 'X' * (COLS + 1)):
            with self.subTest(replacement=replacement):
                bad = list(rows)
                bad[0] = replacement
                with self.assertRaises(OracleIncomplete):
                    TemporalLedger(COLS, ROWS, bad, cursor, COMMAND)

    def test_commit_must_prove_exact_single_enter_not_other_input(self):
        variants = []
        for changes in ({'outcome': 'PENDING'}, {'outcome': 'TERMINAL'},
                        {'commit_count': 0}, {'commit_count': 2},
                        {'finish': None}, {'id': None}, {'payload_hex': 'zz'}):
            commit = enter_commit()
            commit.update(changes)
            variants.append(commit)
        valid_events = json.loads(bytes.fromhex(enter_commit()['payload_hex']))['arguments']['events']
        for events in ([], valid_events[:1], list(reversed(valid_events)),
                       valid_events * 2,
                       valid_events + [{'type': 'btn', 'data': {
                           'down': True, 'button': 'left'}}]):
            variants.append(enter_commit(events=events))
        other = deepcopy(valid_events)
        other[0]['data']['key']['data'] = 'a'
        variants.append(enter_commit(events=other))
        for commit in variants:
            with self.subTest(commit=commit):
                grid = ASCIIGrid()
                ledger = ledger_for(grid, arm=False)
                with self.assertRaises(OracleIncomplete):
                    ledger.arm(commit)
                self.assertIs(ledger.snapshot()['complete'], False)

    def test_commit_identity_is_frozen_and_ledger_cannot_be_rearmed(self):
        grid = ASCIIGrid()
        commit = enter_commit()
        expected = deepcopy(commit)
        ledger = ledger_for(grid, commit=commit)
        commit.update(id=999, payload_hex='mutated', finish=999)
        for key, value in expected.items():
            self.assertEqual(ledger.snapshot()['commit'][key], value)
        for replacement in (expected, enter_commit(batch_id=18)):
            with self.subTest(batch_id=replacement['id']):
                with self.assertRaises(OracleIncomplete):
                    ledger.arm(replacement)

    def test_observation_timestamps_cannot_go_backwards(self):
        grid = ASCIIGrid()
        ledger = ledger_for(grid)
        rows, cursor = grid.capture()
        self.assertFalse(ledger.observe(rows, cursor, FIRST_NS + 10))
        self.assert_unproved(ledger, [(rows, cursor)], start=FIRST_NS)


if __name__ == '__main__':
    unittest.main()
