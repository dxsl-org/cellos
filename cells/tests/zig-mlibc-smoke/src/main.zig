// zig-mlibc-smoke — Tier 1b Zig Level B smoke test.
//
// Links against third_party/mlibc/build/libc.a.
// Prerequisite: pwsh scripts/setup-mlibc.ps1  (riscv64)
//               bash scripts/build-mlibc.sh   (aarch64, WSL2)
//
// Expected output:
//   ZIG-MLIBC: malloc OK
//   ZIG-MLIBC: printf OK (val=42)
//   ZIG-MLIBC: clock OK (sec=N)
//   ZIG-MLIBC: 3/3 pass

const sys = @import("zig-syscall").syscall;
const manifest = @import("zig-syscall").manifest;

comptime {
    manifest.declare(.{ .flags = 0 });
}

// mlibc C symbols — resolved from libc.a at link time.
extern fn malloc(size: usize) ?*anyopaque;
extern fn free(ptr: *anyopaque) void;
extern fn printf(fmt: [*:0]const u8, ...) c_int;
extern fn clock_gettime(clk_id: c_int, tp: *TimeSpec) c_int;

const TimeSpec = extern struct {
    tv_sec:  i64,
    tv_nsec: i64,
};
const CLOCK_MONOTONIC: c_int = 1;

// __libc_start_main from third_party/mlibc/sysdeps/vicell/generic/entry.cpp:
//   int __libc_start_main(int (*main_fn)(int, char**, char**), int argc, char **argv)
// It calls main_fn(argc, argv, null) and then sys_exit.
extern fn __libc_start_main(
    main_fn: *const fn (c_int, [*][*:0]u8, [*][*:0]u8) callconv(.C) c_int,
    argc:    c_int,
    argv:    [*][*:0]u8,
) c_int;

export fn _start() callconv(.C) noreturn {
    const S = struct {
        fn callMain(_: c_int, _: [*][*:0]u8, _: [*][*:0]u8) callconv(.C) c_int {
            cellMain();
            return 0;
        }
    };
    // Pass argc=0. dummy_argv is intentionally bogus — safe because mlibc's entry.cpp
    // trampoline forwards argv to main_fn verbatim and our callMain ignores all args.
    // If mlibc ever dereferences argv[0] during init, replace with a proper sentinel.
    const dummy_argv = [1][*:0]u8{@ptrFromInt(1)};
    _ = __libc_start_main(S.callMain, 0, @constCast(&dummy_argv));
    sys.exit(1); // unreachable: __libc_start_main calls sys_exit internally
}

fn cellMain() void {
    var pass: c_int = 0;
    const total: c_int = 3;

    // Test 1: malloc / free via mlibc's frg::slab_allocator → sys_anon_allocate
    {
        const ptr: ?*u8 = @ptrCast(malloc(64));
        if (ptr) |p| {
            p.* = 0xAB;
            if (p.* == 0xAB) {
                pass += 1;
                _ = printf("ZIG-MLIBC: malloc OK\n");
            } else {
                _ = printf("ZIG-MLIBC: malloc FAIL (bad read)\n");
            }
            free(p);
        } else {
            _ = printf("ZIG-MLIBC: malloc FAIL (null)\n");
        }
    }

    // Test 2: printf via mlibc's formatter → sys_write sysdep
    {
        const n = printf("ZIG-MLIBC: printf OK (val=%d)\n", @as(c_int, 42));
        if (n > 0) {
            pass += 1;
        } else {
            _ = printf("ZIG-MLIBC: printf FAIL\n");
        }
    }

    // Test 3: clock_gettime(CLOCK_MONOTONIC) → sys_clock_get → GetTime op=0
    {
        var ts = TimeSpec{ .tv_sec = 0, .tv_nsec = 0 };
        const ret = clock_gettime(CLOCK_MONOTONIC, &ts);
        if (ret == 0 and (ts.tv_sec > 0 or ts.tv_nsec > 0)) {
            pass += 1;
            _ = printf("ZIG-MLIBC: clock OK (sec=%lld)\n", ts.tv_sec);
        } else {
            _ = printf("ZIG-MLIBC: clock FAIL (ret=%d)\n", ret);
        }
    }

    _ = printf("ZIG-MLIBC: %d/%d pass\n", pass, total);
    sys.exit(if (pass == total) 0 else 1);
}
