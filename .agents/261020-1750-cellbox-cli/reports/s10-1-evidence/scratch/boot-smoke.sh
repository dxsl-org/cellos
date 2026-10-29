#!/usr/bin/env bash
# S10-1 D11-3 early boot smoke: scratch images on a fixture kernel (phase02-terminal-profile,
# private cell key), TCG, /tmp only. Not qualification.
set -euo pipefail
W=/tmp/cellbox-s10-1-3jLV
cd /home/dmin/cellos
EV="$W/evidence/boot-smoke"
mkdir -p "$EV"
export CELLOS_PHASE02_CELL_PUBLIC_KEY="$W/keys/cell.pub"
for profile in headless desktop; do
    echo "==> fixture kernel for $profile (EMBEDDED_OVERRIDE=$W/$profile)"
    EMBEDDED_OVERRIDE="$W/$profile" \
    CARGO_TARGET_DIR="$W/kernel-target" \
    RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
    cargo build --release --locked --offline -p cellos-kernel --features phase02-terminal-profile \
        --target x86_64-unknown-none -Z build-std=core,alloc > "$EV/kernel-build-$profile.log" 2>&1
    cp "$W/kernel-target/x86_64-unknown-none/release/cellos-kernel" "$W/kernel-$profile.elf"
    rm -rf "$W/iso-root-$profile"
    X86_KERNEL="$W/kernel-$profile.elf" X86_ISO_ROOT="$W/iso-root-$profile" \
        bash scripts/x86/make-iso-ci.sh "$W/smoke-$profile.iso" > "$EV/iso-$profile.log" 2>&1
    sha256sum "$W/kernel-$profile.elf" "$W/smoke-$profile.iso" | sed "s#$W#\$W#g" >> "$EV/artifacts.sha256"
    stat -c '%n %s' "$W/kernel-$profile.elf" "$W/smoke-$profile.iso" | sed "s#$W#\$W#g" >> "$EV/artifacts.size"
done
python3 - "$W" "$EV" <<'PY'
import json, os, select, socket, subprocess, sys, time
w, ev = sys.argv[1], sys.argv[2]
seed = open(f"{w}/keys/seed.hex").read().strip()
needles = {
    "headless": ["Cellos >"],
    "desktop": ["[compositor] Compositor v0.2", "[compositor] first scanout flush submitted",
                "[desktop] Ready. Entering event loop.", "Cellos >"],
}
bad = ["PANIC", "panicked", "signature", "bootstrap not in VIFS1", "spawn failed"]
results = {}
for profile, wanted in needles.items():
    sock = f"{w}/serial-{profile}.sock"
    if os.path.exists(sock): os.unlink(sock)
    argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", "tcg", "-cpu", "qemu64,+pdpe1gb",
            "-m", "256M", "-display", "none", "-chardev", f"socket,id=ser0,path={sock},server=on,wait=off",
            "-serial", "chardev:ser0", "-monitor", "none", "-cdrom", f"{w}/smoke-{profile}.iso",
            "-boot", "d", "-no-reboot", "-nic", "none"]
    if profile == "desktop":
        argv += ["-device", "virtio-keyboard-pci", "-device", "virtio-tablet-pci"]
    proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    conn = None
    for _ in range(100):
        try:
            conn = socket.socket(socket.AF_UNIX); conn.connect(sock); break
        except OSError:
            conn = None; time.sleep(0.1)
    log = bytearray(); start = time.time(); seen = {}
    deadline = start + 600
    while time.time() < deadline and proc.poll() is None and conn is not None:
        r, _, _ = select.select([conn], [], [], 0.5)
        if r:
            data = conn.recv(65536)
            if not data: break
            log.extend(data)
        text = log.replace(b"\x00", b"").decode("utf-8", "replace")
        for n in wanted:
            if n not in seen and n in text: seen[n] = round(time.time() - start, 1)
        if len(seen) == len(wanted):
            time.sleep(5)
            try:
                conn.setblocking(False); log.extend(conn.recv(1 << 20))
            except OSError: pass
            break
    proc.terminate()
    try: proc.wait(timeout=10)
    except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    text = log.replace(b"\x00", b"").decode("utf-8", "replace")
    open(f"{ev}/serial-{profile}.log", "w").write(text)
    open(f"{ev}/qemu-argv-{profile}.txt", "w").write(" ".join(argv).replace(w, "$W") + "\n")
    hits = {b: text.count(b) for b in bad if b in text}
    results[profile] = {
        "markers_seconds": seen, "missing": [n for n in wanted if n not in seen],
        "suspicious_strings": hits, "seed_in_log": seed in text,
        "verdict": "PASS" if len(seen) == len(wanted) else "FAIL",
    }
json.dump(results, open(f"{ev}/verdict.json", "w"), indent=2)
print(json.dumps(results, indent=2))
PY
