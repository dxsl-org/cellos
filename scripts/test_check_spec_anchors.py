#!/usr/bin/env python3
"""Unit tests for the structural plan-status/link pass of scripts/check-spec-anchors.py.

Run: `python3 -m unittest scripts/test_check_spec_anchors.py`

Each test builds a throwaway repository with plan frontmatter and the three status
documents, then checks which structural findings and plan claims the checker derives.
"""
from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

_spec = importlib.util.spec_from_file_location(
    "check_spec_anchors", REPO / "scripts/check-spec-anchors.py"
)
checker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checker)

PLAN = ".agents/261001-0000-sample/plan.md"


class StructurePassTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)
        self.write(PLAN, "---\ntitle: Sample\nstatus: in-progress\n---\n\n# Sample\n")
        self.write(".agents/261001-0000-sample/phase-01-start.md", "# Phase 01\n")
        self.write(".agents/plan-portfolio.md", "# Portfolio\n\nSee [plan](261001-0000-sample/plan.md).\n")
        self.write("README.md", "# Root\n\nRead the [docs](./docs/README.md).\n")
        self.write("docs/README.md", "# Docs\n\nThe [plan dir](../.agents/261001-0000-sample/).\n")

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def structural(self):
        findings, _ = checker.collect_structure(self.root)
        self.assertTrue(all(f.severity == "structural" for f in findings))
        return [(f.spec, f.line, f.subject) for f in findings]

    def test_valid_tree_passes_and_reports_linked_plan_claims(self):
        findings, claims = checker.collect_structure(self.root)
        self.assertEqual(findings, [])
        self.assertEqual(
            claims,
            [(PLAN, "in-progress", [".agents/plan-portfolio.md:3", "docs/README.md:3"])],
        )

    def test_missing_local_link_target_is_structural(self):
        self.write("README.md", "# Root\n\nSee [gone](docs/missing.md#intro) and [ok](docs/README.md).\n")
        self.assertEqual(self.structural(), [("README.md", 3, "docs/missing.md#intro")])

    def test_external_and_anchor_only_links_are_ignored(self):
        self.write(
            "README.md",
            "# Root\n\n"
            "[web](https://example.invalid/none.md) [plain](http://example.invalid/x)\n"
            "[mail](mailto:nobody@example.invalid) [here](#no-such-heading)\n"
            "[frag](docs/README.md#no-such-heading)\n",
        )
        self.assertEqual(self.structural(), [])

    def test_links_inside_code_are_not_followed(self):
        self.write(
            "README.md",
            "# Root\n\n`[x](inline/missing.md)`\n\n```\n[y](fenced/missing.md)\n```\n",
        )
        self.assertEqual(self.structural(), [])

    def test_duplicate_frontmatter_status_is_structural(self):
        self.write(PLAN, "---\nstatus: complete\ntitle: Sample\nstatus: pending\n---\n")
        self.assertEqual(self.structural(), [(PLAN, 4, "status")])

    def test_malformed_frontmatter_status_is_structural(self):
        for status_line in ("status:", "status: [a, b]", 'status: "open', "status:pending"):
            with self.subTest(status_line=status_line):
                self.write(PLAN, f"---\n{status_line}\n---\n")
                self.assertEqual(self.structural(), [(PLAN, 2, "status")])

    def test_unterminated_frontmatter_is_structural(self):
        self.write(PLAN, "---\nstatus: pending\n\n# Sample\n")
        self.assertEqual(self.structural(), [(PLAN, 1, "frontmatter")])

    def test_quoted_and_commented_status_values_are_accepted(self):
        for status_line, expected in (
            ('status: "BLOCKED / PENDING"', "BLOCKED / PENDING"),
            ("status: done   # landed later", "done"),
        ):
            with self.subTest(status_line=status_line):
                self.write(PLAN, f"---\n{status_line}\n---\n")
                findings, claims = checker.collect_structure(self.root)
                self.assertEqual(findings, [])
                self.assertEqual(claims[0][1], expected)

    def test_plan_without_frontmatter_status_is_not_an_error(self):
        self.write(PLAN, "---\nexecutionStatus: blocked\nphases:\n  - status: done\n---\n")
        findings, claims = checker.collect_structure(self.root)
        self.assertEqual(findings, [])
        self.assertIsNone(claims[0][1])

    def test_phase_links_are_not_plan_claims(self):
        self.write(".agents/plan-portfolio.md", "[phase](261001-0000-sample/phase-01-start.md)\n")
        self.write("docs/README.md", "# Docs\n")
        findings, claims = checker.collect_structure(self.root)
        self.assertEqual(findings, [])
        self.assertEqual(claims, [])

    def test_claims_are_reported_without_rewriting_sources(self):
        before = (self.root / PLAN).read_text(encoding="utf-8")
        findings, claims = checker.collect_structure(self.root)
        rendered = checker.render_status([], findings, claims)
        self.assertIn(PLAN, rendered)
        self.assertEqual((self.root / PLAN).read_text(encoding="utf-8"), before)


if __name__ == "__main__":
    unittest.main()
