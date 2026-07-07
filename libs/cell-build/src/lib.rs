//! Build helper for ViCell Cells.
//!
//! Generates a per-architecture PIE linker script and emits the Cargo
//! directives needed to compile a cell as a Position-Independent Executable.
//!
//! # Usage
//!
//! In any cell's `build.rs`:
//! ```no_run
//! fn main() {
//!     cell_build::emit_linker_script();
//! }
//! ```
//!
//! For cells with a non-standard entry point (e.g. C cells using `_start`):
//! ```no_run
//! fn main() {
//!     cell_build::emit_linker_script_entry("_start");
//! }
//! ```

use std::{env, fs, path::PathBuf};

/// Arch-neutral linker script template.  `OUTPUT_ARCH` is prepended per-arch.
const TEMPLATE: &str = include_str!("cell.ld.in");

/// Emit the Cargo directives to build this cell as a PIE with `ENTRY(main)`.
///
/// NOTE: entering at bare `main` means the ostd `_start` crt0 (which would run
/// init-array + issue the post-`main` `Exit` ecall) is bypassed, so a cell
/// whose `main` RETURNS falls into `ret` with `ra = 0` → jump to 0 →
/// instruction page fault (scause=0xc, sepc=0) on exit. This is currently
/// COSMETIC: the cell has finished its work; the fault fires as it exits.
/// The clean fix (default to `ENTRY(_start)`) is blocked because `_start`'s
/// init-array asm uses absolute (`ldr =sym` / ABS64) addressing that the `-pie`
/// linker rejects on aarch64/x86 — `_start` must be made PC-relative on all
/// arches first. Tracked in .agents/260706-0952-system-analysis-g1-g3.
pub fn emit_linker_script() {
    emit_linker_script_entry("main");
}

/// Emit the Cargo directives to build this cell as a PIE with a custom entry.
///
/// Use this for cells whose entry symbol is not `main` (e.g. C cells that
/// define `_start` or `cell_main`).
pub fn emit_linker_script_entry(entry: &str) {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let output_arch = match arch.as_str() {
        "riscv64" | "riscv32" => "riscv",
        "aarch64"             => "aarch64",
        "x86_64"              => "i386:x86-64",
        other => {
            println!(
                "cargo:warning=cell-build: unsupported arch '{other}', linker script not emitted"
            );
            return;
        }
    };

    // Replace ENTRY(main) with the requested entry point.
    let content = format!(
        "OUTPUT_ARCH({output_arch})\n{}",
        TEMPLATE.replace("ENTRY(main)", &format!("ENTRY({entry})"))
    );

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set by Cargo");
    let ld_path = PathBuf::from(&out_dir).join("cell.ld");
    fs::write(&ld_path, &content).expect("cell-build: failed to write cell.ld to OUT_DIR");

    // Absolute path prevents the linker-search-path collision where two crates
    // both emit `-T cell.ld` and the wrong one wins (Embedonomicon §Overriding).
    println!("cargo:rustc-link-arg=-T{}", ld_path.display());

    // PIE: pass -pie to the linker so the ELF type is ET_DYN.
    // The kernel loader checks ET_DYN to detect PIE cells and allocates a
    // dynamic VA base at spawn time via the cell VA allocator.
    // NOTE: `cargo:rustc-flags` only allows -l/-L; codegen flags (-C) must be
    // set via .cargo/config.toml per-target or RUSTFLAGS at build time.
    // We rely on the linker -pie flag alone — the linker script VA=0x0 base
    // combined with R_RELATIVE relocations from rustc's default static model
    // is sufficient for the kernel's cell loader.
    println!("cargo:rustc-link-arg=-pie");

    println!("cargo:rerun-if-changed=build.rs");
    // The template is embedded via include_str! — a change to cell.ld.in must
    // also invalidate this build script's output.
    println!("cargo:rerun-if-changed=src/cell.ld.in");
}
