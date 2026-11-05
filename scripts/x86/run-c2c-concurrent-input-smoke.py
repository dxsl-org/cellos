import argparse
import re
import socket
import subprocess
import time
import sys
import os
import hashlib

def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()

def check_topology(raw_log, cpus):
    # Plan 261103-2218 phase 07b: the kernel must have brought every CPU online.
    found = re.findall(rb"\[x86-smp\] topology online=(\d+) processor_ids=", raw_log)
    if found != [str(cpus).encode()]:
        raise RuntimeError(f"kernel topology {found!r} is not exactly online={cpus}")

def main():
    parser = argparse.ArgumentParser(description="C2C concurrent serial input smoke (x86_64)")
    parser.add_argument("--cpus", type=int, choices=range(1, 65), default=4,
                        help="QEMU CPUs (-smp); the kernel must report online=N")
    parser.add_argument("--accel", choices=("tcg", "kvm"), default="tcg",
                        help="QEMU accelerator; KVM uses the host CPU model")
    args = parser.parse_args()
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), "../.."))
    iso_path = os.path.join(repo_root, "build/vicell-x86-c2c-lifecycle.iso")
    kernel_path = os.path.join(repo_root, "build/x86-c2c-lifecycle-iso-root/boot/kernel.elf")
    log_path = os.path.join(repo_root, "docs/evidence/c2c-concurrent-input-x86.log")

    if not os.path.exists(iso_path):
        print(f"Error: {iso_path} does not exist", file=sys.stderr)
        sys.exit(1)

    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.bind(("127.0.0.1", 0))
    srv.listen(1)
    port = srv.getsockname()[1]

    cmd = [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-accel", args.accel,
        "-cpu", "host" if args.accel == "kvm" else "qemu64,+pdpe1gb",
        "-smp", str(args.cpus),
        "-m", "256M",
        "-nographic",
        "-cdrom", iso_path,
        "-boot", "d",
        "-no-reboot",
        "-monitor", "none",
        "-serial", f"tcp:127.0.0.1:{port}",
    ]

    print(f"Starting QEMU: {' '.join(cmd)}")
    start_time = time.time()
    proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    srv.settimeout(30.0)
    try:
        conn, _ = srv.accept()
    except Exception as e:
        proc.kill()
        print(f"Failed to accept QEMU serial connection: {e}", file=sys.stderr)
        sys.exit(1)

    conn.settimeout(1.0)
    raw_log = bytearray()

    def read_until(pattern, timeout_sec):
        deadline = time.time() + timeout_sec
        target = pattern.encode("latin1")
        while time.time() < deadline:
            try:
                chunk = conn.recv(4096)
                if chunk:
                    raw_log.extend(chunk)
                    if target in raw_log:
                        return True
            except socket.timeout:
                pass
            if proc.poll() is not None:
                break
        return False

    print("Waiting for shell prompt...")
    if not read_until("Cellos >", 45.0):
        proc.kill()
        print("Timeout waiting for Cellos >", file=sys.stderr)
        with open(log_path, "wb") as f:
            f.write(raw_log)
        sys.exit(1)

    try:
        check_topology(raw_log, args.cpus)
    except RuntimeError as error:
        proc.kill()
        print(f"Topology check failed: {error}", file=sys.stderr)
        with open(log_path, "wb") as f:
            f.write(raw_log)
        sys.exit(1)
    time.sleep(0.5)

    def send_cmd(cmd_str):
        conn.sendall(cmd_str.encode("latin1") + b"\n")

    print("Writing verification file...")
    send_cmd("vwrite /tmp/c2c-input.txt CONCURRENT_INPUT_SERIAL_OK")
    read_until("Cellos >", 10.0)
    time.sleep(0.5)

    send_cmd("vcat /tmp/c2c-input.txt")
    read_until("Cellos >", 10.0)
    time.sleep(0.5)

    print("Running bench local-service-lifecycle...")
    t_local_start = time.time()
    send_cmd("bench local-service-lifecycle")
    if not read_until("[local-lifecycle] PASS", 90.0):
        proc.kill()
        print("Timeout or fail in local-service-lifecycle", file=sys.stderr)
        with open(log_path, "wb") as f:
            f.write(raw_log)
        sys.exit(1)
    t_local_dur = time.time() - t_local_start
    read_until("Cellos >", 10.0)
    time.sleep(0.5)

    print("Running bench async-lifecycle...")
    t_async_start = time.time()
    send_cmd("bench async-lifecycle")
    if not read_until("[async-lifecycle] PASS", 30.0):
        proc.kill()
        print("Timeout or fail in async-lifecycle", file=sys.stderr)
        with open(log_path, "wb") as f:
            f.write(raw_log)
        sys.exit(1)
    t_async_dur = time.time() - t_async_start
    read_until("Cellos >", 10.0)

    total_dur = time.time() - start_time
    proc.terminate()
    try:
        proc.wait(timeout=5.0)
    except subprocess.TimeoutExpired:
        proc.kill()

    with open(log_path, "wb") as f:
        f.write(raw_log)

    iso_hash = sha256_file(iso_path)
    kernel_hash = sha256_file(kernel_path) if os.path.exists(kernel_path) else "unknown"

    print("\nSUCCESS!")
    print(f"Total session: {total_dur:.2f}s")
    print(f"local lifecycle: {t_local_dur:.2f}s")
    print(f"async lifecycle: {t_async_dur:.2f}s")
    print(f"Log size: {len(raw_log)} bytes")
    print(f"ISO sha256: {iso_hash}")
    print(f"Kernel sha256: {kernel_hash}")

if __name__ == "__main__":
    main()
