#!/usr/bin/env python3
"""Offline synthetic counterexample only: immutable renderer/predicate, no VM/fix."""
import hashlib
import json
import math
from pathlib import Path
import sys
from types import SimpleNamespace

from PIL import ImageDraw

ROOT = Path('/home/dmin/cellos')
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / 'scripts'))
import phase10_canonical_drive as D

GLYPH = Path('/tmp/cellbox-phase10-a3hg7abb/host-target/x86_64-unknown-linux-gnu/release/phase04-gui-glyphs')
GLYPH_SHA = '820c92d2dc0aa5c1f50533a42283974b07998111adaff8e416f56dd4481eec79'
PRIOR = HERE.parent / 'observer-recurring/headless/runtime-result.json'
COMMAND = 'top --records -d 1 --frames 2 | to jsonl | cat'


class TranscriptGrid:
    """Width-one/ASCII subset of source-pinned TerminalGrid delayed autowrap/LF."""
    def __init__(self):
        self.rows = [' ' * D.COLS for _ in range(D.ROWS)]
        self.x = self.y = 0
        self.wrap_pending = False
        self.scrolls = 0

    def newline(self):
        self.x = 0
        self.wrap_pending = False
        if self.y == D.ROWS - 1:
            self.rows.pop(0)
            self.rows.append(' ' * D.COLS)
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
                if self.x + 1 >= D.COLS:
                    self.wrap_pending = True
                else:
                    self.x += 1

    def visible(self):
        rows = [row.rstrip() for row in self.rows]
        while rows and not rows[-1]:
            rows.pop()
        return rows


def main():
    assert hashlib.sha256(GLYPH.read_bytes()).hexdigest() == GLYPH_SHA
    prior = json.loads(PRIOR.read_text())
    event = next(row for row in prior['surface_events'] if row.get('name') == 'finite-stream')
    assert event['command'] == COMMAND
    records = [json.loads(row) for row in event['rows']]
    assert sum(row.get('frame') == 'begin' for row in records) == 2
    assert sum(row.get('frame') == 'end' and row.get('complete') is True for row in records) == 2
    assert all(row.isascii() for row in event['rows'])
    work = HERE / 'render-work'
    (work / 'scratch').mkdir(parents=True, exist_ok=True)
    desk = object.__new__(D.Desk)
    D.Proof.__init__(desk, SimpleNamespace(work=str(work), evidence=str(HERE / 'synthetic'), glyph_bin=str(GLYPH)))
    desk.model = ['', D.BANNER, D.PROMPT]
    desk.window = lambda image: ((0, 0), image)
    grid = TranscriptGrid()
    grid.feed('\n' + D.BANNER + '\n' + D.PROMPT + COMMAND)
    typed_last = D.PROMPT + COMMAND
    predicate = D.Desk.at_fresh_prompt(len(desk.pad(desk.model)), typed_last)
    cases = []
    def capture(label):
        rows = grid.visible()
        cursor = (grid.x, grid.y)
        image = desk.render(rows).copy()
        x = D.gui.PAD_X + cursor[0] * D.CELL_W
        y = D.gui.PAD_Y + cursor[1] * D.CELL_H + 2
        ImageDraw.Draw(image).rectangle((x, y, x + D.CELL_W - 1, y + D.CELL_H - 5), fill=(48, 220, 200))
        image_path = HERE / 'synthetic' / f'{label}.png'
        image.save(image_path)
        got = desk.read_screen(image)
        assert got is not None, f'existing exact reader rejected synthetic {label}'
        read_rows, read_cursor = got
        assert read_rows == [row.rstrip() for row in rows] and read_cursor == cursor
        exact_prompt = bool(read_rows and read_rows[-1] == D.PROMPT.rstrip() and
                            read_cursor == (len(D.PROMPT), len(read_rows) - 1))
        case = {'label': label, 'image': str(image_path.relative_to(HERE)),
                'image_sha256': hashlib.sha256(image_path.read_bytes()).hexdigest(),
                'synthetic_rows': rows, 'reader_rows': read_rows, 'cursor': read_cursor,
                'exact_pixel_reader_accepts': True, 'exact_fresh_prompt_shape': exact_prompt,
                'original_command_echo_visible': any(typed_last in row for row in read_rows[:-1]),
                'unchanged_old_predicate_accepts': bool(predicate(read_rows, read_cursor)),
                'scroll_count': grid.scrolls}
        cases.append(case)
        return case
    typed = capture('typed-line-before-submit')
    assert typed['reader_rows'][-1] == typed_last
    assert desk.diff(desk.render(grid.visible()), grid.visible(), 0, D.ROWS - 1, (grid.x, grid.y))[0] == 0
    grid.feed('\n' + '\n'.join(event['rows']) + '\n' + D.PROMPT)
    complete = capture('valid-two-frame-completion-prompt')
    assert complete['exact_fresh_prompt_shape'] and not complete['original_command_echo_visible']
    assert not complete['unchanged_old_predicate_accepts']
    grid.feed('echo st=$?\nst=0\n' + D.PROMPT)
    status = capture('valid-status-zero-and-new-prompt')
    assert status['exact_fresh_prompt_shape'] and not status['unchanged_old_predicate_accepts']
    report = {'scope': 'Synthetic renderer/transcript counterexample; NOT the missing historical desktop screen',
              'conclusion': 'CONFIRMED synthetic false-negative: actual retained valid two-frame output shape scrolls original echo off79x26 grid; unchanged exact pixel reader accepts final prompt/status screen, but old predicate rejects only for missing echo.',
              'renderer': {'path': str(GLYPH), 'sha256': GLYPH_SHA, 'geometry': [D.WIN_W, D.WIN_H],
                           'grid': [D.COLS, D.ROWS], 'cell': [D.CELL_W, D.CELL_H],
                           'padding': [D.gui.PAD_X, D.gui.PAD_Y], 'font': 'JetBrains Mono Regular14px; retained qualified renderer; no rebuild'},
              'output_fixture': {'path': str(PRIOR), 'sha256': hashlib.sha256(PRIOR.read_bytes()).hexdigest(),
                                 'origin': 'Actual successful S10-4.11 HEADLESS finite result, used only as synthetic desktop input; no desktop-completion inference',
                                 'JSON_records': len(records), 'frame_begin_count': 2, 'frame_end_complete_count': 2,
                                 'row_lengths': [len(row) for row in event['rows']],
                                 'wrapped_grid_rows_for_output': sum(math.ceil(len(row) / D.COLS) for row in event['rows'])},
              'predicate_source': 'scripts/phase10_canonical_drive.py1080-1083; min_rows argument is unused; command visible-echo condition persists after exact typed-line precondition',
              'grid_source': 'cells/apps/terminal/src/grid.rs299-345 delayed autowrap,365-383 LF/scroll,510-526 full-screen scroll; ASCII subset simulated, no terminal/product patch',
              'cases': cases, 'historical_S10_4_14_completion': 'UNPROVEN: actual final screen/read absent; this fixture cannot reconstruct it',
              'operations': {'VM_runs': 0, 'guest_controls': 0, 'product_or_harness_edits': 0,
                             'retained_renderer_invoked_offline': True, 'build_or_package_install': False}}
    (HERE / 'synthetic-proof.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in ('conclusion', 'renderer', 'output_fixture', 'historical_S10_4_14_completion')}, indent=2))
    print(json.dumps([{'case': row['label'], 'reader_accepts': row['exact_pixel_reader_accepts'],
                       'fresh_prompt': row['exact_fresh_prompt_shape'], 'echo_visible': row['original_command_echo_visible'],
                       'old_predicate': row['unchanged_old_predicate_accepts'], 'scrolls': row['scroll_count']}
                      for row in cases], indent=2))


if __name__ == '__main__':
    main()
