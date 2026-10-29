#!/usr/bin/env bash
# S10-2a boot smoke: dev-key candidate images on the matching kernel (default features:
# dev-signing-key, dev-policy-key, signing-required), built and booted entirely in /tmp.
# TCG; explicit X86_KERNEL (G3 refuses only the default kernel path). Not qualification.
set -euo pipefail
W=/tmp/cellbox-s10-2a-u8em
cd /home/dmin/cellos
EV="$W/evidence/boot-smoke"
mkdir -p "$EV"
: > "$EV/artifacts.sha256"
unset CELLOS_PHASE02_CELL_PUBLIC_KEY CELLBOX_PRIVATE_FIXTURE_SIGNING CELLOS_SIGN_ALLOW_PROD_KEY_OUTSIDE_CI
for profile in headless desktop; do
    echo "==> default-feature kernel for $profile (EMBEDDED_OVERRIDE=$W/$profile)"
    EMBEDDED_OVERRIDE="$W/$profile" \
    CARGO_TARGET_DIR="$W/kernel-target" \
    RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
    cargo build --release --locked --offline -p cellos-kernel \
        --target x86_64-unknown-none -Z build-std=core,alloc > "$EV/kernel-build-$profile.log" 2>&1
    cp "$W/kernel-target/x86_64-unknown-none/release/cellos-kernel" "$W/kernel-$profile.elf"
    rm -rf "$W/iso-root-$profile"
    X86_KERNEL="$W/kernel-$profile.elf" X86_ISO_ROOT="$W/iso-root-$profile" \
        bash scripts/x86/make-iso-ci.sh "$W/smoke-$profile.iso" > "$EV/iso-$profile.log" 2>&1
    sha256sum "$W/$profile/kernel_fs.img" "$W/kernel-$profile.elf" "$W/smoke-$profile.iso" | sed "s#$W#\$W#g" >> "$EV/artifacts.sha256"
    stat -c '%n %s' "$W/kernel-$profile.elf" "$W/smoke-$profile.iso" | sed "s#$W#\$W#g" >> "$EV/artifacts.sha256"
done
python3 - "$W" "$EV" <<'PY'
import json, os, re, select, socket, subprocess, sys, time
w, ev = sys.argv[1], sys.argv[2]
needles = {
    "headless": ["[policy] loaded + verified", "Cellos >"],
    "desktop": ["[policy] loaded + verified", "[compositor] Compositor v0.2",
                "[compositor] first scanout flush submitted",
                "[desktop] Ready. Entering event loop.", "Cellos >"],
}
bad = re.compile(r"PANIC|panicked|signature (?:invalid|verify|fail)|bad signature|unsigned|"
                 r"NotSupported|spawn failed|bootstrap not in VIFS1", re.I)
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
    while time.time() < start + 600 and proc.poll() is None and conn is not None:
        r, _, _ = select.select([conn], [], [], 0.5)
        if r:
            data = conn.recv(65536)
            if not data: break
            log.extend(data)
        text = log.replace(b"\x00", b"").decode("utf-8", "replace")
        for n in wanted:
            if n not in seen and n in text: seen[n] = round(time.time() - start, 1)
        if len(seen) == len(wanted):
            break
    if conn is not None and len(seen) == len(wanted):
        # One command through the dev-key-signed shell, then let the log settle.
        time.sleep(1.0)
        conn.sendall(b"echo s10-2a-shell-alive\r")
        end = time.time() + 20
        while time.time() < end:
            r, _, _ = select.select([conn], [], [], 0.5)
            if r:
                data = conn.recv(65536)
                if data: log.extend(data)
            if log.count(b"s10-2a-shell-alive") >= 2: break
        time.sleep(3)
        try:
            conn.setblocking(False); log.extend(conn.recv(1 << 20))
        except OSError: pass
    proc.terminate()
    try: proc.wait(timeout=10)
    except subprocess.TimeoutExpired: proc.kill(); proc.wait()
    text = log.replace(b"\x00", b"").decode("utf-8", "replace")
    open(f"{ev}/serial-{profile}.log", "w").write(text)
    open(f"{ev}/qemu-argv-{profile}.txt", "w").write(" ".join(argv).replace(w, "$W") + "\n")
    lines = [l for l in text.splitlines() if bad.search(l)]
    echoed = text.count("s10-2a-shell-alive") >= 2
    results[profile] = {
        "markers_seconds": seen, "missing": [n for n in wanted if n not in seen],
        "shell_echo_round_trip": echoed, "suspicious_lines": lines[:20],
        "verdict": "PASS" if len(seen) == len(wanted) and echoed and not lines else "FAIL",
    }
json.dump(results, open(f"{ev}/verdict.json", "w"), indent=2)
print(json.dumps(results, indent=2))
PY
