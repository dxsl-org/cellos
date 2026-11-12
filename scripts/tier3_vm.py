"""A KVM QEMU instance driven over its serial socket and QMP, for Tier 3 lanes."""
import json
import socket
import subprocess
import time
from pathlib import Path

from PIL import Image

SCREEN_W, SCREEN_H = 1280, 800


class Vm:
    def __init__(self, qemu: str, iso: Path, work: Path, name: str, memory: str = "1G",
                 extra: tuple = ()):
        self.work = work
        self.name = name
        self.serial_log = work / f"{name}-serial.log"
        for path in (self._sock("serial"), self._sock("qmp"), self.serial_log):
            path.unlink(missing_ok=True)
        self.process = subprocess.Popen([
            qemu, "-machine", "q35", "-accel", "kvm", "-cpu", "host", "-m", memory,
            "-display", "none", "-chardev",
            f"socket,id=s0,path={self._sock('serial')},server=on,wait=off,"
            f"logfile={self.serial_log}",
            "-serial", "chardev:s0", "-qmp", f"unix:{self._sock('qmp')},server=on,wait=off",
            "-monitor", "none", "-cdrom", str(iso), "-boot", "d", "-no-reboot", "-nic", "none",
            "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci", *extra],
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

    def click(self, x: float, y: float) -> None:
        """Left click at screen pixel (x, y) through the virtio tablet."""
        def event(kind, data):
            return {"execute": "input-send-event", "arguments": {"events": [
                {"type": kind, "data": data}]}}
        self.qmp(event("abs", {"axis": "x", "value": int(x * 32767 / (SCREEN_W - 1))}),
                 event("abs", {"axis": "y", "value": int(y * 32767 / (SCREEN_H - 1))}))
        time.sleep(0.1)
        for down in (True, False):
            self.qmp(event("btn", {"down": down, "button": "left"}))
            time.sleep(0.1)

    def stop(self) -> None:
        self.process.kill()
        self.process.wait()
