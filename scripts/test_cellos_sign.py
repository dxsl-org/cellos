#!/usr/bin/env python3
"""Unit tests for the `cellos-sign` admission gate.

Run: `python3 scripts/test_cellos_sign.py`

Scope is the policy logic — allowlist parsing, the two F1 layers, the F5 pin
reader, the production-key guard, the rollout guards and the S10-2a candidate
routes. The general ELF sign/verify round trip is NOT covered here: it needs a
cross-built bare-metal cell and a cross objcopy, and `scripts/test-cell-signing.sh`
already covers it where those exist. A host ELF cannot stand in — gcc puts the ELF
header inside the first PT_LOAD, so objcopy's `e_shoff` rewrite lands inside the
signed payload, which never happens for a cell binary built with the kernel's
linker script. The one route round trip reads a built x86_64 `cellbox` (copied to a
temporary root, never written in place) and is skipped when none exists.
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

class CellboxContainmentTests(unittest.TestCase):
    """Rejected rollout inputs must leave prior artifacts and outputs intact."""

    def test_entire_signing_batch_is_rejected_without_mutating_earlier_targets(self):
        for name in ("cellbox", "app-cellbox", "terminal"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                earlier = write(root, "unrelated.elf", "preserve earlier bytes")
                blocked = write(root, name, "unqualified candidate")
                with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                    signing.sign_and_verify(root, [earlier, blocked], "missing-objcopy", None)
                self.assertEqual(earlier.read_text(), "preserve earlier bytes")
                self.assertEqual(blocked.read_text(), "unqualified candidate")
                self.assertFalse(Path(str(earlier) + ".signed").exists())
                self.assertFalse(Path(str(blocked) + ".signed").exists())

    def test_symlink_alias_cannot_hide_a_blocked_artifact_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            blocked = write(root, "cellbox", "unqualified candidate")
            alias = root / "renamed.elf"
            alias.symlink_to(blocked)
            with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                signing.sign_and_verify(root, [alias], "missing-objcopy", None)
            self.assertEqual(blocked.read_text(), "unqualified candidate")

    def test_blocked_symlink_name_cannot_point_to_an_unblocked_basename(self):
        for name in ("cellbox", "app-cellbox", "terminal"):
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                payload = write(root, "other.elf", "preserve payload bytes")
                blocked = root / name
                blocked.symlink_to(payload)
                with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                    signing.sign_and_verify(root, [blocked], "missing-objcopy", None)
                self.assertTrue(blocked.is_symlink())
                self.assertEqual(payload.read_text(), "preserve payload bytes")

    def test_packager_only_runs_the_assembler_and_keeps_an_existing_image(self):
        """S10-2b: G1 is open, but the packager is only a thin caller of the assembler
        (which assembles fresh and carries every residue/quarantine check)."""
        import json, subprocess
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script = write(root, "scripts/package-desktop-x86.py",
                           (REPO_ROOT / "scripts/package-desktop-x86.py").read_text())
            write(root, "scripts/assemble-x86-image.py",
                  "import json, sys\n"
                  "open(sys.argv[0] + '.argv', 'w').write(json.dumps(sys.argv[1:]))\n")
            image = write(root, "kernel/src/embedded-x86_64/kernel_fs.img", "keep existing image")
            result = subprocess.run([sys.executable, str(script)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            argv = json.loads((root / "scripts/assemble-x86-image.py.argv").read_text())
            self.assertEqual(argv, ["--profile", "desktop", "--out-dir",
                                    str(root / "kernel/src/embedded-x86_64")])
            self.assertNotIn("ROLLOUT BLOCKED", result.stderr)
            self.assertEqual(image.read_text(), "keep existing image")

    def g3_repo(self, root: Path, image: str = "not a FAT image") -> tuple[Path, Path, Path]:
        """A throw-away repository with the real G3 script and helper (never the real tree)."""
        import shutil
        for rel in ("scripts/x86/make-iso-ci.sh", "scripts/x86/check_default_kernel.py",
                    "scripts/generate-boot-config.py"):
            write(root, rel, (REPO_ROOT / rel).read_text())
        shutil.copytree(REPO_ROOT / "scripts/cellos_sign", root / "scripts/cellos_sign",
                        ignore=shutil.ignore_patterns("__pycache__"))
        write(root, "kernel/src/embedded-x86_64/kernel_fs.img", image)
        write(root, "kernel/src/embedded-x86_64/init", "init bytes")
        output = write(root, "build/vicell-x86.iso", "keep existing ISO")
        return root / "scripts/x86/make-iso-ci.sh", root / "target/x86_64-unknown-none/release/cellos-kernel", output

    @unittest.skipUnless(os.name == "posix", "ISO builder requires a POSIX shell")
    def test_default_iso_refuses_before_overwriting_previous_output(self):
        """S10-2b: the default kernel is refused by content (here: not a clean, readable image)."""
        import subprocess
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script, default, output = self.g3_repo(root)
            write(root, "target/x86_64-unknown-none/release/cellos-kernel", "kernel bytes")
            env = {k: v for k, v in os.environ.items()
                   if k not in {"X86_KERNEL", "X86_ISO_ROOT", "XORRISO"}}
            result = subprocess.run(["bash", str(script)], env=env,
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("ROLLOUT BLOCKED", result.stderr)
            self.assertEqual(output.read_text(), "keep existing ISO")
            self.assertFalse((root / "build/x86-iso-root").exists())

    @unittest.skipUnless(os.name == "posix", "ISO builder requires a POSIX shell")
    def test_default_kernel_symlink_cannot_bypass_the_content_rule(self):
        import subprocess
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script, default, output = self.g3_repo(root)
            payload = write(root, "independent/kernel.elf", "preserve kernel bytes")
            default.parent.mkdir(parents=True)
            default.symlink_to(payload)
            env = {k: v for k, v in os.environ.items()
                   if k not in {"X86_KERNEL", "X86_ISO_ROOT", "XORRISO"}}
            result = subprocess.run(["bash", str(script)], env=env,
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("ROLLOUT BLOCKED", result.stderr)
            self.assertEqual(output.read_text(), "keep existing ISO")
            self.assertEqual(payload.read_text(), "preserve kernel bytes")

    @unittest.skipUnless(os.name == "posix", "ISO builder requires a POSIX shell")
    def test_explicit_kernel_elsewhere_is_not_checked_by_the_helper(self):
        """An explicit X86_KERNEL (fixture/lane/candidate) never runs the default-kernel helper."""
        import subprocess
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            script, default, output = self.g3_repo(root)
            kernel = write(root, "elsewhere/kernel.elf", "fixture kernel")
            env = {k: v for k, v in os.environ.items() if k not in {"X86_ISO_ROOT", "XORRISO"}}
            env["X86_KERNEL"] = str(kernel)
            env["XORRISO"] = "/nonexistent/xorriso"  # stop after the guard: no ISO is built
            result = subprocess.run(["bash", str(script), str(root / "out.iso")], env=env,
                                    capture_output=True, text=True)
            self.assertNotIn("ROLLOUT BLOCKED", result.stderr)
            self.assertEqual(output.read_text(), "keep existing ISO")


class CellboxPrivateFixtureAllowanceTests(unittest.TestCase):
    """Phase 04 S1/S3-gui: `cellbox`, `app-cellbox` and `terminal` sign only for an
    explicit private fixture (env var AND an explicit non-dev seed)."""

    NAMES = ("cellbox", "app-cellbox", "terminal")

    PRIVATE_SEED = "11" * 32

    class Reached(Exception):
        """Raised by the patched signer loader: the preflight admitted the batch."""

    def env(self, **extra):
        base = {k: v for k, v in os.environ.items()
                if k not in {signing.CELLBOX_FIXTURE_ENV, signing.PROD_OVERRIDE_ENV}}
        base.update(extra)
        return mock.patch.dict(os.environ, base, clear=True)

    def reaches_signer(self, target, seed):
        """True when sign_and_verify gets past every refusal to load the signer."""
        def reached(_repo):
            raise self.Reached()
        with mock.patch.object(signing, "load_signer", reached):
            try:
                signing.sign_and_verify(target.parent, [target], "missing-objcopy", seed)
            except self.Reached:
                return True
            except signing.SigningRefused:
                return False
        self.fail("sign_and_verify returned without signing or refusing")

    def test_default_still_refuses_fixture_names(self):
        for name in self.NAMES:
            with self.subTest(name=name), self.env(), tempfile.TemporaryDirectory() as tmp:
                target = write(Path(tmp), name, "candidate")
                for seed in (None, self.PRIVATE_SEED):
                    with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                        signing.preflight_blocked_artifacts([target], seed)
                    self.assertFalse(self.reaches_signer(target, seed))
                self.assertEqual(target.read_text(), "candidate")

    def test_allowance_requires_exactly_the_env_var_and_a_private_seed(self):
        for name in self.NAMES:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as tmp:
                target = write(Path(tmp), name, "candidate")
                with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1", signing.PROD_OVERRIDE_ENV: "1"}):
                    signing.preflight_blocked_artifacts([target], self.PRIVATE_SEED)
                    self.assertTrue(self.reaches_signer(target, self.PRIVATE_SEED))
                for value in ("", "0", "true", "yes", "2"):
                    with self.env(**{signing.CELLBOX_FIXTURE_ENV: value, signing.PROD_OVERRIDE_ENV: "1"}):
                        self.assertFalse(self.reaches_signer(target, self.PRIVATE_SEED), value)

    def test_allowance_never_rides_on_the_default_or_dev_key(self):
        with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), tempfile.TemporaryDirectory() as tmp:
            for name in self.NAMES:
                target = write(Path(tmp), name, "candidate")
                for seed in (None, signing.DEV_SEED_HEX, signing.DEV_SEED_HEX.upper()):
                    with self.subTest(name=name, seed=seed):
                        with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                            signing.preflight_blocked_artifacts([target], seed)
                        self.assertFalse(self.reaches_signer(target, seed))

    def test_post_preflight_defense_refuses_dev_seed_for_every_fixture_name(self):
        # Even if the preflight regressed to admit the batch, the dev key must not
        # sign any private fixture name (cellbox, app-cellbox or terminal), directly
        # or through a symlink whose resolved name is a fixture name.
        with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in self.NAMES:
                target = write(root, name, "candidate")
                alias = root / f"{name}-alias"
                alias.symlink_to(target)
                for candidate in (target, alias):
                    with self.subTest(name=name, candidate=candidate.name):
                        with mock.patch.object(signing, "preflight_blocked_artifacts"):
                            with self.assertRaisesRegex(
                                signing.SigningRefused, "never sign with the dev key"
                            ):
                                signing.sign_and_verify(REPO_ROOT, [candidate], "missing-objcopy", None)
                        self.assertEqual(target.read_text(), "candidate")

    def test_dev_seed_constant_matches_the_signer(self):
        signer = signing.load_signer(REPO_ROOT)
        self.assertEqual(bytes.fromhex(signing.DEV_SEED_HEX), signer.DEV_SEED)

    def test_terminal_aliases_are_admitted_only_under_the_exact_conditions(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            terminal = write(root, "terminal", "candidate")
            payload = write(root, "other.elf", "candidate")
            alias = root / "terminal-alias"
            alias.symlink_to(terminal)
            hidden_dir = root / "d"
            hidden_dir.mkdir()
            hidden = hidden_dir / "terminal"
            hidden.symlink_to(payload)
            cellbox = write(root, "x/cellbox", "candidate")
            targets = ([terminal], [alias], [hidden], [cellbox, terminal])

            # Exact conditions: env == "1" and an explicit non-dev seed.
            with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1", signing.PROD_OVERRIDE_ENV: "1"}):
                for batch in targets:
                    signing.preflight_blocked_artifacts(batch, self.PRIVATE_SEED)
                    self.assertTrue(self.reaches_signer(batch[0], self.PRIVATE_SEED))

            # Any single missing condition refuses every alias form and mixed batch.
            refusals = [
                (self.env(), self.PRIVATE_SEED),
                (self.env(**{signing.CELLBOX_FIXTURE_ENV: "true"}), self.PRIVATE_SEED),
                (self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), None),
                (self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), signing.DEV_SEED_HEX),
            ]
            for context, seed in refusals:
                with context:
                    for batch in targets:
                        with self.assertRaisesRegex(signing.SigningRefused, "ROLLOUT BLOCKED"):
                            signing.preflight_blocked_artifacts(batch, seed)
                        self.assertFalse(self.reaches_signer(batch[0], seed))

    def test_a_batch_with_an_unblocked_name_is_unaffected_by_the_allowance(self):
        with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), tempfile.TemporaryDirectory() as tmp:
            other = write(Path(tmp), "other.elf", "candidate")
            signing.preflight_blocked_artifacts([other], self.PRIVATE_SEED)

    def test_quarantined_hashes_stay_refused_under_the_allowance(self):
        import hashlib
        self.assertEqual(len(signing.BLOCKED_ARTIFACT_SHA256), 2)
        with self.env(**{signing.CELLBOX_FIXTURE_ENV: "1"}), tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in (*self.NAMES, "innocent.elf"):
                target = write(root, name, "quarantined bytes")
                digest = hashlib.sha256(b"quarantined bytes").hexdigest()
                with mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
                    with self.assertRaisesRegex(signing.SigningRefused, "quarantined"):
                        signing.preflight_blocked_artifacts([target], self.PRIVATE_SEED)
                    self.assertFalse(self.reaches_signer(target, self.PRIVATE_SEED))
                self.assertEqual(target.read_text(), "quarantined bytes")


class CandidateRouteTests(unittest.TestCase):
    """Phase 10 S10-2a: the dev key signs `cellbox`/`terminal` only on a named candidate
    route, from the pinned assembler, into a root outside the repository."""

    RECIPE = Path(os.path.realpath(REPO_ROOT / "scripts" / "assemble-x86-image.py"))
    HEADLESS, DESKTOP = "x86-candidate-headless", "x86-candidate-desktop"
    CANON_HEADLESS, CANON_DESKTOP = "x86-canonical-headless", "x86-canonical-desktop"
    ALL_ENV = {signing.CELLBOX_FIXTURE_ENV: "1", signing.PROD_OVERRIDE_ENV: "1",
               "GITHUB_ACTIONS": "true", "CELLOS_SIGN_CI": "1"}

    class Reached(Exception):
        """Raised by the patched signer loader: every refusal was passed."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(os.path.realpath(tmp.name)) / "candidate"
        self.root.mkdir()

    def as_recipe(self):
        return mock.patch.object(signing, "_route_caller", return_value=self.RECIPE)

    def env(self, **values):
        base = {k: v for k, v in os.environ.items()
                if k not in {signing.CELLBOX_FIXTURE_ENV, signing.PROD_OVERRIDE_ENV,
                             *signing.CI_MARKERS}}
        base.update(values)
        return mock.patch.dict(os.environ, base, clear=True)

    def batch(self, *names, root=None):
        root = root or self.root
        return [write(root, name, f"candidate {name}") for name in names]

    def reaches(self, targets, route=None, seed=None):
        """True iff sign_and_verify passes every refusal and loads the signer."""
        def reached(_repo):
            raise self.Reached()
        with mock.patch.object(signing, "load_signer", reached):
            try:
                signing.sign_and_verify(REPO_ROOT, targets, "missing-objcopy", seed, route=route)
            except self.Reached:
                return True
        self.fail("sign_and_verify returned without signing or refusing")

    def assert_refused(self, targets, route, pattern="ROLLOUT BLOCKED", seed=None):
        before = {t: t.read_bytes() for t in targets if t.is_file() and not t.is_symlink()}
        with self.assertRaisesRegex(signing.SigningRefused, pattern):
            self.reaches(targets, route, seed)
        for target, data in before.items():
            self.assertEqual(target.read_bytes(), data, target)
            self.assertFalse(Path(str(target) + ".signed").exists())

    def route(self, name=None, root=None):
        return signing.Route(name or self.HEADLESS, root or self.root)

    # ---- the reviewed constants -------------------------------------------------------

    def test_quarantined_set_is_exactly_the_two_literals(self):
        self.assertEqual(signing.BLOCKED_ARTIFACT_SHA256, frozenset({
            "64eb28f3be3d08bf2560d6eb0b58bee4a33ff8ea3439bcd5ed6e1ccc42f84a39",
            "636497acc2bdf735471173c5c428e61d16b1d8c870935cfe868e6d0120e52fa2",
        }))
        self.assertEqual(len(signing.BLOCKED_ARTIFACT_SHA256), 2)

    def test_route_pin_is_the_reviewed_assembler(self):
        import hashlib
        digest = hashlib.sha256(self.RECIPE.read_bytes()).hexdigest()
        self.assertEqual(signing.X86_CANDIDATE_RECIPE_SHA256, digest,
                         "scripts/assemble-x86-image.py changed: re-review and re-pin the route")
        self.assertEqual(set(signing.QUALIFIED_ROUTES),
                         {self.HEADLESS, self.DESKTOP, self.CANON_HEADLESS, self.CANON_DESKTOP})
        for spec in signing.QUALIFIED_ROUTES.values():
            self.assertEqual(spec.recipe_sha256, digest)
            self.assertNotIn("app-cellbox", spec.binaries)
        self.assertEqual(signing.QUALIFIED_ROUTES[self.HEADLESS].fixture_names, {"cellbox"})
        self.assertEqual(signing.QUALIFIED_ROUTES[self.DESKTOP].fixture_names,
                         {"cellbox", "terminal"})

    # ---- positive -----------------------------------------------------------------------

    def test_headless_route_admits_its_batch_with_the_dev_key(self):
        targets = self.batch("cellbox", "platform", "service-vfs", "ls", "app-init")
        with self.env(), self.as_recipe():
            self.assertTrue(self.reaches(targets, self.route()))

    def test_desktop_route_admits_terminal(self):
        targets = self.batch("cellbox", "terminal", "service-compositor", "chrome", "app-init")
        with self.env(), self.as_recipe():
            self.assertTrue(self.reaches(targets, self.route(self.DESKTOP)))

    @unittest.skipUnless(
        (REPO_ROOT / "target/x86_64-unknown-none/release/cellbox").is_file()
        and __import__("shutil").which("objcopy"),
        "needs a built x86_64 cellbox ELF (read-only) and objcopy")
    def test_route_signature_verifies_against_the_dev_key(self):
        import hashlib, shutil, subprocess
        source = REPO_ROOT / "target/x86_64-unknown-none/release/cellbox"
        before = hashlib.sha256(source.read_bytes()).hexdigest()
        target = self.root / "cellbox"
        shutil.copyfile(source, target)
        with self.env(), self.as_recipe():
            signing.sign_and_verify(REPO_ROOT, [target], "objcopy", None, route=self.route())
        verify = subprocess.run(
            [sys.executable, str(REPO_ROOT / "scripts/sign-cell.py"), "--verify", "--in", str(target)],
            capture_output=True, text=True)
        self.assertEqual(verify.returncode, 0, verify.stderr)
        self.assertIn("signature valid", verify.stdout)
        self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), before)

    # ---- negative -----------------------------------------------------------------------

    def test_unknown_route_name(self):
        with self.env(), self.as_recipe():
            self.assert_refused(self.batch("cellbox"), self.route("x86-canonical"),
                                "not a qualified route")

    def test_names_outside_the_route(self):
        cases = [
            (self.HEADLESS, "terminal", "fixture name not on this route"),
            (self.HEADLESS, "app-cellbox", "fixture name not on this route"),
            (self.DESKTOP, "app-cellbox", "fixture name not on this route"),
            (self.HEADLESS, "hotswap", "not a binary of this route"),
            (self.DESKTOP, "sshd", "not a binary of this route"),
            (self.HEADLESS, "chrome", "not a binary of this route"),
        ]
        for route, name, pattern in cases:
            with self.subTest(route=route, name=name), self.env(), self.as_recipe(), \
                    tempfile.TemporaryDirectory() as tmp:
                root = Path(os.path.realpath(tmp))
                self.assert_refused(self.batch("cellbox", name, root=root),
                                    self.route(route, root), pattern)

    def test_caller_must_be_the_pinned_recipe(self):
        targets = self.batch("cellbox")
        with self.env():
            # The test runner is the real `__main__`: not the recipe.
            self.assert_refused(targets, self.route(), "running program is not the recipe")
            for other in (None, REPO_ROOT / "scripts" / "cellos-sign",
                          REPO_ROOT / "scripts" / "package-desktop-x86.py"):
                with mock.patch.object(signing, "_route_caller", return_value=other):
                    self.assert_refused(targets, self.route(), "running program is not the recipe")
            stale = {name: signing.QualifiedRoute(spec.recipe, "0" * 64, spec.fixture_names,
                                                  spec.binaries)
                     for name, spec in signing.QUALIFIED_ROUTES.items()}
            with self.as_recipe(), mock.patch.dict(signing.QUALIFIED_ROUTES, stale):
                self.assert_refused(targets, self.route(), "differs from its reviewed SHA-256 pin")

    def test_routes_take_the_dev_key_only(self):
        targets = self.batch("cellbox")
        for seed in ("11" * 32, signing.DEV_SEED_HEX):
            with self.subTest(seed=seed[:4]), self.env(**self.ALL_ENV), self.as_recipe():
                self.assert_refused(targets, self.route(), "dev key only", seed=seed)

    def test_candidate_root_inside_the_repository(self):
        targets = self.batch("cellbox")
        link = self.root.parent / "into-repo"
        link.symlink_to(REPO_ROOT / "build")
        for root in (REPO_ROOT / "kernel/src/embedded-x86_64", REPO_ROOT / "target",
                     REPO_ROOT / "build", REPO_ROOT, link):
            if not root.is_dir():
                continue
            with self.subTest(root=str(root)), self.env(), self.as_recipe():
                self.assert_refused(targets, self.route(root=root),
                                    "candidate root is inside the repository")
        with self.env(), self.as_recipe():
            self.assert_refused(targets, self.route(root=self.root / "missing"),
                                "candidate root is not a directory")

    def test_targets_must_be_plain_unlinked_files_inside_the_root(self):
        outside = self.batch("cellbox", root=self.root.parent)
        payload = write(self.root, "store/cat", "candidate payload")
        symlink = self.root / "cat"
        symlink.symlink_to(payload)
        linked = write(self.root, "ls", "candidate ls")
        os.link(linked, self.root / "ls-hardlink")
        cases = [(outside, "outside the candidate root"),
                 ([symlink], "not a regular file"),
                 ([linked], "hard-linked target \\(2 links\\)")]
        for targets, pattern in cases:
            with self.subTest(pattern=pattern), self.env(), self.as_recipe():
                self.assert_refused(targets, self.route(), pattern)
        self.assertEqual(payload.read_text(), "candidate payload")

    def test_quarantined_hash_on_every_route_whatever_the_environment(self):
        import hashlib
        for route, name in ((self.HEADLESS, "cellbox"), (self.DESKTOP, "terminal"),
                            (self.HEADLESS, "platform")):
            with self.subTest(route=route, name=name), tempfile.TemporaryDirectory() as tmp:
                root = Path(os.path.realpath(tmp))
                targets = self.batch(name, root=root)
                digest = hashlib.sha256(targets[0].read_bytes()).hexdigest()
                with self.env(**self.ALL_ENV), self.as_recipe(), \
                        mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
                    self.assert_refused(targets, self.route(route, root), "quarantined")

    def test_no_environment_bypass_without_a_route(self):
        for name in ("cellbox", "app-cellbox", "terminal"):
            with self.subTest(name=name), self.env(**self.ALL_ENV), self.as_recipe():
                self.assert_refused(self.batch(name), None)

    def test_batch_atomicity_on_a_route(self):
        earlier = self.batch("platform", "cellbox")
        later = write(self.root.parent, "ls", "outside")
        with self.env(), self.as_recipe():
            self.assert_refused(earlier + [later], self.route(), "outside the candidate root")

    def test_route_is_rechecked_after_the_key_is_loaded(self):
        import hashlib
        targets = self.batch("cellbox")
        loaded = mock.MagicMock(DEV_SEED=bytes.fromhex(signing.DEV_SEED_HEX))
        with self.env(), self.as_recipe(), \
                mock.patch.object(signing, "preflight_blocked_artifacts"), \
                mock.patch.object(signing, "load_signer", return_value=loaded):
            with self.assertRaisesRegex(signing.SigningRefused, "not a qualified route"):
                signing.sign_and_verify(REPO_ROOT, targets, "objcopy", None,
                                        route=self.route("x86-canonical"))
            digest = hashlib.sha256(targets[0].read_bytes()).hexdigest()
            with mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
                with self.assertRaisesRegex(signing.SigningRefused, "quarantined"):
                    signing.sign_and_verify(REPO_ROOT, targets, "objcopy", None,
                                            route=self.route())
        loaded._priv_from_seed.assert_not_called()
        self.assertEqual(targets[0].read_text(), "candidate cellbox")

    def test_cli_has_no_route_and_still_refuses_dev_cellbox(self):
        target = self.batch("cellbox")[0]
        with self.assertRaises(SystemExit) as usage, \
                mock.patch("sys.stderr", new_callable=__import__("io").StringIO):
            cli.main(["--route", self.HEADLESS, "--sign", str(target)])
        self.assertEqual(usage.exception.code, 2)
        verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
        for env in ({}, self.ALL_ENV):
            with self.subTest(env=sorted(env)), self.env(**env), self.as_recipe(), \
                    mock.patch.object(cli.policy, "check", return_value=policy.Result()), \
                    mock.patch.object(cli.toolchain, "check", return_value=verified), \
                    mock.patch("sys.stderr", new_callable=__import__("io").StringIO) as err:
                self.assertEqual(cli.main(["--sign", str(target)]), cli.EXIT_SIGNING)
                self.assertIn("ROLLOUT BLOCKED", err.getvalue())
        self.assertEqual(target.read_text(), "candidate cellbox")

    def test_sign_route_runs_the_f1_f5_check_first(self):
        targets = self.batch("cellbox")
        failed = policy.Result()
        failed.violations.append(policy.Violation("token", "x", "cells/x/src/lib.rs", "unsafe"))
        verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
        with self.env(), self.as_recipe(), \
                mock.patch.object(cli.policy, "check", return_value=failed), \
                mock.patch.object(cli.toolchain, "check", return_value=verified), \
                mock.patch.object(cli.signing, "sign_and_verify") as signer, \
                mock.patch("sys.stdout", new_callable=__import__("io").StringIO), \
                mock.patch("sys.stderr", new_callable=__import__("io").StringIO):
            self.assertEqual(cli.sign_route(REPO_ROOT, self.HEADLESS, self.root, targets,
                                            "objcopy"), cli.EXIT_POLICY)
            signer.assert_not_called()
        with self.env(), \
                mock.patch.object(cli.policy, "check", return_value=policy.Result()), \
                mock.patch.object(cli.toolchain, "check", return_value=verified), \
                mock.patch.object(signing, "load_signer", side_effect=AssertionError("reached")), \
                mock.patch("sys.stderr", new_callable=__import__("io").StringIO) as err:
            # Not running as the recipe: refused with exit 3 before the signer loads.
            self.assertEqual(cli.sign_route(REPO_ROOT, self.HEADLESS, self.root, targets,
                                            "objcopy"), cli.EXIT_SIGNING)
            self.assertIn("running program is not the recipe", err.getvalue())

    # ---- S10-2b canonical routes -------------------------------------------------------

    def canonical_repo(self):
        """A throw-away repository whose `signing.REPO_ROOT` the route sees; the running
        program is its copy of the pinned assembler. The real tree is never written."""
        import shutil
        repo = Path(os.path.realpath(self.root.parent)) / "repo"
        recipe = repo / "scripts" / "assemble-x86-image.py"
        recipe.parent.mkdir(parents=True)
        shutil.copyfile(self.RECIPE, recipe)
        canonical = repo / "kernel" / "src" / "embedded-x86_64"
        canonical.mkdir(parents=True)
        return repo, recipe, canonical

    def in_repo(self, repo, recipe):
        from contextlib import ExitStack
        stack = ExitStack()
        stack.enter_context(mock.patch.object(signing, "REPO_ROOT", repo))
        stack.enter_context(mock.patch.object(signing, "_route_caller", return_value=recipe))
        return stack

    def test_canonical_routes_admit_only_the_exact_canonical_root(self):
        repo, recipe, canonical = self.canonical_repo()
        for name, names in ((self.CANON_HEADLESS, ("cellbox", "platform", "app-init")),
                            (self.CANON_DESKTOP, ("cellbox", "terminal", "chrome", "app-init"))):
            with self.subTest(route=name), self.env(), self.in_repo(repo, recipe):
                targets = self.batch(*names, root=canonical / ".assemble-x" / "inputs")
                self.assertTrue(self.reaches(targets, self.route(name, canonical)))
        for other in (repo / "kernel" / "src" / "embedded-hv", repo / "kernel" / "src",
                      repo / "target", repo / "build", repo, canonical / ".assemble-x"):
            other.mkdir(parents=True, exist_ok=True)
            with self.subTest(root=str(other.relative_to(repo))), self.env(), \
                    self.in_repo(repo, recipe):
                self.assert_refused(self.batch("cellbox", root=other),
                                    self.route(self.CANON_HEADLESS, other),
                                    "canonical root must be exactly")
        alias = repo / "alias-to-hv"
        alias.symlink_to(repo / "kernel" / "src" / "embedded-hv")
        with self.env(), self.in_repo(repo, recipe):
            self.assert_refused(self.batch("cellbox", root=alias),
                                self.route(self.CANON_HEADLESS, alias),
                                "canonical root must be exactly")
        with self.env(), self.in_repo(repo, recipe):  # outside the repository
            self.assert_refused(self.batch("cellbox"),
                                self.route(self.CANON_HEADLESS, self.root),
                                "canonical root must be exactly")

    def test_candidate_routes_stay_outside_the_repository_in_the_same_tree(self):
        repo, recipe, canonical = self.canonical_repo()
        with self.env(), self.in_repo(repo, recipe):
            self.assert_refused(self.batch("cellbox", root=canonical),
                                self.route(self.HEADLESS, canonical),
                                "candidate root is inside the repository")

    def test_canonical_routes_keep_every_other_rule(self):
        import hashlib
        repo, recipe, canonical = self.canonical_repo()
        targets = self.batch("cellbox", root=canonical / ".assemble-x" / "inputs")
        with self.env(**self.ALL_ENV), self.in_repo(repo, recipe):
            for seed in ("11" * 32, signing.DEV_SEED_HEX):
                self.assert_refused(targets, self.route(self.CANON_HEADLESS, canonical),
                                    "dev key only", seed=seed)
            self.assert_refused(self.batch("terminal", root=canonical),
                                self.route(self.CANON_HEADLESS, canonical),
                                "fixture name not on this route")
            self.assert_refused(self.batch("hotswap", root=canonical),
                                self.route(self.CANON_DESKTOP, canonical),
                                "not a binary of this route")
            digest = hashlib.sha256(targets[0].read_bytes()).hexdigest()
            with mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
                for name in (self.CANON_HEADLESS, self.CANON_DESKTOP):
                    self.assert_refused(targets, self.route(name, canonical), "quarantined")
        with self.env(), mock.patch.object(signing, "REPO_ROOT", repo):
            # not running as the recipe: refused whatever the root
            self.assert_refused(targets, self.route(self.CANON_HEADLESS, canonical),
                                "running program is not the recipe")

    def test_canonical_routes_are_unreachable_from_the_cli(self):
        target = self.batch("cellbox")[0]
        with self.assertRaises(SystemExit) as usage, \
                mock.patch("sys.stderr", new_callable=__import__("io").StringIO):
            cli.main(["--route", self.CANON_HEADLESS, "--sign", str(target)])
        self.assertEqual(usage.exception.code, 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
