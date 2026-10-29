#!/usr/bin/env python3
"""Host-built hostile `cellos-vt1` output fixture (Phase 05 S5-4a item 3).

`fixture()` returns the exact bytes of /p04-output/vt-hostile.bin, packaged by the private
Phase04 terminal and GUI runners; `VISIBLE` is the text the GUI Terminal must show for it,
one entry per screen row starting at the cursor row where the output begins (default
colours, no attributes). Both are authored from the emulator contract in
cells/apps/terminal/src/sink.rs, never by running an emulator:

* OSC 52 clipboard set and query, OSC 8 link, OSC 0 and OSC 2 titles: discarded strings
  (no clipboard, link or title effect, no reply). The OSC 8 link text itself is ordinary
  output and stays visible.
* DCS, APC and PM strings over 4 KiB: the parser discards a string body for MAX_STRING
  (4096) bytes including its 2-byte introducer, then gives up; the next byte and the rest
  are ordinary text. Each body is exactly `MAX_STRING - 2` filler bytes followed by a
  short visible tail, so the screen proves the bound and the recovery.
* Device queries DA (primary/secondary), DSR 5n/6n, DECRQM (private and ANSI): no
  effect and no reply bytes.
* 8-bit C1 CSI 0x9b (and 8-bit OSC 0x9d / ST 0x9c): not interpreted (D54-5); each such byte
  is a stray UTF-8 continuation and shows as U+FFFD, the following text stays plain.
* CSI with 17 parameters (bold, red, ...): over MAX_PARAMS (16), a no-op; the text after
  it stays default.
* Embedded bracketed-paste end/start `ESC[201~` / `ESC[200~`: no effect in output.

Usage: phase05_vt_hostile.py OUT_FILE
"""

import sys
from pathlib import Path

ESC, BEL, ST = b"\x1b", b"\x07", b"\x1b\\"
# cells/apps/terminal/src/sink.rs MAX_STRING: the introducer (ESC + 1 byte) counts.
MAX_STRING = 4096
BODY = MAX_STRING - 2
PATH = "/p04-output/vt-hostile.bin"

VISIBLE = [
    "hostile-begin",
    "osc52-hidden",
    "link-text title-hidden",
    "dcs-tail apc-tail pm-tail",
    "queries-silent",
    "\ufffd31mc1-csi \ufffd0;c1-osc\ufffd",
    "params17-plain",
    "paste-end-kept",
    "hostile-end",
]


def long_string(introducer: bytes, tail: bytes) -> bytes:
    """A string of more than 4 KiB: `BODY` discarded bytes, then `tail` past the bound."""
    return ESC + introducer + b"Z" * BODY + tail + ST


def fixture() -> bytes:
    params17 = b";".join([b"1", b"31"] + [b"1"] * 15)
    assert params17.count(b";") == 16
    parts = [
        b"hostile-begin\n",
        # OSC 52: set the clipboard to base64("p04-clip"), then query it.
        ESC + b"]52;c;cDA0LWNsaXA=" + BEL,
        ESC + b"]52;c;?" + ST,
        b"osc52-hidden\n",
        # OSC 8 hyperlink around visible text; OSC 0 / OSC 2 window titles.
        ESC + b"]8;;http://p04.invalid/" + ST + b"link-text" + ESC + b"]8;;" + ST,
        b" ",
        ESC + b"]0;p04-title" + BEL,
        ESC + b"]2;p04-title2" + ST,
        b"title-hidden\n",
        # DCS, APC and PM strings of more than 4 KiB.
        long_string(b"P", b"dcs-tail"), b" ",
        long_string(b"_", b"apc-tail"), b" ",
        long_string(b"^", b"pm-tail"), b"\n",
        # Device queries: DA, secondary DA, DSR status and cursor position, DECRQM.
        ESC + b"[c", ESC + b"[0c", ESC + b"[>c", ESC + b"[5n", ESC + b"[6n",
        ESC + b"[?2004$p", ESC + b"[4$p",
        b"queries-silent\n",
        # 8-bit C1: CSI (0x9b) and OSC (0x9d .. ST 0x9c) are not interpreted.
        b"\x9b31mc1-csi ", b"\x9d0;c1-osc\x9c", b"\n",
        # 17 parameters: bold + red would apply with 16; with 17 the whole CSI is a no-op.
        ESC + b"[" + params17 + b"m", b"params17-plain", ESC + b"[0m", b"\n",
        # Bracketed-paste markers inside output.
        b"paste-", ESC + b"[201~", b"end", ESC + b"[200~", b"-kept\n",
        b"hostile-end\n",
    ]
    data = b"".join(parts)
    for introducer in (b"P", b"_", b"^"):
        assert len(long_string(introducer, b"")) > MAX_STRING
    return data


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        return 2
    Path(sys.argv[1]).write_bytes(fixture())
    return 0


if __name__ == "__main__":
    sys.exit(main())
