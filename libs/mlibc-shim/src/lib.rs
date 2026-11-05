#![no_std]
//! mlibc-shim — link-only crate.
//!
//! Its build.rs emits `cargo:rustc-link-*` directives that inject the pre-built
//! mlibc `libc.a` into every cell that depends on this crate.
//!
//! It also keeps mlibc's start-up in the link: ostd's `_start` calls
//! `__cellos_libc_start(entry_sp)` before `main`, and ostd only carries a weak
//! default. The reference below extracts mlibc's strong definition from
//! libc.a (`third_party/mlibc/sysdeps/vicell/generic/startup.cpp`), which sets
//! up TLS, the Tcb and environ and then runs `.init_array` once (CP1
//! cell-initial-stack-auxv, AX-6).
//!
//! **Exclusivity:** a cell that depends on mlibc-shim MUST also enable the
//! `mlibc` feature on `api` (`api = { features = ["mlibc"] }`).  Failing to
//! do so links BOTH posix.rs symbols AND mlibc symbols, which is a
//! duplicate-symbol link error.

#[cfg(mlibc_linked)]
extern "C" {
    fn __cellos_libc_start_anchor();
}

#[cfg(mlibc_linked)]
#[used]
static KEEP_MLIBC_START: unsafe extern "C" fn() = __cellos_libc_start_anchor;
