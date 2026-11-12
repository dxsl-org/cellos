"""Judge the Tier 3 Chrome lane under KVM (see qemu-tier3-chrome-x86.sh).

One boot: the taskbar's Chrome starts its hypervisor and the Chromium guest; the
guest leases its own address beside the host stack; once Chromium's UI shows in
the window, the URL is typed into Chromium's omnibox and the page must render.
Screenshots and the serial log stay in the work directory as evidence.
Exit 0 PASS, 1 FAIL.
"""
import argparse
import re
import sys
import time
from pathlib import Path

from PIL import Image

from tier3_display_check import CHROME_H, CHROME_TASKBAR, CHROME_W, CHROME_X, CHROME_Y, ipv4
from tier3_vm import Vm

# Chromium's default light theme (sampled from phase 04's evidence).
TAB_STRIP, OMNIBOX = (211, 227, 253), (237, 242, 250)
# https://example.com/ paints its page background #eee in light mode.
EXAMPLE_BACKGROUND = (238, 238, 238)
PAGE_WINDOW = 300


def near(pixel, expected, tolerance=6) -> bool:
    return all(abs(a - b) <= tolerance for a, b in zip(pixel, expected))


def at(image: Image.Image, x: int, y: int):
    """Pixel at window-content coordinates."""
    return image.getpixel((CHROME_X + x, CHROME_Y + y))


def chromium_ui(image: Image.Image) -> bool:
    return near(at(image, 700, 15), TAB_STRIP) and near(at(image, 700, 62), OMNIBOX)


def page_share(image: Image.Image, colour) -> float:
    """Share of the page area (below Chromium's bars) painted `colour`."""
    points = [(x, y) for x in range(20, CHROME_W - 20, 20) for y in range(200, CHROME_H - 20, 10)]
    return sum(at(image, x, y) == colour for x, y in points) / len(points)


def poll_screen(vm: Vm, name: str, predicate, seconds: float):
    deadline = time.time() + seconds
    image = None
    while time.time() < deadline:
        image = vm.screendump(name)
        if predicate(image):
            return True, image
        if "CHROME_GUEST_FAIL" in vm.log() or "CHROME_GUEST_EXITED" in vm.log():
            break
        time.sleep(10)
    return False, image


def run(args) -> list:
    extra = ("-drive", f"file={args.disk},if=none,id=nvme0,format=raw",
             "-device", "nvme,drive=nvme0,serial=CELLOSSD")
    vm = Vm(args.qemu, args.iso, args.work, "chrome", memory="3G", extra=extra)
    checks = []
    try:
        ready = vm.wait_for(lambda t: "desktop] Ready" in t, args.boot_window)
        checks.append(("desktop-ready", ready))
        if not ready:
            return checks
        time.sleep(2)
        started = time.time()
        vm.click(*CHROME_TASKBAR)
        checks.append(("guest-session-bound",
                       vm.wait_for(lambda t: "[chrome] guest session bound" in t, 60)))
        net_done = vm.wait_for(lambda t: "CHROME_GUEST_NET" in t or "CHROME_GUEST_FAIL" in t,
                               args.chrome_window)
        log = vm.log()
        host = re.search(r"\[net\] IP address: (\S+)", log)
        lease = re.search(r"CHROME_GUEST_NET lease=(\S+) dns=(\S+)", log)
        host_ip = ipv4(host.group(1)) if host else None
        guest_ip = ipv4(lease.group(1).split("/")[0]) if lease else None
        checks.append(("guest-own-lease", net_done and guest_ip is not None
                       and host_ip is not None and guest_ip != host_ip))
        checks.append(("guest-resolver", lease is not None and ipv4(lease.group(2)) is not None))
        print(f"[tier3-chrome] host {host_ip}, guest {guest_ip}", flush=True)

        remaining = max(args.chrome_window - (time.time() - started), 60)
        ui, image = poll_screen(vm, "ui", chromium_ui, remaining)
        checks.append(("chromium-ui-in-window", ui))
        print(f"[tier3-chrome] Chromium UI after {time.time() - started:.0f} s", flush=True)
        if not ui:
            return checks
        checks.append(("control-not-example-before", page_share(image, EXAMPLE_BACKGROUND) < 0.5))

        vm.click(CHROME_X + 400, CHROME_Y + 62)
        vm.press("ctrl", "a")
        vm.type_text(args.url)
        vm.press("ret")
        shown, _ = poll_screen(vm, "page",
                               lambda img: page_share(img, EXAMPLE_BACKGROUND) >= 0.8,
                               PAGE_WINDOW)
        checks.append(("https-page-rendered", shown))
    finally:
        vm.stop()
        log = vm.log()
        checks.append(("no-guest-failure",
                       "CHROME_GUEST_FAIL" not in log and "CHROME_GUEST_EXITED" not in log))
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--disk", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--boot-window", type=float, default=180)
    parser.add_argument("--chrome-window", type=float, default=1800)
    parser.add_argument("--url", default="https://example.com/")
    args = parser.parse_args()

    checks = run(args)
    for name, passed in checks:
        print(("PASS " if passed else "FAIL ") + name)
    failed = [name for name, passed in checks if not passed]
    if failed:
        print(f"FAIL: {len(failed)} check(s); evidence in {args.work}", file=sys.stderr)
        return 1
    print(f"PASS: Tier 3 Chrome lane ({len(checks)} checks); evidence in {args.work}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
