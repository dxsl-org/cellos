#!/usr/bin/env python3
"""Drive Stack & Tile on the x86 desktop image over QMP and check pixels.

Used by scripts/qemu-stack-tile-test.sh. Geometry follows the desktop's launch
placement (Terminal content 720x480 at 140,100; Files 800x600 at 100,100) and
the compositor's chrome (window_decoration: tab = content - (1, 28), 28px
tall; icon box at tab + (4, 3), 20px; window menu 4px below it, items 28px
from menu + 4, a 9px separator between sections; Light palette colours).
"""
import argparse
import json
import socket
import subprocess
import sys
import time
from pathlib import Path

ACTIVE_TAB = (0xFF, 0xFF, 0xFF)
INACTIVE_TAB = (0xEB, 0xED, 0xF2)
STACKED_TAB = (0xDF, 0xE2, 0xE9)


class Qmp:
    def __init__(self, path):
        for _ in range(100):
            try:
                self.s = socket.socket(socket.AF_UNIX)
                self.s.settimeout(15)
                self.s.connect(path)
                break
            except OSError:
                time.sleep(0.2)
        self.f = self.s.makefile("rw")
        self.f.readline()
        self.cmd("qmp_capabilities")

    def cmd(self, name, **args):
        self.f.write(json.dumps({"execute": name, "arguments": args} if args else {"execute": name}) + "\n")
        self.f.flush()
        while True:
            line = self.f.readline()
            if '"return"' in line or '"error"' in line:
                return line

    def events(self, events):
        self.cmd("input-send-event", events=events)

    def move(self, x, y):
        self.events([{"type": "abs", "data": {"axis": "x", "value": x * 32768 // 1280}},
                     {"type": "abs", "data": {"axis": "y", "value": y * 32768 // 800}}])
        time.sleep(0.15)

    def button(self, down):
        self.events([{"type": "btn", "data": {"down": down, "button": "left"}}])
        time.sleep(0.15)

    def key(self, qcode, down):
        self.events([{"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": qcode}}}])
        time.sleep(0.1)

    def click(self, x, y):
        self.move(x, y)
        self.button(True)
        self.button(False)
        time.sleep(0.6)

    def drag(self, start, end, hold=None):
        if hold:
            self.key(hold, True)
        self.move(*start)
        self.button(True)
        for step in range(1, 11):
            self.move(start[0] + (end[0] - start[0]) * step // 10, start[1] + (end[1] - start[1]) * step // 10)
        self.button(False)
        if hold:
            self.key(hold, False)
        time.sleep(1.0)


class Screen:
    def __init__(self, path):
        data = Path(path).read_bytes()
        parts = data.split(maxsplit=4)
        self.w, self.h = int(parts[1]), int(parts[2])
        self.px = parts[4]

    def at(self, x, y):
        i = (y * self.w + x) * 3
        return tuple(self.px[i:i + 3])


class Drive:
    def __init__(self, a):
        self.a, self.ev = a, Path(a.evidence)
        self.ev.mkdir(parents=True, exist_ok=True)
        self.results, self.serial = [], self.ev / "serial.log"

    def chk(self, name, ok, why=""):
        self.results.append((name, bool(ok)))
        print(f"{'PASS' if ok else 'FAIL'} {name}" + (f"  <- {why}" if not ok and why else ""), flush=True)
        return bool(ok)

    def wait_serial(self, needle, timeout):
        end = time.time() + timeout
        while time.time() < end:
            if self.serial.exists() and needle in self.serial.read_text(errors="replace"):
                return True
            time.sleep(0.5)
        return False

    def shot(self, label):
        path = self.ev / f"{label}.ppm"
        self.qmp.cmd("screendump", filename=str(path))
        time.sleep(0.4)
        return Screen(path)

    def settled(self, label):
        """Two identical screendumps in a row: the compositor flushed."""
        previous = None
        for _ in range(20):
            screen = self.shot(label)
            if previous is not None and screen.px == previous.px:
                return screen
            previous = screen
            time.sleep(0.3)
        return screen

    def run(self):
        argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", self.a.accel,
                "-cpu", "host" if self.a.accel == "kvm" else "qemu64,+pdpe1gb", "-m", "512M",
                "-display", "none", "-serial", f"file:{self.serial}",
                "-qmp", f"unix:{self.ev / 'qmp.sock'},server=on,wait=off", "-monitor", "none",
                "-cdrom", self.a.iso, "-boot", "d", "-no-reboot", "-nic", "none",
                "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"]
        proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            self.qmp = Qmp(str(self.ev / "qmp.sock"))
            self.scenario()
        finally:
            proc.terminate()
            proc.wait(timeout=20)
        failed = [name for name, ok in self.results if not ok]
        print(f"{len(self.results) - len(failed)} PASS, {len(failed)} FAIL")
        return 1 if failed or not self.results else 0

    def scenario(self):
        q = self.qmp
        if not self.chk("desktop-ready", self.wait_serial("[desktop] Ready. Entering event loop.", self.a.window)):
            return
        time.sleep(3)
        q.click(214, 782)
        if not self.chk("terminal-ready", self.wait_serial("[terminal] Terminal surface ready", 120)):
            return
        q.click(510, 782)
        if not self.chk("files-ready", self.wait_serial("[files] Files surface ready", 120)):
            return
        time.sleep(2)
        # Files' tab (content 100,100) from 250,86 to 750,386: content at 600,400.
        q.drag((250, 86), (750, 386))

        # ---- Stack from the window menu ------------------------------------------
        # Files' icon box 603..623 x 375..395; menu at 603,399. Rows: Maximize 4,
        # Minimize 32, Close 69, then "Stack onto Terminal" at 4 + 3*28 + 2*9 = 106.
        q.click(613, 385)
        menu = self.settled("menu")
        self.chk("menu-open", menu.at(700, 399 + 106 + 14) != menu.at(700, 300), "no menu row at the stack item")
        q.click(653, 399 + 106 + 14)
        s = self.settled("stacked")
        # Terminal's frame (content 140,100): strip y 72..99 spans the frame;
        # Terminal's tab left (stacked fill), Files' tab right (shown, active).
        self.chk("stack-left-tab-is-stacked", s.at(300, 80) == STACKED_TAB, s.at(300, 80))
        self.chk("stack-right-tab-is-shown", s.at(700, 80) == ACTIVE_TAB, s.at(700, 80))

        # ---- Pick the other tab -------------------------------------------------
        q.click(300, 86)
        s = self.settled("picked")
        self.chk("picked-tab-shown", s.at(300, 80) == ACTIVE_TAB, s.at(300, 80))
        self.chk("other-tab-stacked", s.at(700, 80) == STACKED_TAB, s.at(700, 80))

        # ---- Win+drag the Terminal tab out: it leaves the stack -------------------
        q.drag((300, 86), (700, 386), hold="meta_l")
        s = self.settled("detached")
        self.chk("detached-own-tab", s.at(600, 380) == ACTIVE_TAB, s.at(600, 380))
        self.chk("stack-dissolved", s.at(700, 80) not in (ACTIVE_TAB, STACKED_TAB), s.at(700, 80))

        # ---- Win+drag Terminal beside Files: it tiles flush ----------------------
        # Terminal content now 540,400 (visible frame from 539,372); Files' visible
        # frame ends at 941. Drop Terminal's frame at 945,72: it snaps to 941.
        q.drag((600, 386), (600 + 406, 386 - 300), hold="meta_l")
        s = self.settled("tiled")
        # Terminal's tab outline in column 941, its fill from 942: flush with Files.
        self.chk("tiled-flush", s.at(941, 86) not in (ACTIVE_TAB, INACTIVE_TAB) and s.at(943, 86) == ACTIVE_TAB,
                 f"{s.at(941, 86)} {s.at(943, 86)}")
        # Plain drag of Files' tab moves the group: Terminal's tab follows 50px down.
        q.drag((300, 86), (300, 136))
        s = self.settled("group-moved")
        tabs = (ACTIVE_TAB, INACTIVE_TAB)
        self.chk("group-moved-together", s.at(1000, 130) in tabs and s.at(1000, 80) not in tabs,
                 f"{s.at(1000, 130)} {s.at(1000, 80)}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--iso", required=True)
    p.add_argument("--evidence", required=True)
    p.add_argument("--accel", default="kvm", choices=["kvm", "tcg"])
    p.add_argument("--window", type=int, default=300)
    sys.exit(Drive(p.parse_args()).run())


if __name__ == "__main__":
    main()
