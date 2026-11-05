//! The engine's C99 math, behind an integer-only boundary.
//!
//! Rule H1 (`docs/guides/tier1b-c-zig.md`, "x86_64 C and the float boundary"):
//! no `float`/`double` crosses between this cell's Rust and its C by value. On
//! x86_64 the engine's C is hard-float SSE2 and every Rust Cell target is
//! soft-float, so the two sides pass an `f64` in different registers — the
//! engine would read its own argument back as the result. The C side
//! (`vendor/include/math.h`) therefore hands each `double` over as its IEEE-754
//! bit pattern and gets the result back the same way; the bit pattern is exact,
//! so every architecture computes the same `libm` result.
//!
//! Contract with `math.h`: one export `cellos_qjs_<name>_bits` per function it
//! declares, taking and returning `uint64_t` bit patterns, except `lrint`
//! (`int64_t` result) and `modf` (fractional part only; `math.h` takes the
//! integral part from `trunc`).

macro_rules! bits1 {
    ($($c:ident => $f:path),* $(,)?) => {$(
        #[no_mangle]
        pub extern "C" fn $c(x: u64) -> u64 {
            $f(f64::from_bits(x)).to_bits()
        }
    )*};
}

macro_rules! bits2 {
    ($($c:ident => $f:path),* $(,)?) => {$(
        #[no_mangle]
        pub extern "C" fn $c(x: u64, y: u64) -> u64 {
            $f(f64::from_bits(x), f64::from_bits(y)).to_bits()
        }
    )*};
}

bits1! {
    cellos_qjs_fabs_bits => libm::fabs,
    cellos_qjs_floor_bits => libm::floor,
    cellos_qjs_ceil_bits => libm::ceil,
    cellos_qjs_round_bits => libm::round,
    cellos_qjs_trunc_bits => libm::trunc,
    cellos_qjs_sqrt_bits => libm::sqrt,
    cellos_qjs_cbrt_bits => libm::cbrt,
    cellos_qjs_acos_bits => libm::acos,
    cellos_qjs_acosh_bits => libm::acosh,
    cellos_qjs_asin_bits => libm::asin,
    cellos_qjs_asinh_bits => libm::asinh,
    cellos_qjs_atan_bits => libm::atan,
    cellos_qjs_atanh_bits => libm::atanh,
    cellos_qjs_cos_bits => libm::cos,
    cellos_qjs_cosh_bits => libm::cosh,
    cellos_qjs_exp_bits => libm::exp,
    cellos_qjs_expm1_bits => libm::expm1,
    cellos_qjs_log_bits => libm::log,
    cellos_qjs_log10_bits => libm::log10,
    cellos_qjs_log1p_bits => libm::log1p,
    cellos_qjs_log2_bits => libm::log2,
    cellos_qjs_sin_bits => libm::sin,
    cellos_qjs_sinh_bits => libm::sinh,
    cellos_qjs_tan_bits => libm::tan,
    cellos_qjs_tanh_bits => libm::tanh,
}

bits2! {
    cellos_qjs_fmod_bits => libm::fmod,
    cellos_qjs_hypot_bits => libm::hypot,
    cellos_qjs_fmin_bits => libm::fmin,
    cellos_qjs_fmax_bits => libm::fmax,
    cellos_qjs_copysign_bits => libm::copysign,
    cellos_qjs_pow_bits => libm::pow,
    cellos_qjs_atan2_bits => libm::atan2,
}

/// `modf`'s fractional part.
#[no_mangle]
pub extern "C" fn cellos_qjs_modf_bits(x: u64) -> u64 {
    libm::modf(f64::from_bits(x)).0.to_bits()
}

/// `lrint`: round to nearest, ties to even.
///
/// The engine calls it on finite values in the integer range (the
/// `Uint8ClampedArray` store clamps to 0..=255 first), so saturating is the
/// right contract for the rest: NaN and infinities have no `long` value and
/// give 0, and out-of-range values saturate like the hardware conversion.
#[no_mangle]
pub extern "C" fn cellos_qjs_lrint_bits(x: u64) -> i64 {
    let x = f64::from_bits(x);
    if !x.is_finite() {
        return 0;
    }
    let rounded = libm::rint(x);
    if rounded <= i64::MIN as f64 {
        i64::MIN
    } else if rounded >= i64::MAX as f64 {
        i64::MAX
    } else {
        rounded as i64
    }
}
