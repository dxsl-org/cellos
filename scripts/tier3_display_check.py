"""Boot a Tier 3 display image under KVM and judge it (see qemu-tier3-display-x86.sh).

Starts the hypervisor with the serial shell's `hv`, waits for the guest fixture's
markers, and compares the screen with the guest's two-band pattern. Exit 0 PASS,
1 FAIL.
"""
import argparse
import json
import socket
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image

TOP, BOTTOM = (64, 64, 64), (192, 192, 192)
MARKERS = ("X86_GPU_FB:", "X86_GPU_PATTERN_WRITTEN", "X86_INPUT_BOUND", "X86_DISPLAY_IRQ:")
# Columns checked across the full height; the software cursor sits in the top-left corner.
COLUMNS = (100, 640, 1200)


class Vm:
    def __init__(self, qemu: str, iso: Path, work: Path):
        self.work = work
        self.serial_log = work / "serial.log"
        for name in ("serial.sock", "qmp.sock", "serial.log"):
            (work / name).unlink(missing_ok=True)
        self.process = subprocess.Popen([
            qemu, "-machine", "q35", "-accel", "kvm", "-cpu", "host", "-m", "1G",
            "-display", "none", "-chardev",
            f"socket,id=s0,path={work}/serial.sock,server=on,wait=off,logfile={self.serial_log}",
            "-serial", "chardev:s0", "-qmp", f"unix:{work}/qmp.sock,server=on,wait=off",
            "-monitor", "none", "-cdrom", str(iso), "-boot", "d", "-no-reboot", "-nic", "none",
            "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"],
            stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)

    def log(self) -> str:
        try:
            return self.serial_log.read_bytes().decode(errors="replace")
        except FileNotFoundError:
            return ""

    def wait_for(self, predicate, seconds: float) -> bool:
        deadline = time.time() + seconds
        while time.time() < deadline:
            if predicate(self.log()):
                return True
            time.sleep(0.5)
        return False

    def type_serial(self, text: str) -> None:
        with socket.socket(socket.AF_UNIX) as sock:
            sock.connect(str(self.work / "serial.sock"))
            for byte in text.encode():
                sock.send(bytes([byte]))
                time.sleep(0.05)

    def screendump(self, name: str) -> Image.Image:
        path = self.work / name
        with socket.socket(socket.AF_UNIX) as sock:
            sock.connect(str(self.work / "qmp.sock"))
            stream = sock.makefile("rw")
            stream.readline()
            for command in ({"execute": "qmp_capabilities"},
                            {"execute": "screendump", "arguments": {"filename": str(path)}}):
                stream.write(json.dumps(command) + "\n")
                stream.flush()
                while not {"return", "error"} & json.loads(stream.readline()).keys():
                    pass
        return Image.open(path).convert("RGB")

    def stop(self) -> None:
        self.process.kill()
        self.process.wait()


def pattern_mismatches(image: Image.Image) -> int:
    width, height = image.size
    bad = 0
    for x in COLUMNS:
        for y in range(16, height):
            expected = TOP if y < height // 2 else BOTTOM
            bad += image.getpixel((min(x, width - 1), y)) != expected
    return bad


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qemu", required=True)
    parser.add_argument("--iso", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path)
    parser.add_argument("--window", type=float, default=120)
    args = parser.parse_args()

    vm = Vm(args.qemu, args.iso, args.work)
    checks = []
    try:
        ready = vm.wait_for(lambda t: "desktop] Ready" in t and "Cellos > " in t, args.window)
        checks.append(("desktop-ready", ready))
        if ready:
            time.sleep(2)
            before = pattern_mismatches(vm.screendump("before.ppm"))
            checks.append(("control-no-pattern-before-hv", before > 0))
            vm.type_serial("hv\r")
            done = vm.wait_for(lambda t: "X86_DISPLAY_IRQ:" in t or "X86_GPU_FAIL" in t
                               or "[hv-x86] guest exited" in t, args.window)
            checks.append(("guest-fixture-finished", done))
            time.sleep(3)
            after = pattern_mismatches(vm.screendump("after.ppm"))
            checks.append(("screen-shows-guest-pattern-exact", after == 0))
    finally:
        vm.stop()

    log = vm.log()
    for marker in MARKERS:
        checks.append(("marker:" + marker.rstrip(":"), marker in log))
    irq = next((line.split("X86_DISPLAY_IRQ:")[1].strip() for line in log.splitlines()
                if "X86_DISPLAY_IRQ:" in line), "")
    checks.append(("display-irq-delivered", irq.isdigit() and int(irq) > 0))
    checks.append(("no-fixture-failure", "X86_GPU_FAIL" not in log))
    checks.append(("vmx-root", "root operation active (VMX)" in log))
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
