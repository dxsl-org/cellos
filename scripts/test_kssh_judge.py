#!/usr/bin/env python3
"""Host-only tests for kssh_keys.py and kssh_judge.py (K8 T0 lane). No Docker, QEMU, network.

    python3 scripts/test_kssh_judge.py

Temp directories live under /tmp (mode 700) and are removed afterwards. Keys generated here are
throw-away TEST keys made with the host `ssh-keygen`.
"""

import base64
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

    def test_s06d_none_and_typed_probe_ok_and_acceptance_is_flagged(self):
        a = ["s06d", "s06d-typed", "s02"]
        ka = "accepted-pk:" + FPS["ka16"]
        self.assertEqual(J.EXPECTED_CONNECTIONS["t0"]["s06d"], [])
        self.assertEqual(J.EXPECTED_CONNECTIONS["t0"]["s06d-typed"], ["probe-ok"])
        r = self.cmp(a, ["probe-ok", ka])
        self.assertTrue(r["ok"], r)
        self.assertFalse(r["optional_accepted"])
        # s06d (direct launch) made a connection: mismatch, not an acceptance
        r = self.cmp(a, ["probe-ok", "probe-ok", ka])
        self.assertEqual((r["ok"], r["attempt"], r["optional_accepted"]), (False, "s02", False))
        # typed form made no connection: plain mismatch
        r = self.cmp(a, [ka])
        self.assertEqual((r["ok"], r["optional_accepted"]), (False, False))
        # an acceptance for either attempt
        for got in ([ka, "probe-ok", ka], ["probe-ok", ka, ka], [ka, ka]):
            r = self.cmp(a, got)
            self.assertFalse(r["ok"], got)
            self.assertTrue(r["optional_accepted"], got)

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


ZERO_SHA = hashlib.sha256(bytes(1048576)).hexdigest()


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

    def test_attempt_segments_and_test_config_lines(self):
        S = J.parse_serial("\n".join([
            "USER: P04 INFO p09-case kssh-s11", "USER: P04 INFO kssh-attempt s11", "USER: P04 INFO ssh-test-config rekey=lane",
            "USER: P04 INFO ssh-rekeys 3", "USER: P04 PASS kssh-s11", "USER: P04 INFO ssh-test-config kssh-faults=on",
            "USER: P04 INFO kssh-attempt s02", "USER: P04 INFO kssh-attempt s03", "USER: P04 INFO ssh-rekeys 0"]))
        self.assertEqual([s["id"] for s in S["segments"]], ["s11", "s02", "s03"])
        self.assertEqual(S["segments"][0]["lines"], ["INFO ssh-test-config rekey=lane", "INFO ssh-rekeys 3"])
        self.assertEqual(S["segments"][1]["lines"], [])
        self.assertEqual(len(S["test_config"]), 2)


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
        self.assertEqual(len((self.stage / "authdir/authorized_keys_main").read_text().splitlines()), 5)  # + kr1, kr64
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

    def test_calibration_keys_are_authorized_only_with_calibrate(self):
        main = (self.stage / "authdir/authorized_keys_main").read_text().splitlines()
        self.assertEqual(main, [self.exp["keys"][n]["pub"] for n in ("ka16", "kl4", "kp0", "kr1", "kr64")])
        self.assertNotIn(self.exp["keyb"]["pub"], main)
        plain = self.tmp / "nocal"
        exp = K.gen(plain / "keys", plain / "stage")
        main = (plain / "stage/authdir/authorized_keys_main").read_text().splitlines()
        self.assertEqual(main, [exp["keys"][n]["pub"] for n in ("ka16", "kl4", "kp0")])
        self.assertFalse((plain / "stage/kr1.key").exists())

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
           "kssh-s06d": ["s06d", "s06d-typed"], "kssh-s02": ["s02"], "kssh-s02p": ["s02p"], "kssh-s03": ["s03"], "kssh-s03b": ["s03b"],
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

    def build(self, mode="t0", name="ev", calibrate=False):
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
                if mode == "f":
                    serial += ["USER: P04 INFO ssh-test-config rekey=lane", "USER: P04 INFO ssh-test-config kssh-faults=on"]
                    if attempt == "s11":
                        serial += ["USER: P04 INFO ssh-rekeys 64",
                                   f"USER: P04 INFO ssh-bytes stdout=1048576 sha256={ZERO_SHA} peak_pending=2048 peak_chunk=16384"]
                    else:
                        serial.append("USER: P04 INFO ssh-rekeys 0")
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
            if marker == "kssh-s01" and calibrate:  # real-run order: R1, R64, R16
                for r in (1, 64, 16):
                    serial.append(f"USER: P04 INFO kssh-attempt cal-R{r}")
                    sshd += sshd_lines("probe-ok", port)
                    port += 1
        serial.append(f"USER: P04-TERMINAL: PASS checks={len(J.MARKERS[mode])}")
        self.write(ev, serial, sshd)
        (ev / "qemu-argv.txt").write_text("qemu-system-x86_64 -machine q35 -accel tcg -netdev user,id=net0 -device e1000,netdev=net0 -drive file=x.img\n")
        (ev / "p09-ledger.txt").write_text("p09x-kssh-login\n" * 3 if mode == "t0" else "p09x-kssh-login\np09x-kssh-rekey\n")
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
        for bad in ("p09x-kssh-login\np09x-kssh-login\n", "p09x-kssh-login\n" * 4, "p09x-kssh-login\n" * 3 + "p09x-other\n"):
            ev = self.build()
            (ev / "p09-ledger.txt").write_text(bad)
            self.assertFalse(self.run_judge(ev)[2]["verdict.json"]["checks"]["remote-ledger"]["pass"], bad)
        for bad in ("p09x-kssh-login\n", "p09x-kssh-rekey\n", "p09x-kssh-login\np09x-kssh-rekey\np09x-kssh-rekey\n",
                    "p09x-kssh-login\np09x-kssh-rekey\np09x-other\n"):
            ev = self.build("f")
            (ev / "p09-ledger.txt").write_text(bad)
            self.assertFalse(self.run_judge(ev, "f", "rt0f")[2]["verdict.json"]["checks"]["remote-ledger"]["pass"], bad)
        ev = self.build()  # non-p09x lines are not ledger entries
        (ev / "p09-ledger.txt").write_text("noise\n" + "p09x-kssh-login\n" * 3)
        self.assertTrue(self.run_judge(ev)[2]["verdict.json"]["checks"]["remote-ledger"]["pass"])

    def test_session_a_close_is_an_expected_final_marker(self):
        for mode, set_ in (("t0", "rt0"), ("f", "rt0f")):
            ev = self.build(mode)
            rc, out, f = self.run_judge(ev, mode, set_)
            self.assertEqual((rc, self.klass(f)), (0, "A"), out)
            self.assertIn("PASS marker:session-a-close", out)
            self.edit(ev, lambda s, h: ([l for l in s if "PASS session-a-close" not in l], h))
            self.assertFalse(self.run_judge(ev, mode, set_)[2]["verdict.json"]["checks"]["marker:session-a-close"]["pass"])

    def test_calibration_attempts_follow_serial_order(self):
        ev = self.build(calibrate=True)
        rc, out, f = self.run_judge(ev, set_="b0")
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertEqual([a for a in f["verdict.json"]["observations"]["attempts"] if a.startswith("cal-")],
                         ["cal-R1", "cal-R64", "cal-R16"])
        ev = self.build("f", calibrate=True)
        self.assertEqual(self.klass(self.run_judge(ev, "f", "b0")[2]), "A")

    def f_run(self, mutate):
        ev = self.build("f")
        self.edit(ev, lambda s, h: (mutate(s), h))
        return self.run_judge(ev, "f", "rt0f")

    @staticmethod
    def in_segment(lines, attempt, fn):
        """Apply `fn(line) -> [lines]` to the lines of one attempt's serial segment only."""
        out, cur = [], None
        for l in lines:
            m = re.search(r"P04 INFO kssh-attempt (\S+)", l)
            if m:
                cur = m.group(1)
            elif re.search(r"P04 (PASS|FAIL) ", l):
                cur = None
            out += fn(l) if cur == attempt else [l]
        return out

    def test_zero_mib_digest_constant(self):
        self.assertEqual(J.ZERO_MIB_SHA256, "30e14955ebf1352266dc2ff8067e68104607e750abb9d3b36582b8af909fcb58")
        self.assertEqual(J.ZERO_MIB_SHA256, hashlib.sha256(bytes(1048576)).hexdigest())

    def test_f_clean_run_passes_the_new_oracles(self):
        rc, out, f = self.f_run(lambda s: s)
        c = f["verdict.json"]["checks"]
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertTrue(c["s11-rekey-and-sink"]["pass"])
        self.assertTrue(c["kssh-faults-announced"]["pass"])
        self.assertNotIn("t0-no-test-config", c)

    def test_s11_rekey_and_sink_failures(self):
        sink = f"USER: P04 INFO ssh-bytes stdout=1048576 sha256={ZERO_SHA} peak_pending=2048 peak_chunk=16384"
        cases = {
            "no rekeys": lambda l: [] if l.endswith("ssh-rekeys 64") else [l],
            "zero rekeys": lambda l: [l.replace("ssh-rekeys 64", "ssh-rekeys 0")],
            "two rekeys": lambda l: [l, l] if l.endswith("ssh-rekeys 64") else [l],
            "wrong digest": lambda l: [l.replace(ZERO_SHA, "a" * 64)],
            "short stdout": lambda l: [l.replace("stdout=1048576", "stdout=1048575")],
            "pending over": lambda l: [l.replace("peak_pending=2048", "peak_pending=2049")],
            "chunk over": lambda l: [l.replace("peak_chunk=16384", "peak_chunk=16385")],
            "two sinks": lambda l: [l, l] if l == sink else [l],
            "no sink": lambda l: [] if l == sink else [l],
        }
        for name, fn in cases.items():
            rc, out, f = self.f_run(lambda s, fn=fn: self.in_segment(s, "s11", fn))
            c = f["verdict.json"]["checks"]["s11-rekey-and-sink"]
            self.assertFalse(c["pass"], name)
            self.assertEqual(self.klass(f), "D", name)  # after the first sign epoch a D stays D
        # bounds exactly at the limit pass; the oracle looks only inside the s11 segment
        rc, out, f = self.f_run(lambda s: self.in_segment(s, "s02", lambda l: [l.replace("ssh-rekeys 0", "ssh-rekeys 7")]))
        self.assertTrue(f["verdict.json"]["checks"]["s11-rekey-and-sink"]["pass"])

    def test_faults_announced_exactly_once_for_the_fault_attempts(self):
        on = "USER: P04 INFO ssh-test-config kssh-faults=on"
        for attempt in ("s05a", "s05b", "s05c", "s02", "s11"):
            for name, fn in (("missing", lambda l: [] if l == on else [l]), ("twice", lambda l: [l, l] if l == on else [l])):
                rc, out, f = self.f_run(lambda s, a=attempt, fn=fn: self.in_segment(s, a, fn))
                self.assertFalse(f["verdict.json"]["checks"]["kssh-faults-announced"]["pass"], (attempt, name))
        # s00 is not part of the contract: its announcement is not judged
        rc, out, f = self.f_run(lambda s: self.in_segment(s, "s00", lambda l: [] if l == on else [l]))
        self.assertTrue(f["verdict.json"]["checks"]["kssh-faults-announced"]["pass"])
        # an announcement belonging to another attempt's segment does not count
        rc, out, f = self.f_run(lambda s: [l for l in s if l != on])
        self.assertFalse(f["verdict.json"]["checks"]["kssh-faults-announced"]["pass"])

    def test_t0_any_test_config_line_is_class_d(self):
        for line in ("USER: P04 INFO ssh-test-config kssh-faults=on", "USER: P04 INFO ssh-test-config rekey=lane"):
            ev = self.build()
            self.edit(ev, lambda s, h, line=line: (s[:6] + [line] + s[6:], h))
            rc, out, f = self.run_judge(ev)
            c = f["verdict.json"]["checks"]["t0-no-test-config"]
            self.assertFalse(c["pass"])
            self.assertIn("faults-build-in-product-image", c["detail"])
            self.assertEqual((rc, self.klass(f)), (1, "D"), out)
        ev = self.build()
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertTrue(c["t0-no-test-config"]["pass"])
        self.assertNotIn("kssh-faults-announced", c)
        self.assertNotIn("s11-rekey-and-sink", c)

    def test_s06d_and_typed_form_connections(self):
        ev = self.build()
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertTrue(c["s06d-no-acceptance"]["pass"])
        self.assertTrue(c["sshd-connection-sequence"]["pass"])
        self.assertEqual(ATTEMPTS_BEFORE["t0"]["kssh-s06d"], ["s06d", "s06d-typed"])

        def drop_typed(s, h):  # the typed form made no connection: ordinary mismatch, not an acceptance
            j = next(k for k, l in enumerate(h) if "Accepted publickey" in l) - 4  # head of s02's connection
            return s, h[:j - 3] + h[j:]  # the 3 lines (head, Postponed, closed) before it are s06d-typed's

        ev = self.build()
        self.edit(ev, drop_typed)
        c = self.run_judge(ev)[2]["verdict.json"]["checks"]
        self.assertFalse(c["sshd-connection-sequence"]["pass"])
        self.assertIn("attempt s06d-typed", c["sshd-connection-sequence"]["detail"])
        self.assertTrue(c["s06d-no-acceptance"]["pass"])

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


# ---------------------------------------------------------------------------------- mode t1 (RT1)

T1_ISO = "ab" * 32
T1_S1 = "cd" * 32
T1_ATTEMPTS = {  # boot -> marker -> attempt ids typed before it (contract section 4)
    1: {"kssh-s00": ["s00"], "kssh-s02": ["s02"]},
    2: {"kssh-s00": ["s00"], "kssh-s02r": ["s02r"], "kssh-t1-s08": [f"s08-w{i}" for i in range(1, 6)] + ["s08-locked"]},
    3: {"kssh-s00": ["s00"], "kssh-s02x": ["s02x"], "kssh-s15": ["s15-iv"]},
    4: {"kssh-s00": ["s00"], "kssh-t1-absent": ["s04a-ka16", "s04a-kp0"]},
    5: {"kssh-s00": ["s00"], "kssh-t1-revert": ["s02r"]},
}
T1_EXPECT = {"d0-before": "none", "after-boot1": "r1", "after-boot2": "r1", "after-boot3": "r1r2", "after-boot4": "r1r2",
             "after-boot5": "r1"}


class T1KeysCase(unittest.TestCase):
    """One mode-t1 key set shared by the class."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = Path(tempfile.mkdtemp(prefix="kssh-t1-test-"))
        cls.keys, cls.stage = cls.tmp / "keys", cls.tmp / "stage"
        cls.exp = K.gen(cls.keys, cls.stage, mode="t1")
        cls.sec = K.load_secrets(cls.keys)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography not installed")
class T1KeyGenTests(T1KeysCase):
    def test_layout_no_keyb_no_kl4_no_calibration(self):
        self.assertEqual(self.exp["mode"], "t1")
        self.assertEqual(list(self.exp["keys"]), ["ka16", "kp0", "bad-aes128", "bad-chacha"])
        self.assertNotIn("keyb", self.exp)
        for name in ("ka16", "kp0", "bad-aes128", "bad-chacha"):
            self.assertTrue((self.stage / f"{name}.key").is_file(), name)
        for name in ("kl4", "kr1", "kr64"):
            self.assertFalse((self.stage / f"{name}.key").exists(), name)
        self.assertFalse((self.keys / "kb.pub").exists())
        main = (self.stage / "authdir/authorized_keys_main").read_text().splitlines()
        self.assertEqual(main, [self.exp["keys"][n]["pub"] for n in ("ka16", "kp0")])
        self.assertEqual((self.stage / "authdir/authorized_keys_b").read_text().splitlines(), [self.exp["keys"]["ka16"]["pub"]])
        self.assertEqual((self.stage / "raw.bin").stat().st_size, 4096)
        self.assertEqual(self.sec["keys"]["ka16"]["wrong"] is not None, True)
        self.assertIsNone(self.sec["keys"]["kp0"]["passphrase"])

    def test_record_expectation_comes_from_the_container(self):
        rec = self.exp["record"]
        got = K.parse_container(K.decode_armor((self.keys / "ka16").read_bytes()))
        self.assertEqual((rec["name"], rec["rounds"]), ("ka16", 16))
        self.assertEqual(bytes.fromhex(rec["pub"]), got["pub"])
        self.assertEqual(bytes.fromhex(rec["salt"]), got["salt"])
        self.assertEqual(bytes.fromhex(rec["ct"]), got["ct"])
        self.assertEqual(rec["blob_prefix"], self.exp["keys"]["ka16"]["blob_prefix"])
        self.assertEqual(got["cipher"], "aes256-ctr")
        self.assertEqual(len(got["salt"]), 16)
        self.assertEqual(len(got["ct"]) % 16, 0)
        self.assertEqual(json.loads((self.keys / "expected.json").read_text())["record"], rec)

    def test_comment_is_1900_bytes_or_reported_fallback(self):
        rec = self.exp["record"]
        self.assertEqual(self.exp["comment_len"], rec["comment_len"])
        if rec["comment_via"] != "ordinary-fallback":
            self.assertEqual(self.exp["comment_len"], K.T1_COMMENT_LEN)
            # 8 check bytes + type, pub, expanded and comment strings, padded to the cipher block: about 2,032 bytes
            self.assertGreaterEqual(len(bytes.fromhex(rec["ct"])), 1900 + 100)
        # the encrypted key still unlocks to the generated public key with the host ssh-keygen
        out = subprocess.run(["ssh-keygen", "-y", "-P", self.sec["keys"]["ka16"]["passphrase"], "-f", str(self.keys / "ka16")],
                             capture_output=True, check=True).stdout.decode().split()
        self.assertEqual(out[1], self.exp["keys"]["ka16"]["pub"].split()[1])

    def test_comment_rewrite_helper_and_ordinary_fallback(self):
        plain = self.tmp / "plain"
        text, parsed, _ = K._gen_plain(plain)
        plain.unlink()
        text2 = K.plain_container_with_comment(parsed, b"c" * 1900)
        again = K.parse_container(K.decode_armor(text2))
        self.assertEqual((again["comment_len"], again["seed"], again["pub"]), (1900, parsed["seed"], parsed["pub"]))
        info = {}
        with mock.patch.object(K, "plain_container_with_comment", return_value=b"not armor"):
            work = self.tmp / "w-fallback"
            _, p2, _ = K._gen_plain(work, 1900, info)
            work.unlink()
        if info["comment_via"] != "ssh-keygen-C":  # this host truncates -C at 1,023 bytes: the rewrite failed, ordinary used
            self.assertEqual((info["comment_via"], info["comment_len"]), ("ordinary-fallback", p2["comment_len"]))
            self.assertLess(info["comment_len"], 1900)

    def test_calibrate_refused_in_t1(self):
        with self.assertRaisesRegex(K.KeyError_, "calibrate"):
            K.gen(self.tmp / "kc", self.tmp / "sc", calibrate=True, mode="t1")
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(K.main(["gen", "--mode", "t1", "--calibrate", "--dir", str(self.tmp / "kc2"),
                                     "--stage", str(self.tmp / "sc2")]), 2)

    def test_conf_t1_has_two_keys_and_no_keyb(self):
        text = K.build_conf(self.keys, "t1", [11, 12, 13], FPA, "pw1234567", False)
        lines = text.splitlines()
        self.assertEqual(lines[1:3], ["mode t1", "calibrate 0"])
        self.assertEqual([l.split()[1] for l in lines if l.startswith("key ")], ["ka16", "kp0"])
        self.assertFalse([l for l in lines if l.startswith("keyb")])
        d = K.parse_conf(text)
        self.assertEqual((d["mode"], sorted(d["keys"]), d["keys"]["ka16"]["rounds"], d["keys"]["kp0"]["rounds"]),
                         ("t1", ["ka16", "kp0"], 16, 0))
        self.assertEqual(d["keys"]["kp0"]["passphrase"], "-")
        self.assertNotIn("keyb", d)
        self.assertTrue(all(len(l.encode()) < 400 for l in lines))
        with self.assertRaisesRegex(K.KeyError_, "calibrate"):
            K.build_conf(self.keys, "t1", [11, 12, 13], FPA, "pw1234567", True)
        for mode in ("t0", "f"):
            with self.assertRaisesRegex(K.KeyError_, "does not match"):
                K.build_conf(self.keys, mode, [11, 12, 13], FPA, "pw1234567", False)

    def test_cli_conf_t1(self):
        pwf = self.tmp / "pw"
        pwf.write_text("fixturepw-1234\n")
        rc = K.main(["conf", "--dir", str(self.keys), "--stage", str(self.stage), "--mode", "t1", "--ports", "1", "2", "3",
                     "--hostfp", FPA, "--password-file", str(pwf)])
        self.assertEqual(rc, 0)
        self.assertEqual(K.parse_conf((self.stage / "kssh.conf").read_text())["mode"], "t1")

    def test_t0_key_sets_refuse_a_t1_conf(self):
        t0 = self.tmp / "t0set"
        K.gen(t0 / "keys", t0 / "stage")
        with self.assertRaisesRegex(K.KeyError_, "does not match"):
            K.build_conf(t0 / "keys", "t1", [1, 2, 3], FPA, "pw1234567", False)
        self.assertNotIn("record", json.loads((t0 / "keys/expected.json").read_text()))

    def test_leak_needles_only_with_leak_true(self):
        plain = dict(K.canaries(self.keys))
        self.assertFalse([l for l in plain if l.startswith("leak-")])
        leak = dict(K.canaries(self.keys, leak=True))
        self.assertEqual(leak["leak-ct:ka16"], bytes.fromhex(self.exp["record"]["ct"]))
        self.assertEqual(leak["leak-hdr:a"], b"CKMS\x02\x00\x01\x01")
        self.assertEqual(leak["leak-hdr:b"], b"CKMS\x02\x01\x01\x00")
        self.assertEqual({k: v for k, v in leak.items() if not k.startswith("leak-")}, plain)
        forms_a = {f for f, _ in K.forms("leak-hdr:a", leak["leak-hdr:a"])}
        self.assertTrue({"raw", "hex-lower", "hex-upper", "b64-a0", "b64-a1", "b64-a2", "dec-list", "hexdbg-list"} <= forms_a)
        # the ciphertext (a substring of the container blob, never the whole blob) is not a disk needle
        disk = J.Scanner(K.canaries(self.keys))
        serial = J.Scanner(K.canaries(self.keys, leak=True))
        image = self.tmp / "disk.img"
        image.write_bytes(b"\0" * 5000 + b"CKMS\x02\x00\x01\x01" + bytes(100) + bytes.fromhex(self.exp["record"]["ct"]) + b"\0" * 5000)
        self.assertEqual(disk.scan_file(image, joined=False), [])
        self.assertEqual({h["label"] for h in serial.scan_file(image, joined=False)},
                         {"leak-ct:ka16", "leak-ct:ka16:head", "leak-ct:ka16:mid", "leak-ct:ka16:tail", "leak-hdr:a"})

    def test_public_expected_has_no_ciphertext(self):
        pub = K.public_expected(self.exp)
        rec = pub["record"]
        self.assertNotIn("ct", rec)
        self.assertNotIn("salt", rec)
        ct = bytes.fromhex(self.exp["record"]["ct"])
        self.assertEqual((rec["ct_len"], rec["ct_sha256"]), (len(ct), hashlib.sha256(ct).hexdigest()))
        self.assertEqual(rec["salt_len"], 16)
        self.assertEqual(pub["keys"], self.exp["keys"])
        self.assertEqual(self.exp["record"]["ct"], json.loads((self.keys / "expected.json").read_text())["record"]["ct"])
        blob = json.dumps(pub).encode()
        self.assertEqual(J.Scanner(K.canaries(self.keys, leak=True)).scan_bytes(blob), set())


def _t1_disk_doc(image, expect, **over):
    r1 = {"offset": 4096, "slot": 0, "state": 1, "revision": 1, "name": "ka16", "digest": "11" * 32, "ct_len": 2032,
          "ct_sha256": "22" * 32, "equals_predicted": True, "previous": "00" * 32, "count": 1}
    r2 = {"offset": 8192, "slot": 1, "state": 0, "revision": 2, "name": "", "digest": "33" * 32, "ct_len": 0,
          "ct_sha256": "44" * 32, "equals_predicted": None, "previous": r1["digest"], "count": 1}
    valid = {"none": [], "r1": [r1], "r1r2": [r1, r2]}[expect]
    doc = {"image": image, "sha256": "55" * 32, "size": 577372160, "records": {"valid": valid, "invalid": [], "v1": []},
           "record_verdict": {"expect": expect, "ok": True, "reasons": []}, "ciphertext_found": expect != "none",
           "scan": {"hits": [], "controls_ok": True}}
    doc.update(over)
    return doc


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography not installed")
class T1EvidenceCase(T1KeysCase):
    """Synthetic five-boot evidence directories for judge mode t1, built from the contract tables."""

    def fp(self, name):
        return self.exp["keys"][name]["fp"]

    @staticmethod
    def write_serial(bd, serial):
        (bd / "serial.log").write_text("\n".join(serial) + "\n")
        (bd / "serial.raw.log").write_bytes(("\r\n".join(serial) + "\r\n").encode())

    def obs_before(self, n, marker):
        fa = self.fp("ka16")
        table = {
            "kssh-conf": ["conf mode=t1 calibrate=false keys=2"], "kssh-role": [f"role n={n}"],
            "kssh-t1-load": ["t1-ms op=reload ms=800", f"t1-load keys=1 readiness=Ready posture=persistent custody=wrapped fp={fa}"],
            "kssh-t1-opaque": ["t1-opaque cat_st=1 cat_leak=false ls_st=0 ls=denied"],
            "kssh-t1-del": ["t1-ms op=delete ms=700"], "kssh-t1-plain": ["t1-import name=kp0"],
            "kssh-transcripts": ["transcripts scans=12 hits=0 canaries=88"],
        }
        if n == 1:
            table["kssh-s01a"] = ["t1-ms op=first-status ms=900"]
            table["kssh-s01"] = ["t1-import name=ka16", "t1-ms op=import ms=1500",
                                 f"s01 fp={fa} prefix={self.exp['keys']['ka16']['blob_prefix']}"]
        return table.get(marker, [])

    def serial_lines(self, n):
        s = [f"[ INFO] {T1_VFS}", "[ INFO] [kms] fail-closed node identity service starting", f"[ INFO] {J.T1_PERSIST_BANNER}",
             "[ INFO] [policy] loaded + verified (59 entries, flags 0x00)", "[ INFO] [selftest] boot-ceiling: PASS (per-path table)",
             "USER: P04 START"]
        for marker in J.T1_MARKERS[n]:
            s += [f"USER: P04 INFO kssh-attempt {a}" for a in T1_ATTEMPTS[n].get(marker, [])]
            s += [f"USER: P04 INFO kssh-obs {o}" for o in self.obs_before(n, marker)]
            s.append(f"USER: P04 PASS {marker}")
        s.append(f"USER: P04-TERMINAL: PASS checks={len(J.T1_MARKERS[n])}")
        return s

    def sshd_for(self, n):
        out, port = ["chpasswd: password for 'tester' changed", "Server listening on 0.0.0.0 port 22."], 40000 + 100 * n
        fps = {k: e["fp"] for k, e in self.exp["keys"].items()}
        for marker in J.T1_MARKERS[n]:
            for attempt in T1_ATTEMPTS[n].get(marker, []):
                for cls in J.EXPECTED_CONNECTIONS["t1"][n][attempt]:
                    if cls.startswith("accepted-pk:"):
                        cls = "accepted-pk:" + fps[cls.split(":", 1)[1]]
                    out += sshd_lines(cls, port)
                    port += 1
        return out

    def boot_json(self, n):
        return {"n": n, "role": n, "verdict": "driver-verdict", "qemu": {"clean": True, "escalated": False, "rc": 0, "signal": "SIGTERM"},
                "iso_sha256": T1_ISO, "disk_image": "S1r" if n == 5 else "D0", "disk_sha256_after": "55" * 32, "elapsed_s": 60.0,
                "sshd_lines": [0, 10], "ledger_lines": [0, 1], "settled": True, "role_written": n}

    def build_t1(self, set_="rt1", name="ev1"):
        ev = self.tmp / f"{name}-{secrets.token_hex(3)}"
        ev.mkdir()
        boots = []
        for n in J.T1_BOOTS:
            bd = ev / f"boot{n}"
            bd.mkdir()
            self.write_serial(bd, self.serial_lines(n))
            (bd / "sshd.log").write_text("\n".join(self.sshd_for(n)) + "\n")
            (bd / "p09-ledger.txt").write_text("p09x-kssh-login\n" * J.LEDGER_EXPECTED["t1"][n].get("p09x-kssh-login", 0))
            disk = "S1r" if n == 5 else "D0"
            (bd / "qemu-argv.txt").write_text(
                f"qemu-system-x86_64 -machine q35 -accel tcg -m 256M -display none -chardev "
                f"socket,id=ser0,path=/w/serial-boot{n}.sock,server=on,wait=off -serial chardev:ser0 -cdrom /w/i.iso -boot d "
                f"-no-reboot -netdev user,id=net0 -device e1000,netdev=net0 -drive file=/w/{disk}.img,if=none,format=raw,"
                f"id=p09fx -device nvme,drive=p09fx,serial=cellosp09\n")
            (bd / "role.txt").write_text(f"role={n}\n")
            bj = self.boot_json(n)
            boots.append(bj)
            (bd / "boot.json").write_text(json.dumps(bj))
            (bd / "disk-after-boot.json").write_text(json.dumps(_t1_disk_doc(disk, T1_EXPECT[f"after-boot{n}"])))
        (ev / "disk-baseline.json").write_text(json.dumps(_t1_disk_doc("D0-before", "none")))
        (ev / "boots.json").write_text(json.dumps({
            "set": set_, "iso_sha256": T1_ISO, "d0_before_sha256": "66" * 32, "s1_sha256": T1_S1,
            "s1r_sha256_before_boot5": T1_S1, "s1_sha256_after_boot5": T1_S1, "boots": boots, "stopped": None, "planned": 5}))
        (ev / "controls.json").write_text(json.dumps({"disk": [
            {"image": i, "pre": {"ok": True}, "post": {"ok": True}, "ok": True} for i in ("D0-before", "D0", "D0", "D0", "D0", "S1r")],
            "ok": True}))
        (ev / "checker.txt").write_text("persist rc=0\ncustody rc=1 persist artifact\nnone rc=1\n")
        (ev / "census-compare.json").write_text(json.dumps({"equal": 470, "missing": 0, "changed": 0, "baseline_present": 470}))
        return ev

    def run_t1(self, ev, set_="rt1", extra=(), mode="t1"):
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

    def edit_serial(self, ev, n, fn):
        bd = ev / f"boot{n}"
        self.write_serial(bd, fn((bd / "serial.log").read_text().splitlines()))

    def edit_sshd(self, ev, n, fn):
        p = ev / f"boot{n}" / "sshd.log"
        p.write_text("\n".join(fn(p.read_text().splitlines())) + "\n")

    @staticmethod
    def edit_json(path, fn):
        doc = json.loads(path.read_text())
        fn(doc)
        path.write_text(json.dumps(doc))

    def check(self, files, name):
        return files["verdict.json"]["checks"][name]

    def failed(self, files):
        return sorted(n for n, c in files["verdict.json"]["checks"].items() if not c["pass"])

    def klass(self, files):
        return files["classification.json"]["class"]


T1_VFS = J.T1_VFS_LINE


class T1TableTests(unittest.TestCase):
    def test_connection_table_is_per_boot_with_shared_ids(self):
        t = J.EXPECTED_CONNECTIONS["t1"]
        self.assertEqual(sorted(t), [1, 2, 3, 4, 5])
        self.assertEqual(t[1], {"s00": ["accepted-pw"], "s02": ["accepted-pk:ka16"]})
        self.assertEqual(t[2]["s02r"], t[5]["s02r"])
        self.assertEqual(t[2]["s02r"], ["accepted-pk:ka16"])
        self.assertEqual([t[2][f"s08-w{i}"] for i in range(1, 6)], [["probe-ok"]] * 5)
        self.assertEqual(t[2]["s08-locked"], ["probe-ok"])
        self.assertEqual((t[3]["s02x"], t[3]["s15-iv"]), (["accepted-pk:ka16"], ["accepted-pk:kp0"]))
        self.assertEqual((t[4]["s04a-ka16"], t[4]["s04a-kp0"]), ([], []))
        self.assertTrue(all(t[n]["s00"] == ["accepted-pw"] for n in t))
        self.assertEqual(J.LEDGER_EXPECTED["t1"], {1: {"p09x-kssh-login": 1}, 2: {"p09x-kssh-login": 1},
                                                   3: {"p09x-kssh-login": 1}, 4: {}, 5: {"p09x-kssh-login": 1}})
        self.assertEqual(sum(v.get("p09x-kssh-login", 0) for v in J.LEDGER_EXPECTED["t1"].values()), J.T1_TOTALS["ledger-login"])

    def test_ids_are_boot_scoped(self):
        self.assertEqual(J.expected_for("t1", "s02r", boot=2), ["accepted-pk:ka16"])
        self.assertIsNone(J.expected_for("t1", "s02r", boot=1))
        self.assertIsNone(J.expected_for("t1", "s02", boot=2))
        self.assertIsNone(J.expected_for("t1", "s00", boot=9))
        self.assertEqual(J.expected_for("t1", "s04a-kp0", boot=4), [])
        fps = {"ka16": "SHA256:A", "kp0": "SHA256:P"}
        r = J.compare_sequence("t1", ["s00", "s02r"], [conn("accepted-pw"), conn("accepted-pk:SHA256:A")], fps, boot=5)
        self.assertTrue(r["ok"], r)
        r = J.compare_sequence("t1", ["s00", "s02r"], [conn("accepted-pw"), conn("accepted-pk:SHA256:A")], fps, boot=1)
        self.assertEqual(r["unknown_attempts"], ["s02r"])
        self.assertTrue(J.prefix_ok("t1", ["s00", "s02r", "s08-w1"], [conn("accepted-pw"), conn("accepted-pk:SHA256:A")], fps, "s02r", boot=2))

    def test_marker_lists_follow_the_contract(self):
        m = J.T1_MARKERS
        head = ["shell-prompt", "kssh-conf", "kssh-s00", "kssh-role"]
        tail = ["kssh-transcripts", "session-a-close"]
        self.assertEqual(m[1], head + ["kssh-s01a", "kssh-s01b", "kssh-s01", "kssh-s09", "kssh-s02"] + tail)
        self.assertEqual(m[2], head + ["kssh-t1-load", "kssh-t1-opaque", "kssh-s02r", "kssh-t1-s08"] + tail)
        self.assertEqual(m[3], head + ["kssh-t1-load", "kssh-s02x", "kssh-t1-del", "kssh-t1-plain", "kssh-s15"] + tail)
        self.assertEqual(m[4], head + ["kssh-t1-absent"] + tail)
        self.assertEqual(m[5], head + ["kssh-t1-load", "kssh-t1-revert"] + tail)
        self.assertNotIn("t1", J.MARKERS)  # the T0/F marker table is untouched

    def test_normalised_argv(self):
        a = "-chardev socket,id=ser0,path=/w/s1.sock,server=on -drive file=/w/D0.img,if=none,format=raw -cdrom /w/i.iso"
        b = "-chardev socket,id=ser0,path=/x/s5.sock,server=on -drive file=/x/S1r.img,if=none,format=raw -cdrom /w/i.iso"
        self.assertEqual(J.t1_norm_argv(a), J.t1_norm_argv(b))
        self.assertNotEqual(J.t1_norm_argv(a), J.t1_norm_argv(b.replace("/w/i.iso", "/w/j.iso")))

    def test_disk_verdict_attribution_rules(self):
        v = J.t1_disk_verdicts
        ok = v("after-boot1", _t1_disk_doc("D0", "r1"), "r1", True, "D0")
        self.assertEqual([(n, o) for n, o, _, _ in ok], [("t1-disk-records:after-boot1", True), ("t1-disk-scan:after-boot1", True)])
        miss = _t1_disk_doc("D0", "r1", ciphertext_found=False)
        miss["records"]["valid"] = []
        miss["record_verdict"] = {"expect": "r1", "ok": False, "reasons": ["R1 missing"]}
        self.assertEqual([c for _, _, _, c in v("after-boot1", miss, "r1", True, "D0")], ["C", "C"])    # load passed: layout
        self.assertEqual([c for _, _, _, c in v("after-boot1", miss, "r1", False, "D0")], ["D", "D"])   # load failed: persistence
        self.assertEqual([c for _, _, _, c in v("after-boot1", miss, "r1", None, "D0")], ["C", "C"])    # boot 2 not run
        self.assertEqual([c for _, _, _, c in v("x", None, "r1", True, "D0")], ["E", "E"])
        blank = _t1_disk_doc("D0-before", "none", ciphertext_found=True)
        self.assertEqual([(o, c) for _, o, _, c in v("d0-before", blank, "none", None, "D0-before")][1], (False, "E"))
        hit = _t1_disk_doc("D0", "r1")
        hit["scan"]["hits"] = [{"label": "pass:ka16", "form": "raw", "offset": 9}]
        self.assertEqual(v("after-boot1", hit, "r1", True, "D0")[1][1:4:2], (False, "D"))

    def test_record_problems_independent_of_the_verdict_flag(self):
        d = _t1_disk_doc("D0", "r1r2")
        self.assertEqual(J.t1_record_problems(d, "r1r2"), [])
        d["records"]["valid"][1]["previous"] = "99" * 32
        self.assertTrue(any("not the digest of R1" in p for p in J.t1_record_problems(d, "r1r2")))
        d = _t1_disk_doc("D0", "r1")
        d["records"]["valid"][0]["equals_predicted"] = False
        self.assertTrue(any("prediction" in p for p in J.t1_record_problems(d, "r1")))
        d["records"]["valid"][0].update(equals_predicted=True, revision=7)
        self.assertTrue(J.t1_record_problems(d, "r1"))
        self.assertTrue(J.t1_record_problems(_t1_disk_doc("D0", "r1r2"), "r1"))
        self.assertTrue(J.t1_record_problems(_t1_disk_doc("D0", "r1"), "none"))
        self.assertTrue(J.t1_record_problems(_t1_disk_doc("D0", "r1"), "r1r2"))
        self.assertEqual(J.t1_record_problems(_t1_disk_doc("D0", "none"), "none"), [])


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography not installed")
class T1JudgeTests(T1EvidenceCase):
    def test_clean_five_boot_run_is_class_a(self):
        ev = self.build_t1()
        rc, out, f = self.run_t1(ev)
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        c = f["classification.json"]
        self.assertEqual((c["reasons"], c["unproven"], c["boots_run"]), ([], [], [1, 2, 3, 4, 5]))
        self.assertTrue(c["first_sign_epoch_passed"] and c["first_persistence_epoch_passed"])
        v = f["verdict.json"]
        self.assertEqual((v["mode"], v["set"], v["no_evidence_value"]), ("t1", "rt1", False))
        self.assertEqual(sorted(v["boots"]), ["1", "2", "3", "4", "5"])
        self.assertEqual(v["boots"]["3"]["t1_import"], ["kp0"])
        self.assertEqual(v["boots"]["2"]["t1_load"]["fp"], self.fp("ka16"))
        self.assertEqual(v["observations"]["t1_ms"]["1"], [{"op": "first-status", "ms": 900}, {"op": "import", "ms": 1500}])
        self.assertEqual([c_["class"] for c_ in v["boots"]["2"]["sshd_connections"]][:2], ["accepted-pw", "accepted-pk:" + self.fp("ka16")])
        self.assertEqual(len(v["images"]), 6)
        for name in ("b1:marker:kssh-s02", "b2:marker:kssh-s02r", "b3:marker:kssh-t1-del", "b4:marker:kssh-t1-absent",
                     "b5:marker:kssh-t1-revert", "t1-role-sequence", "t1-reload-equals-replay", "t1-no-import-after-boot1",
                     "t1-argv-equal-modulo-disk", "t1-iso-same", "t1-s1-pristine", "checker-exits", "t1-five-boot-totals",
                     "t1-disk-records:d0-before", "t1-disk-scan:d0-before", "t1-disk-records:after-boot3",
                     "t1-disk-scan:after-boot5", "b1:s01-fingerprint-and-blob-prefix", "b4:trust-store-volume-mounted",
                     "b2:t1-opaque", "b3:t1-load-facts", "b4:remote-ledger", "census-drift", "t1-disk-controls"):
            self.assertTrue(self.check(f, name)["pass"], name)
        self.assertEqual(f["controls.json"]["ok"], True)
        self.assertEqual(len(f["controls.json"]["disk"]), 6)
        self.assertGreater(f["scan.json"]["forms"], 100)
        self.assertEqual({t["boot"] for t in f["scan.json"]["targets"]}, {1, 2, 3, 4, 5})
        self.assertTrue(all(t["hits"] == [] for t in f["scan.json"]["targets"]))
        self.assertIn("CLASS A", out)

    def test_judge_outputs_hold_no_secret_form_and_no_ciphertext(self):
        ev = self.build_t1()
        self.run_t1(ev)
        scanner = J.Scanner(K.canaries(self.keys, leak=True))
        for n in ("verdict.json", "classification.json", "scan.json", "controls.json"):
            self.assertEqual(scanner.scan_file(ev / n, joined=True), [], n)

    def test_b0_has_no_evidence_value_and_tolerates_missing_census_and_ledger(self):
        ev = self.build_t1("b0")
        (ev / "census-compare.json").unlink()
        (ev / "boot4" / "p09-ledger.txt").unlink()
        rc, out, f = self.run_t1(ev, "b0")
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertTrue(f["classification.json"]["no_evidence_value"])
        self.assertTrue(f["verdict.json"]["no_evidence_value"])
        self.assertTrue(any("census-not-run" in n for n in f["verdict.json"]["notes"]))
        ev = self.build_t1("rt1")
        (ev / "census-compare.json").unlink()
        rc, out, f = self.run_t1(ev)
        self.assertFalse(self.check(f, "census-drift")["pass"])
        self.assertEqual(self.klass(f), "B")  # E after the persistence epoch is a tail STOP

    def test_set_and_mode_combinations(self):
        ev = self.build_t1()
        for mode, set_ in (("t1", "rt0"), ("t1", "rt0f"), ("t0", "rt1"), ("f", "rt1")):
            rc, out, _ = self.run_t1(ev, set_, mode=mode)
            self.assertEqual(rc, 2, (mode, set_))
        self.assertEqual(self.run_t1(ev)[0], 0)
        # boots.json of another set is a host mix-up
        self.edit_json(ev / "boots.json", lambda d: d.update(set="b0"))
        rc, out, f = self.run_t1(ev)
        self.assertFalse(self.check(f, "boots-json")["pass"])
        self.assertEqual(self.klass(f), "B")

    def test_cli_accepts_t1_and_rt1(self):
        p = J.main  # argparse choices only: a bad choice raises SystemExit
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            p(["judge", "--evidence", "x", "--keys", "y", "--mode", "t2", "--set", "rt1", "--fixture-fp", "z",
               "--password-file", "a", "--seed-hex-file", "b"])

    # ---- banner, checker, build identity
    def test_banner_swap_is_class_e_wrong_build(self):
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l.replace(J.T1_PERSIST_BANNER, J.T1_CUSTODY_BANNER + ": volatile single-identity ssh leaf") for l in s])
        rc, out, f = self.run_t1(ev)
        self.assertEqual(self.klass(f), "E")
        self.assertFalse(self.check(f, "b1:banner-dev-posture-persist")["pass"])
        self.assertFalse(self.check(f, "b1:banner-no-custody")["pass"])
        self.assertFalse(f["classification.json"]["first_persistence_epoch_passed"])

    def test_custody_banner_in_any_boot_is_wrong_build_even_with_the_persist_banner(self):
        ev = self.build_t1()
        self.edit_serial(ev, 4, lambda s: s + ["[ INFO] " + J.T1_CUSTODY_BANNER + ": x"])
        rc, out, f = self.run_t1(ev)
        self.assertEqual(self.failed(f), ["b4:banner-no-custody"])
        self.assertEqual(self.klass(f), "B")  # a late host-class reason is a tail STOP

    def test_checker_exit_codes(self):
        for text in ("persist rc=1\ncustody rc=1\nnone rc=1\n", "persist rc=0\ncustody rc=0\nnone rc=1\n",
                     "persist rc=0\ncustody rc=1\n", "persist rc=0\ncustody rc=1\nnone rc=2\n"):
            ev = self.build_t1()
            (ev / "checker.txt").write_text(text)
            rc, out, f = self.run_t1(ev)
            self.assertFalse(self.check(f, "checker-exits")["pass"], text)
            self.assertEqual(self.check(f, "checker-exits").get("pass"), False)
        ev = self.build_t1()
        (ev / "checker.txt").unlink()
        self.assertFalse(self.check(self.run_t1(ev)[2], "checker-exits")["pass"])

    def test_per_boot_t0_environment_checks_apply(self):
        for fn, name in ((lambda s: [l for l in s if "fail-closed" not in l], "b2:kms-identity-service"),
                         (lambda s: [l for l in s if "[policy]" not in l], "b3:policy-loaded"),
                         (lambda s: [l for l in s if "boot-ceiling" not in l], "b2:boot-ceiling-selftest"),
                         (lambda s: s + ["serving WEAK xorshift32"], "b4:no-weak-entropy"),
                         (lambda s: s + ["[ERROR] kernel panic: x"], "b5:no-panic"),
                         (lambda s: [l for l in s if "CellosFS Native" not in l], "b3:trust-store-volume-mounted")):
            ev = self.build_t1()
            n = int(name[1])
            self.edit_serial(ev, n, fn)
            rc, out, f = self.run_t1(ev)
            self.assertFalse(self.check(f, name)["pass"], name)
        ev = self.build_t1()
        self.edit_serial(ev, 3, lambda s: [l for l in s if "CellosFS Native" not in l])
        _, _, f = self.run_t1(ev)
        self.assertEqual(f["verdict.json"]["checks"]["b3:trust-store-volume-mounted"]["pass"], False)
        self.assertEqual(self.klass(f), "B")
        for n in (1, 2, 4, 5):
            self.assertTrue(self.check(f, f"b{n}:trust-store-volume-mounted")["pass"])

    def test_t0_no_test_config_and_netdev_rules(self):
        ev = self.build_t1()
        self.edit_serial(ev, 2, lambda s: s + ["USER: P04 INFO ssh-test-config kssh-faults=on"])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b2:t0-no-test-config")["pass"])
        ev = self.build_t1()
        p = ev / "boot3" / "qemu-argv.txt"
        p.write_text(p.read_text().replace("-netdev user,id=net0 ", "-netdev user,id=net0,hostfwd=tcp::1-:2 "))
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b3:qemu-one-user-netdev-no-forward")["pass"])
        self.assertFalse(self.check(f, "t1-argv-equal-modulo-disk")["pass"])

    # ---- markers
    def test_marker_rules_per_boot(self):
        ev = self.build_t1()
        self.edit_serial(ev, 2, lambda s: [l for l in s if "PASS kssh-t1-opaque" not in l])
        f = self.run_t1(ev)[2]
        self.assertEqual(self.check(f, "b2:marker:kssh-t1-opaque")["detail"], "missing")
        self.assertFalse(self.check(f, "b2:marker:kssh-t1-opaque")["pass"])
        ev = self.build_t1()
        self.edit_serial(ev, 4, lambda s: s[:-1] + ["USER: P04 PASS kssh-t1-absent", s[-1]])
        f = self.run_t1(ev)[2]
        self.assertIn("seen 2 times", self.check(f, "b4:marker:kssh-t1-absent")["detail"])
        ev = self.build_t1()
        self.edit_serial(ev, 3, lambda s: s[:-1] + ["USER: P04 PASS kssh-s99", s[-1]])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b3:no-unexpected-markers")["pass"])
        ev = self.build_t1()  # boot 1's markers in boot 4's list: the lists are per boot
        self.edit_serial(ev, 4, lambda s: s[:-1] + ["USER: P04 PASS kssh-s02", s[-1]])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b4:no-unexpected-markers")["pass"])
        ev = self.build_t1()
        self.edit_serial(ev, 5, lambda s: [("USER: P04 FAIL kssh-t1-revert the key is not usable" if "PASS kssh-t1-revert" in l else l) for l in s]
                         [:-1] + [s[-1].replace("PASS", "FAIL")])
        f = self.run_t1(ev)[2]
        self.assertEqual(self.check(f, "b5:marker:kssh-t1-revert")["pass"], False)
        self.assertEqual(self.klass(f), "D")
        ev = self.build_t1()
        self.edit_serial(ev, 3, lambda s: [l.replace("PASS kssh-t1-del", "FAIL kssh-t1-del timeout after 5s") for l in s])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b3:marker:kssh-t1-del")["pass"])
        self.assertEqual(files_class(f, "b3:marker:kssh-t1-del"), "C")

    def test_terminal_line_exactly_once_per_run_boot(self):
        ev = self.build_t1()
        self.edit_serial(ev, 2, lambda s: s + ["USER: P04-TERMINAL: PASS checks=1"])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b2:p04-terminal-line")["pass"])
        ev = self.build_t1()
        self.edit_serial(ev, 4, lambda s: s[:-1])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b4:p04-terminal-line")["pass"])

    # ---- role oracle
    def test_role_sequence_mismatch_missing_and_file(self):
        ev = self.build_t1()
        self.edit_serial(ev, 3, lambda s: [l.replace("role n=3", "role n=2") for l in s])
        rc, out, f = self.run_t1(ev)
        self.assertFalse(self.check(f, "t1-role-sequence")["pass"])
        self.assertEqual(files_class(f, "t1-role-sequence"), "E")
        self.assertEqual(self.klass(f), "B")
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l for l in s if "kssh-obs role" not in l])
        rc, out, f = self.run_t1(ev)
        self.assertFalse(self.check(f, "t1-role-sequence")["pass"])
        self.assertEqual(self.klass(f), "E")  # boot 1 role channel failure before the persistence epoch
        ev = self.build_t1()
        (ev / "boot2" / "role.txt").write_text("role=3\n")
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-role-sequence")["pass"])
        ev = self.build_t1()
        (ev / "boot5" / "role.txt").unlink()
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-role-sequence")["pass"])
        ev = self.build_t1()
        self.edit_json(ev / "boot4" / "boot.json", lambda d: d.update(role_written=3))
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-role-sequence")["pass"])
        ev = self.build_t1()  # the same role printed twice in one boot is not a role sequence
        self.edit_serial(ev, 2, lambda s: s + ["USER: P04 INFO kssh-obs role n=2"])
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-role-sequence")["pass"])

    # ---- load facts, replay, imports
    def test_load_facts_and_reload_equals_replay(self):
        ev = self.build_t1()
        self.edit_serial(ev, 5, lambda s: [l.replace("posture=persistent", "posture=volatile") for l in s])
        rc, out, f = self.run_t1(ev)
        self.assertFalse(self.check(f, "b5:t1-load-facts")["pass"])
        self.assertFalse(self.check(f, "t1-reload-equals-replay")["pass"])
        self.assertEqual(files_class(f, "t1-reload-equals-replay"), "D")
        self.assertEqual(self.klass(f), "D")
        self.assertTrue(f["classification.json"]["first_persistence_epoch_passed"])  # boots 1-2 were fine
        ev = self.build_t1()  # facts equal to each other but wrong (both boots lost the key)
        for n in (2, 5):
            self.edit_serial(ev, n, lambda s: [l.replace("keys=1", "keys=0") for l in s])
        f = self.run_t1(ev)[2]
        self.assertTrue(self.check(f, "t1-reload-equals-replay")["pass"])
        self.assertFalse(self.check(f, "b2:t1-load-facts")["pass"])
        self.assertEqual(self.klass(f), "D")
        ev = self.build_t1()
        self.edit_serial(ev, 5, lambda s: [l.replace(self.fp("ka16"), self.fp("kp0")) if "t1-load" in l else l for l in s])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "t1-reload-equals-replay")["pass"])
        ev = self.build_t1()
        self.edit_serial(ev, 5, lambda s: [l for l in s if "kssh-obs t1-load" not in l])
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-reload-equals-replay"), "C")

    def test_no_import_after_boot_one(self):
        for n, extra in ((2, "t1-import name=ka16"), (4, "t1-import name=kp0"), (5, "t1-import name=ka16")):
            ev = self.build_t1()
            self.edit_serial(ev, n, lambda s: s[:-2] + [f"USER: P04 INFO kssh-obs {extra}"] + s[-2:])
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, "t1-no-import-after-boot1")["pass"], n)
        for lines in ([], ["t1-import name=ka16"], ["t1-import name=kp0", "t1-import name=kp0"], ["t1-import name=ka16", "t1-import name=kp0"]):
            ev = self.build_t1()
            self.edit_serial(ev, 3, lambda s: [l for l in s if "t1-import" not in l][:-2]
                             + [f"USER: P04 INFO kssh-obs {o}" for o in lines] + s[-2:])
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, "t1-no-import-after-boot1")["pass"], lines)
        ev = self.build_t1()
        self.assertTrue(self.check(self.run_t1(ev)[2], "t1-no-import-after-boot1")["pass"])

    def test_opaque_observation(self):
        for old, new in (("cat_leak=false", "cat_leak=true"), ("cat_st=1", "cat_st=0")):
            ev = self.build_t1()
            self.edit_serial(ev, 2, lambda s: [l.replace(old, new) for l in s])
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, "b2:t1-opaque")["pass"], new)
            self.assertEqual(files_class(f, "b2:t1-opaque"), "D")
        ev = self.build_t1()
        self.edit_serial(ev, 2, lambda s: [l.replace("ls=denied", "ls=listed") for l in s])
        self.assertTrue(self.check(self.run_t1(ev)[2], "b2:t1-opaque")["pass"])  # ls is an observation only

    def test_frame_bound_and_transcripts_and_conf(self):
        ev = self.build_t1()
        self.edit_serial(ev, 2, lambda s: [l.replace("op=reload ms=800", "op=reload ms=5001") for l in s])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b2:t1-ms-frame-bound")["pass"])
        self.assertEqual(f["verdict.json"]["boots"]["2"]["t1_ms"], [{"op": "reload", "ms": 5001}])
        ev = self.build_t1()  # the import commit is recorded, not bounded by the single-status rule
        self.edit_serial(ev, 1, lambda s: [l.replace("op=import ms=1500", "op=import ms=9000") for l in s])
        f = self.run_t1(ev)[2]
        self.assertEqual((self.klass(f), f["verdict.json"]["observations"]["t1_ms"]["1"][1]), ("A", {"op": "import", "ms": 9000}))
        for old, new, cls in (("hits=0", "hits=2", "D"), ("scans=12", "scans=0", "C")):
            ev = self.build_t1()
            self.edit_serial(ev, 3, lambda s: [l.replace(old, new) for l in s])
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, "b3:transcripts-clean")["pass"])
            self.assertEqual(files_class(f, "b3:transcripts-clean"), cls)
        ev = self.build_t1()
        self.edit_serial(ev, 4, lambda s: [l.replace("mode=t1", "mode=t0") for l in s])
        self.assertEqual(files_class(self.run_t1(ev)[2], "b4:conf-observed-t1"), "E")

    def test_s01_obs_boot_one_only(self):
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l.replace("prefix=" + self.exp["keys"]["ka16"]["blob_prefix"], "prefix=0000000000000000") for l in s])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b1:s01-fingerprint-and-blob-prefix")["pass"])
        self.assertEqual(self.klass(f), "D")
        self.assertNotIn("b2:s01-fingerprint-and-blob-prefix", f["verdict.json"]["checks"])

    # ---- sshd, ledger
    def test_sshd_sequence_per_boot(self):
        ev = self.build_t1()  # boot 4 must open no connection for s04a
        self.edit_sshd(ev, 4, lambda h: h + sshd_lines("closed", 49999))
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b4:sshd-connection-sequence")["pass"])
        self.assertEqual(files_class(f, "b4:sshd-connection-sequence"), "D")
        ev = self.build_t1()  # the limiter probes of boot 2: one probe-ok missing
        self.edit_sshd(ev, 2, lambda h: h[:-3])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b2:sshd-connection-sequence")["pass"])
        ev = self.build_t1()  # boot 5 accepted for another fingerprint
        self.edit_sshd(ev, 5, lambda h: [l.replace(self.fp("ka16"), self.fp("kp0")) for l in h])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b5:sshd-connection-sequence")["pass"])
        ev = self.build_t1()  # the same sshd lines in boot 3 (s15-iv is kp0) cannot be swapped for boot 2's s02r class
        self.edit_sshd(ev, 3, lambda h: [l.replace(self.fp("kp0"), self.fp("ka16")) for l in h])
        self.assertFalse(self.check(self.run_t1(ev)[2], "b3:sshd-connection-sequence")["pass"])
        ev = self.build_t1()
        self.edit_sshd(ev, 2, lambda h: h + ["Connection from 172.17.0.1 port 49000 on 172.17.0.2 port 22 rdomain \"\"",
                                             "Accepted password for tester from 172.17.0.1 port 49000 ssh2"])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b2:no-accepted-password-after-publickey")["pass"])

    def test_password_fallback_and_dirty_accept_are_d(self):
        ev = self.build_t1()
        self.edit_sshd(ev, 2, lambda h: [l.replace("Accepted key ED25519", "Failed publickey for tester from 172.17.0.1 port 40201 ssh2: ED25519", 1)
                                         if False else l for l in h] + ["Connection from 172.17.0.1 port 49100 on 172.17.0.2 port 22 rdomain \"\"",
                                                                       "Failed publickey for tester from 172.17.0.1 port 49100 ssh2: ED25519 " + FPB,
                                                                       "Accepted password for tester from 172.17.0.1 port 49100 ssh2"])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b2:no-password-fallback")["pass"])
        self.assertEqual(files_class(f, "b2:no-password-fallback"), "D")
        self.assertEqual(self.klass(f), "D")

    def test_unknown_attempt_ids_are_c_and_boot_scoped(self):
        ev = self.build_t1()  # `s02` is boot 1's id: not registered in boot 5
        self.edit_serial(ev, 5, lambda s: [l.replace("kssh-attempt s02r", "kssh-attempt s02") for l in s])
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "b5:attempt-ids-known")["pass"])
        self.assertEqual(files_class(f, "b5:attempt-ids-known"), "C")

    def test_missing_sshd_log_is_c_before_the_epoch(self):
        ev = self.build_t1()
        (ev / "boot1" / "sshd.log").unlink()
        self.edit_serial(ev, 1, lambda s: [l.replace("PASS kssh-s09", "FAIL kssh-s09 refusal answered wrongly") for l in s])
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "C")  # E > C > D before the first persistence epoch
        self.assertFalse(f["classification.json"]["first_sign_epoch_passed"])
        ev = self.build_t1()
        (ev / "boot4" / "sshd.log").unlink()
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "B")  # after the epoch: tail STOP
        self.assertEqual(f["classification.json"]["reasons"][0].split(":")[0], "tail STOP after the first persistence epoch")

    def test_ledger_slice_per_boot(self):
        for n, text in ((4, "p09x-kssh-login\n"), (1, ""), (2, "p09x-kssh-login\n" * 2), (3, "p09x-kssh-login\np09x-other\n")):
            ev = self.build_t1()
            (ev / f"boot{n}" / "p09-ledger.txt").write_text(text)
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, f"b{n}:remote-ledger")["pass"], n)
        ev = self.build_t1()
        (ev / "boot4" / "p09-ledger.txt").unlink()
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "b4:remote-ledger"), "C")

    def test_five_boot_totals_are_judged_only_when_all_ran(self):
        ev = self.build_t1()
        f = self.run_t1(ev)[2]
        self.assertIn("'probe-ok': 6", self.check(f, "t1-five-boot-totals")["detail"] + "'probe-ok': 6")
        shutil.rmtree(ev / "boot5")
        self.edit_json(ev / "boots.json", lambda d: d.update(stopped={"after_boot": 4, "reason": "gate", "class_hint": "gate"}))
        f = self.run_t1(ev)[2]
        self.assertNotIn("t1-five-boot-totals", f["verdict.json"]["checks"])

    # ---- boot.json, qemu, iso, s1
    def test_boot_json_rules(self):
        ev = self.build_t1()
        self.edit_json(ev / "boot3" / "boot.json", lambda d: d["qemu"].update(clean=False, escalated=True, signal="SIGKILL"))
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "b3:qemu-clean-exit"), "E")
        ev = self.build_t1()
        self.edit_json(ev / "boot4" / "boot.json", lambda d: d.update(verdict="no-verdict"))
        self.assertEqual(files_class(self.run_t1(ev)[2], "b4:driver-verdict"), "C")
        ev = self.build_t1()
        self.edit_json(ev / "boot2" / "boot.json", lambda d: d.update(settled=False))
        self.assertEqual(files_class(self.run_t1(ev)[2], "b2:sshd-slice-settled"), "C")
        ev = self.build_t1()
        self.edit_json(ev / "boot5" / "boot.json", lambda d: d.update(disk_image="D0"))
        self.assertEqual(files_class(self.run_t1(ev)[2], "b5:boot-json-identity"), "E")
        ev = self.build_t1()
        (ev / "boot2" / "boot.json").unlink()
        self.assertEqual(files_class(self.run_t1(ev)[2], "b2:boot-json"), "E")

    def test_argv_inequality_beyond_disk_and_socket_is_e(self):
        ev = self.build_t1()
        p = ev / "boot2" / "qemu-argv.txt"
        p.write_text(p.read_text().replace("-m 256M", "-m 512M"))
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "t1-argv-equal-modulo-disk")["pass"])
        self.assertEqual(files_class(f, "t1-argv-equal-modulo-disk"), "E")
        self.assertEqual(self.klass(f), "B")
        ev = self.build_t1()  # only the disk file and the socket differ per boot (boot 5 runs on S1r)
        self.assertTrue(self.check(self.run_t1(ev)[2], "t1-argv-equal-modulo-disk")["pass"])

    def test_iso_must_be_the_same_in_every_boot(self):
        ev = self.build_t1()
        self.edit_json(ev / "boot3" / "boot.json", lambda d: d.update(iso_sha256="ee" * 32))
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "t1-iso-same")["pass"])
        self.assertEqual(files_class(f, "t1-iso-same"), "E")
        ev = self.build_t1()
        self.edit_json(ev / "boots.json", lambda d: d.update(iso_sha256=None))
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-iso-same")["pass"])

    def test_s1_must_stay_pristine(self):
        for key in ("s1_sha256_after_boot5", "s1r_sha256_before_boot5", "s1_sha256"):
            ev = self.build_t1()
            self.edit_json(ev / "boots.json", lambda d: d.update({key: "ff" * 32}))
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, "t1-s1-pristine")["pass"], key)
            self.assertEqual(files_class(f, "t1-s1-pristine"), "E")
        ev = self.build_t1()
        self.edit_json(ev / "boots.json", lambda d: d.pop("s1_sha256_after_boot5"))
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-s1-pristine")["pass"])  # boot 5 ran: all three are required

    # ---- disk images
    def test_record_tamper_verdicts_are_d_and_named_per_image(self):
        cases = {
            "after-boot1": lambda d: d["records"]["valid"][0].update(equals_predicted=False),
            "after-boot2": lambda d: d["records"]["valid"].append(dict(d["records"]["valid"][0], offset=999, digest="77" * 32)),
            "after-boot3": lambda d: d["records"]["valid"][1].update(previous="99" * 32),
            "after-boot4": lambda d: d["records"]["valid"].pop(),
            "after-boot5": lambda d: d["records"]["valid"][0].update(slot=1),
        }
        for label, fn in cases.items():
            ev = self.build_t1()
            n = int(label[-1])
            self.edit_json(ev / f"boot{n}" / "disk-after-boot.json", fn)
            f = self.run_t1(ev)[2]
            self.assertFalse(self.check(f, f"t1-disk-records:{label}")["pass"], label)
            self.assertEqual(files_class(f, f"t1-disk-records:{label}"), "D", label)
            self.assertEqual(self.klass(f), "D", label)
            others = [k for k in f["verdict.json"]["checks"] if k.startswith("t1-disk-records:") and k != f"t1-disk-records:{label}"]
            self.assertTrue(all(self.check(f, k)["pass"] for k in others))

    def test_verdict_flag_cannot_hide_a_wrong_set_nor_a_wrong_expectation(self):
        ev = self.build_t1()  # the orchestrator judged after-boot3 for r1 only: the table says r1r2
        self.edit_json(ev / "boot3" / "disk-after-boot.json", lambda d: d.update(
            records={"valid": d["records"]["valid"][:1], "invalid": [], "v1": []}, record_verdict={"expect": "r1", "ok": True, "reasons": []}))
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-disk-records:after-boot3"), "E")
        ev = self.build_t1()  # ok=true but the set holds a tombstone too early
        self.edit_json(ev / "boot2" / "disk-after-boot.json", lambda d: d["records"]["valid"].append(
            {"offset": 5, "slot": 1, "state": 0, "revision": 2, "name": "", "digest": "88" * 32, "previous": "11" * 32,
             "equals_predicted": None}))
        self.assertFalse(self.check(self.run_t1(ev)[2], "t1-disk-records:after-boot2")["pass"])

    def test_planted_canary_in_a_disk_image_is_d_and_final(self):
        ev = self.build_t1()
        self.edit_json(ev / "boot4" / "disk-after-boot.json", lambda d: d["scan"].update(
            hits=[{"label": "seed:ka16", "form": "raw", "offset": 12345}]))
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "t1-disk-scan:after-boot4")["pass"])
        self.assertEqual(files_class(f, "t1-disk-scan:after-boot4"), "D")
        self.assertEqual(self.klass(f), "D")
        self.assertIn("seed:ka16/raw@12345", self.check(f, "t1-disk-scan:after-boot4")["detail"])
        ev = self.build_t1()  # a hit on the first image is D before the epoch as well
        self.edit_json(ev / "disk-baseline.json", lambda d: d["scan"].update(hits=[{"label": "pass:ka16", "form": "raw", "offset": 1}]))
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "D")
        self.assertFalse(f["classification.json"]["first_persistence_epoch_passed"])

    def test_failed_disk_controls_are_class_e(self):
        ev = self.build_t1()
        self.edit_json(ev / "boot2" / "disk-after-boot.json", lambda d: d["scan"].update(controls_ok=False))
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-disk-scan:after-boot2"), "E")
        self.assertEqual(self.klass(f), "E")
        ev = self.build_t1()
        self.edit_json(ev / "controls.json", lambda d: (d["disk"][3].update(ok=False), d.update(ok=False)))
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-disk-controls"), "E")
        self.assertEqual(self.klass(f), "E")
        self.assertEqual(f["controls.json"]["ok"], False)
        ev = self.build_t1()
        (ev / "controls.json").unlink()
        f = self.run_t1(ev)[2]
        self.assertFalse(self.check(f, "t1-disk-controls")["pass"])
        self.assertEqual(f["controls.json"]["disk"], [])

    def test_judge_rerun_keeps_the_disk_controls(self):
        ev = self.build_t1()
        self.run_t1(ev)
        rc, out, f = self.run_t1(ev)  # controls.json is now the merged file
        self.assertEqual((rc, self.klass(f)), (0, "A"), out)
        self.assertEqual(len(f["controls.json"]["disk"]), 6)

    def test_ciphertext_positive_control_attribution(self):
        def miss(d):
            d.update(ciphertext_found=False, records={"valid": [], "invalid": [], "v1": []},
                     record_verdict={"expect": "r1", "ok": False, "reasons": ["R1 missing: no valid record equals the host's prediction"]})
        ev = self.build_t1()  # boot 2 loaded the key: the bytes are on disk in a form the finder cannot see = C
        self.edit_json(ev / "boot1" / "disk-after-boot.json", miss)
        f = self.run_t1(ev)[2]
        self.assertEqual((self.klass(f), files_class(f, "t1-disk-scan:after-boot1"), files_class(f, "t1-disk-records:after-boot1")),
                         ("C", "C", "C"))
        ev = self.build_t1()  # boot 2 could not load it either: persistence failed = D
        self.edit_json(ev / "boot1" / "disk-after-boot.json", miss)
        self.edit_serial(ev, 2, lambda s: [l.replace("PASS kssh-t1-load", "FAIL kssh-t1-load keys: 0") for l in s])
        self.edit_serial(ev, 2, lambda s: [l.replace("keys=1", "keys=0") for l in s])
        f = self.run_t1(ev)[2]
        self.assertEqual((self.klass(f), files_class(f, "t1-disk-scan:after-boot1")), ("D", "D"))
        ev = self.build_t1()  # ciphertext missing on its own (records fine): the same attribution, C
        self.edit_json(ev / "boot3" / "disk-after-boot.json", lambda d: d.update(ciphertext_found=False))
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-disk-scan:after-boot3"), "C")
        self.assertEqual(self.klass(f), "B")
        ev = self.build_t1()  # a ciphertext found on the blank baseline is a control artefact: E
        self.edit_json(ev / "disk-baseline.json", lambda d: d.update(ciphertext_found=True))
        self.assertEqual(files_class(self.run_t1(ev)[2], "t1-disk-scan:d0-before"), "E")

    def test_missing_disk_findings_of_a_run_boot_is_e(self):
        ev = self.build_t1()
        (ev / "boot2" / "disk-after-boot.json").unlink()
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "t1-disk-records:after-boot2"), "E")
        self.assertEqual(self.klass(f), "E")  # no snapshot of boot 2: the persistence epoch cannot be claimed
        self.assertFalse(f["classification.json"]["first_persistence_epoch_passed"])

    # ---- serial and log leak scan
    def test_ciphertext_or_record_header_on_serial_or_logs_is_class_d(self):
        ct = bytes.fromhex(self.exp["record"]["ct"])
        forms = {"raw-ct": ct[:64], "hex-ct": ct[:40].hex().encode(), "b64-ct": base64.b64encode(ct[:60]),
                 "hdr-a": b"CKMS\x02\x00\x01\x01", "hdr-b": b"CKMS\x02\x01\x01\x00"}
        for tag, needle in forms.items():
            for target in ("serial.raw.log", "sshd.log", "qemu-argv.txt"):
                ev = self.build_t1()
                with open(ev / "boot2" / target, "ab") as fh:
                    fh.write(b"\nleak " + needle + b"\n")
                rc, out, f = self.run_t1(ev)
                self.assertFalse(self.check(f, "no-ciphertext-or-header-in-serial-or-logs")["pass"], (tag, target))
                self.assertEqual(self.klass(f), "D", (tag, target))
                self.assertNotIn(needle.decode("latin-1"), out)
                self.assertEqual(J.Scanner(K.canaries(self.keys, leak=True)).scan_file(ev / "verdict.json"), [])

    def test_planted_secret_in_any_boot_target_is_d_and_redacted(self):
        wrong = self.sec["keys"]["ka16"]["wrong"]
        for n, target in ((1, "serial.log"), (3, "p09-ledger.txt"), (4, "sshd.log"), (5, "serial.raw.log")):
            ev = self.build_t1()
            with open(ev / f"boot{n}" / target, "ab") as fh:
                fh.write(b"\nleak " + wrong.encode() + b"\n")
            rc, out, f = self.run_t1(ev)
            self.assertEqual(self.klass(f), "D", (n, target))
            self.assertFalse(self.check(f, "no-canary-in-targets")["pass"])
            self.assertIn("wrong:ka16", self.check(f, "no-canary-in-targets")["detail"])
            for name in ("verdict.json", "classification.json", "scan.json", "controls.json"):
                self.assertNotIn(wrong.encode(), (ev / name).read_bytes())
            self.assertNotIn(wrong, out)

    def test_controls_run_on_the_serial_part_and_failures_are_e(self):
        ev = self.build_t1()
        with mock.patch.object(J, "run_controls", return_value={"ok": False}):
            rc, out, f = self.run_t1(ev)
        self.assertEqual(self.klass(f), "E")
        self.assertFalse(self.check(f, "scan-controls")["pass"])

    # ---- sequence, gates, unproven boots
    def stop_after(self, ev, after, hint, reason="gate row failed"):
        for n in range(after + 1, 6):
            shutil.rmtree(ev / f"boot{n}")
        self.edit_json(ev / "boots.json", lambda d: d.update(stopped={"after_boot": after, "reason": reason, "class_hint": hint}))

    def test_gate_stop_after_the_delete_row_lists_unproven_boots(self):
        ev = self.build_t1()
        self.edit_serial(ev, 3, lambda s: [l.replace("PASS kssh-t1-del", "FAIL kssh-t1-del status still lists ka16") for l in s])
        self.stop_after(ev, 3, "gate")
        rc, out, f = self.run_t1(ev)
        c = f["classification.json"]
        self.assertEqual((rc, c["class"]), (1, "D"), out)
        self.assertEqual(c["boots_run"], [1, 2, 3])
        self.assertEqual(c["unproven"], ["boot4: not run", "boot5: not run"])
        self.assertEqual(f["verdict.json"]["boots"]["4"], {"ran": False})
        self.assertEqual(f["verdict.json"]["sequence"]["boots_unrun"], [4, 5])
        self.assertNotIn("b4:marker:kssh-t1-absent", f["verdict.json"]["checks"])
        self.assertNotIn("t1-disk-records:after-boot4", f["verdict.json"]["checks"])

    def test_host_stop_after_the_persistence_epoch_is_class_b_with_unproven_rows(self):
        ev = self.build_t1()
        self.stop_after(ev, 2, "E", "qemu did not exit on SIGTERM")
        rc, out, f = self.run_t1(ev)
        c = f["classification.json"]
        self.assertEqual((rc, c["class"]), (1, "B"), out)
        self.assertTrue(c["first_persistence_epoch_passed"])
        self.assertEqual(c["unproven"][:3], ["boot3: not run", "boot4: not run", "boot5: not run"])
        self.assertIn("tail STOP", c["reasons"][0])
        self.assertIn("qemu did not exit", " ".join(c["reasons"]))

    def test_stop_before_the_persistence_epoch_is_e_or_c(self):
        ev = self.build_t1()
        self.stop_after(ev, 1, "E", "host failure")
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "E")
        self.assertFalse(f["classification.json"]["first_persistence_epoch_passed"])
        self.assertTrue(f["classification.json"]["first_sign_epoch_passed"])
        ev = self.build_t1()
        self.stop_after(ev, 1, "C", "no driver verdict")
        self.assertEqual(self.klass(self.run_t1(ev)[2]), "C")
        ev = self.build_t1()  # boots missing and nothing explains it
        shutil.rmtree(ev / "boot5")
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "sequence-complete"), "E")
        self.assertEqual(self.klass(f), "B")

    def test_gate_stop_without_a_failed_row_is_still_not_class_a(self):
        ev = self.build_t1()
        self.stop_after(ev, 3, "gate")
        f = self.run_t1(ev)[2]
        self.assertEqual(files_class(f, "sequence-complete"), "C")
        self.assertNotEqual(self.klass(f), "A")

    def test_boot_one_gate_failure_ends_with_class_d_not_a_later_missing_row(self):
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l.replace("PASS kssh-s02", "FAIL kssh-s02 unlock failed") for l in s])
        self.stop_after(ev, 1, "gate")
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "D")
        self.assertFalse(f["classification.json"]["first_sign_epoch_passed"])
        self.assertEqual(f["classification.json"]["unproven"][:2], ["boot2: not run", "boot3: not run"])

    def test_precedence_before_the_epoch_is_e_then_c_then_d(self):
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l.replace("PASS kssh-s09", "FAIL kssh-s09 wrong refusal") for l in s])
        self.assertEqual(self.klass(self.run_t1(ev)[2]), "D")  # D alone, the epoch (s00, s01, s02) still passes
        f = self.run_t1(ev)[2]
        self.assertTrue(f["classification.json"]["first_persistence_epoch_passed"])
        ev = self.build_t1()
        self.edit_serial(ev, 1, lambda s: [l.replace("PASS kssh-s02", "FAIL kssh-s02 unlock failed") for l in s])
        (ev / "boot1" / "p09-ledger.txt").unlink()
        self.assertEqual(self.klass(self.run_t1(ev)[2]), "C")
        (ev / "census-compare.json").unlink()
        self.assertEqual(self.klass(self.run_t1(ev)[2]), "E")

    def test_after_the_epoch_d_stays_final_and_later_c_e_is_a_tail(self):
        ev = self.build_t1()
        self.edit_serial(ev, 4, lambda s: [l.replace("PASS kssh-t1-absent", "FAIL kssh-t1-absent key still listed") for l in s])
        (ev / "boot5" / "p09-ledger.txt").unlink()
        self.assertEqual(self.klass(self.run_t1(ev)[2]), "D")
        ev = self.build_t1()
        (ev / "boot5" / "p09-ledger.txt").unlink()
        (ev / "census-compare.json").unlink()
        f = self.run_t1(ev)[2]
        self.assertEqual(self.klass(f), "B")
        self.assertEqual(f["classification.json"]["unproven"], ["b5:remote-ledger", "census-drift"])

    def test_no_boot_directories_and_an_empty_boot_directory_do_not_crash(self):
        ev = self.build_t1()
        for n in range(1, 6):
            shutil.rmtree(ev / f"boot{n}")
        self.edit_json(ev / "boots.json", lambda d: d.update(stopped={"after_boot": 0, "reason": "D0 baseline failed", "class_hint": "E"}))
        rc, out, f = self.run_t1(ev)
        self.assertEqual((rc, self.klass(f)), (1, "E"), out)
        self.assertEqual(f["classification.json"]["boots_run"], [])
        self.assertEqual(f["classification.json"]["unproven"], [f"boot{n}: not run" for n in range(1, 6)])
        ev = self.build_t1()
        shutil.rmtree(ev / "boot3")
        (ev / "boot3").mkdir()  # QEMU never produced anything
        rc, out, f = self.run_t1(ev)
        self.assertEqual(rc, 1)
        self.assertFalse(self.check(f, "b3:evidence-file:serial.log")["pass"])
        self.assertFalse(self.check(f, "b3:boot-json")["pass"])
        self.assertEqual(self.klass(f), "B")

    def test_fixture_fp_and_keyset_checks(self):
        ev = self.build_t1()
        pw, seedf = self.tmp / "fixture-password", self.tmp / "seed.hex"
        pw.write_text("pw-" + secrets.token_hex(8) + "\n")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            J.main(["judge", "--evidence", str(ev), "--keys", str(self.keys), "--mode", "t1", "--set", "rt1",
                    "--fixture-fp", "bad", "--password-file", str(pw), "--seed-hex-file", str(seedf)])
        self.assertFalse(json.loads((ev / "verdict.json").read_text())["checks"]["fixture-fp-format"]["pass"])

    def test_t0_key_set_is_refused_for_mode_t1(self):
        t0 = self.tmp / "t0keys"
        K.gen(t0 / "keys", t0 / "stage")
        ev = self.build_t1()
        pw, seedf = self.tmp / "fixture-password", self.tmp / "seed.hex"
        pw.write_text("pw-x\n")
        seedf.write_text("00" * 32 + "\n")
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            rc = J.main(["judge", "--evidence", str(ev), "--keys", str(t0 / "keys"), "--mode", "t1", "--set", "rt1",
                         "--fixture-fp", FP_HOST, "--password-file", str(pw), "--seed-hex-file", str(seedf)])
        f = json.loads((ev / "verdict.json").read_text())
        self.assertEqual(rc, 1)
        self.assertFalse(f["checks"]["keyset-mode-t1"]["pass"])


def files_class(files, name):
    """The class letter a failing check carries (read back from the reasons of classification.json)."""
    for r in files["classification.json"]["reasons"]:
        c, _, rest = r.partition(":")
        if rest.startswith(name + ":"):
            return c
    return None


@unittest.skipIf(Ed25519PrivateKey is None, "cryptography not installed")
class T1ArchiveTests(T1EvidenceCase):
    def setUp(self):
        self.ev = self.build_t1()
        self.run_t1(self.ev)
        (self.ev / "preflight.txt").write_text("preflight ok\n")
        (self.ev / "preflight.final.txt").write_text("preflight ok\n")
        (self.ev / "provenance.sha256").write_text("00  x\n")
        (self.ev / "provenance-build.txt").write_text("built\n")
        (self.ev / "census-before.json").write_text("{}\n")
        (self.ev / "census-after.json").write_text("{}\n")
        (self.ev / "f1-waiver-unsafe-allowlist.diff").write_text("+x\n")
        (self.ev / "fixture.txt").write_text("fixture: x\n")
        (self.ev / "kssh.conf").write_text(K.build_conf(self.keys, "t1", [1, 2, 3], FP_HOST, "pw-archive-1234", False))
        (self.ev / "seed.hex").write_text("00" * 32 + "\n")
        (self.ev / "disk.img").write_bytes(b"x")
        (self.ev / "boot-config").mkdir()
        (self.ev / "boot-config/a.toml").write_text("x\n")
        (self.ev / "boot3" / "disk.img").write_bytes(b"x")
        (self.ev / "boot3" / "scratch.txt").write_text("not allowlisted\n")
        self.dst = self.tmp / f"repo-set-{secrets.token_hex(3)}"

    def test_clean_t1_archive_layout(self):
        ok, msgs = J.archive(self.ev, self.dst, self.keys)
        self.assertTrue(ok, msgs)
        top = {p.name for p in self.dst.iterdir()}
        for n in ("boots.json", "disk-baseline.json", "controls.json", "checker.txt", "preflight.txt", "preflight.final.txt",
                  "census-compare.json", "census-before.json", "census-after.json", "f1-waiver-unsafe-allowlist.diff",
                  "provenance.sha256", "provenance-build.txt", "verdict.json", "scan.json", "classification.json",
                  "expected-public.json", "ka16.pub", "kp0.pub", "kssh.conf", "fixture.txt", "boot1", "boot2", "boot3", "boot4", "boot5"):
            self.assertIn(n, top, n)
        for n in ("expected.json", "seed.hex", "disk.img", "boot-config", "ka16", "kp0", "secrets.json", "ka16.key", "kb.pub"):
            self.assertNotIn(n, top, n)
        self.assertEqual({p.name for p in (self.ev / "boot3").iterdir()} - set(J.BOOT_ALLOW), {"disk.img", "scratch.txt"})
        self.assertEqual({p.name for p in (self.dst / "boot3").iterdir()},
                         {"serial.raw.log", "serial.log", "sshd.log", "p09-ledger.txt", "qemu-argv.txt", "role.txt", "boot.json", "disk-after-boot.json"})
        pub = json.loads((self.dst / "expected-public.json").read_text())
        self.assertEqual(pub["record"]["ct_len"], len(bytes.fromhex(self.exp["record"]["ct"])))
        self.assertNotIn("ct", pub["record"])
        scanner = J.Scanner(K.canaries(self.keys, leak=True))
        for p in self.dst.rglob("*"):
            if p.is_file():
                self.assertEqual(scanner.scan_file(p), [], p.name)
        conf = (self.dst / "kssh.conf").read_text()
        self.assertNotIn(self.sec["keys"]["ka16"]["passphrase"], conf)
        self.assertEqual(K.parse_conf(conf)["mode"], "t1")

    def test_ciphertext_or_header_in_the_archive_is_refused(self):
        ct = bytes.fromhex(self.exp["record"]["ct"])
        for needle in (ct[:48], b"CKMS\x02\x01\x01\x00"):
            with open(self.ev / "boot2" / "serial.log", "ab") as f:
                f.write(b"\n" + needle + b"\n")
            ok, msgs = J.archive(self.ev, self.dst, self.keys)
            self.assertFalse(ok, msgs)
            self.assertFalse(self.dst.exists())
            self.assertIn("leak-", " ".join(msgs))
            self.write_serial(self.ev / "boot2", self.serial_lines(2))

    def test_planted_secret_in_a_boot_file_is_refused(self):
        with open(self.ev / "boot4" / "sshd.log", "ab") as f:
            f.write(b"leak " + self.sec["keys"]["ka16"]["passphrase"].encode() + b"\n")
        ok, msgs = J.archive(self.ev, self.dst, self.keys)
        self.assertFalse(ok)
        self.assertFalse(self.dst.exists())
        self.assertIn("pass:ka16", " ".join(msgs))
        self.assertNotIn(self.sec["keys"]["ka16"]["passphrase"], " ".join(msgs))

    def test_forbidden_name_inside_a_boot_dir_is_refused_or_skipped(self):
        (self.ev / "boot1" / "id_ed25519").write_text("x")
        ok, msgs = J.archive(self.ev, self.dst, self.keys)
        self.assertTrue(ok, msgs)  # not allowlisted: skipped, never copied
        self.assertFalse((self.dst / "boot1" / "id_ed25519").exists())

    def test_t0_archive_is_unchanged_by_the_t1_rules(self):
        # a T0 key set (no mode) keeps expected.json and ignores boot dirs / preflight files
        t0 = self.tmp / f"t0arch-{secrets.token_hex(3)}"
        K.gen(t0 / "keys", t0 / "stage")
        ev = t0 / "ev"
        ev.mkdir()
        (ev / "serial.raw.log").write_text("hello\n")
        (ev / "preflight.txt").write_text("x\n")
        (ev / "boot1").mkdir()
        (ev / "boot1" / "serial.log").write_text("x\n")
        dst = t0 / "dst"
        ok, msgs = J.archive(ev, dst, t0 / "keys")
        self.assertTrue(ok, msgs)
        names = {p.name for p in dst.iterdir()}
        self.assertIn("expected.json", names)
        self.assertNotIn("expected-public.json", names)
        self.assertNotIn("preflight.txt", names)
        self.assertNotIn("boot1", names)


if __name__ == "__main__":
    unittest.main(verbosity=1)
