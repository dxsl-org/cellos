"""Judge the Tier 3 Chrome lane under KVM (see qemu-tier3-chrome-x86.sh).

One boot: the taskbar's Chrome starts its hypervisor and the Chromium guest; the
guest leases its own address beside the host stack; once Chromium's UI shows in
the window, the URL is typed into Chromium's omnibox and the page must render;
the window menu's Maximize must bring Chromium's UI to the new width; its Close
must end the launcher and the hypervisor. Screenshots and the serial log stay in
the work directory as evidence. Exit 0 PASS, 1 FAIL.
"""
import argparse
import re
import sys
import time
from pathlib import Path

from PIL import Image

from tier3_display_check import CHROME_H, CHROME_TASKBAR, CHROME_W, CHROME_X, CHROME_Y, ipv4
from tier3_vm import SCREEN_H, SCREEN_W, Vm

# Chromium's default light theme (sampled from phase 04's evidence).
TAB_STRIP, OMNIBOX = (211, 227, 253), (237, 242, 250)
# https://example.com/ paints its page background #eee in light mode.
EXAMPLE_BACKGROUND = (238, 238, 238)
PAGE_WINDOW = 300
# Any of these means the guest will not come up; stop waiting for it.
GUEST_DOWN = ("CHROME_GUEST_FAIL", "CHROME_GUEST_EXITED", "[chrome] guest not started",
              "[hv-x86] guest exited")


def guest_down(log: str) -> bool:
    return any(marker in log for marker in GUEST_DOWN)
RESIZE_WINDOW = 120
# Compositor geometry (window_decoration.rs): a 28px Cell Tab above the content,
# 1px border, 36px taskbar; a maximized frame fills the screen above the taskbar.
MAXIMIZED_X, MAXIMIZED_Y = 1, 28
MAXIMIZED_W = SCREEN_W - 2


def menu_point(content_x: int, content_y: int, item: int):
    """Window menu: Cell Tab icon at content + (3, -25); items 0, 1, separator, Close."""
    icon = (content_x + 3 + 10, content_y - 25 + 10)
    row = content_y - 25 + 20 + 4 + 4 + item * 28 + (9 if item >= 2 else 0) + 14
    return icon, (content_x + 3 + 44, row)


def window_menu(vm: Vm, content_x: int, content_y: int, item: int) -> None:
    icon, entry = menu_point(content_x, content_y, item)
    vm.click(*icon)
    time.sleep(1)
    vm.click(*entry)


def near(pixel, expected, tolerance=6) -> bool:
    return all(abs(a - b) <= tolerance for a, b in zip(pixel, expected))


def at(image: Image.Image, x: int, y: int):
    """Pixel at window-content coordinates."""
    return image.getpixel((CHROME_X + x, CHROME_Y + y))


def chromium_ui(image: Image.Image, x0: int = CHROME_X, y0: int = CHROME_Y,
                width: int = CHROME_W) -> bool:
    """Chromium's tab strip and omnibox, sampled near the content's right edge."""
    x = x0 + width - 330
    return (near(image.getpixel((x, y0 + 15)), TAB_STRIP)
            and near(image.getpixel((x, y0 + 62)), OMNIBOX))


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
        if guest_down(vm.log()):
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
        net_done = vm.wait_for(lambda t: "CHROME_GUEST_NET" in t or guest_down(t),
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

        window_menu(vm, CHROME_X, CHROME_Y, 0)
        resized_at = time.time()
        followed, _ = poll_screen(
            vm, "maximized",
            lambda img: chromium_ui(img, MAXIMIZED_X, MAXIMIZED_Y, MAXIMIZED_W), RESIZE_WINDOW)
        checks.append(("maximize-chromium-follows-new-width", followed))
        print(f"[tier3-chrome] resize followed after {time.time() - resized_at:.0f} s", flush=True)
        log = vm.log()
        checks.append(("no-guest-failure", not guest_down(log)))
        checks.append(("guest-followed-display-change", "CHROME_GUEST_DISPLAY " in log))

        window_menu(vm, MAXIMIZED_X, MAXIMIZED_Y, 2)
        checks.append(("close-ends-hypervisor", vm.wait_for(
            lambda t: "app session terminated by its launcher" in t
            and "[hv-x86] guest exited" in t, 30)))
        checks.append(("close-releases-guest-port", vm.wait_for(
            lambda t: "[net-bridge] guest port released" in t, 10)))
        # The launcher's surface goes with its Cell (compositor owner death); the
        # hypervisor reports its own end above. Both exits are kernel-logged.
        after_close = len(log)
        vm.wait_for(lambda t: t[after_close:].count("Syscall::Exit: task") >= 2, 15)
        tail = vm.log()[after_close:]
        checks.append(("close-both-cells-gone",
                       "[compositor] owner death" in tail
                       and tail.count("Syscall::Exit: task") >= 2))
    finally:
        vm.stop()
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
