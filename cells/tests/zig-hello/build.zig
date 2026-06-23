const std = @import("std");

pub fn build(b: *std.Build) void {
    const target_str = b.option([]const u8, "target", "Target triple")
        orelse "riscv64-freestanding-none";

    const target = b.resolveTargetQuery(
        std.Target.Query.parse(.{ .arch_os_abi = target_str }) catch
            @panic("invalid target triple"),
    );

    const optimize = b.standardOptimizeOption(.{});

    // zig-syscall is a pure module — no build config needed.
    const zig_syscall_dep = b.dependency("zig_syscall", .{});

    const exe = b.addExecutable(.{
        .name = "zig-hello",
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = false,
        .single_threaded = true,
    });

    exe.root_module.addImport("zig-syscall", zig_syscall_dep.module("zig-syscall"));
    exe.pie = true;

    const ld = switch (target.result.cpu.arch) {
        .aarch64 => b.path("zig-hello-arm64.ld"),
        else     => b.path("zig-hello.ld"),
    };
    exe.setLinkerScript(ld);

    exe.root_module.omit_frame_pointer = true;

    b.installArtifact(exe);
}
