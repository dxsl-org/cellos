#!/usr/bin/env python3
"""S10-3 L6 diagnostic (read-only on the repository): does `vwrite /mnt/sd/...` on a FAT32 SATA disk
return to the prompt on an ISO, and how long does it take? Same disk layout and QEMU shape as
tests/integration/tests/ahci-x86.rs (FAT32 at 1 MiB, 524288 sectors, ide-hd on q35). Usage:
diag_vwrite.py <iso> <out-dir>"""
import os, select, socket, subprocess, sys, time
from pathlib import Path

iso, out = sys.argv[1], Path(sys.argv[2])
eol = {"cr": b"\r", "lf": b"\n"}[sys.argv[3] if len(sys.argv) > 3 else "cr"]
settle = float(sys.argv[4]) if len(sys.argv) > 4 else 0.0
out.mkdir(parents=True, exist_ok=True)
vol, disk = out / "fat.vol", out / "sata.img"
with open(vol, "wb") as f:
    f.truncate(524288 * 512)
subprocess.run([sys.executable, "/home/dmin/cellos/tools/mkfat32_inplace.py", str(vol), "524288"], check=True,
               capture_output=True)
with open(disk, "wb") as f:
    f.truncate(2048 * 512 + 524288 * 512 + 1024 * 1024)
    f.seek(2048 * 512)
    f.write(vol.read_bytes())
vol.unlink()
srv = socket.socket(); srv.bind(("127.0.0.1", 0)); srv.listen(1)
port = srv.getsockname()[1]
argv = ["qemu-system-x86_64", "-machine", "q35", "-cpu", "qemu64", "-m", "256M", "-display", "none",
        "-cdrom", iso, "-boot", "d", "-no-reboot", "-monitor", "none",
        "-drive", f"file={disk},format=raw,if=none,id=sata0", "-device", "ide-hd,drive=sata0",
        "-serial", f"tcp:127.0.0.1:{port}"]
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
srv.settimeout(30); conn, _ = srv.accept()
log = bytearray()


def pump(t=0.3):
    r, _, _ = select.select([conn], [], [], t)
    if r:
        d = conn.recv(65536)
        if d: log.extend(d)


def wait(needle, start, timeout):
    end = time.time() + timeout
    while time.time() < end:
        pump()
        if needle.encode() in log[start:]:
            return round(timeout - (end - time.time()), 1)
    return None


results = {}
results["boot_prompt_s"] = wait("Cellos >", 0, 120)
time.sleep(settle)
for cmd, timeout in (("vwrite /mnt/sd/probe.txt phase02b-ahci-ok", 180), ("echo status=$?", 30),
                     ("vcat /mnt/sd/probe.txt", 60), ("ls /mnt/sd", 60), ("echo after-ls", 30)):
    mark = len(log)
    conn.sendall(cmd.encode() + eol)
    time.sleep(0.3)
    results[cmd] = wait("Cellos >", mark + len(cmd), timeout)
proc.terminate(); proc.wait()
text = log.replace(b"\x00", b"").decode("utf-8", "replace")
(out / "serial.log").write_text(text)
disk.unlink()
print(results)
tail = text[text.find("vwrite /mnt/sd"):]
print("\n".join(l for l in tail.splitlines() if "ELF LOAD" not in l)[:4000])
