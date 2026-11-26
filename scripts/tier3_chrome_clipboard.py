"""Chrome lane, phase 09: the clipboard between Chromium and the Terminal.

Text copied in Chromium pastes into the Terminal (Ctrl+Shift+V); text the
Terminal copies (a dragged selection, Ctrl+Shift+C) pastes into Chromium's
omnibox, which then loads it. Nothing reaches the guest while the Chrome window
is not focused. Each step is read from the serial log (byte counts, never the
text) or, for the end result, from the page Chromium shows."""
import time

from tier3_display_check import CHROME_X, CHROME_Y

TERMINAL_TASKBAR = (214, 782)
# The Terminal's content (cells/apps/terminal: `move_to(140, 100)`); grid cell
# (c, r) starts at content + (PAD_X + c * CELL_W, PAD_Y + r * CELL_H).
TERM_X, TERM_Y = 140, 100
PAD_X, PAD_Y, CELL_W, CELL_H, ROWS = 8, 6, 9, 18, 26
OMNIBOX = (CHROME_X + 400, CHROME_Y + 62)
# Beside the Terminal (720x480 content plus its frame) but inside the Chrome window.
CHROME_ONLY = (CHROME_X + 1000, CHROME_Y + 600)
FROM_CHROMIUM = "copied in chromium"
FROM_TERMINAL = "example.com"


def text_rows(image) -> list:
    """Indexes of the Terminal grid rows that show anything (text or cursor)."""
    background = image.getpixel((TERM_X + 2, TERM_Y + 2))
    rows = []
    for row in range(ROWS):
        y0 = TERM_Y + PAD_Y + row * CELL_H
        if any(max(abs(a - b) for a, b in zip(image.getpixel((x, y)), background)) > 24
               for x in range(TERM_X + PAD_X, TERM_X + PAD_X + CELL_W * 40, 2)
               for y in range(y0 + 2, y0 + CELL_H - 2, 2)):
            rows.append(row)
    return rows


def run(vm, page_loaded) -> list:
    """Run the step in a session whose Chrome window is focused and shows
    Chromium. `page_loaded(name)` polls until Chromium shows the example page."""
    checks = []
    mark = len(vm.log())

    def seen(line: str, seconds: float) -> bool:
        return vm.wait_for(lambda t: line in t[mark:], seconds)

    vm.click(*OMNIBOX)
    vm.press("ctrl", "a")
    vm.type_text(FROM_CHROMIUM)
    vm.press("ctrl", "a")
    vm.press("ctrl", "c")
    size = len(FROM_CHROMIUM)
    checks.append(("chromium-copy-reaches-clipboard",
                   seen(f"[hv-x86] clipboard from guest: {size} bytes", 20)
                   and seen(f"[compositor] clipboard set: {size} bytes", 5)))
    vm.press("esc")

    vm.click(*TERMINAL_TASKBAR)
    ready = seen("[terminal] Terminal surface ready", 30)
    time.sleep(3)
    vm.press("ctrl", "shift", "v")
    checks.append(("chromium-copy-pastes-in-terminal",
                   ready and seen(f"[terminal] pasted {size} bytes", 15)))

    # The pasted words run as a command (not found); then the text to copy back.
    vm.press("ret")
    time.sleep(2)
    vm.type_text(f"echo {FROM_TERMINAL}")
    vm.press("ret")
    time.sleep(3)
    rows = text_rows(vm.screendump("terminal"))
    size = len(FROM_TERMINAL)
    copied = False
    # The echo's output is the row above the new prompt.
    if len(rows) >= 2:
        y = TERM_Y + PAD_Y + rows[-2] * CELL_H + CELL_H // 2
        vm.drag((TERM_X + PAD_X + 2, y), (TERM_X + PAD_X + CELL_W * 30, y))
        vm.press("ctrl", "shift", "c")
        copied = seen(f"[terminal] copied {size} bytes", 10)
    print(f"[tier3-chrome] terminal rows with text: {rows}", flush=True)
    checks.append(("terminal-copy-reaches-clipboard", copied))
    time.sleep(2)
    checks.append(("unfocused-guest-gets-no-clipboard",
                   f"[hv-x86] clipboard to guest: {size} bytes" not in vm.log()[mark:]))

    vm.click(*CHROME_ONLY)
    to_guest = seen(f"[hv-x86] clipboard to guest: {size} bytes", 15)
    # The guest's watcher hands the text back once wl-copy has set it: from then
    # on Ctrl+V pastes it. A fixed wait lost that race on a slow host.
    in_guest = seen(f"[hv-x86] clipboard from guest: {size} bytes", 20)
    vm.click(*OMNIBOX)
    vm.press("ctrl", "a")
    vm.press("ctrl", "v")
    time.sleep(1)
    vm.screendump("clipboard-pasted")
    vm.press("ret")
    checks.append(("terminal-copy-pastes-in-chromium",
                   to_guest and in_guest and page_loaded("clipboard-page")))
    return checks
