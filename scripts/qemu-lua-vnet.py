#!/usr/bin/env python3
"""Exercise the real Lua TCP/UDP/DNS bindings in a private RV64 QEMU disk.

Requires an already-built RV64 Lua ELF, kernel, and disk. No tracked image or
release binary is changed; signing and FAT cell-store assembly run on copies.
"""

import argparse
import os
from pathlib import Path
import selectors
import shutil
import socket
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent


def serve_tcp(listener, observations):
    try:
        listener.settimeout(45)
        conn, _ = listener.accept()
        with conn:
            conn.settimeout(10)
            observations["tcp"] = conn.recv(64)
            conn.sendall(b"R\0Z")
    except OSError as exc:
        observations["tcp_error"] = str(exc)
    finally:
        listener.close()


def serve_udp(sock, observations):
    try:
        sock.settimeout(45)
        packet, peer = sock.recvfrom(64)
        observations["udp"] = packet
        sock.sendto(b"S\0T", peer)
    except OSError as exc:
        observations["udp_error"] = str(exc)
    finally:
        sock.close()


def boot(kernel, disk, observations, timeout):
    cmd = [
        "qemu-system-riscv64", "-machine", "virt", "-m", "256M", "-nographic",
        "-bios", "default", "-smp", "1", "-kernel", str(kernel),
        "-drive", f"file={disk},format=raw,id=hd0,if=none",
        "-device", "virtio-blk-device,drive=hd0", "-device", "virtio-keyboard-device",
        "-netdev", "user,id=net0", "-device", "virtio-net-device,netdev=net0",
        "-monitor", "none",
    ]
    proc = subprocess.Popen(
        cmd, cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, bufsize=0,
    )
    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ)
    raw = bytearray()
    sent = False
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            for key, _ in selector.select(0.25):
                chunk = os.read(key.fileobj.fileno(), 16384)
                if chunk:
                    raw.extend(chunk)
            output = raw.decode("utf-8", "replace")
            if not sent and "Cellos >" in output:
                # The shell can reach its prompt while boot-test cells still log.
                # Delaying the command avoids interleaving test output with it.
                time.sleep(1)
                proc.stdin.write(b"lua /bin/lua-net-smoke.lua\n")
                proc.stdin.flush()
                sent = True
            if sent and "/bin/lua-net-smoke.lua:" in output and "LUA_EXPECTED_RUNTIME_ERROR" in output:
                break
            if sent and ("[lua] out of memory" in output or "DENY launch edge" in output):
                break
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        selector.close()
    return raw.decode("utf-8", "replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lua", type=Path, required=True, help="built RV64 Lua cell ELF")
    parser.add_argument("--kernel", type=Path, default=ROOT / "target/riscv64gc-unknown-none-elf/release/cellos-kernel")
    parser.add_argument("--disk", type=Path, default=ROOT / "disk_v3.img")
    parser.add_argument("--timeout", type=int, default=90)
    args = parser.parse_args()
    lua = args.lua.resolve()
    kernel = args.kernel.resolve()
    disk = args.disk.resolve()
    for path in (lua, kernel, disk):
        if not path.is_file():
            parser.error(f"missing input: {path}")

    tcp = socket.socket()
    tcp.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    tcp.bind(("0.0.0.0", 0))
    tcp.listen(1)
    udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    udp.bind(("0.0.0.0", 0))
    observations = {}
    workers = [
        threading.Thread(target=serve_tcp, args=(tcp, observations), daemon=True),
        threading.Thread(target=serve_udp, args=(udp, observations), daemon=True),
    ]
    with tempfile.TemporaryDirectory(prefix="cellos-lua-vnet-") as tmp:
        work = Path(tmp)
        cell = work / "lua"
        private_disk = work / "disk.img"
        script = work / "lua-net-smoke.lua"
        shutil.copyfile(lua, cell)
        # Reflink when available; never mutate the source image.
        subprocess.run(["cp", "--reflink=auto", str(disk), str(private_disk)], check=True)
        script.write_text(
            f"local ip = assert(vnet.resolve('gateway'))\n"
            f"print('LUA_DNS_PASS', ip)\n"
            f"local c = assert(vnet.connect(ip, {tcp.getsockname()[1]}))\n"
            f"local n = vnet.send(c, string.char(65, 0, 66))\n"
            f"local r = vnet.recv(c, 3)\n"
            f"print('LUA_TCP_PASS', n, r and #r, r and string.byte(r, 2))\n"
            f"vnet.close(c)\n"
            f"local u = assert(vnet.udp_socket())\n"
            f"assert(vnet.udp_bind(u, 18474))\n"
            f"local m = vnet.udp_send(u, ip, {udp.getsockname()[1]}, string.char(67, 0, 68))\n"
            f"local from, port, reply = vnet.udp_recv(u, 8)\n"
            f"print('LUA_UDP_PASS', m, from, port, reply and #reply, reply and string.byte(reply, 2))\n"
            f"vnet.close(u)\n"
            f"error('LUA_EXPECTED_RUNTIME_ERROR')\n",
            encoding="utf-8",
        )
        subprocess.run(
            ["bash", "-c", 'source scripts/lib-sign-cells.sh; sign_cells "$1"', "bash", str(cell)],
            cwd=ROOT, check=True, env={**os.environ, "PYTHON_BIN": "python3"},
        )
        subprocess.run(
            ["python3", "tools/add-cell-to-disk.py", str(private_disk),
             f"/bin/lua={cell}", f"/bin/lua-net-smoke.lua={script}"],
            cwd=ROOT, check=True,
        )
        for worker in workers:
            worker.start()
        output = boot(kernel, private_disk, observations, args.timeout)
        for worker in workers:
            worker.join(timeout=1)
        (work / "qemu.log").write_text(output, encoding="utf-8")
        checks = {
            "DNS": "LUA_DNS_PASS\t10.0.2.2" in output,
            "TCP": "LUA_TCP_PASS\t3\t3\t0" in output and observations.get("tcp") == b"A\0B",
            "UDP": "LUA_UDP_PASS\t3\t10.0.2.2\t" in output
                   and "\t3\t0" in output.split("LUA_UDP_PASS")[-1].splitlines()[0]
                   and observations.get("udp") == b"C\0D",
            "script errors visible": "/bin/lua-net-smoke.lua:" in output
                                     and "LUA_EXPECTED_RUNTIME_ERROR" in output,
            "no denied syscall": "[kernel] syscall" not in output[output.find("SpawnFromElf: /bin/lua"):],
        }
        for name, ok in checks.items():
            print(f"{'PASS' if ok else 'FAIL'}: Lua {name}")
        if not all(checks.values()):
            print(output[-3000:])
            raise SystemExit(1)


if __name__ == "__main__":
    main()
