"""T6 final-screen cross-check: tail grid vs the retained source-pinned grid and the real renderer."""
import hashlib
import importlib.util
from pathlib import Path
import random
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import hook_ledger as L
import hookfix as H

spec = importlib.util.spec_from_file_location('retained_prove_scroll', HERE.parent / 'observer-finite-oracle-analysis/prove_scroll.py')
PRIOR = importlib.util.module_from_spec(spec)
spec.loader.exec_module(PRIOR)
D = PRIOR.D


class TailGridParity(unittest.TestCase):
    def test_geometry_constants_match_the_qualified_oracle(self):
        self.assertEqual((L.COLS, L.ROWS), (D.COLS, D.ROWS))
        self.assertEqual(L.PROMPT, D.PROMPT)

    def test_identical_to_retained_transcript_grid_on_wrap_scroll_and_cr(self):
        rng = random.Random(11)
        for case in range(60):
            text = ''
            for _ in range(rng.randrange(1, 40)):
                width = rng.choice([0, 1, 5, 78, 79, 80, 158, 159, 237, 300])
                text += ''.join(rng.choice('abcXYZ019{}:",') for _ in range(width)) + rng.choice(['\n', '\n', '\r\n', ''])
            old, new = PRIOR.TranscriptGrid(), L.TailGrid()
            old.feed(text)
            new.feed(text)
            self.assertEqual((old.visible(), old.x, old.y, old.scrolls, old.wrap_pending),
                             (new.visible(), new.x, new.y, new.scrolls, new.wrap_pending), case)

    def test_exact_width_row_followed_by_lf_adds_no_blank_row(self):
        grid = L.TailGrid()
        grid.feed('a' * 79 + '\nb')
        self.assertEqual(grid.visible(), ['a' * 79, 'b'])
        grid = L.TailGrid()
        grid.feed('a' * 80 + '\nb')
        self.assertEqual(grid.visible(), ['a' * 79, 'a', 'b'])


class ScreenMatch(unittest.TestCase):
    def test_short_epoch_matches_as_suffix_of_a_fuller_screen(self):
        rows, cursor, scrolls = L.expected_tail(L.STATUS, 'st=0\n')
        gui = ['', 'banner', 'Cellos > top', 'earlier output'] + rows
        self.assertTrue(L.screen_matches(gui, (9, len(gui) - 1), rows))
        self.assertEqual(scrolls, 0)

    def test_scrolled_epoch_must_equal_whole_screen(self):
        guest, f, s = H.two_epochs()
        verdict = L.evaluate('finite', L.FINITE, guest.hits, f['typed_grab_ns'], f['commit_finish_ns'], 10 ** 15, f['until_ns'])
        rows = verdict.extra['expected_rows']
        self.assertEqual(len(rows), 26)
        self.assertTrue(L.screen_matches(rows, (9, 25), rows))
        for index in (0, 10, 24, 25):
            altered = list(rows)
            altered[index] = altered[index][:-1] + ('x' if altered[index][-1] != 'x' else 'y')
            self.assertFalse(L.screen_matches(altered, (9, 25), rows), index)

    def test_rejections(self):
        rows, _c, _s = L.expected_tail(L.STATUS, 'st=0\n')
        self.assertFalse(L.screen_matches([], (9, 0), rows))
        self.assertFalse(L.screen_matches(rows, None, rows))
        self.assertFalse(L.screen_matches(rows, (8, len(rows) - 1), rows))
        self.assertFalse(L.screen_matches(rows, (9, len(rows)), rows))
        self.assertFalse(L.screen_matches(rows[1:], (9, len(rows) - 2), rows))  # fewer rows than the epoch needs
        self.assertFalse(L.screen_matches(rows[:-1] + ['Cellos # '], (9, len(rows) - 1), rows))
        with self.assertRaises(L.OracleFail):
            L.expected_tail(L.FINITE, 'caf\u00e9\n')


class RealRenderer(unittest.TestCase):
    """The expected rows survive the qualified glyph renderer and exact pixel reader."""

    def test_rendered_expected_tail_is_read_back_and_matches(self):
        self.assertEqual(hashlib.sha256(PRIOR.GLYPH.read_bytes()).hexdigest(), PRIOR.GLYPH_SHA)
        guest, f, s = H.two_epochs()
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            (work / 'scratch').mkdir()
            desk = object.__new__(D.Desk)
            D.Proof.__init__(desk, SimpleNamespace(work=str(work), evidence=str(work / 'ev'), glyph_bin=str(PRIOR.GLYPH)))
            desk.model = ['', D.BANNER, D.PROMPT]
            desk.window = lambda image: ((0, 0), image)
            from PIL import ImageDraw
            for label, command, text in (('finite', L.FINITE, None), ('status', L.STATUS, 'st=0\n')):
                if text is None:
                    verdict = L.evaluate('finite', command, guest.hits, f['typed_grab_ns'], f['commit_finish_ns'], 10 ** 15, f['until_ns'])
                    text = verdict.extra['output_text']
                rows, cursor, _scrolls = L.expected_tail(command, text)
                image = desk.render(rows).copy()
                x = D.gui.PAD_X + cursor[0] * D.CELL_W
                y = D.gui.PAD_Y + cursor[1] * D.CELL_H + 2
                ImageDraw.Draw(image).rectangle((x, y, x + D.CELL_W - 1, y + D.CELL_H - 5), fill=(48, 220, 200))
                got = desk.read_screen(image)
                self.assertIsNotNone(got, label)
                read_rows, read_cursor = got
                self.assertEqual(read_rows, rows, label)
                self.assertTrue(L.screen_matches(read_rows, read_cursor, rows), label)
                broken = list(rows)
                broken[-2] = broken[-2][:-1] + 'Q'
                self.assertFalse(L.screen_matches(broken, read_cursor, rows), label)


if __name__ == '__main__':
    unittest.main()
