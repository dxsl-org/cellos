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
        .name = "zig-mlibc-smoke",
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        .link_libc = false,
        .single_threaded = true,
    });

    exe.root_module.addImport("zig-syscall", zig_syscall_dep.module("zig-syscall"));
    exe.pie = true;
    exe.bundle_compiler_rt = true;

    // Select pre-built mlibc libc.a based on target arch.
    // Prerequisite: run `pwsh scripts/setup-mlibc.ps1` (riscv64) or
    //               `bash scripts/build-mlibc.sh` in WSL2 (aarch64).
    const mlibc_lib_dir = switch (target.result.cpu.arch) {
        .aarch64 => b.path("../../../third_party/mlibc/build-aarch64"),
        .riscv64  => b.path("../../../third_party/mlibc/build"),
        // mlibc sysdeps do not yet include x86_64 — use Level A (zig-hello) on x86_64.
        else => @panic("zig-mlibc-smoke Level B is only supported on riscv64 and aarch64"),
    };
    exe.addLibraryPath(mlibc_lib_dir);
    exe.linkSystemLibrary("c");

    const ld = switch (target.result.cpu.arch) {
        .aarch64 => b.path("zig-mlibc-smoke-arm64.ld"),
        else      => b.path("zig-mlibc-smoke.ld"),
    };
    exe.setLinkerScript(ld);

    b.installArtifact(exe);
}
