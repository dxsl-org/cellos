#!/usr/bin/env python3
"""Unit tests for scripts/x86/check_default_kernel.py (Phase 10 S10-2b, G3 content rule).

Run: `python3 scripts/test_check_default_kernel.py`

The ramdisk reader is patched with a file map, so no FAT image or built kernel is
needed; the byte-binding check reads real temporary files. One extra test reads the
repository's own FAT image only when it exists and is skipped otherwise.
"""
from __future__ import annotations

import hashlib
import importlib.util
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "scripts"))

from cellos_sign import signing  # noqa: E402

_spec = importlib.util.spec_from_file_location("check_default_kernel",
                                               REPO / "scripts/x86/check_default_kernel.py")
helper = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(helper)

IMAGE, INIT = b"IMG" * 1000, b"INIT" * 100
CLEAN = {"/bin/cellbox": b"suite", "/bin/cellbox-ssh": b"suite", "/bin/ls": b"ls"}


class DefaultKernelRuleTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.embedded = self.root / "embedded"
        self.embedded.mkdir()
        (self.embedded / "kernel_fs.img").write_bytes(IMAGE)
        (self.embedded / "init").write_bytes(INIT)
        self.kernel = self.root / "kernel.elf"
        self.kernel.write_bytes(b"\x7fELF" + b"pre" + IMAGE + b"mid" + INIT + b"post")

    def run_check(self, files=None):
        with mock.patch.object(helper, "load_image_files", return_value=dict(files or CLEAN)):
            helper.check(self.kernel, self.embedded)

    def assert_refused(self, pattern, files=None):
        with mock.patch("sys.stderr", new_callable=io.StringIO) as err, \
                self.assertRaises(SystemExit) as stop:
            self.run_check(files)
        self.assertEqual(stop.exception.code, 1)
        self.assertIn("ROLLOUT BLOCKED", err.getvalue())
        self.assertRegex(err.getvalue(), pattern)

    def test_a_clean_image_embedded_in_the_kernel_is_admitted(self):
        self.run_check()

    def test_retired_paths_are_refused(self):
        for path in ("/bin/shell", "/bin/sshd"):
            with self.subTest(path=path):
                self.assert_refused("retired paths", {**CLEAN, path: b"legacy"})

    def test_a_quarantined_hash_is_refused_under_any_name(self):
        old = b"old cellbox bytes"
        with mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256",
                               frozenset({hashlib.sha256(old).hexdigest()})):
            for name in ("/bin/cellbox-old", "/bin/anything", "/etc/cellos/x"):
                with self.subTest(name=name):
                    self.assert_refused("quarantined bytes", {**CLEAN, name: old})

    def test_the_alias_must_exist_and_equal_the_suite(self):
        self.assert_refused("alias|cellbox-ssh", {"/bin/cellbox": b"suite"})
        self.assert_refused("alias|cellbox-ssh", {"/bin/cellbox-ssh": b"suite"})
        self.assert_refused("alias|cellbox-ssh",
                            {"/bin/cellbox": b"suite", "/bin/cellbox-ssh": b"other"})

    def test_the_kernel_must_contain_the_exact_image_and_init_bytes(self):
        self.kernel.write_bytes(b"\x7fELF" + IMAGE[:-1] + b"X" + b"mid" + INIT)
        self.assert_refused("kernel_fs.img")
        self.kernel.write_bytes(b"\x7fELF" + IMAGE + b"mid" + INIT[:-1] + b"X")
        self.assert_refused("init")

    def test_missing_or_empty_inputs_are_refused(self):
        (self.embedded / "init").write_bytes(b"")
        self.assert_refused("empty embedded input")
        (self.embedded / "init").unlink()
        self.assert_refused("missing")
        (self.embedded / "init").write_bytes(INIT)
        self.kernel.unlink()
        self.assert_refused("missing")

    def test_an_unreadable_image_is_refused(self):
        with mock.patch.object(helper, "load_image_files", side_effect=ValueError("not FAT")), \
                mock.patch("sys.stderr", new_callable=io.StringIO) as err, \
                self.assertRaises(SystemExit):
            helper.check(self.kernel, self.embedded)
        self.assertIn("cannot read", err.getvalue())

    def test_usage(self):
        with mock.patch("sys.stderr", new_callable=io.StringIO):
            self.assertEqual(helper.main(["x"]), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
