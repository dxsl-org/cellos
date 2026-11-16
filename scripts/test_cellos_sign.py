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

from cellos_sign import allowlist, cli, policy, scan, signing, tcb, toolchain  # noqa: E402

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
                      'class = "driver-mmio"\nreason = "r"\napprover = "a"\ndate = "2026-07-01"\n')

    def test_unknown_class_is_fatal(self):
        # A misspelt class would otherwise evade the class-keyed tier gate.
        with self.assertRaisesRegex(allowlist.AllowlistError, "unknown class"):
            self.load(GOOD_ENTRY.replace('class = "driver-mmio"', 'class = "c_ffi"', 1))

    def test_every_class_in_the_real_allowlist_is_known(self):
        allow = allowlist.load(REPO_ROOT)
        used = {e.klass for e in [*allow.files.values(), *allow.crates.values()]}
        self.assertLessEqual(used, allowlist.CLASSES)

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
                '[[file]]\npath = "cells/x/src/main.rs"\nclass = "driver-mmio"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
                '[[crate]]\nname = "x"\nclass = "driver-mmio"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
                '[[file]]\npath = "cells/x/src/gone.rs"\nclass = "driver-mmio"\nreason = "r"\n'
                'approver = "a"\ndate = "2026-07-01"\n'
            )
            result = self.check(root, allow_text)
            self.assertTrue(result.ok)
            self.assertEqual(result.unused_file_entries, ["cells/x/src/gone.rs"])


def ffi_allowlist(klass: str = "c-ffi", crate: str = "x", extra: str = "") -> str:
    return (
        "version = 1\n"
        f'[[file]]\npath = "cells/{crate}/src/main.rs"\nclass = "{klass}"\nreason = "r"\n'
        'approver = "a"\ndate = "2026-11-16"\n'
        f'[[crate]]\nname = "{crate}"\nclass = "{klass}"\nreason = "r"\n'
        'approver = "a"\ndate = "2026-11-16"\n' + extra
    )


def tier_exception(klass: str = "c-ffi", manifest: str = "LEGACY", review_by: str = "2027-02-14",
                   crate: str = "x") -> str:
    return (
        f'[[tier_exception]]\ncrate = "{crate}"\nclass = "{klass}"\nmanifest = "{manifest}"\n'
        'reason = "r"\napprover = "a"\ndate = "2026-11-16"\n'
        + (f'review_by = "{review_by}"\n' if review_by else "")
    )


FFI_CALL = 'extern "C" { fn c_entry() -> i32; }\nfn call() -> i32 { unsafe { c_entry() } }\n'
TIER_LESS = "abi::declare_manifest!(block_io = false, network = false, spawn = false);\n"
TIER_FFI = ("abi::declare_manifest!(block_io = false, network = false, spawn = false,\n"
            "    tier = abi::manifest::PROTECTION_CLASS_FFI);\n")


class TierClassTests(unittest.TestCase):
    """A crate with a c-ffi / legacy-raw-ptr entry must declare a Tier-2 manifest."""

    TODAY = _dt.date(2026, 11, 16)

    def check(self, main_rs: str, allow_text: str, strict: bool = False,
              extra: dict[str, str] | None = None, cargo: str = "") -> policy.Result:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml",
                  '[package]\nname = "x"\nversion = "0.1.0"\n' + cargo)
            write(root, "cells/x/src/main.rs", main_rs)
            for rel, text in (extra or {}).items():
                write(root, rel, text)
            write(root, "a.toml", allow_text)
            return policy.check(root, allowlist.load(root, "a.toml"), today=self.TODAY,
                                strict=strict)

    def tier_violations(self, result: policy.Result) -> list[policy.Violation]:
        return [v for v in result.violations if v.layer == "tier-class"]

    def test_c_ffi_with_a_tier_one_manifest_is_refused(self):
        result = self.check(TIER_LESS + FFI_CALL, ffi_allowlist())
        self.assertEqual([(v.crate, v.path) for v in result.violations], [("x", "cells/x")])
        self.assertIn("reads LEGACY", result.violations[0].detail)

    def test_ffi_and_untrusted_manifests_pass(self):
        self.assertTrue(self.check(TIER_FFI + FFI_CALL, ffi_allowlist()).ok)
        untrusted = TIER_FFI.replace("PROTECTION_CLASS_FFI", "PROTECTION_CLASS_UNTRUSTED")
        self.assertTrue(self.check(untrusted + FFI_CALL, ffi_allowlist()).ok)
        alias = TIER_FFI.replace("PROTECTION_CLASS_FFI", "TIER_TIER1B_FFI")
        self.assertTrue(self.check(alias + FFI_CALL, ffi_allowlist()).ok)

    def test_app_entry_tier_is_read(self):
        main_rs = ("ostd::app_entry!(block_io = false, network = false, spawn = false,\n"
                   "    tier = abi::manifest::PROTECTION_CLASS_UNTRUSTED,\n"
                   "    handler = |ev, rt| { let _ = (ev, rt); });\n" + FFI_CALL)
        self.assertTrue(self.check(main_rs, ffi_allowlist()).ok)

    def test_legacy_raw_ptr_without_a_manifest_is_refused(self):
        # The compositor case: no manifest at all loads as Tier 1.
        result = self.check(FFI_CALL, ffi_allowlist("legacy-raw-ptr"))
        self.assertEqual(len(self.tier_violations(result)), 1)
        self.assertIn("reads ABSENT", result.violations[0].detail)

    def test_other_classes_are_not_gated(self):
        self.assertTrue(self.check(TIER_LESS + FFI_CALL, ffi_allowlist("driver-mmio")).ok)

    def test_tier_through_a_local_constant_reads_as_tier_one(self):
        # Fail closed: only the literal abi::manifest path is believed.
        main_rs = ("const T: u8 = abi::manifest::PROTECTION_CLASS_FFI;\n"
                   "abi::declare_manifest!(block_io = false, network = false, spawn = false,"
                   " tier = T);\n" + FFI_CALL)
        result = self.check(main_rs, ffi_allowlist())
        self.assertIn("reads UNRECOGNISED", self.tier_violations(result)[0].detail)

    def test_tier_spelled_in_a_comment_or_string_does_not_count(self):
        main_rs = ("// tier = abi::manifest::PROTECTION_CLASS_FFI\n"
                   'const S: &str = "declare_manifest!(tier = abi::manifest::PROTECTION_CLASS_FFI)";\n'
                   + TIER_LESS + FFI_CALL)
        self.assertEqual(len(self.tier_violations(self.check(main_rs, ffi_allowlist()))), 1)

    def test_a_second_binary_without_a_manifest_keeps_the_crate_in_tier_one(self):
        result = self.check(TIER_FFI + FFI_CALL, ffi_allowlist(),
                            extra={"cells/x/src/bin/helper.rs": "fn main() {}\n"})
        self.assertIn("reads ABSENT+FFI", self.tier_violations(result)[0].detail)

    def test_a_hand_written_manifest_section_reads_as_tier_one(self):
        main_rs = (TIER_FFI + '#[link_section = "__ViCell_manifest"]\n'
                   "static M: [u8; 8] = [0; 8];\n" + FFI_CALL)
        result = self.check(main_rs, ffi_allowlist())
        self.assertIn("UNRECOGNISED", self.tier_violations(result)[0].detail)

    def test_a_matching_exception_admits_the_crate(self):
        result = self.check(TIER_LESS + FFI_CALL, ffi_allowlist(extra=tier_exception()),
                            strict=True)
        self.assertTrue(result.ok, [str(v) for v in result.violations])
        self.assertEqual(result.tier.used_exceptions, {("x", "c-ffi")})

    def test_an_exception_for_another_manifest_class_reopens_review(self):
        result = self.check(FFI_CALL, ffi_allowlist(extra=tier_exception(manifest="LEGACY")))
        self.assertIn("now reads ABSENT", self.tier_violations(result)[0].detail)

    def test_an_unused_exception_is_a_note_then_a_strict_violation(self):
        allow_text = ffi_allowlist(extra=tier_exception())
        relaxed = self.check(TIER_FFI + FFI_CALL, allow_text)
        self.assertTrue(relaxed.ok)
        self.assertEqual(relaxed.tier.unused_exceptions, ["x [c-ffi, manifest LEGACY]"])
        strict = self.check(TIER_FFI + FFI_CALL, allow_text, strict=True)
        self.assertEqual([v.layer for v in strict.violations], ["tier-class"])

    def test_an_overdue_exception_fails_under_strict(self):
        allow_text = ffi_allowlist(extra=tier_exception(review_by="2026-11-01"))
        self.assertTrue(self.check(TIER_LESS + FFI_CALL, allow_text).ok)
        strict = self.check(TIER_LESS + FFI_CALL, allow_text, strict=True)
        self.assertEqual([v.layer for v in strict.violations], ["stale"])

    def test_exception_schema_is_strict(self):
        for bad in (tier_exception(klass="driver-mmio"),
                    tier_exception(manifest="FFI"),
                    tier_exception(review_by=""),
                    tier_exception() + tier_exception()):
            with self.subTest(bad=bad), tempfile.TemporaryDirectory() as tmp:
                write(Path(tmp), "a.toml", "version = 1\n" + bad)
                with self.assertRaises(allowlist.AllowlistError):
                    allowlist.load(Path(tmp), "a.toml")

    def test_real_tree_exceptions_are_exactly_the_owner_triage(self):
        allow = allowlist.load(REPO_ROOT)
        self.assertEqual(
            set(allow.tier_exceptions),
            {("service-net", "legacy-raw-ptr"), ("service-compositor", "legacy-raw-ptr")},
        )


class SignRefusesTierMismatchTests(unittest.TestCase):
    """The real `--sign` path refuses a c-ffi crate with a Tier-1 manifest.

    Only F5 and the signing call are replaced; `policy.check` is the real one.
    """

    def sign(self, main_rs: str, tier1: bool) -> tuple[int, list, str]:
        signed: list = []
        out = __import__("io").StringIO()
        verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs", main_rs)
            write(root, "a.toml", ffi_allowlist())
            # The closure the generator would write for each variant, so the
            # tier-class gate is the only layer that can differ.
            write_tcb(root, roots=["x"] if tier1 else [], total=(1, 0) if tier1 else (0, 0))
            args = argparse.Namespace(
                repo=root, allowlist="a.toml", quiet=True, strict=False,
                targets=["cell.elf"], objcopy="objcopy", seed_hex=None,
            )
            with mock.patch.object(cli.toolchain, "check", return_value=verified), \
                 mock.patch.object(cli.signing, "sign_and_verify",
                                   side_effect=lambda *a, **k: signed.append(a)), \
                 mock.patch("sys.stdout", out), mock.patch("sys.stderr", out):
                return cli.run_sign(args), signed, out.getvalue()

    def test_tier_one_c_ffi_crate_is_refused_and_nothing_is_signed(self):
        code, signed, output = self.sign(TIER_LESS + FFI_CALL, tier1=True)
        self.assertEqual(code, cli.EXIT_POLICY)
        self.assertEqual(signed, [])
        self.assertIn("[tier-class] x :: cells/x", output)
        self.assertNotIn("[tcb]", output)

    def test_the_same_crate_with_an_ffi_manifest_signs(self):
        code, signed, output = self.sign(TIER_FFI + FFI_CALL, tier1=False)
        self.assertEqual(code, cli.EXIT_OK, output)
        self.assertEqual(len(signed), 1)

    def test_check_tier_class_negative_control_passes(self):
        import subprocess
        run = subprocess.run(
            [sys.executable, str(REPO_ROOT / "scripts/check-tier-class.py"), "--negative-control"],
            capture_output=True, text=True,
        )
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


def write_tcb(root: Path, roots: list[str] = (), total: tuple[int, int] = (0, 0),
              packages: dict[str, str] | None = None, sites: dict[str, tuple[int, int]] | None = None,
              unresolved: dict[str, str] | None = None) -> None:
    """A closure file and a baseline, in the shape the generator and a reviewer write."""
    closure = ["version = 1", "targets = []",
               "tier1_roots = [" + ", ".join(f'"{r}"' for r in roots) + "]"]
    for name, path in (packages or {}).items():
        closure += ["[[package]]", f'name = "{name}"', f'path = "{path}"']
    for path, reason in (unresolved or {}).items():
        closure += ["[[unresolved]]", f'path = "{path}"', f'reason = "{reason}"']
    write(root, tcb.CLOSURE_PATH, "\n".join(closure) + "\n")
    baseline = ["version = 1", "[total]", f"unsafe = {total[0]}", f"attrs = {total[1]}"]
    for path, (unsafe, attrs) in (sites or {}).items():
        baseline += ["[[site]]", f'path = "{path}"', f"unsafe = {unsafe}", f"attrs = {attrs}"]
    write(root, tcb.BASELINE_PATH, "\n".join(baseline) + "\n")


class TcbClosureTests(unittest.TestCase):
    """The Tier-1 closure ratchet: per-file ceilings plus the Tier-1 total."""

    LIB_SRC = "#[no_mangle]\npub extern \"C\" fn f() { unsafe { g() } }\nfn h() { unsafe { } }\n"

    def check(self, strict: bool = True, lib_src: str = LIB_SRC, cell_extra: str = "",
              extra_files: dict[str, str] | None = None, roots: list[str] = ("x",),
              total: tuple[int, int] = (2, 1), sites: dict | None = None,
              packages: dict | None = None, allow_text: str = "version = 1\n",
              extra: dict[str, str] | None = None) -> policy.Result:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs",
                  "#![forbid(unsafe_code)]\n" + TIER_LESS + "fn main() {}\n" + cell_extra)
            write(root, "libs/lib1/Cargo.toml", '[package]\nname = "lib1"\nversion = "0.1.0"\n')
            write(root, "libs/lib1/src/lib.rs", lib_src)
            for rel, text in (extra_files or {}).items():
                write(root, rel, text)
            write(root, "a.toml", allow_text)
            write_tcb(root, roots=list(roots), total=total,
                      packages={"lib1": "libs/lib1"} if packages is None else packages,
                      sites={"libs/lib1/src/lib.rs": (2, 1)} if sites is None else sites)
            return policy.check(root, allowlist.load(root, "a.toml"), strict=strict,
                                closure=tcb.load_closure(root),
                                baseline=tcb.load_baseline(root), extra=extra)

    def tcb_violations(self, result: policy.Result) -> list[str]:
        return [f"{v.path}: {v.detail}" for v in result.violations if v.layer == "tcb"]

    def test_tree_at_its_ceilings_passes_strict(self):
        result = self.check()
        self.assertTrue(result.ok, [str(v) for v in result.violations])
        self.assertEqual(result.tcb.total, tcb.Counts(2, 1))

    def test_one_more_unsafe_in_a_listed_file_fails(self):
        result = self.check(lib_src=self.LIB_SRC + "fn k() { unsafe { } }\n", strict=False)
        self.assertTrue(any("3 `unsafe` site(s) exceed the ceiling of 2" in v
                            for v in self.tcb_violations(result)))

    def test_one_more_unsafe_attribute_fails(self):
        result = self.check(lib_src=self.LIB_SRC + "#[export_name = \"k\"]\nfn k() {}\n",
                            strict=False)
        self.assertTrue(any("2 unsafe attribute site(s) exceed the ceiling of 1" in v
                            for v in self.tcb_violations(result)))

    def test_unsafe_in_a_new_file_without_a_row_fails(self):
        result = self.check(extra_files={"libs/lib1/src/new.rs": "fn n() { unsafe { } }\n"},
                            total=(3, 1), strict=False)
        self.assertEqual(self.tcb_violations(result),
                         ["libs/lib1/src/new.rs: Tier-1 closure file has 1 `unsafe` and 0 "
                          f"unsafe attribute(s) and no row in {tcb.BASELINE_PATH}"])

    def test_extra_code_reaches_the_scan_without_writing(self):
        result = self.check(extra={"libs/lib1/src/new.rs": "fn n() { unsafe { } }"})
        self.assertTrue(any(v.startswith("libs/lib1/src/new.rs") for v in self.tcb_violations(result)))

    def test_a_drop_is_a_note_then_a_strict_violation(self):
        lower = "#[no_mangle]\npub extern \"C\" fn f() { unsafe { g() } }\n"
        relaxed = self.check(lib_src=lower, strict=False)
        self.assertTrue(relaxed.ok)
        self.assertTrue(any("below the ceiling of 2" in n for n in relaxed.tcb_notes))
        strict = self.check(lib_src=lower)
        self.assertEqual(len(self.tcb_violations(strict)), 2)  # the file row and the total

    def test_unlinked_files_and_nested_packages_are_not_counted(self):
        result = self.check(extra_files={
            "libs/lib1/tests/it.rs": "fn t() { unsafe { } }\n",
            "libs/lib1/build.rs": "fn main() { unsafe { } }\n",
            "libs/lib1/sub/Cargo.toml": '[package]\nname = "sub"\nversion = "0.1.0"\n',
            "libs/lib1/sub/src/lib.rs": "fn s() { unsafe { } }\n",
        })
        self.assertTrue(result.ok, [str(v) for v in result.violations])

    def test_cfg_test_modules_in_src_are_counted(self):
        result = self.check(extra_files={
            "libs/lib1/src/tests.rs": "#[cfg(test)]\nfn t() { unsafe { } }\n"}, strict=False)
        self.assertTrue(any(v.startswith("libs/lib1/src/tests.rs")
                            for v in self.tcb_violations(result)))

    def test_the_total_counts_tier_one_cells(self):
        allow_text = ('version = 1\n[[file]]\npath = "cells/x/src/main.rs"\nclass = "driver-mmio"\n'
                      'reason = "r"\napprover = "a"\ndate = "2026-11-16"\n')
        cell = "fn c() { unsafe { } }\n"
        rise = self.check(cell_extra=cell, allow_text=allow_text)
        self.assertTrue(any("total 3 (cells 1 + closure 2) exceeds 2" in v
                            for v in self.tcb_violations(rise)))
        # Moving the site from the cell into the library keeps the total: passes
        # once the library's own ceiling is raised in the same change.
        moved = self.check(cell_extra=cell, allow_text=allow_text, total=(3, 1))
        self.assertTrue(moved.ok, [str(v) for v in moved.violations])

    def test_a_tier_one_cell_missing_from_the_closure_roots_fails(self):
        result = self.check(roots=[], strict=False)
        self.assertTrue(any("not a root" in v for v in self.tcb_violations(result)))

    def test_a_stale_root_is_a_strict_violation(self):
        self.assertTrue(self.check(roots=["x", "gone"], strict=False).ok)
        self.assertTrue(any("no longer a Tier-1 cell" in v
                            for v in self.tcb_violations(self.check(roots=["x", "gone"]))))

    def test_a_listed_package_that_is_gone_fails(self):
        result = self.check(packages={"lib1": "libs/lib1", "old": "libs/old"}, strict=False)
        self.assertTrue(any(v.startswith("libs/old") for v in self.tcb_violations(result)))

    def test_a_row_for_a_clean_or_missing_file_is_a_strict_violation(self):
        sites = {"libs/lib1/src/lib.rs": (2, 1), "libs/lib1/src/gone.rs": (1, 0)}
        self.assertTrue(self.check(sites=sites, strict=False).ok)
        self.assertEqual(len(self.tcb_violations(self.check(sites=sites))), 1)

    def test_baseline_schema_is_strict(self):
        for bad in ('version = 1\n[total]\nunsafe = 0\nattrs = 0\n'
                    '[[site]]\npath = "p"\nunsafe = 0\nattrs = 0\n',
                    'version = 1\n[total]\nunsafe = -1\nattrs = 0\n',
                    'version = 1\n[total]\nunsafe = 0\nattrs = 0\nceiling = 3\n',
                    'version = 1\n[total]\nunsafe = 0\n'):
            with self.subTest(bad=bad), tempfile.TemporaryDirectory() as tmp:
                write(Path(tmp), tcb.BASELINE_PATH, bad)
                with self.assertRaises(tcb.TcbError):
                    tcb.load_baseline(Path(tmp))

    def test_missing_closure_file_refuses_to_sign(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "a.toml", "version = 1\n")
            args = argparse.Namespace(repo=root, allowlist="a.toml", quiet=True, strict=False,
                                      targets=["cell.elf"], objcopy="objcopy", seed_hex=None)
            with mock.patch.object(cli.signing, "sign_and_verify") as signer, \
                 mock.patch("sys.stdout", new_callable=__import__("io").StringIO), \
                 mock.patch("sys.stderr", new_callable=__import__("io").StringIO):
                self.assertEqual(cli.run_sign(args), cli.EXIT_USAGE)
            signer.assert_not_called()

    def test_real_baseline_lists_the_nic_driver_sites(self):
        baseline = tcb.load_baseline(REPO_ROOT)
        self.assertEqual(baseline.sites["libs/nic-driver/src/dma.rs"], tcb.Counts(2, 0))
        self.assertEqual(baseline.sites["libs/nic-driver/src/ring.rs"], tcb.Counts(5, 0))

    def test_check_tcb_closure_negative_control_passes(self):
        import subprocess
        run = subprocess.run(
            [sys.executable, str(REPO_ROOT / "scripts/check-tcb-closure.py"), "--negative-control"],
            capture_output=True, text=True,
        )
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


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

    def test_a_bare_quote_after_a_digit_opens_a_string(self):
        # P18 review HIGH 2: `0"/*"` is the integer 0 and a string to rustc
        # (it compiles inside a macro call), so the `/*` must not open a comment.
        src = ('macro_rules! eat { ($($t:tt)*) => {} }\n'
               'eat!(0"/*");\n'
               'pub fn f(p: *const u8) -> u8 { unsafe { *p } }\n'
               'eat!(0"*/");\n'
               '#[no_mangle]\npub extern "C" fn g() {}\n')
        self.assertEqual(self.unsafe(src), 1)
        self.assertEqual(scan.count_unsafe_attrs(scan.strip_noncode(src)), 1)

    def test_a_quote_after_a_digit_tries_a_char_literal(self):
        # `0'"'` is the integer 0 and the char `"`: the `"` opens no string.
        src = ("macro_rules! eat { ($($t:tt)*) => {} }\n"
               "eat!(0'\"');\n"
               "pub fn f(p: *const u8) -> u8 { unsafe { *p } }\n"
               "eat!(0'\"');\n")
        self.assertEqual(self.unsafe(src), 1)

    def test_a_manifest_inside_a_digit_prefixed_string_is_not_read(self):
        from cellos_sign import tier_class
        src = ('eat!(0" abi::declare_manifest!(block_io = false, network = false, '
               'spawn = false, tier = abi::manifest::PROTECTION_CLASS_FFI); ");\n')
        self.assertEqual(tier_class.invocation_classes(scan.strip_noncode(src)), [])

    def test_letter_prefixes_still_need_a_boundary(self):
        # `r#type` is a raw identifier, not a raw string, and `cab` merely ends
        # in `b`; the code after each must stay visible.
        self.assertEqual(self.unsafe('let r#type = 1; unsafe { }\n'), 1)
        self.assertEqual(self.unsafe('let cab = 1; unsafe { }\n'), 1)

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

    def test_stale_entries_become_violations_under_strict(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs", '#![forbid(unsafe_code)]\nfn main() { }\n')
            allow_content = (
                'version = 1\n'
                'max_age_days = 90\n\n'
                '[[crate]]\n'
                'name = "old-crate"\n'
                'class = "driver-mmio"\n'
                'reason = "test"\n'
                'approver = "test"\n'
                'date = "2026-01-01"\n'
            )
            write(root, "a.toml", allow_content)
            allow = allowlist.load(root, "a.toml")
            today = _dt.date(2026, 6, 1)

            # Non-strict mode: stale entries are in stale_entries, but not violations
            normal = policy.check(root, allow, today=today, strict=False)
            self.assertTrue(normal.ok)
            self.assertEqual(len(normal.stale_entries), 1)
            self.assertEqual(len(normal.violations), 0)

            # Strict mode: stale entries become violations
            strict_res = policy.check(root, allow, today=today, strict=True)
            self.assertFalse(strict_res.ok)
            self.assertEqual(len(strict_res.stale_entries), 1)
            self.assertEqual(len(strict_res.violations), 1)
            self.assertEqual(strict_res.violations[0].layer, "stale")
            self.assertEqual(strict_res.violations[0].crate, "old-crate")


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


class ImageNameTests(unittest.TestCase):
    """`__ViCell_image` is assigned by the signer from the artifact name."""

    def setUp(self):
        self.signer = signing.load_signer(REPO_ROOT)

    def test_crate_prefix_is_stripped(self):
        name = self.signer.image_name_for
        self.assertEqual(name("target/riscv64gc-unknown-none-elf/release/service-sshd"), b"sshd")
        self.assertEqual(name("/r/app-init"), b"init")
        self.assertEqual(name("/r/driver-virtio-blk"), b"virtio-blk")
        self.assertEqual(name("/r/cellbox"), b"cellbox")

    def test_unusable_names_get_no_section(self):
        for path in ("/r/cell.elf", "/r/Service-X", "/r/" + "a" * 25, "/r/service-"):
            self.assertIsNone(self.signer.image_name_for(path), path)

    def objcopy_args(self, target: str) -> list:
        with mock.patch.object(self.signer.subprocess, "run") as run:
            self.signer._add_signature_placeholder(target, target + ".out", "objcopy")
        return run.call_args.args[0]

    def test_placeholder_pass_replaces_the_section(self):
        args = self.objcopy_args("/r/service-sshd")
        image = self.signer.IMAGE_SECTION
        self.assertIn(f"--remove-section={image}", args)
        added = [a for a in args if a.startswith(f"--add-section={image}=")]
        self.assertEqual(len(added), 1)
        # A nameless artifact still drops any inherited section and adds none.
        args = self.objcopy_args("/r/cell.elf")
        self.assertIn(f"--remove-section={image}", args)
        self.assertFalse(any(a.startswith(f"--add-section={image}") for a in args))


class SignPathStrictnessTests(unittest.TestCase):
    """The sign path must fail closed on an unverifiable F5, flag or no flag."""

    def sign(self, status: toolchain.ToolchainStatus) -> tuple[int, list]:
        signed: list = []
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "a.toml", "version = 1\n")
            write_tcb(root)
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

    def test_quarantined_bytes_reject_the_whole_batch_without_mutation(self):
        import hashlib
        digest = hashlib.sha256(b"quarantined bytes").hexdigest()
        with tempfile.TemporaryDirectory() as tmp, \
                mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
            root = Path(tmp)
            earlier = write(root, "unrelated.elf", "preserve earlier bytes")
            payload = write(root, "store/payload", "quarantined bytes")
            alias = root / "renamed.elf"
            alias.symlink_to(payload)
            for blocked in (payload, alias):
                with self.subTest(blocked=blocked.name):
                    with self.assertRaisesRegex(signing.SigningRefused, "quarantined"):
                        signing.sign_and_verify(root, [earlier, blocked], "missing-objcopy", None)
                    self.assertEqual(earlier.read_text(), "preserve earlier bytes")
                    self.assertEqual(payload.read_text(), "quarantined bytes")
                    self.assertFalse(Path(str(earlier) + ".signed").exists())

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


class KeySeparationTests(unittest.TestCase):
    """Test/release separation is by key, not by name (plan 261031-1200): the dev key
    signs every cell, `cellbox`/`terminal` included; a non-dev key signs only in CI or
    on an audited KMS signer; quarantined bytes are refused whatever the key."""

    NAMES = ("cellbox", "app-cellbox", "terminal", "service-vfs")
    OTHER_SEED = "11" * 32

    class Reached(Exception):
        """Raised by the patched signer loader: every refusal was passed."""

    def env(self, **extra):
        base = {k: v for k, v in os.environ.items()
                if k not in {signing.PROD_OVERRIDE_ENV, *signing.CI_MARKERS}}
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

    def test_dev_key_signs_every_name_and_alias(self):
        with self.env(), tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in self.NAMES:
                target = write(root, name, "candidate")
                alias = root / f"{name}-alias"
                alias.symlink_to(target)
                for candidate in (target, alias):
                    with self.subTest(candidate=candidate.name):
                        self.assertTrue(self.reaches_signer(candidate, None))

    def test_non_dev_key_needs_ci_or_the_audited_override(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = write(Path(tmp), "cellbox", "candidate")
            with self.env():
                self.assertFalse(self.reaches_signer(target, self.OTHER_SEED))
            for extra in ({"CELLOS_SIGN_CI": "1"}, {signing.PROD_OVERRIDE_ENV: "1"}):
                with self.subTest(env=sorted(extra)), self.env(**extra):
                    self.assertTrue(self.reaches_signer(target, self.OTHER_SEED))

    def test_dev_seed_constant_matches_the_signer(self):
        signer = signing.load_signer(REPO_ROOT)
        self.assertEqual(bytes.fromhex(signing.DEV_SEED_HEX), signer.DEV_SEED)

    def test_quarantined_hashes_are_refused_whatever_the_name_key_or_env(self):
        import hashlib
        self.assertEqual(len(signing.BLOCKED_ARTIFACT_SHA256), 2)
        digest = hashlib.sha256(b"quarantined bytes").hexdigest()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in (*self.NAMES, "innocent.elf"):
                target = write(root, name, "quarantined bytes")
                for env, seed in (({}, None), ({"CELLOS_SIGN_CI": "1"}, self.OTHER_SEED)):
                    with self.subTest(name=name, seed=bool(seed)), self.env(**env), \
                            mock.patch.object(signing, "BLOCKED_ARTIFACT_SHA256", frozenset({digest})):
                        with self.assertRaisesRegex(signing.SigningRefused, "quarantined"):
                            signing.preflight_blocked_artifacts([target])
                        self.assertFalse(self.reaches_signer(target, seed))
                self.assertEqual(target.read_text(), "quarantined bytes")


class CandidateRouteTests(unittest.TestCase):
    """Phase 10 S10-2a: the dev key signs `cellbox`/`terminal` only on a named candidate
    route, from the pinned assembler, into a root outside the repository."""

    RECIPE = Path(os.path.realpath(REPO_ROOT / "scripts" / "assemble-x86-image.py"))
    HEADLESS, DESKTOP = "x86-candidate-headless", "x86-candidate-desktop"
    CANON_HEADLESS, CANON_DESKTOP = "x86-canonical-headless", "x86-canonical-desktop"
    ALL_ENV = {signing.PROD_OVERRIDE_ENV: "1", "GITHUB_ACTIONS": "true", "CELLOS_SIGN_CI": "1"}

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
                if k not in {signing.PROD_OVERRIDE_ENV, *signing.CI_MARKERS}}
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
            (self.HEADLESS, "terminal", "not a binary of this route"),
            (self.HEADLESS, "app-cellbox", "not a binary of this route"),
            (self.DESKTOP, "app-cellbox", "not a binary of this route"),
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
            stale = {name: signing.QualifiedRoute(spec.recipe, "0" * 64, spec.binaries)
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

    def test_cli_has_no_route_option(self):
        target = self.batch("cellbox")[0]
        with self.assertRaises(SystemExit) as usage, \
                mock.patch("sys.stderr", new_callable=__import__("io").StringIO):
            cli.main(["--route", self.HEADLESS, "--sign", str(target)])
        self.assertEqual(usage.exception.code, 2)
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
                                "not a binary of this route")
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
