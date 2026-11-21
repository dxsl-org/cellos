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

from cellos_sign import allowlist, cli, elf_manifest, policy, scan, signing, tcb, toolchain  # noqa: E402

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
RAW_PTR = "fn read(p: *const u8) -> u8 { unsafe { *p } }\n"
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
        result = self.check(RAW_PTR, ffi_allowlist("legacy-raw-ptr"))
        self.assertEqual(len(self.tier_violations(result)), 1)
        self.assertIn("reads ABSENT", result.violations[0].detail)

    def test_other_classes_are_not_gated(self):
        self.assertTrue(self.check(TIER_LESS + RAW_PTR, ffi_allowlist("driver-mmio")).ok)

    def test_an_extern_block_needs_the_c_ffi_class(self):
        # P18 review LOW: a C import cannot hide behind a class the gate does not read.
        for klass in ("driver-mmio", "legacy-raw-ptr", "hypervisor"):
            with self.subTest(klass=klass):
                result = self.check(TIER_FFI + FFI_CALL, ffi_allowlist(klass))
                self.assertEqual([(v.layer, v.path) for v in result.violations],
                                 [("tier-class", "cells/x/src/main.rs")])
                self.assertIn(f"class `{klass}`", result.violations[0].detail)
        for klass in ("c-ffi", "test-harness", "test-violation"):
            with self.subTest(klass=klass):
                self.assertTrue(self.check(TIER_FFI + FFI_CALL, ffi_allowlist(klass)).ok)

    def test_an_extern_block_without_any_entry_is_refused(self):
        # A forbid-clean file can declare the import; another file calls it.
        result = self.check("#![forbid(unsafe_code)]\n" + TIER_FFI +
                            'extern "C" { fn c_entry() -> i32; }\nfn main() {}\n',
                            "version = 1\n")
        self.assertEqual([v.path for v in self.tier_violations(result)], ["cells/x/src/main.rs"])
        self.assertIn("no [[file]] entry", result.violations[0].detail)

    def test_extern_fn_and_extern_crate_are_not_blocks(self):
        src = ("#![forbid(unsafe_code)]\n" + TIER_LESS + "extern crate alloc;\n"
               'pub extern "C" fn f() {}\nfn main() {}\n')
        self.assertTrue(self.check(src, "version = 1\n").ok)

    def test_two_cells_crates_sharing_a_package_name_fail(self):
        # P18 review LOW: records are keyed by name, so a Tier-1 c-ffi crate could
        # merge into an FFI crate of the same name.
        result = self.check(TIER_LESS + FFI_CALL, ffi_allowlist(), extra={
            "cells/y/Cargo.toml": '[package]\nname = "x"\nversion = "0.1.0"\n',
            "cells/y/src/main.rs": "#![forbid(unsafe_code)]\n" + TIER_FFI + "fn main() {}\n",
        })
        self.assertTrue(any(v.layer == "tier-class" and v.path == "cells/x, cells/y"
                            and "share this package name" in v.detail
                            for v in result.violations), [str(v) for v in result.violations])

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
            {("service-net", "legacy-raw-ptr"), ("service-compositor", "legacy-raw-ptr"),
             ("driver-bcm-display", "legacy-raw-ptr")},
        )
        for exception in allow.tier_exceptions.values():
            self.assertEqual(exception.entry.approver, "lungmat8 (owner, S8b approval 2026-11-16)")


class SignRefusesTierMismatchTests(unittest.TestCase):
    """The real `--sign` path refuses a c-ffi crate with a Tier-1 manifest.

    Only F5 and the signing call are replaced; `policy.check` is the real one.
    """

    def sign(self, main_rs: str, tier1: bool, elf_class: int | None = None
             ) -> tuple[int, list, str]:
        signed: list = []
        out = __import__("io").StringIO()
        verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
        if elf_class is None:
            elf_class = 0xFF if tier1 else 2  # the class the source declares
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs", main_rs)
            write(root, "a.toml", ffi_allowlist())
            # The closure the generator would write for each variant, so the
            # tier-class gate is the only layer that can differ.
            write_tcb(root, roots=["x"] if tier1 else [], total=(1, 0) if tier1 else (0, 0))
            target = root / "build/x"
            target.parent.mkdir()
            target.write_bytes(elf_manifest.minimal_elf(elf_manifest.manifest_record(elf_class)))
            args = argparse.Namespace(
                repo=root, allowlist="a.toml", quiet=True, strict=False,
                targets=[str(target)], objcopy="objcopy", seed_hex=None,
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
              extra: dict[str, str] | None = None, signing: bool = False) -> policy.Result:
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
                                baseline=tcb.load_baseline(root), extra=extra, signing=signing)

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

    def test_a_drop_does_not_block_signing(self):
        # P18 review MED: `--sign` is strict, but a lower count never weakens what
        # the signature attests; CI's `--check --strict` still fails on it.
        lower = "#[no_mangle]\npub extern \"C\" fn f() { unsafe { g() } }\n"
        signing = self.check(lib_src=lower, signing=True)
        self.assertTrue(signing.ok, [str(v) for v in signing.violations])
        self.assertEqual(len(signing.tcb_notes), 2)
        # A rise still fails the signing run.
        rise = self.check(lib_src=self.LIB_SRC + "fn k() { unsafe { } }\n", signing=True)
        self.assertFalse(rise.ok)

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

    def test_a_tier_one_cell_missing_from_the_closure_roots_fails_ci_only(self):
        # A new cell crate: CI fails until the closure file is regenerated, signing
        # notes it (P18 review MED).
        self.assertTrue(any("not a root" in v for v in self.tcb_violations(self.check(roots=[]))))
        for relaxed in (self.check(roots=[], strict=False), self.check(roots=[], signing=True)):
            self.assertTrue(relaxed.ok, [str(v) for v in relaxed.violations])
            self.assertTrue(any("not a root" in n for n in relaxed.tcb_notes))

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


NC_CARGO = '[package]\nname = "nc"\nversion = "0.1.0"\n'


def sign_tree(files: dict[str, str], allow_text: str, targets: dict[str, bytes | None],
              total: tuple[int, int] = (100, 100),
              aliases: dict[str, str] | None = None) -> tuple[int, list, str]:
    """The real `cellos-sign --sign` path on a throwaway tree; only F5 and the
    signing call are replaced. Each target is a minimal ELF carrying the given
    manifest record (None: no section). The generous total keeps the closure
    layer quiet (a drop is only a NOTE on --sign), so the tier gates decide.
    `aliases` maps a file name under build/ to the `--as` binary name."""
    signed: list = []
    out = __import__("io").StringIO()
    verified = toolchain.ToolchainStatus(ok=True, skipped=False, detail="pinned")
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for rel, text in files.items():
            write(root, rel, text)
        write(root, "a.toml", allow_text)
        write_tcb(root, total=total)
        paths = []
        for name, record in targets.items():
            path = root / "build" / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(elf_manifest.minimal_elf(record))
            paths.append(str(path))
        as_args = [f"{root / 'build' / name}={bin_name}"
                   for name, bin_name in (aliases or {}).items()]
        args = argparse.Namespace(repo=root, allowlist="a.toml", quiet=True, strict=False,
                                  targets=paths, objcopy="objcopy", seed_hex=None,
                                  aliases=as_args)
        with mock.patch.object(cli.toolchain, "check", return_value=verified), \
             mock.patch.object(cli.signing, "sign_and_verify",
                               side_effect=lambda *a, **k: signed.append(a)), \
             mock.patch("sys.stdout", out), mock.patch("sys.stderr", out):
            return cli.run_sign(args), signed, out.getvalue()


class ElfTierGateTests(unittest.TestCase):
    """P18 review HIGH 1: the class the kernel reads is the one in the ELF.

    Repros (a)–(d) each signed before the fix; the source reading alone could
    not see that the FFI manifest never reached the binary.
    """

    FFI = elf_manifest.manifest_record(2)
    LEGACY = elf_manifest.manifest_record(0xFF)

    def refused_by_binary(self, code, signed, output, klass="ABSENT"):
        self.assertEqual(code, cli.EXIT_POLICY, output)
        self.assertEqual(signed, [])
        self.assertIn("FAIL: tier class (binary)", output)
        self.assertIn(f"binary of `nc` carries manifest {klass}", output)

    def test_a_cfg_gated_ffi_manifest_is_refused(self):
        main = "#![no_std]\n#[cfg(any())]\n" + TIER_FFI + FFI_CALL
        result = sign_tree({"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
                           ffi_allowlist(crate="nc"), {"nc": None})
        self.refused_by_binary(*result)
        self.assertNotIn("[tier-class]", result[2])  # the source alone reads FFI

    def test_b_an_ffi_manifest_in_a_never_expanded_macro_is_refused(self):
        main = "#![no_std]\nmacro_rules! unused { () => { " + TIER_FFI + " } }\n" + FFI_CALL
        self.refused_by_binary(*sign_tree(
            {"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
            ffi_allowlist(crate="nc"), {"nc": None}))

    def test_c_an_autobin_without_a_manifest_is_refused(self):
        allow_text = ("version = 1\n"
                      '[[file]]\npath = "cells/nc/src/bin/evil/main.rs"\nclass = "c-ffi"\n'
                      'reason = "r"\napprover = "a"\ndate = "2026-11-16"\n'
                      '[[crate]]\nname = "nc"\nclass = "c-ffi"\nreason = "r"\napprover = "a"\n'
                      'date = "2026-11-16"\n')
        files = {"cells/nc/Cargo.toml": NC_CARGO,
                 "cells/nc/src/main.rs": "#![no_std]\n" + TIER_FFI,
                 "cells/nc/src/bin/evil/main.rs": "#![no_std]\n" + FFI_CALL}
        code, signed, output = sign_tree(files, allow_text, {"evil": None})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        # The autobin is a root now, so the source reading already adds ABSENT.
        self.assertIn("[tier-class] nc :: cells/nc", output)
        self.assertIn("reads ABSENT+FFI", output)

    def test_d_a_manifest_hidden_in_a_digit_prefixed_string_is_refused(self):
        main = ('#![no_std]\nmacro_rules! eat { ($($t:tt)*) => {} }\neat!(0" '
                + TIER_FFI.replace("\n", " ") + ' ");\n' + FFI_CALL)
        code, signed, output = sign_tree(
            {"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
            ffi_allowlist(crate="nc"), {"nc": None})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        self.assertIn("reads ABSENT", output)

    def test_control_a_plain_tier_one_manifest_is_refused(self):
        main = "#![no_std]\n" + TIER_LESS + FFI_CALL
        code, signed, output = sign_tree(
            {"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
            ffi_allowlist(crate="nc"), {"nc": self.LEGACY})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)

    def test_an_ffi_binary_of_an_ffi_crate_signs(self):
        main = "#![no_std]\n" + TIER_FFI + FFI_CALL
        for name in ("nc", "nc.elf"):
            with self.subTest(name=name):
                code, signed, output = sign_tree(
                    {"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
                    ffi_allowlist(crate="nc"), {name: self.FFI})
                self.assertEqual((code, len(signed)), (cli.EXIT_OK, 1), output)

    def test_a_tier_one_binary_is_held_to_its_exception_class(self):
        files = {"cells/nc/Cargo.toml": NC_CARGO,
                 "cells/nc/src/main.rs": "#![no_std]\n" + TIER_LESS + RAW_PTR}
        allow_text = ffi_allowlist("legacy-raw-ptr", crate="nc") + tier_exception(
            "legacy-raw-ptr", "LEGACY", crate="nc")
        code, signed, output = sign_tree(files, allow_text, {"nc": self.LEGACY})
        self.assertEqual((code, len(signed)), (cli.EXIT_OK, 1), output)
        standard = elf_manifest.manifest_record(1)
        self.refused_by_binary(*sign_tree(files, allow_text, {"nc": standard}), klass="STANDARD")

    def test_a_malformed_manifest_is_refused(self):
        main = "#![no_std]\n#![forbid(unsafe_code)]\n" + TIER_LESS
        code, signed, output = sign_tree(
            {"cells/nc/Cargo.toml": NC_CARGO, "cells/nc/src/main.rs": main},
            "version = 1\n", {"nc": elf_manifest.manifest_record(9)})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        self.assertIn("`__ViCell_manifest` is malformed", output)

    APP = {"cells/app/Cargo.toml": '[package]\nname = "app"\nversion = "0.1.0"\n',
           "cells/app/src/main.rs": "#![no_std]\n#![forbid(unsafe_code)]\n"}

    def gated_tree(self) -> dict[str, str]:
        return {"cells/nc/Cargo.toml": NC_CARGO,
                "cells/nc/src/main.rs": "#![no_std]\n" + TIER_FFI + FFI_CALL, **self.APP}

    def test_an_unattributable_tier_one_binary_is_refused(self):
        code, signed, output = sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                         {"init": self.LEGACY})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        self.assertIn("init: loads in Tier 1 (manifest LEGACY) and its file name is no binary "
                      "target of a cells/ crate", output)

    def test_a_renamed_binary_signs_when_as_names_its_ungated_crate(self):
        code, signed, output = sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                         {"init": self.LEGACY}, aliases={"init": "app"})
        self.assertEqual((code, len(signed)), (cli.EXIT_OK, 1), output)

    def test_as_holds_a_renamed_binary_to_its_gated_crate(self):
        self.refused_by_binary(*sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                          {"init": self.LEGACY}, aliases={"init": "nc"}),
                               klass="LEGACY")

    def test_as_overrides_a_file_name_that_matches_another_crate(self):
        # A gated binary copied over an ungated crate's name must not borrow that crate.
        self.refused_by_binary(*sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                          {"app": self.LEGACY}, aliases={"app": "nc"}),
                               klass="LEGACY")

    def test_as_naming_no_crate_binary_is_refused(self):
        code, signed, output = sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                         {"init": self.FFI}, aliases={"init": "nope"})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        self.assertIn("--as names `nope`, which is no binary target", output)

    def test_as_naming_no_sign_target_is_a_usage_error(self):
        code, signed, output = sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                         {"init": self.LEGACY}, aliases={"other": "app"})
        self.assertEqual((code, signed), (cli.EXIT_USAGE, []), output)
        self.assertIn("names no --sign target", output)

    def test_an_unattributable_tier_two_binary_signs_without_as(self):
        code, signed, output = sign_tree(self.gated_tree(), ffi_allowlist(crate="nc"),
                                         {"foreign": self.FFI})
        self.assertEqual((code, len(signed)), (cli.EXIT_OK, 1), output)

    def test_as_without_sign_is_a_usage_error(self):
        with mock.patch("sys.stderr", __import__("io").StringIO()):
            self.assertEqual(cli.main(["--check", "--as", "x=app"]), cli.EXIT_USAGE)

    def test_a_shared_package_name_is_refused_on_the_sign_path(self):
        files = {"cells/a/Cargo.toml": NC_CARGO,
                 "cells/a/src/main.rs": "#![no_std]\n" + TIER_LESS + FFI_CALL,
                 "cells/b/Cargo.toml": NC_CARGO,
                 "cells/b/src/main.rs": "#![no_std]\n#![forbid(unsafe_code)]\n" + TIER_FFI}
        allow_text = ('version = 1\n[[file]]\npath = "cells/a/src/main.rs"\nclass = "c-ffi"\n'
                      'reason = "r"\napprover = "a"\ndate = "2026-11-16"\n'
                      '[[crate]]\nname = "nc"\nclass = "c-ffi"\nreason = "r"\napprover = "a"\n'
                      'date = "2026-11-16"\n')
        code, signed, output = sign_tree(files, allow_text, {"nc": self.FFI})
        self.assertEqual((code, signed), (cli.EXIT_POLICY, []), output)
        self.assertIn("share this package name", output)


class ElfManifestTests(unittest.TestCase):
    """`elf_manifest.classify` agrees with the kernel's `manifest_section::classify`."""

    def test_classes(self):
        self.assertEqual(elf_manifest.classify(elf_manifest.minimal_elf(None)), "ABSENT")
        for byte, name in ((0, "TRUSTED_CORE"), (1, "STANDARD"), (2, "FFI"), (3, "UNTRUSTED"),
                           (0xFF, "LEGACY")):
            record = elf_manifest.manifest_record(byte)
            self.assertEqual(elf_manifest.classify(elf_manifest.minimal_elf(record)), name)

    def test_a_v1_record_upcasts_to_legacy(self):
        v1 = (0x5649_4345).to_bytes(4, "little") + bytes([1, 0x03, 0, 0])
        self.assertEqual(elf_manifest.classify(elf_manifest.minimal_elf(v1)), "LEGACY")

    def test_malformed_records_and_files(self):
        good = elf_manifest.manifest_record(2)
        bad_records = {
            "unknown class": elf_manifest.manifest_record(4),
            "unknown flag": elf_manifest.manifest_record(2, flags=0x1000),
            "reserved": good[:12] + b"\x01\0\0\0",
            "bad magic": b"\0" * 4 + good[4:],
            "v2 version in 8 bytes": good[:8],
            "trailing byte": good + b"\0",
        }
        for label, record in bad_records.items():
            with self.subTest(label):
                self.assertEqual(elf_manifest.classify(elf_manifest.minimal_elf(record)),
                                 "MALFORMED")
        self.assertEqual(elf_manifest.classify(b"not an elf"), "MALFORMED")

    def test_two_manifest_sections_are_malformed(self):
        import struct
        elf = bytearray(elf_manifest.minimal_elf(elf_manifest.manifest_record(2)))
        shoff = struct.unpack_from("<Q", elf, 40)[0]
        elf += elf[shoff + 2 * 64: shoff + 3 * 64]  # a second copy of the manifest header
        struct.pack_into("<H", elf, 60, 4)
        self.assertEqual(elf_manifest.classify(bytes(elf)), "MALFORMED")

    def test_a_built_cell_if_present(self):
        built = REPO_ROOT / "target/x86_64-unknown-none/release/cellbox"
        if not built.is_file():
            self.skipTest("no built x86_64 cellbox")
        self.assertEqual(elf_manifest.classify(built.read_bytes()), "LEGACY")


class RootDiscoveryTests(unittest.TestCase):
    """Bin roots follow cargo: autobins stay on beside an explicit `[[bin]] path`."""

    def crate(self, cargo_extra: str, files: dict[str, str], recorded=None) -> scan.Crate:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            write(root, "cells/nc/Cargo.toml", NC_CARGO + cargo_extra)
            for rel, text in files.items():
                write(root, f"cells/nc/{rel}", text)
            (crate,) = scan.discover_crates(root, ["cells"], recorded)
            crate.bins = {n: p.relative_to(root / "cells/nc").as_posix()
                          for n, p in crate.bins.items()}
            return crate

    def test_explicit_bin_path_keeps_main_and_autobins(self):
        crate = self.crate('[[bin]]\nname = "x"\npath = "src/x.rs"\n', {
            "src/main.rs": "", "src/x.rs": "", "src/bin/tool.rs": "", "src/bin/evil/main.rs": ""})
        self.assertEqual(crate.bins, {"nc": "src/main.rs", "x": "src/x.rs",
                                      "tool": "src/bin/tool.rs", "evil": "src/bin/evil/main.rs"})

    def test_an_explicit_bin_without_a_path_is_found(self):
        crate = self.crate('[[bin]]\nname = "deep"\n', {"src/bin/deep/main.rs": ""})
        self.assertEqual(crate.bins, {"deep": "src/bin/deep/main.rs"})

    def test_recorded_metadata_bins_are_merged(self):
        crate = self.crate("", {"src/main.rs": "", "odd/entry.rs": ""},
                           recorded={"cells/nc": {"odd": "cells/nc/odd/entry.rs"}})
        self.assertEqual(crate.bins, {"nc": "src/main.rs", "odd": "odd/entry.rs"})

    def test_every_bin_root_must_forbid(self):
        # P18 review: the F1 forbid check skipped `main.rs` beside an explicit bin.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/nc/Cargo.toml", NC_CARGO + '[[bin]]\nname = "x"\npath = "src/x.rs"\n')
            write(root, "cells/nc/src/x.rs", "#![forbid(unsafe_code)]\nfn main() {}\n")
            write(root, "cells/nc/src/main.rs", "fn main() {}\n")
            write(root, "cells/nc/src/bin/evil/main.rs", "fn main() {}\n")
            write(root, "a.toml", "version = 1\n")
            result = policy.check(root, allowlist.load(root, "a.toml"))
            self.assertEqual(sorted(v.path for v in result.violations if v.layer == "attribute"),
                             ["cells/nc/src/bin/evil/main.rs", "cells/nc/src/main.rs"])


class GlobalAsmTests(unittest.TestCase):
    def test_global_asm_counts_as_an_attribute_and_asm_does_not(self):
        # P18 review MED: rustc rejects global_asm! under forbid(unsafe_code).
        code = scan.strip_noncode('core::arch::global_asm!(".globl x\\nx: ret");\n'
                                  "global_asm!(\"nop\");\nfn f() { unsafe { core::arch::asm!(\"nop\") } }\n")
        self.assertEqual(tcb.count(code), tcb.Counts(1, 2))


class BaselineWriterTests(unittest.TestCase):
    """`--write-baseline` lowers ceilings mechanically and never raises one."""

    TEXT = ("# header\nversion = 1\n\n[total]\nunsafe = 10\nattrs = 5\n\n"
            '# ── lib ──\n\n[[site]]\npath = "a.rs"\nunsafe = 3\nattrs = 1\n\n'
            '[[site]]\npath = "b.rs"\nunsafe = 2\nattrs = 0\n\n'
            '[[site]]\npath = "c.rs"\nunsafe = 1\nattrs = 0\n\n'
            '[[site]]\npath = "d.rs"\nunsafe = 3\nattrs = 0\n')

    def test_lowers_drops_and_never_raises(self):
        report = tcb.Report(per_file={"a.rs": tcb.Counts(2, 1), "c.rs": tcb.Counts(4, 0),
                                      "d.rs": tcb.Counts(0, 1)},
                            cells=tcb.Counts(1, 0), closure=tcb.Counts(4, 7))
        out = tcb.lowered_baseline(self.TEXT, report)
        self.assertEqual(out, (
            "# header\nversion = 1\n\n[total]\nunsafe = 5\nattrs = 5\n\n"
            '# ── lib ──\n\n[[site]]\npath = "a.rs"\nunsafe = 2\nattrs = 1\n\n'
            '[[site]]\npath = "c.rs"\nunsafe = 1\nattrs = 0\n\n'))

    def test_cli_write_baseline_makes_a_dropped_tree_pass_strict(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "cells/x/Cargo.toml", '[package]\nname = "x"\nversion = "0.1.0"\n')
            write(root, "cells/x/src/main.rs", "#![forbid(unsafe_code)]\n" + TIER_LESS)
            write(root, "libs/l/Cargo.toml", '[package]\nname = "l"\nversion = "0.1.0"\n')
            write(root, "libs/l/src/lib.rs", "fn f() { unsafe { } }\n")
            write(root, "a.toml", "version = 1\n")
            write_tcb(root, roots=["x"], total=(3, 0), packages={"l": "libs/l"},
                      sites={"libs/l/src/lib.rs": (2, 0), "libs/l/src/gone.rs": (1, 0)})
            load = lambda: policy.check(root, allowlist.load(root, "a.toml"), strict=True,  # noqa: E731
                                        closure=tcb.load_closure(root),
                                        baseline=tcb.load_baseline(root))
            self.assertFalse(load().ok)
            with mock.patch("sys.stdout", new_callable=__import__("io").StringIO):
                self.assertEqual(cli.main(["--repo", str(root), "--allowlist", "a.toml",
                                           "--write-baseline"]), cli.EXIT_OK)
            self.assertTrue(load().ok, [str(v) for v in load().violations])
            baseline = tcb.load_baseline(root)
            self.assertEqual(baseline.total, tcb.Counts(1, 0))
            self.assertEqual(baseline.sites, {"libs/l/src/lib.rs": tcb.Counts(1, 0)})

    def test_values_and_headers_are_read_as_toml(self):
        # P18 re-review LOW: hex/underscore values and commented or spaced headers.
        text = ("version = 1\n[ total ]  # the Tier-1 total\nunsafe = 0x0A  # ten\nattrs = 5\n\n"
                '[[site]] # one row\npath = "a.rs"  # x\nunsafe = 1_0\nattrs = 0o1\n\n'
                '[[ site ]]\npath = "b.rs"\nunsafe = 0x2\nattrs = 0\n')
        report = tcb.Report(per_file={"a.rs": tcb.Counts(4, 1)}, closure=tcb.Counts(4, 1))
        out = tcb.lowered_baseline(text, report)
        self.assertEqual(out, (
            "version = 1\n[ total ]  # the Tier-1 total\nunsafe = 4  # ten\nattrs = 1\n\n"
            '[[site]] # one row\npath = "a.rs"  # x\nunsafe = 4\nattrs = 0o1\n\n'))
        with tempfile.TemporaryDirectory() as tmp:
            write(Path(tmp), tcb.BASELINE_PATH, out)
            baseline = tcb.load_baseline(Path(tmp))
        self.assertEqual((baseline.total, baseline.sites),
                         (tcb.Counts(4, 1), {"a.rs": tcb.Counts(4, 1)}))

    def test_a_row_at_its_ceiling_is_left_byte_for_byte(self):
        text = 'version = 1\n[total]\nunsafe=0x3\nattrs = 0\n[[site]]\npath="a.rs"\nunsafe=0x3\nattrs=0\n'
        report = tcb.Report(per_file={"a.rs": tcb.Counts(3, 0)}, closure=tcb.Counts(3, 0))
        self.assertEqual(tcb.lowered_baseline(text, report), text)

    def test_never_raised_reports_every_loosening(self):
        before = tcb.Baseline(sites={"a.rs": tcb.Counts(2, 0)}, total=tcb.Counts(2, 0))
        after = tcb.Baseline(sites={"a.rs": tcb.Counts(3, 0), "b.rs": tcb.Counts(1, 0)},
                             total=tcb.Counts(2, 1))
        self.assertEqual(len(tcb.never_raised(before, after)), 3)
        self.assertEqual(tcb.never_raised(before, before), [])


class ClosureFileTests(unittest.TestCase):
    BASE = 'version = 1\ntargets = []\ntier1_roots = []\n'

    def load(self, text: str) -> tcb.Closure:
        with tempfile.TemporaryDirectory() as tmp:
            write(Path(tmp), tcb.CLOSURE_PATH, self.BASE + text)
            return tcb.load_closure(Path(tmp))

    def test_pins_are_keyed_by_the_full_source(self):
        pin = '[[thirdparty]]\nname = "f"\nversion = "1"\nsource = "{}"\nunsafe = 0\nattrs = 0\n'
        git_a = "git+https://x/f?rev=a#aaaa"
        both = self.load(pin.format(git_a) + pin.format("registry+https://r"))
        self.assertEqual(len(both.thirdparty), 2)
        with self.assertRaises(tcb.TcbError):
            self.load(pin.format(git_a) + pin.format(git_a))

    def test_proc_macro_and_bin_tables(self):
        closure = self.load('[[proc_macro]]\nname = "m"\nversion = "1"\nsource = "registry+r"\n'
                            '[[bin]]\nname = "b"\npackage = "p"\ndir = "cells/p"\nsrc = "cells/p/src/main.rs"\n')
        self.assertEqual(closure.bins, {"cells/p": {"b": "cells/p/src/main.rs"}})
        self.assertEqual(len(closure.proc_macros), 1)
        for bad in ('[[proc_macro]]\nname = "m"\nversion = "1"\n',
                    '[[bin]]\nname = "b"\npackage = "p"\ndir = "cells/p"\n'):
            with self.subTest(bad=bad), self.assertRaises(tcb.TcbError):
                self.load(bad)

    def test_the_real_closure_pins_proc_macros_and_softfloat(self):
        closure = tcb.load_closure(REPO_ROOT)
        names = {m["name"] for m in closure.proc_macros}
        self.assertLessEqual({"zerocopy-derive", "defmt-macros", "serde_derive", "enumn"}, names)
        self.assertEqual(closure.packages.get("viui-macros"), "libs/viui-macros")
        targets = tomllib_load(REPO_ROOT / tcb.CLOSURE_PATH)["targets"]
        self.assertIn("aarch64-unknown-none-softfloat", targets)
        fatfs = [t for t in closure.thirdparty if t["name"] == "fatfs"]
        self.assertTrue(fatfs and all("rev=" in t["source"] and "#" in t["source"] for t in fatfs))


def tomllib_load(path: Path) -> dict:
    import tomllib
    return tomllib.loads(path.read_text(encoding="utf-8"))


class GeneratorTests(unittest.TestCase):
    """`gen-tier1-closure.py` refusals, driven with a fake `cargo metadata`."""

    def generator(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location(
            "gen_tier1_closure", REPO_ROOT / "scripts/gen-tier1-closure.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def build(self, files: dict[str, str], packages: list[dict], edges: dict[str, list[str]],
              build_edges: dict[str, list[str]] | None = None, reviewed: dict | None = None):
        import copy
        gen = self.generator()
        packages = copy.deepcopy(packages)
        build_edges = build_edges or {}
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            for rel, text in files.items():
                write(root, rel, text)
            for pkg in packages:
                pkg.setdefault("version", "0.1.0")
                pkg.setdefault("source", None)
                pkg["manifest_path"] = str(root / pkg.pop("dir") / "Cargo.toml")
                for target in pkg.setdefault("targets", []):
                    target["src_path"] = str(root / target["src_path"])
            meta = {"packages": packages,
                    "workspace_members": [p["id"] for p in packages if p["source"] is None],
                    "resolve": {"nodes": [{"id": p["id"], "deps": [
                        {"pkg": d, "dep_kinds": [{"kind": None}]} for d in edges.get(p["id"], [])
                    ] + [{"pkg": d, "dep_kinds": [{"kind": "build"}]}
                         for d in build_edges.get(p["id"], [])]} for p in packages]}}
            with mock.patch.object(gen, "REPO", root), \
                 mock.patch.object(gen, "metadata", return_value=meta), \
                 mock.patch.object(gen, "NON_MEMBER_CELLS", {}), \
                 mock.patch.object(gen, "REVIEWED_GENERATED_INCLUDES", reviewed or {}):
                return gen.build()

    CELL_A = {"cells/a/Cargo.toml": '[package]\nname = "a"\nversion = "0.1.0"\n',
              "cells/a/src/main.rs": "#![forbid(unsafe_code)]\n" + TIER_LESS}
    PKG_A = {"id": "a", "name": "a", "dir": "cells/a",
             "targets": [{"kind": ["bin"], "name": "a", "src_path": "cells/a/src/main.rs"}]}

    def test_a_tier_two_cells_dependency_of_a_tier_one_root_fails(self):
        files = dict(self.CELL_A, **{
            "cells/b/Cargo.toml": '[package]\nname = "b"\nversion = "0.1.0"\n',
            "cells/b/src/lib.rs": TIER_FFI + "pub fn f() { unsafe { } }\n"})
        packages = [dict(self.PKG_A), {"id": "b", "name": "b", "dir": "cells/b", "targets": [
            {"kind": ["lib"], "name": "b", "src_path": "cells/b/src/lib.rs"}]}]
        with self.assertRaises(SystemExit) as raised:
            self.build(files, packages, {"a": ["b"]})
        self.assertIn("cells/b (b) reads as Tier 2", str(raised.exception))

    BUILD_RS = 'fn main() { write("pub unsafe fn g() {}"); }\n'

    def lib_l(self, lib_rs: str, build_rs: str = BUILD_RS) -> tuple[dict, list]:
        files = dict(self.CELL_A, **{
            "libs/l/Cargo.toml": '[package]\nname = "l"\nversion = "0.1.0"\n',
            "libs/l/src/lib.rs": lib_rs, "libs/l/build.rs": build_rs})
        packages = [dict(self.PKG_A), {"id": "l", "name": "l", "dir": "libs/l", "targets": [
            {"kind": ["lib"], "name": "l", "src_path": "libs/l/src/lib.rs"},
            {"kind": ["custom-build"], "name": "build-script-build", "src_path": "libs/l/build.rs"}]}]
        return files, packages

    def test_a_computed_include_with_unsafe_in_the_build_script_fails(self):
        # P18 re-review LOW: any non-literal include! argument, not only OUT_DIR.
        for include in ('include!(concat!(env!("OUT_DIR"), "/gen.rs"));',
                        'include!(env!("GEN_RS"));',
                        'macro_rules! inc { ($p:expr) => { include!($p); } }\ninc!("x.rs");'):
            with self.subTest(include=include):
                files, packages = self.lib_l(include + "\n")
                with self.assertRaises(SystemExit) as raised:
                    self.build(files, packages, {"a": ["l"]})
                self.assertIn("l@0.1.0 has a build script and include!s a computed path",
                              str(raised.exception))

    def test_a_literal_include_or_no_build_script_is_not_flagged(self):
        files, packages = self.lib_l('include!("gen.rs");\n')
        files["libs/l/src/gen.rs"] = ""
        self.assertIn('[[package]]\nname = "l"', self.build(files, packages, {"a": ["l"]}))
        files, packages = self.lib_l('include!(concat!(env!("CARGO_MANIFEST_DIR"), "/x.rs"));\n')
        packages[1]["targets"].pop()  # no build script: nothing can be generated
        self.assertIn('[[package]]\nname = "l"', self.build(files, packages, {"a": ["l"]}))

    def test_a_third_party_build_dependency_counts_as_a_build_input(self):
        files, packages = self.lib_l('include!(concat!(env!("OUT_DIR"), "/gen.rs"));\n',
                                     build_rs="fn main() { gen::emit(); }\n")
        files["reg/gen/src/lib.rs"] = 'pub fn emit() { out("#[no_mangle] fn x() {}"); }\n'
        packages.append({"id": "gen", "name": "gen", "dir": "reg/gen",
                         "source": "registry+https://github.com/rust-lang/crates.io-index"})
        with self.assertRaises(SystemExit) as raised:
            self.build(files, packages, {"a": ["l"]}, build_edges={"l": ["gen"]})
        self.assertIn("reg/gen/src/lib.rs: no_mangle", str(raised.exception))

    def test_a_reviewed_generated_include_is_pinned_to_its_build_script(self):
        import hashlib
        files, packages = self.lib_l('include!(concat!(env!("OUT_DIR"), "/gen.rs"));\n')
        digest = hashlib.sha256(self.BUILD_RS.encode()).hexdigest()
        text = self.build(files, packages, {"a": ["l"]}, reviewed={"l@0.1.0": (digest, "ok")})
        self.assertIn('[[package]]\nname = "l"', text)
        with self.assertRaises(SystemExit) as raised:
            self.build(files, packages, {"a": ["l"]}, reviewed={"l@0.1.0": ("0" * 64, "old")})
        self.assertIn("l@0.1.0: its build script changed since the review", str(raised.exception))
        files, packages = self.lib_l('include!("gen.rs");\n')
        files["libs/l/src/gen.rs"] = ""
        with self.assertRaises(SystemExit) as raised:
            self.build(files, packages, {"a": ["l"]}, reviewed={"l@0.1.0": (digest, "ok")})
        self.assertIn("REVIEWED_GENERATED_INCLUDES lists l@0.1.0, which no longer needs it",
                      str(raised.exception))

    def test_the_real_reviewed_entries_match_their_build_scripts(self):
        import hashlib
        gen = self.generator()
        silo = REPO_ROOT / "cells/services/silo/build.rs"
        self.assertEqual(gen.REVIEWED_GENERATED_INCLUDES["service-silo@0.1.0"][0],
                         hashlib.sha256(silo.read_bytes()).hexdigest())

    def test_clean_closure_records_bins_pins_and_proc_macros(self):
        files = dict(self.CELL_A, **{"reg/dep/src/lib.rs": "pub fn d() { unsafe { } }\n",
                                     "reg/mac/src/lib.rs": ""})
        registry = "registry+https://github.com/rust-lang/crates.io-index"
        packages = [dict(self.PKG_A),
                    {"id": "dep", "name": "dep", "dir": "reg/dep", "source": registry},
                    {"id": "mac", "name": "mac", "dir": "reg/mac", "source": registry,
                     "targets": [{"kind": ["proc-macro"], "name": "mac",
                                  "src_path": "reg/mac/src/lib.rs"}]}]
        text = self.build(files, packages, {"a": ["dep", "mac"]})
        self.assertIn('[[thirdparty]]\nname = "dep"\nversion = "0.1.0"\n'
                      f'source = "{registry}"\nunsafe = 1\nattrs = 0', text)
        self.assertIn(f'[[proc_macro]]\nname = "mac"\nversion = "0.1.0"\nsource = "{registry}"', text)
        self.assertIn('[[bin]]\nname = "a"\npackage = "a"\ndir = "cells/a"\n'
                      'src = "cells/a/src/main.rs"', text)


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
            target = root / "cell"
            # FFI (Tier 2): the mocked tree has no crate, so a Tier-1 target is unattributable.
            target.write_bytes(elf_manifest.minimal_elf(elf_manifest.manifest_record(2)))
            args = argparse.Namespace(
                repo=root, allowlist="a.toml", quiet=True, strict=False,
                targets=[str(target)], objcopy="objcopy", seed_hex=None,
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
        # The ELF tier gate reads the manifest before any route rule; FFI (Tier 2) passes
        # it without a crate, which the mocked check below does not provide.
        for target in targets:
            target.write_bytes(elf_manifest.minimal_elf(elf_manifest.manifest_record(2)))
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
