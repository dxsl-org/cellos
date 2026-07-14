// SPDX-License-Identifier: MPL-2.0
// stdio: FILE type, printf/fprintf/sprintf/snprintf, fopen/fclose/fread/fwrite
//
// v* variants take VaList<'_> directly (C `va_list` ABI).
// Variadic variants take `...` and pass to the v* helpers via implicit coercion.

#![allow(unsafe_code)]
#![allow(non_upper_case_globals)]

use super::net::_close;
use super::stdio_fmt::vsnprintf_core;
use super::sysio::{_open, _read, _write};
use core::ffi::{c_char, c_int, c_long, c_void, VaList};

// ---------------------------------------------------------------------------
// FILE type and standard streams
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct FILE {
    pub fd: i32,
    _pad: [u8; 28], // match common struct size expectations
}

static mut STDOUT_FILE: FILE = FILE {
    fd: 1,
    _pad: [0; 28],
};
static mut STDERR_FILE: FILE = FILE {
    fd: 2,
    _pad: [0; 28],
};
static mut STDIN_FILE: FILE = FILE {
    fd: 0,
    _pad: [0; 28],
};

#[no_mangle]
pub static mut stdout: *mut FILE = core::ptr::addr_of_mut!(STDOUT_FILE);
#[no_mangle]
pub static mut stderr: *mut FILE = core::ptr::addr_of_mut!(STDERR_FILE);
#[no_mangle]
pub static mut stdin: *mut FILE = core::ptr::addr_of_mut!(STDIN_FILE);

// ---------------------------------------------------------------------------
// Helper: extract fd from a FILE*, handling both our FILE structs and
// foreign picolibc FILE* (which has _flags at offset 0, not fd).
// ---------------------------------------------------------------------------
unsafe fn fd_of(stream: *mut FILE) -> i32 {
    if stream.is_null() {
        return 1;
    }
    if core::ptr::eq(stream, core::ptr::addr_of!(STDOUT_FILE) as *const _) {
        return 1;
    }
    if core::ptr::eq(stream, core::ptr::addr_of!(STDERR_FILE) as *const _) {
        return 2;
    }
    if core::ptr::eq(stream, core::ptr::addr_of!(STDIN_FILE) as *const _) {
        return 0;
    }
    let raw = (*stream).fd;
    if raw > 0 {
        raw
    } else {
        2
    }
}

// ---------------------------------------------------------------------------
// v* variants — accept VaList<'_> directly (C va_list ABI-compatible)
// ---------------------------------------------------------------------------

/// Formats `fmt` with `args` into `buf`, writing at most `size` bytes (including
/// the NUL terminator).
///
/// # Safety
/// `buf` must be null (with `size == 0`) or valid for writes of at least `size`
/// bytes; `fmt` must point to a valid NUL-terminated C string; `args` must be a
/// `VaList` whose remaining arguments match the conversion specifiers in `fmt`.
#[no_mangle]
pub unsafe extern "C" fn vsnprintf(
    buf: *mut c_char,
    size: usize,
    fmt: *const c_char,
    args: VaList<'_>,
) -> c_int {
    vsnprintf_core(buf, size, fmt, args) as c_int
}

/// Formats `fmt` with `args` into `buf` with no length limit (C `sprintf` semantics).
///
/// # Safety
/// `buf` must be valid for writes large enough to hold the entire formatted
/// output plus a NUL terminator — the caller is responsible for sizing it, as
/// this function performs no bounds checking beyond an internal cap; `fmt` must
/// point to a valid NUL-terminated C string; `args` must match `fmt`'s specifiers.
#[no_mangle]
pub unsafe extern "C" fn vsprintf(buf: *mut c_char, fmt: *const c_char, args: VaList<'_>) -> c_int {
    vsnprintf_core(buf, usize::MAX / 2, fmt, args) as c_int
}

/// Formats `fmt` with `args` and writes the result to stdout (fd 1).
///
/// # Safety
/// `fmt` must point to a valid NUL-terminated C string; `args` must be a
/// `VaList` whose remaining arguments match the conversion specifiers in `fmt`.
#[no_mangle]
pub unsafe extern "C" fn vprintf(fmt: *const c_char, args: VaList<'_>) -> c_int {
    let mut tmp = [0u8; 1024];
    let n = vsnprintf_core(tmp.as_mut_ptr(), tmp.len(), fmt, args);
    _write(1, tmp.as_ptr() as *const c_void, n.min(tmp.len()));
    n as c_int
}

/// Formats `fmt` with `args` and writes the result to `stream`.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (one of `stdout`/`stderr`/`stdin`
/// or a pointer previously returned by [`fopen`]), since it is dereferenced by
/// [`fd_of`]; `fmt` must point to a valid NUL-terminated C string; `args` must
/// match `fmt`'s specifiers.
#[no_mangle]
pub unsafe extern "C" fn vfprintf(
    stream: *mut FILE,
    fmt: *const c_char,
    args: VaList<'_>,
) -> c_int {
    let fd = fd_of(stream);
    let mut tmp = [0u8; 1024];
    let n = vsnprintf_core(tmp.as_mut_ptr(), tmp.len(), fmt, args);
    _write(fd, tmp.as_ptr() as *const c_void, n.min(tmp.len()));
    n as c_int
}

// ---------------------------------------------------------------------------
// Variadic public API — pass `args` directly to v* (implicit ... → VaList coercion)
// ---------------------------------------------------------------------------

/// Formats `fmt` with the trailing variadic arguments and writes the result to stdout.
///
/// # Safety
/// Same preconditions as [`vprintf`]: `fmt` must point to a valid NUL-terminated
/// C string, and the variadic arguments must match its conversion specifiers.
#[no_mangle]
pub unsafe extern "C" fn printf(fmt: *const c_char, args: ...) -> c_int {
    vprintf(fmt, args)
}

/// Formats `fmt` with the trailing variadic arguments and writes the result to `stream`.
///
/// # Safety
/// Same preconditions as [`vfprintf`]: `stream` must be null or a valid `FILE*`,
/// `fmt` must point to a valid NUL-terminated C string, and the variadic
/// arguments must match its conversion specifiers.
#[no_mangle]
pub unsafe extern "C" fn fprintf(stream: *mut FILE, fmt: *const c_char, args: ...) -> c_int {
    vfprintf(stream, fmt, args)
}

/// Formats `fmt` with the trailing variadic arguments into `buf`, unbounded (C
/// `sprintf` semantics).
///
/// # Safety
/// Same preconditions as [`vsprintf`]: `buf` must be valid for writes large
/// enough to hold the entire formatted output plus a NUL terminator, `fmt`
/// must point to a valid NUL-terminated C string, and the variadic arguments
/// must match its conversion specifiers.
#[no_mangle]
pub unsafe extern "C" fn sprintf(buf: *mut c_char, fmt: *const c_char, args: ...) -> c_int {
    vsprintf(buf, fmt, args)
}

/// Formats `fmt` with the trailing variadic arguments into `buf`, writing at
/// most `size` bytes (including the NUL terminator).
///
/// # Safety
/// Same preconditions as [`vsnprintf`]: `buf` must be null (with `size == 0`)
/// or valid for writes of at least `size` bytes, `fmt` must point to a valid
/// NUL-terminated C string, and the variadic arguments must match its
/// conversion specifiers.
#[no_mangle]
pub unsafe extern "C" fn snprintf(
    buf: *mut c_char,
    size: usize,
    fmt: *const c_char,
    args: ...
) -> c_int {
    vsnprintf(buf, size, fmt, args)
}

// ---------------------------------------------------------------------------
// FILE I/O
// ---------------------------------------------------------------------------

/// Opens a file. Returns a heap-allocated FILE* on success, NULL on failure.
///
/// # Safety
/// `path` must point to a valid NUL-terminated C string; `mode` must be null
/// or point to a valid NUL-terminated C string (only its first byte is read).
#[no_mangle]
pub unsafe extern "C" fn fopen(path: *const c_char, mode: *const c_char) -> *mut FILE {
    let flags = if !mode.is_null() && *mode == b'w' as c_char {
        0x201
    } else {
        0
    };
    let fd = _open(path, flags, 0o644);
    if fd < 0 {
        return core::ptr::null_mut();
    }
    let f = super::alloc::malloc_impl(core::mem::size_of::<FILE>()) as *mut FILE;
    if f.is_null() {
        _close(fd);
        return core::ptr::null_mut();
    }
    (*f).fd = fd;
    (*f)._pad = [0; 28];
    f
}

/// Closes `stream`, freeing it if it was heap-allocated by [`fopen`].
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (one of `stdout`/`stderr`/`stdin`
/// or a pointer previously returned by [`fopen`] and not yet closed) — it is
/// dereferenced by [`fd_of`] and, if heap-allocated, passed to `free_impl`.
#[no_mangle]
pub unsafe extern "C" fn fclose(stream: *mut FILE) -> c_int {
    if stream.is_null() {
        return -1;
    }
    let fd = fd_of(stream);
    // Only free heap-allocated FILE* (opened via fopen); never free static streams.
    if fd > 2 {
        super::alloc::free_impl(stream as *mut c_void);
    }
    _close(fd)
}

/// Reads up to `size * nmemb` bytes from `stream` into `ptr`, looping until
/// the full amount is read or the underlying `_read` returns EOF/error.
///
/// # Safety
/// `ptr` must be null or valid for writes of at least `size * nmemb` bytes;
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fread(
    ptr: *mut c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if stream.is_null() || ptr.is_null() {
        return 0;
    }
    let total = size.saturating_mul(nmemb);
    let fd = fd_of(stream);
    // Loop to match C standard fread semantics: read until all bytes received
    // or EOF/error. A single _read may return fewer bytes than requested.
    let mut done = 0usize;
    while done < total {
        let n = _read(fd, (ptr as *mut u8).add(done) as *mut c_void, total - done);
        if n <= 0 {
            break;
        }
        done += n as usize;
    }
    done / size.max(1)
}

/// Writes `size * nmemb` bytes from `ptr` to `stream`.
///
/// # Safety
/// `ptr` must be null or valid for reads of at least `size * nmemb` bytes;
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fwrite(
    ptr: *const c_void,
    size: usize,
    nmemb: usize,
    stream: *mut FILE,
) -> usize {
    if stream.is_null() || ptr.is_null() {
        return 0;
    }
    let total = size.saturating_mul(nmemb);
    let n = _write(fd_of(stream), ptr, total);
    if n <= 0 {
        0
    } else {
        n as usize / size.max(1)
    }
}

/// Writes the NUL-terminated string `s` to `stream` (no trailing newline added).
///
/// # Safety
/// `s` must point to a valid NUL-terminated C string; `stream` must be null
/// or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fputs(s: *const c_char, stream: *mut FILE) -> c_int {
    if s.is_null() {
        return -1;
    }
    let len = super::strings::strlen(s);
    _write(fd_of(stream), s as *const c_void, len)
}

/// Writes the single byte `c` to `stream`.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fputc(c: c_int, stream: *mut FILE) -> c_int {
    let b = [c as u8];
    if _write(fd_of(stream), b.as_ptr() as *const c_void, 1) == 1 {
        c
    } else {
        -1
    }
}

/// Reads a single byte from `stream`, returning it as an unsigned `c_int` or
/// `-1` on EOF/error.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fgetc(stream: *mut FILE) -> c_int {
    let mut b = 0u8;
    // For reads, fd_of returns 2 for unknown streams, but 0 (stdin) for stdin.
    let fd = if stream.is_null() { 0 } else { fd_of(stream) };
    if _read(fd, &mut b as *mut u8 as *mut c_void, 1) == 1 {
        b as c_int
    } else {
        -1
    }
}

/// Reads up to `n - 1` bytes from `stream` into `buf` and NUL-terminates the result.
///
/// # Safety
/// `buf` must be valid for writes of at least `n` bytes when `n > 0`; `stream`
/// must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fgets(buf: *mut c_char, n: c_int, stream: *mut FILE) -> *mut c_char {
    if buf.is_null() || n <= 0 {
        return core::ptr::null_mut();
    }
    let fd = if stream.is_null() { 0 } else { fd_of(stream) };
    let r = _read(fd, buf as *mut c_void, (n - 1) as usize);
    if r <= 0 {
        return core::ptr::null_mut();
    }
    *buf.add(r as usize) = 0;
    buf
}

/// Writes the single byte `c` to stdout (fd 1).
///
/// # Safety
/// No pointer preconditions beyond the C ABI; safe to call with any `c`.
#[no_mangle]
pub unsafe extern "C" fn putchar(c: c_int) -> c_int {
    let b = [c as u8];
    _write(1, b.as_ptr() as *const c_void, 1);
    c
}

/// Writes the NUL-terminated string `s` followed by a newline to stdout.
///
/// # Safety
/// `s` must point to a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn puts(s: *const c_char) -> c_int {
    if s.is_null() {
        return -1;
    }
    let len = super::strings::strlen(s);
    _write(1, s as *const c_void, len);
    _write(1, b"\n".as_ptr() as *const c_void, 1);
    1
}

// ---------------------------------------------------------------------------
// File positioning — fseek / ftell / rewind
// ---------------------------------------------------------------------------

/// fseek: seek fd to `offset` from `whence` (SEEK_SET=0, SEEK_CUR=1, SEEK_END=2).
/// We implement this ourselves so our simple FILE* (fd at offset 0) is used
/// correctly — picolibc's fseek reads _file from a different offset and would
/// call lseek(0, ...) instead of the real fd.
/// Seeks `stream` to `offset` from `whence` (SEEK_SET=0, SEEK_CUR=1, SEEK_END=2).
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fseek(stream: *mut FILE, offset: c_long, whence: c_int) -> c_int {
    if stream.is_null() {
        return -1;
    }
    let fd = fd_of(stream);
    let r = super::sysio::_lseek(fd, offset, whence);
    if r < 0 {
        -1
    } else {
        0
    }
}

/// Returns the current seek position of `stream`.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn ftell(stream: *mut FILE) -> c_long {
    if stream.is_null() {
        return -1;
    }
    super::sysio::_lseek(fd_of(stream), 0, 1) // SEEK_CUR
}

/// Resets `stream`'s seek position to the beginning.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn rewind(stream: *mut FILE) {
    if stream.is_null() {
        return;
    }
    super::sysio::_lseek(fd_of(stream), 0, 0); // SEEK_SET
}

// ---------------------------------------------------------------------------
// Error / state stubs
// ---------------------------------------------------------------------------

/// Stub: always reports no error.
///
/// # Safety
/// `_stream` is unused; no preconditions.
#[no_mangle]
pub unsafe extern "C" fn ferror(_stream: *mut FILE) -> c_int {
    0
}
/// Stub: always reports not-at-EOF.
///
/// # Safety
/// `_stream` is unused; no preconditions.
#[no_mangle]
pub unsafe extern "C" fn feof(_stream: *mut FILE) -> c_int {
    0
}
/// Stub: no-op (no error state is tracked).
///
/// # Safety
/// `_stream` is unused; no preconditions.
#[no_mangle]
pub unsafe extern "C" fn clearerr(_stream: *mut FILE) {}
/// Stub: no-op (writes are unbuffered).
///
/// # Safety
/// `_stream` is unused; no preconditions.
#[no_mangle]
pub unsafe extern "C" fn fflush(_stream: *mut FILE) -> c_int {
    0
}
/// Returns the underlying file descriptor for `stream`.
///
/// # Safety
/// `stream` must be null or a valid `FILE*` (dereferenced by [`fd_of`]).
#[no_mangle]
pub unsafe extern "C" fn fileno(stream: *mut FILE) -> c_int {
    if stream.is_null() {
        -1
    } else {
        fd_of(stream)
    }
}
