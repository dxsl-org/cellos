"""Judge the Tier 3 Chrome lane under KVM (see qemu-tier3-chrome-x86.sh).

One boot: the taskbar's Chrome starts its hypervisor and the Chromium guest; the
guest leases its own address beside the host stack; the window shows the
launcher's status, never a blank guest screen, until Chromium's UI is in it; the
URL is typed into Chromium's omnibox and the page must render. The window shows
Chromium and nothing else of the guest: no window buttons or warning bar of
Chromium's own, no guest pointer, and Ctrl+Alt+F2 reaches no Linux console. The
window menu's Maximize must bring Chromium's UI to the new width; its Close must
end the launcher and the hypervisor. A second session carries text both ways
between Chromium and the Terminal (tier3_chrome_clipboard) and is then ended from inside
(Chromium's close-window key), which must close the window too. Screenshots and
the serial log stay in the work directory as evidence. Exit 0 PASS, 1 FAIL.
"""
import argparse
import re
import sys
import time
from pathlib import Path

from PIL import Image

import tier3_chrome_clipboard
import tier3_chrome_files
from tier3_display_check import CHROME_H, CHROME_TASKBAR, CHROME_W, CHROME_X, CHROME_Y, ipv4
from tier3_vm import SCREEN_H, SCREEN_W, Vm

# Chromium's default light theme (sampled from phase 04's evidence). The omnibox
# is white while it holds keyboard focus, which it does when Chromium is up
# before the window loses focus.
TAB_STRIP = (211, 227, 253)
OMNIBOX, OMNIBOX_FOCUSED = (237, 242, 250), (255, 255, 255)
# https://example.com/ paints its page background #eee in light mode.
EXAMPLE_BACKGROUND = (238, 238, 238)
BLACK = (0, 0, 0)
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
            and any(near(image.getpixel((x, y0 + 62)), colour)
                    for colour in (OMNIBOX, OMNIBOX_FOCUSED)))


def page_share(image: Image.Image, colour) -> float:
    """Share of the page area (below Chromium's bars) painted `colour`."""
    points = [(x, y) for x in range(20, CHROME_W - 20, 20) for y in range(200, CHROME_H - 20, 10)]
    return sum(at(image, x, y) == colour for x, y in points) / len(points)


def uniform(image: Image.Image, colour, box, step: int = 4) -> bool:
    """Whether screen rectangle `box` (x0, y0, x1, y1) is `colour` at every sampled pixel."""
    x0, y0, x1, y1 = box
    return all(near(image.getpixel((x, y)), colour)
               for x in range(x0, x1, step) for y in range(y0, y1, step))


def pointer_pixels(vm: Vm, name: str, spot) -> frozenset:
    """The pointer's shape at `spot`: offsets from `spot` of the pixels that
    change when the pointer leaves the window (to the screen's left margin)."""
    vm.move(*spot)
    time.sleep(2)
    with_pointer = vm.screendump(name)
    vm.move(60, spot[1])
    time.sleep(2)
    without = vm.screendump(f"{name}-away")
    x0, y0 = spot
    return frozenset((dx, dy) for dx in range(-24, 40) for dy in range(-24, 40)
                     if not near(with_pointer.getpixel((x0 + dx, y0 + dy)),
                                 without.getpixel((x0 + dx, y0 + dy)), 12))


def extent(pixels: frozenset) -> tuple:
    """Width and height of the box around `pixels`; (0, 0) when empty."""
    if not pixels:
        return (0, 0)
    xs, ys = [x for x, _ in pixels], [y for _, y in pixels]
    return (max(xs) - min(xs) + 1, max(ys) - min(ys) + 1)


def poll_screen(vm: Vm, name: str, predicate, seconds: float, interval: float = 10,
                since: int = 0):
    """Screenshot every `interval` s until `predicate` holds; stop early when the
    guest is down in the log after offset `since` (where this session started:
    an earlier session's exit must not end the wait)."""
    deadline = time.time() + seconds
    image = None
    while time.time() < deadline:
        image = vm.screendump(name)
        if predicate(image):
            return True, image
        if guest_down(vm.log()[since:]):
            break
        time.sleep(interval)
    return False, image


def wait_for_chromium(vm: Vm, name: str, seconds: float, since: int = 0):
    """Watch the window, once a second, until Chromium's UI is in it.

    Returns (UI shown, blank seen, last screen). `blank seen`: the window showed a
    black guest screen (the scanout before Chromium's first frame) instead of the
    launcher's status. `since` is the log offset this session started at."""
    deadline = time.time() + seconds
    blank, image = False, None
    while time.time() < deadline:
        image = vm.screendump(name)
        if chromium_ui(image):
            return True, blank, image
        blank = blank or page_share(image, BLACK) >= 0.9
        if guest_down(vm.log()[since:]):
            break
        time.sleep(1)
    return False, blank, image


def run(args) -> list:
    # Eight CPUs: the Chrome guest gets one vCPU per two, up to four
    # (`GuestProfile::vcpus`).
    extra = ("-smp", "8", "-drive", f"file={args.disk},if=none,id=nvme0,format=raw",
             "-device", "nvme,drive=nvme0,serial=CELLOSSD",
             # Phase 10: the data disk (VFS: /mnt/disk/<device>). On q35's AHCI port 1: each
             # block driver Cell drives one controller, so a second NVMe would go unserved.
             "-drive", f"file={args.files_disk},if=none,id=files,format=raw",
             "-device", "ide-hd,drive=files,bus=ide.1")
    # The Chrome guest takes up to 3072 MiB in one run (`CHROME_RAM_SIZES`).
    vm = Vm(args.qemu, args.iso, args.work, "chrome", memory="6G", extra=extra)
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
        # The window is up from here on.
        ui, blank, image = wait_for_chromium(vm, "ui", args.chrome_window)
        log = vm.log()
        host = re.search(r"\[net\] IP address: (\S+)", log)
        lease = re.search(r"CHROME_GUEST_NET lease=(\S+) dns=(\S+)", log)
        host_ip = ipv4(host.group(1)) if host else None
        guest_ip = ipv4(lease.group(1).split("/")[0]) if lease else None
        checks.append(("guest-own-lease", guest_ip is not None
                       and host_ip is not None and guest_ip != host_ip))
        checks.append(("guest-resolver", lease is not None and ipv4(lease.group(2)) is not None))
        print(f"[tier3-chrome] host {host_ip}, guest {guest_ip}", flush=True)
        # Phase 06: a 3072 MiB guest shows about 2950 MiB; 1536 MiB showed 1475.
        memory = re.search(r"CHROME_GUEST_DEVICES mem_total_mib=(\d+)", log)
        checks.append(("guest-memory-3g", memory is not None and int(memory.group(1)) >= 2800))
        # Phase 04: the guest's clock event is its local APIC, in deadline mode.
        checks.append(("guest-apic-deadline-timer",
                       "CHROME_GUEST_CLOCK event=lapic-deadline" in log and "hres=1" in log))
        # Phase 07: AVX and AVX2 in the guest's /proc/cpuinfo.
        checks.append(("guest-avx2", "CHROME_GUEST_SIMD avx=1 avx2=1" in log))
        print(f"[tier3-chrome] guest memory {memory.group(1) if memory else '?'} MiB", flush=True)
        # Phase 05: four vCPUs, all of them started by the guest.
        cpus = re.search(r"CHROME_GUEST_DEVICES .*cpus=(\d+)", log)
        checks.append(("guest-four-vcpus", cpus is not None and int(cpus.group(1)) == 4))
        print(f"[tier3-chrome] guest CPUs {cpus.group(1) if cpus else '?'}", flush=True)
        checks.append(("chromium-ui-in-window", ui))
        print(f"[tier3-chrome] Chromium UI after {time.time() - started:.0f} s", flush=True)
        if not ui:
            return checks
        checks.append(("status-until-chromium-no-blank-guest-screen", not blank))
        # Where Chromium's own minimize, maximize and close buttons would be.
        checks.append(("chromium-draws-no-window-buttons", uniform(
            image, TAB_STRIP,
            (CHROME_X + CHROME_W - 110, CHROME_Y + 8, CHROME_X + CHROME_W - 6, CHROME_Y + 30))))
        checks.append(("control-not-example-before", page_share(image, EXAMPLE_BACKGROUND) < 0.5))

        vm.click(CHROME_X + 400, CHROME_Y + 62)
        vm.press("ctrl", "a")
        vm.type_text(args.url)
        vm.press("ret")
        shown, image = poll_screen(vm, "page",
                                   lambda img: page_share(img, EXAMPLE_BACKGROUND) >= 0.8,
                                   PAGE_WINDOW)
        checks.append(("https-page-rendered", shown))
        # The page starts right under the toolbar: no warning bar above it.
        checks.append(("chromium-shows-no-warning-bar", uniform(
            image, EXAMPLE_BACKGROUND,
            (CHROME_X + 20, CHROME_Y + 96, CHROME_X + 600, CHROME_Y + 130))))
        # The pointer is the host's. The guest keeps the last position it was sent,
        # so a pointer of its own would stay drawn there after the host's left.
        spot = (CHROME_X + 60, CHROME_Y + 400)
        vm.move(*spot)
        time.sleep(1)
        vm.move(60, spot[1])
        time.sleep(2)
        checks.append(("guest-draws-no-pointer", uniform(
            vm.screendump("pointer"), EXAMPLE_BACKGROUND,
            (spot[0] - 24, spot[1] - 24, spot[0] + 24, spot[1] + 24), step=1)))
        # Phase 08: the pointer over the window wears the guest's cursor: an
        # arrow over the page's empty background, an I-beam over the omnibox;
        # each one pointer no larger than a cursor (32x32), not a 64x64 block.
        over_page = pointer_pixels(vm, "pointer-page", spot)
        over_omnibox = pointer_pixels(vm, "pointer-omnibox", (CHROME_X + 400, CHROME_Y + 62))
        print(f"[tier3-chrome] pointer pixels: page {len(over_page)} in {extent(over_page)}, "
              f"omnibox {len(over_omnibox)} in {extent(over_omnibox)}", flush=True)
        checks.append(("pointer-follows-guest-cursor",
                       0 < max(extent(over_page)) <= 32 and 0 < max(extent(over_omnibox)) <= 32
                       and over_page != over_omnibox))
        vm.press("ctrl", "alt", "f2")
        time.sleep(3)
        checks.append(("ctrl-alt-f2-reaches-no-linux-console",
                       page_share(vm.screendump("vt"), EXAMPLE_BACKGROUND) >= 0.8))

        window_menu(vm, CHROME_X, CHROME_Y, 0)
        resized_at = time.time()
        # The old picture stays in the window until the guest redraws, so the tab
        # strip must reach the new right edge (the guest's width is a multiple of 8).
        followed, _ = poll_screen(
            vm, "maximized",
            lambda img: chromium_ui(img, MAXIMIZED_X, MAXIMIZED_Y, MAXIMIZED_W)
            and near(img.getpixel((MAXIMIZED_X + MAXIMIZED_W - 40, MAXIMIZED_Y + 15)), TAB_STRIP),
            RESIZE_WINDOW, interval=1)
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
        # The hypervisor names its launcher's tid when the session binds; both
        # Cells' exits are kernel-logged after Close.
        bound = re.search(r"app session bound to tid=(\d+)", log)
        after_close = len(log)
        vm.wait_for(lambda t: t[after_close:].count("Syscall::Exit: task") >= 2, 15)
        tail = vm.log()[after_close:]
        checks.append(("close-both-cells-gone",
                       bound is not None
                       and f"Syscall::Exit: task {bound.group(1)} exited" in tail
                       and tail.count("Syscall::Exit: task") >= 2))

        # A second session, ended from inside: Chromium's close-window key ends
        # Chromium, the guest powers off, and the window goes with it.
        second = len(vm.log())
        vm.click(*CHROME_TASKBAR)
        vm.wait_for(lambda t: "[chrome] guest session bound" in t[second:], 60)
        ui, _, _ = wait_for_chromium(vm, "relaunch", args.chrome_window, second)
        checks.append(("relaunch-shows-chromium", ui))
        if not ui:
            return checks
        bound = re.search(r"app session bound to tid=(\d+)", vm.log()[second:])
        # Phase 09, on the fresh window (it shows about:blank).
        checks.extend(tier3_chrome_clipboard.run(vm, lambda name: poll_screen(
            vm, name, lambda img: page_share(img, EXAMPLE_BACKGROUND) >= 0.8,
            PAGE_WINDOW, interval=2, since=second)[0]))
        # Phase 10, in the same session.
        checks.extend(tier3_chrome_files.run(vm, lambda name, colour: poll_screen(
            vm, name, lambda img: page_share(img, colour) >= 0.8,
            60, interval=2, since=second)[0]))
        vm.press("ctrl", "shift", "w")
        checks.append(("chromium-quit-ends-guest", vm.wait_for(
            lambda t: "CHROME_GUEST_EXITED" in t[second:]
            and "[hv-x86] guest exited" in t[second:], 60)))
        closed = bound is not None and vm.wait_for(
            lambda t: f"Syscall::Exit: task {bound.group(1)} exited" in t[second:], 15)
        time.sleep(1)
        checks.append(("chromium-quit-closes-window",
                       closed and not chromium_ui(vm.screendump("closed"))))
    finally:
        vm.stop()
    # The data disk is consistent only once the machine has stopped.
    if any(name == "guest-mounts-downloads" for name, _ in checks):
        checks.append(tier3_chrome_files.downloaded(args.files_disk, args.work))
    return checks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--disk", required=True, type=Path)
    parser.add_argument("--files-disk", required=True, type=Path)
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
