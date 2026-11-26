"""Chrome lane, phase 11: Chromium's sound reaches the machine's sound device.

The machine has QEMU's virtio-sound recording to a WAV file (`audio-wav`), unless
the lane runs with CHROME_LANE_SOUND=0. A page served by the build host
(tests/guests/chrome-sound/tone.html) plays a 440 Hz tone for 5 s when clicked and
reports how long it played. With a device the recording, read after the machine
quit, must hold that tone; without one the page must still finish on time (the
hypervisor plays silence instead of stalling the guest)."""
import http.server
import json
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path
from urllib.parse import parse_qsl, urlsplit

from tier3_display_check import CHROME_X, CHROME_Y

REPO = Path(__file__).resolve().parent.parent
PAGE = REPO / "tests/guests/chrome-sound/tone.html"
OMNIBOX = (CHROME_X + 400, CHROME_Y + 62)
PAGE_SPOT = (CHROME_X + 500, CHROME_Y + 400)
HOST = "10.0.2.2"
TONE_S, TONE_HZ = 5.0, 440.0


def machine_args(wav: Path) -> tuple:
    """QEMU arguments adding the recording sound device."""
    return ("-audiodev", f"wav,id=snd0,path={wav}",
            "-device", "virtio-sound-pci,audiodev=snd0,streams=1")


class Server:
    """Serve the tone page; its report arrives on `reports`."""

    def __init__(self):
        body = PAGE.read_bytes()
        reports = self.reports = queue.Queue()

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                url = urlsplit(self.path)
                if url.path == "/done":
                    reports.put(dict(parse_qsl(url.query)))
                    self.send_response(204)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        self.httpd = http.server.ThreadingHTTPServer(("0.0.0.0", 0), Handler)
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()


def run(vm, with_device: bool) -> list:
    """Play the tone in a session whose Chrome window is focused."""
    log = vm.log()
    checks = [("guest-has-sound-card", "CHROME_GUEST_SOUND cards=1" in log)]
    server = Server()
    try:
        vm.click(*OMNIBOX)
        vm.press("ctrl", "a")
        vm.type_text(f"{HOST}:{server.httpd.server_address[1]}/")
        vm.press("ret")
        time.sleep(8)
        # Pages may start sound only after a user gesture.
        vm.click(*PAGE_SPOT)
        try:
            report = server.reports.get(timeout=30)
        except queue.Empty:
            report = {}
    finally:
        server.httpd.shutdown()
    played = int(report.get("ms", 0))
    print(f"[tier3-chrome] tone page report: {report}", flush=True)
    name = "tone-plays-on-time" if with_device else "tone-plays-on-time-without-device"
    # Wall-clock time, so it includes opening the sound device in the guest
    # (up to about half a second on a busy host); the recording judges the
    # timing exactly. This check is that the page never stalls.
    checks.append((name, TONE_S * 1000 - 500 <= played <= TONE_S * 1000 + 2000))
    if with_device:
        checks.append(("hypervisor-forwards-sound",
                       "[hv-x86] sound: stream started" in vm.log()))
    return checks


def recorded(wav: Path) -> tuple:
    """After the machine quit: the recording holds the tone, whole and unbroken."""
    result = subprocess.run([sys.executable, str(REPO / "scripts/tier3_wav_tone.py"), str(wav)],
                            capture_output=True, text=True)
    try:
        tone = json.loads(result.stdout)
    except json.JSONDecodeError:
        tone = {}
    print(f"[tier3-chrome] recorded tone: {tone or result.stderr.strip()}", flush=True)
    ok = (bool(tone) and abs(tone["loud_s"] - TONE_S) <= 0.5
          and abs(tone["freq_hz"] - TONE_HZ) <= 5 and tone["longest_gap_ms"] <= 50)
    return ("sound-device-records-tone", ok)
