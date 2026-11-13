#!/usr/bin/env python3
"""Unit tests for scripts/check-posix-ledger.py.

Run: `python3 -m unittest scripts/test_check_posix_ledger.py`

Each test builds a throwaway fixture repository (Tier A Rust source, Tier B
sysdep source, a witness test, an evidence artifact and a ledger) and runs the
checker against it with `--repo`. Assertions check the exit status and the
`path:line` of each diagnostic, not prose.
"""
from __future__ import annotations

import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("check_posix_ledger", REPO / "scripts/check-posix-ledger.py")
checker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checker)

POSIX = "libs/posix-shim/src"
SYSDEPS = "third_party/mlibc/sysdeps/vicell/generic/generic.cpp"

SOURCES = {
    f"{POSIX}/io.rs": """\
#[no_mangle]
pub unsafe extern "C" fn open(name: *const u8) -> i32 {
    -1
}

#[cfg(target_arch = "riscv64")]
#[unsafe(naked)]
#[no_mangle]
pub unsafe extern "C" fn setjmp(env: *mut u64) -> i32 {
    0
}
#[cfg(not(target_arch = "riscv64"))]
#[no_mangle]
pub unsafe extern "C" fn setjmp(env: *mut u64) -> i32 {
    0
}

#[no_mangle]
pub static mut stdout: *mut u8 = core::ptr::null_mut();
""",
    f"{POSIX}/math.rs": """\
macro_rules! math1 {
    ($c:ident, $rust:path) => {
        #[no_mangle]
        pub extern "C" fn $c(x: f64) -> f64 {
            $rust(x)
        }
    };
}

math1!(sin, libm::sin);
math1!(cos, libm::cos);
""",
    SYSDEPS: """\
namespace mlibc {
int Sysdeps<Open>::operator()(const char *pathname, int flags, mode_t mode, int *fd) {
    long ret = vicell_syscall(VI_SYS_OPEN, 0, 0, 0, 0);
    return 0;
}
void Sysdeps<Exit>::operator()(int status) {
    sysdep<Open>(nullptr, 0, 0, nullptr);
}
}
""",
    "tests/witness.rs": """\
#[test]
fn open_refuses() {}

fn helper() {}
""",
    "docs/evidence/run.txt": "RESULT=1 passed\nGUEST=marker OK\n",
}

A_HEAD = (
    "| profile | kind | entry_point | backing | source | semantic_subset | unsupported_result | status | evidence |\n"
    "|---|---|---|---|---|---|---|---|---|\n"
)
B_HEAD = (
    "| profile | kind | sysdep | consumer_visible_symbol | backing | source | semantic_subset "
    "| unsupported_result | status | evidence |\n"
    "|---|---|---|---|---|---|---|---|---|---|\n"
)
ROW_OPEN = (
    f"| tier-a | libc-symbol | `open` | none | `{POSIX}/io.rs:1-4` | none | `-1` | refused "
    "| `tests/witness.rs::open_refuses` |\n"
)
ROW_SETJMP = f"| tier-a | libc-symbol | `setjmp` | asm | `{POSIX}/io.rs:9`; `{POSIX}/io.rs:14` | x | x | unverified | none |\n"
ROW_STDOUT = f"| tier-a | c-global | `stdout` | static | `{POSIX}/io.rs:18-19` | fd 1 | n/a | unverified | none |\n"
ROW_MATH = (
    f"| tier-a | libc-symbol-family | `sin, cos` | libm | `{POSIX}/math.rs:10-11` | libm | n/a | partial "
    "| `docs/evidence/run.txt:1-2` |\n"
)
ROW_SYS_OPEN = (
    f"| tier-b | mlibc-sysdep | `Open` | `open` [INFERENCE] | VI_SYS_OPEN | `{SYSDEPS}:2-5` | x | ENOENT "
    "| unverified | none |\n"
)
ROW_SYS_EXIT = f"| tier-b | mlibc-sysdep | `Exit` | `exit` | VI_SYS_EXIT | `{SYSDEPS}:6` | x | n/a | unverified | none |\n"


def ledger(*, a_rows=None, b_rows=None) -> str:
    a_rows = a_rows if a_rows is not None else [ROW_OPEN, ROW_SETJMP, ROW_STDOUT, ROW_MATH]
    b_rows = b_rows if b_rows is not None else [ROW_SYS_OPEN, ROW_SYS_EXIT]
    return (
        "# POSIX ledger\n\n"
        "| unrelated | table |\n|---|---|\n| ignored | row |\n\n"
        "## Tier A\n\n" + A_HEAD + "".join(a_rows) + "\n"
        "## Tier B\n\n" + B_HEAD + "".join(b_rows)
    )


LEDGER_A_FIRST_ROW = 11  # line of the first Tier A data row in ledger()


class PosixLedgerCheckerTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.repo = Path(tmp.name)
        for rel, text in SOURCES.items():
            self.write(rel, text)
        self.write("docs/posix-ledger.md", ledger())

    def write(self, rel: str, text: str) -> None:
        path = self.repo / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def run_check(self) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = checker.main(["--repo", str(self.repo)])
        return code, out.getvalue()

    def assert_rejected(self, location: str) -> str:
        code, out = self.run_check()
        self.assertEqual(code, 1, out)
        self.assertIn(f"  {location}: ", out)
        return out

    def test_accepts_a_minimal_valid_repo(self):
        code, out = self.run_check()
        self.assertEqual(code, 0, out)
        self.assertIn("Tier A 5 exports (1 globals), Tier B 2 sysdeps, 6 rows", out)

    def test_discovers_macro_exports_and_counts_cfg_variants_once(self):
        found = checker.Checker(self.repo).discover_tier_a()
        self.assertEqual(sorted(found), ["cos", "open", "setjmp", "sin", "stdout"])
        self.assertEqual(found["stdout"].kind, "static")

    def test_rejects_an_unlisted_function_export(self):
        self.write(f"{POSIX}/extra.rs", '#[no_mangle]\npub extern "C" fn dlopen() {}\n')
        out = self.assert_rejected(f"{POSIX}/extra.rs:2")
        self.assertIn("'dlopen'", out)

    def test_rejects_an_unlisted_macro_export(self):
        self.write(f"{POSIX}/math.rs", SOURCES[f"{POSIX}/math.rs"] + "math1!(tan, libm::tan);\n")
        self.assert_rejected(f"{POSIX}/math.rs:12")

    def test_rejects_an_unlisted_sysdep(self):
        self.write(SYSDEPS, SOURCES[SYSDEPS].replace("}\n}\n", "}\nint Sysdeps<Isatty>::operator()(int fd) {\n    return 0;\n}\n}\n"))
        self.assert_rejected(f"{SYSDEPS}:9")

    def test_rejects_an_unlisted_sysdep_in_another_source_file(self):
        thread = SYSDEPS.replace("generic.cpp", "thread.cpp")
        self.write(thread, "namespace mlibc {\nvoid Sysdeps<Yield>::operator()() {\n}\n}\n")
        out = self.assert_rejected(f"{thread}:2")
        self.assertIn("'Yield'", out)

    def test_rejects_a_sysdep_defined_in_two_source_files(self):
        thread = SYSDEPS.replace("generic.cpp", "thread.cpp")
        self.write(thread, "namespace mlibc {\nvoid Sysdeps<Exit>::operator()(int status) {\n}\n}\n")
        out = self.assert_rejected(f"{thread}:2")
        self.assertIn("'Exit' defined twice", out)

    def test_rejects_a_ledger_symbol_that_is_not_exported(self):
        row = f"| tier-a | libc-symbol | `fork` | none | `{POSIX}/io.rs:1` | x | x | unverified | none |\n"
        self.write("docs/posix-ledger.md", ledger(a_rows=[ROW_OPEN, ROW_SETJMP, ROW_STDOUT, ROW_MATH, row]))
        out = self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW + 4}")
        self.assertIn("'fork'", out)

    def test_rejects_a_row_with_the_wrong_column_count(self):
        malformed = ROW_OPEN.replace("| none | `-1` |", "| `-1` |")
        self.write("docs/posix-ledger.md", ledger(a_rows=[malformed, ROW_SETJMP, ROW_STDOUT, ROW_MATH]))
        self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW}")

    def test_rejects_a_status_outside_the_vocabulary(self):
        bad = ROW_SETJMP.replace("| unverified |", "| complete |")
        self.write("docs/posix-ledger.md", ledger(a_rows=[ROW_OPEN, bad, ROW_STDOUT, ROW_MATH]))
        self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW + 1}")

    def test_rejects_a_kind_outside_the_vocabulary_or_disagreeing_with_source(self):
        for kind in ("posix-call", "libc-symbol"):
            with self.subTest(kind=kind):
                bad = ROW_STDOUT.replace("| c-global |", f"| {kind} |")
                self.write("docs/posix-ledger.md", ledger(a_rows=[ROW_OPEN, ROW_SETJMP, bad, ROW_MATH]))
                self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW + 2}")

    def test_rejects_a_behavioural_status_without_evidence(self):
        for evidence in ("none", f"`{POSIX}/io.rs:1-4`", "`tests/witness.rs::helper`"):
            with self.subTest(evidence=evidence):
                bad = ROW_OPEN.replace("`tests/witness.rs::open_refuses`", evidence)
                self.write("docs/posix-ledger.md", ledger(a_rows=[bad, ROW_SETJMP, ROW_STDOUT, ROW_MATH]))
                self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW}")

    def test_rejects_an_anchor_whose_path_does_not_exist(self):
        cases = (
            ("`tests/witness.rs::open_refuses`", "`tests/missing.rs::open_refuses`"),
            ("`tests/witness.rs::open_refuses`", "`docs/evidence/missing.txt:1`"),
            (f"`{POSIX}/io.rs:1-4`", f"`{POSIX}/gone.rs:1-4`"),
        )
        for old, new in cases:
            with self.subTest(anchor=new):
                self.write("docs/posix-ledger.md", ledger(a_rows=[ROW_OPEN.replace(old, new), ROW_SETJMP, ROW_STDOUT, ROW_MATH]))
                self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW}")

    def test_rejects_a_source_anchor_that_does_not_cite_the_symbol(self):
        bad = ROW_OPEN.replace(f"`{POSIX}/io.rs:1-4`", f"`{POSIX}/io.rs:18-19`")
        self.write("docs/posix-ledger.md", ledger(a_rows=[bad, ROW_SETJMP, ROW_STDOUT, ROW_MATH]))
        self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW}")

    def test_rejects_a_symbol_listed_twice(self):
        self.write("docs/posix-ledger.md", ledger(a_rows=[ROW_OPEN, ROW_SETJMP, ROW_STDOUT, ROW_MATH, ROW_SETJMP]))
        self.assert_rejected(f"docs/posix-ledger.md:{LEDGER_A_FIRST_ROW + 4}")


if __name__ == "__main__":
    unittest.main(verbosity=2)
