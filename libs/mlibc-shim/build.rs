// mlibc-shim build.rs — inject the pre-built mlibc libc.a into the linker.
//
// libc.a must be produced BEFORE a full `cargo build`.  Two paths:
//   Windows:  pwsh scripts/setup-mlibc.ps1   (clones + builds natively)
//   WSL2/Linux: bash scripts/build-mlibc.sh
//
// If libc.a is absent this script emits a Cargo warning and returns early
// so that `cargo check` (no link step) still succeeds.  A full `cargo build`
// of any cell that links mlibc-shim will fail at the linker step with
// "cannot find -lc", which is the intended signal to run the setup script.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(mlibc_linked)");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent() // libs/
        .and_then(|p| p.parent())
        .expect("mlibc-shim: could not find workspace root from CARGO_MANIFEST_DIR");

    let build_dir = match arch.as_str() {
        "aarch64" => workspace_root.join("third_party/mlibc/build-aarch64"),
        "x86_64" => workspace_root.join("third_party/mlibc/build-x86_64"),
        _ => workspace_root.join("third_party/mlibc/build"),
    };

    let lib_path = build_dir.join("libc.a");

    if !lib_path.exists() {
        // Non-fatal: warn and skip.  `cargo check` has no link step so this is
        // safe.  `cargo build` will fail at the linker — that error is the cue
        // to run the setup script below.
        println!(
            "cargo:warning=mlibc libc.a missing for arch={arch}. \
             Run `pwsh scripts/setup-mlibc.ps1` (Windows) or \
             `bash scripts/build-mlibc.sh` (WSL2/Linux). \
             Expected: {}",
            lib_path.display()
        );
        return;
    }

    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rerun-if-changed={}", lib_path.display());
    // src/lib.rs keeps mlibc's start-up object in the link only when libc.a is
    // actually linked; without it the anchor would be an undefined symbol.
    println!("cargo:rustc-cfg=mlibc_linked");

    // libgcc.a of the compiler that built libc.a (scripts/build-mlibc.sh).
    // Listed after libc so lld resolves mlibc's helper calls here, before
    // Rust's compiler_builtins, whose riscv64 C objects use the soft-float ABI.
    let libgcc_path = build_dir.join("libgcc.a");
    if libgcc_path.exists() {
        println!("cargo:rustc-link-lib=static=gcc");
    }
    println!("cargo:rerun-if-changed={}", libgcc_path.display());
}
