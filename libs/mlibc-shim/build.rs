// mlibc-shim build.rs — inject the pre-built mlibc libc.a into the linker.
//
// The .a must be produced by running `scripts/build-mlibc.sh` in WSL2 first.
// If the file is absent, this script emits a clear error rather than silently
// falling back to posix.rs (which would cause duplicate-symbol link failures).

use std::env;
use std::path::PathBuf;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // Locate workspace root: CARGO_MANIFEST_DIR is libs/mlibc-shim/, go up twice.
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()   // libs/
        .and_then(|p| p.parent()) // workspace root
        .expect("mlibc-shim: could not find workspace root from CARGO_MANIFEST_DIR");

    // Per-arch build directory under third_party/mlibc/
    let build_dir = if arch == "aarch64" {
        workspace_root.join("third_party/mlibc/build-aarch64")
    } else {
        workspace_root.join("third_party/mlibc/build")
    };

    let lib_path = build_dir.join("libc.a");

    if !lib_path.exists() {
        eprintln!("\n\
            ╔══════════════════════════════════════════════════════════════╗\n\
            ║  mlibc-shim: libc.a not found for arch={}{}║\n\
            ║                                                              ║\n\
            ║  Build it first:                                             ║\n\
            ║    (in WSL2)  bash scripts/build-mlibc.sh                   ║\n\
            ║                                                              ║\n\
            ║  Expected path: {}  ║\n\
            ╚══════════════════════════════════════════════════════════════╝\n",
            arch,
            " ".repeat(18usize.saturating_sub(arch.len())),
            lib_path.display(),
        );
        // Fail the build explicitly
        panic!("mlibc libc.a is missing — run scripts/build-mlibc.sh in WSL2");
    }

    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=c");
    println!("cargo:rerun-if-changed={}", lib_path.display());
}
