#!/usr/bin/env python3
"""Scratch RV64 QEMU TCG run: same QEMU line as QemuRunner::boot_with_pointer,
but launches `ocel` in the foreground (the lane's `&` form is refused by the
shell at HEAD before ocel starts)."""
import socket, subprocess, sys, time

root, log_path = sys.argv[1], sys.argv[2]
kernel = f"{root}/target/riscv64gc-unknown-none-elf/release/cellos-kernel"
disk = f"{root}/disk_v3.img"
lst = socket.socket(); lst.bind(("127.0.0.1", 0)); lst.listen(1)
port = lst.getsockname()[1]
qmp = socket.socket(); qmp.bind(("127.0.0.1", 0)); qport = qmp.getsockname()[1]; qmp.close()
q = subprocess.Popen(["qemu-system-riscv64", "-machine", "virt", "-m", "256M", "-display", "none",
    "-bios", "default", "-kernel", kernel, "-drive", f"file={disk},format=raw,id=hd0,if=none",
    "-device", "virtio-blk-device,drive=hd0", "-netdev", "user,id=net0",
    "-device", "virtio-net-device,netdev=net0", "-device", "virtio-keyboard-device",
    "-device", "virtio-gpu-device", "-device", "virtio-tablet-device",
    "-qmp", f"tcp:127.0.0.1:{qport},server,nowait", "-serial", f"tcp:127.0.0.1:{port}"],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
lst.settimeout(30); s, _ = lst.accept(); s.settimeout(0.5)
buf = b""
def wait_for(pat, t):
    global buf
    end = time.time() + t
    while time.time() < end:
        if pat.encode() in buf:
            return True
        try:
            d = s.recv(65536)
            if not d:
                break
            buf += d
        except socket.timeout:
            pass
    return pat.encode() in buf
try:
    print("shell:", wait_for("Cellos >", 90))
    print("vfs-test:", wait_for("[vfs-test] ALL TESTS PASSED", 40))
    time.sleep(0.5)
    s.sendall(b"ocel file:///data/ocel-js-demo.html\n")
    for m in ["[domain] admitted cell 'ocel-quickjs' to Tier 2 Paged Domain (SATP isolation)",
              "[ocel-quickjs] quickjs ",
              "[ocel-quickjs] self-check 55:2,4,6:accent:0.30",
              "[ocel] dom title: sum=10 doubled=2, 4, 6"]:
        print(f"{m!r}:", wait_for(m, 60))
finally:
    q.kill(); q.wait()
    open(log_path, "wb").write(buf)
