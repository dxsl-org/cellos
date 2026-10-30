#!/usr/bin/env python3
"""Host-only tests for kssh_keys.py and kssh_judge.py (K8 T0 lane). No Docker, QEMU, network.

    python3 scripts/test_kssh_judge.py

Temp directories live under /tmp (mode 700) and are removed afterwards. Keys generated here are
throw-away TEST keys made with the host `ssh-keygen`.
"""

import contextlib
import hashlib
import io
import json
import os
import re
import secrets
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import kssh_judge as J  # noqa: E402
import kssh_keys as K  # noqa: E402

try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
except ImportError:  # pragma: no cover
    Ed25519PrivateKey = None

# Exact lines of the C-host control against the real OpenSSH_9.9p2 fixture
# (.agents/261030-1000-kms-ssh-key-custody/reports/k8-evidence/c-host/sshd.log), LogLevel VERBOSE.
FPA = "SHA256:tgqKHasdZQEX4EV+YPi9ehSJ+3a6kAhBc5SIaSP7sZ8"
FPB = "SHA256:nq/WKR7eebxgBG55/XSXqTFsruGZRJ9t5iwm9iAvubw"
CHOST = f"""chpasswd: password for 'tester' changed
Server listening on 0.0.0.0 port 2223.
Server listening on 0.0.0.0 port 22.
Server listening on 0.0.0.0 port 2222.
Server listening on :: port 2223.
Connection from 172.17.0.1 port 35938 on 172.17.0.2 port 22 rdomain ""
Accepted key ED25519 {FPA} found at /fixture/authorized_keys_main:1
Postponed publickey for tester from 172.17.0.1 port 35938 ssh2 [preauth]
Accepted key ED25519 {FPA} found at /fixture/authorized_keys_main:1
Accepted publickey for tester from 172.17.0.1 port 35938 ssh2: ED25519 {FPA}
User child is on pid 24
Starting session: command for tester from 172.17.0.1 port 35938 id 0
Received disconnect from 172.17.0.1 port 35938:11: disconnected by user
Disconnected from user tester 172.17.0.1 port 35938
Connection from 172.17.0.1 port 35942 on 172.17.0.2 port 22 rdomain ""
Failed publickey for tester from 172.17.0.1 port 35942 ssh2: ED25519 {FPB}
Connection closed by authenticating user tester 172.17.0.1 port 35942 [preauth]
Connection from 172.17.0.1 port 35956 on 172.17.0.2 port 22 rdomain ""
Accepted key ED25519 {FPA} found at /fixture/authorized_keys_main:1
Postponed publickey for tester from 172.17.0.1 port 35956 ssh2 [preauth]
Connection closed by authenticating user tester 172.17.0.1 port 35956 [preauth]
Connection from 172.17.0.1 port 47216 on 172.17.0.2 port 2223 rdomain ""
Accepted key ED25519 {FPB} found at /fixture/authorized_keys_b:1
Postponed publickey for tester from 172.17.0.1 port 47216 ssh2 [preauth]
Accepted key ED25519 {FPB} found at /fixture/authorized_keys_b:1
Accepted publickey for tester from 172.17.0.1 port 47216 ssh2: ED25519 {FPB}
User child is on pid 32
Starting session: command for tester from 172.17.0.1 port 47216 id 0
Received disconnect from 172.17.0.1 port 47216:11: disconnected by user
Disconnected from user tester 172.17.0.1 port 47216
Connection from 172.17.0.1 port 47224 on 172.17.0.2 port 2223 rdomain ""
Failed publickey for tester from 172.17.0.1 port 47224 ssh2: ED25519 {FPA}
Connection closed by authenticating user tester 172.17.0.1 port 47224 [preauth]
Connection from 172.17.0.1 port 35970 on 172.17.0.2 port 22 rdomain ""
kex_exchange_identification: Connection closed by remote host
Connection closed by 172.17.0.1 port 35970
"""
# L5 password forms (rerun/lanes/default-p09-nofaults/sshd.log)
L5 = """Connection from 172.17.0.1 port 60418 on 172.17.0.3 port 22 rdomain ""
Connection closed by 172.17.0.1 port 60418 [preauth]
Connection from 172.17.0.1 port 37736 on 172.17.0.3 port 22 rdomain ""
Accepted password for tester from 172.17.0.1 port 37736 ssh2
User child is on pid 33
Starting session: command for tester from 172.17.0.1 port 37736 id 0
Close session: user tester from 172.17.0.1 port 37736 id 0
Connection closed by 172.17.0.1 port 37736
Transferred: sent 1504, received 612 bytes
Closing connection to 172.17.0.1 port 37736
Connection from 172.17.0.1 port 37766 on 172.17.0.3 port 22 rdomain ""
Failed password for tester from 172.17.0.1 port 37766 ssh2
Connection closed by authenticating user tester 172.17.0.1 port 37766 [preauth]
"""


def classes(text):
    return [c["cls"] for c in J.parse_sshd(text.splitlines())["conns"]]


class SshdPatternTests(unittest.TestCase):
    def test_c_host_log_classes(self):
        self.assertEqual(classes(CHOST), [
            "accepted-pk:" + FPA, "probe-rejected", "probe-ok", "accepted-pk:" + FPB,
            "probe-rejected", "closed"])

    def test_probe_ok_logs_accepted_key_but_is_not_accepted(self):
        text = "Connection from" + CHOST.split("Connection from")[3]
        c = J.parse_sshd(text.splitlines())["conns"][0]
        self.assertEqual(c["cls"], "probe-ok")
        self.assertNotIn("accepted-pk", [e for e, _ in c["events"]])

    def test_l5_password_forms(self):
        self.assertEqual(classes(L5), ["closed", "accepted-pw", "failed-pw"])

    def test_each_pattern_matches_its_line(self):
        cases = {
            "connect": 'Connection from 10.0.0.1 port 5 on 10.0.0.2 port 22 rdomain ""',
            "accepted-pk": f"Accepted publickey for tester from 10.0.0.1 port 5 ssh2: ED25519 {FPA}",
            "postponed-pk": "Postponed publickey for tester from 10.0.0.1 port 5 ssh2 [preauth]",
            "failed-pk": f"Failed publickey for tester from 10.0.0.1 port 5 ssh2: ED25519 {FPB}",
            "accepted-pw": "Accepted password for tester from 10.0.0.1 port 5 ssh2",
            "failed-pw": "Failed password for tester from 10.0.0.1 port 5 ssh2",
            "session": "Starting session: command for tester from 10.0.0.1 port 5 id 0",
            "closed": "Connection closed by authenticating user tester 10.0.0.1 port 5 [preauth]",
        }
        for event, line in cases.items():
            with self.subTest(event=event):
                hits = [e for e, rx in J.SSHD_PATTERNS if rx.search(line)]
                self.assertEqual(hits[0], event)
        for line in ("Connection closed by 10.0.0.1 port 5 [preauth]", "Disconnected from user tester 10.0.0.1 port 5",
                     "Received disconnect from 10.0.0.1 port 5:11: Bye", "Connection reset by 10.0.0.1 port 5 [preauth]"):
            with self.subTest(line=line):
                self.assertEqual([e for e, rx in J.SSHD_PATTERNS if rx.search(line)][0], "closed")

    def test_grouping_by_port_with_interleaving(self):
        lines = ['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""',
                 'Connection from 1.1.1.1 port 11 on 2.2.2.2 port 22 rdomain ""',
                 "Failed publickey for tester from 1.1.1.1 port 11 ssh2: ED25519 " + FPB,
                 "Postponed publickey for tester from 1.1.1.1 port 10 ssh2 [preauth]",
                 "Connection closed by authenticating user tester 1.1.1.1 port 11 [preauth]",
                 f"Accepted publickey for tester from 1.1.1.1 port 10 ssh2: ED25519 {FPA}"]
        got = J.parse_sshd(lines)["conns"]
        self.assertEqual([(c["port"], c["cls"]) for c in got], [(10, "accepted-pk:" + FPA), (11, "probe-rejected")])

    def test_port_reuse_starts_a_new_connection(self):
        lines = ['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""', "Accepted password for tester from 1.1.1.1 port 10 ssh2",
                 'Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""', "Connection closed by 1.1.1.1 port 10 [preauth]"]
        self.assertEqual([c["cls"] for c in J.parse_sshd(lines)["conns"]], ["accepted-pw", "closed"])

    def test_unknown_auth_wording_is_unclassified_and_orphans_reported(self):
        p = J.parse_sshd(['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""',
                          "Weird publickey outcome for tester from 1.1.1.1 port 10 ssh2",
                          "Failed password for tester from 9.9.9.9 port 99 ssh2"])
        self.assertEqual(p["conns"][0]["cls"], "unclassified")
        self.assertEqual(len(p["orphans"]), 1)

    def test_signature_rejected_and_dirty_accept(self):
        lines = ['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""',
                 "Postponed publickey for tester from 1.1.1.1 port 10 ssh2 [preauth]",
                 "Failed publickey for tester from 1.1.1.1 port 10 ssh2: ED25519 " + FPA]
        self.assertEqual(classes("\n".join(lines)), ["sig-rejected"])
        dirty = ['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""',
                 f"Accepted publickey for tester from 1.1.1.1 port 10 ssh2: ED25519 {FPA}",
                 f"Accepted publickey for tester from 1.1.1.1 port 10 ssh2: ED25519 {FPA}"]
        self.assertEqual(classes("\n".join(dirty)), ["accepted-pk-dirty"])

    def test_password_after_publickey_is_a_fallback_and_never_a_pass(self):
        lines = ['Connection from 1.1.1.1 port 10 on 2.2.2.2 port 22 rdomain ""',
                 "Failed publickey for tester from 1.1.1.1 port 10 ssh2: ED25519 " + FPB,
                 "Accepted password for tester from 1.1.1.1 port 10 ssh2"]
        p = J.parse_sshd(lines)
        self.assertEqual(J.password_fallbacks(p["conns"]), [0])
        self.assertEqual(p["conns"][0]["cls"], "accepted-pw")
        self.assertEqual(J.password_fallbacks(J.parse_sshd(L5.splitlines())["conns"]), [])


FPS = {"ka16": "SHA256:A" * 1, "kl4": "SHA256:L", "kp0": "SHA256:P"}


def conn(cls, port=1):
    return {"cls": cls, "port": port, "idx": 0, "events": []}


class SequenceTests(unittest.TestCase):
    def test_expected_tables_follow_the_contract(self):
        t = J.EXPECTED_CONNECTIONS["t0"]
        self.assertEqual(t["s00"], ["accepted-pw"])
        self.assertEqual(t["s04a"], [])
        self.assertEqual(t["s15-iii"], ["closed"])
        self.assertEqual([t[f"s08-w{i}"] for i in range(1, 6)], [["probe-ok"]] * 5)
        self.assertEqual(J.EXPECTED_CONNECTIONS["f"]["s11"], ["accepted-pk:ka16"])
        self.assertEqual(J.expected_for("t0", "cal-R64"), ["probe-ok"])
        self.assertIsNone(J.expected_for("t0", "s99"))

    def cmp(self, attempts, got, mode="t0"):
        return J.compare_sequence(mode, attempts, [conn(c, 100 + i) for i, c in enumerate(got)], FPS)

    def test_match(self):
        r = self.cmp(["s00", "s04a", "s02", "s03"], ["accepted-pw", "accepted-pk:" + FPS["ka16"], "probe-ok"])
        self.assertTrue(r["ok"], r)

    def test_mismatch_reports_first_index_and_ids(self):
        r = self.cmp(["s00", "s02", "s03"], ["accepted-pw", "accepted-pk:" + FPS["ka16"], "probe-rejected"])
        self.assertFalse(r["ok"])
        self.assertEqual((r["index"], r["attempt"], r["expected_class"], r["got_class"]), (2, "s03", "probe-ok", "probe-rejected"))

    def test_wrong_key_fingerprint_is_a_mismatch(self):
        r = self.cmp(["s02"], ["accepted-pk:" + FPS["kl4"]])
        self.assertFalse(r["ok"])
        self.assertEqual(r["index"], 0)

    def test_extra_and_missing_connections(self):
        r = self.cmp(["s04a"], ["closed"])
        self.assertEqual((r["ok"], r["index"], r["got_class"]), (False, 0, "closed"))
        r = self.cmp(["s00", "s03"], ["accepted-pw"])
        self.assertEqual((r["ok"], r["index"], r["attempt"], r["got_class"]), (False, 1, "s03", "<none>"))

    def test_unknown_attempt_id(self):
        self.assertEqual(self.cmp(["s00", "zz"], ["accepted-pw"])["unknown_attempts"], ["zz"])

    def test_s06d_optional_non_acceptance_and_acceptance_is_flagged(self):
        a = ["s06d", "s02"]
        self.assertTrue(self.cmp(a, ["accepted-pk:" + FPS["ka16"]])["ok"])
        self.assertTrue(self.cmp(a, ["probe-rejected", "accepted-pk:" + FPS["ka16"]])["ok"])
        r = self.cmp(a, ["accepted-pk:" + FPS["ka16"], "accepted-pk:" + FPS["ka16"]])
        self.assertFalse(r["ok"])
        self.assertTrue(r["optional_accepted"])

    def test_calibration_attempts(self):
        self.assertTrue(self.cmp(["cal-R1", "cal-R16", "cal-R64"], ["probe-ok"] * 3)["ok"])

    def test_prefix_up_to_first_epoch(self):
        conns = [conn("accepted-pw"), conn("accepted-pk:" + FPS["ka16"]), conn("closed")]
        attempts = ["s00", "s02", "s03"]
        self.assertTrue(J.prefix_ok("t0", attempts, conns[:2], FPS, "s02"))
        self.assertTrue(J.prefix_ok("t0", attempts, conns, FPS, "s02"))  # later mismatch does not matter
        self.assertFalse(J.prefix_ok("t0", attempts, [conn("accepted-pw"), conn("probe-ok")], FPS, "s02"))
        self.assertFalse(J.prefix_ok("t0", ["s00"], conns, FPS, "s02"))

    def test_f_mode_table(self):
        r = J.compare_sequence("f", ["s00", "s05a", "s05b", "s05c", "s02", "s11"],
                               [conn(c) for c in ["accepted-pw", "probe-ok", "probe-ok", "probe-ok",
                                                  "accepted-pk:" + FPS["ka16"], "accepted-pk:" + FPS["ka16"]]], FPS)
        self.assertTrue(r["ok"], r)


class SerialTests(unittest.TestCase):
    def test_parse(self):
        text = "\n".join([
            "USER: P04 START", "USER: P04 PASS shell-prompt", "USER: P04 INFO kssh-attempt s00",
            "USER: P04 INFO kssh-obs s01 fp=SHA256:xyz prefix=0123456789abcdef",
            "USER: P04 FAIL kssh-s02 unlock timeout after 5s", "USER: Cellos > echo P04 PASS fake",
            "P04 INFO kssh-obs unlock-ms 12", "USER: P04 INFO kssh-obs s03 unlock-ms=1234",
            "USER: P04-TERMINAL: FAIL failures=1 checks=3"])
        S = J.parse_serial(text)
        self.assertTrue(S["started"])
        self.assertEqual(list(S["passes"]), ["shell-prompt"])
        self.assertEqual(S["fails"], [("kssh-s02", "unlock timeout after 5s")])
        self.assertEqual(S["attempts"], ["s00"])
        self.assertEqual(S["obs"][0]["kv"], {"fp": "SHA256:xyz", "prefix": "0123456789abcdef"})
        self.assertEqual(S["obs"][-1]["kv"], {"unlock-ms": "1234"})
        self.assertEqual(S["terminal"], ["FAIL failures=1 checks=3"])


# ---------------------------------------------------------------------------------- keys fixture

class KeysCase(unittest.TestCase):
    """One generated key set shared by the class (generation costs about a second)."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = Path(tempfile.mkdtemp(prefix="kssh-test-"))
        cls.keys, cls.stage = cls.tmp / "keys", cls.tmp / "stage"
        cls.exp = K.gen(cls.keys, cls.stage, calibrate=True)
        cls.sec = K.load_secrets(cls.keys)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography not installed")
class KeyGenTests(KeysCase):
    def test_modes_and_layout(self):
        self.assertEqual(stat.S_IMODE(self.keys.stat().st_mode), 0o700)
        self.assertEqual(stat.S_IMODE((self.keys / "secrets.json").stat().st_mode), 0o600)
        for name in ("ka16", "kl4", "kp0", "bad-aes128", "bad-chacha", "kr1", "kr64"):
            self.assertEqual(stat.S_IMODE((self.keys / name).stat().st_mode), 0o600, name)
            self.assertTrue((self.stage / f"{name}.key").read_bytes().startswith(b"-----BEGIN OPENSSH PRIVATE KEY-----\n"))
            self.assertNotIn(b"\r", (self.stage / f"{name}.key").read_bytes())
        self.assertEqual((self.stage / "raw.bin").stat().st_size, 4096)
        self.assertEqual(len((self.stage / "authdir/authorized_keys_main").read_text().splitlines()), 3)
        self.assertEqual(len((self.stage / "authdir/authorized_keys_b").read_text().splitlines()), 1)

    def test_fingerprints_equal_ssh_keygen(self):
        for name, e in self.exp["keys"].items():
            out = subprocess.run(["ssh-keygen", "-lf", str(self.keys / f"{name}.pub")], capture_output=True, text=True, check=True).stdout
            self.assertEqual(out.split()[1], e["fp"], name)
        out = subprocess.run(["ssh-keygen", "-lf", str(self.keys / "kb.pub")], capture_output=True, text=True, check=True).stdout
        self.assertEqual(out.split()[1], self.exp["keyb"]["fp"])

    def test_kb_private_half_is_deleted(self):
        names = sorted(p.name for p in self.keys.iterdir())
        self.assertIn("kb.pub", names)
        self.assertNotIn("kb", names)
        self.assertFalse([n for n in names if n.startswith(".work")])
        self.assertNotIn("kb", self.sec["keys"])
        # no file in either directory holds a private key for kb
        kb_blob = K.base64.b64decode(self.exp["keyb"]["pub"].split()[1])
        for p in list(self.keys.iterdir()) + list(self.stage.rglob("*")):
            if p.is_file() and p.name.endswith(".key") or p.name in ("ka16", "kl4", "kp0"):
                self.assertNotEqual(K.parse_container(K.decode_armor(p.read_bytes()))["pubblob"], kb_blob)

    def test_seed_derivation_matches_public_key(self):
        for name in ("ka16", "kl4", "kp0", "kr1", "kr64", "bad-aes128", "bad-chacha"):
            seed = bytes.fromhex(self.sec["keys"][name]["seed"])
            sk = Ed25519PrivateKey.from_private_bytes(seed)
            pub = K.parse_pub_ed25519(K.base64.b64decode((self.keys / f"{name}.pub").read_text().split()[1]))
            self.assertEqual(sk.public_key().public_bytes_raw(), pub, name)
            Ed25519PublicKey.from_public_bytes(pub).verify(sk.sign(b"kssh"), b"kssh")
        # plain key: the container itself yields the same seed
        plain = K.parse_container(K.decode_armor((self.keys / "kp0").read_bytes()))
        self.assertEqual(plain["seed"].hex(), self.sec["keys"]["kp0"]["seed"])

    def test_wrapped_files_parse_as_intended(self):
        for name, rounds in (("ka16", 16), ("kl4", 4), ("kr1", 1), ("kr64", 64)):
            c = K.parse_container(K.decode_armor((self.keys / name).read_bytes()))
            self.assertEqual((c["cipher"], c["kdf"], c["rounds"]), ("aes256-ctr", "bcrypt", rounds))
            self.assertEqual(self.exp["keys"][name]["rounds"], rounds)
        c = K.parse_container(K.decode_armor((self.keys / "kp0").read_bytes()))
        self.assertEqual((c["cipher"], c["kdf"]), ("none", "none"))
        self.assertEqual(self.exp["keys"]["kp0"]["rounds"], 0)
        for name, cipher in K.BAD_KEYS.items():
            self.assertEqual(K.parse_container(K.decode_armor((self.keys / name).read_bytes()))["cipher"], cipher)

    def test_passphrases_and_wrong_values(self):
        for name in ("ka16", "kl4", "kr1", "kr64"):
            k = self.sec["keys"][name]
            for v in (k["passphrase"], k["wrong"]):
                self.assertRegex(v, r"^[A-Za-z0-9]{24}$")
            self.assertNotEqual(k["passphrase"], k["wrong"])
        self.assertIsNone(self.sec["keys"]["kp0"]["passphrase"])
        self.assertRegex(self.sec["nonce"], r"^[0-9a-f]{16}$")

    def test_blob_prefix_is_sha256_of_decoded_body(self):
        for name in ("ka16", "kl4", "kp0"):
            text = (self.keys / name).read_text().splitlines()
            body = K.base64.b64decode("".join(text[1:-1]))
            self.assertEqual(hashlib.sha256(body).hexdigest()[:16], self.exp["keys"][name]["blob_prefix"])
            self.assertEqual(K.blob_prefix(body), self.exp["keys"][name]["blob_prefix"])
            self.assertEqual(len(body), self.exp["keys"][name]["blob_len"])

    def test_raw_bin(self):
        raw = (self.stage / "raw.bin").read_bytes()
        self.assertIn(b"\0", raw)
        self.assertIn(b"\xff", raw)
        self.assertEqual(raw, K.raw_bin(self.sec["nonce"]))
        self.assertEqual(hashlib.sha256(raw).hexdigest(), self.exp["raw"]["sha256"])

    def test_conf_round_trip(self):
        pw = "fixturepw-1234"
        text = K.build_conf(self.keys, "t0", [2200, 2222, 2223], FPA, pw, True)
        self.assertTrue(all(len(l.encode()) < 400 for l in text.splitlines()))
        d = K.parse_conf(text)
        self.assertEqual((d["mode"], d["calibrate"], d["ports"], d["hostfp"], d["password"]), ("t0", 1, [2200, 2222, 2223], FPA, pw))
        self.assertEqual(d["nonce"], self.sec["nonce"])
        self.assertEqual(d["raw"], {"sha256": self.exp["raw"]["sha256"], "len": 4096})
        self.assertEqual(sorted(d["keys"]), ["ka16", "kl4", "kp0", "kr1", "kr64"])
        for name, k in d["keys"].items():
            s = self.sec["keys"][name]
            self.assertEqual((k["passphrase"], k["wrong"]), (s["passphrase"] or "-", s["wrong"] or "-"))
            self.assertEqual((k["fp"], k["blob_prefix"], k["rounds"]),
                             (self.exp["keys"][name]["fp"], self.exp["keys"][name]["blob_prefix"], self.exp["keys"][name]["rounds"]))
        self.assertEqual(d["keyb"], self.exp["keyb"]["fp"])
        self.assertNotIn("kr1", K.parse_conf(K.build_conf(self.keys, "f", [1, 2, 3], FPA, pw, False))["keys"])

    def test_conf_rejects_bad_input_and_redaction(self):
        for bad in (dict(mode="x"), dict(ports=[1, 2]), dict(hostfp="SHA256:short"), dict(password="a b")):
            args = dict(mode="t0", ports=[1, 2, 3], hostfp=FPA, password="pw1234567")
            args.update(bad)
            with self.assertRaises(K.KeyError_):
                K.build_conf(self.keys, args["mode"], args["ports"], args["hostfp"], args["password"], False)
        text = K.build_conf(self.keys, "t0", [1, 2, 3], FPA, "pw1234567", False)
        red = K.redact_conf(text)
        self.assertNotIn("pw1234567", red)
        for name in ("ka16", "kl4"):
            self.assertNotIn(self.sec["keys"][name]["passphrase"], red)
            self.assertNotIn(self.sec["keys"][name]["wrong"], red)
        self.assertIn(self.exp["keys"]["ka16"]["fp"], red)
        self.assertEqual(K.parse_conf(red)["password"], "<redacted>")

    def test_cli_conf_writes_mode_600_and_dir_checks(self):
        pwf = self.tmp / "pw"
        pwf.write_text("fixturepw-1234\n")
        rc = K.main(["conf", "--dir", str(self.keys), "--stage", str(self.stage), "--mode", "f", "--ports", "1", "2", "3",
                     "--hostfp", FPA, "--password-file", str(pwf)])
        self.assertEqual(rc, 0)
        p = self.stage / "kssh.conf"
        self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o600)
        self.assertEqual(K.parse_conf(p.read_text())["mode"], "f")

    def test_refuses_dir_inside_git_work_tree_and_loose_modes(self):
        repo = self.tmp / "fakerepo"
        (repo / ".git").mkdir(parents=True)
        with self.assertRaisesRegex(K.KeyError_, "git work tree"):
            K.gen(repo / "keys", self.tmp / "stage2")
        self.assertFalse((repo / "keys").exists())
        with self.assertRaisesRegex(K.KeyError_, "git work tree"):
            K.gen(self.tmp / "keys3", repo / "stage")
        loose = self.tmp / "loose"
        loose.mkdir(mode=0o755)
        os.chmod(loose, 0o755)
        with self.assertRaisesRegex(K.KeyError_, "mode 700"):
            K.gen(loose, self.tmp / "stage4")
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(K.main(["gen", "--dir", str(repo / "k"), "--stage", str(self.tmp / "s5")]), 2)

    def test_second_gen_into_same_dir_refused(self):
        with self.assertRaisesRegex(K.KeyError_, "already holds"):
            K.gen(self.keys, self.tmp / "stage6")

    def test_canaries_contain_every_secret(self):
        can = dict(K.canaries(self.keys))
        for name in ("ka16", "kl4", "kr1", "kr64"):
            self.assertEqual(can[f"pass:{name}"], self.sec["keys"][name]["passphrase"].encode())
            self.assertEqual(can[f"wrong:{name}"], self.sec["keys"][name]["wrong"].encode())
        for name in ("ka16", "kl4", "kp0"):
            seed = bytes.fromhex(self.sec["keys"][name]["seed"])
            self.assertEqual(can[f"seed:{name}"], seed)
            self.assertEqual(can[f"expanded:{name}"], seed + K.parse_pub_ed25519(K.base64.b64decode(self.exp["keys"][name]["pub"].split()[1])))
        for name in ("ka16", "kp0", "bad-aes128", "bad-chacha"):
            self.assertEqual(can[f"armor:{name}"], (self.keys / name).read_bytes().strip())
            self.assertTrue(can[f"blob:{name}"].startswith(K.MAGIC))
            self.assertIn(f"body:{name}", can)
        self.assertEqual(can["magic"], b"openssh-key-v1\0")
        self.assertFalse([l for l in can if "kb" == l.split(":")[-1]])
        self.assertTrue(all(len(v) >= 8 for v in can.values()))

    def test_extra_files_become_canaries(self):
        seedf, pwf = self.tmp / "seed.hex", self.tmp / "fixture-password"
        seedf.write_text(secrets.token_hex(32) + "\n")
        pwf.write_text("pw-" + secrets.token_hex(6) + "\n")
        can = dict(K.canaries(self.keys, [(seedf, "seed"), (pwf, "password")]))
        self.assertEqual(can["extra:seed.hex"], seedf.read_text().strip().encode())
        self.assertEqual(can["extra-bin:seed.hex"], bytes.fromhex(seedf.read_text().strip()))
        self.assertEqual(can["pass:fixture-password"], pwf.read_text().strip().encode())


class FormsTests(unittest.TestCase):
    def test_forms_of_a_known_value(self):
        data = b"0123456789abcdefghijklmn"
        f = dict(K.forms("pass:x", data))
        self.assertEqual(f["raw"], data)
        self.assertEqual(f["hex-lower"], data.hex().encode())
        self.assertEqual(f["hex-upper"], data.hex().upper().encode())
        self.assertTrue(f["dec-list"].startswith(b"48, 49, 50, 51, 52"))
        self.assertIn(b"48, 49, 50", f["dec-list"])
        self.assertEqual(f["dec-list"].count(b", "), len(data) - 1)
        self.assertIn("keys-u32le-hex", f)
        self.assertEqual(f["keys-u32le-hex"], data.decode().encode("utf-32-le").hex().encode())
        self.assertNotIn("keys-u32le-hex", dict(K.forms("seed:x", data)))

    def test_base64_forms_cover_three_alignments_in_context(self):
        import base64
        # a value that needs both alphabets, so the std and url-safe needles differ
        data = bytes([0xFB, 0xFF, 0xBE]) * 4 + secrets.token_bytes(28)
        forms = list(K.forms("seed:x", data))
        for pad in range(3):
            for enc, tag in ((base64.b64encode, "b64"), (base64.urlsafe_b64encode, "b64u")):
                ctx = enc(secrets.token_bytes(pad) + data + secrets.token_bytes(5))
                hits = [n for n, needle in forms if n.startswith(tag + "-") and needle in ctx]
                self.assertEqual(hits, [f"{tag}-a{pad}"], (pad, tag))

    def test_short_values_produce_nothing(self):
        self.assertEqual(list(K.forms("x", b"1234567")), [])
        self.assertEqual(list(K.forms("pass:x", b"abc")), [])
        self.assertTrue(list(K.forms("x", b"12345678")))


# ---------------------------------------------------------------------------------- scanner

def synthetic():
    return [("pass:syn", secrets.token_hex(12).encode()), ("wrong:syn", secrets.token_hex(12).encode()),
            ("seed:syn", secrets.token_bytes(32)), ("expanded:syn", secrets.token_bytes(64)),
            ("armor:syn", b"-----BEGIN OPENSSH PRIVATE KEY-----\n" + secrets.token_hex(60).encode() + b"\n-----END OPENSSH PRIVATE KEY-----"),
            ("blob:syn", K.MAGIC + secrets.token_bytes(200)), ("extra:seed.hex", secrets.token_hex(32).encode())]


class ScannerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="kssh-scan-"))
        self.sc = J.Scanner(synthetic())

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_every_form_of_every_canary_is_detected(self):
        self.assertGreater(len(self.sc.all_forms()), 50)
        for needle, lfs in self.sc.needles.items():
            found = self.sc.scan_bytes(b"noise\nUSER: line " + needle + b" tail\n")
            for lf in lfs:
                self.assertIn(lf, found, lf)

    def test_every_form_detected_in_a_file_at_odd_offsets_and_chunk_edges(self):
        for i, needle in enumerate(list(self.sc.needles)[::3]):
            p = self.tmp / f"f{i}"
            p.write_bytes(b"\0" * 5000 + b"x" * (i % 7) + needle + b"y" * 11)
            for chunk in (4096, 5000, 1 << 20):
                hits = self.sc.scan_file(p, joined=False, chunk=chunk)
                self.assertTrue(hits, (chunk, i))

    def test_all_zero_chunks_are_skipped_without_losing_boundary_hits(self):
        needle = self.sc.canaries[0][1]
        p = self.tmp / "disk.img"
        with open(p, "wb") as f:
            f.write(b"\0" * (1 << 20) + needle + b"\0" * (1 << 20))
        self.assertTrue(self.sc.scan_file(p, joined=False, chunk=1 << 20))
        q = self.tmp / "zeros.img"
        q.write_bytes(b"\0" * (3 << 20))
        self.assertEqual(self.sc.scan_file(q, joined=False, chunk=1 << 20), [])

    def test_wrapped_secret_found_in_joined_pass_only(self):
        secret = self.sc.canaries[0][1]
        p = self.tmp / "wrapped.log"
        p.write_bytes(b"USER: " + secret[:10] + b"\r\nUSER: " + secret[10:] + b"\n")
        self.assertEqual(self.sc.scan_file(p, joined=False), [])
        hits = self.sc.scan_file(p, joined=True)
        self.assertTrue(any(h["label"] == "pass:syn" and h["form"] == "raw+joined" for h in hits))

    def test_hits_never_contain_secret_bytes(self):
        p = self.tmp / "serial.raw.log"
        p.write_bytes(b"USER: " + self.sc.canaries[0][1] + b"\n")
        text = json.dumps(self.sc.scan_file(p))
        for _, data in self.sc.canaries:
            self.assertNotIn(data.decode("latin-1"), text)

    def test_near_misses_are_not_detected(self):
        for label, data in self.sc.canaries:
            if label.startswith(J.ATOMIC_PREFIXES):
                bad = bytearray(data)
                bad[len(bad) // 2] ^= 1
                self.assertEqual(self.sc.scan_bytes(b"x" + bytes(bad) + b"x"), set(), label)
                self.assertEqual(self.sc.scan_bytes(data[:-1] + b"@"), set(), label)
                self.assertEqual(self.sc.scan_bytes(data[:len(data) // 2]), set(), label)
        # a prefix of a base64/hex rendering, a different key's fingerprint, public strings
        self.assertEqual(self.sc.scan_bytes(b"[kms] DEV_POSTURE ssh-dev-custody " + FPA.encode() + b" Accepted publickey"), set())
        seed = self.sc.canaries[2][1]
        self.assertEqual(self.sc.scan_bytes(seed.hex().encode()[:-2] + b"zz"), set())

    def test_short_canaries_are_not_used(self):
        sc = J.Scanner([("pass:short", b"abc1234")])
        self.assertEqual(sc.needles, {})

    def test_controls_pass_and_fail_when_the_scanner_is_blind(self):
        res = J.run_controls(self.sc, b"benign serial\n" * 50, self.tmp)
        self.assertTrue(res["ok"], res)
        blind = J.Scanner(synthetic())
        blind.scan_file = lambda *a, **k: []
        self.assertFalse(J.run_controls(blind, b"benign\n", self.tmp)["ok"])
        eager = J.Scanner(synthetic())
        eager.scan_file = lambda path, joined=True, chunk=J.CHUNK: (
            [{"label": "x", "form": "raw", "offset": 0}] if Path(path).name == "decoys.bin" else
            J.Scanner.scan_file(eager, path, joined, chunk))
        self.assertFalse(J.run_controls(eager, b"benign\n", self.tmp)["ok"])

    def test_selftest_all_pass(self):
        res = J.selftest()
        self.assertTrue(res)
        self.assertTrue(all(ok for _, ok, _ in res), res)

    def test_redaction_of_output_strings(self):
        obj = {"a": {"b": ["fine", "oops " + self.sc.canaries[0][1].decode()]}, "n": 3}
        red, n = J.redact_obj(obj, self.sc)
        self.assertEqual(n, 1)
        self.assertEqual(red["a"]["b"], ["fine", "<redacted: canary>"])
        self.assertEqual(red["n"], 3)


# ---------------------------------------------------------------------------------- classes

class ClassTests(unittest.TestCase):
    def test_precedence_before_first_sign_epoch(self):
        d = J.decide_class
        self.assertEqual(d([("D", "x"), ("C", "y"), ("E", "z")], False, True, False, False)[0], "E")
        self.assertEqual(d([("D", "x"), ("C", "y")], False, True, False, False)[0], "C")
        self.assertEqual(d([("D", "x")], False, True, False, False)[0], "D")
        self.assertEqual(d([("D", "x")], False, True, True, False)[0], "D")

    def test_after_first_sign_epoch_d_stays_d_e_and_c_become_tail_b(self):
        d = J.decide_class
        self.assertEqual(d([], True, True, False, True), ("A", False))
        for cls in ("E", "C"):
            self.assertEqual(d([(cls, "later")], True, True, False, True), ("B", True))
        self.assertEqual(d([("D", "later")], True, True, False, True), ("D", False))
        self.assertEqual(d([("E", "x"), ("C", "y"), ("D", "z")], True, True, False, True), ("D", False))

    def test_controls_failure_is_class_e_even_after_epoch(self):
        self.assertEqual(J.decide_class([("E", "scan-controls")], True, False, False, False)[0], "E")
        self.assertEqual(J.decide_class([("E", "scan-controls"), ("D", "x")], False, False, False, False)[0], "E")

    def test_canary_hit_after_epoch_is_final_d(self):
        self.assertEqual(J.decide_class([("D", "no-canary-in-targets")], True, True, True, False)[0], "D")

    def test_after_epoch_without_core_d_still_wins_otherwise_precedence(self):
        self.assertEqual(J.decide_class([("C", "x"), ("D", "y")], True, True, False, False)[0], "D")
        self.assertEqual(J.decide_class([("C", "x"), ("E", "y")], True, True, False, False)[0], "E")


# ---------------------------------------------------------------------------------- end to end

FP_HOST = "SHA256:cONn2eJlrd6p5aQzoj+EKOthgBXRtv+n46chw14RSLo"
ATTEMPTS_BEFORE = {  # marker -> attempt ids typed before it
    "t0": {"kssh-s00": ["s00"], "kssh-s10": ["s10-changed", "s10-untrusted"], "kssh-s04a": ["s04a"], "kssh-s04b": ["s04b"],
           "kssh-s06d": ["s06d"], "kssh-s02": ["s02"], "kssh-s02p": ["s02p"], "kssh-s03": ["s03"], "kssh-s03b": ["s03b"],
           "kssh-s02x": ["s02x"], "kssh-s08": [f"s08-w{i}" for i in range(1, 6)] + ["s08-locked", "s08-ok"],
           "kssh-s15": ["s15-i", "s15-iii", "s15-iv"]},
    "f": {"kssh-s00": ["s00"], "kssh-s05a": ["s05a"], "kssh-s05b": ["s05b"], "kssh-s05c": ["s05c"], "kssh-s02": ["s02"],
          "kssh-s11": ["s11"]},
}


def sshd_lines(cls, port, listener=22):
    head = f'Connection from 172.17.0.1 port {port} on 172.17.0.2 port {listener} rdomain ""'
    if cls == "accepted-pw":
        return [head, f"Accepted password for tester from 172.17.0.1 port {port} ssh2", "User child is on pid 9",
                f"Starting session: command for tester from 172.17.0.1 port {port} id 0",
                f"Connection closed by 172.17.0.1 port {port}"]
    if cls.startswith("accepted-pk:"):
        fp = cls.split(":", 1)[1]
        return [head, f"Accepted key ED25519 {fp} found at /fixture/authorized_keys_main:1",
                f"Postponed publickey for tester from 172.17.0.1 port {port} ssh2 [preauth]",
                f"Accepted key ED25519 {fp} found at /fixture/authorized_keys_main:1",
                f"Accepted publickey for tester from 172.17.0.1 port {port} ssh2: ED25519 {fp}", "User child is on pid 9",
                f"Starting session: command for tester from 172.17.0.1 port {port} id 0",
                f"Received disconnect from 172.17.0.1 port {port}:11: disconnected by user",
                f"Disconnected from user tester 172.17.0.1 port {port}"]
    if cls == "probe-ok":
        return [head, f"Postponed publickey for tester from 172.17.0.1 port {port} ssh2 [preauth]",
                f"Connection closed by authenticating user tester 172.17.0.1 port {port} [preauth]"]
    if cls == "probe-rejected":
        return [head, f"Failed publickey for tester from 172.17.0.1 port {port} ssh2: ED25519 {FPB}",
                f"Connection closed by authenticating user tester 172.17.0.1 port {port} [preauth]"]
    if cls == "closed":
        return [head, f"Connection closed by 172.17.0.1 port {port} [preauth]"]
    raise AssertionError(cls)


class EvidenceCase(KeysCase):
    """Synthetic evidence directories for the judge, built from the contract tables."""

    def build(self, mode="t0", name="ev"):
        ev = self.tmp / f"{name}-{secrets.token_hex(3)}"
        ev.mkdir()
        fps = {n: e["fp"] for n, e in self.exp["keys"].items()}
        serial = ["[ INFO] [kms] fail-closed node identity service starting",
                  "[ INFO] [kms] DEV_POSTURE ssh-dev-custody: volatile single-identity ssh leaf",
                  "[ INFO] [policy] loaded + verified (59 entries, flags 0x00)", "[ INFO] [selftest] boot-ceiling: PASS (per-path table)",
                  "USER: P04 START"]
        sshd, port = ["chpasswd: password for 'tester' changed", "Server listening on 0.0.0.0 port 22."], 40000
        for marker in J.MARKERS[mode]:
            for attempt in ATTEMPTS_BEFORE[mode].get(marker, []):
                serial.append(f"USER: P04 INFO kssh-attempt {attempt}")
                for cls in J.EXPECTED_CONNECTIONS[mode][attempt]:
                    if cls.startswith("accepted-pk:"):
                        cls = "accepted-pk:" + fps[cls.split(":", 1)[1]]
                    sshd += sshd_lines(cls, port)
                    port += 1
            if marker == "kssh-s01":
                serial.append(f"USER: P04 INFO kssh-obs s01 fp={fps['ka16']} prefix={self.exp['keys']['ka16']['blob_prefix']}")
            if marker == "kssh-s03":
                serial.append("USER: P04 INFO kssh-obs s03 unlock-ms=2345")
            serial.append(f"USER: P04 PASS {marker}")
        serial.append(f"USER: P04-TERMINAL: PASS checks={len(J.MARKERS[mode])}")
        self.write(ev, serial, sshd)
        (ev / "qemu-argv.txt").write_text("qemu-system-x86_64 -machine q35 -accel tcg -netdev user,id=net0 -device e1000,netdev=net0 -drive file=x.img\n")
        (ev / "p09-ledger.txt").write_text("p09x-kssh-login\n" if mode == "t0" else "p09x-kssh-rekey\n")
        (ev / "census-compare.json").write_text(json.dumps({"equal": 470, "missing": 0, "changed": 0, "baseline_present": 470}))
        return ev

    @staticmethod
    def write(ev, serial, sshd):
        (ev / "serial.log").write_text("\n".join(serial) + "\n")
        (ev / "serial.raw.log").write_bytes(("\r\n".join(serial) + "\r\n").encode())
        (ev / "sshd.log").write_text("\n".join(sshd) + "\n")

    def run_judge(self, ev, mode="t0", set_="rt0", extra=()):
        pw, seedf = self.tmp / "fixture-password", self.tmp / "seed.hex"
        if not pw.exists():
            pw.write_text("pw-" + secrets.token_hex(8) + "\n")
            seedf.write_text(secrets.token_hex(32) + "\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            rc = J.main(["judge", "--evidence", str(ev), "--keys", str(self.keys), "--mode", mode, "--set", set_,
                         "--fixture-fp", FP_HOST, "--password-file", str(pw), "--seed-hex-file", str(seedf), *extra])
        files = {n: json.loads((ev / n).read_text()) for n in ("verdict.json", "classification.json", "scan.json", "controls.json")
                 if (ev / n).exists()}
        return rc, out.getvalue(), files

    def edit(self, ev, fn):
        s = (ev / "serial.log").read_text().splitlines()
        h = (ev / "sshd.log").read_text().splitlines()
        s, h = fn(s, h)
        self.write(ev, s, h)


class JudgeTests(EvidenceCase):
    def klass(self, files):
        return files["classification.json"]["class"]

    def test_clean_t0_run_is_class_a(self):
        ev = self.build()
        rc, out, f = self.run_judge(ev)
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertTrue(f["classification.json"]["first_sign_epoch_passed"])
        self.assertEqual(f["classification.json"]["reasons"], [])
        self.assertFalse(f["verdict.json"]["no_evidence_value"])
        self.assertIn("PASS sshd-connection-sequence", out)
        self.assertIn("PASS marker:kssh-s15", out)
        self.assertEqual(f["verdict.json"]["observations"]["unlock_ms"], [2345])
        self.assertEqual(f["controls.json"]["ok"], True)
        self.assertGreater(f["scan.json"]["forms"], 100)
        self.assertEqual(f["verdict.json"]["sshd_connections"][0]["class"], "accepted-pw")
        for t in f["scan.json"]["targets"]:
            self.assertEqual(t["hits"], [])

    def test_clean_f_run_is_class_a(self):
        ev = self.build("f")
        rc, out, f = self.run_judge(ev, "f", "rt0f")
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)

    def test_b0_writes_no_evidence_value_and_tolerates_missing_census_and_ledger(self):
        ev = self.build()
        (ev / "census-compare.json").unlink()
        (ev / "p09-ledger.txt").unlink()
        rc, out, f = self.run_judge(ev, set_="b0")
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertTrue(f["classification.json"]["no_evidence_value"])
        self.assertTrue(f["verdict.json"]["no_evidence_value"])
        self.assertTrue(any("census-not-run" in n for n in f["verdict.json"]["notes"]))

    def test_rt0_census_and_ledger_rules(self):
        ev = self.build()
        (ev / "census-compare.json").unlink()
        rc, out, f = self.run_judge(ev)
        self.assertEqual(rc, 1)
        self.assertIn("FAIL census-drift", out)
        # after the first sign epoch a host (E) reason is a tail STOP: class B
        self.assertEqual(self.klass(f), "B")
        ev = self.build()
        (ev / "census-compare.json").write_text(json.dumps({"equal": 469, "missing": 0, "changed": 1, "baseline_present": 470}))
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["census-drift"]["pass"])
        ev = self.build()
        (ev / "p09-ledger.txt").unlink()
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["remote-ledger"]["pass"])
        ev = self.build()
        (ev / "p09-ledger.txt").write_text("p09x-kssh-login\np09x-kssh-login\n")
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["remote-ledger"]["pass"])
        ev = self.build("f")
        (ev / "p09-ledger.txt").write_text("p09x-kssh-login\n")
        self.assertFalse(self.run_judge(ev, "f", "rt0f")[2]["verdict.json"]["checks"]["remote-ledger"]["pass"])

    def test_policy_shape_absent(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l for l in s if "[policy]" not in l], h))
        rc, out, f = self.run_judge(ev)
        self.assertEqual(self.klass(f), "B")
        self.assertIn("FAIL policy-loaded", out)
        rc, out, f = self.run_judge(ev, extra=["--policy-shape", "absent"])
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertTrue(any("policy-shape absent" in n for n in f["verdict.json"]["notes"]))

    def test_duplicate_missing_and_unexpected_markers(self):
        ev = self.build()
        self.edit(ev, lambda s, h: (s + ["USER: P04 PASS kssh-s03"], h))
        f = self.run_judge(ev)[2]
        self.assertFalse(f["verdict.json"]["checks"]["marker:kssh-s03"]["pass"])
        self.assertEqual(self.klass(f), "B")  # later than the first sign epoch
        ev = self.build()
        self.edit(ev, lambda s, h: (s + ["USER: P04 PASS kssh-s99"], h))
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertFalse(c["no-unexpected-markers"]["pass"])
        ev = self.build()
        self.edit(ev, lambda s, h: ([l for l in s if "PASS kssh-s04a" not in l], h))
        self.assertIn("missing", self.run_judge(ev)[2]["verdict.json"]["checks"]["marker:kssh-s04a"]["detail"])
        ev = self.build()
        self.edit(ev, lambda s, h: ([l for l in s if "PASS kssh-s00" not in l], h))
        rc, out, f = self.run_judge(ev)
        self.assertEqual(self.klass(f), "C")  # before the first sign epoch: core marker missing
        self.assertFalse(f["classification.json"]["first_sign_epoch_passed"])

    def test_marker_order(self):
        ev = self.build()

        def swap(s, h):
            i, j = s.index("USER: P04 PASS kssh-s04a"), s.index("USER: P04 PASS kssh-s04b")
            s[i], s[j] = s[j], s[i]
            return s, h

        self.edit(ev, swap)
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["markers-in-order"]["pass"])

    def test_fail_marker_before_epoch_is_d_and_timeout_is_c(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s02", "FAIL kssh-s02 wrong output", 1) if l.endswith("PASS kssh-s02") else l for l in s], h))
        rc, out, f = self.run_judge(ev)
        self.assertEqual((rc, self.klass(f)), (1, "D"), out)
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s02", "FAIL kssh-s02 timeout waiting for prompt", 1) if l.endswith("PASS kssh-s02") else l for l in s], h))
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "C")

    def test_later_scenario_fail_after_epoch_stays_d(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s08", "FAIL kssh-s08 limiter", 1) if l.endswith("PASS kssh-s08") else l for l in s], h))
        rc, out, f = self.run_judge(ev)
        self.assertEqual((rc, self.klass(f)), (1, "D"), out)
        self.assertTrue(f["classification.json"]["first_sign_epoch_passed"])
        self.assertFalse(f["classification.json"]["reasons"][0].startswith("tail STOP"))

    def test_later_timeout_fail_after_epoch_is_tail_stop_b(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s08", "FAIL kssh-s08 timeout waiting for prompt", 1) if l.endswith("PASS kssh-s08") else l for l in s], h))
        rc, out, f = self.run_judge(ev)
        self.assertEqual((rc, self.klass(f)), (1, "B"), out)
        self.assertTrue(f["classification.json"]["first_sign_epoch_passed"])
        self.assertTrue(f["classification.json"]["reasons"][0].startswith("tail STOP"))

    def test_later_d_with_later_e_is_d(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s08", "FAIL kssh-s08 limiter", 1) if l.endswith("PASS kssh-s08") else l for l in s], h))
        (ev / "census-compare.json").unlink()
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "D")

    def test_precedence_e_over_c_over_d_end_to_end(self):
        ev = self.build()  # D (FAIL s00) + C (missing sshd) + E (qemu forward) all before the epoch
        self.edit(ev, lambda s, h: ([l.replace("PASS kssh-s00", "FAIL kssh-s00 bad") for l in s], h))
        (ev / "sshd.log").unlink()
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "C")
        (ev / "qemu-argv.txt").write_text("qemu -netdev user,id=net0,hostfwd=tcp::22-:22 \n")
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "E")

    def test_missing_sshd_log_is_c_not_e(self):
        ev = self.build()
        (ev / "sshd.log").unlink()
        rc, out, f = self.run_judge(ev)
        self.assertEqual(self.klass(f), "C", out)

    def test_sshd_sequence_mismatch_names_index_and_ids(self):
        ev = self.build()

        def drop_probe(s, h):  # s03's connection becomes a probe-rejected one
            h = [l.replace("Postponed publickey", "Failed publickey", 1) if i == next(
                k for k, x in enumerate(h) if "Postponed" in x and "port 40" in x and k > 0 and sum(1 for y in h[:k] if "Accepted publickey" in y) == 2)
                else l for i, l in enumerate(h)]
            return s, h

        # simpler: delete the whole s04b connection
        def drop(s, h):
            i = next(k for k, l in enumerate(h) if l.startswith("Failed publickey"))
            return s, h[:i - 1] + h[i + 2:]

        self.edit(ev, drop)
        rc, out, f = self.run_judge(ev)
        d = f["verdict.json"]["checks"]["sshd-connection-sequence"]
        self.assertFalse(d["pass"])
        self.assertIn("attempt s04b", d["detail"])
        self.assertIn("expected probe-rejected", d["detail"])
        self.assertEqual(rc, 1)

    def test_unknown_attempt_id_and_unrecognised_wording_are_c(self):
        ev = self.build()
        self.edit(ev, lambda s, h: (s[:5] + ["USER: P04 INFO kssh-attempt s99"] + s[5:], h))
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "C")
        ev = self.build()
        self.edit(ev, lambda s, h: (s, h[:3] + ["Weird publickey line for tester from 172.17.0.1 port 40000 ssh2"] + h[3:]))
        f = self.run_judge(ev)[2]
        self.assertFalse(f["verdict.json"]["checks"]["sshd-wording-recognised"]["pass"])
        self.assertNotEqual(self.klass(f), "A")

    def test_password_fallback_is_flagged(self):
        ev = self.build()

        def fallback(s, h):
            i = next(k for k, l in enumerate(h) if l.startswith("Failed publickey"))
            m = re.search(r"port (\d+)", h[i]).group(1)
            h.insert(i + 1, f"Accepted password for tester from 172.17.0.1 port {m} ssh2")
            return s, h

        self.edit(ev, fallback)
        rc, out, f = self.run_judge(ev)
        c = f["verdict.json"]["checks"]
        self.assertFalse(c["no-password-fallback"]["pass"])
        self.assertFalse(c["no-accepted-password-after-publickey"]["pass"])
        self.assertEqual(rc, 1)

    def test_s06d_acceptance_is_d_after_epoch_it_is_a_tail(self):
        ev = self.build()

        def served(s, h):
            i = next(k for k, l in enumerate(h) if l.startswith("Connection from"))
            fa = self.exp["keys"]["ka16"]["fp"]
            j = next(k for k, l in enumerate(h) if "Accepted publickey" in l)
            k0 = j - 4
            block = h[k0:j + 5]
            port = 49000
            block = [re.sub(r"port \d+", f"port {port}", l) for l in block]
            # place an extra accepted connection right before s02's one (s06d position)
            return s, h[:k0] + block + h[k0:]

        self.edit(ev, served)
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertFalse(c["s06d-no-acceptance"]["pass"])

    def test_fingerprint_and_prefix_observation_compared_with_expected(self):
        ev = self.build()
        self.edit(ev, lambda s, h: ([l.replace("prefix=", "prefix=0", 1).replace("prefix=0" + self.exp["keys"]["ka16"]["blob_prefix"][:1], "prefix=" + "f", 1)
                                      if "kssh-obs s01" in l else l for l in s], h))
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["s01-fingerprint-and-blob-prefix"]["pass"])
        ev = self.build()
        self.edit(ev, lambda s, h: ([l for l in s if "kssh-obs s01" not in l], h))
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]["s01-fingerprint-and-blob-prefix"]
        self.assertFalse(c["pass"])

    def test_banner_weak_entropy_panic_and_qemu(self):
        for mutate, check in (
                (lambda s: [l for l in s if "DEV_POSTURE" not in l], "banner-dev-posture"),
                (lambda s: s + ["[ WARN] serving WEAK xorshift32"], "no-weak-entropy"),
                (lambda s: s + ["kernel PANIC at x"], "no-panic"),
                (lambda s: [l for l in s if "boot-ceiling" not in l], "boot-ceiling-selftest"),
                (lambda s: [l for l in s if "fail-closed" not in l], "kms-identity-service"),
                (lambda s: s + ["USER: P04-TERMINAL: PASS checks=1"], "p04-terminal-line")):
            ev = self.build()
            self.edit(ev, lambda s, h, m=mutate: (m(s), h))
            c = self.run_judge(ev)[2]["verdict.json"]["checks"]
            self.assertFalse(c[check]["pass"], check)
        ev = self.build()
        (ev / "qemu-argv.txt").write_text("qemu -netdev user,id=net0 -netdev user,id=net1 -device e1000,netdev=net0\n")
        self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["qemu-one-user-netdev-no-forward"]["pass"])

    def test_canary_in_serial_is_d_and_final_after_epoch(self):
        for fname in ("serial.raw.log", "serial.log", "sshd.log"):
            ev = self.build()
            with open(ev / fname, "ab") as f:
                f.write(b"USER: leak " + self.sec["keys"]["ka16"]["passphrase"].encode() + b"\n")
            rc, out, f = self.run_judge(ev)
            self.assertEqual((rc, self.klass(f)), (1, "D"), fname)
            self.assertTrue(f["classification.json"]["first_sign_epoch_passed"])
            self.assertIn("pass:ka16", out)
            for p in ev.iterdir():  # nothing the judge wrote or printed holds the secret
                if p.name.endswith(".json"):
                    self.assertNotIn(self.sec["keys"]["ka16"]["passphrase"], p.read_text())
            self.assertNotIn(self.sec["keys"]["ka16"]["passphrase"], out)

    def test_canary_forms_seed_blob_password_trust_disk(self):
        seed = bytes.fromhex(self.sec["keys"]["ka16"]["seed"])
        body = "".join((self.keys / "ka16").read_text().splitlines()[1:-1]).encode()
        pw = (self.tmp / "fixture-password")
        for payload in (seed.hex().encode(), seed, K.base64.b64encode(seed), body, (self.keys / "ka16").read_bytes(),
                        b"[" + ", ".join(str(b) for b in seed).encode() + b"]"):
            ev = self.build()
            self.run_judge(ev)  # creates fixture-password/seed.hex
            with open(ev / "serial.raw.log", "ab") as f:
                f.write(b"junk " + payload + b"\n")
            self.assertEqual(self.klass(self.run_judge(ev)[2]), "D", payload[:20])
        ev = self.build()
        self.run_judge(ev)
        with open(ev / "serial.log", "a") as f:
            f.write("x " + pw.read_text().strip() + "\n")
        self.assertEqual(self.klass(self.run_judge(ev)[2]), "D")
        ev = self.build()
        disk = self.tmp / "trust.img"
        disk.write_bytes(b"\0" * (2 << 20) + seed + b"\0" * 4096)
        rc, out, f = self.run_judge(ev, extra=["--trust-disk", str(disk)])
        self.assertEqual(self.klass(f), "D", out)
        disk.write_bytes(b"\0" * (2 << 20))
        self.assertEqual(self.klass(self.run_judge(ev, extra=["--trust-disk", str(disk)])[2]), "A")

    def test_fixture_host_key_is_a_canary(self):
        ev = self.build()
        hk = self.tmp / "host_ed25519"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(hk)], check=True, stdin=subprocess.DEVNULL)
        rc, out, f = self.run_judge(ev, extra=["--fixture-host-key", str(hk)])
        self.assertEqual(self.klass(f), "A", out)
        with open(ev / "sshd.log", "ab") as fh:
            fh.write(b"x " + hk.read_bytes().splitlines()[1] + b"\n")
        self.assertEqual(self.klass(self.run_judge(ev, extra=["--fixture-host-key", str(hk)])[2]), "D")

    def test_control_failure_is_class_e(self):
        ev = self.build()
        with mock.patch.object(J, "run_controls", return_value={"ok": False}):
            rc, out, f = self.run_judge(ev)
        self.assertEqual((rc, self.klass(f)), (1, "E"), out)
        self.assertFalse(f["controls.json"]["ok"])

    def test_expected_public_strings_required(self):
        ev = self.build()
        self.edit(ev, lambda s, h: (s, [l for l in h if "Accepted publickey" not in l]))
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertFalse(c["scan-expected-public-strings"]["pass"])

    def test_sshd_log_override(self):
        ev = self.build()
        other = self.tmp / "other-sshd.log"
        shutil.move(ev / "sshd.log", other)
        rc, out, f = self.run_judge(ev, extra=["--sshd-log", str(other)])
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)

    def test_mode_set_mismatch_refused(self):
        ev = self.build()
        rc, _, _ = self.run_judge(ev, "t0", "rt0f")
        self.assertEqual(rc, 2)


# ---------------------------------------------------------------------------------- archive

class ArchiveTests(EvidenceCase):
    def setUp(self):
        self.ev = self.build()
        self.run_judge(self.ev)
        (self.ev / "fixture.txt").write_text("fixture: x\n")
        (self.ev / "provenance.sha256").write_text("00  x\n")
        (self.ev / "p09-hooks.json").write_text("[]\n")
        (self.ev / "kssh.conf").write_text(K.build_conf(self.keys, "t0", [1, 2, 3], FP_HOST, "pw-archive-1234", False))
        (self.ev / "seed.hex").write_text("00" * 32 + "\n")
        (self.ev / "disk.img").write_bytes(b"x")
        (self.ev / "boot-config").mkdir()
        (self.ev / "boot-config/a.toml").write_text("x\n")
        self.dst = self.tmp / f"repo-set-{secrets.token_hex(3)}"

    def test_clean_copy_is_allowlisted_and_redacted(self):
        ok, msgs = J.archive(self.ev, self.dst, self.keys)
        self.assertTrue(ok, msgs)
        names = {p.name for p in self.dst.iterdir()}
        for n in ("serial.raw.log", "serial.log", "sshd.log", "verdict.json", "scan.json", "controls.json", "classification.json",
                  "qemu-argv.txt", "fixture.txt", "provenance.sha256", "p09-hooks.json", "p09-ledger.txt", "kssh.conf",
                  "expected.json", "ka16.pub", "kb.pub", "census-compare.json"):
            self.assertIn(n, names)
        for n in ("seed.hex", "disk.img", "boot-config", "ka16", "kl4", "kp0", "secrets.json", "ka16.key"):
            self.assertNotIn(n, names)
        conf = (self.dst / "kssh.conf").read_text()
        self.assertNotIn(self.sec["keys"]["ka16"]["passphrase"], conf)
        self.assertNotIn("pw-archive-1234", conf)
        self.assertEqual(K.parse_conf(conf)["keys"]["ka16"]["passphrase"], "<redacted>")
        self.assertIn(self.exp["keys"]["ka16"]["fp"], conf)

    def test_planted_canary_is_refused_and_dst_deleted(self):
        with open(self.ev / "sshd.log", "ab") as f:
            f.write(b"leak " + self.sec["keys"]["kl4"]["wrong"].encode() + b"\n")
        ok, msgs = J.archive(self.ev, self.dst, self.keys)
        self.assertFalse(ok)
        self.assertFalse(self.dst.exists())
        self.assertIn("wrong:kl4", " ".join(msgs))
        self.assertNotIn(self.sec["keys"]["kl4"]["wrong"], " ".join(msgs))

    def test_extra_secret_file_is_scanned(self):
        extra = self.tmp / "fixture-password"
        extra.write_text("pw-" + secrets.token_hex(8) + "\n")
        with open(self.ev / "serial.log", "a") as f:
            f.write(extra.read_text())
        ok, _ = J.archive(self.ev, self.dst, self.keys, [extra])
        self.assertFalse(ok)
        self.assertFalse(self.dst.exists())

    def test_forbidden_names_refused(self):
        (self.keys / "id_ed25519.pub").write_text(self.exp["keys"]["ka16"]["pub"] + "\n")
        try:
            ok, msgs = J.archive(self.ev, self.dst, self.keys)
        finally:
            (self.keys / "id_ed25519.pub").unlink()
        self.assertFalse(ok)
        self.assertFalse(self.dst.exists())
        self.assertIn("forbidden", " ".join(msgs))

    def test_nonempty_destination_is_refused_and_kept(self):
        self.dst.mkdir()
        (self.dst / "keep.txt").write_text("keep")
        ok, _ = J.archive(self.ev, self.dst, self.keys)
        self.assertFalse(ok)
        self.assertTrue((self.dst / "keep.txt").exists())

    def test_cli_exit_codes(self):
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(J.main(["archive", "--src", str(self.ev), "--dst", str(self.dst), "--keys", str(self.keys)]), 0)
        shutil.rmtree(self.dst)
        with open(self.ev / "serial.raw.log", "ab") as f:
            f.write(self.sec["keys"]["ka16"]["passphrase"].encode())
        with contextlib.redirect_stdout(out):
            self.assertEqual(J.main(["archive", "--src", str(self.ev), "--dst", str(self.dst), "--keys", str(self.keys)]), 1)
        self.assertFalse(self.dst.exists())


# ---------------------------------------------------------------------------------- census

class CensusTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="kssh-census-"))
        self.data = self.tmp / "data"
        self.data.mkdir()
        self.files = []
        for i in range(3):
            p = self.data / f"f{i}.bin"
            p.write_bytes(secrets.token_bytes(1000 + i))
            self.files.append(str(p))
        self.base = self.tmp / "base.json"
        base = J.census_hash(self.files + [str(self.data / "gone")])
        self.base.write_text(json.dumps(base))
        self.assertEqual(len(base["missing"]), 1)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def run_census(self, *extra):
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
            rc = J.main(["census", "--baseline", str(self.base), *extra])
        return rc, out.getvalue()

    def test_format_matches_the_s10_baseline(self):
        real = Path(__file__).resolve().parent.parent / ".agents/261020-1750-cellbox-cli/reports/s10-9-evidence/measurement-runs/cand-headless/census-before.json"
        if real.exists():
            ref = json.loads(real.read_text())
            self.assertEqual(set(ref), {"paths", "method", "present", "missing"})
            self.assertEqual(set(json.loads(self.base.read_text())), set(ref))
            some = next(iter(ref["present"].values()))
            self.assertEqual(set(some), {"size", "sha256"})

    def test_hashes_exactly_the_present_paths_and_compares_equal(self):
        out, cmp = self.tmp / "after.json", self.tmp / "cmp.json"
        rc, text = self.run_census("--out", str(out), "--compare-baseline", str(cmp))
        self.assertEqual(rc, 0, text)
        now = json.loads(out.read_text())
        for p in self.files:
            self.assertEqual(now["present"][p]["sha256"], hashlib.sha256(Path(p).read_bytes()).hexdigest())
            self.assertEqual(now["present"][p]["size"], Path(p).stat().st_size)
        self.assertEqual(set(now["present"]), set(self.files))
        self.assertEqual(json.loads(cmp.read_text()), {"equal": 3, "missing": 0, "changed": 0, "baseline_present": 3})

    def test_changed_and_missing_exit_one(self):
        cmp = self.tmp / "cmp.json"
        Path(self.files[0]).write_bytes(b"changed")
        rc, _ = self.run_census("--compare-baseline", str(cmp))
        self.assertEqual(rc, 1)
        self.assertEqual(json.loads(cmp.read_text()), {"equal": 2, "missing": 0, "changed": 1, "baseline_present": 3})
        os.unlink(self.files[1])
        rc, _ = self.run_census("--compare-baseline", str(cmp))
        self.assertEqual(rc, 1)
        self.assertEqual(json.loads(cmp.read_text()), {"equal": 1, "missing": 1, "changed": 1, "baseline_present": 3})

    def test_refuses_to_write_inside_census_paths_and_writes_nothing_there(self):
        before = sorted(os.listdir(self.data))
        rc, text = self.run_census("--out", str(self.data / "after.json"))
        self.assertEqual(rc, 2, text)
        self.assertEqual(sorted(os.listdir(self.data)), before)

    def test_streaming_hash_large_file(self):
        p = self.tmp / "big"
        with open(p, "wb") as f:
            for _ in range(5):
                f.write(b"a" * (1 << 20))
        h = hashlib.sha256(b"a" * (5 << 20)).hexdigest()
        self.assertEqual(J._sha256_file(p, bufsize=1 << 18), {"size": 5 << 20, "sha256": h})


if __name__ == "__main__":
    unittest.main(verbosity=1)
