"""S10-4.17 pure ledger, drain-grammar, tokenizer and acceptance oracle.

No I/O except the optional ledger JSONL, no guest, QMP or RSP access.  The oracle accepts
ONLY from frontend-consumed bytes handed to `ByteSink::feed`; it is a whitelist tokenizer,
not a terminal emulator.  Failure codes carry a class hint:

  C = observability-incomplete (ledger/GUI cannot prove), D = ledger intact and content
  contradicts the expected output.  Host failures (class E) are decided by the runner.
"""
import copy
import hashlib
import json
import os
import re
import threading

PROMPT = 'Cellos > '
COLS, ROWS = 79, 26
SOFT_RESET = b'\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m'
REDRAW = b'\r\x1b[2K'
FINITE = 'top --records -d 1 --frames 2 | to jsonl | cat'
STATUS = 'echo st=$?'
RET_STREAM = {0x14b: 'stdout', 0x1a5: 'stdout', 0x1e5: 'stderr', 0x22d: 'stderr'}
BURST_START = {0x14b, 0x1e5}
BURST_CONTINUE = {0x1a5: {0x14b, 0x1a5}, 0x22d: {0x1e5, 0x22d}}
MAX_RUN = 31
MAX_HITS = 2048
MAX_BYTES = 512 * 1024
QUIESCENCE_NS = 3_000_000_000


class LedgerViolation(RuntimeError):
    def __init__(self, code, reason):
        super().__init__(f'{code}: {reason}')
        self.code, self.reason = code, reason


class OracleFail(RuntimeError):
    """cls is 'C' (observability-incomplete) or 'D' (observed negative result)."""

    def __init__(self, cls, code, reason):
        super().__init__(f'{cls}/{code}: {reason}')
        self.cls, self.code, self.reason = cls, code, reason


def hit_record(seq, ns, rsp, ret_off, length, data, cr3=None, rdi=None, rsi=None):
    return {'seq': seq, 'ns': ns, 'rsp': rsp, 'ret_off': ret_off, 'stream': RET_STREAM.get(ret_off),
            'length': length, 'sha256': hashlib.sha256(data).hexdigest(), 'data_hex': data.hex(),
            'cr3': cr3, 'rdi': rdi, 'rsi': rsi}


class Ledger:
    """Ordered frontend byte ledger; lock is a leaf (never held across any I/O)."""

    def __init__(self, path=None, max_hits=MAX_HITS, max_bytes=MAX_BYTES):
        self._lock = threading.Lock()
        self._hits = []
        self._bytes = 0
        self._last_site = None
        self._run = 0
        self._path = path
        self._fd = None
        self.max_hits, self.max_bytes = max_hits, max_bytes
        self.violation = None
        if path is not None:
            self._fd = os.open(str(path), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)

    def _check(self, record, previous_ns, expected_seq):
        site = record['ret_off']
        if record['seq'] != expected_seq:
            raise LedgerViolation('sequence', f'seq {record["seq"]} expected {expected_seq}')
        if previous_ns is not None and record['ns'] < previous_ns:
            raise LedgerViolation('time-order', 'host time went backwards')
        if site not in RET_STREAM:
            raise LedgerViolation('unknown-return-site', f'{site:#x}')
        data = bytes.fromhex(record['data_hex'])
        if not 1 <= record['length'] <= 0x200 or len(data) != record['length']:
            raise LedgerViolation('length', f'length {record["length"]} data {len(data)}')
        if hashlib.sha256(data).hexdigest() != record['sha256']:
            raise LedgerViolation('hash', 'payload hash mismatch')
        if site in BURST_CONTINUE:
            if self._last_site not in BURST_CONTINUE[site]:
                raise LedgerViolation('drain-grammar', f'{site:#x} without burst predecessor (prev {self._last_site})')
            if self._run >= MAX_RUN:
                raise LedgerViolation('drain-grammar', f'burst longer than {MAX_RUN + 1} feeds')
        if len(self._hits) + 1 > self.max_hits:
            raise LedgerViolation('cap-hits', f'more than {self.max_hits} hits')
        if self._bytes + len(data) > self.max_bytes:
            raise LedgerViolation('cap-bytes', f'more than {self.max_bytes} bytes')

    def append(self, record):
        with self._lock:
            previous = self._hits[-1]['ns'] if self._hits else None
            try:
                self._check(record, previous, len(self._hits) + 1)
            except LedgerViolation as error:
                self.violation = {'code': error.code, 'reason': error.reason, 'record': copy.deepcopy(record)}
                accepted = False
                failure = error
            else:
                self._hits.append(copy.deepcopy(record))
                self._bytes += record['length']
                site = record['ret_off']
                self._run = self._run + 1 if site in BURST_CONTINUE else 0
                self._last_site = site
                accepted = True
                failure = None
        if self._fd is not None:
            os.write(self._fd, (json.dumps({**record, 'accepted': accepted}) + '\n').encode())
            os.fsync(self._fd)
        if failure:
            raise failure

    def snapshot(self):
        with self._lock:
            return copy.deepcopy(self._hits)

    def count(self):
        with self._lock:
            return len(self._hits)

    def total_bytes(self):
        with self._lock:
            return self._bytes

    def close(self):
        if self._fd is not None:
            os.close(self._fd)
            self._fd = None


# ---------------------------------------------------------------- tokenizer

def tokenize(data, final=False):
    """Whitelist tokens: ('text', str) ('nl',) ('cr',) ('redraw',) ('reset',).

    Any other control/ESC byte, invalid UTF-8 or C1 code point raises OracleFail('C').
    A trailing incomplete UTF-8 sequence is tolerated unless `final`.
    """
    tokens = []
    i, n = 0, len(data)
    run = bytearray()

    def flush(at_end):
        if not run:
            return
        try:
            text = bytes(run).decode('utf-8')
        except UnicodeDecodeError as error:
            if at_end and not final and error.start >= len(run) - 3 and error.reason == 'unexpected end of data':
                text = bytes(run[:error.start]).decode('utf-8')
            else:
                raise OracleFail('C', 'invalid-utf8', str(error)) from error
        if any(0x80 <= ord(c) <= 0x9f for c in text):
            raise OracleFail('C', 'c1-control', 'C1 control code point')
        if text:
            tokens.append(('text', text))
        run.clear()

    while i < n:
        if data.startswith(REDRAW, i):
            flush(False)
            tokens.append(('redraw',))
            i += len(REDRAW)
        elif data.startswith(SOFT_RESET, i):
            flush(False)
            tokens.append(('reset',))
            i += len(SOFT_RESET)
        else:
            byte = data[i]
            if byte == 0x0a:
                flush(False)
                tokens.append(('nl',))
            elif byte == 0x0d:
                flush(False)
                tokens.append(('cr',))
            elif byte < 0x20 or byte == 0x7f:
                raise OracleFail('C', 'unsupported-control', f'byte {byte:#04x} at {i}')
            else:
                run.append(byte)
            i += 1
    flush(True)
    return tokens


def last_line(data):
    """Pre-Enter tail: bytes after the last LF only; history before is never interpreted."""
    return data[data.rfind(b'\n') + 1:]


def line_text(tokens):
    """Reduce one line's tokens: text appends, redraw clears, reset no-op, bare CR unsupported."""
    text = ''
    for token in tokens:
        if token[0] == 'text':
            text += token[1]
        elif token[0] == 'redraw':
            text = ''
        elif token[0] == 'reset':
            pass
        else:
            raise OracleFail('C', 'unsupported-line-token', repr(token))
    return text


def split_lines(tokens):
    """Split at 'nl'; a 'cr' is only legal immediately before 'nl'. Returns (full_lines, tail_tokens)."""
    lines, current = [], []
    for index, token in enumerate(tokens):
        if token[0] == 'nl':
            if current and current[-1][0] == 'cr':
                current.pop()
            lines.append(current)
            current = []
        else:
            current.append(token)
    return lines, current


# ---------------------------------------------------------------- JSON records

def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key: ' + key)
        result[key] = value
    return result


def _constant(value):
    raise ValueError('non-finite JSON constant: ' + value)


_DECODER = json.JSONDecoder(object_pairs_hook=_object, parse_constant=_constant)


def decode_records(text):
    records, position = [], 0
    while position < len(text):
        while position < len(text) and text[position] in ' \n\r\t':
            position += 1
        if position == len(text):
            break
        try:
            record, position = _DECODER.raw_decode(text, position)
        except (ValueError, RecursionError) as error:
            raise OracleFail('C', 'malformed-json', str(error)) from error
        try:
            json.dumps(record, allow_nan=False)
        except (ValueError, RecursionError) as error:
            raise OracleFail('C', 'non-finite-json', str(error)) from error
        records.append(record)
    return records


def validate_records(records):
    """Same record rules as the retained TemporalLedger._validate_records, final=True."""
    frames, active, samples, cells, values = [], None, set(), set(), set()
    for record in records:
        if not isinstance(record, dict):
            raise OracleFail('C', 'record-not-object', 'JSON output record must be an object')
        tag = record.get('frame')
        if tag == 'begin':
            sample = record.get('sample_id')
            if active is not None or len(frames) >= 2:
                raise OracleFail('D', 'frame-count', 'frame begin out of order or exceeds two frames')
            if type(sample) is not int or sample < 0 or sample in samples:
                raise OracleFail('C', 'sample-identity', 'frame sample identity missing, invalid or duplicated')
            meta = record.get('meta', {})
            if not isinstance(meta, dict):
                raise OracleFail('C', 'meta', 'frame metadata must be an object when present')
            count = meta.get('total_cells')
            if 'total_cells' in meta and (type(count) is not int or count < 0):
                raise OracleFail('C', 'total-cells', 'frame total_cells is not a nonnegative count')
            samples.add(sample)
            active = {'sample_id': sample, 'begin': copy.deepcopy(record), 'row_count': 0,
                      'expected_rows': count, 'ended': False}
            frames.append(active)
            cells, values = set(), set()
        elif tag == 'row':
            if active is None or not isinstance(record.get('value'), dict):
                raise OracleFail('C', 'row-structure', 'row lacks an active frame or object value')
            value = record['value']
            signature = json.dumps(value, sort_keys=True, allow_nan=False)
            if signature in values:
                raise OracleFail('C', 'duplicate-row', 'duplicate row value within a frame')
            values.add(signature)
            if 'cell_id' in value:
                cell = value['cell_id']
                if type(cell) is not int or cell < 0 or cell in cells:
                    raise OracleFail('C', 'duplicate-cell', 'duplicate or invalid cell identity within frame')
                cells.add(cell)
            if 'generation' in value and (type(value['generation']) is not int or value['generation'] < 0):
                raise OracleFail('C', 'generation', 'invalid cell generation')
            active['row_count'] += 1
            if active['expected_rows'] is not None and active['row_count'] > active['expected_rows']:
                raise OracleFail('C', 'row-overflow', 'frame has more rows than total_cells')
        elif tag == 'end':
            if active is None:
                raise OracleFail('C', 'end-without-begin', 'end lacks an active frame')
            if record.get('complete') is not True or type(record.get('dropped')) is not int or record['dropped'] != 0:
                raise OracleFail('D', 'end-incomplete', 'frame end is incomplete or reports dropped material')
            count = active['expected_rows']
            if (count is not None and active['row_count'] != count) or (count is None and active['row_count'] == 0):
                raise OracleFail('C', 'row-count', 'frame row count does not establish complete payload')
            active['ended'] = True
            active['end'] = copy.deepcopy(record)
            active = None
        else:
            raise OracleFail('C', 'unknown-tag', 'unknown or missing frame tag')
    if active is not None:
        raise OracleFail('C', 'unfinished-frame', 'prompt arrived inside an unfinished frame')
    if len(frames) != 2:
        raise OracleFail('D', 'frame-count', f'{len(frames)} complete frames, expected 2')
    return frames


# ---------------------------------------------------------------- tail grid (F5)

class TailGrid:
    """Width-one ASCII subset of the source-pinned TerminalGrid delayed-autowrap/LF model."""

    def __init__(self, cols=COLS, rows=ROWS):
        self.cols, self.height = cols, rows
        self.rows = [' ' * cols for _ in range(rows)]
        self.x = self.y = 0
        self.wrap_pending = False
        self.scrolls = 0

    def newline(self):
        self.x = 0
        self.wrap_pending = False
        if self.y == self.height - 1:
            self.rows.pop(0)
            self.rows.append(' ' * self.cols)
            self.scrolls += 1
        else:
            self.y += 1

    def feed(self, text):
        for char in text:
            if char == '\n':
                self.newline()
            elif char == '\r':
                self.x = 0
                self.wrap_pending = False
            else:
                if self.wrap_pending:
                    self.newline()
                row = self.rows[self.y]
                self.rows[self.y] = row[:self.x] + char + row[self.x + 1:]
                if self.x + 1 >= self.cols:
                    self.wrap_pending = True
                else:
                    self.x += 1

    def visible(self):
        rows = [row.rstrip() for row in self.rows]
        while rows and not rows[-1]:
            rows.pop()
        return rows


def expected_tail(command, output_text):
    """Rows the Terminal must show for the epoch alone: typed line, output, fresh prompt."""
    if not (PROMPT + command + output_text + PROMPT).isascii():
        raise OracleFail('C', 'non-ascii-tail', 'tail model is width-one ASCII only')
    grid = TailGrid()
    grid.feed(PROMPT + command + '\n' + output_text + PROMPT)
    return grid.visible(), (grid.x, grid.y), grid.scrolls


def screen_matches(read_rows, read_cursor, expected_rows):
    """GUI rows must end with the epoch rows; a scrolled epoch must equal the whole screen."""
    if not isinstance(read_rows, (list, tuple)) or not read_rows:
        return False
    if read_cursor is None or tuple(read_cursor) != (len(PROMPT), len(read_rows) - 1):
        return False
    if len(read_rows) < len(expected_rows):
        return False
    return list(read_rows[len(read_rows) - len(expected_rows):]) == list(expected_rows)


# ---------------------------------------------------------------- oracle

class Verdict:
    def __init__(self, status, code=None, reason=None, cls=None, **extra):
        self.status, self.code, self.reason, self.cls = status, code, reason, cls
        self.extra = extra

    def as_dict(self):
        return {'status': self.status, 'code': self.code, 'reason': self.reason, 'cls': self.cls, **self.extra}


def split_hits(hits, typed_grab_ns, commit_finish_ns, until_ns=None):
    pre = [h for h in hits if h['ns'] <= typed_grab_ns]
    window = [h for h in hits if typed_grab_ns < h['ns'] <= commit_finish_ns]
    post = [h for h in hits if h['ns'] > commit_finish_ns and (until_ns is None or h['ns'] <= until_ns)]
    return pre, window, post


def check_typed(pre_hits, command):
    """F1: the last line before the epoch cut is exactly the prompt plus the typed command."""
    data = b''.join(bytes.fromhex(h['data_hex']) for h in pre_hits)
    tokens = tokenize(last_line(data), final=True)
    for token in tokens:
        if token[0] in ('cr', 'reset'):
            raise OracleFail('C', 'typed-line-token', repr(token))
    text = line_text(tokens)
    if text != PROMPT + command:
        raise OracleFail('C', 'typed-line-mismatch', f'ledger last line {text!r} != prompt+command')
    return text


def evaluate(kind, command, hits, typed_grab_ns, commit_finish_ns, now_ns, until_ns=None):
    """F1-F4 over the ledger.  PENDING until the stream ends in the prompt and is quiescent."""
    if kind not in ('finite', 'status') or command != (FINITE if kind == 'finite' else STATUS):
        return Verdict('FAIL', 'wrong-command', 'epoch kind/command is not the exact original', 'C')
    try:
        pre, window, post = split_hits(hits, typed_grab_ns, commit_finish_ns, until_ns)
        if window:
            raise OracleFail('C', 'F2-window-hit', f'{len(window)} hit(s) between typed-exact grab and Enter commit')
        check_typed(pre, command)
        data = b''.join(bytes.fromhex(h['data_hex']) for h in post)
        if not data:
            return Verdict('PENDING', 'no-output-yet')
        tokens = tokenize(data)
        # The first token must be the Enter echo LF (optionally CR LF).
        head = 1 if tokens and tokens[0][0] == 'nl' else 2 if len(tokens) > 1 and tokens[0][0] == 'cr' and tokens[1][0] == 'nl' else 0
        if not head:
            raise OracleFail('C', 'F3-enter-echo', f'first post-Enter token {tokens[0]!r} is not LF')
        lines, tail = split_lines(tokens[head:])
        out_lines = []
        for line in lines:
            if any(t[0] in ('reset', 'redraw') for t in line):
                raise OracleFail('C', 'F3-control-in-output', 'reset/redraw token inside output lines')
            if any(t[0] == 'cr' for t in line):
                raise OracleFail('C', 'F3-bare-cr', 'carriage return inside an output line')
            out_lines.append(line_text(line))
        current = line_text(tail) if all(t[0] in ('text', 'redraw', 'reset') for t in tail) else None
        if current is None:
            return Verdict('PENDING', 'trailing-cr')
        output_text = ''.join(line + '\n' for line in out_lines)
        if current != PROMPT:
            return Verdict('PENDING', 'prompt-not-reached', None, None, hits=len(post), bytes=len(data))
        # Prompt reached: full content validation.
        if kind == 'status':
            if output_text != 'st=0\n':
                match = re.fullmatch(r'st=([0-9]+)\n', output_text)
                if match and int(match[1]) != 0:
                    raise OracleFail('D', 'status-nonzero', f'status {match[1]}')
                raise OracleFail('C', 'status-text', f'unexpected status epoch output {output_text!r}')
            frames = []
        else:
            records = decode_records(output_text)
            frames = validate_records(records)
        last_ns = post[-1]['ns']
        if now_ns - last_ns < QUIESCENCE_NS:
            return Verdict('PENDING', 'quiescence', None, None, prompt=True, last_hit_ns=last_ns)
        rows, cursor, scrolls = expected_tail(command, output_text)
        return Verdict('READY', None, None, None, output_text=output_text, frames=len(frames), expected_rows=rows,
                       expected_cursor=cursor, scrolls=scrolls, hits=len(post), bytes=len(data), last_hit_ns=last_ns)
    except OracleFail as error:
        return Verdict('FAIL', error.code, error.reason, error.cls)
