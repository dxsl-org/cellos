#!/usr/bin/env python3
"""Host tests of scripts/tier3_ssh_guest_judge.py (marker accounting, argv invariant, secret scan,
fingerprint rule, no-sshd rule, STOP classification, archive refusal). Run:
    python3 -m unittest scripts/test_tier3_ssh_guest_judge.py   (or python3 scripts/test_tier3_ssh_guest_judge.py)
"""
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tier3_ssh_guest_judge as J  # noqa: E402

FP = "SHA256:KMrVscNX8yI70XYJahx+kcv9hKzqMJE7vwcrbPSjz2s"
PORT = "40123"
PASSWORD = "Zq9pLmN2xV7bT4rY8cK1wE6u"
SEED = "ab" * 32
KEY_LINES = ["b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW",
             "QyNTUxOQAAACBzZWNyZXRzZWNyZXRzZWNyZXRzZWNyZXRzZWNyZXRzZWNyZXQAAAA="]
ARGV = ("/home/u/.cache/cellos/qemu-10.2.0/bin/qemu-system-x86_64 -machine q35 -accel tcg -cpu "
        "qemu64,+pdpe1gb,+svm,+rdrand,+rdseed -m 1G -display none -chardev socket,id=ser0,path=/tmp/s,server=on,wait=off "
        "-serial chardev:ser0 -monitor none -cdrom /tmp/i.iso -boot d -no-reboot "
        "-netdev user,id=net0,hostfwd=tcp:127.0.0.1:%s-10.0.2.16:22 -device e1000,netdev=net0 "
        "-drive file=/tmp/t.img,if=none,format=raw,id=p09fx -device nvme,drive=p09fx,serial=cellosp09\n") % PORT
SERIAL = """x86 virt: root operation active (SVM)
[hv-x86] hypervisor cell starting (SVM PVH)
[hv-x86] guest rdinit: /bin/virtio-e2e-init
[hv-x86] vCPU ready — entering run loop
[guest-uart] [    0.000000] Linux version 6.12.81-0-virt (buildozer@build) #1-Alpine
[guest-uart] TIER3_SSHD_GUEST_INIT_START
[guest-uart] TIER3_SSHD_KERNEL 6.12.81-0-virt
[guest-uart] TIER3_SSHD_CRNG_READY
[guest-uart] TIER3_SSHD_NET_UP eth0 10.0.2.16/24 gw 10.0.2.2
[guest-uart] TIER3_SSHD_APK_INSTALLED OpenSSH_9.9p2, OpenSSL 3.3.7 7 Apr 2026
[guest-uart] TIER3_SSHD_HOSTKEY 256 %s tier3-guest (ED25519)
[guest-uart] Server listening on 0.0.0.0 port 22.
[guest-uart] TIER3_SSHD_LISTENING
[guest-uart] Connection from 10.0.2.2 port 5001 on 10.0.2.16 port 22 rdomain ""
[guest-uart] Connection from 10.0.2.2 port 5002 on 10.0.2.16 port 22 rdomain ""
P04 INFO ssh-guest-ready attempts=3
""" % FP
SHA = {
    "tier3.iso": "1" * 64, "cellos-kernel": "2" * 64, "initramfs-sshd.cpio.gz": "3" * 64, "kernel_fs.img": "4" * 64,
    "vmlinux": J.PINS["vmlinux"], "vmlinuz-virt": J.PINS["vmlinuz-virt"], "initramfs-virt": J.PINS["initramfs-virt"],
    "qemu-system-x86_64": J.PINS["qemu-system-x86_64"],
}
RECLAIM_LINE = "[net] owner death: cell 7 gen 1: aborted 1 TCP, released 0 other"
WATCH_LINE = "[net] owner death watch unavailable; socket reclaim deferred"


def full_serial(skip=(), dup=(), fail=(), extra="", logins=4, accepted_after=4, selfkill=1, final="PASS checks=21", accepted_before=6,
                reclaim="in", watch_unavailable=False):
    """A passing T3-D serial log: the boot-stage lines plus the 16 reused markers, the three guest
    scenarios and the guest sshd's log lines, with knobs to break one rule at a time."""
    out = ["[ INFO] [selftest] boot-ceiling: PASS (per-path table, no union collapse)",
           "[vfs] CellosFS Native mounted at existing volume", SERIAL.rstrip("\n"), "P04 START", "P04 PASS shell-prompt"]
    if watch_unavailable:  # True: a host line; "guest": the same text on a [guest-uart] line
        out.insert(2, ("[guest-uart] " if watch_unavailable == "guest" else "") + WATCH_LINE)
    port = [50000]

    def session(ok=True):
        port[0] += 2
        out.append('[guest-uart] Connection from 10.0.2.2 port %d on 10.0.2.16 port 22 rdomain ""' % port[0])
        if ok:
            out.append("[guest-uart] Accepted password for tester from 10.0.2.2 port %d ssh2" % port[0])
            out.append("[guest-uart] Starting session: command for tester from 10.0.2.2 port %d id 0" % port[0])
        else:
            out.append("[guest-uart] Failed password for tester from 10.0.2.2 port %d ssh2" % port[0])

    for name in J.S93:
        if name == "ssh-first-trust-yes":
            for _ in range(accepted_before):
                session()
            session(ok=False)
        if name not in skip:
            out.append("P04 %s %s" % ("FAIL" if name in fail else "PASS", name))
        if name in dup:
            out.append("P04 PASS " + name)
    if reclaim == "before":
        out.append(RECLAIM_LINE)
    out.append("P04 INFO p09-case ssh-guest-cancel-producer")
    if reclaim in ("in", "zero"):
        out.append(RECLAIM_LINE if reclaim == "in" else RECLAIM_LINE.replace("aborted 1", "aborted 0"))
    for _ in range(accepted_after):
        session()
    for name in J.GUEST_MARKERS:
        if name not in skip:
            out.append("P04 %s %s" % ("FAIL" if name in fail else "PASS", name))
        if name == "ssh-guest-cancel-producer" and reclaim == "after":
            out.append(RECLAIM_LINE)
    out += ["P04 INFO ssh-guest-selfkill dispatch=accepted", "P04 INFO ssh-guest-ledger selfkill=%d lines=1" % selfkill,
            "P04 INFO ssh-guest-logins %d" % logins, "P04 PASS session-a-close", "P04-TERMINAL: " + final, extra]
    return "\n".join(l for l in out if l) + "\n"


def make_run(root: Path, serial=SERIAL, argv=ARGV, c1_ok=True, verdict="gates-satisfied", layout="/bin/hypervisor\n", full=False):
    ev, keys = root / "ev", root / "keys"
    (ev / "boot-config").mkdir(parents=True)
    keys.mkdir()
    (keys / "seed.hex").write_text(SEED)
    (keys / "password").write_text(PASSWORD + "\n")
    (keys / "host_ed25519").write_text("-----BEGIN OPENSSH PRIVATE KEY-----\n" + "\n".join(KEY_LINES) + "\n-----END OPENSSH PRIVATE KEY-----\n")
    (ev / "serial.log").write_text(serial)
    (ev / "serial.raw.log").write_bytes(serial.encode())
    (ev / "qemu-argv.txt").write_text(argv)
    (ev / "timeline.json").write_text(json.dumps({"verdict": verdict, "timeline_s": {"sshd-server-listening": 90.0, "listening": 100.0, "ssh-guest-ready": 140.0}}))
    (ev / "c1-keyscan.json").write_text(json.dumps({"ok": c1_ok, "attempts": [
        {"rc": 0, "got_fingerprint": FP if c1_ok else "", "match": c1_ok, "connected_returned_key": c1_ok}]}))
    (ev / "provenance.sha256").write_text("".join("%s  /w/%s\n" % (h, n) for n, h in SHA.items()))
    (ev / "fat-layout.txt").write_text(layout)
    (ev / "boot-config" / "autoload.toml").write_text('[[cells]]\nname = "hypervisor"\npath = "/bin/hypervisor"\n')
    if full:
        tl = json.loads((ev / "timeline.json").read_text())
        tl.update({"verdict": "driver-verdict", "window_s": 7200})
        (ev / "timeline.json").write_text(json.dumps(tl))
    (ev / "provenance-build.txt").write_text("kernel features phase02-terminal-profile,phase04-driver-profile; lane-only\n")
    (ev / "protected-before.txt").write_text("a  kernel\n")
    (ev / "protected-after.txt").write_text("a  kernel\n")
    return ev, keys


class BootJudge(unittest.TestCase):
    def judge(self, **kw):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), **kw)
            return J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head")

    def test_pass(self):
        v, stop = self.judge()
        self.assertTrue(v["pass"], [k for k, c in v["checks"].items() if not c["pass"]])
        self.assertIsNone(stop)
        self.assertEqual(v["guest_kernel"], "6.12.81-0-virt")
        self.assertEqual(v["time_to_listening_s"], 100.0)

    def failed(self, v):
        return {k for k, c in v["checks"].items() if not c["pass"]}

    def test_missing_vcpu_ready_is_stop1(self):
        v, stop = self.judge(serial=SERIAL.replace("[hv-x86] vCPU ready — entering run loop\n", ""))
        self.assertEqual(stop, "STOP-1")
        self.assertIn("B1:vcpu-ready", self.failed(v))

    def test_duplicate_vcpu_ready_fails(self):
        v, stop = self.judge(serial=SERIAL + "[hv-x86] vCPU ready — entering run loop\n")
        self.assertIn("B1:vcpu-ready", self.failed(v))
        self.assertEqual(stop, "STOP-1")

    def test_cell_fault_and_triple_fault_are_stop1(self):
        for bad in ("[fault] Cell 12 page fault\n", "KERNEL PANIC: x\n", "[hv-x86] guest triple-fault\n", "[hv-x86] create_vm failed\n"):
            v, stop = self.judge(serial=SERIAL + bad)
            self.assertEqual(stop, "STOP-1", bad)

    def test_guest_line_with_error_word_is_not_an_hv_error(self):
        v, stop = self.judge(serial=SERIAL + "[guest-uart] [ 1.0] virtio_net: error handling disabled\n")
        self.assertTrue(v["pass"])

    def test_missing_listening_is_stop2(self):
        v, stop = self.judge(serial=SERIAL.replace("[guest-uart] TIER3_SSHD_LISTENING\n", ""))
        self.assertEqual(stop, "STOP-2")
        self.assertIn("B2:listening-marker", self.failed(v))

    def test_guest_fail_marker(self):
        v, stop = self.judge(serial=SERIAL + "[guest-uart] TIER3_SSHD_FAIL: apk-add\n")
        self.assertEqual(stop, "STOP-2")

    def test_crng_timeout_fails(self):
        v, stop = self.judge(serial=SERIAL.replace("CRNG_READY", "CRNG_WAIT_TIMEOUT"))
        self.assertIn("B2:crng-ready", self.failed(v))

    def test_hostkey_must_equal_pinned(self):
        v, stop = self.judge(serial=SERIAL.replace(FP, "SHA256:" + "A" * 43))
        self.assertIn("B2:guest-hostkey-equals-pinned", self.failed(v))

    def test_c1_failure_is_stop3(self):
        v, stop = self.judge(c1_ok=False)
        self.assertEqual(stop, "STOP-3")

    def test_c2_failure_is_stop4(self):
        v, stop = self.judge(serial=SERIAL.replace("P04 INFO ssh-guest-ready attempts=3\n", "P04 INFO ssh-guest-ready never attempts=9\n"))
        self.assertEqual(stop, "STOP-4")

    def test_c2_needs_a_connection_beyond_the_keyscan(self):
        one = SERIAL.replace('[guest-uart] Connection from 10.0.2.2 port 5002 on 10.0.2.16 port 22 rdomain ""\n', "")
        v, stop = self.judge(serial=one)
        self.assertIn("C2:sshd-saw-cell-connection-beyond-keyscan", self.failed(v))
        self.assertEqual(stop, "STOP-4")

    def test_argv_invariant(self):
        for bad in (ARGV.replace("hostfwd=tcp:127.0.0.1", "hostfwd=tcp:0.0.0.0"),
                    ARGV.replace("user,id=net0,", "user,id=net0,restrict=on,"),
                    ARGV.replace("-accel tcg", "-accel kvm"),
                    ARGV.replace("-m 1G", "-m 256M"),
                    ARGV.replace("-device e1000,netdev=net0", "-device e1000,netdev=net0 -device e1000,netdev=net0"),
                    ARGV.rstrip("\n") + " -netdev user,id=n2,hostfwd=tcp:127.0.0.1:1-10.0.2.16:22\n",
                    ARGV.replace("qemu-10.2.0", "qemu-8.2.2")):
            v, _ = self.judge(argv=bad)
            self.assertIn("argv:one-loopback-hostfwd-tcg-qemu-10.2.0", self.failed(v), bad)

    def test_secret_in_serial_is_stop5(self):
        for leak in (PASSWORD, SEED, KEY_LINES[0], PASSWORD.encode().hex()):
            v, stop = self.judge(serial=SERIAL + "[guest-uart] x %s y\n" % leak)
            self.assertEqual(stop, "STOP-5", leak)

    def test_sshd_in_outer_image_fails(self):
        v, _ = self.judge(layout="/bin/hypervisor\n/bin/sshd\n")
        self.assertIn("no-sshd-in-outer-image", self.failed(v))

    def test_protected_digest_change_is_stop6(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            (ev / "protected-after.txt").write_text("b  kernel\n")
            v, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT)
            self.assertEqual(stop, "STOP-6")

    def test_pin_mismatch(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            (ev / "provenance.sha256").write_text("".join("%s  /w/%s\n" % ("0" * 64 if n == "vmlinux" else h, n) for n, h in SHA.items()))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT)
            self.assertIn("hashes:pins-match-and-iso-kernel-initramfs-recorded", self.failed(v))

    def test_ready_may_precede_the_polling_listening_marker_but_not_sshd(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            tl = json.loads((ev / "timeline.json").read_text())
            tl["timeline_s"].update({"sshd-server-listening": 90.0, "ssh-guest-ready": 95.0, "listening": 100.0})
            (ev / "timeline.json").write_text(json.dumps(tl))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT)
            self.assertTrue(v["pass"], [k for k, c in v["checks"].items() if not c["pass"]])
            tl["timeline_s"]["ssh-guest-ready"] = 80.0
            (ev / "timeline.json").write_text(json.dumps(tl))
            v, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT)
            self.assertIn("C2:ready-after-sshd-server-listening", self.failed(v))
            self.assertEqual(stop, "STOP-4")

    def test_rejudge_carries_only_the_secret_checks(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            first, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT)
            again, stop = J.judge_boot(ev, None, FP, PORT, carry=first)
            self.assertTrue(again["pass"])
            self.assertIn("carried", again["checks"]["no-secret-in-serial-raw"]["detail"])
            (ev / "serial.log").write_text(SERIAL.replace("vCPU ready", "vCPU nope"))
            again, stop = J.judge_boot(ev, None, FP, PORT, carry=first)
            self.assertEqual(stop, "STOP-1")

    def test_run_must_end_by_gates(self):
        v, _ = self.judge(verdict="window-expired")
        self.assertIn("run-ended-by-gates", self.failed(v))


class FullJudge(unittest.TestCase):
    def judge(self, serial=None, **kw):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), serial=serial if serial is not None else full_serial(), verdict="driver-verdict", full=True, **kw)
            return J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="full")

    def failed(self, v):
        return {k for k, c in v["checks"].items() if not c["pass"]}

    def test_pass(self):
        v, stop = self.judge()
        self.assertTrue(v["pass"], self.failed(v))
        self.assertIsNone(stop)
        self.assertEqual(v["stage"], "full")

    def test_missing_marker_is_marker_fail(self):
        for name in ("ssh-pty-resize", "ssh-guest-fault-may-have-run"):
            v, stop = self.judge(serial=full_serial(skip=(name,)))
            self.assertIn("D:marker:" + name, self.failed(v))
            self.assertEqual(stop, "MARKER-FAIL")

    def test_duplicate_and_fail_markers(self):
        v, stop = self.judge(serial=full_serial(dup=("ssh-exec-bytes",)))
        self.assertIn("D:marker:ssh-exec-bytes", self.failed(v))
        v, stop = self.judge(serial=full_serial(fail=("ssh-guest-cancel-producer",)))
        self.assertIn("D:no-FAIL-markers", self.failed(v))
        self.assertEqual(stop, "MARKER-FAIL")

    def test_unexpected_marker_fails(self):
        v, _ = self.judge(serial=full_serial(extra="P04 PASS ssh-fault-before-request"))
        self.assertIn("D:no-unexpected-markers", self.failed(v))

    def test_driver_final_verdict(self):
        for final in ("FAIL failures=1 checks=21", "PASS checks=20"):
            v, _ = self.judge(serial=full_serial(final=final))
            self.assertIn("D:driver-final-verdict", self.failed(v), final)

    def test_replayed_selfkill_fails(self):
        v, stop = self.judge(serial=full_serial(selfkill=2))
        self.assertIn("D:no-replay-ledger-exactly-one-selfkill", self.failed(v))
        v, _ = self.judge(serial=full_serial(selfkill=0))
        self.assertIn("D:no-replay-ledger-exactly-one-selfkill", self.failed(v))

    def test_sshd_log_accounting(self):
        v, _ = self.judge(serial=full_serial(accepted_before=5))
        self.assertIn("D:sshd-accepted-password-six-before-guest-scenarios", self.failed(v))
        v, _ = self.judge(serial=full_serial(accepted_after=3))
        self.assertIn("D:guest-scenario-logins-accounted", self.failed(v))
        v, _ = self.judge(serial=full_serial(accepted_after=5, logins=4))
        self.assertIn("D:guest-scenario-logins-accounted", self.failed(v))
        v, _ = self.judge(serial=full_serial(accepted_after=5, logins=5))  # one retried follow-up is accounted
        self.assertTrue(v["pass"], self.failed(v))

    def test_second_session_on_one_connection_fails(self):
        dup = "[guest-uart] Starting session: command for tester from 10.0.2.2 port 50002 id 1"
        v, _ = self.judge(serial=full_serial(extra=dup))
        self.assertIn("D:sshd-one-session-per-connection", self.failed(v))

    def test_boot_checks_still_apply_in_full(self):
        v, stop = self.judge(serial=full_serial().replace("[hv-x86] vCPU ready — entering run loop\n", ""))
        self.assertEqual(stop, "STOP-1")

    def test_window_cap(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), serial=full_serial(), verdict="driver-verdict", full=True)
            tl = json.loads((ev / "timeline.json").read_text()); tl["window_s"] = 20000
            (ev / "timeline.json").write_text(json.dumps(tl))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, stage="full")
            self.assertIn("window-within-4h-cap", self.failed(v))

    def test_run_must_end_by_driver_verdict(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), serial=full_serial(), verdict="window-expired", full=True)
            tl = json.loads((ev / "timeline.json").read_text()); tl["verdict"] = "window-expired"
            (ev / "timeline.json").write_text(json.dumps(tl))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, stage="full")
            self.assertIn("run-ended-by-driver-verdict", self.failed(v))

    def test_secret_in_full_log_is_stop5(self):
        v, stop = self.judge(serial=full_serial(extra="[guest-uart] x %s" % PASSWORD))
        self.assertEqual(stop, "STOP-5")

    def test_marker_set_is_21_and_includes_the_cancel_marker(self):
        self.assertEqual(len(J.EXPECTED_MARKERS), 21)
        self.assertIn("ssh-guest-cancel-producer", J.EXPECTED_MARKERS)
        self.assertEqual(len(set(J.EXPECTED_MARKERS)), 21)

    def test_cancel_marker_fail_and_missing_are_marker_fail(self):
        v, stop = self.judge(serial=full_serial(fail=("ssh-guest-cancel-producer",)))
        self.assertEqual(stop, "MARKER-FAIL")
        self.assertIn("ssh-guest-cancel-producer", v["checks"]["D:no-FAIL-markers"]["detail"])
        self.assertIn("D:marker:ssh-guest-cancel-producer", self.failed(v))
        v, stop = self.judge(serial=full_serial(skip=("ssh-guest-cancel-producer",)))
        self.assertEqual(stop, "MARKER-FAIL")
        self.assertIn("D:marker:ssh-guest-cancel-producer", self.failed(v))

    def test_terminal_must_be_pass_with_exactly_21_checks(self):
        for final in ("PASS checks=22", "PASS checks=20", "FAIL failures=1 checks=21"):
            v, stop = self.judge(serial=full_serial(final=final))
            self.assertIn("D:driver-final-verdict", self.failed(v), final)
            self.assertEqual(stop, "MARKER-FAIL")
        v, stop = self.judge(serial=full_serial(final="PASS checks=21", extra="P04-TERMINAL: PASS checks=21"))
        self.assertIn("D:driver-final-verdict", self.failed(v))  # exactly once

    def test_clean_full_run_records_the_reclaim_line(self):
        v, stop = self.judge()
        self.assertTrue(v["pass"], self.failed(v))
        self.assertEqual(v["net"]["watch_unavailable"], False)
        self.assertEqual(v["net"]["in_cancel_window"], True)
        (line,) = v["net"]["owner_death_lines"]
        self.assertEqual((line["cell"], line["gen"], line["aborted"], line["released"], line["in_cancel_window"]), (7, 1, 1, 0, True))
        self.assertGreater(line["serial_line"], 0)

    def test_reclaim_line_rules_in_the_full_stage(self):
        for how in ("none", "zero", "before", "after"):
            v, stop = self.judge(serial=full_serial(reclaim=how))
            self.assertIn("net:owner-death-reclaim-logged-in-cancel-window", self.failed(v), how)
            self.assertEqual(stop, "MARKER-FAIL", how)
            self.assertFalse(v["pass"], how)

    def test_stop7_in_the_full_stage(self):
        v, stop = self.judge(serial=full_serial(watch_unavailable=True))
        self.assertEqual(stop, "STOP-7")
        self.assertIn("net:no-owner-death-watch-unavailable", self.failed(v))
        self.assertTrue(v["net"]["watch_unavailable"])
        # even when the gates are incomplete (the line is printed before the guest is up)
        v, stop = self.judge(serial=full_serial(watch_unavailable=True).replace("[hv-x86] vCPU ready — entering run loop\n", ""))
        self.assertEqual(stop, "STOP-7")
        # a secret still wins (STOP-5 is evaluated first)
        v, stop = self.judge(serial=full_serial(watch_unavailable=True, extra="[guest-uart] x %s" % PASSWORD))
        self.assertEqual(stop, "STOP-5")

    def test_watch_text_on_a_guest_uart_line_is_not_stop7(self):
        v, stop = self.judge(serial=full_serial(watch_unavailable="guest"))
        self.assertTrue(v["pass"], self.failed(v))
        self.assertIsNone(stop)
        self.assertFalse(v["net"]["watch_unavailable"])

    def test_boot_stage_has_no_net_checks(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head")
        self.assertFalse([k for k in v["checks"] if k.startswith("net:")])
        self.assertNotIn("net", v)


# ---- T3-D2 diagnostic stage -------------------------------------------------------------------

DIAG_ARGV = ARGV.rstrip("\n") + " -object filter-dump,id=fd0,netdev=net0,file=/tmp/cellbox-tier3-261030a/net0.pcap\n"
T_START = 1000.0
T_CANCEL = 1100.0  # wall time of the cancel-done mark


def tcp_frame(src, dst, sport, dport, flags, win=1000, plen=0, vlan=False):
    """An Ethernet/IPv4/TCP frame (payload bytes zeroed)."""
    ip = lambda s: bytes(int(x) for x in s.split("."))
    tcp = struct.pack(">HHIIBBHHH", sport, dport, 1, 1, 5 << 4, flags, win, 0, 0) + bytes(plen)
    iph = struct.pack(">BBHHHBBH", 0x45, 0, 20 + len(tcp), 0, 0, 64, 6, 0) + ip(src) + ip(dst)
    eth = b"\x02" * 6 + b"\x04" * 6 + (b"\x81\x00\x00\x01" if vlan else b"") + b"\x08\x00"
    return eth + iph + tcp


def pcap_file(path, pkts, magic=b"\xd4\xc3\xb2\xa1", nano=False):
    """pkts: (time, frame). Writes a classic little-endian libpcap file."""
    with open(path, "wb") as fh:
        fh.write(magic + struct.pack("<HHiIII", 2, 4, 0, 0, 65535, 1))
        for t, frame in pkts:
            sec = int(t)
            frac = int(round((t - sec) * (1e9 if nano else 1e6)))
            fh.write(struct.pack("<IIII", sec, frac, len(frame), len(frame)) + frame)


SYN, FIN, RST, ACK, PSH = 0x02, 0x01, 0x04, 0x10, 0x08
CELL, SLIRP, GUEST = "10.0.2.15", "10.0.2.2", "10.0.2.16"


def conn_pkts(t, eph, gport, fin_cell=None, fin_slirp=None, rst_cell=None, zero_after=None, probes=()):
    """One cooperative-or-not connection on both flows: A cell:eph -> slirp:PORT, B slirp:gport -> guest:22.
    fin_*: time of a FIN; zero_after: time of a zero-window ACK from the cell; probes: slirp->cell 1-byte probes."""
    port = int(PORT)
    p = [(t, tcp_frame(CELL, SLIRP, eph, port, SYN)), (t + .01, tcp_frame(SLIRP, CELL, port, eph, SYN | ACK)),
         (t + .02, tcp_frame(CELL, SLIRP, eph, port, ACK)), (t + .2, tcp_frame(SLIRP, GUEST, gport, 22, SYN)),
         (t + .21, tcp_frame(GUEST, SLIRP, 22, gport, SYN | ACK)), (t + .3, tcp_frame(CELL, SLIRP, eph, port, ACK | PSH, plen=100)),
         (t + .4, tcp_frame(SLIRP, CELL, port, eph, ACK, win=4096, plen=300)), (t + .5, tcp_frame(GUEST, SLIRP, 22, gport, ACK | PSH, plen=50))]
    if fin_cell is not None:
        p += [(fin_cell, tcp_frame(CELL, SLIRP, eph, port, FIN | ACK))]
    if fin_slirp is not None:
        p += [(fin_slirp, tcp_frame(SLIRP, GUEST, gport, 22, FIN | ACK)), (fin_slirp + .05, tcp_frame(GUEST, SLIRP, 22, gport, FIN | ACK))]
    if rst_cell is not None:
        p += [(rst_cell, tcp_frame(CELL, SLIRP, eph, port, RST | ACK))]
    for pt in probes:
        p.append((pt, tcp_frame(SLIRP, CELL, port, eph, ACK, plen=1)))
    if zero_after is not None:
        p.append((zero_after, tcp_frame(CELL, SLIRP, eph, port, ACK, win=0)))
    return p


class PcapSummary(unittest.TestCase):
    PORTS = {"control": 5500, "cancel": 5600}
    MARKS = {"cancel-done": T_CANCEL}

    def summarize(self, pkts, **kw):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            pcap_file(p, sorted(pkts, key=lambda x: x[0]), **kw)
            return J.summarize_pcap(p, int(PORT), self.PORTS, self.MARKS, T_START)

    def test_leaked_connection_has_no_fin_rst_and_zero_window_acks(self):
        pkts = (conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1)
                + conn_pkts(1090.0, 49200, 5600, zero_after=1105.0, probes=(1104.9, 1165.0)))
        s = self.summarize(pkts)
        d = s["derived"]
        self.assertIs(d["cancel_A_fin_rst_from_cell"], False)
        self.assertIs(d["cancel_B_fin_rst_from_slirp"], False)
        self.assertIs(d["control_A_fin_rst_from_cell"], True)
        self.assertIs(d["control_B_fin_rst_from_slirp"], True)
        self.assertEqual(d["cancel_A_zero_window_from_cell_after_mark"], 1)
        self.assertEqual(s["cancel"]["A"]["client"], "10.0.2.15:49200")
        self.assertEqual(s["cancel"]["B"]["client"], "10.0.2.2:5600")
        self.assertEqual(s["cancel"]["A_pairing"], "paired-by-syn-time")
        self.assertEqual(s["control"]["A"]["client"], "10.0.2.15:49100")
        after = s["cancel"]["A"]["after_cancel"]
        self.assertIn([5.0, "c2s", "A", 0, 0], after)       # the zero-window ack, 5 s after the mark
        self.assertEqual(s["cancel"]["A"]["first_fin"], None)
        self.assertEqual(s["control"]["A"]["first_fin"], {"t": 58.0, "dir": "c2s"})
        self.assertEqual(s["pcap"]["linktype"], 1)
        self.assertNotIn("after_cancel", s["connections"]["A"][0])

    def test_rst_from_the_cell_after_cancel_is_seen(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1)
                           + conn_pkts(1090.0, 49200, 5600, rst_cell=1120.0, fin_slirp=1120.1))
        self.assertIs(s["derived"]["cancel_A_fin_rst_from_cell"], True)
        self.assertIs(s["derived"]["cancel_B_fin_rst_from_slirp"], True)
        self.assertEqual(s["cancel"]["A"]["first_rst"], {"t": 120.0, "dir": "c2s"})

    def test_fin_from_cell_not_forwarded_to_guest(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0) + conn_pkts(1090.0, 49200, 5600))
        self.assertIs(s["derived"]["control_A_fin_rst_from_cell"], True)
        self.assertIs(s["derived"]["control_B_fin_rst_from_slirp"], False)

    def test_unidentified_connections_give_none_and_the_a_fallback_is_named(self):
        pkts = conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1)
        s = self.summarize(pkts)  # no connection with the cancel port: the last A SYN before the mark is used
        self.assertEqual(s["cancel"]["A_pairing"], "last-A-syn-before-the-cancel-mark")
        self.assertIsNone(s["derived"]["cancel_B_fin_rst_from_slirp"])
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            pcap_file(p, sorted(pkts, key=lambda x: x[0]))
            s = J.summarize_pcap(p, int(PORT), self.PORTS, {"cancel-done": 1000.0}, T_START)
        self.assertIsNone(s["derived"]["cancel_A_fin_rst_from_cell"])

    def test_port_reuse_after_close_starts_a_new_connection(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1) + conn_pkts(1070.0, 49100, 5501)
                           + conn_pkts(1090.0, 49200, 5600))
        self.assertEqual([c["client"] for c in s["connections"]["A"]], ["10.0.2.15:49100", "10.0.2.15:49100", "10.0.2.15:49200"])

    def test_other_traffic_and_vlan_and_nanosecond_pcap(self):
        junk = [(1001.0, b"\x00" * 10), (1001.1, tcp_frame("10.0.2.15", "10.0.2.3", 1, 2, SYN)),
                (1001.2, tcp_frame(CELL, SLIRP, 7, 8, SYN)), (1001.3, tcp_frame(SLIRP, GUEST, 9, 22, SYN, vlan=True))]
        s = self.summarize(junk + conn_pkts(1090.0, 49200, 5600), magic=b"\x4d\x3c\xb2\xa1", nano=True)
        self.assertEqual(s["pcap"]["packets"], len(junk) + 8)
        self.assertEqual([c["client"] for c in s["connections"]["B"]], ["10.0.2.2:9", "10.0.2.2:5600"])
        self.assertEqual(len(s["connections"]["A"]), 1)

    def test_bad_pcap_is_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            p.write_bytes(b"not a pcap file at all, but long enough")
            with self.assertRaises(ValueError):
                J.summarize_pcap(p, int(PORT), self.PORTS, self.MARKS, T_START)
            p.write_bytes(b"\xd4\xc3\xb2\xa1")
            with self.assertRaises(ValueError):
                J.summarize_pcap(p, int(PORT), self.PORTS, self.MARKS, T_START)

    def test_local_time_shifted_stamps_are_calibrated(self):
        base = 100000.0  # keeps every shifted stamp non-negative
        pkts = (conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1)
                + conn_pkts(1090.0, 49200, 5600, zero_after=1105.0, probes=(1104.9, 1165.0)))
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            # stamps shifted by the host UTC offset (+07:00 -> -25200 s), as QEMU 10.2.0's filter-dump wrote them
            pcap_file(p, sorted([(t + base - 25200.0, f) for t, f in pkts], key=lambda x: x[0]))
            s = J.summarize_pcap(p, int(PORT), self.PORTS, {"cancel-done": T_CANCEL + base}, T_START + base)
        self.assertEqual(s["pcap"]["clock_offset_s"], -25200.0)
        self.assertEqual(s["control"]["A"]["first_fin"], {"t": 58.0, "dir": "c2s"})
        self.assertIn([5.0, "c2s", "A", 0, 0], s["cancel"]["A"]["after_cancel"])
        self.assertEqual(s["derived"]["cancel_A_zero_window_from_cell_after_mark"], 1)
        self.assertEqual(s["pcap"]["first_t"], 50.0)

    def test_after_cancel_list_is_bounded(self):
        flood = [(T_CANCEL + i * .001, tcp_frame(CELL, SLIRP, 49200, int(PORT), ACK, win=0)) for i in range(50)]
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            pcap_file(p, sorted(conn_pkts(1090.0, 49200, 5600) + flood, key=lambda x: x[0]))
            s = J.summarize_pcap(p, int(PORT), self.PORTS, self.MARKS, T_START, max_after=10)
        self.assertEqual(len(s["cancel"]["A"]["after_cancel"]), 10)
        self.assertTrue(s["cancel"]["A"]["after_cancel_truncated"])

    def test_cell_rst_sets_rst_and_not_fin(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, rst_cell=1058.0)
                           + conn_pkts(1090.0, 49200, 5600, rst_cell=1100.5, fin_slirp=1100.6))
        d = s["derived"]
        self.assertIs(d["cancel_A_rst_from_cell"], True)
        self.assertIs(d["cancel_A_fin_from_cell"], False)
        self.assertEqual(d["cancel_A_zero_window_from_cell_after_first_rst"], 0)
        self.assertIs(d["cancel_B_fin_rst_from_slirp"], True)
        self.assertIs(d["control_A_rst_from_cell"], True)
        self.assertIs(d["control_A_fin_from_cell"], False)
        self.assertEqual(d["control_A_zero_window_from_cell_after_first_rst"], 0)
        self.assertEqual(s["cancel"]["A"]["first_rst"], {"t": 100.5, "dir": "c2s"})

    def test_no_rst_and_fin_only(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1)
                           + conn_pkts(1090.0, 49200, 5600, fin_cell=1120.0, zero_after=1105.0))
        d = s["derived"]
        self.assertIs(d["cancel_A_rst_from_cell"], False)
        self.assertIs(d["cancel_A_fin_from_cell"], True)
        self.assertEqual(d["cancel_A_zero_window_from_cell_after_first_rst"], 0)  # no RST: nothing is "after" it
        self.assertIs(d["control_A_fin_from_cell"], True)
        self.assertIs(d["control_A_rst_from_cell"], False)

    def test_fin_and_rst_set_both(self):
        s = self.summarize(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0) + conn_pkts(1090.0, 49200, 5600, fin_cell=1100.4, rst_cell=1100.5))
        self.assertIs(s["derived"]["cancel_A_rst_from_cell"], True)
        self.assertIs(s["derived"]["cancel_A_fin_from_cell"], True)

    def test_zero_window_ack_counts_only_after_the_first_rst(self):
        def zero(t):
            return (t, tcp_frame(CELL, SLIRP, 49200, int(PORT), ACK, win=0))
        base = conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0, fin_slirp=1058.1) + conn_pkts(1090.0, 49200, 5600, rst_cell=1100.5, fin_slirp=1100.6)
        key = "cancel_A_zero_window_from_cell_after_first_rst"
        self.assertEqual(self.summarize(base + [zero(1095.0)])["derived"][key], 0)
        self.assertEqual(self.summarize(base + [zero(1105.0)])["derived"][key], 1)
        self.assertEqual(self.summarize(base + [zero(1095.0), zero(1105.0), zero(1106.0)])["derived"][key], 2)

    def test_a_rst_from_slirp_also_starts_the_after_rst_count(self):
        pk = conn_pkts(1090.0, 49200, 5600) + [(1100.5, tcp_frame(SLIRP, CELL, int(PORT), 49200, RST | ACK)),
                                               (1105.0, tcp_frame(CELL, SLIRP, 49200, int(PORT), ACK, win=0))]
        d = self.summarize(pk)["derived"]
        self.assertIs(d["cancel_A_rst_from_cell"], False)
        self.assertEqual(d["cancel_A_zero_window_from_cell_after_first_rst"], 1)

    def test_unidentified_cancel_connection_gives_none_for_the_new_keys(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "n.pcap"
            pcap_file(p, sorted(conn_pkts(1050.0, 49100, 5500, fin_cell=1058.0), key=lambda x: x[0]))
            s = J.summarize_pcap(p, int(PORT), self.PORTS, {"cancel-done": 1000.0}, T_START)
        for k in ("cancel_A_rst_from_cell", "cancel_A_fin_from_cell", "cancel_A_zero_window_from_cell_after_first_rst"):
            self.assertIsNone(s["derived"][k], k)


class SnapParsing(unittest.TestCase):
    def test_parse_snap(self):
        s = J.parse_snap_lines(["up=99.5", "ssh_conn=10.0.2.2 5700 10.0.2.16 22", "pidfile=497", "proc 497 (yes) S 496",
                                "proc 100 (sshd) S 1", "tcp 1002000A:0016 0202000A:15E0 st=01 q=00000010:00000000",
                                "tcp 00000000:0016 00000000:0000 st=0A q=00000000:00000000", "diag pid=497 ppid=496"])
        self.assertEqual(s["up"], "99.5")
        self.assertEqual(s["tcp"][0]["remote"], "10.0.2.2:5600")
        self.assertEqual((s["tcp"][0]["state"], s["tcp"][0]["tx"]), ("ESTABLISHED", 16))
        self.assertEqual(s["tcp"][1]["state"], "LISTEN")
        self.assertTrue(J.yes_alive(s))
        s["procs"][0]["state"] = "Z"
        self.assertFalse(J.yes_alive(s))

    def test_ss_parse(self):
        t = ("# name=x wall=1 offset_s=1\n# cmd=ss\n# rc=0\nState Recv-Q Send-Q Local Address:Port Peer Address:Port\n"
             "ESTAB 0 0 127.0.0.1:41000 127.0.0.1:40123\n\t cubic wscale:7,7 rto:204\nESTAB 0 0 127.0.0.1:40123 127.0.0.1:41000\n"
             "TIME-WAIT 0 0 127.0.0.1:40123 127.0.0.1:41001\n")
        p = J.parse_ss_file(t)
        self.assertEqual((p["rc"], p["states"]), (0, {"ESTAB": 2, "TIME-WAIT": 1}))


def diag_data(**kw):
    d = {"env_failure": None, "control_ok": True, "control_timed_out": True, "control_gone": True, "control_closed_logged": True,
         "control_producer_started": True, "control_producer_stopped_at_snap_a": True,
         "cancel_marker_pass": False, "net_fin_rst_after_cancel": False, "slirp_fin_rst_to_guest_after_cancel": False}
    for x in "abc":
        d.update({"cancel_conn_state_at_snap_" + x: "ESTABLISHED", "yes_alive_at_snap_" + x: True, "host_ss_established_at_" + x: 2})
    d.update(kw)
    return d


O7_KEYS = ("cancel_A_rst_from_cell", "cancel_A_fin_from_cell", "cancel_A_zero_window_from_cell_after_first_rst",
           "net_owner_death_in_cancel_window", "net_watch_unavailable")


def o7_data(**kw):
    """Observed data of a passing O7 ("reclaimed") diag run."""
    d = diag_data(cancel_marker_pass=True, net_fin_rst_after_cancel=True, slirp_fin_rst_to_guest_after_cancel=True,
                  cancel_A_rst_from_cell=True, cancel_A_fin_from_cell=False, cancel_A_zero_window_from_cell_after_first_rst=0,
                  net_owner_death_in_cancel_window=True, net_watch_unavailable=False)
    for x in "abc":
        d.update({"cancel_conn_state_at_snap_" + x: "absent", "yes_alive_at_snap_" + x: False, "yes_pid_known_at_snap_" + x: True,
                  "host_ss_established_at_" + x: 0})
    d.update(kw)
    return d


class Classify(unittest.TestCase):
    def label(self, **kw):
        return J.classify(diag_data(**kw))[0]

    def test_o1(self):
        self.assertEqual(self.label(), "O1")

    def test_o1_needs_every_observation(self):
        for kw in ({"control_closed_logged": False}, {"net_fin_rst_after_cancel": True}, {"net_fin_rst_after_cancel": None},
                   {"slirp_fin_rst_to_guest_after_cancel": True}, {"yes_alive_at_snap_b": False}, {"host_ss_established_at_c": 1},
                   {"cancel_conn_state_at_snap_c": "absent"}, {"cancel_marker_pass": True}, {"control_producer_started": False},
                   {"control_producer_stopped_at_snap_a": False}, {"control_producer_started": None}):
            self.assertNotEqual(self.label(**kw), "O1", kw)

    def test_o2_o3(self):
        self.assertEqual(self.label(control_ok=False, control_gone=False, control_closed_logged=True), "O2")
        self.assertEqual(self.label(control_ok=False, control_gone=False, control_closed_logged=False), "O3")

    def test_o4_delayed_reclaim(self):
        self.assertEqual(self.label(cancel_conn_state_at_snap_c="absent", yes_alive_at_snap_c=False), "O4")
        self.assertEqual(self.label(cancel_conn_state_at_snap_b="absent", cancel_conn_state_at_snap_c="absent"), "O4")

    def test_o5_and_o6(self):
        self.assertEqual(self.label(env_failure="ss missing"), "O5")
        self.assertEqual(self.label(cancel_conn_state_at_snap_a="absent", yes_alive_at_snap_a=False, cancel_marker_pass=True), "O6")
        self.assertEqual(self.label(control_ok=False, control_timed_out=False, control_gone=False), "O6")


class ClassifyO7(unittest.TestCase):
    def test_o7_positive(self):
        self.assertEqual(J.classify(o7_data())[0], "O7")
        # the connection may be in a closing state at snap-a/b as long as it is not ESTABLISHED/CLOSE_WAIT, and is absent at snap-c
        self.assertEqual(J.classify(o7_data(cancel_conn_state_at_snap_a="TIME_WAIT", cancel_conn_state_at_snap_b="FIN_WAIT2"))[0], "O7")

    def test_each_row_alone_prevents_o7(self):
        flips = [{"control_ok": False}, {"control_closed_logged": False}, {"control_closed_logged": None},
                 {"control_producer_started": False}, {"control_producer_started": None},
                 {"control_producer_stopped_at_snap_a": False}, {"control_producer_stopped_at_snap_a": None},
                 {"cancel_marker_pass": False},
                 {"cancel_A_rst_from_cell": False}, {"cancel_A_rst_from_cell": None},
                 {"cancel_A_fin_from_cell": True}, {"cancel_A_fin_from_cell": None},
                 {"cancel_A_zero_window_from_cell_after_first_rst": 1}, {"cancel_A_zero_window_from_cell_after_first_rst": None},
                 {"cancel_A_zero_window_from_cell_after_first_rst": False},
                 {"slirp_fin_rst_to_guest_after_cancel": False}, {"slirp_fin_rst_to_guest_after_cancel": None},
                 {"cancel_conn_state_at_snap_c": "TIME_WAIT"}, {"cancel_conn_state_at_snap_c": None}, {"cancel_conn_state_at_snap_a": None},
                 {"env_failure": "ss missing"},
                 {"net_owner_death_in_cancel_window": False}, {"net_owner_death_in_cancel_window": None},
                 {"net_watch_unavailable": True}, {"net_watch_unavailable": None}]
        for x in "abc":
            flips += [{"cancel_conn_state_at_snap_" + x: "ESTABLISHED"}, {"cancel_conn_state_at_snap_" + x: "CLOSE_WAIT"},
                      {"yes_alive_at_snap_" + x: True}, {"yes_alive_at_snap_" + x: None}, {"yes_pid_known_at_snap_" + x: False},
                      {"host_ss_established_at_" + x: 1}, {"host_ss_established_at_" + x: None}]
        for kw in flips:
            self.assertNotEqual(J.classify(o7_data(**kw))[0], "O7", kw)

    def test_data_without_the_new_keys_is_never_o7(self):
        for k in O7_KEYS + ("yes_pid_known_at_snap_a",):
            d = o7_data()
            del d[k]
            self.assertNotEqual(J.classify(d)[0], "O7", k)
        self.assertEqual(J.classify(diag_data())[0], "O1")  # the historical O1 fixture is unchanged

    def test_o4_shape_is_not_o7(self):
        d = o7_data(cancel_conn_state_at_snap_a="ESTABLISHED", yes_alive_at_snap_a=True, host_ss_established_at_a=2)
        self.assertEqual(J.classify(d)[0], "O4")


class DiagRun:
    """A consistent synthetic T3-D2 evidence directory: serial log, host ss files, marks, pcap."""

    def __init__(self, control="pass", control_closed=True, cancel="fail", snap_states=("01", "01", "01"), yes=(True, True, True),
                 cancel_fin_cell=False, ss_estab=(2, 2, 2), extra="", control_started=True, control_stopped=True, pcap_shift=0.0,
                 reclaim="in", watch_unavailable=False, cancel_rst_cell=False, cancel_fin_slirp=False, zero_ack=1105.0, zero_before_rst=False):
        self.kw = dict(control=control, control_closed=control_closed, cancel=cancel, snap_states=snap_states, yes=yes,
                       cancel_fin_cell=cancel_fin_cell, ss_estab=ss_estab, extra=extra, control_started=control_started,
                       control_stopped=control_stopped, pcap_shift=pcap_shift, reclaim=reclaim, watch_unavailable=watch_unavailable,
                       cancel_rst_cell=cancel_rst_cell, cancel_fin_slirp=cancel_fin_slirp, zero_ack=zero_ack, zero_before_rst=zero_before_rst)

    def serial(self):
        k = self.kw
        out = ["[ INFO] [selftest] boot-ceiling: PASS (per-path table, no union collapse)",
               "[vfs] CellosFS Native mounted at existing volume", SERIAL.rstrip("\n"), "P04 START", "P04 PASS shell-prompt"]
        if k["watch_unavailable"]:
            out.insert(2, ("[guest-uart] " if k["watch_unavailable"] == "guest" else "") + WATCH_LINE)
        port = [50000]

        def session(ok=True):
            port[0] += 2
            out.append('[guest-uart] Connection from 10.0.2.2 port %d on 10.0.2.16 port 22 rdomain ""' % port[0])
            out.append(("[guest-uart] Accepted password for tester from 10.0.2.2 port %d ssh2" % port[0]) if ok else
                       ("[guest-uart] Failed password for tester from 10.0.2.2 port %d ssh2" % port[0]))
            if ok:
                out.append("[guest-uart] Starting session: command for tester from 10.0.2.2 port %d id 0" % port[0])
            return port[0]

        for name in J.S93:
            if name == "ssh-first-trust-yes":
                for _ in range(6):
                    session()
                session(ok=False)
            out.append("P04 PASS " + name)
        out.append("P04 INFO p09-case ssh-guest-consumer-close-producer")
        self.control_port = session()
        if k["control_closed"]:
            out.append("[guest-uart] Connection closed by 10.0.2.2 port %d" % self.control_port)
        if k["control"] == "pass":
            out.append("P04 PASS ssh-guest-consumer-close-producer")
        else:
            out.append("P04 FAIL ssh-guest-consumer-close-producer timed_out=true gone_ok=false")
        session()  # the control's `p9 gone`
        if k["reclaim"] == "before":
            out.append(RECLAIM_LINE)
        out.append("P04 INFO p09-case ssh-guest-cancel-producer")
        if k["reclaim"] in ("in", "zero"):
            out.append(RECLAIM_LINE if k["reclaim"] == "in" else RECLAIM_LINE.replace("aborted 1", "aborted 0"))
        self.cancel_port = session()
        out.append("P04 INFO ssh-guest-diag-mark cancel-done off_ms=0")
        for i, x in enumerate("abc"):
            out.append("P04 INFO ssh-guest-diag-mark snap-%s off_ms=%d" % (x, (3000, 25000, 70000)[i]))
            own = session()
            st = k["snap_states"][i]
            out += ["P04 INFO ssh-guest-snap snap-%s st=st=0 ms=3000" % x,
                    "P04 INFO ssh-guest-snap snap-%s | up=%d.5" % (x, 200 + i),
                    "P04 INFO ssh-guest-snap snap-%s | ssh_conn=10.0.2.2 %d 10.0.2.16 22" % (x, own),
                    "P04 INFO ssh-guest-snap snap-%s | pidfile=497" % x,
                    "P04 INFO ssh-guest-snap snap-%s | proc 100 (sshd) S 1" % x]
            if k["control_started"]:
                out.append("P04 INFO ssh-guest-snap snap-%s | diag pid=490 ppid=489" % x)
            out.append("P04 INFO ssh-guest-snap snap-%s | diag pid=497 ppid=496" % x)
            if not k["control_stopped"]:
                out.append("P04 INFO ssh-guest-snap snap-%s | proc 490 (yes) S 489" % x)
            if k["yes"][i]:
                out.append("P04 INFO ssh-guest-snap snap-%s | proc 497 (yes) S 496" % x)
            out.append("P04 INFO ssh-guest-snap snap-%s | tcp 1002000A:0016 0202000A:%04X st=01 q=00000000:00000000" % (x, own))
            if st is not None:
                out.append("P04 INFO ssh-guest-snap snap-%s | tcp 1002000A:0016 0202000A:%04X st=%s q=00000000:00000000" % (x, self.cancel_port, st))
            out.append("P04 INFO ssh-guest-snap snap-%s end" % x)
            if x == "b":
                session()  # `p9 gone`
                out.append("P04 PASS ssh-guest-cancel-producer" if k["cancel"] == "pass" else
                           "P04 FAIL ssh-guest-cancel-producer cancelled=true gone_ok=false")
                if k["reclaim"] == "after":
                    out.append(RECLAIM_LINE)
        session()
        out.append("P04 PASS ssh-guest-fault-may-have-run")
        session()
        fails = (k["control"] != "pass") + (k["cancel"] != "pass")
        out += ["P04 INFO ssh-guest-selfkill dispatch=accepted", "P04 INFO ssh-guest-ledger selfkill=1 lines=1",
                "P04 INFO ssh-guest-logins 12", "P04 PASS ssh-guest-no-replay-ledger", "P04 PASS session-a-close",
                "P04-TERMINAL: " + ("PASS checks=22" if not fails else "FAIL failures=%d checks=22" % fails), k["extra"]]
        return "\n".join(l for l in out if l) + "\n"

    def write(self, root, argv=DIAG_ARGV, window_s=3600, elapsed_s=400.0):
        serial = self.serial()
        ev, keys = make_run(root, serial=serial, argv=argv, verdict="driver-verdict", full=True)
        tl = json.loads((ev / "timeline.json").read_text())
        tl.update({"window_s": window_s, "elapsed_s": elapsed_s})
        (ev / "timeline.json").write_text(json.dumps(tl))
        k = self.kw
        names = J.DIAG_SS_NAMES
        marks = {}
        for i, n in enumerate(names):
            wall = {"cancel-done": T_CANCEL, "snap-a": T_CANCEL + 5, "snap-b": T_CANCEL + 27, "snap-c": T_CANCEL + 95}.get(n, T_CANCEL + 100 + i)
            marks[n] = {"wall": wall, "off_s": wall - T_START, "ss_rc": 0}
            estab = {"snap-a": k["ss_estab"][0], "snap-b": k["ss_estab"][1], "snap-c": k["ss_estab"][2]}.get(n, 2)
            rows = "".join("ESTAB 0 0 127.0.0.1:%d 127.0.0.1:%d\n" % (41000 + j, 40123) for j in range(estab))
            (ev / ("host-ss-%s.txt" % n)).write_text("# name=%s wall=%s offset_s=1\n# cmd=ss\n# rc=0\nState Recv-Q Send-Q\n%s" % (n, wall, rows))
        (ev / "diag-marks.json").write_text(json.dumps({"qemu_start_wall": T_START, "marks": marks}))
        cancel_pk = conn_pkts(1090.0, 49200, self.cancel_port, fin_cell=1120.0 if k["cancel_fin_cell"] else None,
                              fin_slirp=1100.6 if k["cancel_fin_slirp"] else None, rst_cell=1100.5 if k["cancel_rst_cell"] else None,
                              zero_after=k["zero_ack"])
        if k["zero_before_rst"]:
            cancel_pk.append((1095.0, tcp_frame(CELL, SLIRP, 49200, int(PORT), ACK, win=0)))
        pk = (conn_pkts(1050.0, 49100, self.control_port, fin_cell=1058.0 if k["control_closed"] else None,
                        fin_slirp=1058.1 if k["control_closed"] else None) + cancel_pk)
        pcap = Path(root) / "net0.pcap"
        pcap_file(pcap, sorted([(t + k["pcap_shift"], f) for t, f in pk], key=lambda x: x[0]))
        J.prep_diag(ev, pcap, J.secrets_from(keys), PORT)
        return ev, keys, pcap


def reclaimed(**kw):
    """A synthetic O7 run: the cancel marker PASSes, the cell resets flow A (no FIN, no zero-window ACK after the RST), slirp
    ends flow B, the guest connection is gone at every snapshot, `yes` is gone, host ss has no ESTAB, the Net cell logs the reclaim."""
    base = dict(cancel="pass", snap_states=(None, None, None), yes=(False, False, False), ss_estab=(0, 0, 0),
                cancel_rst_cell=True, cancel_fin_slirp=True, zero_ack=None)
    base.update(kw)
    return DiagRun(**base)


class DiagJudge(unittest.TestCase):
    def run_diag(self, run=None, expect="", **kw):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = (run or DiagRun()).write(Path(d), **kw)
            return J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag", expect_outcome=expect)

    def failed(self, v):
        return {k for k, c in v["checks"].items() if not c["pass"]}

    def test_o1_run_passes_with_the_cancel_marker_recorded_as_fail(self):
        v, stop = self.run_diag()
        self.assertTrue(v["pass"], self.failed(v))
        self.assertIsNone(stop)
        dg = v["diag"]
        self.assertEqual(dg["outcome"], "O1", dg["outcome_why"])
        self.assertEqual(dg["cancel_conn_state_at_snap_c"], "ESTABLISHED")
        self.assertIs(dg["net_fin_rst_after_cancel"], False)
        self.assertEqual(dg["host_ss_established_at_a"], 2)
        self.assertEqual(dg["yes_ppid_at_snap_a"], "496")
        self.assertEqual(v["stage"], "diag")

    def test_cancel_marker_pass_is_also_accepted_as_an_observation(self):
        v, stop = self.run_diag(DiagRun(cancel="pass", snap_states=("01", "01", None), yes=(True, True, False)))
        self.assertTrue(v["pass"], self.failed(v))
        self.assertEqual(v["diag"]["outcome"], "O4")

    def test_o4_when_the_cell_closes_late(self):
        v, _ = self.run_diag(DiagRun(snap_states=("01", "01", None), yes=(True, True, False), cancel_fin_cell=True, ss_estab=(2, 2, 0)))
        self.assertEqual(v["diag"]["outcome"], "O4")

    def test_failed_control_is_a_claim_failure_and_o2_or_o3(self):
        v, stop = self.run_diag(DiagRun(control="fail"))
        self.assertIn("D:marker:ssh-guest-consumer-close-producer", self.failed(v))
        self.assertEqual(stop, "MARKER-FAIL")
        self.assertEqual(v["diag"]["outcome"], "O2")
        v, _ = self.run_diag(DiagRun(control="fail", control_closed=False))
        self.assertEqual(v["diag"]["outcome"], "O3")

    def test_other_marker_fail_is_a_claim_failure(self):
        r = DiagRun(extra="P04 FAIL ssh-pty-resize x")
        v, stop = self.run_diag(r)
        self.assertIn("D:no-FAIL-markers", self.failed(v))
        self.assertEqual(stop, "MARKER-FAIL")

    def test_observation_must_be_recorded_exactly_once(self):
        v, _ = self.run_diag(DiagRun(extra="P04 PASS ssh-guest-cancel-producer"))
        self.assertIn("D:marker:ssh-guest-cancel-producer", self.failed(v))

    def test_final_verdict_must_count_the_observation_failure(self):
        v, _ = self.run_diag(DiagRun(extra="P04-TERMINAL: PASS checks=22"))
        self.assertIn("D:driver-final-verdict", self.failed(v))

    def test_missing_snapshot_is_o6_and_a_failed_check(self):
        r = DiagRun()
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = r.write(Path(d))
            log = (ev / "serial.log").read_text().replace("snap-c end", "snap-c gone")
            (ev / "serial.log").write_text(log)
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
        self.assertIn("diag:three-snaps-present", self.failed(v))
        self.assertEqual(v["diag"]["outcome"], "O6")

    def test_missing_ss_file_and_missing_pcap_summary(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = DiagRun().write(Path(d))
            (ev / "host-ss-after-kill.txt").unlink()
            (ev / "pcap-summary.json").write_text(json.dumps({"error": "pcap missing or empty"}))
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
        self.assertIn("diag:host-ss-files-present-nonempty", self.failed(v))
        self.assertIn("diag:pcap-summary-present", self.failed(v))
        self.assertEqual(v["diag"]["outcome"], "O5")

    def test_scenarios_never_started_is_o5(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = DiagRun().write(Path(d))
            log = (ev / "serial.log").read_text().replace("P04 INFO p09-case ssh-guest-consumer-close-producer\n", "")
            (ev / "serial.log").write_text(log)
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
        self.assertEqual(v["diag"]["outcome"], "O5")

    def test_argv_allows_exactly_the_one_filter_dump(self):
        for argv, stop in ((ARGV, "STOP-D3"),
                           (DIAG_ARGV.rstrip("\n") + " -object secret,id=x\n", "STOP-D3"),
                           (DIAG_ARGV.rstrip("\n") + " -object filter-dump,id=fd1,netdev=net0,file=/tmp/cellbox-tier3-261030a/net0.pcap\n", "STOP-D3"),
                           (DIAG_ARGV.replace("file=/tmp/cellbox-tier3-261030a/net0.pcap", "file=/etc/net0.pcap"), "STOP-D3"),
                           (DIAG_ARGV.replace("-monitor none", "-monitor stdio"), "STOP-D3")):
            v, s = self.run_diag(argv=argv)
            self.assertEqual(s, stop, argv)
            self.assertIn("argv:one-loopback-hostfwd-tcg-qemu-10.2.0", self.failed(v))
        v, s = self.run_diag()
        self.assertNotIn("argv:one-loopback-hostfwd-tcg-qemu-10.2.0", self.failed(v))

    def test_full_stage_refuses_any_object_argument(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), serial=full_serial(), argv=DIAG_ARGV, verdict="driver-verdict", full=True)
            v, _ = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, stage="full")
        self.assertIn("argv:one-loopback-hostfwd-tcg-qemu-10.2.0", self.failed(v))

    def test_window_cap_is_one_hour(self):
        v, stop = self.run_diag(window_s=7200)
        self.assertIn("diag:window-within-1h-cap", self.failed(v))
        self.assertEqual(stop, "STOP-D5")
        v, stop = self.run_diag(elapsed_s=3700.0)
        self.assertEqual(stop, "STOP-D5")

    def test_secrets_anywhere_are_stop_d2(self):
        v, stop = self.run_diag(DiagRun(extra="[guest-uart] x %s" % PASSWORD))
        self.assertEqual(stop, "STOP-D2")
        with tempfile.TemporaryDirectory() as d:
            ev, keys, pcap = DiagRun().write(Path(d))
            (ev / "host-ss-snap-a.txt").write_text("# rc=0\nleak %s\n" % PASSWORD)
            v, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
            self.assertEqual(stop, "STOP-D2")
            pcap.write_bytes(pcap.read_bytes() + b"..." + PASSWORD.encode() + b"...")
            J.prep_diag(ev, pcap, J.secrets_from(keys), PORT)
            (ev / "host-ss-snap-a.txt").write_text("# rc=0\nok\n")
            v, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
            self.assertEqual(stop, "STOP-D2")
            self.assertIn("diag:no-secret-in-pcap", self.failed(v))

    def test_protected_digest_change_is_stop_d1(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = DiagRun().write(Path(d))
            (ev / "protected-after.txt").write_text("b  kernel\n")
            _, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
        self.assertEqual(stop, "STOP-D1")

    def test_gate_failure_is_stop_d4(self):
        r = DiagRun()
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = r.write(Path(d))
            (ev / "serial.log").write_text((ev / "serial.log").read_text().replace("[hv-x86] vCPU ready — entering run loop\n", ""))
            _, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag")
        self.assertEqual(stop, "STOP-D4")

    def test_cli_diag_end_to_end_and_archive(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, pcap = DiagRun().write(Path(d))
            rc = J.main(["diag", "--evidence", str(ev), "--keys", str(keys), "--fingerprint", FP, "--port", PORT, "--pcap", str(pcap)])
            self.assertEqual(rc, 0)
            self.assertEqual(json.loads((ev / "verdict.json").read_text())["diag"]["outcome"], "O1")
            dst = Path(d) / "out"
            self.assertEqual(J.main(["archive", "--src", str(ev), "--dst", str(dst), "--keys", str(keys)]), 0)
            for n in ("host-ss-final.txt", "host-ss-after-kill.txt", "pcap-summary.json", "diag-marks.json", "guest-snaps.txt"):
                self.assertTrue((dst / n).exists(), n)
            self.assertFalse((dst / "net0.pcap").exists())
            self.assertEqual(J.main(["diag", "--evidence", str(ev), "--keys", str(keys), "--fingerprint", FP, "--port", PORT]), 2)

    def test_control_must_really_have_started_and_stopped(self):
        v, _ = self.run_diag(DiagRun(control_started=False))
        self.assertEqual(v["diag"]["outcome"], "O6")
        self.assertIs(v["diag"]["control_producer_started"], False)
        v, _ = self.run_diag(DiagRun(control_stopped=False))
        self.assertEqual(v["diag"]["outcome"], "O6")
        v, _ = self.run_diag()
        self.assertEqual((v["diag"]["control_producer_pid"], v["diag"]["control_producer_stopped_at_snap_a"]), ("490", True))

    def test_shifted_pcap_stamps_are_judged_like_plain_ones(self):
        v, _ = self.run_diag(DiagRun(pcap_shift=25200.0))
        self.assertEqual(v["diag"]["outcome"], "O1", v["diag"]["outcome_why"])


class ReclaimedDiag(unittest.TestCase):
    """O7, STOP-7, the reclaim-line rules and --expect-outcome end to end (synthetic evidence directories)."""

    def run_diag(self, run, expect="O7", **kw):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = run.write(Path(d), **kw)
            return J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag", expect_outcome=expect)

    def failed(self, v):
        return {k for k, c in v["checks"].items() if not c["pass"]}

    def test_o7_run_passes_with_expect_outcome(self):
        v, stop = self.run_diag(reclaimed())
        self.assertTrue(v["pass"], self.failed(v))
        self.assertIsNone(stop)
        self.assertEqual(v["diag"]["outcome"], "O7", v["diag"]["outcome_why"])
        self.assertTrue(v["checks"]["diag:outcome-is-O7"]["pass"])
        self.assertIs(v["diag"]["cancel_A_rst_from_cell"], True)
        self.assertIs(v["diag"]["cancel_A_fin_from_cell"], False)
        self.assertEqual(v["diag"]["cancel_A_zero_window_from_cell_after_first_rst"], 0)
        self.assertIs(v["diag"]["net_owner_death_in_cancel_window"], True)
        self.assertEqual(v["net"]["owner_death_lines"][0]["aborted"], 1)

    def test_o7_without_expect_outcome_has_no_outcome_check(self):
        v, stop = self.run_diag(reclaimed(), expect="")
        self.assertNotIn("diag:outcome-is-O7", v["checks"])
        self.assertEqual(v["diag"]["outcome"], "O7")

    def test_o7_allows_closing_states_early_and_a_zero_window_ack_before_the_rst(self):
        v, stop = self.run_diag(reclaimed(snap_states=("06", "05", None), zero_ack=None, zero_before_rst=True))
        self.assertTrue(v["pass"], self.failed(v))
        self.assertEqual(v["diag"]["outcome"], "O7")

    def test_expect_outcome_o7_fails_an_o1_run(self):
        v, stop = self.run_diag(DiagRun())
        self.assertEqual(v["diag"]["outcome"], "O1")
        self.assertIn("diag:outcome-is-O7", self.failed(v))
        self.assertFalse(v["pass"])
        self.assertEqual(stop, "MARKER-FAIL")
        # without the reclaim line as well (what the T3-D2 boot really looked like)
        v, stop = self.run_diag(DiagRun(reclaim="none"))
        self.assertEqual({"diag:outcome-is-O7", "net:owner-death-reclaim-logged-in-cancel-window"} - self.failed(v), set())
        self.assertEqual(stop, "MARKER-FAIL")

    def test_o4_shaped_run_is_not_o7(self):
        run = reclaimed(snap_states=("01", "01", None), yes=(True, True, False), ss_estab=(2, 2, 0))
        v, stop = self.run_diag(run)
        self.assertEqual(v["diag"]["outcome"], "O4")
        self.assertIn("diag:outcome-is-O7", self.failed(v))
        self.assertEqual(stop, "MARKER-FAIL")

    def test_each_observation_alone_prevents_o7_end_to_end(self):
        cases = {
            "no RST from the cell": dict(cancel_rst_cell=False),
            "FIN as well as RST": dict(cancel_fin_cell=True),
            "zero-window ACK after the RST": dict(zero_ack=1105.0),
            "no end towards the guest": dict(cancel_fin_slirp=False),
            "host ESTAB at snap-c": dict(ss_estab=(0, 0, 1)),
            "yes alive at snap-b": dict(yes=(False, True, False)),
            "ESTABLISHED at snap-a": dict(snap_states=("01", None, None)),
            "CLOSE_WAIT at snap-b": dict(snap_states=(None, "08", None)),
            "present at snap-c": dict(snap_states=(None, None, "06")),
            "cancel marker FAIL": dict(cancel="fail"),
            "control close not logged": dict(control_closed=False),
            "control producer still alive": dict(control_stopped=False),
            "control producer never started": dict(control_started=False),
            "no reclaim line": dict(reclaim="none"),
            "reclaim line aborted 0": dict(reclaim="zero"),
            "reclaim line before the cancel case": dict(reclaim="before"),
            "reclaim line after the cancel verdict": dict(reclaim="after"),
        }
        for name, kw in cases.items():
            v, _ = self.run_diag(reclaimed(**kw))
            self.assertNotEqual(v["diag"]["outcome"], "O7", name)
            self.assertIn("diag:outcome-is-O7", self.failed(v), name)
            self.assertFalse(v["pass"], name)

    def test_reclaim_line_rules_in_the_diag_stage(self):
        for how in ("none", "zero", "before", "after"):
            v, stop = self.run_diag(reclaimed(reclaim=how))
            self.assertIn("net:owner-death-reclaim-logged-in-cancel-window", self.failed(v), how)
            self.assertEqual(stop, "MARKER-FAIL", how)

    def test_stop7_in_the_diag_stage(self):
        v, stop = self.run_diag(reclaimed(watch_unavailable=True))
        self.assertEqual(stop, "STOP-7")
        self.assertIn("net:no-owner-death-watch-unavailable", self.failed(v))
        self.assertEqual(v["diag"]["net_watch_unavailable"], True)
        self.assertEqual(v["diag"]["outcome"], "O6")  # never O7 with the watch unavailable
        # even when the gates are incomplete: the label stays STOP-7, not STOP-D4
        with tempfile.TemporaryDirectory() as d:
            ev, keys, _ = reclaimed(watch_unavailable=True).write(Path(d))
            (ev / "serial.log").write_text((ev / "serial.log").read_text().replace("[hv-x86] vCPU ready — entering run loop\n", ""))
            _, stop = J.judge_boot(ev, J.secrets_from(keys), FP, PORT, "head", stage="diag", expect_outcome="O7")
        self.assertEqual(stop, "STOP-7")
        # the rules before it still win: argv (STOP-D3), a secret (STOP-D2)
        _, stop = self.run_diag(reclaimed(watch_unavailable=True), argv=ARGV)
        self.assertEqual(stop, "STOP-D3")
        _, stop = self.run_diag(reclaimed(watch_unavailable=True, extra="[guest-uart] x %s" % PASSWORD))
        self.assertEqual(stop, "STOP-D2")

    def test_watch_text_on_a_guest_uart_line_is_not_stop7(self):
        v, stop = self.run_diag(reclaimed(watch_unavailable="guest"))
        self.assertTrue(v["pass"], self.failed(v))
        self.assertIsNone(stop)

    def test_cli_expect_outcome(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys, pcap = reclaimed().write(Path(d))
            base = ["diag", "--evidence", str(ev), "--keys", str(keys), "--fingerprint", FP, "--port", PORT, "--pcap", str(pcap)]
            self.assertEqual(J.main(base + ["--expect-outcome", "O7"]), 0)
            verdict = json.loads((ev / "verdict.json").read_text())
            self.assertEqual(verdict["diag"]["outcome"], "O7")
            self.assertEqual(verdict["net"]["owner_death_lines"][0]["aborted"], 1)
            self.assertFalse(verdict["net"]["watch_unavailable"])
            self.assertEqual(J.main(base + ["--expect-outcome", "O1"]), 1)
            self.assertEqual(J.main(base + ["--expect-outcome", "O9"]), 2)
            self.assertEqual(J.main(base), 0)  # no expectation: the O7 run passes, as any diag run that satisfies the checks

    def test_cli_full_has_net_in_the_verdict_and_rejects_expect_outcome(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d), serial=full_serial(), verdict="driver-verdict", full=True)
            base = ["full", "--evidence", str(ev), "--keys", str(keys), "--fingerprint", FP, "--port", PORT]
            self.assertEqual(J.main(base), 0)
            self.assertEqual(json.loads((ev / "verdict.json").read_text())["net"]["owner_death_lines"][0]["aborted"], 1)
            self.assertEqual(J.main(base + ["--expect-outcome", "O7"]), 2)


class Archive(unittest.TestCase):
    def test_archive_copies_text_only(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            (ev / "serial.raw.log").write_text("raw")
            dst = Path(d) / "out"
            rc = J.main(["archive", "--src", str(ev), "--dst", str(dst), "--keys", str(keys)])
            self.assertEqual(rc, 0)
            self.assertTrue((dst / "serial.log").exists())
            self.assertTrue((dst / "boot-config" / "autoload.toml").exists())
            self.assertFalse((dst / "serial.raw.log").exists())

    def test_archive_refuses_on_secret(self):
        with tempfile.TemporaryDirectory() as d:
            ev, keys = make_run(Path(d))
            (ev / "preflight.txt").write_text("leak %s\n" % PASSWORD)
            dst = Path(d) / "out"
            rc = J.main(["archive", "--src", str(ev), "--dst", str(dst), "--keys", str(keys)])
            self.assertEqual(rc, 1)
            self.assertFalse(dst.exists())


if __name__ == "__main__":
    unittest.main()
