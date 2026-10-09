//! Build recipe for the `cpp-freestanding` runtime profile.
//!
//! The profile is the C++ *language* subset on a bare-metal target: no C++
//! standard library, no exceptions, no RTTI, no thread-safe statics, no
//! `__cxa_atexit`. Neither cross toolchain in this environment ships C++
//! standard headers (`riscv64-unknown-elf-g++` has no libstdc++ headers and
//! `clang++` has no libc++ sysroot), so the sources use compiler builtins and
//! `cpp/cxx_support.hpp` instead of `<cstdint>`/`<type_traits>`/`<array>`.
//! `cpp/cxx_runtime.cpp` supplies the few ABI symbols the compiler still emits
//! (`operator new/delete`, `__cxa_pure_virtual`, `atexit`).
//!
//! See `docs/guides/tier1b-c-zig.md` § "C++ (freestanding subset)" and
//! `docs/specs/05-application.md` § 3.2.

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

/// Probe a candidate C++ compiler: the bare-metal cross g++ where the platform
/// has it, clang otherwise (CI installs the `-elf`/`-linux-gnu` gcc packages, so
/// the g++ counterpart may or may not be present).
fn have(compiler: &str) -> bool {
    std::process::Command::new(compiler)
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();

    let mut build = cc::Build::new();
    build
        .cpp(true)
        // The profile links no hosted C++ runtime: without this, `cc` emits a
        // `-lstdc++` directive and the bare-metal link fails on the missing
        // library (which is exactly the point — see the guide's C++ section).
        .cpp_link_stdlib(None)
        .file("cpp/engine.cpp")
        .flag("-fPIC")
        .flag("-ffreestanding")
        .flag("-fno-exceptions")
        .flag("-fno-rtti")
        .flag("-fno-threadsafe-statics")
        .flag("-fno-use-cxa-atexit")
        .flag("-Wall")
        .flag("-Wextra");

    let env_key = target.replace('-', "_");
    let custom_cxx = std::env::var(format!("CXX_{env_key}"))
        .or_else(|_| std::env::var("CXX"))
        .ok();
    let use_zig = std::env::var("CELLOS_USE_ZIG").map(|v| v == "1" || v == "true").unwrap_or(false);
    if use_zig {
        if let Some(wrapper) = repo_wrapper("cellos-zig-cxx") {
            build.compiler(wrapper);
            if target.starts_with("riscv64") {
                build.flag("-mabi=lp64d");
            } else if target.starts_with("aarch64") {
                build.flag("-mgeneral-regs-only");
            } else if target.starts_with("x86_64") {
                build.flag("-mno-red-zone").flag("-mcmodel=small");
            }
        }
    } else if let Some(cxx) = custom_cxx {
        build.compiler(cxx);
        if target.starts_with("riscv64") {
            build.flag("-mabi=lp64d");
        } else if target.starts_with("aarch64") {
            build.flag("-mgeneral-regs-only");
        } else if target.starts_with("x86_64") {
            build.flag("-mno-red-zone").flag("-mcmodel=small");
        }
    } else if target.starts_with("riscv64") {
        if have("riscv64-unknown-elf-g++") {
            build
                .compiler("riscv64-unknown-elf-g++")
                .flag("-march=rv64gc")
                .flag("-mabi=lp64d")
                .flag("-mcmodel=medany");
        } else if have("clang++") {
            build
                .compiler("clang++")
                .flag("--target=riscv64-unknown-none-elf")
                .flag("-march=rv64gc")
                .flag("-mabi=lp64d");
        } else if let Some(wrapper) = repo_wrapper("cellos-zig-cxx") {
            build.compiler(wrapper).flag("-mabi=lp64d");
        } else {
            panic!(
                "no RISC-V C++ compiler: install `g++-riscv64-unknown-elf`, clang++, or tools/cellos-zig-cxx — \
                 see docs/guides/tier1b-c-zig.md"
            );
        }
    } else if target.starts_with("aarch64") {
        // CI's aarch64 cells already compile C with the `-linux-gnu` cross gcc,
        // so C++ mirrors it; clang is the fallback for hosts without it.
        if have("aarch64-linux-gnu-g++") {
            build
                .compiler("aarch64-linux-gnu-g++")
                .flag("-mgeneral-regs-only");
        } else if have("clang++") {
            build
                .compiler("clang++")
                .flag("--target=aarch64-unknown-none-elf")
                .flag("-mgeneral-regs-only");
        } else if let Some(wrapper) = repo_wrapper("cellos-zig-cxx") {
            build.compiler(wrapper).flag("-mgeneral-regs-only");
        } else {
            panic!(
                "no AArch64 C++ compiler: install `aarch64-linux-gnu-g++`, clang++, or tools/cellos-zig-cxx — \
                 see docs/guides/tier1b-c-zig.md"
            );
        }
    } else if target.starts_with("x86_64") {
        if have("g++") {
            build
                .compiler("g++")
                .flag("-mno-red-zone")
                .flag("-mcmodel=small");
        } else if have("clang++") {
            build
                .compiler("clang++")
                .flag("--target=x86_64-unknown-none-elf")
                .flag("-mno-red-zone")
                .flag("-mcmodel=small");
        } else if let Some(wrapper) = repo_wrapper("cellos-zig-cxx") {
            build
                .compiler(wrapper)
                .flag("-mno-red-zone")
                .flag("-mcmodel=small");
        } else {
            panic!("no x86_64 C++ compiler: install g++, clang++, or tools/cellos-zig-cxx");
        }
    } else {
        // The profile's runtime layer is the POSIX shim's C++ ABI
        // (`libs/api/src/services/posix/{alloc.rs,cxxabi.rs}`).
        panic!(
            "cpp-freestanding is admitted on riscv64, aarch64, and x86_64 only (the POSIX shim's C++ \
             ABI layer does not exist for target `{target}`); see docs/specs/05-application.md § 3.2"
        );
    }

    build.compile("cpp_smoke_cpp");

    cell_build::emit_linker_script();
}
