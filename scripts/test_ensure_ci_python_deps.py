#!/usr/bin/env python3
"""Unit tests for scripts/ensure-ci-python-deps.py and the committed CI pins.

Run: `python3 -m unittest scripts/test_ensure_ci_python_deps.py`
"""
from __future__ import annotations

import importlib.util
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("ensure_ci_python_deps",
                                               REPO / "scripts/ensure-ci-python-deps.py")
ensure = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ensure)

HASH_A = "--hash=sha256:" + "a" * 64
HASH_B = "--hash=sha256:" + "b" * 64


class DirectPinsTest(unittest.TestCase):
    def test_continued_requirement_and_via_dependency(self):
        text = (f"# header\nalpha==1.0 \\\n    {HASH_A} \\\n    {HASH_B}\n"
                f"beta==2.0 \\\n    {HASH_A}\n    # via alpha\n")
        self.assertEqual(ensure.direct_pins(text), [("alpha", "1.0")])

    def test_via_comment_marks_only_the_preceding_pin(self):
        text = f"alpha==1.0 {HASH_A}\n# via nothing\nbeta==2.0 {HASH_B}\n"
        self.assertEqual(ensure.direct_pins(text), [("beta", "2.0")])

    def test_unhashed_pin_is_refused(self):
        with self.assertRaisesRegex(SystemExit, "no --hash"):
            ensure.direct_pins(f"alpha==1.0 {HASH_A}\nbeta==2.0\n")

    def test_range_requirement_is_refused(self):
        with self.assertRaisesRegex(SystemExit, "exact"):
            ensure.direct_pins(f"alpha>=1.0 {HASH_A}\n")

    def test_committed_requirements(self):
        # Parsing raises on an unhashed or inexact pin; the signing library
        # stays a direct pin so runners without it install it.
        pins = dict(ensure.direct_pins(ensure.REQUIREMENTS.read_text()))
        self.assertIn("cryptography", pins)


if __name__ == "__main__":
    unittest.main()
