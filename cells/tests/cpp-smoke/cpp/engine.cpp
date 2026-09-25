//! The C++ language surface the `cpp-freestanding` profile claims.
//!
//! Each `extern "C"` entry point is one claim, and the Rust host asserts its
//! result and prints a marker:
//!
//!   * static construction  — `__init_array` really runs (crt0 walks it)
//!   * virtual dispatch     — vtables, override, and a virtual destructor
//!   * templates            — instantiation and constant folding
//!   * `new`/`delete`       — through the POSIX shim allocator, no libstdc++
//!   * file I/O             — the shim's `open`/`read`/`close` C ABI

#include "cxx_support.hpp"

// ── static construction ─────────────────────────────────────────────────────

namespace {

volatile u32 g_static_ctor_marker = 0;
volatile u32 g_static_dtor_marker = 0;

struct StaticBoot {
    StaticBoot() { g_static_ctor_marker = 0xC0FFEE11u; }
    ~StaticBoot() { g_static_dtor_marker = 1; }
};

StaticBoot g_static_boot;

}  // namespace

extern "C" u32 cpp_static_ctor_marker() {
    return g_static_ctor_marker;
}

/// Static destructors are not run (see `cxx_runtime.cpp`); the marker stays 0 and
/// the host asserts that, so the non-feature is pinned rather than assumed.
extern "C" u32 cpp_static_dtor_marker() {
    return g_static_dtor_marker;
}

// ── virtual dispatch ────────────────────────────────────────────────────────

namespace {

class Shape {
public:
    virtual i32 area() const = 0;
    virtual const char* name() const = 0;
    virtual ~Shape() {}
};

class Rect : public Shape {
public:
    Rect(i32 w, i32 h) : w_(w), h_(h) {}
    i32 area() const override { return w_ * h_; }
    const char* name() const override { return "rect"; }

protected:
    i32 w_;
    i32 h_;
};

class Square : public Rect {
public:
    explicit Square(i32 side) : Rect(side, side) {}
    const char* name() const override { return "square"; }
};

}  // namespace

/// 3x4 rectangle plus a 5x5 square = 12 + 25 = 37, through a `Shape*` array so
/// the call is a vtable dispatch and not a devirtualized direct call.
extern "C" i32 cpp_virtual_dispatch_total() {
    Rect rect(3, 4);
    Square square(5);
    Shape* shapes[2] = {&rect, &square};
    i32 total = 0;
    for (usize i = 0; i < 2; ++i) {
        total += shapes[i]->area();
    }
    return total;
}

/// Deleting through a base pointer exercises the virtual destructor and the
/// `operator delete` the shim provides.
extern "C" i32 cpp_virtual_delete() {
    Shape* shape = new Square(6);
    i32 area = shape->area();
    delete shape;
    return area;
}

// ── templates ───────────────────────────────────────────────────────────────

namespace {

template <typename T, usize N>
struct Stack {
    T data[N];

    T sum() const {
        T acc = 0;
        for (usize i = 0; i < N; ++i) {
            acc += data[i];
        }
        return acc;
    }
};

template <typename T>
T twice(T value) {
    return value + value;
}

}  // namespace

/// `tmax` from the support header plus two instantiations of `Stack`/`twice`:
/// 7 + 9 + (1+2+3) + (10+11) + (2 * 21 - 21) = 64.
extern "C" i32 cpp_template_total() {
    Stack<i32, 3> small{{1, 2, 3}};
    Stack<i64, 2> wide{{10, 11}};
    return tmax<i32>(3, 7) + static_cast<i32>(tmin<i64>(9, 40)) + small.sum() +
           static_cast<i32>(wide.sum()) + twice<i32>(21) - 21;
}

// ── allocator ───────────────────────────────────────────────────────────────

/// Allocate, write, read back, and release `iterations` blocks. The host asserts
/// the checksum, so a broken `operator new`/`delete` (or a shim allocator that
/// aliases blocks) shows up as a wrong sum rather than a crash.
extern "C" i32 cpp_heap_churn(i32 iterations) {
    i32 checksum = 0;
    for (i32 i = 0; i < iterations; ++i) {
        u32* block = new u32[16];
        for (usize j = 0; j < 16; ++j) {
            block[j] = static_cast<u32>(i) + static_cast<u32>(j);
        }
        for (usize j = 0; j < 16; ++j) {
            checksum += static_cast<i32>(block[j] % 7u);
        }
        delete[] block;
    }
    return checksum;
}

// ── POSIX shim file I/O ─────────────────────────────────────────────────────

extern "C" i32 open(const char* name, i32 flags, i32 mode);
extern "C" i32 read(i32 handle, void* buf, usize count);
extern "C" i32 close(i32 handle);

/// Read `path` (NUL-terminated) into `buf`, returning the byte count or a
/// negative error. This is the same C ABI a ported C/C++ library would use.
extern "C" i32 cpp_read_file(const char* path, u8* buf, usize len) {
    i32 fd = open(path, 0, 0);
    if (fd < 0) {
        return -1;
    }
    i32 total = 0;
    while (static_cast<usize>(total) < len) {
        i32 n = read(fd, buf + total, len - static_cast<usize>(total));
        if (n <= 0) {
            break;
        }
        total += n;
    }
    close(fd);
    return total;
}
