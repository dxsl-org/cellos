//! Rust half of the reproducer, built for `x86_64-unknown-none` exactly like the
//! cell's Rust: soft-float, so an `f64` travels in general-purpose registers.

#![no_std]

use core::ffi::c_void;

include!(concat!(env!("OUT_DIR"), "/tree.rs"));

/// `ffi.rs`'s view of `JSValue`: two integer words.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JSValue {
    pub u: u64,
    pub tag: i64,
}

// Pre-fix tree (6fee21927): `cells/services/ocel-quickjs/src/ffi.rs:81-82` and
// `:107-119`, verbatim. Upstream C takes the address of `lrint`; the cell's Rust
// supplied it with an `f64` argument.
#[cfg(not(t2_fixed))]
extern "C" {
    fn vios_js_is_float(v: JSValue) -> core::ffi::c_int;
    fn vios_js_as_float(v: JSValue) -> f64;
}

#[cfg(not(t2_fixed))]
#[no_mangle]
pub extern "C" fn lrint(x: f64) -> i64 {
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

/// How the cell's Rust reads a float JSValue (`engine.rs:277-278`: the tag
/// check, then the read), returned as bits so the driver sees what Rust saw.
#[cfg(not(t2_fixed))]
#[no_mangle]
pub extern "C" fn t2_rust_reads_float(_ctx: *mut c_void, v: JSValue) -> u64 {
    if unsafe { vios_js_is_float(v) } == 0 {
        return u64::MAX;
    }
    unsafe { vios_js_as_float(v) }.to_bits()
}

#[cfg(t2_fixed)]
extern "C" {
    fn JS_ToFloat64(ctx: *mut c_void, out: *mut f64, v: JSValue) -> core::ffi::c_int;
}

#[cfg(t2_fixed)]
#[no_mangle]
pub extern "C" fn t2_rust_reads_float(ctx: *mut c_void, v: JSValue) -> u64 {
    let mut d = 0.0f64;
    if unsafe { JS_ToFloat64(ctx, &mut d, v) } != 0 {
        return u64::MAX;
    }
    d.to_bits()
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
