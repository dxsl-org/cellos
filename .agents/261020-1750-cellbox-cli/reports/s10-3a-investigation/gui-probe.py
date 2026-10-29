#!/usr/bin/env python3
"""S10-3a investigation: launch the GUI Terminal, Files and the 2nd/4th pinned apps by taskbar
click on a desktop ISO (fleet vs default kernel) and record the policy/launch lines and readiness.
usage: gui-probe.py <iso> <out-prefix>"""
import json, os, select, socket, subprocess, sys, time

iso, out = sys.argv[1], sys.argv[2]
ser, qmp = out + ".serial.sock", out + ".qmp.sock"
for p in (ser, qmp):
    if os.path.exists(p): os.unlink(p)
argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-cpu", "qemu64,+pdpe1gb", "-m", "256M",
        "-display", "none", "-chardev", f"socket,id=s,path={ser},server=on,wait=off", "-serial", "chardev:s",
        "-qmp", f"unix:{qmp},server=on,wait=off", "-monitor", "none", "-cdrom", iso, "-boot", "d",
        "-no-reboot", "-nic", "none", "-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"]
proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def connect(path):
    for _ in range(100):
        try:
            s = socket.socket(socket.AF_UNIX); s.connect(path); return s
        except OSError:
            time.sleep(0.1)
    raise SystemExit("socket never appeared: " + path)


conn = connect(ser)
q = connect(qmp); qf = q.makefile("rw")
qf.readline(); qf.write(json.dumps({"execute": "qmp_capabilities"}) + "\n"); qf.flush(); qf.readline()
log = bytearray()


def pump(seconds):
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([conn], [], [], 0.3)
        if r:
            d = conn.recv(65536)
            if d: log.extend(d)


def qcmd(name, **args):
    qf.write(json.dumps({"execute": name, "arguments": args}) + "\n"); qf.flush()
    while True:
        reply = json.loads(qf.readline())
        if "return" in reply or "error" in reply:
            return reply


def click(x, y):
    qcmd("input-send-event", events=[{"type": "abs", "data": {"axis": "x", "value": int(x * 32768 / 1280)}},
                                     {"type": "abs", "data": {"axis": "y", "value": int(y * 32768 / 800)}}])
    time.sleep(0.6)
    qcmd("input-send-event", events=[{"type": "btn", "data": {"down": True, "button": "left"}}]); time.sleep(0.15)
    qcmd("input-send-event", events=[{"type": "btn", "data": {"down": False, "button": "left"}}])


start = time.time()
while time.time() < start + 180 and b"[desktop] Ready" not in log:
    pump(0.5)
pump(4)
marks = {}
for name, x in (("pin1-terminal", 140 + 74), ("pin2", 294 + 74), ("pin3-files", 448 + 74), ("pin4", 602 + 74)):
    before = len(log)
    click(x, 800 - 36 + 4 + 14)
    pump(25)
    seg = log[before:].replace(b"\x00", b"").decode("utf-8", "replace")
    marks[name] = [l for l in seg.splitlines() if any(k in l for k in (
        "[policy]", "DENY", "denied", "ready", "Ready", "initialized", "SpawnFromElf", "Spawned", "PANIC",
        "fault", "failed", "error", "Error"))][:25]
proc.terminate(); proc.wait()
open(out + ".serial.log", "w").write(log.replace(b"\x00", b"").decode("utf-8", "replace"))
print(json.dumps(marks, indent=1))
