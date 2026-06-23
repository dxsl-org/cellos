// zig-hello — Tier 1b Zig Level A smoke test.
//
// No mlibc dependency. Uses libs/zig-syscall for raw Cellos syscalls.
// Expected output:
//   ZIG-HELLO: Zig cell running on Cellos!
//   ZIG-HELLO: sys_get_time ticks=<N>

const sys = @import("zig-syscall").syscall;
const manifest = @import("zig-syscall").manifest;

// Emit __ViCell_manifest — no capabilities needed for this smoke test.
comptime {
    manifest.declare(.{ .flags = 0 });
}

fn print(msg: []const u8) void {
    sys.write(1, msg);
}

// Format a u64 as decimal into buf, return the slice written.
fn fmtU64(buf: []u8, val: u64) []u8 {
    if (val == 0) {
        buf[0] = '0';
        return buf[0..1];
    }
    var digits: [20]u8 = undefined;
    var d: usize = 0;
    var v = val;
    while (v > 0) : (d += 1) {
        digits[d] = '0' + @as(u8, @intCast(v % 10));
        v /= 10;
    }
    var i: usize = 0;
    while (d > 0) : ({ d -= 1; i += 1; }) {
        buf[i] = digits[d - 1];
    }
    return buf[0..i];
}

export fn _start() callconv(.C) noreturn {
    print("ZIG-HELLO: Zig cell running on Cellos!\n");

    const ticks = sys.get_time(.ticks);
    var tbuf: [24]u8 = undefined;
    print("ZIG-HELLO: sys_get_time ticks=");
    print(fmtU64(&tbuf, ticks));
    print("\n");

    sys.exit(0);
}
