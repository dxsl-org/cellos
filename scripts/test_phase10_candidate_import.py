#!/usr/bin/env python3
"""Host tests for `scripts/phase10_candidate_import.py` (S10-3.3 import mode; no cargo, no QEMU).

Cells are minimal ELF64 files with a real `__ViCell_sig` section signed with throw-away
Ed25519 keys; the signed payload is computed by the real `scripts/sign-cell.py`.

    python3 -B scripts/test_phase10_candidate_import.py
"""

from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

REPO = Path(__file__).resolve().parent.parent


def load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


imp = load("phase10_candidate_import", REPO / "scripts" / "phase10_candidate_import.py")
RUNNERS = ["qemu-phase02-session-test.sh", "qemu-phase03-pipeline-test.sh", "qemu-phase04-terminal-test.sh",
           "qemu-phase04-gui-test.sh", "qemu-phase09-ssh-test.sh"]


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def key(seed_byte: int) -> Ed25519PrivateKey:
    return Ed25519PrivateKey.from_private_bytes(bytes([seed_byte]) * 32)


def pub(private: Ed25519PrivateKey) -> bytes:
    return private.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


def signed_elf(body: bytes, private: Ed25519PrivateKey) -> bytes:
    """ELF64 header, body, 64-byte `__ViCell_sig`, .shstrtab, three section headers."""
    strtab = b"\x00__ViCell_sig\x00.shstrtab\x00"
    sig_off = 64 + len(body)
    str_off = sig_off + 64
    sh_off = str_off + len(strtab)
    header = bytearray(64)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<Q", header, 40, sh_off)
    struct.pack_into("<HHH", header, 58, 64, 3, 2)

    def shdr(name: int, kind: int, offset: int, size: int) -> bytes:
        entry = bytearray(64)
        struct.pack_into("<II", entry, 0, name, kind)
        struct.pack_into("<QQ", entry, 24, offset, size)
        return bytes(entry)

    sections = shdr(0, 0, 0, 0) + shdr(1, 1, sig_off, 64) + shdr(14, 3, str_off, len(strtab))
    unsigned = bytes(header) + body + bytes(64) + strtab + sections
    payload = unsigned[:sig_off] + unsigned[sig_off + 64:]
    return unsigned[:sig_off] + private.sign(payload) + unsigned[sig_off + 64:]


DEV, LANE, OTHER = key(0x43), key(0x11), key(0x22)


class Fixture:
    """A candidate.json with a headless profile and a lane work dir."""

    def __init__(self, root: Path):
        self.root = root
        cand = root / "cand"
        (cand / "headless" / "files").mkdir(parents=True)
        self.bodies = {"cellbox": b"cellbox-code" * 7, "vfs": b"vfs-code" * 5, "ls": b"ls-code" * 3}
        files = {}
        for name, body in self.bodies.items():
            data = signed_elf(body, DEV)
            path = cand / "headless" / "files" / name
            path.write_bytes(data)
            row = {"file": str(path), "sha256": sha(data), "size": len(data), "binary": name, "package": name,
                   "signed": True, "payload_sha256": imp.payload_sha256(data), "dev_signature_valid": True}
            files["/bin/" + name] = row
        files["/bin/cellbox-ssh"] = dict(files["/bin/cellbox"])
        init = signed_elf(b"init-code" * 9, DEV)
        (cand / "headless" / "init").write_bytes(init)
        self.record = {
            "schema": "cellos-x86-candidate/1", "root": str(cand),
            "profiles": {"headless": {"dir": str(cand / "headless"), "files": files,
                                      "init": {"path": str(cand / "headless" / "init"), "sha256": sha(init),
                                               "size": len(init), "payload_sha256": imp.payload_sha256(init),
                                               "dev_signature_valid": True}}},
            "failures": [],
        }
        self.candidate = cand / "candidate.json"
        self.write_candidate()
        self.release = root / "work" / "cells-target" / "release"
        self.release.mkdir(parents=True)
        self.evidence = root / "work" / "evidence"
        self.evidence.mkdir()
        # Lane builds (what cargo would have left): different code, unsigned stand-ins.
        for name in ("cellbox", "service-vfs", "ls", "phase03-filter", "app-init"):
            (self.release / name).write_bytes(signed_elf(b"lane-" + name.encode(), OTHER))
        self.cells = [("/bin/cellbox", str(self.release / "cellbox")),
                      ("/bin/vfs", str(self.release / "service-vfs")),
                      ("/bin/ls", str(self.release / "ls")),
                      ("/bin/p03-filter", str(self.release / "phase03-filter"))]

    def write_candidate(self):
        self.candidate.write_text(json.dumps(self.record, indent=2) + "\n")

    def do_import(self, cells=None, lane_only=None):
        return imp.do_import(self.candidate, "headless", "test-lane", self.release, self.evidence,
                             cells or self.cells, self.release / "app-init", lane_only or {})

    def resign(self, private=LANE, tamper: str | None = None):
        """What the runner's signing call does: same payload, new signature (or a changed body)."""
        for path, lane_file in self.cells + [("init", str(self.release / "app-init"))]:
            name = Path(lane_file).name
            source = {"service-vfs": "vfs", "app-init": None}.get(name, name)
            if path == "/bin/p03-filter":
                continue
            if source is None:
                body = b"init-code" * 9
            else:
                body = self.bodies[source]
            if tamper == path:
                body = body + b"!"
            Path(lane_file).write_bytes(signed_elf(body, private))


class ImportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.fx = Fixture(Path(self.tmp.name))

    def tearDown(self):
        self.tmp.cleanup()

    def test_shared_copies_verified_lane_only_listed_and_candidate_hash_recorded(self):
        fx = self.fx
        out = fx.do_import()
        self.assertEqual(out["candidate_json"]["sha256"], sha(fx.candidate.read_bytes()))
        self.assertEqual(out["mode"], "import")
        self.assertEqual([r["path"] for r in out["shared"]], ["/bin/cellbox", "/bin/vfs", "/bin/ls", "init"])
        self.assertEqual(out["lane_only"], [{"path": "/bin/p03-filter", "lane_file": str(fx.release / "phase03-filter"),
                                             "reason": "not packaged in the candidate profile"}])
        files = fx.record["profiles"]["headless"]["files"]
        self.assertEqual(sha((fx.release / "service-vfs").read_bytes()), files["/bin/vfs"]["sha256"])
        self.assertEqual(sha((fx.release / "app-init").read_bytes()),
                         fx.record["profiles"]["headless"]["init"]["sha256"])
        self.assertEqual(json.loads((fx.evidence / imp.IMPORT_RECORD).read_text()), out)

    def test_copy_replaces_hard_link_without_writing_through(self):
        fx = self.fx
        deps = fx.root / "deps-ls"
        deps.write_bytes(b"cargo deps output")
        (fx.release / "ls").unlink()
        os.link(deps, fx.release / "ls")
        fx.do_import()
        self.assertEqual(deps.read_bytes(), b"cargo deps output")

    def test_candidate_sha_mismatch_is_refused(self):
        fx = self.fx
        fx.record["profiles"]["headless"]["files"]["/bin/vfs"]["sha256"] = "0" * 64
        fx.write_candidate()
        with self.assertRaisesRegex(imp.ImportRefused, r"/bin/vfs: candidate file sha256 .* != candidate.json"):
            fx.do_import()

    def test_candidate_file_changed_on_disk_is_refused(self):
        fx = self.fx
        Path(fx.record["profiles"]["headless"]["files"]["/bin/ls"]["file"]).write_bytes(signed_elf(b"x", DEV))
        with self.assertRaisesRegex(imp.ImportRefused, r"/bin/ls: candidate file sha256"):
            fx.do_import()
        argv = ["import", "--candidate", str(fx.candidate), "--profile", "headless", "--lane", "t",
                "--release-dir", str(fx.release), "--evidence", str(fx.evidence),
                "--init", str(fx.release / "app-init"), "--cell", f"/bin/ls={fx.release / 'ls'}"]
        self.assertEqual(imp.main(argv), 1)

    def test_candidate_with_failures_or_wrong_schema_is_refused(self):
        fx = self.fx
        fx.record["failures"] = ["headless: kernel ELF does not embed the exact image/init"]
        fx.write_candidate()
        with self.assertRaisesRegex(imp.ImportRefused, "records failures"):
            fx.do_import()
        fx.record["failures"], fx.record["schema"] = [], "other/1"
        fx.write_candidate()
        with self.assertRaisesRegex(imp.ImportRefused, "schema"):
            fx.do_import()

    def test_lane_only_override_keeps_the_lane_build(self):
        fx = self.fx
        before = (fx.release / "service-vfs").read_bytes()
        out = fx.do_import(lane_only={"/bin/vfs": "test-hooks flavour"})
        self.assertEqual((fx.release / "service-vfs").read_bytes(), before)
        self.assertIn({"path": "/bin/vfs", "lane_file": str(fx.release / "service-vfs"), "reason": "test-hooks flavour"},
                      out["lane_only"])
        with self.assertRaisesRegex(imp.ImportRefused, "does not package"):
            fx.do_import(lane_only={"/bin/nope": "x"})

    def test_one_lane_file_at_two_paths_needs_equal_candidate_bytes(self):
        fx = self.fx
        cells = fx.cells + [("/bin/cellbox-ssh", str(fx.release / "cellbox"))]
        out = fx.do_import(cells=cells)
        self.assertIn("/bin/cellbox-ssh", [r["path"] for r in out["shared"]])
        files = fx.record["profiles"]["headless"]["files"]
        other = signed_elf(b"other", DEV)
        Path(files["/bin/ls"]["file"]).write_bytes(other)
        files["/bin/ls"].update(sha256=sha(other), payload_sha256=imp.payload_sha256(other))
        fx.write_candidate()
        with self.assertRaisesRegex(imp.ImportRefused, "candidate bytes differ"):
            fx.do_import(cells=fx.cells + [("/bin/cellbox-ssh", str(fx.release / "ls"))])

    def test_copy_target_outside_release_dir_is_refused(self):
        fx = self.fx
        with self.assertRaisesRegex(imp.ImportRefused, "outside the lane release dir"):
            fx.do_import(cells=[("/bin/ls", str(fx.root / "elsewhere" / "ls"))])


class VerifyTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.fx = Fixture(Path(self.tmp.name))
        self.fx.do_import()
        self.pub = self.fx.root / "work" / "cell.pub"
        self.pub.write_bytes(pub(LANE))

    def tearDown(self):
        self.tmp.cleanup()

    def test_resigned_copies_payload_match(self):
        fx = self.fx
        fx.resign()
        out = imp.do_verify(fx.evidence, None, None, self.pub)
        self.assertTrue(out["all_equal"], out["failures"])
        for row in out["binaries"]:
            self.assertTrue(row["equal"])
            self.assertFalse(row["byte_identical"])  # only the 64 signature bytes differ
            self.assertTrue(row["lane_signature_valid"])
            self.assertNotEqual(row["lane_sha256"], row["candidate_sha256"])
            self.assertEqual(row["lane_payload_sha256"], row["candidate_payload_sha256"])
        self.assertEqual([r["path"] for r in out["lane_only"]], ["/bin/p03-filter"])
        self.assertTrue(out["lane_only"][0]["lane_only"])
        self.assertEqual(out["lane_only"][0]["lane_sha256"], sha((fx.release / "phase03-filter").read_bytes()))
        self.assertEqual(out["candidate_json_sha256"], sha(fx.candidate.read_bytes()))
        self.assertEqual(json.loads((fx.evidence / imp.PAYLOAD_MATCH).read_text()), out)

    def test_unresigned_copies_are_byte_identical(self):
        out = imp.do_verify(self.fx.evidence, None, None, None)
        self.assertTrue(out["all_equal"])
        self.assertTrue(all(r["byte_identical"] for r in out["binaries"]))

    def test_payload_mismatch_fails_and_is_recorded(self):
        fx = self.fx
        fx.resign(tamper="/bin/vfs")
        argv = ["verify", "--evidence", str(fx.evidence), "--trust-pub", str(self.pub)]
        self.assertEqual(imp.main(argv), 1)
        out = json.loads((fx.evidence / imp.PAYLOAD_MATCH).read_text())
        self.assertFalse(out["all_equal"])
        bad = [r["path"] for r in out["binaries"] if not r["equal"]]
        self.assertEqual(bad, ["/bin/vfs"])
        self.assertTrue(any(f.startswith("/bin/vfs: signed payload") for f in out["failures"]))

    def test_wrong_lane_key_fails(self):
        fx = self.fx
        fx.resign(private=OTHER)
        out = imp.do_verify(fx.evidence, None, None, self.pub)
        self.assertFalse(out["all_equal"])
        self.assertTrue(all(r["equal"] and not r["lane_signature_valid"] for r in out["binaries"]))

    def test_candidate_json_changed_after_import_fails(self):
        fx = self.fx
        fx.resign()
        fx.candidate.write_text(fx.candidate.read_text() + " ")
        out = imp.do_verify(fx.evidence, None, None, self.pub)
        self.assertFalse(out["all_equal"])
        self.assertIn("candidate.json changed since import", out["failures"][0])

    def test_assembled_image_must_carry_the_verified_bytes(self):
        fx = self.fx
        fx.resign()
        files = {path: Path(lane).read_bytes() for path, lane in fx.cells}
        init = fx.root / "embedded-init"
        init.write_bytes((fx.release / "app-init").read_bytes())
        fake = mock.Mock()
        fake.image_files.return_value = files
        with mock.patch.object(imp, "_READER", fake):
            out = imp.do_verify(fx.evidence, fx.root / "kernel_fs.img", init, self.pub)
            self.assertTrue(out["all_equal"], out["failures"])
            self.assertTrue(all(r["in_image"] for r in out["binaries"] + out["lane_only"]))
            files["/bin/ls"] = b"stale"
            out = imp.do_verify(fx.evidence, fx.root / "kernel_fs.img", init, self.pub)
        self.assertEqual(out["failures"], ["/bin/ls: assembled image bytes differ from the verified lane file"])

    def test_annotate_records_candidate_hash_mode_and_features(self):
        fx = self.fx
        fx.resign()
        imp.do_verify(fx.evidence, None, None, self.pub)
        provenance = fx.evidence / "provenance.json"
        provenance.write_text(json.dumps({"kernel_sha256": "k"}) + "\n")
        info = imp.do_annotate(fx.evidence, "default+phase02-terminal-profile", None, [provenance])
        merged = json.loads(provenance.read_text())
        self.assertEqual(merged["kernel_sha256"], "k")
        self.assertEqual(merged["candidate_import"], info)
        self.assertEqual(info["mode"], "import")
        self.assertEqual(info["candidate_json_sha256"], sha(fx.candidate.read_bytes()))
        self.assertEqual(info["kernel_fixture_features"], "default+phase02-terminal-profile")
        self.assertEqual(info["payload_match_sha256"], sha((fx.evidence / imp.PAYLOAD_MATCH).read_bytes()))
        verdict = fx.evidence / "verdict.json"
        verdict.write_text("{}")
        self.assertEqual(imp.do_annotate(fx.evidence, None, None, [verdict]), info)
        self.assertEqual(json.loads(verdict.read_text())["candidate_import"], info)

    def test_annotate_refuses_a_failed_payload_match(self):
        fx = self.fx
        fx.resign(tamper="init")
        imp.do_verify(fx.evidence, None, None, self.pub)
        with self.assertRaisesRegex(imp.ImportRefused, "payload match failed"):
            imp.do_annotate(fx.evidence, "default+x", None, [])


class RunnerWiring(unittest.TestCase):
    """Every runner verifies the payloads before it builds the kernel, i.e. before boot."""

    def test_verify_precedes_kernel_build_and_boot(self):
        for name in RUNNERS:
            text = (REPO / "scripts" / name).read_text()
            with self.subTest(runner=name):
                imp_at = text.index("phase10_candidate_import.py import")
                verify_at = text.index("phase10_candidate_import.py verify")
                kernel_at = text.index("-p cellos-kernel")
                boot_at = min(m.start() for m in re.finditer(r"qemu-system-x86_64|phase04_gui_drive\.py --work", text))
                sign_at = text.index("scripts/cellos-sign")
                self.assertLess(imp_at, sign_at)
                self.assertLess(sign_at, verify_at)
                self.assertLess(verify_at, kernel_at)
                self.assertLess(kernel_at, boot_at)

    def test_import_is_opt_in(self):
        for name in RUNNERS:
            text = (REPO / "scripts" / name).read_text()
            with self.subTest(runner=name):
                self.assertIn('CANDIDATE=""', text)
                for match in re.finditer(r"phase10_candidate_import\.py (import|verify|annotate)", text):
                    guard = text.rfind("if [[", 0, match.start())
                    self.assertIn("-n \"$CANDIDATE\"", text[guard:match.start()])


if __name__ == "__main__":
    unittest.main()
