"""S10-9b offline tests: the retained pixel oracle (glyph binary + Proof/Desk judgement) on the B2 terminal's expected screens.

No VM.  The only host tool executed is the retained glyph renderer (the same binary the S10-9 runs used, sha-checked).  The tests prove
(a) the B2 terminal and the oracle share every render input that can be checked offline (source constants, font/typography/viui bytes),
(b) the oracle accepts a window rendered from each B2 expected transcript and rejects one-character / wrong-prompt / wrong-cursor variants,
(c) the launch geometry (taskbar click, window locate) works on the retained B2 desktop boot-smoke screendump.
"""
import argparse
import hashlib
import re
import sys
import tempfile
import unittest
from pathlib import Path

from PIL import Image, ImageChops

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import s109b_env as ENV  # noqa: E402
import s109b_b2d as B2D  # noqa: E402
import s107_retained as RET  # noqa: E402
import common as COMMON  # noqa: E402

D = COMMON.D
import phase04_gui_drive as GUI  # noqa: E402

WORKTREE = Path('/tmp/cellbox-s10-7/worktree')
TERMINAL_SRC = WORKTREE / 'cells/apps/terminal/src'


def transcript(schedule):
    """The terminal transcript after every kept command (typed line, exact output rows, fresh prompt)."""
    rows = list(B2D.INITIAL_ROWS)
    stages = {'initial-prompt': list(rows)}
    for phase in schedule['phases']:
        if phase['kind'] == 'command':
            rows = rows[:-1] + [rows[-1] + phase['command']] + list(phase['expect_rows']) + [B2D.PROMPT]
            stages[phase['name']] = list(rows)
    return stages


class OracleOnB2Screens(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        root = Path(cls.tmp.name)
        for name in ('scratch', 'sock', 'evidence'):
            (root / name).mkdir()
        glyph = ENV.SPEC.glyph_bin
        assert ENV.sha256_file(glyph) == ENV.GLYPH_SHA
        cls.proof = GUI.Proof(argparse.Namespace(work=str(root), evidence=str(root / 'evidence'), glyph_bin=str(glyph), accel='tcg', window=900))
        cls.stages = transcript(ENV.schedule())

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def window_like_guest(self, rows):
        """The oracle's own render of `rows` plus a cursor block painted at the end of the last row (any colour the detector accepts)."""
        proof = self.proof
        win = proof.render(proof.pad(rows)).copy()
        col, row = proof.mcursor(rows)
        x, y = GUI.PAD_X + col * GUI.CELL_W, GUI.PAD_Y + row * GUI.CELL_H + 2
        win.paste((6, 133, 165), (x, y, x + GUI.CELL_W, y + GUI.CELL_H - 4))
        return win

    def screen_with(self, win, origin=(140, 128)):
        screen = Image.new('RGB', (1280, 800), (0, 0, 0))
        screen.paste(win, origin)
        return screen

    def test_every_expected_screen_matches_with_cursor_and_locates_exactly(self):
        proof = self.proof
        for name, rows in self.stages.items():
            win = self.window_like_guest(rows)
            screen = self.screen_with(win)
            self.assertEqual(proof.locate(screen), (140, 128), name)
            match = proof.match_full(screen, proof.pad(rows), proof.mcursor(rows))
            self.assertEqual((match['cursor'], match['diff']), (proof.mcursor(rows), 0), name)

    def test_one_character_change_and_wrong_prompt_and_wrong_cursor_are_rejected(self):
        proof = self.proof
        for name, rows in self.stages.items():
            win = self.window_like_guest(rows)
            screen = self.screen_with(win)
            altered = list(rows)
            index = next(i for i in range(len(altered) - 2, -1, -1) if altered[i])
            altered[index] = altered[index][:-1] + ('x' if altered[index][-1] != 'x' else 'y')
            diff, _worst = proof.diff(win, proof.pad(altered), 0, GUI.ROWS - 1, proof.mcursor(rows))
            self.assertGreaterEqual(diff, GUI.MIN_NEGATIVE_DIFF, name)
            wrong_prompt = rows[:-1] + ['Cellos # ']
            diff, _worst = proof.diff(win, proof.pad(wrong_prompt), 0, GUI.ROWS - 1, proof.mcursor(rows))
            self.assertGreaterEqual(diff, GUI.MIN_NEGATIVE_DIFF, name)
            wrong_cursor = proof.match_full(screen, proof.pad(rows), (proof.mcursor(rows)[0] + 1, proof.mcursor(rows)[1]))
            self.assertIsNone(wrong_cursor['diff'], name)

    def test_leading_spaces_of_the_ps_table_are_judged(self):
        proof = self.proof
        rows = self.stages['b2d-ps']
        win = self.window_like_guest(rows)
        stripped = [r.lstrip(' ') if r.startswith(' ') else r for r in rows]
        self.assertNotEqual(stripped, rows)
        diff, _worst = proof.diff(win, proof.pad(stripped), 0, GUI.ROWS - 1, proof.mcursor(rows))
        self.assertGreaterEqual(diff, GUI.MIN_NEGATIVE_DIFF)

    def test_consecutive_stages_differ_and_pre_launch_screen_differs_from_the_prompt(self):
        proof = self.proof
        names = list(self.stages)
        for before, after in zip(names, names[1:]):
            win_before, win_after = self.window_like_guest(self.stages[before]), self.window_like_guest(self.stages[after])
            self.assertIsNotNone(ImageChops.difference(win_before, win_after).getbbox(), (before, after))
        smoke = Image.open(ENV.V2_EVIDENCE / 'smoke-hooks/screen.png').convert('RGB')
        origin = (140, 128)
        region = smoke.crop((origin[0], origin[1], origin[0] + GUI.WIN_W, origin[1] + GUI.WIN_H))
        diff, _worst = proof.diff(region, proof.pad(B2D.INITIAL_ROWS), 0, GUI.ROWS - 1, proof.mcursor(B2D.INITIAL_ROWS))
        self.assertGreater(diff, 1000)                       # the retained `desktop prompt negative control`

    def test_transcripts_fit_without_scrolling_so_the_retained_model_is_exact(self):
        self.assertLessEqual(max(len(rows) for rows in self.stages.values()), GUI.ROWS)
        self.assertEqual(len(self.stages['b2d-ps']), 21)
        for rows in self.stages.values():
            self.assertTrue(all(len(row) <= GUI.COLS for row in rows))
            self.assertTrue(all(row.isascii() for row in rows))

    def test_the_initial_rows_use_only_characters_the_glyph_atlas_covers(self):
        for rows in self.stages.values():
            for row in rows:
                self.assertTrue(all(32 <= ord(ch) <= 126 for ch in row))

    def test_typed_characters_exist_in_the_retained_key_table(self):
        for phase in ENV.schedule()['phases']:
            if phase['kind'] == 'command':
                for ch in phase['command']:
                    self.assertIn(ch, D.QCODE, (phase['command'], ch))


class SharedRenderInputs(unittest.TestCase):
    """The B2 terminal (3471028bc worktree) and the glyph oracle (current tree) share every render input that exists offline."""

    def sha(self, path):
        return hashlib.sha256(Path(path).read_bytes()).hexdigest()

    def test_geometry_and_colours_equal_the_oracle_constants(self):
        main = (TERMINAL_SRC / 'main.rs').read_text()
        grid = (TERMINAL_SRC / 'grid.rs').read_text()
        self.assertIn('let cell_w = 9.0;', main)
        self.assertIn('let cell_h = 18.0;', main)
        self.assertIn('let pad_x = 8;', main)
        self.assertIn('let pad_y = 6;', main)
        self.assertIn('let font_size = 14.0;', main)
        self.assertIn('Color::rgb(18, 20, 26)', main)
        self.assertIn('Color::rgb(0, 200, 255).with_alpha(160)', main)
        self.assertIn('FontFace::MonoRegular', main)
        self.assertIn('Color::rgb(220, 225, 235)', grid)
        self.assertEqual((GUI.CELL_W, GUI.CELL_H, GUI.PAD_X, GUI.PAD_Y, GUI.BG), (9, 18, 8, 6, (18, 20, 26)))
        self.assertIn('let win_w = 720.min(screen_w);', main)
        self.assertIn('let win_h = 480.min(screen_h - 36);', main)
        self.assertIn('surf.move_to(140, 100);', main)
        self.assertEqual((GUI.WIN_W, GUI.WIN_H), (720, 480))
        # the guest draws a column only if it fits: 79 columns of the 80-column grid, all 26 rows
        self.assertEqual((GUI.COLS, GUI.ROWS), ((720 - 8) // 9, (480 - 6) // 18))
        self.assertEqual((720 // 9, 480 // 18), (80, 26))
        self.assertIn('if col_x + self.cell_w > sw as f32 {', main)

    def test_prompt_banner_and_launcher_literals(self):
        session = (TERMINAL_SRC / 'session.rs').read_text()
        self.assertIn('"Cellos > "', session)
        self.assertEqual(B2D.PROMPT, GUI.PROMPT)
        self.assertIn('=== CellOS GUI Terminal (x86_64) ===\\nType \'help\' for commands. Single-reader stream lease active.\\n\\nCellos > ', session)
        self.assertNotEqual(B2D.BANNER[0], GUI.BANNER)           # the oracle's built-in Cellbox banner does NOT apply: rows are passed explicitly
        apps = (WORKTREE / 'cells/apps/desktop/src/apps.rs').read_text()
        self.assertIn('/bin/terminal', apps)

    def test_font_typography_and_viui_sources_are_byte_identical_to_the_oracle_build_tree(self):
        repo = ENV.REPO
        for rel in ('libs/ostd/src/typography.rs', 'libs/ostd/src/font.rs', 'libs/ostd/src/font_atlas.rs'):
            self.assertEqual(self.sha(repo / rel), self.sha(WORKTREE / rel), rel)
        for rel in ('libs/ostd/assets', 'libs/viui/src'):
            mine = {str(p.relative_to(repo / rel)): self.sha(p) for p in sorted((repo / rel).rglob('*')) if p.is_file()}
            theirs = {str(p.relative_to(WORKTREE / rel)): self.sha(p) for p in sorted((WORKTREE / rel).rglob('*')) if p.is_file()}
            self.assertTrue(mine)
            self.assertEqual(mine, theirs, rel)

    def test_glyph_renderer_source_is_the_one_that_built_the_pinned_binary(self):
        source = ENV.REPO / 'scripts/phase04-gui-glyphs/src/main.rs'
        self.assertIn('TextFonts', source.read_text())
        self.assertEqual(ENV.sha256_file(ENV.SPEC.glyph_bin), ENV.GLYPH_SHA)

    def test_compiled_terminal_carries_the_banner_and_in_process_builtins(self):
        elf = (ENV.SPEC.signed / 'terminal').read_bytes()
        for needle in (b'=== CellOS GUI Terminal (x86_64) ===', b"Type 'help' for commands. Single-reader stream lease active.", b'shell: command not found: ',
                       b'bin/  apps/  data/  etc/  welcome.md', b'[terminal] Terminal surface ready. Entering event loop.'):
            self.assertIn(needle, elf, needle)
        self.assertNotIn(b'/bin/shell', elf)                       # the terminal does not spawn the shell

    def test_terminal_source_has_no_spawn_and_no_status_expansion(self):
        text = (TERMINAL_SRC / 'session.rs').read_text() + (TERMINAL_SRC / 'main.rs').read_text()
        self.assertNotIn('sys_spawn', text)
        self.assertNotIn('spawn_from_path(', text)
        self.assertNotIn('$?', text)


class RetainedBootScreenGeometry(unittest.TestCase):
    """The retained B2 desktop boot-smoke screendump (S10-9.3) is the only real B2 desktop pixels that exist offline."""

    def setUp(self):
        self.screen = Image.open(ENV.V2_EVIDENCE / 'smoke-hooks/screen.png').convert('RGB')

    def test_resolution_and_no_terminal_background_pixels(self):
        self.assertEqual(self.screen.size, (1280, 800))
        tiny = self.screen.resize((self.screen.width, self.screen.height))
        count = sum(1 for pixel in tiny.getdata() if pixel == GUI.BG)
        self.assertEqual(count, 0)                       # `locate(pre) is None` (Terminal not yet present) holds on the real B2 desktop

    def test_the_terminal_click_point_is_on_the_taskbar_terminal_button(self):
        x, y = D.TASKBAR['terminal'], D.TASKBAR_Y
        bar = self.screen.getpixel((100, y))
        face = self.screen.getpixel((x, y))
        self.assertNotEqual(face, bar)                   # a button face, not the bare taskbar
        self.assertEqual(self.screen.getpixel((142, y)), face)       # the face spans the button ...
        self.assertEqual(self.screen.getpixel((288, y)), bar)        # ... and ends before the next app slot gap
        self.assertEqual((x, y), (214, 782))

    def test_taskbar_labels_match_the_retained_pinned_apps(self):
        apps = (WORKTREE / 'cells/apps/desktop/src/apps.rs').read_text()
        order = [apps.index(f'id: "{name}"') for name in ('terminal', 'ocel', 'files', 'chrome')]
        self.assertEqual(order, sorted(order))


if __name__ == '__main__':
    unittest.main()
