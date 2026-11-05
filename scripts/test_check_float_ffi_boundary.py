#!/usr/bin/env python3
"""Unit tests for scripts/check-float-ffi-boundary.py.

Run: `python3 -m unittest scripts/test_check_float_ffi_boundary.py`

Each test writes a throwaway Rust file or C header and asserts which symbols the
checker reports. The fixtures are the phase R1 list: `double` argument, `float`
return, `struct { double }` by value, `__m128`, and a callback taking `float`,
on both sides; plus clean int/pointer boundaries that must pass.
"""
from __future__ import annotations

import contextlib
import importlib.util
import io
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location("check_float_ffi", REPO / "scripts/check-float-ffi-boundary.py")
checker = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(checker)


def run(rust: str = "", header: str = "", allow: tuple[str, ...] = ()) -> tuple[int, set[str]]:
    with tempfile.TemporaryDirectory() as tmp:
        argv: list[str] = []
        if rust:
            (Path(tmp) / "cell.rs").write_text(rust)
            argv += ["--rust", f"{tmp}/cell.rs"]
        if header:
            (Path(tmp) / "lib.h").write_text(header)
            argv += ["--c-header", f"{tmp}/lib.h"]
        for name in allow:
            argv += ["--allow", name]
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            status = checker.main(argv)
    return status, {line.split(": ")[1] for line in out.getvalue().splitlines()}


class CHeaders(unittest.TestCase):
    def test_r1_fixtures_are_rejected(self):
        status, syms = run(header="""
            #include <stddef.h>
            struct vec { int n; double x; };
            typedef struct { float w; } weight_t;
            typedef void (*on_progress)(float done);
            double scale(double x);              /* double argument */
            float  ratio(int a, int b);          /* float return */
            void   take_vec(struct vec v);       /* struct { double } */
            void   take_weight(weight_t w);      /* typedef'd struct */
            void   simd(__m128 lanes);           /* vector type */
            void   watch(void (*cb)(float done)); /* callback taking float */
            void   watch2(on_progress cb);       /* callback via typedef */
        """)
        self.assertEqual(status, 1)
        self.assertEqual(syms, {"scale", "ratio", "take_vec", "take_weight", "simd", "watch", "watch2"})

    def test_int_and_pointer_boundary_passes(self):
        status, syms = run(header="""
            struct vec { int n; double x; };
            int    sum(const int *xs, size_t n);
            void   fill(double *out, size_t n);  /* pointer to double is fine */
            void   take_vec_ptr(const struct vec *v);
            void   each(int (*cb)(void *ctx, double *v), void *ctx);
            static inline double helper(double x) { return x; }
        """)
        self.assertEqual((status, syms), (0, set()))

    def test_nested_struct_containing_float_struct(self):
        status, syms = run(header="""
            struct inner { double d; };
            struct outer { int tag; struct inner in; };
            void f(struct outer o);
        """)
        self.assertEqual(syms, {"f"})


class RustSide(unittest.TestCase):
    def test_r1_fixtures_are_rejected(self):
        status, syms = run(rust="""
            #[repr(C)] pub struct Vec2 { pub n: i32, pub x: f64 }
            #[repr(C)] pub struct Wrap(pub f32);
            extern "C" {
                fn scale(x: f64) -> i32;
                fn ratio(a: i32) -> f32;
                fn take_vec(v: Vec2);
                fn take_wrap(w: Wrap);
                fn simd(v: core::arch::x86_64::__m128);
                fn watch(cb: Option<unsafe extern "C" fn(f32) -> i32>);
            }
            #[no_mangle]
            pub extern "C" fn rust_export(v: f64) -> i32 { 0 }
        """)
        self.assertEqual(status, 1)
        self.assertEqual(syms, {"scale", "ratio", "take_vec", "take_wrap", "simd", "watch", "rust_export"})

    def test_clean_rust_boundary_and_private_float_code_pass(self):
        status, syms = run(rust="""
            extern "C" {
                fn sum(xs: *const i32, n: usize) -> i32;
                fn fill(out: *mut f64, n: usize);
                fn each(cb: extern "C" fn(*mut core::ffi::c_void, *mut f64) -> i32);
            }
            fn internal_math(x: f64) -> f64 { x * 2.0 } // not a boundary
            #[no_mangle]
            pub extern "C" fn cell_main(argc: i32) -> i32 { argc }
        """)
        self.assertEqual((status, syms), (0, set()))

    def test_allow_list_suppresses_only_named_symbols(self):
        status, syms = run(rust='extern "C" { fn a(x: f64); fn b(x: f32); }', allow=("a",))
        self.assertEqual((status, syms), (1, {"b"}))

    def test_macro_template_export_is_reported_under_its_metavariable(self):
        # libs/api/src/services/posix/math.rs generates sin/cos/... this way.
        status, syms = run(rust="""
            macro_rules! math1_f64 {
                ($c:ident, $rust:path) => {
                    #[no_mangle]
                    pub extern "C" fn $c(x: f64) -> f64 { $rust(x) }
                };
            }
        """)
        self.assertEqual((status, syms), (1, {"$c"}))


if __name__ == "__main__":
    unittest.main()
