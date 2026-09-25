// Build script for the Tetris cell.
//
// The upstream source is vendored in-tree: `src/c/tetris-os/` carries the files
// copied from Banaxi-Tech/Tetris-OS (MIT, Copyright (c) 2026 Banaxi; the licence
// text ships alongside as `src/c/tetris-os/LICENSE`) at commit 66c4466
// ("Initial Tetris OS kernel"). It used to be a bare gitlink with no
// `.gitmodules` entry, which no checkout could fetch, and the tree carried an
// uncommitted local edit for RISC-V. Both are fixed by vendoring:
//
//   * `src/tetris.c`'s idle loop now selects the wait instruction per ISA
//     (`hlt` on x86, `wfi` on RISC-V). Upstream has the x86 form unconditionally,
//     which does not assemble for riscv64 — the target this cell builds for.
//     That is the only change against upstream; keep it when re-syncing.
//   * Nothing else in the upstream tree is compiled, but the rest of `src/` is
//     kept so a future diff against upstream stays honest.
//
// Compiles tetris.c from Tetris-OS plus our vicell_platform.c.
// Platform hooks (vga_*, timer_*, keyboard_*, speaker_*) are implemented
// in src/c/vicell_platform.c — the original hardware drivers are NOT compiled.
//
// The game entry point is tetris_run() declared in tetris.h. If upstream ever
// renames it, update the extern "C" declaration in src/main.rs and the call in
// vicell_platform.c accordingly.

use std::path::Path;

const TETRIS_OS_DIR: &str = "src/c/tetris-os";

fn main() {
    let dir = Path::new(TETRIS_OS_DIR);
    if !dir.exists() {
        panic!(
            "vendored Tetris-OS source missing at {TETRIS_OS_DIR}; restore it from Banaxi-Tech/Tetris-OS (MIT) — see the header of this build script"
        );
    }

    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();

    // RISC-V bare-metal toolchain
    if target.contains("riscv") {
        if std::env::var("CC_riscv64gc_unknown_none_elf").is_err() {
            build.compiler("riscv-none-elf-gcc");
        }
        build.flag("-mabi=lp64d");
        if std::env::var("AR_riscv64gc_unknown_none_elf").is_err()
            && std::env::var("AR_riscv64gc-unknown-none-elf").is_err()
            && std::env::var("TARGET_AR").is_err()
            && std::env::var("AR").is_err()
        {
            build.archiver("riscv-none-elf-ar");
        }
        let sysroot = run_riscv_gcc(&["--print-sysroot"]);
        if !sysroot.is_empty() && sysroot != "." {
            build.flag(format!("-I{}/include", sysroot));
        }
    }

    if target.contains("x86_64") || target.contains("aarch64") {
        build.flag("-mcmodel=small");
    }

    build.warnings(false);
    build.flag_if_supported("-std=gnu99");

    // Make tetris-os headers visible for both tetris.c and vicell_platform.c
    build.include(TETRIS_OS_DIR);
    build.include(format!("{TETRIS_OS_DIR}/src"));
    // Make src/c/ visible so vicell_platform.c can #include local helpers if needed
    build.include("src/c");

    // Compile ONLY the pure game logic from tetris-os.
    // Excluded: vga.c, keyboard.c, timer.c, speaker.c (replaced by vicell_platform.c)
    //           main.c (entry provided by Rust main())
    //           Any x86 kernel bootstrap files (idt.c, gdt.c, isr.c, etc.)
    let tetris_c = if Path::new(TETRIS_OS_DIR).join("tetris.c").exists() {
        Path::new(TETRIS_OS_DIR).join("tetris.c")
    } else if Path::new(TETRIS_OS_DIR).join("src/tetris.c").exists() {
        Path::new(TETRIS_OS_DIR).join("src/tetris.c")
    } else {
        panic!("required source missing: {TETRIS_OS_DIR}/tetris.c");
    };
    build.file(&tetris_c);
    // Our ViCell platform implementation replaces all hardware drivers
    build.file("src/c/vicell_platform.c");

    build.compile("tetris");

    // PIE linker script — kernel assigns VA at spawn time.
    cell_build::emit_linker_script();

    println!("cargo:rerun-if-changed={TETRIS_OS_DIR}");
    println!("cargo:rerun-if-changed=src/c/vicell_platform.c");
}

fn run_riscv_gcc(args: &[&str]) -> String {
    std::process::Command::new("riscv64-unknown-elf-gcc")
        .args(["-march=rv64gc", "-mabi=lp64d"])
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}
