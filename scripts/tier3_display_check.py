"""Judge a Tier 3 display image under KVM (see qemu-tier3-display-x86.sh).

Two boots of the same image, both running the x86-gpu-pattern guest fixture:
  console  the serial shell's `hv` starts the hypervisor; the guest fills the
           screen (full-screen compositor surface);
  chrome   the taskbar's Chrome starts its own hypervisor; the guest shows in the
           Chrome window at the window's size; Ocel fetches a page through the
           host stack while the guest runs; the window menu's Close ends the
           guest and its hypervisor.
In both, the guest takes its own DHCP lease (not the host stack's), resolves a
name and fetches a page over the net service's bridge.
Exit 0 PASS, 1 FAIL.
"""
import argparse
import ipaddress
import re
import sys
import time
from pathlib import Path

from PIL import Image

from tier3_vm import SCREEN_H, SCREEN_W, Vm

TOP, BOTTOM = (64, 64, 64), (192, 192, 192)
MARKERS = ("X86_GPU_FB:", "X86_GPU_PATTERN_WRITTEN", "X86_INPUT_BOUND", "X86_DISPLAY_IRQ:",
           "X86_FPU_OK", "X86_NET_HTTP_OK")
OCEL_URL = "http://example.com/"
# The Chrome window (app.rs): 1024x680 content, centred on the 1280x800 screen.
CHROME_W, CHROME_H = 1024, 680
CHROME_X, CHROME_Y = (SCREEN_W - CHROME_W) // 2, (SCREEN_H - CHROME_H) // 2
CHROME_TASKBAR = (676, 782)
OCEL_TASKBAR = (368, 782)
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


def marker_value(log: str, marker: str) -> str:
    return next((line.split(marker, 1)[1].strip() for line in log.splitlines()
                 if marker in line), "")


def ipv4(text: str):
    try:
        return ipaddress.IPv4Address(text)
    except ValueError:
        return None


def guest_checks(log: str, prefix: str) -> list:
    checks = [(f"{prefix}marker:" + m.rstrip(":"), m in log) for m in MARKERS]
    irq = marker_value(log, "X86_DISPLAY_IRQ:")
    checks.append((f"{prefix}display-irq-delivered", irq.isdigit() and int(irq) > 0))
    checks.append((f"{prefix}no-fixture-failure", "X86_GPU_FAIL" not in log))
    checks.append((f"{prefix}guest-xmm-intact", "X86_FPU_CORRUPT" not in log))
    # Phase 07 of the Chrome plan: the VMX guest has AVX, and all 256 bits of
    # ymm0..ymm15 survive VM exits (fpu-check.S).
    checks.append((f"{prefix}guest-ymm-intact", "X86_AVX_OK" in log))
    checks.append((f"{prefix}vmx-root", "root operation active (VMX)" in log))
    host = re.search(r"\[net\] IP address: (\S+)", log)
    host_ip = ipv4(host.group(1)) if host else None
    guest_ip = ipv4(marker_value(log, "X86_NET_LEASE:").split("/")[0])
    checks.append((f"{prefix}host-stack-lease", host_ip is not None))
    checks.append((f"{prefix}guest-own-lease",
                   guest_ip is not None and host_ip is not None and guest_ip != host_ip))
    checks.append((f"{prefix}guest-dns", ipv4(marker_value(log, "X86_NET_DNS:")) is not None))
    return checks


def fixture_done(log: str) -> bool:
    return "X86_NET_HTTP_OK" in log or "X86_GPU_FAIL" in log or "[hv-x86] guest exited" in log


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
            # The host stack serves Ocel while the guest's port is open. Ocel is
            # opened from the taskbar: with a window focused the serial shell does
            # not get the serial input. Ocel's window opens over Chrome's at the same place,
            # so the first window menu Close below closes Ocel and the second
            # closes Chrome.
            vm.click(*OCEL_TASKBAR)
            if vm.wait_for(lambda t: "[ocel] Window initialized and painted." in t, 30):
                time.sleep(1)
                vm.press("ctrl", "l")
                vm.type_text(OCEL_URL)
                vm.press("ret")
            checks.append(("chrome:host-stack-ocel-fetch-while-guest-runs", vm.wait_for(
                lambda t: f"[ocel] loaded {OCEL_URL}" in t, 60)))
            for _ in ("ocel", "chrome"):
                vm.click(*MENU_ICON)
                time.sleep(1)
                vm.click(*MENU_CLOSE)
                time.sleep(2)
            ended = vm.wait_for(lambda t: "[hv-x86] guest exited" in t, 30)
            checks.append(("chrome:close-ends-guest", ended))
            checks.append(("chrome:guest-port-released", vm.wait_for(
                lambda t: "[net-bridge] guest port released" in t, 10)))
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
