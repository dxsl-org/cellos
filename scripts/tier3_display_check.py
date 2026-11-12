"""Judge a Tier 3 display image under KVM (see qemu-tier3-display-x86.sh).

Two boots of the same image, both running the x86-gpu-pattern guest fixture:
  console  the serial shell's `hv` starts the hypervisor; the guest fills the
           screen (full-screen compositor surface);
  chrome   the taskbar's Chrome starts its own hypervisor; the guest shows in the
           Chrome window at the window's size, and the window menu's Close ends
           the guest and its hypervisor.
Exit 0 PASS, 1 FAIL.
"""
import argparse
import sys
import time
from pathlib import Path

from PIL import Image

from tier3_vm import SCREEN_H, SCREEN_W, Vm

TOP, BOTTOM = (64, 64, 64), (192, 192, 192)
MARKERS = ("X86_GPU_FB:", "X86_GPU_PATTERN_WRITTEN", "X86_INPUT_BOUND", "X86_DISPLAY_IRQ:",
           "X86_FPU_OK")
# The Chrome window (app.rs): 1024x680 content, centred on the 1280x800 screen.
CHROME_W, CHROME_H = 1024, 680
CHROME_X, CHROME_Y = (SCREEN_W - CHROME_W) // 2, (SCREEN_H - CHROME_H) // 2
CHROME_TASKBAR = (676, 782)
# Window menu (Cell Tab icon at content + (3, -25)); Close is the third item.
MENU_ICON = (CHROME_X + 3 + 10, CHROME_Y - 25 + 10)
MENU_CLOSE = (CHROME_X + 3 + 44, CHROME_Y - 25 + 20 + 4 + 4 + 2 * 28 + 9 + 14)


def pattern_mismatches(image: Image.Image, x0: int, y0: int, width: int, height: int) -> int:
    """Pixels of the guest pattern area that differ; rows 0..16 hold the cursor."""
    bad = 0
    for x in (x0 + 100, x0 + width // 2, x0 + width - 80):
        for y in range(y0 + 16, y0 + height):
            expected = TOP if y - y0 < height // 2 else BOTTOM
            bad += image.getpixel((x, y)) != expected
    return bad


def guest_checks(log: str, prefix: str) -> list:
    checks = [(f"{prefix}marker:" + m.rstrip(":"), m in log) for m in MARKERS]
    irq = next((line.split("X86_DISPLAY_IRQ:")[1].strip() for line in log.splitlines()
                if "X86_DISPLAY_IRQ:" in line), "")
    checks.append((f"{prefix}display-irq-delivered", irq.isdigit() and int(irq) > 0))
    checks.append((f"{prefix}no-fixture-failure", "X86_GPU_FAIL" not in log))
    checks.append((f"{prefix}guest-xmm-intact", "X86_FPU_CORRUPT" not in log))
    checks.append((f"{prefix}vmx-root", "root operation active (VMX)" in log))
    return checks


def fixture_done(log: str) -> bool:
    return "X86_FPU_" in log or "X86_GPU_FAIL" in log or "[hv-x86] guest exited" in log


def console(args) -> list:
    vm = Vm(args.qemu, args.iso, args.work, "console")
    checks = []
    try:
        ready = vm.wait_for(lambda t: "desktop] Ready" in t and "Cellos > " in t, args.window)
        checks.append(("console:desktop-ready", ready))
        if ready:
            time.sleep(2)
            before = pattern_mismatches(vm.screendump("before"), 0, 0, SCREEN_W, SCREEN_H)
            checks.append(("console:control-no-pattern-before-hv", before > 0))
            vm.type_serial("hv\r")
            checks.append(("console:fixture-finished", vm.wait_for(fixture_done, args.window)))
            time.sleep(3)
            after = pattern_mismatches(vm.screendump("after"), 0, 0, SCREEN_W, SCREEN_H)
            checks.append(("console:screen-shows-guest-pattern-exact", after == 0))
    finally:
        vm.stop()
    return checks + guest_checks(vm.log(), "console:")


def chrome(args) -> list:
    vm = Vm(args.qemu, args.iso, args.work, "chrome")
    checks = []
    try:
        ready = vm.wait_for(lambda t: "desktop] Ready" in t, args.window)
        checks.append(("chrome:desktop-ready", ready))
        if ready:
            time.sleep(2)
            vm.click(*CHROME_TASKBAR)
            bound = vm.wait_for(lambda t: "[chrome] guest session bound" in t, 30)
            checks.append(("chrome:guest-session-bound", bound))
            checks.append(("chrome:fixture-finished", vm.wait_for(fixture_done, args.window)))
            time.sleep(3)
            window = vm.screendump("window")
            bad = pattern_mismatches(window, CHROME_X, CHROME_Y, CHROME_W, CHROME_H)
            checks.append(("chrome:window-shows-guest-pattern-exact", bad == 0))
            outside = window.getpixel((CHROME_X - 40, CHROME_Y + 200))
            checks.append(("chrome:control-pattern-only-inside-window",
                           outside not in (TOP, BOTTOM)))
            vm.click(*MENU_ICON)
            time.sleep(1)
            vm.click(*MENU_CLOSE)
            ended = vm.wait_for(lambda t: "[hv-x86] guest exited" in t, 30)
            checks.append(("chrome:close-ends-guest", ended))
            time.sleep(2)
            gone = pattern_mismatches(vm.screendump("closed"), CHROME_X, CHROME_Y,
                                      CHROME_W, CHROME_H)
            checks.append(("chrome:window-gone-after-close", gone > 0))
    finally:
        vm.stop()
    log = vm.log()
    checks.append(("chrome:guest-display-is-window-size",
                   f"X86_GPU_FB:{CHROME_W}x{CHROME_H}:" in log))
    checks.append(("chrome:session-terminated-by-launcher",
                   "app session terminated by its launcher" in log))
    return checks + guest_checks(log, "chrome:")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--window", type=float, default=120)
    args = parser.parse_args()

    checks = console(args) + chrome(args)
    for name, passed in checks:
        print(("PASS " if passed else "FAIL ") + name)
    failed = [name for name, passed in checks if not passed]
    if failed:
        print(f"FAIL: {len(failed)} check(s); evidence in {args.work}", file=sys.stderr)
        return 1
    print(f"PASS: Tier 3 display lane ({len(checks)} checks); evidence in {args.work}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
