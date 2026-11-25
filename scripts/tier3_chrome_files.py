"""Chrome lane, phase 10: Chromium's Downloads folder is the data disk's Chrome folder.

A file served by the build host is downloaded; once the machine has stopped, its
bytes are read back from the data disk image (mtools) and must equal what was
served. A page the lane put on that disk before boot opens in Chromium from
file:///home/chrome/Downloads/ and must show its colour."""
import hashlib
import http.server
import subprocess
import threading
import time
from pathlib import Path

from tier3_display_check import CHROME_X, CHROME_Y

OMNIBOX = (CHROME_X + 400, CHROME_Y + 62)
# Chromium's toolbar download button, and Keep on the bubble's first row (Chromium 136,
# 1024 px window).
DOWNLOADS_BUTTON = (CHROME_X + 930, CHROME_Y + 62)
KEEP = (CHROME_X + 860, CHROME_Y + 148)
# The QEMU user network's address for the build host, as the guest sees it.
HOST = "10.0.2.2"
PAYLOAD = "payload.bin"
PAYLOAD_SIZE = 300 * 1024
# from-host.html's background (qemu-tier3-chrome-x86.sh).
HOST_PAGE_COLOUR = (0x3c, 0x6e, 0x9f)
FAT_OFFSET = 2048 * 512


def payload() -> bytes:
    """Deterministic, incompressible-looking bytes: a download that a short or
    shifted write cannot match by accident."""
    out = bytearray()
    block = b"cellos-phase10"
    while len(out) < PAYLOAD_SIZE:
        block = hashlib.sha256(block).digest()
        out += block
    return bytes(out[:PAYLOAD_SIZE])


class Server:
    """Serve `payload.bin` as an attachment on an ephemeral port of the build host."""

    def __init__(self):
        body = payload()

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path != f"/{PAYLOAD}":
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Disposition", f'attachment; filename="{PAYLOAD}"')
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        self.httpd = http.server.ThreadingHTTPServer(("0.0.0.0", 0), Handler)
        self.port = self.httpd.server_address[1]
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()

    def stop(self):
        self.httpd.shutdown()


def open_url(vm, url: str) -> None:
    vm.click(*OMNIBOX)
    vm.press("ctrl", "a")
    vm.type_text(url)
    vm.press("ret")


def run(vm, colour_share) -> list:
    """Run the step in a session whose Chrome window is focused. `colour_share(name,
    colour)` polls until the page area is mostly `colour`. Returns the checks that
    can be judged while the machine runs."""
    checks = [("guest-mounts-downloads", "CHROME_GUEST_DOWNLOADS mounted" in vm.log())]
    server = Server()
    try:
        open_url(vm, f"{HOST}:{server.port}/{PAYLOAD}")
        # Chromium blocks a download served over plain HTTP until it is kept; the lane
        # keeps it the way a user would, from the toolbar's download bubble.
        time.sleep(10)
        vm.click(*DOWNLOADS_BUTTON)
        time.sleep(2)
        vm.screendump("download-bubble")
        vm.click(*KEEP)
        # No completion signal leaves the guest; the file is judged from the disk later.
        time.sleep(15)
        vm.screendump("download")
        vm.press("esc")
    finally:
        server.stop()
    open_url(vm, "file:///home/chrome/Downloads/from-host.html")
    checks.append(("host-file-opens-in-chromium", colour_share("from-host", HOST_PAGE_COLOUR)))
    return checks


def downloaded(files_disk: Path, work: Path) -> tuple:
    """After the machine stopped: whether the data disk holds exactly the served bytes."""
    out = work / f"downloaded-{PAYLOAD}"
    out.unlink(missing_ok=True)
    copied = subprocess.run(
        ["mcopy", "-n", "-i", f"{files_disk}@@{FAT_OFFSET}", f"::/Chrome/{PAYLOAD}", str(out)],
        capture_output=True, text=True)
    listing = subprocess.run(["mdir", "-b", "-i", f"{files_disk}@@{FAT_OFFSET}", "::/Chrome"],
                             capture_output=True, text=True).stdout
    print(f"[tier3-chrome] data disk /Chrome: {listing.split()}", flush=True)
    same = copied.returncode == 0 and out.read_bytes() == payload()
    return ("download-lands-on-data-disk", same)
