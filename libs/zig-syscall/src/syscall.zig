// Cellos syscall ABI — inline asm for riscv64, aarch64, x86_64.
//
// riscv64: a7=nr, a0-a3=args, a0=ret, ecall
// aarch64: x0=nr, x1-x4=args, x0=ret, svc #0  ← NOT Linux's x8=nr
// x86_64:  rax=nr, rdi/rsi/rdx/r10=args, rax=ret, syscall
//
// Level B (mlibc linking) is riscv64 + aarch64 only — mlibc sysdeps have no
// x86_64 target yet. Level A (raw syscalls) works on all three architectures.

const builtin = @import("builtin");

// Syscall opcodes — must stay in sync with libs/api/src/syscall.rs
pub const SYS_EXIT: usize    = 60;
pub const SYS_LOG: usize     = 11;
pub const SYS_WRITE: usize   = 109;
pub const SYS_READ: usize    = 102;
pub const SYS_OPEN: usize    = 101;
pub const SYS_CLOSE: usize   = 103;
pub const SYS_SEEK: usize    = 106;
pub const SYS_GETTIME: usize = 120;

pub fn raw(nr: usize, a0: usize, a1: usize, a2: usize, a3: usize) usize {
    return switch (builtin.cpu.arch) {
        .riscv64 => asm volatile ("ecall"
            : [ret] "={a0}" (-> usize)
            : [nr]  "{a7}"  (nr),
              [a0]  "{a0}"  (a0),
              [a1]  "{a1}"  (a1),
              [a2]  "{a2}"  (a2),
              [a3]  "{a3}"  (a3),
            : "memory"
        ),
        // CELLOS ARM64 ABI: x0=nr (differs from Linux's x8=nr — not a typo).
        // x0 appears in both input (nr) and output (ret): LLVM handles this as
        // a tied register — nr is read, svc executes, result written back to x0.
        .aarch64 => asm volatile ("svc #0"
            : [ret] "={x0}" (-> usize)
            : [nr]  "{x0}"  (nr),
              [a0]  "{x1}"  (a0),
              [a1]  "{x2}"  (a1),
              [a2]  "{x3}"  (a2),
              [a3]  "{x4}"  (a3),
            : "memory"
        ),
        .x86_64 => asm volatile ("syscall"
            : [ret] "={rax}" (-> usize)
            : [nr]  "{rax}"  (nr),
              [a0]  "{rdi}"  (a0),
              [a1]  "{rsi}"  (a1),
              [a2]  "{rdx}"  (a2),
              [a3]  "{r10}"  (a3),
            : "rcx", "r11", "memory"
        ),
        else => @compileError("zig-syscall: unsupported Cellos architecture"),
    };
}

/// Terminate the cell. Does not return.
pub fn exit(code: u8) noreturn {
    _ = raw(SYS_EXIT, @as(usize, code), 0, 0, 0);
    unreachable;
}

/// Write a kernel log message (sys_log). The kernel prepends the cell's tid.
pub fn log(msg: []const u8) void {
    _ = raw(SYS_LOG, @intFromPtr(msg.ptr), msg.len, 0, 0);
}

/// Write bytes to a file descriptor (stdout = 1, stderr = 2).
pub fn write(fd: usize, buf: []const u8) void {
    _ = raw(SYS_WRITE, fd, @intFromPtr(buf.ptr), buf.len, 0);
}

pub const GetTimeOp = enum(usize) {
    /// Arch-specific monotonic ticks since boot.
    /// Frequency: 10 MHz on riscv64 (mtime), 62.5 MHz on aarch64 (CNTPCT), ns on x86_64.
    /// Do NOT assume 10 MHz on ARM64 — it is 6.25x faster.
    ticks    = 0,
    /// Wall-clock epoch nanoseconds (requires RTC).
    epoch_ns = 2,
    /// Wall-clock epoch seconds (requires RTC).
    epoch_secs = 3,
};

/// Return a time value from the kernel. Low 64-bit result only.
pub fn get_time(op: GetTimeOp) u64 {
    return @as(u64, raw(SYS_GETTIME, @intFromEnum(op), 0, 0, 0));
}
