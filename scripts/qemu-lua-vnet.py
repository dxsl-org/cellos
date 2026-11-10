#!/usr/bin/env python3
"""Exercise the real Lua TCP/UDP/DNS bindings in a private RV64 QEMU disk.

Two boots run the same script, one per install path of the same Lua binary
(the Lua cell parks after a script, so each run needs its own boot):

1. `lua` (`/bin/lua`, no network right, annex S2-N D-N3) must be refused: DNS
   and TCP return nil, the script raises `LUA_NET_REFUSED`, and the host
   listeners see no traffic;
2. `lua-net` (`/bin/lua-net`, reviewed NET_CLIENT edge, user decision
   2026-11-10) must resolve, exchange TCP with the host, be refused an explicit
   `udp_bind` (no Listen right), exchange UDP through the ephemeral port its
   first send auto-binds, then raise `LUA_EXPECTED_RUNTIME_ERROR` so script
   errors stay visible.

Requires an already-built RV64 Lua ELF, kernel, and disk. `/bin/lua-net` is
served from the kernel's VIFS1 (`gen_disk.ps1` packs it), because its network
edge is admitted only on the kernel path route; `--lua` replaces `/bin/lua` on a
private disk copy. No tracked image or release binary is changed; signing and
cell-table assembly run on copies.
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
PROMPT = "Cellos >"
SCRIPT = "/bin/lua-net-smoke.lua"


def serve_tcp(listener, observations, lua_net_started, timeout):
    try:
        listener.settimeout(timeout)
        conn, _ = listener.accept()
        observations["tcp_before_lua_net"] = not lua_net_started.is_set()
        with conn:
            conn.settimeout(10)
            observations["tcp"] = conn.recv(64)
            conn.sendall(b"R\0Z")
    except OSError as exc:
        observations["tcp_error"] = str(exc)
    finally:
        listener.close()


def serve_udp(sock, observations, lua_net_started, timeout):
    try:
        sock.settimeout(timeout)
        packet, peer = sock.recvfrom(64)
        observations["udp_before_lua_net"] = not lua_net_started.is_set()
        observations["udp"] = packet
        sock.sendto(b"S\0T", peer)
    except OSError as exc:
        observations["udp_error"] = str(exc)
    finally:
        sock.close()


def boot(kernel, disk, command, timeout):
    """Boot, run `command` at the first shell prompt, and return the serial log
    once the script reports its error line (every run of the script ends in one)
    or the launch fails."""
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
    sent_at = -1
    deadline = time.monotonic() + timeout
    try:
        while time.monotonic() < deadline:
            for key, _ in selector.select(0.25):
                chunk = os.read(key.fileobj.fileno(), 16384)
                if chunk:
                    raw.extend(chunk)
            output = raw.decode("utf-8", "replace")
            if sent_at < 0:
                if PROMPT in output:
                    # The shell can reach its prompt while boot-test cells still
                    # log. Delaying the command avoids interleaving test output.
                    time.sleep(1)
                    sent_at = len(raw)
                    proc.stdin.write(f"{command} {SCRIPT}\n".encode())
                    proc.stdin.flush()
                continue
            tail = raw[sent_at:].decode("utf-8", "replace")
            ran = tail.split(f"{command} {SCRIPT}", 1)[-1]
            marker = ran.find(f"{SCRIPT}:")
            if marker >= 0 and "\n" in ran[marker:]:
                break  # the script's error line is complete
            if any(marker in tail for marker in (
                    "[lua] out of memory", "DENY launch edge", "command not found")):
                break
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        selector.close()
    if sent_at < 0:
        return ""
    return raw[sent_at:].decode("utf-8", "replace")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lua", type=Path, required=True, help="built RV64 Lua cell ELF")
    parser.add_argument("--kernel", type=Path, default=ROOT / "target/riscv64gc-unknown-none-elf/release/cellos-kernel")
    parser.add_argument("--disk", type=Path, default=ROOT / "disk_v3.img")
    parser.add_argument("--timeout", type=int, default=90, help="per-boot timeout in seconds")
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
    tcp_port, udp_port = tcp.getsockname()[1], udp.getsockname()[1]
    observations = {}
    lua_net_started = threading.Event()
    # The listeners outlive both boots: traffic during the `lua` boot is recorded
    # as such, and the `lua-net` boot is served by the same sockets.
    serve_for = 2 * args.timeout + 30
    workers = [
        threading.Thread(target=serve_tcp, args=(tcp, observations, lua_net_started, serve_for), daemon=True),
        threading.Thread(target=serve_udp, args=(udp, observations, lua_net_started, serve_for), daemon=True),
    ]
    with tempfile.TemporaryDirectory(prefix="cellos-lua-vnet-") as tmp:
        work = Path(tmp)
        cell = work / "lua"
        private_disk = work / "disk.img"
        script = work / "lua-net-smoke.lua"
        shutil.copyfile(lua, cell)
        # Reflink when available; never mutate the source image.
        subprocess.run(["cp", "--reflink=auto", str(disk), str(private_disk)], check=True)
        # Without the right, DNS and the literal-address connect both return nil;
        # the script then raises before any UDP traffic.
        script.write_text(
            f"local ip = vnet.resolve('gateway')\n"
            f"if not ip then\n"
            f"  local c, why = vnet.connect('10.0.2.2', {tcp_port})\n"
            f"  error('LUA_NET_REFUSED resolve=nil connect=' .. tostring(c) .. ' ' .. tostring(why))\n"
            f"end\n"
            f"print('LUA_DNS_PASS', ip)\n"
            f"local c = assert(vnet.connect(ip, {tcp_port}))\n"
            f"local n = vnet.send(c, string.char(65, 0, 66))\n"
            f"local r = vnet.recv(c, 3)\n"
            f"print('LUA_TCP_PASS', n, r and #r, r and string.byte(r, 2))\n"
            f"vnet.close(c)\n"
            f"local u = assert(vnet.udp_socket())\n"
            # Choosing a port is Listen-gated, which lua-net does not hold; the
            # first send binds an ephemeral port that receives the reply.
            f"print('LUA_UDP_BIND_REFUSED', not vnet.udp_bind(u, 18474))\n"
            f"local m = vnet.udp_send(u, ip, {udp_port}, string.char(67, 0, 68))\n"
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
             f"/bin/lua={cell}", f"{SCRIPT}={script}"],
            cwd=ROOT, check=True,
        )
        for worker in workers:
            worker.start()
        plain = boot(kernel, private_disk, "lua", args.timeout)
        lua_net_started.set()
        granted = boot(kernel, private_disk, "lua-net", args.timeout)
        for worker in workers:
            worker.join(timeout=1)
        checks = {
            "plain lua refused network": f"{SCRIPT}:" in plain
                                         and "LUA_NET_REFUSED resolve=nil connect=nil" in plain
                                         and "LUA_DNS_PASS" not in plain,
            "plain lua sent no host traffic": not observations.get("tcp_before_lua_net", False)
                                              and not observations.get("udp_before_lua_net", False),
            "lua-net on the kernel path route": "SpawnFromPath refused" not in granted
                                                and "SpawnFromElf: /bin/lua-net" not in granted,
            "lua-net DNS": "LUA_DNS_PASS\t10.0.2.2" in granted,
            "lua-net TCP": "LUA_TCP_PASS\t3\t3\t0" in granted and observations.get("tcp") == b"A\0B",
            "lua-net UDP bind refused (no listen)": "LUA_UDP_BIND_REFUSED\ttrue" in granted,
            "lua-net UDP": "LUA_UDP_PASS\t3\t10.0.2.2\t" in granted
                           and "\t3\t0" in granted.split("LUA_UDP_PASS")[-1].splitlines()[0]
                           and observations.get("udp") == b"C\0D",
            "lua-net script errors visible": f"{SCRIPT}:" in granted
                                             and "LUA_EXPECTED_RUNTIME_ERROR" in granted,
            "no denied syscall": "[kernel] syscall" not in plain + granted,
        }
        for name, ok in checks.items():
            print(f"{'PASS' if ok else 'FAIL'}: {name}")
        for label, log in (("lua", plain), ("lua-net", granted)):
            for line in log.splitlines():
                if "LUA_" in line or "SpawnFrom" in line:
                    print(f"  {label} | {line.strip()}")
        if not all(checks.values()):
            print("--- lua boot tail ---")
            print(plain[-2000:])
            print("--- lua-net boot tail ---")
            print(granted[-2000:])
            raise SystemExit(1)


if __name__ == "__main__":
    main()
