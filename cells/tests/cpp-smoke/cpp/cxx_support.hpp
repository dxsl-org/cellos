//! Minimal freestanding C++ support for the `cpp-freestanding` profile.
//!
//! Why this exists: neither cross toolchain in this environment provides C++
//! standard headers. `riscv64-unknown-elf-g++` ships no libstdc++ headers, and
//! `clang++ --target=aarch64-unknown-none-elf` has no libc++ sysroot, so
//! `#include <cstdint>` fails on both. The profile is therefore *language-only*:
//! types come from compiler builtins and the helpers below, not from the
//! standard library.
//!
//! Providing a vendored libc++ header subset (or a freestanding `<type_traits>`
//! shim) is a separate decision with its own binary-size measurement.

#pragma once

using u8 = __UINT8_TYPE__;
using u16 = __UINT16_TYPE__;
using u32 = __UINT32_TYPE__;
using u64 = __UINT64_TYPE__;
using i8 = __INT8_TYPE__;
using i16 = __INT16_TYPE__;
using i32 = __INT32_TYPE__;
using i64 = __INT64_TYPE__;
using usize = __SIZE_TYPE__;
using isize = __PTRDIFF_TYPE__;

static_assert(sizeof(u32) == 4, "u32 must be 32 bits");
static_assert(sizeof(usize) == sizeof(void*), "usize must match pointer width");

/// Placement new. The compiler emits calls to it for by-value class returns and
/// for array element construction; without exceptions it can never fail.
inline void* operator new(usize, void* place) noexcept { return place; }
inline void operator delete(void*, void*) noexcept {}

/// Minimal compile-time helpers, in place of `<algorithm>`/`<utility>`.
template <typename T>
constexpr T tmax(T a, T b) {
    return a > b ? a : b;
}

template <typename T>
constexpr T tmin(T a, T b) {
    return a < b ? a : b;
}

template <typename T>
constexpr void tswap(T& a, T& b) {
    T t = a;
    a = b;
    b = t;
}
