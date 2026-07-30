#!/usr/bin/env python3
"""Unit tests for the `cellos-sign` admission gate.

Run: `python3 scripts/test_cellos_sign.py`

Scope is the policy logic — allowlist parsing, the two F1 layers, the F5 pin
reader and the production-key guard. The ELF sign/verify round trip is NOT
covered here: it needs a cross-built bare-metal cell and a cross objcopy, and
`scripts/test-cell-signing.sh` already covers it where those exist. A host ELF
cannot stand in — gcc puts the ELF header inside the first PT_LOAD, so objcopy's
`e_shoff` rewrite lands inside the signed payload, which never happens for a
cell binary built with the kernel's linker script.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

from cellos_sign import allowlist, cli, policy, scan, signing, toolchain  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent

GOOD_ENTRY = """
version = 1
max_age_days = 90

[[file]]
path = "cells/x/src/raw.rs"
class = "driver-mmio"
reason = "MMIO"
approver = "someone"
date = "2026-07-01"

[[crate]]
name = "x"
class = "driver-mmio"
reason = "owns raw.rs"
approver = "someone"
date = "2026-07-01"
"""


def write(root: Path, rel: str, text: str) -> Path:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return path


class AllowlistTests(unittest.TestCase):
    def load(self, text: str) -> allowlist.Allowlist:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "a.toml", text)
            return allowlist.load(root, "a.toml")

    def test_parses_both_tables(self):
        allow = self.load(GOOD_ENTRY)
        self.assertIn("cells/x/src/raw.rs", allow.files)
        self.assertIn("x", allow.crates)

    def test_missing_required_field_is_fatal(self):
        # A typo'd entry must not degrade into "not allowlisted, nobody warned".
        text = GOOD_ENTRY.replace('approver = "someone"\ndate = "2026-07-01"\n\n[[crate]]',
                                  'date = "2026-07-01"\n\n[[crate]]')
        with self.assertRaises(allowlist.AllowlistError):
            self.load(text)

    def test_unknown_field_is_fatal(self):
        with self.assertRaises(allowlist.AllowlistError):
            self.load(GOOD_ENTRY + '\nexpires = "never"\n[[file]]\npath = "p"\n'
                      'class = "c"\nreason = "r"\napprover = "a"\ndate = "2026-07-01"\n')

    def test_duplicate_path_is_fatal(self):
        with self.assertRaises(allowlist.AllowlistError):
            self.load(GOOD_ENTRY + GOOD_ENTRY.split("max_age_days = 90", 1)[1])

    def test_wrong_version_is_fatal(self):
        with self.assertRaises(allowlist.AllowlistError):
            self.load(GOOD_ENTRY.replace("version = 1", "version = 2"))

    def test_staleness_uses_review_by_then_max_age(self):
        allow = self.load(GOOD_ENTRY)
        entry = allow.files["cells/x/src/raw.rs"]
        self.assertFalse(entry.is_stale(_dt.date(2026, 8, 1), 90))
        self.assertTrue(entry.is_stale(_dt.date(2026, 12, 1), 90))
        dated = allowlist.Entry(**{**entry.__dict__, "review_by": _dt.date(2026, 7, 5)})
        self.assertTrue(dated.is_stale(_dt.date(2026, 7, 6), 9999))


class PolicyTests(unittest.TestCase):
    """Runs against a throwaway tree; `tracked_sources` falls back to a walk."""

    def build(self, tmp: str, root_src: str, extra: dict[str, str] | None = None) -> Path:
        root = Path(tmp)
        write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
        write(root, "cells/x/src/main.rs", root_src)
        for rel, text in (extra or {}).items():
            write(root, rel, text)
        write(root, "a.toml", "version = 1\n")
        return root

    def check(self, root: Path, allow_text: str = "version = 1\n") -> policy.Result:
        write(root, "a.toml", allow_text)
        return policy.check(root, allowlist.load(root, "a.toml"))

    def test_clean_crate_with_attribute_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.build(tmp, "#![forbid(unsafe_code)]\nfn main() {}\n")
            self.assertTrue(self.check(root).ok)

    def test_missing_attribute_is_a_violation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.build(tmp, "fn main() {}\n")
            result = self.check(root)
            self.assertFalse(result.ok)
            self.assertEqual(result.violations[0].layer, "attribute")
            self.assertEqual(result.violations[0].crate, "x")

    def test_unsafe_in_a_file_outside_the_module_graph_is_caught(self):
        # The whole point of the token layer: rustc never compiles orphan.rs.
        with tempfile.TemporaryDirectory() as tmp:
            root = self.build(
                tmp, "#![forbid(unsafe_code)]\nfn main() {}\n",
                {"cells/x/src/orphan.rs": "fn f() { unsafe { } }\n"},
            )
            result = self.check(root)
            self.assertFalse(result.ok)
            self.assertEqual([v.layer for v in result.violations], ["token"])
            self.assertTrue(result.violations[0].path.endswith("orphan.rs"))

    def test_unsafe_in_a_comment_is_not_a_violation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.build(
                tmp, "#![forbid(unsafe_code)]\nfn main() {}\n",
                {"cells/x/src/doc.rs": "// this is not unsafe\n/* nor unsafe */\n"},
            )
            self.assertTrue(self.check(root).ok)

    def test_allowlist_clears_both_layers_and_reports_unused_entries(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self.build(tmp, "fn main() { unsafe { } }\n")
            allow_text = (
                'version = 1\n'
                '[[file]]\npath = "cells/x/src/main.rs"\nclass = "c"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
                '[[crate]]\nname = "x"\nclass = "c"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
                '[[file]]\npath = "cells/x/src/gone.rs"\nclass = "c"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
            )
            result = self.check(root, allow_text)
            self.assertTrue(result.ok)
            self.assertEqual(result.unused_file_entries, ["cells/x/src/gone.rs"])


class ScannerLiteralTests(unittest.TestCase):
    """A literal must never hide code from either layer.

    Each case below is a *false negative* if the scanner reads literals as code:
    the delimiter that opens a comment, or the attribute itself, can be spelled
    inside a string, so the reduction has to lex literals exactly as rustc does.
    """

    def unsafe(self, src: str) -> int:
        return scan.count_unsafe(scan.strip_noncode(src))

    def forbid(self, src: str) -> bool:
        return scan.has_forbid(scan.strip_noncode(src))

    def test_block_comment_opener_in_a_string_does_not_swallow_code(self):
        self.assertEqual(self.unsafe('const P: &str = "/*";\nfn f() { unsafe { } }\n'), 1)

    def test_line_comment_marker_in_a_string_does_not_swallow_code(self):
        self.assertEqual(self.unsafe('let u = "a // b"; unsafe { g() }\n'), 1)

    def test_attribute_spelled_inside_a_string_is_not_the_attribute(self):
        self.assertFalse(self.forbid('const S: &str = "#![forbid(unsafe_code)]";\n'))

    def test_raw_string_does_not_swallow_code(self):
        self.assertEqual(self.unsafe('const P: &str = r#"/*"#;\nfn f() { unsafe { } }\n'), 1)

    def test_combined_fake_attribute_and_hidden_comment_opener(self):
        # The bypass both layers had to miss at once: a counterfeit attribute in
        # one literal, a comment opener in another eating the real `unsafe`.
        src = (
            'const A: &str = "#![forbid(unsafe_code)]";\n'
            'const B: &str = "/*";\n'
            'fn f() { unsafe { g() } }\n'
        )
        self.assertFalse(self.forbid(src))
        self.assertEqual(self.unsafe(src), 1)

    def test_quote_and_comment_openers_in_char_and_byte_literals(self):
        self.assertEqual(self.unsafe("let q = '\"'; unsafe { }\n"), 1)
        self.assertEqual(self.unsafe('let b = b"/*"; unsafe { }\n'), 1)
        self.assertEqual(self.unsafe("let e = '\\''; unsafe { }\n"), 1)
        self.assertEqual(self.unsafe("let s = b'\\\\'; unsafe { }\n"), 1)

    def test_lifetimes_are_not_char_literals(self):
        # `'a` opens no literal, so the `unsafe` after it must still be seen.
        self.assertEqual(self.unsafe("fn f<'a>(x: &'a str) { unsafe { } }\n"), 1)

    def test_real_attribute_and_real_comments_still_read_correctly(self):
        src = "//! docs mentioning unsafe\n#![forbid(unsafe_code)]\n/* unsafe */\nfn f() {}\n"
        self.assertTrue(self.forbid(src))
        self.assertEqual(self.unsafe(src), 0)

    def test_attribute_must_start_a_line(self):
        self.assertTrue(self.forbid("  #![forbid(unsafe_code)]\n"))
        self.assertFalse(self.forbid("let x = 1; #![forbid(unsafe_code)]\n"))

    def test_stripping_preserves_line_structure(self):
        reduced = scan.strip_noncode('let x = "a\nb";\n#![forbid(unsafe_code)]\n')
        self.assertTrue(scan.has_forbid(reduced))
        self.assertEqual(len(reduced.splitlines()), 3)

    def test_a_string_hiding_a_comment_opener_fails_the_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs",
                  'const A: &str = "#![forbid(unsafe_code)]";\n'
                  'const B: &str = "/*";\n'
                  'fn main() { unsafe { } }\n')
            write(root, "a.toml", "version = 1\n")
            result = policy.check(root, allowlist.load(root, "a.toml"))
            self.assertFalse(result.ok)
            self.assertEqual({v.layer for v in result.violations}, {"attribute", "token"})


class ToolchainTests(unittest.TestCase):
    def test_reads_the_pin_from_disk(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "rust-toolchain.toml", '[toolchain]\nchannel = "nightly-2026-05-01"\n')
            self.assertEqual(toolchain.pinned_channel(root), "nightly-2026-05-01")

    def test_absent_pin_reads_as_none_and_fails_the_check(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertIsNone(toolchain.pinned_channel(root))
            self.assertFalse(toolchain.check(root).ok)


class ProdKeyGuardTests(unittest.TestCase):
    def setUp(self):
        self._saved = {k: os.environ.pop(k, None)
                       for k in (*signing.CI_MARKERS, signing.PROD_OVERRIDE_ENV)}

    def tearDown(self):
        for key, value in self._saved.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def test_dev_key_is_always_allowed(self):
        signing.guard_prod_key(None)  # must not raise

    def test_prod_key_refused_outside_ci(self):
        with self.assertRaises(signing.SigningRefused):
            signing.guard_prod_key("00" * 32)

    def test_prod_key_allowed_in_ci(self):
        os.environ["GITHUB_ACTIONS"] = "true"
        signing.guard_prod_key("00" * 32)

    def test_falsey_ci_marker_does_not_count(self):
        os.environ["GITHUB_ACTIONS"] = "false"
        with self.assertRaises(signing.SigningRefused):
            signing.guard_prod_key("00" * 32)

    def test_named_override_allows_a_kms_signer(self):
        os.environ[signing.PROD_OVERRIDE_ENV] = "1"
        signing.guard_prod_key("00" * 32)


class AdmissionSentinelTests(unittest.TestCase):
    """`sign-cell.py` must not mint a signature no policy check stands behind."""

    def setUp(self):
        self.signer = signing.load_signer(REPO_ROOT)
        self.addCleanup(setattr, self.signer, "_CHECKED", False)

    def args(self, **overrides) -> argparse.Namespace:
        base = dict(seed_hex=None, unchecked_dev_signature=False)
        return argparse.Namespace(**{**base, **overrides})

    def test_unchecked_direct_signing_is_refused(self):
        self.signer._CHECKED = False
        with self.assertRaises(signing.SigningRefused):
            self.signer._guard_admission(self.args())

    def test_sentinel_from_the_checked_wrapper_admits_signing(self):
        self.signer._CHECKED = True
        self.signer._guard_admission(self.args())  # must not raise

    def test_named_opt_in_admits_a_dev_signature(self):
        self.signer._CHECKED = False
        self.signer._guard_admission(self.args(unchecked_dev_signature=True))

    def test_named_opt_in_never_admits_a_production_key(self):
        self.signer._CHECKED = False
        with self.assertRaises(signing.SigningRefused):
            self.signer._guard_admission(
                self.args(unchecked_dev_signature=True, seed_hex="00" * 32)
            )


class SignPathStrictnessTests(unittest.TestCase):
    """The sign path must fail closed on an unverifiable F5, flag or no flag."""

    def sign(self, status: toolchain.ToolchainStatus) -> tuple[int, list]:
        signed: list = []
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "a.toml", "version = 1\n")
            args = argparse.Namespace(
                repo=root, allowlist="a.toml", quiet=True, strict=False,
                targets=["cell.elf"], objcopy="objcopy", seed_hex=None,
            )
            with mock.patch.object(cli.policy, "check", return_value=policy.Result()), \
                 mock.patch.object(cli.toolchain, "check", return_value=status), \
                 mock.patch.object(cli.signing, "sign_and_verify",
                                   side_effect=lambda *a, **k: signed.append(a)):
                return cli.run_sign(args), signed

    def test_skipped_f5_refuses_to_sign_even_without_the_flag(self):
        skipped = toolchain.ToolchainStatus(
            ok=True, skipped=True, detail="no working rustc on PATH")
        code, signed = self.sign(skipped)
        self.assertEqual(code, cli.EXIT_POLICY)
        self.assertEqual(signed, [])

    def test_verified_f5_signs(self):
        verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
        code, signed = self.sign(verified)
        self.assertEqual(code, cli.EXIT_OK)
        self.assertEqual(len(signed), 1)


if __name__ == "__main__":
    unittest.main(verbosity=2)
