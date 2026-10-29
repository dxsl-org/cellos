#!/usr/bin/env bash
# S10-3a investigation (read-only on the repository): build the fleet-profile kernel
# (default features + policy-required) around the S10-3 candidate images in /tmp, boot
# under TCG, run the CI boot check, and probe worker/app commands under policy-required.
set -euo pipefail
I=/tmp/cellbox-s10-3a-inv
C=/tmp/cellbox-s10-3-7yWa/cand
cd /home/dmin/cellos
unset RUSTFLAGS CELLOS_PHASE02_CELL_PUBLIC_KEY
for p in headless desktop; do
    EMBEDDED_OVERRIDE="$C/$p" CARGO_TARGET_DIR="$I/ktarget-$p" \
        cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc \
        --features policy-required > "$I/kernel-$p.log" 2>&1
    cp "$I/ktarget-$p/x86_64-unknown-none/release/cellos-kernel" "$I/fleet-$p.elf"
    rm -rf "$I/iso-root-$p"
    X86_KERNEL="$I/fleet-$p.elf" X86_ISO_ROOT="$I/iso-root-$p" bash scripts/x86/make-iso-ci.sh "$I/fleet-$p.iso" > "$I/iso-$p.log" 2>&1
done
sha256sum "$I"/fleet-*.elf "$I"/fleet-*.iso
mkdir -p "$I/boot-check"
(cd "$I/boot-check" && BOOT_WINDOW=90 bash /home/dmin/cellos/scripts/qemu-x86_64-test.sh "$I/fleet-headless.iso" > run.log 2>&1; echo "qemu-x86_64-test.sh on fleet headless: exit $?") || true
grep -a "Tier 2 admission\|\[policy\] loaded" "$I/boot-check/qemu-x86_64.log" | head -3
python3 - "$I" <<'PY'
import os, select, socket, subprocess, sys, time
I = sys.argv[1]
cmds = ["echo builtin-ok", "ls /", "/bin/ls /", "echo st=$?", "/bin/echo worker-echo", "echo st=$?",
        "/bin/cat /etc/cellos/system.toml", "echo st=$?", "/bin/ps", "echo st=$?", "ps", "echo st=$?",
        "export K=V", "echo st=$?", "cellbox ls /", "echo st=$?"]
for profile in ("headless", "desktop"):
    sock = f"{I}/serial-{profile}.sock"
    if os.path.exists(sock): os.unlink(sock)
    argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-cpu", "qemu64,+pdpe1gb", "-m", "256M",
            "-display", "none", "-chardev", f"socket,id=s,path={sock},server=on,wait=off", "-serial", "chardev:s",
            "-monitor", "none", "-cdrom", f"{I}/fleet-{profile}.iso", "-boot", "d", "-no-reboot", "-nic", "none"]
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    conn = None
    for _ in range(100):
        try: conn = socket.socket(socket.AF_UNIX); conn.connect(sock); break
        except OSError: conn = None; time.sleep(0.1)
    log = bytearray()
    def pump(until):
        while time.time() < until:
            r, _, _ = select.select([conn], [], [], 0.3)
            if r:
                d = conn.recv(65536)
                if d: log.extend(d)
    end = time.time() + 120
    while time.time() < end and b"Cellos >" not in log: pump(time.time() + 0.5)
    pump(time.time() + (8 if profile == "desktop" else 2))
    for c in cmds:
        conn.sendall(c.encode() + b"\r"); pump(time.time() + 3)
    proc.terminate(); proc.wait()
    text = log.replace(b"\x00", b"").decode("utf-8", "replace")
    open(f"{I}/serial-{profile}.log", "w").write(text)
    keep = [l for l in text.splitlines() if any(k in l for k in ("Tier 2", "[policy]", "DENY", "denied", "st=",
            "worker-echo", "builtin-ok", "Cellos >", "[desktop] Ready", "[compositor] Compositor", "PANIC",
            "spawn failed", "[fb-console]"))]
    print(f"===== {profile}"); print("\n".join(keep[-60:]))
PY
