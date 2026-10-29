#!/usr/bin/env python3
"""Boot + drive + judge the private Phase04 GUI Terminal image (S3-gui).

Called only by scripts/qemu-phase04-gui-test.sh, after the private image exists. It owns
exactly one QEMU process (never touches any other VM), talks to it through a private unix
QMP socket and a private unix serial socket, and writes only under the owned work dir.

Oracle (see the runner README for the full statement):
  * every judged frame is a QMP `screendump` of the guest scan-out, taken after the
    compositor composited the Terminal window;
  * the Terminal window (720x480, opaque) is located by exact colour, then compared pixel
    for pixel against `phase04-gui-glyphs`, a host renderer that reuses the Terminal's own
    `ostd::typography::TextFonts` (JetBrains Mono Regular 14 px) and geometry. Expected
    frames are built from a transcript model of what `cellbox sh` is specified to print, so
    a match proves the exact text, not "something changed";
  * the cursor block is located independently by its colour and must sit at the model's
    cursor cell; the cursor cell itself is excluded from the exact comparison;
  * every positive match has negative controls: the same frame must NOT match a transcript
    with one character changed, and the frame from before the action must NOT match the
    expected after-state.
  * S5-2 `cellos-vt1` screens are judged the same way on styled expected screens (the
    glyph helper's `styled` mode: colours, bold, inverse), authored below from the
    emulator contract, never by running an emulator.
  * S5-4: the hostile-output transcript (rows from scripts/phase05_vt_hostile.py) is judged
    the same way.
  * S6-4: the interactive vitui `top` (TOP-0..5) is judged by structure between exact prompt
    screens: cell colours of the title/status bars, ink in the last column, border
    continuity scans and the highlight band, each with a control on the prompt screen.
Tolerance: zero differing pixels (exact). A match with any difference is a failure.
"""

import argparse
import hashlib
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

from PIL import Image, ImageChops

# S5-4 hostile output fixture: bytes (packaged by the runner) and expected rows, one source.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase05_vt_hostile import PATH as VT_HOSTILE, VISIBLE as VT_HOSTILE_ROWS  # noqa: E402

WIN_W, WIN_H = 720, 480
CELL_W, CELL_H, PAD_X, PAD_Y = 9, 18, 8, 6
# The Terminal's grid (terminal::grid_dims): whole cells right of PAD_X and below PAD_Y,
# 79 x 26 at 720x480. Every cell of it is inside the window.
COLS, ROWS = (WIN_W - PAD_X) // CELL_W, (WIN_H - PAD_Y) // CELL_H
BG = (18, 20, 26)
PROMPT = "Cellos > "
BANNER = "=== Cellos shell ready \u2014 type 'help' for commands ==="
STDERR_ITERATIONS = 10
MIN_NEGATIVE_DIFF = 10  # a one-character change must differ by at least this many pixels

QCODE = {c: c for c in "abcdefghijklmnopqrstuvwxyz0123456789"}
QCODE.update({" ": "spc", "-": "minus", "/": "slash", ".": "dot", "=": "equal", "_": ("minus", True),
              "$": ("4", True), "?": ("slash", True), "|": ("backslash", True), ">": ("dot", True)})

# ---- C5 (Files on the VFS, Spotlight without power actions): geometry and colours --------
# Every value is the guest's own constant; the expected pixels are rendered by the glyph
# helper's `ops` mode, which replays viui's fill/stroke/label/button primitives.
FILES_ORG, FILES_W, FILES_H = (100, 100), 800, 600  # files/src/main.rs: 800x600, move_to(100, 100)
SPOT_ORG, SPOT_W, SPOT_H = (400, 100), 480, 320     # desktop main.rs ((1280-480)/2, 100); spotlight.rs
FILES_TASKBAR = (448 + 74, 800 - 36 + 4 + 14)       # third pinned app (taskbar.rs: apps_x 140, step 154)
SPOT_BUTTON = (8 + 42, 800 - 36 + 4 + 14)           # taskbar "CellOS" button opens Spotlight
# Visible strip of the Terminal's title tab (137..377 x 73..100), right of the Files tab
# (97..337, transparent beyond it) and above both tab controls (y 79..93): a raise, nothing else.
TERM_TAB = (355, 75)
# Files status bar below the Terminal frame (Terminal ends at y 583): no hit target in Files.
FILES_FOCUS = (500, 690)
PARK = (1200, 40)                                   # pointer parked over no checked region


def rgba(r, g, b, a=255):
    return "%02x%02x%02x%02x" % (r, g, b, a)


# viui::theme::LIGHT_THEME (Files)
L_BG, L_SURFACE, L_BORDER = rgba(245, 245, 248), rgba(255, 255, 255), rgba(215, 218, 224)
L_TEXT, L_TEXT2, L_ACCENT = rgba(20, 20, 28), rgba(95, 100, 115), rgba(35, 90, 215)
L_SELECTED = rgba(35, 90, 215, 60)                  # list_selected_bg = accent.with_alpha(60)
# desktop/src/draw.rs theme (Spotlight)
D_MODAL_BG, D_MODAL_BORDER, D_TASKBAR_BORDER = rgba(22, 24, 30), rgba(65, 70, 85), rgba(50, 54, 66)
D_BTN_NORMAL, D_BTN_HOVER, D_BTN_ACTIVE = rgba(38, 41, 52), rgba(52, 57, 72), rgba(68, 75, 96)
D_CYAN, D_BLUE, D_TEXT, D_MUTED = rgba(0, 200, 220), rgba(70, 130, 245), rgba(240, 242, 248), rgba(140, 145, 160)
# Judged regions, in surface coordinates (x0, y0, x1, y1): the toolbar path label, the whole
# ListView pane (header + every row slot) and the status bar; Spotlight's results area.
FILES_REGIONS = {"title": (48, 10, 600, 34), "list": (160, 44, 800, 576), "status": (0, 576, 800, 600)}
SPOT_REGIONS = {"results": (1, 77, 479, 292)}
SYSTEM_ACTIONS = [("[T]", "Appearance Theme", "Cycle Light / Dark / Paper", "Cycle")]
REMOVED_POWER_ACTIONS = [("[R]", "Reboot System", "Authorized restart after session consent", "Reboot"),
                         ("[P]", "Power Off", "Safely shut down the computer", "Shut Down")]

# ---- S5-2 (cellos-vt1 screen): fixtures built by the runner; expected screens authored
# here from the emulator contract (CUP absolute, ED/EL with the current background, LF
# implies CR, ?1049 alternate screen with saved cursor, SGR palette/bold/inverse), not by
# running an emulator. A styled row is a list of spans (col, text, (fg, bg, flags)) with
# fg/bg `d` (default), `pN` (indexed) or `RRGGBB`, flags `-` or any of b/i/w.
DEF = ("d", "d", "-")
VT_ALT_ENTER, VT_ALT_EXIT = "/p04-output/vt-alt-enter.bin", "/p04-output/vt-alt-exit.bin"
VT_SAMPLE = "/p04-output/vt-sample.bin"
# vt-alt-enter.bin: ?1049h; CUP 1;1 bold white-on-magenta " ALT SCREEN "; CUP 3;5 red
# "alt-red", " ", inverse "alt-inverse"; CUP 5;1 -> the shell prompt lands on alt row 4.
VT_ALT_ROWS = {0: [(0, " ALT SCREEN ", ("p15", "p5", "b"))],
               2: [(4, "alt-red", ("p1", "d", "-")), (12, "alt-inverse", ("d", "d", "i"))]}
VT_ALT_PROMPT_ROW = 4
# vt-sample.bin final screen: ED 2 clears; row 0 "JUNK-ROW-0" and row 1 "JUNK00" are erased
# by ED 1 at (1,5); rows 6, 2, 5, 4, 3 are drawn in that order by CUP; row 8 EL 1 at col 9
# with bg 43 (bce); row 9 EL 0 at col 11; row 10 EL 2 then "2K" at col 2; row 12 ED 0 at
# col 4 (row 13 "more-junk" vanishes); the alternate-screen text never shows; row 0 title
# last (bold); "end-of-sample\n" on row 14 puts the prompt on row 15.
VT_SAMPLE_ROWS = {
    0: [(0, "cellos-vt1 sample", ("d", "d", "b"))],
    1: [(6, "ed1-kept", DEF)],
    2: [(0, "red", ("p1", "d", "-")), (4, "bold-yellow", ("p3", "d", "b")), (16, "white-on-red", ("p15", "p1", "-"))],
    3: [(0, " black-on-yellow ", ("p0", "p11", "-")), (18, "magenta95", ("p13", "d", "-"))],
    4: [(0, "idx208", ("p208", "d", "-")), (7, "on53", ("d", "p53", "-"))],
    5: [(0, "rgb-fg", ("fac864", "d", "-")), (7, "rgb-bg", ("d", "3c1e5a", "-"))],
    6: [(0, "inverse", ("d", "d", "i")), (8, "inv-red", ("p1", "d", "i")), (16, "bold-inv", ("p3", "d", "bi"))],
    8: [(0, " " * 10, ("d", "p3", "-")), (10, "keep-right", DEF)],
    9: [(0, "erase-tail:", DEF)],
    10: [(2, "2K", DEF)],
    12: [(0, "ed0-", DEF)],
    14: [(0, "end-of-sample", DEF)],
}
VT_SAMPLE_PROMPT_ROW = 15

# ---- S6-4 (interactive `top`, vitui theme in a GUI-kind session = truecolor) -------------
# Title-bar accent, status/key-hint surface, selected-row highlight (phase-06-s4 §3.7).
TOP_ACCENT, TOP_SURFACE, TOP_HIGHLIGHT = (0xcb, 0xa6, 0xf7), (0x31, 0x32, 0x44), (0x45, 0x47, 0x5a)
TOP_FIRST_DATA_ROW = 4  # rows 0 title, 1 status, 2 top border, 3 table header
# vitui palette background that `edit` paints under its text (TrueColor on the GUI).
EDIT_BG = (0x1e, 0x1e, 0x2e)


def file_icon(name, is_dir):
    """files/src/model.rs FileItem::new icon rule (only the kinds this proof lists)."""
    if is_dir:
        return "[D]"
    return "[M]" if name.endswith((".md", ".txt")) else "[?]"


class StepFailed(Exception):
    pass


class Serial(threading.Thread):
    def __init__(self, path):
        super().__init__(daemon=True)
        self.path, self.buf, self.lock, self.stop, self.conn = path, bytearray(), threading.Lock(), False, None

    def connect(self):
        for _ in range(200):
            try:
                c = socket.socket(socket.AF_UNIX)
                c.connect(self.path)
                self.conn = c
                return
            except OSError:
                time.sleep(0.1)
        raise StepFailed("QEMU serial socket never appeared")

    def run(self):
        while not self.stop:
            try:
                data = self.conn.recv(65536)
            except OSError:
                return
            if not data:
                return
            with self.lock:
                self.buf.extend(data)

    def raw(self):
        with self.lock:
            return bytes(self.buf)

    def text(self):
        return self.raw().replace(b"\x00", b"").decode("utf-8", "replace")


class Qmp:
    def __init__(self, path):
        self.sock = None
        for _ in range(200):
            try:
                s = socket.socket(socket.AF_UNIX)
                s.connect(path)
                self.sock = s
                break
            except OSError:
                time.sleep(0.1)
        if self.sock is None:
            raise StepFailed("QEMU QMP socket never appeared")
        self.file = self.sock.makefile("rwb", buffering=0)
        self.file.readline()  # greeting
        self.cmd("qmp_capabilities")

    def cmd(self, name, **args):
        msg = {"execute": name}
        if args:
            msg["arguments"] = args
        self.file.write(json.dumps(msg).encode() + b"\n")
        while True:
            line = self.file.readline()
            if not line:
                raise StepFailed("QMP closed")
            reply = json.loads(line)
            if "return" in reply:
                return reply["return"]
            if "error" in reply:
                raise StepFailed(f"QMP {name}: {reply['error']}")

    def keys(self, events):
        self.cmd("input-send-event", events=[
            {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": q}}} for q, down in events])

    def abs_move(self, x, y):
        self.cmd("input-send-event", events=[{"type": "abs", "data": {"axis": "x", "value": x}},
                                             {"type": "abs", "data": {"axis": "y", "value": y}}])

    def button(self, down):
        self.cmd("input-send-event", events=[{"type": "btn", "data": {"down": down, "button": "left"}}])


class Proof:
    def __init__(self, a):
        self.a = a
        self.work = Path(a.work)
        self.ev = Path(a.evidence)
        self.shots = self.ev / "screens"
        self.shots.mkdir(parents=True, exist_ok=True)
        self.tmp = self.work / "sock"
        self.checks = {}
        self.notes = []
        self.counter = 0
        self.expected_cache = {}
        self.org = None
        self.rendered = {}
        self.clicks = 0

    # ---- bookkeeping -------------------------------------------------------------
    def chk(self, name, ok, why="", info=False):
        self.checks[name] = {"pass": bool(ok), "detail": why if not ok else (why or ""), "informational": info}
        print(("PASS " if ok else ("INFO-FAIL " if info else "FAIL ")) + name + ("" if ok else "  <- " + str(why)), flush=True)

    def need(self, name, ok, why=""):
        self.chk(name, ok, why)
        if not ok:
            raise StepFailed(f"{name}: {why}")

    # ---- host renderer ------------------------------------------------------------
    def render(self, rows):
        key = tuple(rows)
        if key not in self.rendered:
            rows_file = self.work / "scratch" / "rows.txt"
            rows_file.write_text("\n".join(rows), encoding="utf-8")
            raw = subprocess.run([self.a.glyph_bin, str(WIN_W), str(WIN_H), str(rows_file)],
                                 check=True, capture_output=True).stdout
            self.rendered[key] = Image.frombytes("RGB", (WIN_W, WIN_H), raw)
        return self.rendered[key]

    def render_ops(self, w, h, ops):
        """A w x h surface painted by the viui primitives in `ops` (glyph helper `ops` mode)."""
        key = (w, h, tuple(ops))
        if key not in self.rendered:
            ops_file = self.work / "scratch" / "ops.txt"
            ops_file.write_text("\n".join("\t".join(str(v) for v in op) for op in ops), encoding="utf-8")
            raw = subprocess.run([self.a.glyph_bin, "ops", str(w), str(h), str(ops_file)],
                                 check=True, capture_output=True).stdout
            self.rendered[key] = Image.frombytes("RGB", (w, h), raw)
        return self.rendered[key]

    def render_styled(self, screen):
        """A Terminal window painted from a styled screen (glyph helper `styled` mode)."""
        key = ("styled", tuple(tuple(spans) for spans in screen))
        if key not in self.rendered:
            screen_file = self.work / "scratch" / "screen.txt"
            screen_file.write_text("\n".join(f"{r}\t{c}\t{fg}\t{bg}\t{flags}\t{text}"
                                             for r, spans in enumerate(screen) for c, text, (fg, bg, flags) in spans),
                                   encoding="utf-8")
            raw = subprocess.run([self.a.glyph_bin, "styled", str(WIN_W), str(WIN_H), str(screen_file)],
                                 check=True, capture_output=True).stdout
            self.rendered[key] = Image.frombytes("RGB", (WIN_W, WIN_H), raw)
        return self.rendered[key]

    def text_row_height(self, face, size):
        """`(TextFonts::line_height(face, size) + 1.0) as u32`, as spotlight.rs sizes its rows."""
        key = ("lineheight", face, size)
        if key not in self.rendered:
            out = subprocess.run([self.a.glyph_bin, "lineheight", face, str(size)],
                                 check=True, capture_output=True, text=True).stdout.split()
            self.rendered[key] = int(out[1])
        return self.rendered[key]

    # ---- QEMU ---------------------------------------------------------------------
    def grab(self):
        ppm = self.tmp / "dump.ppm"
        ppm.unlink(missing_ok=True)
        self.qmp.cmd("screendump", filename=str(ppm), format="ppm")
        for _ in range(100):
            if ppm.is_file() and ppm.stat().st_size > 1000:
                time.sleep(0.05)
                break
            time.sleep(0.05)
        img = Image.open(ppm).convert("RGB")
        img.load()
        ppm.unlink(missing_ok=True)
        return img

    def shot(self, label, img=None):
        """Grab (or reuse) a frame and save it as evidence."""
        img = img if img is not None else self.grab()
        self.counter += 1
        img.save(self.shots / f"{self.counter:02d}-{label}.png")
        return img

    def wait_for(self, pred, what, timeout, interval=1.0):
        end, img = time.time() + timeout, None
        while time.time() < end:
            img = self.grab()
            result = pred(img)
            if result:
                return img, result
            time.sleep(interval)
        if img is not None:
            self.shot("timeout-" + re.sub("[^a-z0-9]+", "-", what.lower()), img)
        raise StepFailed(f"timeout waiting for {what}")

    def wait_serial(self, needle, timeout, what):
        end = time.time() + timeout
        while time.time() < end:
            if needle in self.serial.text():
                return
            if self.proc.poll() is not None:
                raise StepFailed(f"QEMU exited while waiting for {what}")
            time.sleep(0.5)
        raise StepFailed(f"timeout waiting for serial marker: {what}")

    def type_text(self, text, gap=0.12):
        for ch in text:
            q = QCODE[ch]
            shift = isinstance(q, tuple)
            if shift:
                q = q[0]
            ev = ([("shift", True)] if shift else []) + [(q, True), (q, False)] + ([("shift", False)] if shift else [])
            self.qmp.keys(ev)
            time.sleep(gap)

    def tap(self, q, gap=0.3):
        self.qmp.keys([(q, True), (q, False)])
        time.sleep(gap)

    def ctrl_c(self):
        self.qmp.keys([("ctrl", True), ("c", True), ("c", False), ("ctrl", False)])

    def click_px(self, x, y):
        self.clicks += 1
        self.qmp.abs_move(int(x * 32768 / self.sw), int(y * 32768 / self.sh))
        time.sleep(0.6)
        self.qmp.button(True)
        time.sleep(0.15)
        self.qmp.button(False)
        time.sleep(0.6)

    def combo(self, *keys, gap=0.4):
        """Press `keys` together (modifiers first), release in reverse order."""
        self.qmp.keys([(k, True) for k in keys] + [(k, False) for k in reversed(keys)])
        time.sleep(gap)

    def park(self):
        self.qmp.abs_move(int(PARK[0] * 32768 / self.sw), int(PARK[1] * 32768 / self.sh))
        time.sleep(1)

    # ---- pixel judgement ----------------------------------------------------------
    def locate(self, img):
        """Window origin: the exact bounding box of every pixel equal to the Terminal bg."""
        mask = ImageChops.difference(img, Image.new("RGB", img.size, BG)).convert("L").point(lambda v: 0 if v else 255)
        box = mask.getbbox()
        if box is None or (box[2] - box[0], box[3] - box[1]) != (WIN_W, WIN_H):
            return None
        return box[0], box[1]

    def window(self, img):
        org = self.locate(img)
        if org is None:
            return None, None
        if self.org is None:
            self.org = org
        return org, img.crop((org[0], org[1], org[0] + WIN_W, org[1] + WIN_H))

    @staticmethod
    def cursor_of(win):
        """Cursor block = the only cyan pixels (G - R large). Returns (col, row) or None."""
        px, xs, ys = win.load(), [], []
        for y in range(WIN_H):
            for x in range(WIN_W):
                r, g, _ = px[x, y]
                if g - r > 60:
                    xs.append(x)
                    ys.append(y)
        if not xs:
            return None
        x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
        if (x1 - x0 + 1, y1 - y0 + 1) != (CELL_W, CELL_H - 4) or (x0 - PAD_X) % CELL_W or (y0 - PAD_Y - 2) % CELL_H:
            return None
        return (x0 - PAD_X) // CELL_W, (y0 - PAD_Y - 2) // CELL_H

    def diff(self, win, rows, first=0, last=None, cursor=None):
        """(differing pixels, max channel delta) of rows `first..last` against the host render.

        `rows[i]` is the expected text of grid row `first + i`. The cursor cell rectangle is
        excluded; the band outside [first, last] is not judged."""
        expected = self.render([""] * first + list(rows))
        last = first + len(rows) - 1 if last is None else last
        top, bottom = (0 if first == 0 else PAD_Y + first * CELL_H), (WIN_H if last >= ROWS - 1 else PAD_Y + (last + 1) * CELL_H)
        a = win.crop((0, top, WIN_W, bottom))
        b = expected.crop((0, top, WIN_W, bottom))
        d = ImageChops.difference(a, b)
        if cursor is not None:
            cx, cy = PAD_X + cursor[0] * CELL_W, PAD_Y + cursor[1] * CELL_H + 2 - top
            d.paste((0, 0, 0), (cx, cy, cx + CELL_W, cy + CELL_H - 4))
        hist = d.convert("L").histogram()
        return sum(hist[1:]), max(e[1] for e in d.getextrema())

    def match_full(self, img, rows, cursor):
        org, win = self.window(img)
        if win is None:
            return None
        found = self.cursor_of(win)
        if found != cursor:
            return {"cursor": found, "diff": None}
        pixels, worst = self.diff(win, rows, 0, ROWS - 1, cursor)
        return {"cursor": found, "diff": pixels, "max": worst}

    def diff_styled(self, win, screen, cursor):
        """(differing pixels, max channel delta) of the whole window against the styled
        render of `screen`; the cursor cell rectangle is excluded."""
        d = ImageChops.difference(win, self.render_styled(screen))
        if cursor is not None:
            cx, cy = PAD_X + cursor[0] * CELL_W, PAD_Y + cursor[1] * CELL_H + 2
            d.paste((0, 0, 0), (cx, cy, cx + CELL_W, cy + CELL_H - 4))
        hist = d.convert("L").histogram()
        return sum(hist[1:]), max(e[1] for e in d.getextrema())

    def match_styled(self, img, screen, cursor):
        org, win = self.window(img)
        if win is None:
            return None
        found = self.cursor_of(win)
        if found != cursor:
            return {"cursor": found, "diff": None}
        pixels, worst = self.diff_styled(win, screen, cursor)
        return {"cursor": found, "diff": pixels, "max": worst}

    def ink_rows(self, win, cursor):
        bgimg = Image.new("RGB", (WIN_W, WIN_H), BG)
        d = ImageChops.difference(win, bgimg)
        if cursor is not None:
            cx, cy = PAD_X + cursor[0] * CELL_W, PAD_Y + cursor[1] * CELL_H + 2
            d.paste((0, 0, 0), (cx, cy, cx + CELL_W, cy + CELL_H - 4))
        out = []
        for r in range(ROWS):
            band = d.crop((0, PAD_Y + r * CELL_H, WIN_W, PAD_Y + (r + 1) * CELL_H)).convert("L")
            out.append(band.getbbox() is not None)
        return out

    # ---- transcript model -----------------------------------------------------------
    @staticmethod
    def pad(rows):
        """The grid for a transcript: the last ROWS lines (the grid scrolls up one line per
        newline at the bottom; the line being edited is always the last one), blank-padded."""
        rows = list(rows)[-ROWS:]
        return rows + [""] * (ROWS - len(rows))

    @staticmethod
    def mcursor(rows):
        """Cursor cell for a transcript: end of its last line, on the last used grid row."""
        return len(rows[-1]), min(len(rows), ROWS) - 1

    def expect_screen(self, rows, label, timeout):
        """Wait until the window equals `rows` exactly (cursor on the last row), return img."""
        cursor = self.mcursor(rows)
        full = self.pad(rows)
        last = {}

        def pred(i):
            m = self.match_full(i, full, cursor)
            last.update(m or {"window": None})
            return m is not None and m.get("diff") == 0
        try:
            got, _ = self.wait_for(pred, label, timeout)
        except StepFailed:
            self.notes.append(f"{label}: last observation {last}")
            raise
        return got

    def expect_styled(self, screen, cursor, label, timeout):
        """Wait until the window equals the styled `screen` exactly with the cursor at `cursor`."""
        last = {}

        def pred(i):
            m = self.match_styled(i, screen, cursor)
            last.clear()
            last.update(m or {"window": None})
            return m is not None and m.get("diff") == 0
        try:
            got, _ = self.wait_for(pred, label, timeout)
        except StepFailed:
            self.notes.append(f"{label}: last observation {last}")
            raise
        return got

    # ---- scenario -------------------------------------------------------------------
    def run(self):
        a = self.a
        self.serial_path, self.qmp_path = str(self.tmp / "serial.sock"), str(self.tmp / "qmp.sock")
        argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", a.accel,
                "-cpu", "qemu64,+pdpe1gb" if a.accel == "tcg" else "host", "-m", "256M", "-display", "none",
                "-chardev", f"socket,id=ser0,path={self.serial_path},server=on,wait=off", "-serial", "chardev:ser0",
                "-qmp", f"unix:{self.qmp_path},server=on,wait=off", "-monitor", "none",
                "-cdrom", a.iso, "-boot", "d", "-no-reboot", "-nic", "none",
                "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"]
        (self.ev / "qemu-argv.txt").write_text(" ".join(argv).replace(str(self.work), "$WORK") + "\n")
        for p in (self.serial_path, self.qmp_path):
            Path(p).unlink(missing_ok=True)
        self.proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.serial = Serial(self.serial_path)
        try:
            self.serial.connect()
            self.serial.start()
            self.qmp = Qmp(self.qmp_path)
            self.scenarios()
        except StepFailed as e:
            self.chk("run-aborted", False, str(e))
        finally:
            self.finish_qemu()
        self.uart_checks()
        return self.write_verdict()

    def finish_qemu(self):
        if self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
        self.serial.stop = True
        raw = self.serial.raw()
        (self.ev / "serial.raw.log").write_bytes(raw)
        (self.ev / "serial.log").write_text(self.serial.text())

    def scenarios(self):
        a = self.a
        # ---- GUI-1/2: boot, (click-)launch, prompt ---------------------------------
        self.wait_serial("[desktop] Ready. Entering event loop.", a.window, "desktop ready")
        time.sleep(3)
        pre = self.shot("desktop-before-terminal")
        size = pre.size
        self.sw, self.sh = size
        self.need("screen-1280x800", size == (1280, 800), size)
        if a.launch == "click":
            self.need("no-window-before-launch", self.locate(pre) is None, "a bg-coloured 720x480 rectangle exists before launch")
            # First pinned taskbar app = Terminal (apps.rs catalog; taskbar.rs layout: 1280 wide
            # => paged, apps_x 140, button 148x28 at y = 800 - 36 + 4).
            self.click_px(140 + 74, 800 - 36 + 4 + 14)
        else:
            # Autoload: init launches /bin/terminal at boot, so its window legitimately may
            # exist already in this frame. The mode-appropriate precondition is that the driver
            # issues no taskbar (or any) click before the window shows up on its own.
            self.need("autoload-no-click-before-window", self.clicks == 0, f"{self.clicks} click(s) issued before the autoload window")
        self.wait_serial("[terminal] Terminal surface ready. Entering event loop.", a.window, "terminal ready")
        rows = ["", BANNER, PROMPT]
        shot_prompt = self.expect_screen(rows, "initial prompt in the window", 240)
        self.shot("prompt", shot_prompt)
        org = self.org
        self.chk("window-located", org is not None, f"origin={org}")
        if a.launch == "click":
            # Pixel control without the window: the pre-launch frame has no such rectangle and
            # the same region of it does not match the prompt render.
            region = pre.crop((org[0], org[1], org[0] + WIN_W, org[1] + WIN_H))
            d, _ = self.diff(region, self.pad(rows), 0, ROWS - 1, self.mcursor(rows))
            self.chk("GUI-2-prompt-matches-exactly", True, "diff=0 (expect_screen)")
            self.chk("GUI-2-control-pre-launch-region-differs", d > 1000, f"{d} differing pixels")
        else:
            # No pre-launch frame exists without a launch; the window must have come up with
            # zero clicks issued, and the exact prompt screen was already matched above.
            self.chk("GUI-2-prompt-matches-exactly", True, "diff=0 (expect_screen)")
            self.chk("GUI-2-autoload-window-without-click", self.clicks == 0, f"{self.clicks} click(s) issued before the window matched")
        wrong = rows[:-1] + ["Cellos # "]
        _, win = self.window(shot_prompt)
        d2, _ = self.diff(win, self.pad(wrong), 0, ROWS - 1, (len(PROMPT), 2))
        self.chk("GUI-2-control-wrong-prompt-differs", d2 >= MIN_NEGATIVE_DIFF, f"{d2} differing pixels")
        cur = self.cursor_of(win)
        self.chk("GUI-2-cursor-cell", cur == (len(PROMPT), 2), cur)

        # Focus: a real click inside the window, as a user would do (also closes the focus race).
        self.click_px(org[0] + 360, org[1] + 300)
        self.qmp.abs_move(int(1200 * 32768 / self.sw), int(40 * 32768 / self.sh))  # park the pointer outside the window
        time.sleep(2)

        model = list(rows)
        state = {"img": shot_prompt}

        def command(name, text, outputs, timeout=180, diagnose=None):
            """Type `text`, press Enter, verify the exact screen; controls. Returns the matched
            alternative index.

            `outputs` lists the acceptable exact outcomes, each the list of output rows
            (the shell then shows a fresh prompt). Nothing else passes. `diagnose`, if given,
            is a transcript tail (list of rows after the typed line) that is NOT acceptable;
            it is only used to name a failing screen in an INFO marker."""
            before = state["img"]
            typed_rows = model[:-1] + [model[-1] + text]
            self.type_text(text)
            typed = self.expect_screen(typed_rows, f"{name}: typed line", 120)
            self.shot(f"{name}-typed", typed)
            self.chk(f"{name}-typed-line-exact", True, "diff=0")
            self.tap("ret")
            cands = [typed_rows + lines + [PROMPT] for lines in outputs]
            seen = {}

            def pred(i):
                for n, cand in enumerate(cands):
                    m = self.match_full(i, self.pad(cand), self.mcursor(cand))
                    if m and m.get("diff") == 0:
                        return n + 1
                    seen[n] = m
                return 0
            try:
                after, which = self.wait_for(pred, f"{name}: output", timeout)
            except StepFailed:
                self.notes.append(f"{name}: last observation {seen}")
                self.chk(f"{name}-output-exact", False, "no acceptable exact screen")
                if diagnose is not None:
                    last = self.grab()
                    cand = typed_rows + diagnose
                    m = self.match_full(last, self.pad(cand), self.mcursor(cand)) if self.window(last)[1] is not None else None
                    self.shot(f"{name}-FAILED-final", last)
                    self.chk(f"INFO-{name}-observed", m is not None and m.get("diff") == 0,
                             "PROMPT-THEN-STDERR exact screen (stderr drawn over/after the next prompt)"
                             if m is not None and m.get("diff") == 0 else f"unclassified final screen {m}", info=True)
                raise
            which -= 1
            self.shot(f"{name}-after", after)
            final = cands[which]
            self.chk(f"{name}-output-exact", True, f"alternative {which}: {outputs[which]}")
            # negative controls
            dprev = self.match_full(before, self.pad(final), self.mcursor(final))
            self.chk(f"{name}-control-before-frame-differs", dprev is not None and (dprev["diff"] is None or dprev["diff"] >= MIN_NEGATIVE_DIFF),
                     dprev)
            altered = list(final)
            for i in range(len(typed_rows), len(final) - 1):
                if altered[i] and i >= len(final) - ROWS:
                    altered[i] = altered[i][:-1] + ("x" if altered[i][-1] != "x" else "y")
                    break
            else:
                # No output row (a silent command): change the typed command line instead.
                i = len(typed_rows) - 1
                altered[i] = altered[i][:-1] + ("x" if altered[i][-1] != "x" else "y")
            _, win = self.window(after)
            dalt, _ = self.diff(win, self.pad(altered), 0, ROWS - 1, self.mcursor(final))
            self.chk(f"{name}-control-one-char-change-differs", dalt >= MIN_NEGATIVE_DIFF, f"{dalt} differing pixels")
            dc = self.window(after)[1]
            ch = ImageChops.difference(before.crop((org[0], org[1], org[0] + WIN_W, org[1] + WIN_H)), dc).convert("L").histogram()
            self.chk(f"{name}-before-after-window-pixels-changed", sum(ch[1:]) >= MIN_NEGATIVE_DIFF, f"{sum(ch[1:])} pixels changed")
            model[:] = final
            state["img"] = after
            return which

        # ---- GUI-3: stock output -----------------------------------------------------
        command("GUI-3-echo", "echo gui-proof-1", [["gui-proof-1"]])
        command("GUI-3-stock-echo", "/bin/echo gui-proof-ext", [["gui-proof-ext"]])
        command("GUI-3-stock-ls", "/bin/ls /gui-list",
                [[".", "..", "alpha-entry", "beta-entry"], [".", "..", "beta-entry", "alpha-entry"]])
        command("GUI-3-stock-cat", "/bin/cat /gui-text/note.txt", [["gui-cat-line-one", "gui-cat-line-two"]])
        # ---- GUI-4: stderr ------------------------------------------------------------
        # Strict and repeated: EVERY one of N consecutive failing commands must show the
        # stderr line fully before the next prompt, as an exact-pixel screen. Only that screen
        # passes; a prompt-then-stderr screen (the pre-fix Terminal drained stdout before
        # stderr, so the line was drawn over the prompt row) fails the run immediately. The
        # transcript model scrolls like the grid, so every iteration is exact.
        err = "ls: /gui-missing: no such directory"
        for n in range(1, STDERR_ITERATIONS + 1):
            command(f"GUI-4-stderr-{n:02d}", "/bin/ls /gui-missing", [[err]], timeout=90,
                    diagnose=[PROMPT + err, ""])
        self.chk("GUI-4-stderr-all-iterations-before-prompt", True,
                 f"{STDERR_ITERATIONS}/{STDERR_ITERATIONS} exact stderr-before-prompt screens")
        command("GUI-4-status-of-failure", "echo st=$?", [["st=1"]])

        # ---- C5: desktop <-> CLI parity on the same VFS service -------------------------
        self.c5(command)

        # ---- S5-2: cellos-vt1 screen (alternate screen, colours, addressing, erase) ------
        self.vt(model, state)

        # ---- S5-3 option C: window close -> release -> automatic re-create -> re-attach ----
        self.s53c_reattach(command, model, state)

        # ---- S5-4: hostile output ---------------------------------------------------------------
        self.hostile(model, state)

        # ---- S6-4: interactive `top` (vitui/Ratatui) -----------------------------------------
        self.top(command, model, state)

        # ---- S8-5: interactive `edit` (vitui) ----------------------------------------------
        self.edit(command, model, state)

        # ---- GUI-4: Ctrl-C ---------------------------------------------------------------
        self.type_text("top --records --frames 100000 | to jsonl | /bin/p03-filter cat")
        self.tap("ret")

        def producing(i):
            org2, w = self.window(i)
            if w is None:
                return False
            # Producer JSON lines are longer than the 79-column grid; nothing typed or printed
            # earlier reaches the last three columns, so ink there on >= 3 rows is the producer.
            cur2 = self.cursor_of(w)
            right = w.crop((PAD_X + (COLS - 3) * CELL_W, 0, WIN_W, WIN_H))
            rows_with_ink = 0
            for r in range(ROWS):
                band = ImageChops.difference(right.crop((0, PAD_Y + r * CELL_H, right.width, PAD_Y + (r + 1) * CELL_H)),
                                             Image.new("RGB", (right.width, CELL_H), BG)).convert("L")
                rows_with_ink += band.getbbox() is not None
            return rows_with_ink >= 3
        flowing, _ = self.wait_for(producing, "ctrl-c producer output visible", 240)
        self.shot("ctrl-c-producer-flowing", flowing)
        time.sleep(3)
        flowing2 = self.shot("ctrl-c-producer-flowing-later")
        w1, w2 = self.window(flowing)[1], self.window(flowing2)[1]
        moved = sum(ImageChops.difference(w1, w2).convert("L").histogram()[1:])
        self.chk("GUI-4-control-producer-still-running-before-ctrl-c", moved >= MIN_NEGATIVE_DIFF, f"{moved} pixels changed in 3 s")
        self.ctrl_c()

        def settled_prompt(i):
            org2, w = self.window(i)
            if w is None:
                return False
            cur2 = self.cursor_of(w)
            return cur2 is not None and cur2[0] == len(PROMPT) and self.diff(w, [PROMPT], cur2[1], cur2[1], cur2)[0] == 0
        back, _ = self.wait_for(settled_prompt, "prompt after Ctrl-C", 120)
        time.sleep(4)
        later = self.shot("ctrl-c-recovered")
        wb, wl = self.window(back)[1], self.window(later)[1]
        still = sum(ImageChops.difference(wb, wl).convert("L").histogram()[1:])
        self.chk("GUI-4-ctrl-c-output-stops-prompt-recovered", still == 0, f"{still} pixels changed 4 s after the prompt returned")
        self.type_text("echo cc=$?")
        self.tap("ret")
        want = ["Cellos > echo cc=$?", "cc=130", PROMPT]

        def status_tail(i):
            org2, w = self.window(i)
            if w is None:
                return False
            c = self.cursor_of(w)
            if c is None or c[0] != len(PROMPT) or c[1] < 2:
                return False
            return self.diff(w, want, c[1] - 2, c[1], c)[0] == 0
        done, _ = self.wait_for(status_tail, "exact status line after Ctrl-C", 120)
        self.shot("ctrl-c-status", done)
        w = self.window(done)[1]
        c = self.cursor_of(w)
        self.chk("GUI-4-ctrl-c-status-130-exact", True, f"rows {c[1]-2}..{c[1]} diff=0")
        bad = ["Cellos > echo cc=$?", "cc=0", PROMPT]
        dbad, _ = self.diff(w, bad, c[1] - 2, c[1], c)
        self.chk("GUI-4-control-status-0-differs", dbad >= MIN_NEGATIVE_DIFF, f"{dbad} differing pixels")

        # ---- window close control (informational) ------------------------------------------
        try:
            self.type_text("exit")
            self.tap("ret")

            time.sleep(8)
            self.tap("esc")
            gone, _ = self.wait_for(lambda i: self.locate(i) is None, "window gone after exit + Esc", 120)
            self.shot("window-closed", gone)
            self.chk("window-close-control", True, "no Terminal-coloured window rectangle remains", info=True)
        except StepFailed as e:
            self.chk("window-close-control", False, str(e), info=True)

    # ---- S5-3 option C: same shell across a window close inside the live Terminal Cell -----
    def s53c_reattach(self, command, model, state):
        """phase-05-s3c-frontend-annex.md §7, decision A. The Terminal's close control inside
        the live Terminal Cell releases its frontend (the shell keeps a Detached session and
        logs that it observed Lost(4)); the Terminal then re-creates its window AT ONCE by
        itself and re-attaches the same shell with its holder-bound resume capability. This
        proves close -> ReleaseFrontend -> Detached -> ReattachFrontend in the same Cell with
        an automatic window re-create; a user-facing re-open of a running Terminal (desktop
        single-instance / taskbar) is NOT delivered and not claimed."""
        command("S53C-cd", "cd /tmp", [[]])
        command("S53C-pwd-before-close", "pwd", [["/tmp"]])
        org = self.org
        # Close control: tab = content - (3, 27); close box 14x14 at tab + (6, 6) (compositor
        # window_decoration::control_rect), clicked in its centre. The Terminal is on top.
        self.click_px(org[0] - 3 + 6 + 7, org[1] - 27 + 6 + 7)
        self.park()
        for needle, what in (
            ("[terminal] window closed; frontend released, shell detached", "frontend released on window close"),
            ("[terminal] window re-created; frontend re-attached to the same shell", "window re-created and frontend re-attached"),
            ("shell: frontend detached; session waits", "shell observed Lost(4) and waits"),
            ("shell: frontend re-attached; redraw", "shell observed the re-attach"),
        ):
            try:
                self.wait_serial(needle, 120, what)
                self.chk("S53C-serial:" + what.replace(" ", "-"), True)
            except StepFailed as e:
                self.chk("S53C-serial:" + what.replace(" ", "-"), False, str(e))
                raise
        text = self.serial.text()
        order = [text.find(n) for n in ("[terminal] window closed;", "shell: frontend detached;",
                                        "shell: frontend re-attached;")]
        self.chk("S53C-release-before-reattach", -1 not in order and order[0] < order[2] and order[1] < order[2], order)
        # The re-created window shows the same transcript; the shell redrew its prompt line.
        reopened = self.expect_screen(model, "re-created window shows the same transcript and prompt", 180)
        self.shot("s53c-reopened", reopened)
        self.chk("S53C-reopened-screen-exact", True, "diff=0")
        altered = model[:-2] + [model[-2][:-1] + ("x" if model[-2][-1] != "x" else "y"), model[-1]]
        _, win = self.window(reopened)
        dalt, _ = self.diff(win, self.pad(altered), 0, ROWS - 1, self.mcursor(model))
        self.chk("S53C-control-one-char-change-differs", dalt >= MIN_NEGATIVE_DIFF, f"{dalt} differing pixels")
        self.chk("S53C-window-same-origin", self.locate(reopened) == org, self.locate(reopened))
        # Keyboard to the re-created window, as a user would do.
        self.click_px(org[0] + 360, org[1] + 300)
        self.park()
        time.sleep(2)
        state["img"] = reopened
        # Same shell: cwd and history survive; nothing restarted (no new banner).
        command("S53C-pwd-after-reattach", "pwd", [["/tmp"]])
        before = state["img"]
        self.tap("up")
        recalled = model[:-1] + [model[-1] + "pwd"]
        img = self.expect_screen(recalled, "history Up recalls the last line after re-attach", 120)
        self.shot("s53c-history-recalled", img)
        self.chk("S53C-history-recalled-exact", True, "diff=0")
        self.tap("ret")
        final = recalled + ["/tmp", PROMPT]
        img = self.expect_screen(final, "recalled line runs in the same cwd", 120)
        self.shot("s53c-history-rerun", img)
        self.chk("S53C-history-rerun-exact", True, "diff=0")
        dprev = self.match_full(before, self.pad(final), self.mcursor(final))
        self.chk("S53C-control-before-frame-differs", dprev is not None and (dprev["diff"] is None or dprev["diff"] >= MIN_NEGATIVE_DIFF), dprev)
        model[:] = final
        state["img"] = img
        self.chk("S53C-no-second-banner", self.serial.text().count("[terminal] Terminal surface ready. Entering event loop.") == 1)

    # ---- S5-2: cellos-vt1 styled screens ----------------------------------------------------
    @staticmethod
    def vt_screen(spans_by_row, prompt_row):
        """(styled screen, cursor): `spans_by_row` plus the shell prompt on `prompt_row`."""
        screen = [list(spans_by_row.get(r, ())) for r in range(ROWS)]
        screen[prompt_row].append((0, PROMPT, DEF))
        return screen, (len(PROMPT), prompt_row)

    @staticmethod
    def vt_swap(screen, row, new):
        """`screen` with the span of `row` that starts at column new[0] replaced by `new`."""
        out = [list(spans) for spans in screen]
        out[row] = [new if span[0] == new[0] else span for span in out[row]]
        assert out[row] != list(screen[row]), (row, new)
        return out

    def vt_plain(self, rows):
        """(styled screen, cursor) of a plain transcript (default colours, no attributes)."""
        return [[(0, r, DEF)] if r else [] for r in self.pad(rows)], self.mcursor(rows)

    def vt_step(self, name, text, base, after, state, controls):
        """`command()` on styled screens: type `text` at the cursor of the exact screen `base`
        = (screen, cursor), press Enter, wait for the exact screen `after` = (screen, cursor).
        Controls as in `command()`: before frame, `controls` (label, styled screen) pairs,
        led by the one-character change, and the window pixels changed."""
        screen, (col, row) = base
        typed = [list(spans) for spans in screen]
        typed[row].append((col, text, DEF))
        before = state["img"]
        self.type_text(text)
        img = self.expect_styled(typed, (col + len(text), row), f"{name}: typed line", 120)
        self.shot(f"{name}-typed", img)
        self.chk(f"{name}-typed-line-exact", True, "diff=0")
        self.tap("ret")
        want, cursor = after
        try:
            got = self.expect_styled(want, cursor, f"{name}: output", 180)
        except StepFailed:
            self.chk(f"{name}-output-exact", False, "no exact styled screen")
            raise
        self.shot(f"{name}-after", got)
        self.chk(f"{name}-output-exact", True, "diff=0")
        dprev = self.match_styled(before, want, cursor)
        self.chk(f"{name}-control-before-frame-differs",
                 dprev is not None and (dprev["diff"] is None or dprev["diff"] >= MIN_NEGATIVE_DIFF), dprev)
        win = self.window(got)[1]
        for label, other in controls:
            d, _ = self.diff_styled(win, other, cursor)
            self.chk(f"{name}-control-{label}-differs", d >= MIN_NEGATIVE_DIFF, f"{d} differing pixels")
        org = self.org
        ch = ImageChops.difference(before.crop((org[0], org[1], org[0] + WIN_W, org[1] + WIN_H)), win).convert("L").histogram()
        self.chk(f"{name}-before-after-window-pixels-changed", sum(ch[1:]) >= MIN_NEGATIVE_DIFF, f"{sum(ch[1:])} pixels changed")
        state["img"] = got

    def vt(self, model, state):
        """S5-2 (D52-3): the GUI Terminal's cellos-vt1 screen on host-built fixtures."""
        def one(s):
            return s[:-1] + ("x" if s[-1] != "x" else "y")

        # VT-1: `cat` enters the alternate screen and leaves it up: a cleared alt buffer with
        # the fixture's styled content and the next prompt on alt row 4; no primary text.
        enter = "cat " + VT_ALT_ENTER
        typed1 = model[:-1] + [model[-1] + enter]
        alt = self.vt_screen(VT_ALT_ROWS, VT_ALT_PROMPT_ROW)
        inv = VT_ALT_ROWS[2][1]
        self.vt_step("VT-1-alt-enter", enter, self.vt_plain(model), alt, state,
                     [("one-char-change", self.vt_swap(alt[0], 2, (inv[0], one(inv[1]), inv[2])))])
        # VT-2: the exit fixture returns to the untouched primary transcript (typed VT-1 line
        # included) and restores the cursor saved on entry: the new prompt lands right below.
        leave = "cat " + VT_ALT_EXIT
        restored = typed1 + [PROMPT]
        self.vt_step("VT-2-alt-exit", leave, alt, self.vt_plain(restored), state,
                     [("one-char-change", self.vt_plain(typed1[:-1] + [one(typed1[-1]), PROMPT])[0])])
        model[:] = restored
        # VT-3: the primary sample (colours, bold, inverse, CUP out of order, ED/EL, alt screen).
        sample = self.vt_screen(VT_SAMPLE_ROWS, VT_SAMPLE_PROMPT_ROW)
        bold, inv = VT_SAMPLE_ROWS[2][1], VT_SAMPLE_ROWS[6][1]
        self.vt_step("VT-3-sample", "cat " + VT_SAMPLE, self.vt_plain(model), sample, state, [
            ("one-char-change", self.vt_swap(sample[0], 2, (bold[0], one(bold[1]), bold[2]))),
            ("bold-dropped", self.vt_swap(sample[0], 2, (bold[0], bold[1], (bold[2][0], bold[2][1], "-")))),
            ("inverse-dropped", self.vt_swap(sample[0], 6, (inv[0], inv[1], (inv[2][0], inv[2][1], "-")))),
        ])
        # VT-4: `clear` (ED 2 + CUP 1;1) leaves a plain prompt screen for the scenarios after.
        self.vt_step("VT-4-clear", "clear", sample, self.vt_plain([PROMPT]), state,
                     [("one-char-change", self.vt_plain([one(PROMPT)])[0])])
        model[:] = [PROMPT]

    # ---- S5-4: hostile output ---------------------------------------------------------------
    def hostile(self, model, state):
        """phase-05-s4-proposal.md S5-4a item 3 (GUI). From a cleared screen and an empty
        history, `cat` of the hostile fixture shows exactly its VISIBLE rows in default colours
        and the next prompt; a typed `echo` then runs exactly as typed and `history` lists
        exactly the cat, the echo and itself: no reply byte reached the shell's input."""
        def one(s):
            return s[:-1] + ("x" if s[-1] != "x" else "y")

        self.vt_step("HOSTILE-0-clear", "clear", self.vt_plain(model), self.vt_plain([PROMPT]), state,
                     [("one-char-change", self.vt_plain([one(PROMPT)])[0])])
        cleared = [PROMPT + "history -c", PROMPT]
        self.vt_step("HOSTILE-1-history-clear", "history -c", self.vt_plain([PROMPT]), self.vt_plain(cleared), state,
                     [("one-char-change", self.vt_plain([one(cleared[0]), PROMPT])[0])])
        # The fixture's output starts on the row below the typed command; every row is plain.
        cat = "cat " + VT_HOSTILE
        shown = cleared[:-1] + [PROMPT + cat] + VT_HOSTILE_ROWS + [PROMPT]
        screen, cursor = self.vt_plain(shown)
        p17 = shown.index("params17-plain")
        self.vt_step("HOSTILE-2-cat", cat, self.vt_plain(cleared), (screen, cursor), state, [
            ("one-char-change", self.vt_plain(shown[:-2] + [one(shown[-2]), PROMPT])[0]),
            ("params17-bold-red", self.vt_swap(screen, p17, (0, shown[p17], ("p1", "d", "b")))),
        ])
        # No reply bytes: the next typed line and its output are exactly what was typed.
        token = "hostile-ok"
        echoed = shown[:-1] + [PROMPT + "echo " + token, token, PROMPT]
        self.vt_step("HOSTILE-3-echo-as-typed", "echo " + token, (screen, cursor), self.vt_plain(echoed), state,
                     [("one-char-change", self.vt_plain(echoed[:-2] + [one(token), PROMPT])[0])])
        # History holds exactly the three commands typed since `history -c` (format "{:5}  {}").
        listed = ["cat " + VT_HOSTILE, "echo " + token, "history"]
        final = echoed[:-1] + [PROMPT + "history"] + [f"{n + 1:5}  {e}" for n, e in enumerate(listed)] + [PROMPT]
        uncleared = (echoed[:-1] + [PROMPT + "history"]
                     + [f"{n + 1:5}  {e}" for n, e in enumerate(["history -c"] + listed)] + [PROMPT])
        self.vt_step("HOSTILE-4-history", "history", self.vt_plain(echoed), self.vt_plain(final), state, [
            ("one-char-change", self.vt_plain(final[:-2] + [one(final[-2]), PROMPT])[0]),
            ("history-not-cleared", self.vt_plain(uncleared)[0]),
        ])
        model[:] = final

    # ---- S6-4: interactive `top` ------------------------------------------------------------
    @staticmethod
    def cell_box(col, row):
        """Pixel box of grid cell (col, row) in window coordinates."""
        return PAD_X + col * CELL_W, PAD_Y + row * CELL_H, PAD_X + (col + 1) * CELL_W, PAD_Y + (row + 1) * CELL_H

    @staticmethod
    def share(img, rgb):
        """Fraction of the pixels of `img` that are exactly `rgb`."""
        total = img.width * img.height
        return sum(n for n, colour in img.getcolors(total) if colour == rgb) / total

    def cell_share(self, win, col, row, rgb):
        return self.share(win.crop(self.cell_box(col, row)), rgb)

    def cell_inked(self, win, col, row):
        """Any pixel of the cell differs from the window background (a glyph or a cell colour)."""
        return any(colour != BG for _, colour in win.crop(self.cell_box(col, row)).getcolors(CELL_W * CELL_H))

    def band_rows(self, win):
        """Grid rows whose table width (columns 1..COLS-2) is mostly the highlight colour."""
        return [r for r in range(ROWS)
                if self.share(win.crop((PAD_X + CELL_W, PAD_Y + r * CELL_H, PAD_X + (COLS - 1) * CELL_W,
                                        PAD_Y + (r + 1) * CELL_H)), TOP_HIGHLIGHT) > 0.5]

    @staticmethod
    def blank_lines(win, box, vertical):
        """Pixel rows (`vertical`) or pixel columns of `box` whose pixels all equal the box's
        most common colour, in window coordinates."""
        region = win.crop(box)
        bg = max(region.getcolors(region.width * region.height))[1]
        px = region.load()
        if vertical:
            return [box[1] + y for y in range(region.height) if all(px[x, y] == bg for x in range(region.width))]
        return [box[0] + x for x in range(region.width) if all(px[x, y] == bg for y in range(region.height))]

    def top(self, command, model, state):
        """phase-06-s4-proposal.md §7.2 TOP-0..5: the interactive vitui `top` in the GUI
        Terminal, judged by structure (cell colours, glyph ink, the highlight band) between
        exact prompt screens. The window cannot be resized: resize is a terminal-runner claim."""
        def one(s):
            return s[:-1] + ("x" if s[-1] != "x" else "y")

        def step(name, pred, what, timeout=60):
            try:
                return self.wait_for(pred, what, timeout)[0]
            except StepFailed:
                self.chk(name, False, f"timeout: {what}")
                raise

        def band_at(row):
            def pred(i):
                w = self.window(i)[1]
                return w is not None and self.band_rows(w) == [row]
            return pred

        # TOP-0: the typed line, exact on the prompt screen.
        typed_rows = model[:-1] + [model[-1] + "top"]
        self.type_text("top")
        prompt = self.expect_screen(typed_rows, "TOP-0: typed line", 120)
        self.shot("TOP-0-typed", prompt)
        self.chk("TOP-0-typed-line-exact", True, "diff=0")
        pwin = self.window(prompt)[1]
        self.tap("ret")

        # TOP-1: the frame is the window grid (79x26): the title bar carries the accent on
        # every cell, the status row the surface colour, and the last column is inked on
        # every row (bars and the right border). Control: the prompt screen before `top` has
        # blank last-column cells.
        def drawn(i):
            w = self.window(i)[1]
            return w is not None and all(self.cell_share(w, c, 0, TOP_ACCENT) > 0.5 for c in range(COLS))
        step("TOP-1-title-row-accent-all-cells", drawn, "TOP-1: top frame with its title bar", 180)
        time.sleep(2)
        frame = self.shot("TOP-1-frame")
        win = self.window(frame)[1]
        if win is None:
            self.chk("TOP-1-last-column-inked-every-row", False, "window not located on the top frame")
            raise StepFailed("TOP-1: window not located on the top frame")
        blank_last = [r for r in range(ROWS) if not self.cell_inked(win, COLS - 1, r)]
        self.chk("TOP-1-last-column-inked-every-row", not blank_last, f"blank last-column rows {blank_last}")
        prompt_blank = [r for r in range(ROWS) if not self.cell_inked(pwin, COLS - 1, r)]
        self.chk("TOP-1-control-prompt-last-column-blank", bool(prompt_blank),
                 f"{len(prompt_blank)} blank last-column cells on the prompt screen")
        title = [c for c in range(COLS) if self.cell_share(win, c, 0, TOP_ACCENT) <= 0.5]
        self.chk("TOP-1-title-row-accent-all-cells", not title, f"columns without the accent {title}")
        status = [c for c in range(COLS) if self.cell_share(win, c, 1, TOP_SURFACE) <= 0.5]
        self.chk("TOP-1-status-row-surface-all-cells", not status, f"columns without the surface colour {status}")

        # TOP-2: continuous borders: no blank pixel row between the stacked `│` of rows
        # 3..ROWS-3 on the left and right borders, no blank pixel column along the bottom `─`
        # (columns 1..COLS-2 of row ROWS-2). Control: the same scan finds the gap between two
        # stacked glyphs of the prompt screen.
        top_y, bottom_y = PAD_Y + 3 * CELL_H, PAD_Y + (ROWS - 2) * CELL_H
        for side, col in (("left", 0), ("right", COLS - 1)):
            x0 = PAD_X + col * CELL_W
            gaps = self.blank_lines(win, (x0, top_y, x0 + CELL_W, bottom_y), True)
            self.chk(f"TOP-2-{side}-border-no-gap", not gaps, f"blank pixel rows {gaps[:8]}")
        gaps = self.blank_lines(win, (PAD_X + CELL_W, bottom_y, PAD_X + (COLS - 1) * CELL_W, bottom_y + CELL_H), False)
        self.chk("TOP-2-bottom-border-no-gap", not gaps, f"blank pixel columns {gaps[:8]}")
        rows = self.pad(typed_rows)
        pair = next(((c, r) for r in range(ROWS - 1) for c in range(COLS)
                     if c < len(rows[r]) and c < len(rows[r + 1]) and rows[r][c] != " " and rows[r + 1][c] != " "), None)
        found = []
        if pair is not None:
            c, r = pair
            box = (PAD_X + c * CELL_W, PAD_Y + r * CELL_H, PAD_X + (c + 1) * CELL_W, PAD_Y + (r + 2) * CELL_H)
            blank = self.blank_lines(pwin, box, True)
            inked = [y for y in range(box[1], box[3]) if y not in blank]
            found = [y for y in blank if inked and min(inked) < y < max(inked)]
        self.chk("TOP-2-control-scan-finds-prompt-row-gap", bool(found), f"cell {pair}: gap pixel rows {found[:8]}")

        # TOP-3: on a stable order (`o` four times: CPU% -> QUOTA -> LIMIT -> STATE -> CELL)
        # PageUp puts the band on the first data row (control); Down moves it exactly one
        # cell row; Enter drills into that Cell: the band is on the first row again and the
        # title region changes.
        for _ in range(4):
            self.type_text("o", gap=0.8)
        self.tap("pgup")
        first = step("TOP-3-control-band-on-first-data-row", band_at(TOP_FIRST_DATA_ROW),
                     "TOP-3: band on the first data row")
        self.shot("TOP-3-band-first-row", first)
        self.chk("TOP-3-control-band-on-first-data-row", True, f"band rows [{TOP_FIRST_DATA_ROW}]")
        self.tap("down")
        moved = step("TOP-3-down-moves-band-one-row", band_at(TOP_FIRST_DATA_ROW + 1), "TOP-3: band one row lower")
        self.shot("TOP-3-band-moved", moved)
        self.chk("TOP-3-down-moves-band-one-row", True, f"band rows [{TOP_FIRST_DATA_ROW}] -> [{TOP_FIRST_DATA_ROW + 1}]")
        title_box = (0, 0, WIN_W, PAD_Y + CELL_H)
        mtitle = self.window(moved)[1].crop(title_box)

        def title_changed(w):
            return sum(ImageChops.difference(mtitle, w.crop(title_box)).convert("L").histogram()[1:])

        def drilled(i):
            w = self.window(i)[1]
            return w is not None and self.band_rows(w) == [TOP_FIRST_DATA_ROW] and title_changed(w) >= MIN_NEGATIVE_DIFF
        self.tap("ret")
        cell = step("TOP-3-enter-band-first-row", drilled, "TOP-3: Enter drills in")
        self.shot("TOP-3-drilled", cell)
        cwin = self.window(cell)[1]
        self.chk("TOP-3-enter-band-first-row", self.band_rows(cwin) == [TOP_FIRST_DATA_ROW], self.band_rows(cwin))
        self.chk("TOP-3-enter-title-region-changes", title_changed(cwin) >= MIN_NEGATIVE_DIFF,
                 f"{title_changed(cwin)} pixels changed")

        # TOP-4: `q` restores the prompt screen exactly (the typed line, then a fresh prompt).
        self.type_text("q")
        restored = typed_rows + [PROMPT]
        try:
            img = self.expect_screen(restored, "TOP-4: restored prompt screen", 120)
        except StepFailed:
            self.chk("TOP-4-restored-prompt-exact", False, "no exact restored prompt screen")
            raise
        self.shot("TOP-4-restored", img)
        self.chk("TOP-4-restored-prompt-exact", True, "diff=0")
        altered = typed_rows[:-1] + [one(typed_rows[-1]), PROMPT]
        dalt, _ = self.diff(self.window(img)[1], self.pad(altered), 0, ROWS - 1, self.mcursor(restored))
        self.chk("TOP-4-control-one-char-change-differs", dalt >= MIN_NEGATIVE_DIFF, f"{dalt} differing pixels")
        model[:] = restored
        state["img"] = img

        # TOP-5: `top` exited with status 0.
        command("TOP-5-status", "echo st=$?", [["st=0"]])

    def edit(self, command, model, state):
        """phase-08-s0-proposal.md S8-5 EDIT-0..5: `edit` in the GUI Terminal through real
        key input, judged by structure (the status row on the surface colour across every
        cell, typed glyph ink in the first text row) and an exact restored prompt screen.
        Saving is judged by the terminal runner on CellosFS; this window has no such volume."""
        def step(name, pred, what, timeout=60):
            try:
                return self.wait_for(pred, what, timeout)[0]
            except StepFailed:
                self.chk(name, False, f"timeout: {what}")
                raise

        typed_rows = model[:-1] + [model[-1] + "edit"]
        self.type_text("edit")
        self.expect_screen(typed_rows, "EDIT-0: typed line", 120)
        self.chk("EDIT-0-typed-line-exact", True, "diff=0")
        self.tap("ret")

        # EDIT-1: the editor owns the window: the status row (second to last) carries the
        # surface colour on every cell; the first text row is empty.
        status_row = ROWS - 2

        def drawn(i):
            w = self.window(i)[1]
            return w is not None and all(self.cell_share(w, c, status_row, TOP_SURFACE) > 0.5 for c in range(COLS))
        empty = step("EDIT-1-status-row-surface-all-cells", drawn, "EDIT-1: editor status row", 180)
        self.shot("EDIT-1-editor", empty)
        ewin = self.window(empty)[1]
        self.chk("EDIT-1-status-row-surface-all-cells", True, f"row {status_row}")
        # The editor paints its own background (vitui palette bg), so ink means a glyph pixel:
        # neither the window nor the editor background. Column 0 holds the cursor.
        def glyph(w, col, row):
            return any(c not in (BG, EDIT_BG) for _, c in w.crop(self.cell_box(col, row)).getcolors(CELL_W * CELL_H))
        self.chk("EDIT-1-first-row-empty", not any(glyph(ewin, c, 0) for c in range(1, 8)), "glyph ink in row 0")

        # EDIT-2: typed keys land as text at the start of the first row; nothing past it.
        self.type_text("hello")

        def typed(i):
            w = self.window(i)[1]
            return (w is not None and all(glyph(w, c, 0) for c in range(5))
                    and not any(glyph(w, c, 0) for c in range(6, 12)))
        hello = step("EDIT-2-typed-text-drawn", typed, "EDIT-2: hello in row 0")
        self.shot("EDIT-2-typed", hello)
        self.chk("EDIT-2-typed-text-drawn", True, "cells 0..4 glyph-inked, 6..11 blank")

        # EDIT-3: Ctrl-Q on the dirty buffer asks instead of exiting: the bottom row changes
        # and the window stays the editor's; `d` then discards.
        bottom = (PAD_X, PAD_Y + (ROWS - 1) * CELL_H, PAD_X + COLS * CELL_W, PAD_Y + ROWS * CELL_H)
        hwin = self.window(hello)[1]

        def asked(i):
            w = self.window(i)[1]
            changed = w is not None and sum(ImageChops.difference(hwin.crop(bottom), w.crop(bottom)).convert("L").histogram()[1:])
            return changed and changed >= MIN_NEGATIVE_DIFF and all(self.cell_share(w, c, status_row, TOP_SURFACE) > 0.5 for c in range(COLS))
        self.combo("ctrl", "q")
        question = step("EDIT-3-dirty-quit-asks", asked, "EDIT-3: dirty-exit question")
        self.shot("EDIT-3-asks", question)
        self.chk("EDIT-3-dirty-quit-asks", True, "bottom row changed, editor still drawn")
        self.type_text("d")

        # EDIT-4: the prompt screen comes back exactly (the typed line, then a fresh prompt).
        restored = typed_rows + [PROMPT]
        try:
            img = self.expect_screen(restored, "EDIT-4: restored prompt screen", 120)
        except StepFailed:
            self.chk("EDIT-4-restored-prompt-exact", False, "no exact restored prompt screen")
            raise
        self.shot("EDIT-4-restored", img)
        self.chk("EDIT-4-restored-prompt-exact", True, "diff=0")
        model[:] = restored
        state["img"] = img

        # EDIT-5: discarding exits with status 0.
        command("EDIT-5-status", "echo st=$?", [["st=0"]])

    # ---- C5: Files and Spotlight screens ------------------------------------------------
    @staticmethod
    def files_ops(view):
        """Files' toolbar path label, ListView pane and status bar for `view` = (path, rows,
        selected, status), painted the way main.rs/Toolbar/ListView/StatusBar paint them.
        `rows` are (name, is_dir, size label) in display order."""
        path, rows, selected, status = view
        x, y, w, h = 160, 44, FILES_W - 160, FILES_H - 44 - 24
        ops = [("fill", 0, 0, FILES_W, 44, L_SURFACE),
               ("label", 48, 10, FILES_W - 248, 24, "UiSemibold", 15.0, L_TEXT, 0, "Files: " + path),
               ("fill", x, y, w, h, L_BG), ("fill", x, y, w, 28, L_SURFACE), ("fill", x, y + 27, w, 1, L_BORDER),
               ("label", x + 36, y + 4, w - 220, 20, "UiSemibold", 12.0, L_TEXT2, 0, "Name"),
               ("label", x + w - 170, y + 4, 80, 20, "UiSemibold", 12.0, L_TEXT2, 0, "Size"),
               ("label", x + w - 70, y + 4, 60, 20, "UiSemibold", 12.0, L_TEXT2, 0, "Kind")]
        for i, (name, is_dir, size) in enumerate(rows):
            ry = y + 28 + i * 32
            if i == selected:
                ops += [("fill", x, ry, w, 32, L_SELECTED), ("fill", x, ry, 3, 32, L_ACCENT)]
            ops += [("label", x + 8, ry + 6, 24, 20, "UiSemibold", 14.0, L_ACCENT if is_dir else L_TEXT, 0,
                     file_icon(name, is_dir)),
                    ("label", x + 36, ry + 6, w - 220, 20, "UiRegular", 14.0, L_TEXT, 0, name),
                    ("label", x + w - 170, ry + 6, 80, 20, "UiRegular", 13.0, L_TEXT2, 0, size),
                    ("label", x + w - 70, ry + 6, 60, 20, "UiRegular", 13.0, L_TEXT2, 0, "Folder" if is_dir else "File"),
                    ("fill", x, ry + 31, w, 1, L_BORDER)]
        ops += [("fill", 0, FILES_H - 24, FILES_W, 24, L_SURFACE), ("fill", 0, FILES_H - 24, FILES_W, 1, L_BORDER),
                ("label", 14, FILES_H - 22, FILES_W - 28, 22, "UiRegular", 12.0, L_TEXT2, 0, status)]
        return ops

    def spot_ops(self, kind, alter=False):
        """Spotlight's results area: "none" = the no-match label, "system" = the System section
        (spotlight.rs render_system, row 0 selected), "system-with-power" = the removed rows too.
        `alter` changes one character of the first label."""
        change = (lambda s: s[:-1] + ("x" if s[-1] != "x" else "y")) if alter else (lambda s: s)
        ops = [("fill", 0, 0, SPOT_W, SPOT_H, D_MODAL_BG), ("stroke", 0, 0, SPOT_W, SPOT_H, D_MODAL_BORDER)]
        if kind == "none":
            ops.append(("label", 24, 108, SPOT_W - 48, 32, "UiRegular", 15.0, D_MUTED, 1, change("No matching cells found")))
            return ops
        actions = SYSTEM_ACTIONS + (REMOVED_POWER_ACTIONS if kind == "system-with-power" else [])
        nh, dh = self.text_row_height("UiSemibold", 15.0), self.text_row_height("UiRegular", 13.0)
        text_w = (SPOT_W - 92) - 10 - 70  # run_x - 10 - text_x
        for i, (icon, name, desc, button) in enumerate(actions):
            ry, selected = 80 + i * 62, i == 0
            ops.append(("fill", 14, ry, SPOT_W - 28, 56, D_BTN_ACTIVE if selected else D_BTN_NORMAL))
            if selected:
                ops.append(("fill", 14, ry, 4, 56, D_CYAN))
            ty = ry + (56 - nh - dh - 4) // 2
            ops += [("stroke", 14, ry, SPOT_W - 28, 56, D_BLUE if selected else D_TASKBAR_BORDER),
                    ("label", 24, ry, 34, 56, "UiRegular", 14.0, D_CYAN, 1, icon),
                    ("label", 70, ty, text_w, nh, "UiSemibold", 15.0, D_TEXT, 0, change(name) if selected else name),
                    ("label", 70, ty + nh + 4, text_w, dh, "UiRegular", 13.0, D_MUTED, 0, desc),
                    ("button", SPOT_W - 92, ry + 14, 64, 28, D_BTN_NORMAL, D_BTN_HOVER, D_TEXT, button)]
        return ops

    @staticmethod
    def region_diffs(img, org, expected, regions):
        out = {}
        for name, (x0, y0, x1, y1) in regions.items():
            seen = img.crop((org[0] + x0, org[1] + y0, org[0] + x1, org[1] + y1))
            out[name] = sum(ImageChops.difference(seen, expected.crop((x0, y0, x1, y1))).convert("L").histogram()[1:])
        return out

    def files_diffs(self, img, view):
        return self.region_diffs(img, FILES_ORG, self.render_ops(FILES_W, FILES_H, self.files_ops(view)), FILES_REGIONS)

    def spot_diffs(self, img, kind, alter=False):
        return self.region_diffs(img, SPOT_ORG, self.render_ops(SPOT_W, SPOT_H, self.spot_ops(kind, alter)), SPOT_REGIONS)

    def expect_surface(self, name, diffs, view, altered, before, timeout=180):
        """Wait until every judged region equals the render of `view` exactly (0 differing
        pixels); controls: the one-character-changed `altered` view and the frame from before
        the action must both differ."""
        last = {}

        def pred(i):
            d = diffs(i, view)
            last.clear()
            last.update(d)
            return all(n == 0 for n in d.values())
        try:
            got, _ = self.wait_for(pred, name, timeout)
        except StepFailed:
            self.notes.append(f"{name}: last region diffs {last}")
            self.chk(f"{name}-exact", False, f"region diffs {last}")
            raise
        self.shot(name, got)
        self.chk(f"{name}-exact", True, "diff=0 in " + ", ".join(last))
        da = sum(diffs(got, altered).values())
        self.chk(f"{name}-control-one-char-change-differs", da >= MIN_NEGATIVE_DIFF, f"{da} differing pixels")
        db = sum(diffs(before, view).values())
        self.chk(f"{name}-control-before-frame-differs", db >= MIN_NEGATIVE_DIFF, f"{db} differing pixels")
        return got

    def expect_files(self, name, path, rows, selected, status, before):
        altered = (path, rows, selected, status[:-1] + ("x" if status[-1] != "x" else "y"))
        return self.expect_surface(name, self.files_diffs, (path, rows, selected, status), altered, before)

    def expect_spot(self, name, kind, before):
        return self.expect_surface(name, lambda i, v: self.spot_diffs(i, *v), (kind, False), (kind, True), before)

    def raise_terminal(self):
        """Raise the Terminal above Files through its title tab, then give it the keyboard with
        a content click (the Terminal re-requests focus on a left click)."""
        self.click_px(*TERM_TAB)
        self.park()
        self.wait_for(lambda i: self.locate(i) == self.org, "Terminal raised above Files", 60)
        self.click_px(self.org[0] + 360, self.org[1] + 300)
        self.park()

    def focus_files(self):
        """Raise Files and give it the keyboard: a click on its status bar, which hits nothing."""
        self.click_px(*FILES_FOCUS)
        self.park()

    def c5(self, command):
        """Proposal §3 steps 1-6: Terminal (cellbox sh) and Files act on the same VFS paths."""
        up, txt = ("..", True, "--"), ("a.txt", False, "3 B")
        c5new = ("c5new", True, "--")
        # 1. The CLI creates the fixture.
        command("C5-1-mkdir", "mkdir /tmp/c5dir", [[]])
        command("C5-1-write", "echo c5 > /tmp/c5dir/a.txt", [[]])
        # 2. Files, launched by a real taskbar click, lists it.
        self.click_px(*FILES_TASKBAR)
        self.park()
        self.wait_serial("[files] Files surface ready. Entering event loop.", 300, "files ready")
        time.sleep(3)
        before = self.shot("C5-2-files-launched")
        self.combo("ctrl", "l")
        self.type_text("/tmp/c5dir")
        self.tap("ret")
        cur = self.expect_files("C5-2-files-lists-a-txt", "/tmp/c5dir", [up, txt], 0, "/tmp/c5dir: 1 item(s)", before)
        # 3. Files creates a folder and copies a.txt into it; the CLI sees the copy.
        self.combo("ctrl", "n")
        self.type_text("c5new")
        self.tap("ret")
        cur = self.expect_files("C5-3-files-mkdir", "/tmp/c5dir", [up, c5new, txt], 1, "Created folder c5new", cur)
        self.tap("down")
        self.combo("ctrl", "c")
        cur = self.expect_files("C5-3-files-copy", "/tmp/c5dir", [up, c5new, txt], 2, "Copied a.txt", cur)
        self.tap("up")
        self.tap("ret")
        cur = self.expect_files("C5-3-files-open-c5new", "/tmp/c5dir/c5new", [up], 0, "/tmp/c5dir/c5new: 0 item(s)", cur)
        self.combo("ctrl", "v")
        self.expect_files("C5-3-files-paste", "/tmp/c5dir/c5new", [up, txt], 1, "Pasted a.txt -> a.txt", cur)
        self.raise_terminal()
        command("C5-3-cli-ls-copy", "ls /tmp/c5dir/c5new", [["a.txt"]])
        command("C5-3-cli-cat-copy", "cat /tmp/c5dir/c5new/a.txt", [["c5"]])
        # 4. Files deletes the copy, then the folder; the CLI no longer lists either.
        #    kp_decimal is the keypad Del key (Num Lock is off): the input layout maps evdev
        #    KEY_DELETE (111) to Unknown, so the dedicated Delete key never reaches a cell.
        self.focus_files()
        before = self.grab()
        self.tap("kp_decimal")
        self.expect_files("C5-4-files-delete-file", "/tmp/c5dir/c5new", [up], 0,
                          "Permanently deleted a.txt (irreversible)", before)
        self.raise_terminal()
        command("C5-4-cli-ls-file-gone", "ls /tmp/c5dir/c5new", [[]])
        self.focus_files()
        before = self.grab()
        self.tap("backspace")
        cur = self.expect_files("C5-4-files-up", "/tmp/c5dir", [up, c5new, txt], 0, "/tmp/c5dir: 2 item(s)", before)
        self.tap("down")
        self.tap("kp_decimal")
        self.expect_files("C5-4-files-delete-folder", "/tmp/c5dir", [up, txt], 1,
                          "Permanently deleted c5new (irreversible)", cur)
        self.raise_terminal()
        command("C5-4-cli-ls-folder-gone", "ls /tmp/c5dir", [["a.txt"]])
        # 5. A path the VFS refuses shows the refusal; the listing stays the real one.
        self.focus_files()
        before = self.grab()
        self.combo("ctrl", "l")
        self.type_text("/srv/cellos")
        self.tap("ret")
        self.expect_files("C5-5-files-srv-cellos-denied", "/tmp/c5dir", [up, txt], 1,
                          "Cannot open /srv/cellos: permission denied (VFS ERR_DENIED)", before)
        # 6. Spotlight: no power action by search, none in the System section.
        self.click_px(*SPOT_BUTTON)
        self.park()
        self.wait_serial("[desktop] Taskbar: Opening Spotlight Search", 60, "spotlight opened")
        time.sleep(2)
        before = self.shot("C5-6-spotlight-open")
        self.type_text("shutdown")
        self.expect_spot("C5-6-spotlight-shutdown-no-match", "none", before)
        for _ in "shutdown":
            self.tap("backspace", 0.15)
        cleared, _ = self.wait_for(lambda i: sum(self.spot_diffs(i, "none").values()) >= MIN_NEGATIVE_DIFF,
                                   "Spotlight query cleared", 60)
        self.type_text("reboot")
        cur = self.expect_spot("C5-6-spotlight-reboot-no-match", "none", cleared)
        self.tap("tab")
        self.tap("tab")
        cur = self.expect_spot("C5-6-spotlight-system-no-power", "system", cur)
        dold = sum(self.spot_diffs(cur, "system-with-power").values())
        self.chk("C5-6-control-removed-power-rows-would-differ", dold >= MIN_NEGATIVE_DIFF, f"{dold} differing pixels")
        self.tap("esc")
        self.raise_terminal()

    # ---- UART --------------------------------------------------------------------------
    def uart_checks(self):
        raw = (self.ev / "serial.raw.log").read_bytes()
        log = (self.ev / "serial.log").read_text()
        seed = Path(self.a.seed_file).read_text().strip()
        payloads = {
            "echo": "gui-proof-1", "stock-echo": "gui-proof-ext", "ls-alpha": "alpha-entry", "ls-beta": "beta-entry",
            "cat-one": "gui-cat-line-one", "cat-two": "gui-cat-line-two",
            "stderr": "no such directory", "ctrl-c-status": "cc=130", "typed-command": "/bin/cat /gui-text",
            "shell-banner": "Cellos shell ready", "shell-prompt": "Cellos >", "c5-typed-command": "/tmp/c5dir/a.txt",
            "vt-typed-command": "/p04-output/vt-", "vt-sample-output": "end-of-sample",
            # S5-4 hostile output: its text, the typed token and the discarded clipboard (base64
            # "p04-clip"), link and title payloads never reach the UART.
            "vt-hostile-output": "hostile-begin", "hostile-echo": "hostile-ok",
            "hostile-clipboard": "cDA0LWNsaXA=", "hostile-link": "p04.invalid", "hostile-title": "p04-title",
        }
        for name, needle in payloads.items():
            self.chk("no-command-uart:" + name, needle.encode() not in raw, "payload appeared on the physical UART")
        self.chk("uart-terminal-ready-once", log.count("[terminal] Terminal surface ready. Entering event loop.") == 1)
        # S5-3 option C: exactly one window close/re-create, one shell re-attach notice.
        self.chk("uart-s53c-one-release", log.count("[terminal] window closed; frontend released, shell detached") == 1)
        self.chk("uart-s53c-one-reattach", log.count("[terminal] window re-created; frontend re-attached to the same shell") == 1)
        self.chk("uart-s53c-shell-reattach-once", log.count("shell: frontend re-attached; redraw") == 1)
        self.chk("uart-files-ready-once", log.count("[files] Files surface ready. Entering event loop.") == 1)
        self.chk("no-spotlight-power-claim", "Authorized system" not in log, "a fake power action reported success")
        self.chk("no-key-material-in-log", seed not in log)
        lines = log.splitlines()
        denies = [l for l in lines if "DENY" in l]
        self.chk("no-launch-deny", not denies, denies[:3])
        bad = [l for l in lines if re.search(r"panic|PANIC|page fault|PAGE FAULT|cell fault|CELL FAULT|OOM: cell heap", l)]
        self.chk("no-panic-or-fault", not bad, bad[:3])

    def write_verdict(self):
        (self.ev / "verdict.json").write_text(json.dumps({
            "checks": self.checks,
            "notes": self.notes,
            "window_origin": self.org,
            "scope": "private GUI Terminal proof on a private desktop image; not release acceptance",
        }, indent=2) + "\n")
        bad = [n for n, c in self.checks.items() if not c["pass"] and not c["informational"]]
        if bad:
            print("FAIL: %d check(s) failed: %s" % (len(bad), ", ".join(bad)))
            return 1
        print("PASS: %d GUI checks" % len(self.checks))
        return 0


def main():
    p = argparse.ArgumentParser()
    for name in ("work", "iso", "evidence", "glyph-bin", "accel", "seed-file", "launch"):
        p.add_argument("--" + name, required=True)
    p.add_argument("--window", type=int, default=900)
    sys.exit(Proof(p.parse_args()).run())


if __name__ == "__main__":
    main()
