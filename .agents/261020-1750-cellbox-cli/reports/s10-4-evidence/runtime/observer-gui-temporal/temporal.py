"""Pure GUI evidence ledger for the source-pinned 79x26 ASCII Terminal.

This is not a terminal emulator or a command-success heuristic.  The caller
supplies exact Desk.read_screen results and the existing input gate's Enter
commit.  Only uniquely overlapping physical rows may extend the output epoch.

Source semantics: cells/apps/terminal/src/grid.rs:299-345,365-383,510-526
(printable width-one characters, delayed autowrap, LF and full-screen scroll).
No escape sequences, cursor addressing, scrollback recovery or invisible output
are reconstructed.  In particular, a whole 62-row burst missed by a 26-row
observer is missing evidence, even if its final prompt and status look correct.
"""

import copy
import json
import math


PROMPT = 'Cellos > '
# Opaque pre-output row from the source-pinned phase04/10 exact GUI oracle.
# Its em dash is one qualified renderer cell; it is not decoded command output.
KNOWN_BANNER = "=== Cellos shell ready \u2014 type 'help' for commands ==="


class OracleIncomplete(RuntimeError):
    """The observed epoch cannot establish complete, attributable GUI output."""


def _object(pairs):
    """Do not let duplicate JSON keys silently replace observed evidence."""
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key: ' + key)
        result[key] = value
    return result


def _constant(value):
    raise ValueError('non-finite JSON constant: ' + value)


_DECODER = json.JSONDecoder(object_pairs_hook=_object, parse_constant=_constant)


class TemporalLedger:
    """One typed-command/committed-Enter/observed-output epoch.

    Protocol and coverage failures are terminal OracleIncomplete exceptions;
    snapshot() remains available after them.  Unarmed observations and unreadable
    captures return False without advancing the last exact screen anchor.  A
    later exact capture may recover an unreadable gap only through the same
    unique-overlap rule.  A duplicate capture never adds rows or JSON records.
    """

    def __init__(self, cols, rows, typed_rows, typed_cursor, command, *,
                 kind='finite', expected_status=0):
        self.complete = False
        self._failed = False
        self._reasons = []
        self._commit = None
        self._commit_ns = None
        self._last_ns = None
        self._unreadable = False
        self._captures = []
        self._records = []
        self._frames = []
        self._transcript = ''
        self._physical = {}
        self._window_start = 0
        self._prompt_row = None
        self.cols, self.height = cols, rows
        self.kind, self.command = kind, command
        self.expected_status = expected_status
        if type(cols) is not int or type(rows) is not int or (cols, rows) != (79, 26):
            self._fail('unsupported geometry: source-pinned grid is 79x26')
        if kind not in ('finite', 'status'):
            self._fail('unknown output epoch kind')
        if (not isinstance(command, str) or not command or
                any(not 32 <= ord(char) <= 126 for char in command)):
            self._fail('typed command must be nonempty width-one ASCII')
        if type(expected_status) is not int:
            self._fail('expected status must be an integer')
        if kind == 'status' and command != 'echo st=$?':
            self._fail('status must have its own original echo st=$? phase')
        if kind == 'finite' and command != 'top --records -d 1 --frames 2 | to jsonl | cat':
            self._fail('finite epoch must use the exact original two-frame command')
        screen, cursor = self._screen(typed_rows, typed_cursor)
        line = PROMPT + command
        if len(line) >= cols:
            self._fail('typed command must fit without autowrap')
        last = len(typed_rows) - 1
        if (last < 0 or typed_rows[last] != line or
                cursor != (len(line), last)):
            self._fail('pre-Enter typed line or cursor is not exact')
        self._screen_rows, self._cursor = screen, cursor
        self._typed_rows = list(typed_rows)
        self._typed_cursor = cursor
        self._typed_abs = cursor[1]
        self._output_start = self._typed_abs + 1
        self._physical[self._typed_abs] = line

    def _fail(self, reason):
        self.complete = False
        self._failed = True
        if reason not in self._reasons:
            self._reasons.append(reason)
        raise OracleIncomplete('oracle-observability-incomplete: ' + reason)

    def _screen(self, rows, cursor):
        if (not isinstance(rows, (list, tuple)) or len(rows) > self.height or
                any(not isinstance(row, str) or len(row) > self.cols or
                    row != row.rstrip(' ') or
                    (row != KNOWN_BANNER and any(not 32 <= ord(char) <= 126 for char in row))
                    for row in rows)):
            self._fail('screen is not exact trimmed width-one ASCII rows')
        if (not isinstance(cursor, (list, tuple)) or len(cursor) != 2 or
                any(type(value) is not int for value in cursor)):
            self._fail('missing or malformed exact cursor')
        x, y = cursor
        if not (0 <= x < self.cols and 0 <= y < self.height):
            self._fail('cursor outside source grid')
        screen = list(rows) + [''] * (self.height - len(rows))
        if any(screen[y + 1:]):
            self._fail('append-only epoch has material below cursor')
        # A full row leaves the source cursor in its last column, wrap pending.
        if len(screen[y]) > x and not (len(screen[y]) == self.cols and x == self.cols - 1):
            self._fail('cursor retreats into observed row material')
        return screen, (x, y)

    def arm(self, commit):
        """Freeze the existing GUI gate's single exact Enter commit summary."""
        if self._failed:
            self._fail('epoch already failed')
        if self._commit is not None:
            self._fail('epoch cannot be rearmed or reuse an Enter commit')
        if (not isinstance(commit, dict) or type(commit.get('id')) is not int or
                commit['id'] <= 0 or commit.get('outcome') != 'COMMITTED' or
                type(commit.get('commit_count')) is not int or commit['commit_count'] != 1):
            self._fail('Enter lacks a single committed input batch identity')
        finish = commit.get('finish')
        if (type(finish) not in (int, float) or not math.isfinite(finish) or finish < 0):
            self._fail('Enter lacks a finite monotonic finish time in seconds')
        try:
            payload_hex = commit['payload_hex']
            if not isinstance(payload_hex, str):
                raise ValueError('payload_hex must be text')
            payload = json.loads(bytes.fromhex(payload_hex).decode('ascii'),
                                 object_pairs_hook=_object, parse_constant=_constant)
            # Copy all gate evidence, not just its interpreted success fields.
            frozen = json.loads(json.dumps(commit, allow_nan=False))
        except (KeyError, ValueError, TypeError, UnicodeError, OverflowError) as error:
            self._fail('unreadable Enter commit payload: ' + str(error))
        events = [{'type': 'key', 'data': {'down': down,
                   'key': {'type': 'qcode', 'data': 'ret'}}} for down in (True, False)]
        if payload != {'execute': 'input-send-event', 'arguments': {'events': events}}:
            self._fail('Enter payload is not exactly ret down/up without other input')
        # Python bool/int equality must not admit down=1 or down=0 as provenance.
        if any(type(event['data']['down']) is not bool
               for event in payload['arguments']['events']):
            self._fail('Enter key states must be exact booleans')
        self._commit = frozen
        self._commit_ns = math.ceil(finish * 1_000_000_000)
        return None

    def _shifts(self, screen, cursor):
        """All source-consistent forward window offsets with actual overlap."""
        old_x, old_y = self._cursor
        x, y = cursor
        old_line = self._screen_rows[old_y]
        written = self.cols if len(old_line) == self.cols else old_x
        prefix = old_line.ljust(written)
        shifts = []
        for shift in range(self.height):
            # Losing the old current row could lose an unobserved suffix.  It
            # cannot be repaired by a recognizable later JSON record or prompt.
            mapped = old_y - shift
            if mapped < 0 or y < mapped or (shift and y != self.height - 1):
                continue
            if y == mapped and x < old_x:
                continue
            if any(self._screen_rows[index] != screen[index - shift]
                   for index in range(shift, old_y)):
                continue
            if screen[mapped].ljust(self.cols)[:written] != prefix:
                continue
            # At least one complete nonblank stable row must survive any scroll.
            # A repeated JSON prefix alone can mimic shift25 after a small append
            # and cannot establish retained physical-row coverage.
            if shift and not any(self._screen_rows[index] for index in range(shift, old_y)):
                continue
            shifts.append(shift)
        return shifts

    def observe(self, rows, cursor, monotonic_ns):
        """Append one exact GUI capture; return True only for a complete proof."""
        if self._failed:
            self._fail('epoch already failed')
        if self._commit is None:
            if 'unarmed observation ignored' not in self._reasons:
                self._reasons.append('unarmed observation ignored')
            return False
        if (type(monotonic_ns) is not int or monotonic_ns <= self._commit_ns or
                (self._last_ns is not None and monotonic_ns <= self._last_ns)):
            self._fail('stale or nonmonotonic capture time relative to Enter commit')
        self._last_ns = monotonic_ns
        if rows is None:
            self.complete = False
            self._unreadable = True
            self._captures.append({'monotonic_ns': monotonic_ns, 'readable': False})
            return False
        screen, cursor = self._screen(rows, cursor)
        if screen == self._screen_rows and cursor == self._cursor:
            self._captures.append({'monotonic_ns': monotonic_ns, 'readable': True,
                                   'duplicate': True, 'shift': 0})
            self._unreadable = False
            return self._evaluate()
        shifts = self._shifts(screen, cursor)
        if len(shifts) != 1:
            self._captures.append({'monotonic_ns': monotonic_ns, 'readable': True,
                                   'candidate_shifts': shifts, 'rows': list(rows),
                                   'cursor': list(cursor)})
            self._fail('ambiguous row overlap' if shifts else
                       'missing physical-row coverage or non-append screen change')
        shift = shifts[0]
        old_abs = self._window_start + self._cursor[1]
        new_start = self._window_start + shift
        new_abs = new_start + cursor[1]
        # The only effect allowed on the already-proven typed command is LF.
        if old_abs == self._typed_abs and screen[self._typed_abs - new_start] != PROMPT + self.command:
            self._fail('typed command changed instead of committed Enter newline')
        for absolute in range(old_abs, new_abs + 1):
            self._physical[absolute] = screen[absolute - new_start]
        self._window_start = new_start
        self._screen_rows, self._cursor = screen, cursor
        self._unreadable = False
        self._captures.append({'monotonic_ns': monotonic_ns, 'readable': True,
                               'duplicate': False, 'shift': shift,
                               'window_start': new_start, 'absolute_cursor_row': new_abs,
                               'cursor': list(cursor)})
        return self._evaluate()

    def _evaluate(self):
        """Reconstruct only covered physical rows, then validate actual records."""
        absolute = self._window_start + self._cursor[1]
        current = self._screen_rows[self._cursor[1]]
        prompt = (absolute >= self._output_start and current == PROMPT.rstrip() and
                  self._cursor[0] == len(PROMPT))
        self._prompt_row = absolute if prompt else None
        end = absolute if prompt else absolute + 1
        pieces = []
        for index in range(self._output_start, end):
            if index not in self._physical:
                self._fail('output has a missing absolute physical row')
            row = self._physical[index]
            pieces.append(row)
            if index < absolute and len(row) < self.cols:
                pieces.append('\n')
        self._transcript = ''.join(pieces)
        if self.kind == 'status':
            wanted = 'st=' + str(self.expected_status) + '\n'
            if prompt and self._transcript != wanted:
                self._fail('status epoch lacks exact own output ' + wanted.rstrip())
            self.complete = bool(prompt and self._transcript == wanted)
            return self.complete
        records = []
        position = 0
        text = self._transcript
        while position < len(text):
            while position < len(text) and text[position] in ' \n\r\t':
                position += 1
            if position == len(text):
                break
            try:
                record, position = _DECODER.raw_decode(text, position)
            except (ValueError, RecursionError) as error:
                if prompt:
                    self._fail('malformed or unfinished JSON output: ' + str(error))
                break  # A later exact append can finish the current JSON object.
            try:
                # Exponents such as 1e999 decode as infinity even though they
                # are not the decoder's explicitly forbidden NaN constants.
                json.dumps(record, allow_nan=False)
            except (ValueError, RecursionError) as error:
                self._records = records
                self._fail('record is not finite JSON evidence: ' + str(error))
            records.append(record)
        self._records = records
        self._validate_records(records, final=prompt)
        self.complete = bool(prompt and len(self._frames) == 2 and
                             all(frame['ended'] for frame in self._frames))
        return self.complete

    def _validate_records(self, records, *, final):
        frames = []
        active = None
        samples = set()
        cells = set()
        values = set()
        self._frames = frames
        for record in records:
            if not isinstance(record, dict):
                self._fail('JSON output record must be an object')
            tag = record.get('frame')
            if tag == 'begin':
                sample = record.get('sample_id')
                if active is not None or len(frames) >= 2:
                    self._fail('frame begin is out of order or exceeds two frames')
                if type(sample) is not int or sample < 0 or sample in samples:
                    self._fail('frame sample identity is missing, invalid or duplicated')
                meta = record.get('meta', {})
                if not isinstance(meta, dict):
                    self._fail('frame metadata must be an object when present')
                count = meta.get('total_cells')
                if 'total_cells' in meta and (type(count) is not int or count < 0):
                    self._fail('frame total_cells is not a nonnegative count')
                samples.add(sample)
                active = {'sample_id': sample, 'begin': copy.deepcopy(record),
                          'row_count': 0, 'expected_rows': count, 'ended': False}
                frames.append(active)
                cells, values = set(), set()
            elif tag == 'row':
                if active is None or not isinstance(record.get('value'), dict):
                    self._fail('row lacks an active frame or object value')
                value = record['value']
                signature = json.dumps(value, sort_keys=True, allow_nan=False)
                if signature in values:
                    self._fail('duplicate row value within a frame')
                values.add(signature)
                if 'cell_id' in value:
                    cell = value['cell_id']
                    if type(cell) is not int or cell < 0 or cell in cells:
                        self._fail('duplicate or invalid cell identity/generation within frame')
                    cells.add(cell)
                if 'generation' in value and (type(value['generation']) is not int or value['generation'] < 0):
                    self._fail('invalid cell generation')
                active['row_count'] += 1
                if active['expected_rows'] is not None and active['row_count'] > active['expected_rows']:
                    self._fail('frame has more rows than observed begin total_cells')
            elif tag == 'end':
                if active is None:
                    self._fail('end lacks an active frame')
                if (record.get('complete') is not True or type(record.get('dropped')) is not int or
                        record['dropped'] != 0):
                    self._fail('frame end is incomplete or has dropped material')
                count = active['expected_rows']
                if ((count is not None and active['row_count'] != count) or
                        (count is None and active['row_count'] == 0)):
                    self._fail('frame row count does not establish complete observed payload')
                active['ended'] = True
                active['end'] = copy.deepcopy(record)
                active = None
            else:
                self._fail('unknown or missing frame tag')
        if final and (active is not None or len(frames) != 2):
            self._fail('fresh prompt arrived without two complete observed frames')

    def snapshot(self):
        """Return a detached JSON-compatible proof/failure diagnostic."""
        reasons = list(self._reasons)
        if self._commit is None:
            reasons.append('Enter epoch is not armed')
        if self._unreadable:
            reasons.append('last capture unreadable; exact overlap recovery required')
        if not self.complete and not self._failed:
            reasons.append('complete own output and fresh prompt not yet proved')
        physical = [{'absolute_row': index, 'text': text}
                    for index, text in sorted(self._physical.items())
                    if index >= self._output_start]
        return copy.deepcopy({
            'kind': self.kind, 'command': self.command,
            'geometry': [self.cols, self.height], 'complete': self.complete,
            'failed': self._failed, 'reasons': reasons,
            'typed_rows': self._typed_rows, 'typed_cursor': list(self._typed_cursor),
            'commit': self._commit, 'rows': physical, 'transcript': self._transcript,
            'records': self._records, 'frames': self._frames,
            'window_start': self._window_start, 'cursor': list(self._cursor),
            'prompt_absolute_row': self._prompt_row,
            'time': {'commit_finish_ns': self._commit_ns, 'last_capture_ns': self._last_ns},
            'evidence': self._captures,
        })
