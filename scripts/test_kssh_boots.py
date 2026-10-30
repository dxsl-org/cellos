#!/usr/bin/env python3
"""Host tests for kssh_boots.py with a fake docker, a fake QEMU and a fake disk layer.

    python3 scripts/test_kssh_boots.py

The fakes keep ONE virtual clock (sleeping advances it), so the 3 s / 10 s / 30 s rules run instantly.
Nothing here boots QEMU, calls docker or needs kssh_records.py (the fake disk layer replaces it).
"""
from __future__ import annotations

import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kssh_boots as kb  # noqa: E402

MID = {
    1: ["kssh-s01a", "kssh-s01b", "kssh-s01", "kssh-s09", "kssh-s02"],
    2: ["kssh-t1-load", "kssh-t1-opaque", "kssh-s02r", "kssh-t1-s08"],
    3: ["kssh-t1-load", "kssh-s02x", "kssh-t1-del", "kssh-t1-plain", "kssh-s15"],
    4: ["kssh-t1-absent"],
    5: ["kssh-t1-load", "kssh-t1-revert"],
}


# =========================================================================== fakes

class FakeClock:
    def __init__(self):
        self.t = 1000.0

    def now(self):
        return self.t

    def sleep(self, s):
        self.t += s


class FakeDocker:
    def __init__(self, world):
        self.w = world
        self.base = ["listener up", "sshd started", "ready"]
        self.timed = []  # (due, line)
        self.ledger = []
        self.timed_ledger = []

    def add(self, *lines):
        self.base.extend(lines)

    def add_at(self, delay, line):
        self.timed.append((self.w.clock.now() + delay, line))

    def drip(self, start, count, step=1.0):
        for i in range(count):
            self.timed.append((self.w.clock.now() + start + i * step, f"drip {i}"))

    def exec_sh(self, script):
        self.w.events.append(("exec", script))
        if "p09-ledger" in script:
            return "".join(x + "\n" for x in self.ledger)
        if script.startswith('printf "role=%s'):
            self.w.role_scripts.append(script)
            return ""
        raise AssertionError(script)

    def logs(self):
        now = self.w.clock.now()
        due = sorted(x for x in self.timed if x[0] <= now)
        self.timed = [x for x in self.timed if x[0] > now]
        self.base.extend(l for _, l in due)  # docker logs only ever grow
        return list(self.base)


class FakeQemu:
    def __init__(self, world, argv, sock):
        self.w, self.argv, self.sock = world, argv, sock
        self.n = len(world.argvs) + 1
        world.argvs.append(argv)
        world.running += 1
        world.events.append(("start", self.n))
        self.script = world.scripts[self.n - 1]
        self.items = list(self.script.get("items", []))
        self.alive = True
        self.rc = None
        self.disk_path = [a for a in argv if a.startswith("file=")][0][5:].split(",")[0]
        self.signals = []

    def connect(self):
        return not self.script.get("no_socket", False)

    def recv(self, timeout):
        if not self.alive:
            return b""
        if self.items:
            it = self.items.pop(0)
            self.w.clock.sleep(0.1)
            if callable(it):
                it(self)
                return b""
            return it
        self.w.clock.sleep(timeout)
        return b""

    def poll(self):
        return None if self.alive else self.rc

    def _die(self, rc):
        if self.alive:
            self.alive, self.rc = False, rc
            self.w.running -= 1
            self.w.events.append(("exit", self.n))
            if "on_exit" in self.script:
                self.script["on_exit"](self)

    def terminate(self):
        self.signals.append("TERM")
        if not self.script.get("ignore_term"):
            self._die(self.script.get("exit_rc", 0))

    def kill(self):
        self.signals.append("KILL")
        if not self.script.get("ignore_kill"):
            self._die(-9)

    def wait(self, timeout):
        if not self.alive:
            return self.rc
        self.w.clock.sleep(timeout)
        return None

    def close(self):
        pass


class FakeDisk:
    """Content-addressed in-memory disk layer; every access checks that no QEMU is running."""

    def __init__(self, world):
        self.w = world
        self.analyzed = []
        self.sha_calls = []
        self.controls_ok = {}

    def _guard(self, what, path):
        self.w.events.append((what, str(path)))
        if self.w.running:
            self.w.violations.append((what, str(path)))

    def sha256(self, path):
        self._guard("sha256", path)
        self.sha_calls.append(str(path))
        return hashlib.sha256(self.w.disks.get(str(path), b"<iso>")).hexdigest()

    def sparse_copy(self, src, dst):
        self._guard("copy", f"{src}->{dst}")
        if str(dst) in self.w.disks:
            raise FileExistsError(dst)
        self.w.disks[str(dst)] = self.w.disks[str(src)]

    def analyze(self, image, path, expect, sha):
        self._guard("analyze", path)
        self.analyzed.append((image, str(path), expect, sha))
        ok = self.controls_ok.get(image, True)
        doc = {"image": image, "sha256": sha, "size": len(self.w.disks[str(path)]), "records": {"valid": [], "invalid": [], "v1": []},
               "record_verdict": {"expect": expect, "ok": True, "reasons": []}, "ciphertext_found": image != "D0-before",
               "scan": {"hits": [], "controls_ok": ok}}
        return doc, {"image": image, "region_bytes": 10, "pre": {"ok": ok}, "post": {"ok": ok}, "ok": ok}


class World:
    def __init__(self, tmp: Path):
        self.clock = FakeClock()
        self.events, self.role_scripts, self.argvs, self.violations = [], [], [], []
        self.running = 0
        self.docker = FakeDocker(self)
        self.disk = FakeDisk(self)
        self.scripts = []
        self.tmp = tmp
        self.out, self.work = tmp / "evidence", tmp / "work"
        self.out.mkdir()
        self.work.mkdir()
        self.d0 = str(tmp / "D0.img")
        self.disks = {self.d0: b"d0-pristine"}
        self.qemus = []

    def factory(self, argv, sock):
        q = FakeQemu(self, argv, sock)
        self.qemus.append(q)
        return q

    def run(self, max_boots=5, window=1800, accel="tcg"):
        cfg = {"iso": str(self.tmp / "t.iso"), "disk": self.d0, "out": str(self.out), "work": str(self.work),
               "set": "rt1", "accel": accel, "window": window, "max_boots": max_boots}
        env = kb.Env(self.docker, self.factory, self.disk, self.clock)
        self.seq = kb.Sequence(cfg, env)
        self.rc = self.seq.run()
        return self.rc

    def boots(self):
        return json.loads((self.out / "boots.json").read_text())

    def boot_json(self, n):
        return json.loads((self.out / f"boot{n}" / "boot.json").read_text())


def serial(n, role="auto", fail=(), drop=(), terminal=True):
    """Serial bytes of boot n as a list of chunks (with a NUL and CRLFs like the real stream)."""
    role = n if role == "auto" else role
    rows = ["shell-prompt", "kssh-conf", "kssh-s00", "kssh-role"] + MID[n] + ["kssh-transcripts", "session-a-close"]
    lines = []
    for r in rows:
        if r in drop:
            continue
        lines.append(f"P04 {'FAIL' if r in fail else 'PASS'} {r} detail")
        if r == "kssh-s00" and role is not None:
            lines.append(f"P04 INFO kssh-obs role n={role}")
    if terminal:
        lines.append("P04-TERMINAL: PASS checks=9")
    data = ("".join(l + "\r\n" for l in lines)).encode()
    cut = len(data) // 2
    return [data[:cut] + b"\x00", data[cut:]]


def good_script(n, **kw):
    def writes(q):
        q.w.disks[q.disk_path] = f"{q.disk_path}|boot{n}".encode()

    def sshd(q):
        q.w.docker.add(f"sshd boot{n} accept", f"sshd boot{n} close")
        q.w.docker.ledger.append(f"p09x boot{n}")

    chunks = serial(n, **{k: v for k, v in kw.items() if k in ("role", "fail", "drop", "terminal")})
    script = {"items": [sshd, chunks[0], writes, chunks[1]]}
    script.update({k: v for k, v in kw.items() if k not in ("role", "fail", "drop", "terminal")})
    return script


class Base(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.addCleanup(self._td.cleanup)
        self.w = World(Path(self._td.name))
        self.w.scripts = [good_script(n) for n in range(1, 6)]

    def read(self, n, name):
        return (self.w.out / f"boot{n}" / name).read_text()


# =========================================================================== tests

class Sequencing(Base):
    def test_full_run_order_roles_and_files(self):
        self.assertEqual(self.w.run(), 0)
        self.assertEqual(self.w.role_scripts, [f'printf "role=%s\\n" {n} > /tmp/kssh-role' for n in range(1, 6)])
        ev = [e for e in self.w.events if e[0] in ("start", "exit")]
        self.assertEqual(ev, [x for n in range(1, 6) for x in (("start", n), ("exit", n))])
        # per boot: role exec precedes the start of that boot
        roles = [i for i, e in enumerate(self.w.events) if e[0] == "exec" and "role=" in e[1]]
        starts = [i for i, e in enumerate(self.w.events) if e[0] == "start"]
        exits = [i for i, e in enumerate(self.w.events) if e[0] == "exit"]
        analyzes = [i for i, e in enumerate(self.w.events) if e[0] == "analyze"]
        self.assertTrue(all(r < s < x for r, s, x in zip(roles, starts, exits)))
        self.assertEqual(len(analyzes), 6)  # baseline + 5
        for n in range(1, 6):
            for f in ("serial.raw.log", "serial.log", "sshd.log", "p09-ledger.txt", "qemu-argv.txt", "role.txt",
                      "boot.json", "disk-after-boot.json"):
                self.assertTrue((self.w.out / f"boot{n}" / f).is_file(), f"boot{n}/{f}")
            self.assertEqual(self.read(n, "role.txt"), f"role={n}\n")
            bj = self.w.boot_json(n)
            self.assertEqual((bj["n"], bj["role"], bj["role_written"], bj["verdict"]), (n, n, n, "driver-verdict"))
            self.assertEqual(bj["qemu"], {"clean": True, "escalated": False, "rc": 0, "signal": "SIGTERM"})
        self.assertEqual([a[:3] for a in self.w.disk.analyzed],
                         [("D0-before", self.w.d0, "none"), ("D0", self.w.d0, "r1"), ("D0", self.w.d0, "r1"),
                          ("D0", self.w.d0, "r1r2"), ("D0", self.w.d0, "r1r2"), ("S1r", str(self.w.work / "S1r.img"), "r1")])
        self.assertTrue((self.w.out / "disk-baseline.json").is_file())
        top = self.w.boots()
        self.assertEqual((top["set"], top["planned"], top["stopped"], len(top["boots"])), ("rt1", 5, None, 5))
        self.assertEqual([b["disk_image"] for b in top["boots"]], ["D0", "D0", "D0", "D0", "S1r"])
        self.assertEqual(self.w.violations, [])
        self.assertTrue(json.loads((self.w.out / "controls.json").read_text())["ok"])

    def test_serial_files_strip_nul_only_in_clean_log(self):
        self.w.run(max_boots=1)
        raw = (self.w.out / "boot1" / "serial.raw.log").read_bytes()
        self.assertIn(b"\x00", raw)
        self.assertNotIn("\x00", self.read(1, "serial.log"))
        self.assertIn("P04-TERMINAL: PASS", self.read(1, "serial.log"))

    def test_argv_equal_modulo_disk_and_socket(self):
        self.w.run()
        base = self.w.argvs[0]
        self.assertEqual(base[0], "qemu-system-x86_64")
        for n, argv in enumerate(self.w.argvs, 1):
            self.assertEqual(len(argv), len(base))
            diff = [i for i, (a, b) in enumerate(zip(argv, base)) if a != b]
            kinds = {("sock" if "path=" in argv[i] else "disk" if argv[i].startswith("file=") else "other") for i in diff}
            self.assertLessEqual(kinds, {"sock", "disk"}, n)
            self.assertEqual(self.read(n, "qemu-argv.txt"), " ".join(argv) + "\n")
        socks = [[a for a in argv if "path=" in a][0] for argv in self.w.argvs]
        self.assertEqual(len(set(socks)), 5)
        self.assertTrue(all(str(self.w.work) in s for s in socks))
        self.assertEqual(base[1:11], ["-machine", "q35", "-accel", "tcg", "-cpu", "qemu64,+pdpe1gb,+rdrand,+rdseed", "-m", "256M",
                                      "-display", "none"])
        self.assertEqual(base[11], "-chardev")
        self.assertEqual([a for i, a in enumerate(base) if a == "-netdev"], ["-netdev"])
        self.assertIn("user,id=net0", base)
        self.assertIn("-no-reboot", base)
        self.assertIn("nvme,drive=p09fx,serial=cellosp09", base)
        self.assertFalse(any("fwd" in a or "restrict" in a for a in base))

    def test_argv_asserts(self):
        with self.assertRaises(kb.HostError):
            kb.qemu_argv("a.iso", "/tmp/fwd.img", "/tmp/s", "tcg")
        argv = kb.qemu_argv("a.iso", "/tmp/d.img", "/tmp/s", "kvm")
        self.assertIn("host", argv)

    def test_s1_snapshot_and_s1r_only_boot5(self):
        self.w.run()
        d0, s1, s1r = self.w.d0, str(self.w.work / "S1.img"), str(self.w.work / "S1r.img")
        self.assertEqual(self.w.disks[s1], f"{d0}|boot1".encode())  # copy of D0 as it was after boot 1
        self.assertEqual(self.w.disks[d0], f"{d0}|boot4".encode())  # D0 advanced to boot 4, boot 5 left it alone
        self.assertEqual(self.w.disks[s1r], f"{s1r}|boot5".encode())
        for q in self.w.qemus:
            self.assertNotEqual(q.disk_path, s1)
        self.assertEqual([q.disk_path for q in self.w.qemus], [d0, d0, d0, d0, s1r])
        top = self.w.boots()
        sha = lambda b: hashlib.sha256(b).hexdigest()  # noqa: E731
        self.assertEqual(top["s1_sha256"], sha(f"{d0}|boot1".encode()))
        self.assertEqual(top["s1r_sha256_before_boot5"], top["s1_sha256"])
        self.assertEqual(top["s1_sha256_after_boot5"], top["s1_sha256"])
        self.assertEqual(top["d0_before_sha256"], sha(b"d0-pristine"))
        self.assertEqual(self.w.boot_json(5)["disk_sha256_after"], sha(f"{s1r}|boot5".encode()))
        # S1r is created after boot 4 exited and before boot 5 started
        ev = self.w.events
        i_copy = next(i for i, e in enumerate(ev) if e[0] == "copy" and e[1].endswith("S1r.img"))
        self.assertGreater(i_copy, ev.index(("exit", 4)))
        self.assertLess(i_copy, ev.index(("start", 5)))
        self.assertEqual(self.w.violations, [])

    def test_s1_tampered_before_boot5_stops_E(self):
        s1 = str(self.w.work / "S1.img")
        self.w.scripts[2]["items"].insert(0, lambda q: q.w.disks.__setitem__(s1, b"tampered"))
        self.assertEqual(self.w.run(), 1)
        self.assertEqual(len(self.w.qemus), 4)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (4, "E"))
        self.assertIn("pristine", st["reason"])
        self.assertFalse(any(e[0] == "copy" and e[1].endswith("S1r.img") for e in self.w.events))

    def test_max_boots_limit(self):
        self.assertEqual(self.w.run(max_boots=2), 1)
        self.assertEqual(len(self.w.qemus), 2)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (2, "E"))

    def test_boot_dirs_are_not_reused(self):
        (self.w.out / "boot1").mkdir()
        self.assertEqual(self.w.run(), 1)  # mkdir of an existing boot dir is a harness failure
        self.assertEqual(len(self.w.qemus), 0)


class Slices(Base):
    def test_slice_boundaries_and_settle(self):
        self.w.docker.add("pre-existing 1", "pre-existing 2")
        self.w.docker.ledger.append("old ledger line")
        self.w.run(max_boots=2)
        b1, b2 = self.w.boot_json(1), self.w.boot_json(2)
        self.assertEqual(b1["sshd_lines"], [5, 7])
        self.assertEqual(b2["sshd_lines"], [7, 9])
        self.assertEqual(self.read(1, "sshd.log"), "sshd boot1 accept\nsshd boot1 close\n")
        self.assertEqual(self.read(2, "sshd.log"), "sshd boot2 accept\nsshd boot2 close\n")
        self.assertEqual(b1["ledger_lines"], [1, 2])
        self.assertEqual(b2["ledger_lines"], [2, 3])
        self.assertEqual(self.read(1, "p09-ledger.txt"), "p09x boot1\n")
        self.assertEqual(self.read(2, "p09-ledger.txt"), "p09x boot2\n")
        self.assertTrue(b1["settled"] and b2["settled"])

    def test_late_close_after_qemu_exit_stays_in_slice(self):
        # the cut connection logs its close 2 s after QEMU is gone: inside the quiet window restart
        self.w.scripts[0]["on_exit"] = lambda q: q.w.docker.add_at(2.0, "sshd boot1 late close")
        self.w.run(max_boots=2)
        self.assertIn("sshd boot1 late close\n", self.read(1, "sshd.log"))
        self.assertNotIn("late close", self.read(2, "sshd.log"))
        self.assertTrue(self.w.boot_json(1)["settled"])
        self.assertEqual(self.w.boot_json(2)["sshd_lines"][0], self.w.boot_json(1)["sshd_lines"][1])

    def test_line_after_quiet_window_is_the_next_slice_problem(self):
        self.w.scripts[0]["on_exit"] = lambda q: q.w.docker.add_at(8.0, "sshd orphan")
        self.w.run(max_boots=2)
        self.assertNotIn("orphan", self.read(1, "sshd.log"))
        self.assertTrue(self.w.boot_json(1)["settled"])
        # it lands after boot 1's slice was cut, so it shows up in boot 2's slice (the judge calls that an orphan, class C)
        self.assertIn("sshd orphan\n", self.read(2, "sshd.log"))

    def test_settle_cap_30s(self):
        self.w.scripts[0]["on_exit"] = lambda q: q.w.docker.drip(0.5, 80, 1.0)
        t_before = None

        def mark(q):
            nonlocal t_before
            t_before = q.w.clock.now()
        self.w.scripts[0]["items"].insert(0, mark)
        self.w.run(max_boots=1)
        b1 = self.w.boot_json(1)
        self.assertFalse(b1["settled"])
        self.assertLessEqual(self.w.clock.now() - t_before, 3 + 10 + 31 + 5)
        n_drip = self.read(1, "sshd.log").count("drip")
        self.assertTrue(28 <= n_drip <= 31, n_drip)
        self.assertIn("max-boots", self.w.seq.stopped["reason"])  # an unsettled boot is recorded, not a stop

    def test_settle_polls_without_sleeping_past_cap(self):
        calls = []
        orig = self.w.docker.logs
        self.w.docker.logs = lambda: (calls.append(self.w.clock.now()), orig())[1]
        self.w.scripts[0]["on_exit"] = lambda q: q.w.docker.drip(0.5, 200, 1.0)
        self.w.run(max_boots=1)
        after_exit = [c for c in calls if c >= calls[-1] - 31]
        self.assertTrue(all(b - a <= kb.SETTLE_POLL_S + 1e-9 for a, b in zip(after_exit, after_exit[1:])))


class Unclean(Base):
    def test_sigkill_escalation_is_unclean_E_without_snapshot(self):
        self.w.scripts[0]["ignore_term"] = True
        self.assertEqual(self.w.run(), 1)
        q = self.w.qemus[0]
        self.assertEqual(q.signals, ["TERM", "KILL"])
        b1 = self.w.boot_json(1)
        self.assertEqual(b1["qemu"]["clean"], False)
        self.assertEqual(b1["qemu"]["escalated"], True)
        self.assertEqual(b1["qemu"]["signal"], "SIGKILL")
        self.assertIsNone(b1["disk_sha256_after"])
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (1, "E"))
        self.assertEqual(len(self.w.qemus), 1)
        self.assertFalse((self.w.work / "S1.img").exists())
        self.assertNotIn(str(self.w.work / "S1.img"), self.w.disks)
        self.assertEqual([a[0] for a in self.w.disk.analyzed], ["D0-before"])  # no disk work after the unclean exit
        self.assertFalse((self.w.out / "boot1" / "disk-after-boot.json").exists())
        self.assertTrue((self.w.out / "boot1" / "serial.log").is_file())  # evidence kept
        self.assertTrue((self.w.out / "boot1" / "sshd.log").is_file())
        self.assertIsNone(self.w.boots()["s1_sha256"])

    def test_qemu_still_alive_after_kill_touches_no_disk(self):
        self.w.scripts[0].update(ignore_term=True, ignore_kill=True)
        self.assertEqual(self.w.run(), 1)
        self.assertEqual(self.w.boots()["stopped"]["class_hint"], "E")
        self.assertEqual(self.w.violations, [])
        self.assertEqual([a[0] for a in self.w.disk.analyzed], ["D0-before"])
        self.assertEqual(self.w.disk.sha_calls.count(self.w.d0), 1)  # only the baseline read
        self.assertIsNone(self.w.boot_json(1)["qemu"]["rc"])

    def test_socket_never_appeared(self):
        self.w.scripts[0]["no_socket"] = True
        self.assertEqual(self.w.run(), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (1, "E"))
        self.assertFalse(self.w.boot_json(1)["qemu"]["clean"])

    def test_qemu_failure_before_verdict_is_E_not_C(self):
        self.w.scripts[0] = {"items": [serial(1, terminal=False)[0], lambda q: q._die(1)]}
        self.assertEqual(self.w.run(), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual(st["class_hint"], "E")
        self.assertFalse(self.w.boot_json(1)["qemu"]["clean"])
        self.assertEqual([a[0] for a in self.w.disk.analyzed], ["D0-before"])

    def test_qemu_exiting_by_itself_after_verdict_is_clean(self):
        s = good_script(1)
        s["items"].append(lambda q: q._die(0))
        self.w.scripts[0] = s
        self.w.run(max_boots=1)
        q = self.w.boot_json(1)["qemu"]
        self.assertEqual((q["clean"], q["signal"], q["escalated"]), (True, None, False))
        self.assertEqual(self.w.qemus[0].signals, [])

    def test_exception_in_a_boot_is_E_and_kills_qemu(self):
        def boom(q):
            raise OSError("serial exploded")
        self.w.scripts[1]["items"].insert(1, boom)
        self.assertEqual(self.w.run(), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (2, "E"))
        self.assertFalse(self.w.qemus[1].alive)
        self.assertEqual(len(self.w.qemus), 2)
        self.assertTrue((self.w.out / "boot2" / "boot.json").is_file())


class Gates(Base):
    def stops(self, n, **kw):
        self.w.scripts[n - 1] = good_script(n, **kw)
        rc = self.w.run()
        return rc, self.w.boots()["stopped"]

    def test_gate_rows_stop(self):
        cases = [(1, dict(fail=("kssh-s01",))), (1, dict(fail=("kssh-s02",))), (1, dict(drop=("kssh-s02",))),
                 (1, dict(drop=("kssh-s01",))), (2, dict(fail=("kssh-t1-load",))), (2, dict(drop=("kssh-s02r",))),
                 (2, dict(fail=("kssh-s02r",))), (3, dict(fail=("kssh-t1-del",))), (3, dict(drop=("kssh-t1-del",)))]
        for n, kw in cases:
            with self.subTest(n=n, kw=kw):
                self.setUp()
                rc, st = self.stops(n, **kw)
                self.assertEqual(rc, 1)
                self.assertEqual((st["after_boot"], st["class_hint"]), (n, "gate"))
                self.assertEqual(len(self.w.qemus), n)
                self.assertEqual(len(self.w.boots()["boots"]), n)

    def test_non_gate_rows_do_not_stop(self):
        cases = [(1, dict(fail=("kssh-s09", "kssh-s01a"))), (2, dict(fail=("kssh-t1-opaque", "kssh-t1-s08"))),
                 (3, dict(fail=("kssh-s02x", "kssh-t1-plain", "kssh-s15"))), (4, dict(fail=("kssh-t1-absent",))),
                 (5, dict(fail=("kssh-t1-revert", "kssh-t1-load")))]
        for n, kw in cases:
            with self.subTest(n=n):
                self.setUp()
                self.w.scripts[n - 1] = good_script(n, **kw)
                self.assertEqual(self.w.run(), 0)
                self.assertEqual(len(self.w.qemus), 5)

    def test_prefix_row_names_do_not_satisfy_gate(self):
        # kssh-s01a/kssh-s01b passing must not count as kssh-s01; kssh-s02x as kssh-s02
        rc, st = self.stops(1, drop=("kssh-s01",))
        self.assertEqual(st["class_hint"], "gate")
        self.setUp()
        rc, st = self.stops(3, drop=("kssh-t1-del",))
        self.assertEqual(st["after_boot"], 3)

    def test_pass_then_fail_of_same_row_is_failed(self):
        s = good_script(1)
        extra = b"P04 FAIL kssh-s02 again\r\n"
        s["items"].append(extra)
        self.w.scripts[0] = s
        self.assertEqual(self.w.run(), 1)
        self.assertEqual(self.w.boots()["stopped"]["class_hint"], "gate")

    def test_stopped_boot_still_recorded_and_snapshot_taken_after_boot1(self):
        rc, st = self.stops(1, fail=("kssh-s02",))
        self.assertEqual(self.w.boot_json(1)["verdict"], "driver-verdict")
        self.assertIn("kssh-s02", st["reason"])
        self.assertTrue(self.w.boots()["s1_sha256"])

    def test_role_mismatch(self):
        rc, st = self.stops(2, role=1)
        self.assertEqual((rc, st["after_boot"], st["class_hint"]), (1, 2, "E"))
        self.assertIn("mismatch", st["reason"])
        self.assertEqual(len(self.w.qemus), 2)

    def test_role_missing_with_verdict(self):
        rc, st = self.stops(3, role=None)
        self.assertEqual((st["after_boot"], st["class_hint"]), (3, "E"))
        self.assertIn("missing", st["reason"])

    def test_role_mismatch_beats_gate_failure(self):
        rc, st = self.stops(1, role=3, fail=("kssh-s02",))
        self.assertEqual(st["class_hint"], "E")

    def test_role_mismatch_without_verdict_is_E(self):
        rc, st = self.stops(1, role=4, terminal=False)
        self.assertEqual(st["class_hint"], "E")

    def test_no_verdict_stops_C(self):
        self.w.scripts[0] = good_script(1, terminal=False)
        self.assertEqual(self.w.run(window=40), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (1, "C"))
        b1 = self.w.boot_json(1)
        self.assertEqual(b1["verdict"], "no-verdict")
        self.assertEqual(b1["qemu"]["signal"], "SIGTERM")
        self.assertTrue(b1["qemu"]["clean"])
        self.assertEqual(len(self.w.qemus), 1)
        self.assertEqual(self.w.violations, [])

    def test_window_is_respected(self):
        self.w.scripts[0] = {"items": []}
        t0 = self.w.clock.now()
        self.w.run(window=100, max_boots=1)
        self.assertTrue(100 <= self.w.boot_json(1)["elapsed_s"] <= 100 + 3 + 10 + 2, self.w.boot_json(1)["elapsed_s"])
        self.assertGreaterEqual(self.w.clock.now() - t0, 100)

    def test_verdict_waits_3s_before_sigterm(self):
        seen = {}
        s = good_script(1)
        s["items"].append(lambda q: seen.update(verdict_at=q.w.clock.now()))
        orig = FakeQemu.terminate

        def term(q):
            seen["term_at"] = q.w.clock.now()
            orig(q)
        with mock.patch.object(FakeQemu, "terminate", term):
            self.w.scripts[0] = s
            self.w.run(max_boots=1)
        self.assertGreaterEqual(seen["term_at"] - seen["verdict_at"], 2.9)

    def test_failed_controls_stop_E(self):
        self.w.disk.controls_ok["D0"] = False
        self.assertEqual(self.w.run(), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (1, "E"))
        self.assertFalse(json.loads((self.w.out / "controls.json").read_text())["ok"])

    def test_failed_baseline_controls_stop_before_boot1(self):
        self.w.disk.controls_ok["D0-before"] = False
        self.assertEqual(self.w.run(), 1)
        st = self.w.boots()["stopped"]
        self.assertEqual((st["after_boot"], st["class_hint"]), (0, "E"))
        self.assertEqual(len(self.w.qemus), 0)


class DiskAfterExit(Base):
    def test_no_disk_access_while_qemu_runs(self):
        self.w.run()
        self.assertEqual(self.w.violations, [])

    def test_guard_detects_access_while_running(self):
        self.w.running = 1
        self.w.disk.sha256(self.w.d0)
        self.assertEqual(len(self.w.violations), 1)

    def test_disk_read_events_follow_exit_events(self):
        self.w.run()
        ev = self.w.events
        for n in range(1, 6):
            exit_i = ev.index(("exit", n))
            start_i = ev.index(("start", n))
            between = [e for e in ev[start_i:exit_i] if e[0] in ("sha256", "analyze", "copy")]
            self.assertEqual(between, [], n)
            self.assertTrue(any(e[0] == "analyze" for e in ev[exit_i:]))
        # the sparse copy of boot 1 happens strictly after boot 1's analysis
        i_an = next(i for i, e in enumerate(ev) if e[0] == "analyze" and i > ev.index(("exit", 1)))
        i_cp = next(i for i, e in enumerate(ev) if e[0] == "copy" and e[1].endswith("S1.img"))
        self.assertLess(i_an, i_cp)


# =========================================================================== real adapters and helpers

class Adapters(unittest.TestCase):
    def test_docker_commands(self):
        calls = []

        def run(args, **kw):
            calls.append((args, kw))
            return subprocess.CompletedProcess(args, 0, stdout=b"a\nb\n", stderr=b"")
        d = kb.DockerCli("ctr")
        with mock.patch.object(subprocess, "run", run):
            self.assertEqual(d.logs(), ["a", "b"])
            d.exec_sh("echo hi")
        self.assertEqual(calls[0][0], ["docker", "logs", "ctr"])
        self.assertEqual(calls[0][1]["stderr"], subprocess.STDOUT)  # `2>&1`
        self.assertEqual(calls[1][0], ["docker", "exec", "ctr", "sh", "-c", "echo hi"])

    def test_docker_failure_is_host_error(self):
        with mock.patch.object(subprocess, "run", lambda a, **k: subprocess.CompletedProcess(a, 1, b"", b"x")):
            with self.assertRaises(kb.HostError):
                kb.DockerCli("c").logs()

    def test_serial_log_marker_across_chunks_and_nuls(self):
        s = kb.SerialLog()
        s.add(b"abc P04-TERM\x00")
        self.assertFalse(s.terminal)
        s.add(b"INAL: PASS\r\n")
        self.assertTrue(s.terminal)
        self.assertEqual(s.text(), "abc P04-TERMINAL: PASS\r\n")

    def test_rows_and_role_parsing(self):
        t = "P04 PASS kssh-s01 x\r\nP04 FAIL kssh-s01a y\r\nP04 INFO kssh-obs role n=3\r\n"
        rows = kb.serial_rows(t)
        self.assertTrue(kb.row_passed(rows, "kssh-s01"))
        self.assertFalse(kb.row_passed(rows, "kssh-s01a"))
        self.assertFalse(kb.row_passed(rows, "kssh-s02"))
        self.assertEqual(kb.role_echoes(t), ["3"])

    def test_cli_usage_errors(self):
        devnull = open("/dev/null", "w")
        self.addCleanup(devnull.close)
        with tempfile.TemporaryDirectory() as td, mock.patch.object(sys, "stderr", devnull):
            self.assertEqual(kb.main(["run"]), 2)
            self.assertEqual(kb.main(["bogus"]), 2)
            args = ["run", "--iso", f"{td}/nope.iso", "--disk", f"{td}/nope", "--container", "c", "--out", f"{td}/o",
                    "--keys", td, "--work", f"{td}/w", "--set", "b0"]
            self.assertEqual(kb.main(args), 2)
            iso = Path(td) / "x.iso"
            iso.write_bytes(b"i")
            args[2] = args[4] = str(iso)
            self.assertEqual(kb.main(args + ["--max-boots", "6"]), 2)
            self.assertEqual(kb.main(args + ["--set", "t0"]), 2)


class DiskControls(unittest.TestCase):
    def scanner(self):
        import kssh_judge
        return kssh_judge.Scanner([("pass:a", b"correct-horse-battery-staple-9"), ("seed:k", bytes(range(1, 33)))])

    def test_controls_pass_on_real_scanner(self):
        sc = self.scanner()
        region = bytes(range(256)) * 512
        with tempfile.TemporaryDirectory() as td:
            r = kb.disk_controls(sc, region, Path(td))
        self.assertTrue(r["ok"], r)
        self.assertEqual(r["disk-unaligned"]["missing"], [])
        self.assertEqual(r["disk-straddle"]["missing"], [])
        self.assertGreater(r["disk-unaligned"]["unaligned_plants"], 0)
        self.assertNotIn("correct-horse", json.dumps(r))

    def test_controls_fail_when_scanner_is_blind(self):
        sc = self.scanner()
        sc.scan_file = lambda *a, **k: []
        with tempfile.TemporaryDirectory() as td:
            r = kb.disk_controls(sc, b"\x01" * 4096, Path(td))
        self.assertFalse(r["ok"])

    def test_controls_fail_when_scanner_misses_chunk_straddle(self):
        sc = self.scanner()
        orig = sc.scan_file

        def blind(path, joined=True, chunk=kb.__dict__.get("CHUNK", 8 << 20)):
            return [] if Path(path).name == "disk-straddle.bin" else orig(path, joined=joined, chunk=chunk)
        sc.scan_file = blind
        with tempfile.TemporaryDirectory() as td:
            r = kb.disk_plant_controls(sc, b"\x01" * 4096, Path(td))
        self.assertFalse(r["ok"])
        self.assertTrue(r["disk-straddle"]["missing"])

    def test_file_contains_straddles_chunk_and_skips_zeros(self):
        needle = bytes(range(10, 30))
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "img"
            data = bytearray(4096 * 4)
            data[4096 - 7:4096 - 7 + 20] = needle
            p.write_bytes(bytes(data))
            self.assertTrue(kb.file_contains(p, needle, chunk=4096))
            self.assertFalse(kb.file_contains(p, needle[::-1], chunk=4096))
            p.write_bytes(bytes(4096 * 4))
            self.assertFalse(kb.file_contains(p, needle, chunk=4096))

    def test_data_region_takes_nonzero_blocks_only(self):
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "img"
            p.write_bytes(bytes(1024) + b"\x07" * 1024 + bytes(1024) + b"\x09" * 100)
            self.assertEqual(kb.data_region(p, limit=10**6, block=1024), b"\x07" * 1024 + b"\x09" * 100)
            self.assertEqual(len(kb.data_region(p, limit=10, block=1024)), 10)
            p.write_bytes(bytes(4096))
            self.assertEqual(kb.data_region(p), b"")

    def test_disk_ops_analyze_with_stub_records(self):
        """DiskOps glue: records oracle + scan + controls + positive ciphertext control, no secret in output."""
        ct = bytes(range(100, 164))
        sc = self.scanner()
        stub = mock.Mock()
        stub.find_records.return_value = {"valid": [{"offset": 5}], "invalid": [], "v1": []}
        stub.evaluate_set.return_value = {"expect": "r1", "ok": True, "reasons": []}
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "img"
            p.write_bytes(bytes(8192) + b"filler" * 100 + ct + bytes(8192))
            with mock.patch.object(kb, "_records", return_value=stub):
                ops = kb.DiskOps(sc, {"ct": ct.hex()})
                doc, ctl = ops.analyze("D0", p, "r1", "ab" * 32)
                self.assertTrue(doc["ciphertext_found"])
                self.assertEqual(doc["scan"], {"hits": [], "controls_ok": True})
                self.assertTrue(ctl["ok"])
                self.assertEqual((doc["image"], doc["sha256"], doc["size"]), ("D0", "ab" * 32, p.stat().st_size))
                stub.evaluate_set.assert_called_once()
                p.write_bytes(bytes(8192) + b"filler" * 100 + bytes(8192))
                doc, ctl = ops.analyze("D0-before", p, "none", "cd" * 32)
                self.assertFalse(doc["ciphertext_found"])
                # a canary on the image is reported (label/form/offset only) and does not fail the controls
                p.write_bytes(bytes(8192) + b"xx" + b"correct-horse-battery-staple-9" + bytes(8192))
                doc, ctl = ops.analyze("D0", p, "r1", "ef" * 32)
                self.assertTrue(any(h["label"] == "pass:a" for h in doc["scan"]["hits"]))
                self.assertTrue(doc["scan"]["controls_ok"])
                self.assertNotIn("correct-horse", json.dumps([doc, ctl]))

    def test_disk_ops_empty_region_fails_controls(self):
        stub = mock.Mock()
        stub.find_records.return_value = {"valid": [], "invalid": [], "v1": []}
        stub.evaluate_set.return_value = {"expect": "none", "ok": True, "reasons": []}
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "img"
            p.write_bytes(bytes(8192))
            with mock.patch.object(kb, "_records", return_value=stub):
                doc, ctl = kb.DiskOps(self.scanner(), None).analyze("D0-before", p, "none", "00" * 32)
        self.assertFalse(ctl["ok"])
        self.assertFalse(doc["scan"]["controls_ok"])


class RealRecords(unittest.TestCase):
    """DiskOps against the real kssh_records.py on a small synthetic image (public test material only)."""

    def test_r1_image_with_real_oracle_and_scan(self):
        import kssh_judge
        import kssh_records as rec
        exp = {"name": "ka16", "pub": bytes(range(32)).hex(), "rounds": 16, "salt": bytes(range(16)).hex(),
               "ct": bytes((i * 7) % 251 for i in range(144)).hex()}
        r1 = rec.predict_r1(exp)
        sc = kssh_judge.Scanner([("pass:a", b"correct-horse-battery-staple-9"), ("seed:k", bytes(range(1, 33)))])
        ops = kb.DiskOps(sc, exp)
        with tempfile.TemporaryDirectory() as td:
            img = Path(td) / "img"
            img.write_bytes(b"\x11" * 4096 + bytes(65536) + b"\x00" * 5 + r1 + b"\x22" * 3000 + bytes(10000))
            doc, ctl = ops.analyze("D0", img, "r1", ops.sha256(img))
            self.assertTrue(doc["record_verdict"]["ok"], doc["record_verdict"])
            self.assertTrue(doc["ciphertext_found"])
            self.assertEqual(doc["scan"], {"hits": [], "controls_ok": True})
            self.assertEqual(len(doc["records"]["valid"]), 1)
            self.assertNotIn(exp["ct"], json.dumps([doc, ctl]))
            doc0, _ = ops.analyze("D0-before", img, "none", doc["sha256"])
            self.assertFalse(doc0["record_verdict"]["ok"])
            dst = Path(td) / "S1.img"
            ops.sparse_copy(img, dst)
            self.assertEqual(ops.sha256(dst), ops.sha256(img))
            with self.assertRaises(Exception):
                ops.sparse_copy(img, dst)


if __name__ == "__main__":
    unittest.main(verbosity=1)
