//! C ABI of the vendored engine, plus the shim symbols it needs that the
//! Tier-A C ABI (`api::services::posix`) does not export.
//!
//! Everything else the engine links against — `malloc`/`free`/`realloc`,
//! `mem*`/`str*`, `snprintf`/`vsnprintf`/`fprintf`/`fputc`/`fwrite`,
//! `stdout`/`stderr`, `abort`, and the C99 math set — comes from `libs/api`,
//! which every cell already links. This file therefore carries only the
//! declarations of upstream `quickjs.h` that the engine driver uses, and the
//! three bodies upstream expects from an OS: the clock and the epoch
//! conversion behind `Date`.

#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

/// Opaque `JSRuntime`.
#[repr(C)]
pub struct JSRuntime {
    _private: [u8; 0],
}

/// Opaque `JSContext`.
#[repr(C)]
pub struct JSContext {
    _private: [u8; 0],
}

/// `JSValue` is two words; the engine treats it as a tagged union.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JSValue {
    /// `JSValueUnion` — either the small int or the `f64`, read through the
    /// glue's `vios_js_as_*` once the glue has told us which it is.
    pub u: u64,
    /// Tag from the header's `JS_TAG_*` table. Only the glue interprets it.
    pub tag: i64,
}

/// `JS_EVAL_TYPE_GLOBAL`: evaluate as a global script.
pub const JS_EVAL_TYPE_GLOBAL: c_int = 0;

extern "C" {
    /// Upstream's default runtime: the engine allocates through the Tier-A
    /// `malloc` family (`js_def_malloc` inside the archive), so the cell heap
    /// backs it without the driver passing its own function table.
    pub fn JS_NewRuntime() -> *mut JSRuntime;
    pub fn JS_FreeRuntime(rt: *mut JSRuntime);
    pub fn JS_NewContext(rt: *mut JSRuntime) -> *mut JSContext;
    pub fn JS_FreeContext(ctx: *mut JSContext);
    pub fn JS_SetMaxStackSize(rt: *mut JSRuntime, stack_size: usize);
    pub fn JS_SetMemoryLimit(rt: *mut JSRuntime, limit: usize);
    pub fn JS_SetGCThreshold(rt: *mut JSRuntime, gc_threshold: usize);

    pub fn JS_Eval(
        ctx: *mut JSContext,
        input: *const c_char,
        input_len: usize,
        filename: *const c_char,
        eval_flags: c_int,
    ) -> JSValue;

    pub fn JS_ToBool(ctx: *mut JSContext, v: JSValue) -> c_int;
    pub fn JS_ToFloat64(ctx: *mut JSContext, out: *mut f64, v: JSValue) -> c_int;
    pub fn JS_ToCStringLen2(
        ctx: *mut JSContext,
        plen: *mut usize,
        v: JSValue,
        cesu8: c_int,
    ) -> *const c_char;
    pub fn JS_FreeCString(ctx: *mut JSContext, ptr: *const c_char);
    pub fn JS_GetException(ctx: *mut JSContext) -> JSValue;

    // Glue (src/c/qjs_cellos_glue.c): the head's `static inline` predicates and
    // free, exported so this side never mirrors the conditional JSValue layout.
    pub fn vios_js_is_exception(v: JSValue) -> c_int;
    pub fn vios_js_is_undefined(v: JSValue) -> c_int;
    pub fn vios_js_is_null(v: JSValue) -> c_int;
    pub fn vios_js_is_bool(v: JSValue) -> c_int;
    pub fn vios_js_is_number(v: JSValue) -> c_int;
    pub fn vios_js_is_float(v: JSValue) -> c_int;
    pub fn vios_js_as_float(v: JSValue) -> f64;
    pub fn vios_js_free(ctx: *mut JSContext, v: JSValue);
}

/// `lrint` — round to nearest, ties to even — which the Tier-A math set does
/// not export and the engine's `Math` table takes the address of.
///
/// The engine only ever calls it on finite values in the integer range
/// (`js_math_round`-adjacent paths), so the saturating cast below is the right
/// contract: out-of-range or NaN input cannot be represented and saturates,
/// exactly like the hardware conversion instruction.
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

// ── Shim symbols the engine expects from the OS ─────────────────────────────
//
// `Date` is the only engine feature that needs wall-clock time and a
// calendar. The cell has no timezone database, so `localtime_r` reports UTC —
// documented in docs/guides/ocel-viewer.md as a limitation, not a silent
// approximation of some other zone.

const USEC_PER_SEC: u64 = 1_000_000;

/// The only timezone this cell can name (see `localtime_r`).
static UTC_ZONE: &[u8] = b"UTC\0";
const SECS_PER_DAY: i64 = 86_400;

fn cell_time_usec() -> u64 {
    // `GetTime` op 0 is the architected counter at 10 MHz on RV64; op 1 is the
    // scheduler tick. `sys_get_time` returns the monotonic counter, which is
    // what `performance.now()`/`Date.now()` want at cell resolution.
    ostd::syscall::sys_get_time()
}

/// Convert a Unix timestamp to the civil date/time, UTC.
///
/// Howard Hinnant's `civil_from_days` algorithm, which is exact for the whole
/// `i64` range of the input and needs no lookup tables.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `gettimeofday` over the cell clock (ignores the (unused) timezone argument).
///
/// # Safety
/// `tv` must be null or point to a writable `struct timeval`.
#[no_mangle]
pub unsafe extern "C" fn gettimeofday(tv: *mut CellosTimeval, _tz: *mut c_void) -> c_int {
    if tv.is_null() {
        return -1;
    }
    let now = cell_time_usec();
    (*tv).tv_sec = (now / USEC_PER_SEC) as i64;
    (*tv).tv_usec = (now % USEC_PER_SEC) as i64;
    0
}

/// `clock_gettime` over the cell clock; both clocks this cell can be asked for
/// are the same monotonic counter.
///
/// # Safety
/// `tp` must be null or point to a writable `struct timespec`.
#[no_mangle]
pub unsafe extern "C" fn clock_gettime(_clk_id: c_int, tp: *mut CellosTimespec) -> c_int {
    if tp.is_null() {
        return -1;
    }
    let now = cell_time_usec();
    (*tp).tv_sec = (now / USEC_PER_SEC) as i64;
    (*tp).tv_nsec = ((now % USEC_PER_SEC) * 1000) as i64;
    0
}

/// `localtime_r`, UTC only (see the module comment).
///
/// # Safety
/// `timep` and `result` must be non-null and point to writable objects.
#[no_mangle]
pub unsafe extern "C" fn localtime_r(timep: *const i64, result: *mut CellosTm) -> *mut CellosTm {
    if timep.is_null() || result.is_null() {
        return core::ptr::null_mut();
    }
    let secs = *timep;
    let mut days = secs.div_euclid(SECS_PER_DAY);
    let mut rem = secs.rem_euclid(SECS_PER_DAY);
    // 1970-01-01 was a Thursday (wday = 4, Sunday = 0).
    let mut wday = (days + 4).rem_euclid(7) as i32;
    if wday < 0 {
        wday += 7;
    }
    let (year, month, day) = civil_from_days(days);
    let yday = {
        let jan1 = days - (day_of_year_utc(year, month, day) as i64);
        days -= jan1;
        days as i32
    };
    let t = &mut *result;
    t.tm_hour = (rem / 3600) as i32;
    rem %= 3600;
    t.tm_min = (rem / 60) as i32;
    t.tm_sec = (rem % 60) as i32;
    t.tm_mday = day as i32;
    t.tm_mon = month as i32 - 1;
    t.tm_year = year as i32 - 1900;
    t.tm_wday = wday;
    t.tm_yday = yday;
    t.tm_isdst = 0;
    t.tm_gmtoff = 0;
    t.tm_zone = UTC_ZONE.as_ptr() as *const c_char;
    let _ = &mut days;
    result
}

fn day_of_year_utc(year: i64, month: u32, day: u32) -> u32 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let extra = u32::from(leap && month > 2);
    CUM[(month - 1) as usize] + day - 1 + extra
}

/// `struct timeval` as the engine's `<sys/time.h>` declares it.
#[repr(C)]
pub struct CellosTimeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

/// `struct timespec`.
#[repr(C)]
pub struct CellosTimespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}

/// `struct tm`, matching `vendor/include/time.h`.
#[repr(C)]
pub struct CellosTm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    /// glibc/BSD extension: seconds east of UTC. The cell has no timezone
    /// database, so this is always 0 (UTC) and `tm_zone` always "UTC".
    pub tm_gmtoff: i64,
    pub tm_zone: *const c_char,
}
