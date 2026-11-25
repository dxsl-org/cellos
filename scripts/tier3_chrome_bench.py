"""Bench the Tier 3 Chrome guest (see qemu-tier3-chrome-bench-x86.sh).

Serves tests/guests/chrome-bench/bench.html to Chromium in the guest and records,
per run: the time from launch to Chromium's UI, the time from navigation to the
page's report, what the page measured, and the hypervisor's statistics lines
(`[hv-x86] vm … stats`, `[hv-x86] exits to the Cell`).

Targets are benched in turns, one run of each per round, because the build host
is not a quiet machine: only numbers taken side by side compare. Each
`--iso LABEL=PATH` is a CellOS desktop image; `--reference` adds the same guest
image straight under QEMU/KVM (microvm, virtio-mmio devices), without CellOS.
Every run also times a fixed loop on the build host (`host_ms`), so a loaded host
shows in the record.

Writes `bench.json` in the work directory and prints the medians.

`--soak SECONDS` (CellOS targets) keeps reloading the page in the same Chromium
until SECONDS have passed: animation, scrolling, timers and workers for minutes
on end. A round that does not report (a stall) fails the run, and so does a
lockup, RCU stall, segfault or oops in the serial log.

Exit 0 when every run reported, 1 otherwise.
"""
import argparse
import http.server
import json
import queue
import re
import statistics
import sys
import threading
import time
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

from tier3_chrome_check import chromium_ui, wait_for_chromium, window_menu
from tier3_display_check import CHROME_H, CHROME_TASKBAR, CHROME_W, CHROME_X, CHROME_Y
from tier3_vm import Vm

PAGE = Path(__file__).resolve().parent.parent / "tests/guests/chrome-bench/bench.html"
# QEMU's user network presents the build host's loopback at this address.
GATEWAY = "10.0.2.2"
UI_WINDOW, REPORT_WINDOW = 300, 300
STATS = re.compile(r"\[hv-x86\] vm \d+ vcpu (\d+) stats: (.*)")
CELL_EXITS = re.compile(r"\[hv-x86\] exits to the Cell: (.*)")
# What a guest (or the host kernel) prints when it stalls or crashes.
TROUBLE = re.compile(r"soft lockup|hard LOCKUP|detected stalls?|rcu: INFO|segfault|Oops|BUG: ")
# Printed first; every other key follows in name order.
HEADLINE = ("host_ms", "ui_s", "report_s", "cores", "raf_frames", "raf_p95", "timer_p95",
            "js1_ms", "jsN_ms", "scroll_frames", "scroll_p95", "paint_frames", "paint_p95")


class ReferenceVm(Vm):
    """The guest image straight under QEMU/KVM: no CellOS, the screen is the guest's."""
    screen = (CHROME_W, CHROME_H)

    def __init__(self, qemu: str, kernel: Path, initrd: Path, image: Path, work: Path,
                 name: str, memory: str, smp: int):
        self.start(work, name, [
            qemu, "-M", "microvm,acpi=off", "-global", "virtio-mmio.force-legacy=false",
            "-accel", "kvm", "-cpu", "host", "-m", memory, "-smp", str(smp),
            "-kernel", str(kernel), "-initrd", str(initrd),
            "-append", "console=ttyS0 loglevel=4 rdinit=/bin/chrome-guest-stage1 panic=1",
            "-drive", f"id=root,file={image},format=raw,if=none,readonly=on",
            "-device", "virtio-blk-device,drive=root",
            "-device", f"virtio-gpu-device,xres={CHROME_W},yres={CHROME_H}",
            "-device", "virtio-keyboard-device", "-device", "virtio-tablet-device",
            "-netdev", "user,id=net", "-device", "virtio-net-device,netdev=net"])


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url = urlsplit(self.path)
        if url.path == "/report":
            self.server.reports.put(dict(parse_qsl(url.query)))
            self.send_response(204)
            self.end_headers()
        elif url.path == "/":
            body = PAGE.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_error(404)

    def log_message(self, *args):
        pass


def host_ms() -> float:
    """A fixed loop on the build host: its time moves with what else the host is doing."""
    started = time.perf_counter()
    total = 0
    for value in range(10_000_000):
        total += value
    return round((time.perf_counter() - started) * 1000)


def numbers(text: str) -> dict:
    """`a=1 b=2` as a dict of integers."""
    return {key: int(value) for key, value in re.findall(r"([\w-]+)=(\d+)", text)}


def navigate(vm: Vm, server, x0: int, y0: int) -> dict:
    """Type the bench URL into Chromium's omnibox and wait for the page's report."""
    while not server.reports.empty():
        server.reports.get()
    vm.click(x0 + 400, y0 + 62)
    vm.press("ctrl", "a")
    vm.type_text(f"http://{GATEWAY}:{server.server_port}/")
    vm.press("ret")
    asked = time.time()
    report = server.reports.get(timeout=REPORT_WINDOW)
    if "error" in report:
        raise RuntimeError(f"bench page failed: {report['error']}")
    result = {key: float(value) for key, value in report.items()}
    result["report_s"] = round(time.time() - asked, 1)
    return result


def soak(vm: Vm, server, seconds: int) -> dict:
    """Reload the bench page until `seconds` have passed; see the module note."""
    started = time.time()
    rounds, slowest = 0, 0.0
    while time.time() - started < seconds:
        report = navigate(vm, server, CHROME_X, CHROME_Y)
        rounds += 1
        slowest = max(slowest, report["report_s"])
    trouble = sorted(set(TROUBLE.findall(vm.log())))
    if trouble:
        raise RuntimeError(f"trouble in the serial log during the soak: {trouble}")
    return {"soak_s": round(time.time() - started), "soak_rounds": rounds,
            "soak_slowest_report_s": slowest}


def run_cellos(args, server, name: str, iso: Path) -> dict:
    extra = ("-smp", str(args.smp), "-drive", f"file={args.disk},if=none,id=nvme0,format=raw",
             "-device", "nvme,drive=nvme0,serial=CELLOSSD")
    vm = Vm(args.qemu, iso, args.work, name, memory=args.memory, extra=extra)
    try:
        if not vm.wait_for(lambda t: "desktop] Ready" in t, 180):
            raise RuntimeError("the desktop did not come up")
        time.sleep(2)
        launched = time.time()
        vm.click(*CHROME_TASKBAR)
        ui, _, _ = wait_for_chromium(vm, "ui", UI_WINDOW)
        if not ui:
            raise RuntimeError("Chromium's UI did not appear")
        result = {"ui_s": round(time.time() - launched, 1)}
        if args.soak:
            result.update(soak(vm, server, args.soak))
        elif not args.launch_only:
            result.update(navigate(vm, server, CHROME_X, CHROME_Y))
        # Closing the window ends the VM, which is when the statistics are printed.
        window_menu(vm, CHROME_X, CHROME_Y, 2)
        vm.wait_for(lambda t: STATS.search(t) is not None, 30)
        log = vm.log()
        for vcpu, text in STATS.findall(log):
            result.update({f"vcpu{vcpu}_{key}": value for key, value in numbers(text).items()})
        cell = CELL_EXITS.search(log)
        if cell:
            result.update({f"cell_{key}": value for key, value in numbers(cell.group(1)).items()})
        return result
    finally:
        vm.stop()


def run_reference(args, server, name: str) -> dict:
    vm = ReferenceVm(args.qemu, args.kernel, args.initrd, args.image, args.work, name,
                     args.guest_memory, args.guest_smp)
    try:
        launched = time.time()
        # The QMP socket and the guest's display exist by the time cage starts.
        if not vm.wait_for(lambda t: "CHROME_GUEST_STARTING_CAGE" in t, UI_WINDOW):
            raise RuntimeError("the guest did not reach cage")
        deadline = launched + UI_WINDOW
        while True:
            image = vm.screendump("ui")
            if image.size == (CHROME_W, CHROME_H) and chromium_ui(image, 0, 0, CHROME_W):
                break
            if time.time() > deadline or vm.process.poll() is not None:
                raise RuntimeError("Chromium's UI did not appear")
            time.sleep(0.25)
        result = {"ui_s": round(time.time() - launched, 1)}
        if not args.launch_only:
            result.update(navigate(vm, server, 0, 0))
        return result
    finally:
        vm.stop()


def labelled_iso(text: str) -> tuple:
    label, separator, path = text.partition("=")
    if not separator or not label or label == "reference":
        raise argparse.ArgumentTypeError("expected LABEL=PATH, LABEL other than `reference`")
    return label, Path(path)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--rounds", type=int, default=3, help="runs of every target")
    parser.add_argument("--iso", action="append", default=[], type=labelled_iso,
                        metavar="LABEL=PATH", help="a CellOS desktop ISO to bench (repeatable)")
    parser.add_argument("--disk", type=Path, help="Chrome guest disk for the CellOS targets")
    parser.add_argument("--smp", type=int, default=4, help="CPUs of the CellOS machine")
    parser.add_argument("--memory", default="6G", help="memory of the CellOS machine")
    parser.add_argument("--launch-only", action="store_true",
                        help="stop at Chromium's UI: the statistics then cover the launch alone")
    parser.add_argument("--soak", type=int, metavar="SECONDS",
                        help="reload the page for SECONDS in one Chromium (CellOS targets)")
    parser.add_argument("--reference", action="store_true", help="also bench without CellOS")
    parser.add_argument("--kernel", type=Path, help="guest kernel (reference)")
    parser.add_argument("--initrd", type=Path, help="Chrome guest initramfs (reference)")
    parser.add_argument("--image", type=Path, help="chrome-guest.sqfs (reference)")
    parser.add_argument("--guest-smp", type=int, default=1, help="vCPUs of the reference guest")
    parser.add_argument("--guest-memory", default="3072M", help="memory of the reference guest")
    args = parser.parse_args()

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.reports = queue.Queue()
    threading.Thread(target=server.serve_forever, daemon=True).start()
    targets = {label: (lambda name, iso=iso: run_cellos(args, server, name, iso))
               for label, iso in args.iso}
    if args.reference:
        targets["reference"] = lambda name: run_reference(args, server, name)
    runs = {label: [] for label in targets}
    failed = 0
    for round_index in range(args.rounds):
        for label, run in targets.items():
            calibration = host_ms()
            try:
                result = {"host_ms": calibration, **run(f"bench-{label}-{round_index}")}
                runs[label].append(result)
                print(f"[bench] {label} round {round_index + 1}: {json.dumps(result)}", flush=True)
            except (RuntimeError, queue.Empty) as error:
                failed += 1
                print(f"[bench] {label} round {round_index + 1} failed: {error!r}", flush=True)
    server.shutdown()

    median = {label: {key: statistics.median(result[key] for result in results if key in result)
                      for key in {key for result in results for key in result}}
              for label, results in runs.items()}
    out = args.work / "bench.json"
    out.write_text(json.dumps({"smp": args.smp, "memory": args.memory, "guest_smp": args.guest_smp,
                               "guest_memory": args.guest_memory, "runs": runs, "median": median},
                              indent=1, sort_keys=True) + "\n")
    every = {key for values in median.values() for key in values}
    keys = [key for key in HEADLINE if key in every] + sorted(every - set(HEADLINE))
    # A target that failed a round has fewer runs behind its median: say how many.
    print(f"{'median (runs)':<22}"
          + "".join(f"{label + ' (' + str(len(runs[label])) + ')':>14}" for label in median))
    for key in keys:
        cells = (f"{values[key]:>14.1f}" if key in values else f"{'-':>14}"
                 for values in median.values())
        print(f"{key:<22}" + "".join(cells))
    reported = sum(len(results) for results in runs.values())
    print(f"[bench] {reported} run(s) reported, {failed} failed; {out}")
    return 1 if failed or not reported else 0


if __name__ == "__main__":
    sys.exit(main())
