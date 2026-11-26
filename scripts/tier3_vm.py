"""A KVM QEMU instance driven over its serial socket and QMP, for Tier 3 lanes."""
import json
import socket
import subprocess
import time
from pathlib import Path

from PIL import Image

SCREEN_W, SCREEN_H = 1280, 800


class Vm:
    """The CellOS desktop machine. `screen` is what the tablet's axes span."""
    screen = (SCREEN_W, SCREEN_H)

    def __init__(self, qemu: str, iso: Path, work: Path, name: str, memory: str = "1G",
                 extra: tuple = ()):
        self.start(work, name, [
            qemu, "-machine", "q35", "-accel", "kvm", "-cpu", "host", "-m", memory,
            "-cdrom", str(iso), "-boot", "d",
            # User-mode network: the host stack and every guest each get a lease.
            "-nic", "user,model=e1000,mac=52:54:00:12:34:56",
            "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci", *extra])

    def start(self, work: Path, name: str, machine: list) -> None:
        """Run `machine` (emulator and devices) headless, with the serial log and QMP."""
        self.work = work
        self.name = name
        self.serial_log = work / f"{name}-serial.log"
        for path in (self._sock("serial"), self._sock("qmp"), self.serial_log):
            path.unlink(missing_ok=True)
        self.process = subprocess.Popen([
            *machine, "-display", "none", "-chardev",
            f"socket,id=s0,path={self._sock('serial')},server=on,wait=off,"
            f"logfile={self.serial_log}",
            "-serial", "chardev:s0", "-qmp", f"unix:{self._sock('qmp')},server=on,wait=off",
            "-monitor", "none", "-no-reboot"],
            stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT)

    def _sock(self, kind: str) -> Path:
        return self.work / f"{self.name}-{kind}.sock"

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
            sock.connect(str(self._sock("serial")))
            for byte in text.encode():
                sock.send(bytes([byte]))
                time.sleep(0.05)

    def qmp(self, *commands: dict) -> None:
        with socket.socket(socket.AF_UNIX) as sock:
            sock.connect(str(self._sock("qmp")))
            stream = sock.makefile("rw")
            stream.readline()
            for command in ({"execute": "qmp_capabilities"}, *commands):
                stream.write(json.dumps(command) + "\n")
                stream.flush()
                while not {"return", "error"} & json.loads(stream.readline()).keys():
                    pass

    def screendump(self, name: str) -> Image.Image:
        path = self.work / f"{self.name}-{name}.ppm"
        self.qmp({"execute": "screendump", "arguments": {"filename": str(path)}})
        return Image.open(path).convert("RGB")

    def move(self, x: float, y: float) -> None:
        """Move the pointer to screen pixel (x, y) through the virtio tablet."""
        def axis(name, value, span):
            return {"execute": "input-send-event", "arguments": {"events": [
                {"type": "abs", "data": {"axis": name, "value": int(value * 32767 / (span - 1))}}]}}
        self.qmp(axis("x", x, self.screen[0]), axis("y", y, self.screen[1]))

    def click(self, x: float, y: float) -> None:
        """Left click at screen pixel (x, y) through the virtio tablet."""
        self.move(x, y)
        time.sleep(0.1)
        for down in (True, False):
            self.qmp({"execute": "input-send-event", "arguments": {"events": [
                {"type": "btn", "data": {"down": down, "button": "left"}}]}})
            time.sleep(0.1)

    def drag(self, start: tuple, end: tuple, steps: int = 8) -> None:
        """Press the left button at screen pixel `start`, move to `end` in
        `steps` moves, and release there."""
        def button(down):
            return {"execute": "input-send-event", "arguments": {"events": [
                {"type": "btn", "data": {"down": down, "button": "left"}}]}}
        self.move(*start)
        time.sleep(0.1)
        self.qmp(button(True))
        for step in range(1, steps + 1):
            time.sleep(0.05)
            self.move(start[0] + (end[0] - start[0]) * step / steps,
                      start[1] + (end[1] - start[1]) * step / steps)
        time.sleep(0.1)
        self.qmp(button(False))
        time.sleep(0.1)

    def press(self, *qcodes: str) -> None:
        """Press a key chord (QEMU qcodes, modifiers first) through the virtio keyboard."""
        def key(code, down):
            return {"execute": "input-send-event", "arguments": {"events": [
                {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": code}}}]}}
        self.qmp(*(key(code, True) for code in qcodes))
        time.sleep(0.05)
        self.qmp(*(key(code, False) for code in reversed(qcodes)))
        time.sleep(0.1)

    def type_text(self, text: str) -> None:
        """Type URL-like text (letters, digits, space, `.:/-_?=`) key by key; capitals with Shift."""
        named = {".": ("dot",), "/": ("slash",), ":": ("shift", "semicolon"),
                 "-": ("minus",), "_": ("shift", "minus"), "?": ("shift", "slash"),
                 "=": ("equal",), " ": ("spc",)}
        for char in text:
            if char.isascii() and (char.isdigit() or char.islower()):
                self.press(char)
            elif char.isascii() and char.isupper():
                self.press("shift", char.lower())
            else:
                self.press(*named[char])

    def quit(self, seconds: float = 10) -> None:
        """Ask QEMU to quit and wait for it: a `-audiodev wav` recording is complete
        (data flushed, RIFF sizes written) only after a clean quit. Kills it when
        it does not leave in time."""
        self.qmp({"execute": "quit"})
        try:
            self.process.wait(timeout=seconds)
        except subprocess.TimeoutExpired:
            self.stop()

    def stop(self) -> None:
        self.process.kill()
        self.process.wait()
