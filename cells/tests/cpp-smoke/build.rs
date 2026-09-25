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

    if target.starts_with("riscv64") {
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
        } else {
            panic!(
                "no RISC-V C++ compiler: install `g++-riscv64-unknown-elf` (or clang) — \
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
        } else {
            panic!(
                "no AArch64 C++ compiler: install `g++-aarch64-linux-gnu` (or clang) — \
                 see docs/guides/tier1b-c-zig.md"
            );
        }
    } else {
        // The profile's runtime layer is the POSIX shim's C++ ABI
        // (`libs/api/src/services/posix/{alloc.rs,cxxabi.rs}`), and that module is
        // `#![cfg(any(riscv64, aarch64, wasm32, doc))]`. Failing here names the
        // reason instead of leaving an undefined-symbol dump behind.
        panic!(
            "cpp-freestanding is admitted on riscv64 and aarch64 only (the POSIX shim's C++ \
             ABI layer does not exist for target `{target}`); see docs/specs/05-application.md § 3.2"
        );
    }

    build.compile("cpp_smoke_cpp");

    cell_build::emit_linker_script();
}
