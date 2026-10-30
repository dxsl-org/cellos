#!/usr/bin/env python3
"""RT1 multi-boot orchestrator (k8-rt1-amendment.md 7.3, 9, 12.5): five QEMU boots of one ISO.

    kssh_boots.py run --iso ISO --disk D0 --container NAME --out EVIDENCE --keys KEYS --work W
                      --set b0|rt1 [--extra-secret-file F ...] [--window 1800] [--accel tcg] [--max-boots 5]

Per boot N (1..5): write the role into the fixture container, record the pre-boot `docker logs` and ledger
line counts, start QEMU (the T0 argv), pump the serial socket until `P04-TERMINAL:` or the window, SIGTERM
QEMU 3 s after the verdict (SIGKILL escalation or a surviving QEMU = unclean: no disk work, class_hint E),
wait for the exit, apply the settle rule to `docker logs`, and only then touch the disk (hash, record oracle,
secret scan with controls, host-ciphertext positive control).  After boot 1 S1 is a sparse copy of D0 (never
booted, never written); boot 5 runs on S1r, a sparse copy of S1.  Gate rules of amendment 12.5 decide whether
the sequence continues.  Nothing written here contains secret bytes; no token is claimed (the runner does).

Exit codes: 0 all five boots completed; 1 stopped early (boots.json "stopped"); 2 usage/host error.
Stop class hints: E host failure (QEMU/docker/harness, unclean exit, role mismatch/missing, control failure,
S1 not pristine, operator limit), C no driver verdict, gate = a gate row failed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import select
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

BOOTS = 5
MARKER = b"P04-TERMINAL:"
SETTLE_QUIET_S = 3.0
SETTLE_CAP_S = 30.0
SETTLE_POLL_S = 1.0
POST_VERDICT_S = 3.0
TERM_WAIT_S = 10.0
SOCK_MAX = 100  # sun_path is 108 bytes on Linux

EXPECT = {1: "r1", 2: "r1", 3: "r1r2", 4: "r1r2", 5: "r1"}
GATES = {
    1: ("kssh-s01", "kssh-s02"),
    2: ("kssh-t1-load", "kssh-s02r"),
    3: ("kssh-t1-del",),
    4: (),
    5: (),
}
ROLE_RE = re.compile(r"P04 INFO kssh-obs role n=(\S+)")
ROW_RE = re.compile(r"P04 (PASS|FAIL) (\S+)")
LEDGER_CMD = "cat /tmp/p09-ledger 2>/dev/null || true"


class HostError(Exception):
    """A QEMU/docker/harness failure (class E); str(e) carries no secret."""


# =========================================================================== adapters (real)

class Clock:
    def now(self) -> float:
        return time.monotonic()

    def sleep(self, s: float) -> None:
        time.sleep(s)


class DockerCli:
    """The only two docker interactions of the lane: `exec CONTAINER sh -c` and `logs CONTAINER 2>&1`."""

    def __init__(self, container: str):
        self.container = container

    def _run(self, args, merge: bool) -> str:
        try:
            r = subprocess.run(["docker", *args], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT if merge else subprocess.PIPE, timeout=60)
        except (OSError, subprocess.TimeoutExpired) as e:
            raise HostError(f"docker {args[0]}: {type(e).__name__}") from None
        if r.returncode != 0:
            raise HostError(f"docker {args[0]} rc={r.returncode}")
        return r.stdout.decode("utf-8", "replace")

    def exec_sh(self, script: str) -> str:
        return self._run(["exec", self.container, "sh", "-c", script], merge=False)

    def logs(self) -> list:
        return _lines(self._run(["logs", self.container], merge=True))


class QemuProc:
    """One QEMU process plus its serial unix socket."""

    def __init__(self, argv, sock_path):
        self.argv, self.sock_path = list(argv), sock_path
        self.conn = None
        self.proc = subprocess.Popen(self.argv, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL)

    def connect(self, tries: int = 100, delay: float = 0.1) -> bool:
        for _ in range(tries):
            try:
                c = socket.socket(socket.AF_UNIX)
                c.connect(self.sock_path)
                self.conn = c
                return True
            except OSError:
                time.sleep(delay)
        return False

    def recv(self, timeout: float) -> bytes:
        if self.conn is None:
            return b""
        try:
            r, _, _ = select.select([self.conn], [], [], timeout)
            data = self.conn.recv(65536) if r else b""
        except OSError:
            return b""
        if r and not data:  # EOF: do not spin
            time.sleep(timeout)
        return data

    def poll(self):
        return self.proc.poll()

    def terminate(self) -> None:
        self.proc.terminate()

    def kill(self) -> None:
        self.proc.kill()

    def wait(self, timeout: float):
        """Exit code, or None when still running after `timeout`."""
        try:
            return self.proc.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            return None

    def close(self) -> None:
        if self.conn is not None:
            try:
                self.conn.close()
            except OSError:
                pass
            self.conn = None


def _lines(text: str) -> list:
    out = text.split("\n")
    if out and out[-1] == "":
        out.pop()
    return out


# =========================================================================== disk operations (real)

def _records():
    import kssh_records  # peer module, imported lazily so adapters/tests stay importable without it
    return kssh_records


def file_contains(path, needle: bytes, chunk: int = 8 << 20) -> bool:
    """Raw byte search over a (sparse) file; all-zero chunks are skipped, chunk overlap kept."""
    keep = max(len(needle) - 1, 0)
    chunk = max(chunk, keep + 1)
    carry = b""
    with open(path, "rb") as f:
        while True:
            buf = f.read(chunk)
            if not buf:
                return False
            if buf.count(0) == len(buf) and not carry.strip(b"\0"):
                carry = b""
                continue
            data = carry + buf
            if needle in data:
                return True
            carry = data[-keep:] if keep else b""


def data_region(path, limit: int = 4 << 20, block: int = 1 << 20) -> bytes:
    """Up to `limit` bytes of non-zero blocks of the image (a data-bearing region, never bytes in an output)."""
    out = bytearray()
    with open(path, "rb") as f:
        while len(out) < limit:
            buf = f.read(block)
            if not buf:
                break
            if buf.count(0) != len(buf):
                out += buf[: limit - len(out)]
    return bytes(out)


def disk_plant_controls(scanner, region: bytes, tmp: Path) -> dict:
    """Plants in copies of a real-image region: (1) every needle from a sector-unaligned offset, (2) every
    needle straddling a distinct chunk boundary of the scan chunk size."""
    expected = scanner.all_forms()
    needles = list(scanner.needles)
    res = {}

    def got_of(p: Path, chunk: int) -> set:
        return {(h["label"], h["form"].replace("+joined", "")) for h in scanner.scan_file(p, joined=False, chunk=chunk)}

    # (1) unaligned: first plant at an offset == 137 (mod 512), the rest follow after one-byte separators
    body = bytearray(region)
    body += b"\0" * ((-len(body)) % 512 + 137)
    offsets = []
    for nd in needles:
        offsets.append(len(body))
        body += nd + b"\n"
    p = tmp / "disk-unaligned.bin"
    p.write_bytes(bytes(body))
    missing = sorted(expected - got_of(p, 8 << 20))
    res["disk-unaligned"] = {"forms": len(expected), "plants": len(offsets),
                             "unaligned_plants": sum(1 for o in offsets if o % 512), "missing": [f"{l}/{f}" for l, f in missing]}
    ok = not missing and res["disk-unaligned"]["unaligned_plants"] > 0
    # (2) chunk-straddling: plant i spans the boundary (i+1)*C of a C-byte scan chunk
    chunk = max(scanner.maxlen + 1, 64)
    base = len(region) + (-len(region)) % chunk
    body = bytearray(region) + bytearray(base - len(region))
    body += bytearray(chunk * (len(needles) + 1))
    for i, nd in enumerate(needles):
        at = base + (i + 1) * chunk - len(nd) // 2
        body[at:at + len(nd)] = nd
    p = tmp / "disk-straddle.bin"
    p.write_bytes(bytes(body))
    missing = sorted(expected - got_of(p, chunk))
    res["disk-straddle"] = {"forms": len(expected), "chunk": chunk, "missing": [f"{l}/{f}" for l, f in missing]}
    ok = ok and not missing
    res["ok"] = ok
    return res


def disk_controls(scanner, region: bytes, tmp: Path) -> dict:
    import kssh_judge
    res = kssh_judge.run_controls(scanner, region, tmp)
    extra = disk_plant_controls(scanner, region, tmp)
    res["disk-unaligned"], res["disk-straddle"] = extra["disk-unaligned"], extra["disk-straddle"]
    res["ok"] = bool(res["ok"] and extra["ok"])
    return res


class DiskOps:
    """Everything that reads or copies a disk file.  Callers guarantee QEMU has exited."""

    def __init__(self, scanner, record_exp: dict | None):
        self.scanner, self.record_exp = scanner, record_exp

    def sha256(self, path) -> str:
        return _records().sha256_file(path)

    def sparse_copy(self, src, dst) -> None:
        _records().sparse_copy(src, dst)

    def analyze(self, image: str, path, expect: str, sha: str):
        """-> (disk-after-boot.json object, controls.json entry); neither contains secret bytes."""
        rec = _records()
        found = rec.find_records(path)
        verdict = rec.evaluate_set(found, expect, self.record_exp)
        region = data_region(path)
        with tempfile.TemporaryDirectory(prefix="kssh-ctl-") as td:
            tmp = Path(td)
            pre = disk_controls(self.scanner, region, tmp)
            hits = self.scanner.scan_file(path, joined=False)
            post = disk_controls(self.scanner, region, tmp)
        ctl_ok = bool(pre["ok"] and post["ok"] and len(region) > 0)
        ct_found = False
        if self.record_exp and self.record_exp.get("ct"):
            ct_found = file_contains(path, bytes.fromhex(self.record_exp["ct"]))
        doc = {"image": image, "sha256": sha, "size": os.stat(path).st_size, "records": found,
               "record_verdict": verdict, "ciphertext_found": ct_found,
               "scan": {"hits": hits, "controls_ok": ctl_ok}}
        return doc, {"image": image, "region_bytes": len(region), "pre": pre, "post": post, "ok": ctl_ok}


class Env:
    def __init__(self, docker, qemu_factory, disk, clock=None):
        self.docker, self.qemu_factory, self.disk = docker, qemu_factory, disk
        self.clock = clock or Clock()


# =========================================================================== serial log

class SerialLog:
    def __init__(self):
        self.raw = bytearray()
        self.clean = bytearray()  # NULs stripped (as the T0 text())
        self.terminal = False

    def add(self, data: bytes) -> None:
        self.raw += data
        stripped = data.replace(b"\x00", b"")
        tail = bytes(self.clean[-(len(MARKER) - 1):])
        self.clean += stripped
        if MARKER in tail + stripped:
            self.terminal = True

    def text(self) -> str:
        return self.clean.decode("utf-8", "replace")


def qemu_argv(iso, disk, sock_path, accel):
    argv = ["qemu-system-x86_64", "-machine", "q35", "-accel", accel,
            "-cpu", "qemu64,+pdpe1gb,+rdrand,+rdseed" if accel == "tcg" else "host",
            "-m", "256M", "-display", "none", "-chardev", f"socket,id=ser0,path={sock_path},server=on,wait=off",
            "-serial", "chardev:ser0", "-monitor", "none", "-cdrom", str(iso), "-boot", "d", "-no-reboot",
            "-netdev", "user,id=net0", "-device", "e1000,netdev=net0",
            "-drive", f"file={disk},if=none,format=raw,id=p09fx", "-device", "nvme,drive=p09fx,serial=cellosp09"]
    netdevs = [argv[i + 1] for i, a in enumerate(argv) if a == "-netdev"]
    if (netdevs != ["user,id=net0"] or any(a in ("-nic", "-net") for a in argv)
            or any("fwd" in a for a in argv) or any("restrict" in a for a in argv)):
        raise HostError("QEMU networking must be one user netdev without forwards and without restrict")
    return argv


def serial_rows(text: str) -> dict:
    rows: dict = {}
    for line in text.splitlines():
        m = ROW_RE.search(line)
        if m:
            rows.setdefault(m.group(2), []).append(m.group(1))
    return rows


def row_passed(rows: dict, name: str) -> bool:
    got = rows.get(name, [])
    return "PASS" in got and "FAIL" not in got


def role_echoes(text: str) -> list:
    return [m.group(1) for m in ROLE_RE.finditer(text)]


# =========================================================================== sequence

class Sequence:
    def __init__(self, cfg: dict, env: Env):
        """cfg: iso, disk, out, work, set, accel, window, max_boots."""
        self.cfg, self.env = cfg, env
        self.out, self.work = Path(cfg["out"]), Path(cfg["work"])
        self.boots: list = []
        self.controls: list = []
        self.stopped = None
        self.s1 = self.work / "S1.img"
        self.s1r = self.work / "S1r.img"
        self.top = {"set": cfg["set"], "iso_sha256": None, "d0_before_sha256": None, "s1_sha256": None,
                    "s1r_sha256_before_boot5": None, "s1_sha256_after_boot5": None}

    # ---- persistence of the evidence summary
    def _write_json(self, path: Path, obj) -> None:
        path.write_text(json.dumps(obj, indent=2) + "\n")

    def save(self) -> None:
        self._write_json(self.out / "boots.json", {**self.top, "boots": self.boots, "stopped": self.stopped,
                                                   "planned": BOOTS})
        self._write_json(self.out / "controls.json",
                         {"disk": self.controls, "ok": all(c["ok"] for c in self.controls)})

    def stop(self, after_boot: int, reason: str, hint: str) -> None:
        self.stopped = {"after_boot": after_boot, "reason": reason, "class_hint": hint}

    # ---- main
    def run(self) -> int:
        cfg, env = self.cfg, self.env
        self.top["iso_sha256"] = env.disk.sha256(cfg["iso"])
        try:
            self.baseline()
            for n in range(1, min(BOOTS, cfg["max_boots"]) + 1):
                if self.stopped:
                    break
                if n == 5 and not self.prepare_s1r():
                    break
                self.boot(n)
                self.save()
            else:
                if cfg["max_boots"] < BOOTS and not self.stopped:
                    self.stop(cfg["max_boots"], f"--max-boots {cfg['max_boots']} reached", "E")
        finally:
            self.save()
        return 0 if len(self.boots) == BOOTS and self.stopped is None else 1

    def baseline(self) -> None:
        d0 = self.cfg["disk"]
        try:
            sha = self.env.disk.sha256(d0)
            self.top["d0_before_sha256"] = sha
            doc, ctl = self.env.disk.analyze("D0-before", d0, "none", sha)
        except Exception as e:
            self.stop(0, f"D0 baseline failed: {type(e).__name__}: {e}", "E")
            raise
        self._write_json(self.out / "disk-baseline.json", doc)
        self.controls.append(ctl)
        if not ctl["ok"]:
            self.stop(0, "disk scan controls failed on the D0 baseline", "E")
        self.save()

    def prepare_s1r(self) -> bool:
        d = self.env.disk
        try:
            if d.sha256(self.s1) != self.top["s1_sha256"]:
                self.stop(4, "S1 is not pristine before boot 5", "E")
                return False
            d.sparse_copy(self.s1, self.s1r)
            self.top["s1r_sha256_before_boot5"] = d.sha256(self.s1r)
        except Exception as e:
            self.stop(4, f"S1r preparation failed: {type(e).__name__}: {e}", "E")
            return False
        if self.top["s1r_sha256_before_boot5"] != self.top["s1_sha256"]:
            self.stop(4, "S1r differs from S1 after the copy", "E")
            return False
        return True

    # ---- one boot
    def boot(self, n: int) -> None:
        cfg, env = self.cfg, self.env
        bdir = self.out / f"boot{n}"
        try:
            bdir.mkdir()
        except OSError as e:
            self.stop(n, f"cannot create {bdir.name}: {type(e).__name__}", "E")
            return
        disk_path, image = (cfg["disk"], "D0") if n < 5 else (self.s1r, "S1r")
        boot = {"n": n, "role": n, "verdict": "no-verdict",
                "qemu": {"clean": False, "escalated": False, "rc": None, "signal": None},
                "iso_sha256": self.top["iso_sha256"], "disk_image": image, "disk_sha256_after": None,
                "elapsed_s": 0.0, "sshd_lines": None, "ledger_lines": None, "settled": False, "role_written": None}
        self.boots.append(boot)
        log, proc, stop = SerialLog(), None, None
        try:
            # 1. role + pre-boot counts
            env.docker.exec_sh(f'printf "role=%s\\n" {n} > /tmp/kssh-role')
            boot["role_written"] = n
            pre_log = len(env.docker.logs())
            pre_ledger = len(_lines(env.docker.exec_sh(LEDGER_CMD)))
            # 2. QEMU
            sock = str(self.work / f"serial-boot{n}.sock")
            if len(sock) > SOCK_MAX:
                raise HostError("serial socket path too long")
            if os.path.exists(sock):
                os.unlink(sock)
            argv = qemu_argv(cfg["iso"], disk_path, sock, cfg["accel"])
            (bdir / "qemu-argv.txt").write_text(" ".join(argv) + "\n")
            t0 = env.clock.now()
            proc = env.qemu_factory(argv, sock)
            q = self.drive(proc, log, boot)
            boot["elapsed_s"] = round(env.clock.now() - t0, 1)
            # 4. settle (QEMU is gone or declared unclean)
            lines, boot["settled"] = self.settle()
            if len(lines) < pre_log:
                raise HostError("docker logs shrank during the boot")
            ledger = _lines(env.docker.exec_sh(LEDGER_CMD))
            if len(ledger) < pre_ledger:
                raise HostError("ledger shrank during the boot")
            boot["sshd_lines"], boot["ledger_lines"] = [pre_log, len(lines)], [pre_ledger, len(ledger)]
            (bdir / "sshd.log").write_text("".join(x + "\n" for x in lines[pre_log:]))
            (bdir / "p09-ledger.txt").write_text("".join(x + "\n" for x in ledger[pre_ledger:]))
            # 5. serial files, role, then (only now) the disk
            (bdir / "serial.raw.log").write_bytes(bytes(log.raw))
            text = log.text()
            (bdir / "serial.log").write_text(text)
            (bdir / "role.txt").write_text(f"role={n}\n")
            stop = q["stop"]
            if q["clean"]:
                sha = env.disk.sha256(disk_path)
                boot["disk_sha256_after"] = sha
                doc, ctl = env.disk.analyze(image, disk_path, EXPECT[n], sha)
                self._write_json(bdir / "disk-after-boot.json", doc)
                self.controls.append(ctl)
                if not ctl["ok"] and stop is None:
                    stop = ("disk scan controls failed", "E")
                # 6. snapshots
                if n == 1:
                    env.disk.sparse_copy(disk_path, self.s1)
                    self.top["s1_sha256"] = env.disk.sha256(self.s1)
                if n == 5:
                    self.top["s1_sha256_after_boot5"] = env.disk.sha256(self.s1)
            # 7. gate rules
            if stop is None:
                stop = self.decide(n, text, boot)
        except Exception as e:
            stop = (f"{type(e).__name__}: {e}", "E")
        finally:
            if proc is not None:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait(TERM_WAIT_S)
                proc.close()
            self._write_json(self.out / f"boot{n}" / "boot.json", boot)
            if log.raw and not (self.out / f"boot{n}" / "serial.raw.log").exists():
                (self.out / f"boot{n}" / "serial.raw.log").write_bytes(bytes(log.raw))
                (self.out / f"boot{n}" / "serial.log").write_text(log.text())
        if stop:
            self.stop(n, stop[0], stop[1])

    def drive(self, proc, log: SerialLog, boot: dict) -> dict:
        """Pump the serial until the verdict or the window, end QEMU, fill boot["qemu"/"verdict"].
        Returns {"clean": bool, "stop": (reason, hint) | None}."""
        clock, window = self.env.clock, self.cfg["window"]
        q = boot["qemu"]

        def pump(timeout):
            data = proc.recv(timeout)
            if data:
                log.add(data)
            return bool(data)

        if not proc.connect():
            proc.kill()
            proc.wait(TERM_WAIT_S)
            q.update(clean=False, escalated=True, signal="SIGKILL")
            return {"clean": False, "stop": ("QEMU serial socket never appeared", "E")}
        deadline = clock.now() + window
        while clock.now() < deadline and proc.poll() is None:
            pump(0.5)
            if log.terminal:
                boot["verdict"] = "driver-verdict"
                break
        end = clock.now() + POST_VERDICT_S
        while clock.now() < end and proc.poll() is None:
            pump(0.3)
        sig, escalated, rc = None, False, proc.poll()
        if rc is None:
            proc.terminate()
            sig = "SIGTERM"
            rc = proc.wait(TERM_WAIT_S)
            if rc is None:
                proc.kill()
                sig, escalated = "SIGKILL", True
                rc = proc.wait(TERM_WAIT_S)
        alive = proc.poll() is None
        for _ in range(5):  # what the guest wrote just before the exit
            if not pump(0.2):
                break
        failed_early = sig is None and boot["verdict"] == "no-verdict" and rc not in (0, None)
        clean = not escalated and not alive and not failed_early
        q.update(clean=clean, escalated=escalated, rc=None if alive else rc, signal=sig)
        stop = None
        if alive:
            stop = ("QEMU still alive after SIGKILL", "E")
        elif escalated:
            stop = ("QEMU needed SIGKILL (unclean exit)", "E")
        elif failed_early:
            stop = (f"QEMU exited rc={rc} before the driver verdict", "E")
        return {"clean": clean, "stop": stop}

    def settle(self):
        """Poll `docker logs` until the line count is unchanged for 3 s (cap 30 s): (lines, settled)."""
        clock, docker = self.env.clock, self.env.docker
        start = clock.now()
        lines = docker.logs()
        last, changed = len(lines), start
        while True:
            clock.sleep(SETTLE_POLL_S)
            lines = docker.logs()
            now = clock.now()
            if len(lines) != last:
                last, changed = len(lines), now
            if now - changed >= SETTLE_QUIET_S:
                return lines, True
            if now - start >= SETTLE_CAP_S:
                return lines, False

    def decide(self, n: int, text: str, boot: dict):
        """Stop precedence after a clean QEMU: role mismatch (E), no verdict (C), role missing (E), gate rows."""
        echoes = role_echoes(text)
        if echoes and set(echoes) != {str(n)}:
            return (f"role echo mismatch (boot {n}, serial says {sorted(set(echoes))})", "E")
        if boot["verdict"] != "driver-verdict":
            return ("no driver verdict within the window", "C")
        if not echoes:
            return (f"role echo `kssh-obs role n={n}` missing from the serial", "E")
        rows = serial_rows(text)
        bad = [g for g in GATES[n] if not row_passed(rows, g)]
        if bad:
            return (f"gate row(s) failed or missing: {', '.join(bad)}", "gate")
        return None


# =========================================================================== CLI

def _fail(msg: str) -> int:
    print(f"kssh_boots: {msg}", file=sys.stderr)
    return 2


def cmd_run(a) -> int:
    for what, p in (("--iso", a.iso), ("--disk", a.disk), ("--keys", a.keys)):
        if not os.path.exists(p):
            return _fail(f"{what} {p} does not exist")
    if not 1 <= a.max_boots <= BOOTS:
        return _fail(f"--max-boots must be 1..{BOOTS}")
    out, work = Path(a.out), Path(a.work)
    out.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    clash = [str(p) for p in [out / "boots.json", out / "disk-baseline.json", out / "controls.json",
                              work / "S1.img", work / "S1r.img", *(out / f"boot{n}" for n in range(1, BOOTS + 1))]
             if p.exists()]
    if clash:
        return _fail("refusing to overwrite: " + ", ".join(clash))
    try:
        import kssh_judge
        import kssh_keys
        extras = [(f, "password" if "password" in Path(f).name.lower() else None) for f in a.extra_secret_file]
        scanner = kssh_judge.Scanner(kssh_keys.canaries(a.keys, extras))
        record_exp = kssh_keys.load_expected(a.keys).get("record")
    except Exception as e:
        return _fail(f"key material: {type(e).__name__}")
    cfg = {"iso": a.iso, "disk": a.disk, "out": str(out), "work": str(work), "set": a.set, "accel": a.accel,
           "window": a.window, "max_boots": a.max_boots}
    env = Env(DockerCli(a.container), QemuProc, DiskOps(scanner, record_exp))
    try:
        return Sequence(cfg, env).run()
    except Exception as e:
        return _fail(f"host error: {type(e).__name__}: {e}")


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--iso", required=True)
    r.add_argument("--disk", required=True)
    r.add_argument("--container", required=True)
    r.add_argument("--out", required=True)
    r.add_argument("--keys", required=True)
    r.add_argument("--work", required=True)
    r.add_argument("--set", required=True, choices=("b0", "rt1"))
    r.add_argument("--extra-secret-file", action="append", default=[])
    r.add_argument("--window", type=int, default=1800)
    r.add_argument("--accel", default="tcg")
    r.add_argument("--max-boots", type=int, default=BOOTS)
    try:
        a = ap.parse_args(argv)
    except SystemExit as e:
        return 2 if e.code else 0
    return cmd_run(a)


if __name__ == "__main__":
    sys.exit(main())
