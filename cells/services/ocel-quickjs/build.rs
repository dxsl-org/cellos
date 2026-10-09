//! Build the vendored QuickJS engine for this cell.
//!
//! Provenance, the vendored file set, and the complete inventory of local
//! deviations from upstream live in `cells/services/ocel-quickjs/README.md`.
//!
//! Two things make this build different from the Lua cell's:
//!
//! * **No external libc.** Lua links the toolchain's `libc.a`/`libm.a` on
//!   riscv64, which is why CI cannot build it (`Ubuntu's bare-elf gcc ships no
//!   libc.a`). QuickJS here links nothing but the Rust crates it depends on:
//!   `libs/api` already exports the C ABI it needs (allocation, strings, stdio,
//!   the C99 math set via `libm`), and this cell supplies the three symbols that
//!   ABI does not have (`gettimeofday`, `clock_gettime`, `localtime_r`) plus an
//!   `errno`-free config that avoids float `printf` and `strtod` entirely.
//! * **We own the headers.** Upstream includes `<stdlib.h>`, `<stdio.h>`,
//!   `<math.h>`, … which no bare-elf toolchain provides; `vendor/include/`
//!   declares exactly the subset the engine uses.

use std::path::{Path, PathBuf};

fn repo_wrapper(name: &str) -> Option<PathBuf> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let path = Path::new(&manifest).join("../../../tools").join(name);
    if path.exists() && have(path.to_str().unwrap_or_default()) {
        path.canonicalize().ok()
    } else {
        None
    }
}

fn have(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}
/// Upstream file set of `libquickjs` (`Makefile`: `QJS_LIB_OBJS`).
const ENGINE_SOURCES: &[&str] = &[
    "quickjs.c",
    "libregexp.c",
    "libunicode.c",
    "cutils.c",
    "dtoa.c",
];

fn main() {
    println!("cargo::rustc-check-cfg=cfg(qjs_c_unavailable)");
    println!("cargo:rerun-if-changed=vendor/quickjs");
    println!("cargo:rerun-if-changed=vendor/include");
    println!("cargo:rerun-if-changed=vendor/qjs_vios_config.h");
    println!("cargo:rerun-if-changed=src/c/qjs_cellos_glue.c");

    // The cell's identity line names the engine version it actually vendored,
    // read from the same VERSION file the licence travels with.
    let version = std::fs::read_to_string("vendor/quickjs/VERSION")
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|_| String::from("unknown"));
    println!("cargo:rustc-env=QJS_VENDOR_VERSION={version}");

    let target = std::env::var("TARGET").unwrap_or_default();

    // The engine links against the Tier-A C ABI, which `libs/api` provides only
    // on riscv64/aarch64 (`#![cfg(any(target_arch = "riscv64",
    // target_arch = "aarch64", …))]` in `libs/api/src/services/posix.rs`). On any
    // other target — and on a host with no ELF-capable C compiler — the cell
    // builds as a stub that says so instead of failing the workspace build at
    // link time. Same shape as the Lua cell's `lua_c_unavailable`.
    let tier_a_abi = target.starts_with("riscv64")
        || target.starts_with("aarch64")
        || target.starts_with("x86_64");
    if !tier_a_abi || !has_elf_compiler(&target) {
        println!("cargo:rustc-cfg=qjs_c_unavailable");
        println!("cargo:warning=ocel-quickjs: no engine for {target};");
        if !tier_a_abi {
            println!("cargo:warning=  the Tier-A C ABI exists only on riscv64/aarch64/x86_64.");
        } else {
            println!("cargo:warning=  no ELF-capable C compiler for {target}.");
        }
        println!("cargo:warning=  building the cell without the engine (stub).");
        cell_build::emit_linker_script();
        return;
    }

    compile_engine(&target);
    cell_build::emit_linker_script();
}

fn compile_engine(target: &str) {
    let vendor = Path::new("vendor/quickjs");
    let include = Path::new("vendor/include");

    // cc-rs appends the toolchain's `CFLAGS_<target>` (from `.cargo/config.toml`
    // or CI) to its own flags. Those flags exist for the C in *other* cells:
    // they add `-I third_party/freestanding-include` (a declaration set this
    // crate deliberately does not use — `vendor/include` is complete), and on
    // x86_64 they add `-mno-sse`, which a floating-point engine cannot compile
    // under. Dropping the variable makes this crate's flags the only ones.
    let cflags_key = format!("CFLAGS_{}", target.replace('-', "_"));
    std::env::remove_var(&cflags_key);

    let mut build = cc::Build::new();
    let env_key = target.replace('-', "_");
    let use_zig = std::env::var("CELLOS_USE_ZIG").map(|v| v == "1" || v == "true").unwrap_or(false);
    let custom_cc = std::env::var(format!("CC_{env_key}"))
        .or_else(|_| std::env::var("CC"))
        .ok();
    let custom_ar = std::env::var(format!("AR_{env_key}"))
        .or_else(|_| std::env::var("AR"))
        .ok();

    if use_zig {
        if let Some(wrapper) = repo_wrapper("cellos-zig-cc") {
            build.compiler(wrapper);
            if let Some(ar) = repo_wrapper("cellos-zig-ar") {
                build.archiver(ar);
            }
        }
    } else if let Some(cc) = custom_cc {
        build.compiler(cc);
    } else if let Some(wrapper) = repo_wrapper("cellos-zig-cc") {
        build.compiler(wrapper);
        if let Some(ar) = repo_wrapper("cellos-zig-ar") {
            build.archiver(ar);
        }
    }
    if let Some(ar) = custom_ar {
        build.archiver(ar);
    }
    build.include(vendor);
    build.include(include);
    build.include("vendor");

    // The engine's own config layer: declares the -D switches and substitutes
    // the one symbol the Tier-A ABI lacks (see the header's comments).
    build.flag("-include").flag("qjs_vios_config.h");

    build.define("_GNU_SOURCE", None);
    build.define("CONFIG_VERSION", Some("\"2026-06-04\""));
    build.define("NDEBUG", None);

    // Bare-metal: no host headers beyond vendor/include, no unwind tables, and
    // nothing that would emit a call into a libc this cell does not link.
    build.flag("-ffreestanding");
    build.flag("-fno-stack-protector");
    build.flag("-fno-asynchronous-unwind-tables");
    build.flag("-fno-unwind-tables");
    build.flag_if_supported("-std=gnu11");
    // Size, not speed, is the constraint that matters here: this cell's ELF is
    // read through the VFS cell (whose heap is a few MiB) on its way to the
    // loader, and the engine has no hot enough path to notice -O2 over -Os.
    build.flag_if_supported("-Os");
    build.warnings(false);
    // A cell has no debugger and no dynamic linker; the symbol table is pure
    // file weight (700 KB of the engine's ~1.4 MB ELF) on a path that copies
    // the whole file. Keep it out of the link.
    println!("cargo:rustc-link-arg=--strip-all");
    // Cells link as -pie (cell.ld): the objects must be position independent.
    build.pic(true);

    if target.contains("riscv") {
        // The cc crate's auto-detection looks for `riscv64-unknown-elf-gcc`;
        // the xPack toolchain in this tree is `riscv-none-elf-gcc`. An explicit
        // CC_<target> wins when set (CI sets it).
        if std::env::var("CC_riscv64gc_unknown_none_elf").is_err() {
            build.compiler("riscv-none-elf-gcc");
        }
        // The arch+ABI flags must apply unconditionally: without them a gcc
        // that defaults to soft-float produces objects rust-lld rejects against
        // the lp64d Rust target.
        build.flag("-march=rv64gc");
        build.flag("-mabi=lp64d");
        build.flag("-mcmodel=medany");
    }
    if target.contains("aarch64") {
        build.flag("-mgeneral-regs-only");
    }
    if target.contains("x86_64") {
        build.flag("-mno-red-zone");
        build.flag("-mcmodel=small");
        // The engine is floating point throughout; SSE2 is the x86_64 baseline
        // and the Rust side of this target compiles with it enabled.
        build.flag("-msse2");
        build.flag("-mfpmath=sse");
    }

    for file in ENGINE_SOURCES {
        build.file(vendor.join(file));
    }
    // Cellos glue: exports the header's `static inline` predicates and `abs`
    // (src/c/qjs_cellos_glue.c).
    build.file("src/c/qjs_cellos_glue.c");
    build.compile("quickjs");
}

/// True when an ELF-capable C compiler is available for `target`.
///
/// Mirrors the Lua cell's probe: an explicit `CC_<target>` means the caller has
/// decided, a non-MSVC host always produces ELF, and on MSVC a cross compiler
/// must be found by name.
fn has_elf_compiler(target: &str) -> bool {
    if repo_wrapper("cellos-zig-cc").is_some() {
        return true;
    }
    let env_key = target.replace('-', "_");
    if std::env::var(format!("CC_{env_key}")).is_ok() {
        return true;
    }
    let host = std::env::var("HOST").unwrap_or_default();
    if !host.contains("msvc") {
        return true;
    }
    let candidates: &[&str] = if target.contains("aarch64") {
        &["clang", "aarch64-none-elf-gcc", "aarch64-linux-gnu-gcc"]
    } else {
        &["clang", "x86_64-elf-gcc", "x86_64-linux-gnu-gcc"]
    };
    candidates.iter().any(|c| {
        std::process::Command::new(c)
            .arg("--version")
            .output()
            .is_ok()
    })
}
