#!/usr/bin/env python3
"""Host tests of scripts/tier3_ssh_guest_judge.py (marker accounting, argv invariant, secret scan,
fingerprint rule, no-sshd rule, STOP classification, archive refusal). Run:
    python3 -m unittest scripts/test_tier3_ssh_guest_judge.py   (or python3 scripts/test_tier3_ssh_guest_judge.py)
"""
import json
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


def make_run(root: Path, serial=SERIAL, argv=ARGV, c1_ok=True, verdict="gates-satisfied", layout="/bin/hypervisor\n"):
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
