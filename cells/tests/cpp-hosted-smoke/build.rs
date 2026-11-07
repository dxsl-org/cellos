//! Compiles `cpp/witness.cpp` with the `cpp-hosted` profile flags
//! (third_party/llvm-runtimes/README.md) against libc++'s headers from
//! `scripts/build-llvm-runtimes.sh` and mlibc's installed headers, links the
//! static libc++, libc++abi and libunwind archives, and links the Cell with a
//! GNU build-id note (owner decision H-6). mlibc's libc.a comes from
//! `libs/mlibc-shim`, as for every mlibc Cell.
//!
//! Workspace builds without the runtime (CI, other targets) still succeed: the
//! C++ part is skipped with a Cargo warning, `cfg(cpp_hosted_witness)` stays
//! unset, and the binary prints `CPP-HOSTED: NOT-BUILT` and exits 1 if run.

use std::{env, path::PathBuf};

fn main() {
    cell_build::emit_linker_script();
    println!("cargo:rustc-check-cfg=cfg(cpp_hosted_witness)");
    println!("cargo:rerun-if-changed=cpp/witness.cpp");
    println!("cargo:rerun-if-env-changed=CELLOS_LLVM_RUNTIMES_DIR");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("x86_64-") || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../../..");
    let runtime = env::var_os("CELLOS_LLVM_RUNTIMES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/llvm-runtimes/x86_64"));
    // Must be the headers of the libc.a that libs/mlibc-shim links.
    let mlibc = root.join("third_party/mlibc/build-x86_64/sysroot/include");
    let libcxx = runtime.join("include/c++/v1");
    let lib = runtime.join("lib");
    for path in [&libcxx, &mlibc, &lib.join("libc++.a")] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let (Ok(libcxx), Ok(mlibc), Ok(lib)) = (
        libcxx.canonicalize(),
        mlibc.canonicalize(),
        lib.canonicalize(),
    ) else {
        println!(
            "cargo:warning=cpp-hosted runtime or mlibc headers missing ({}); witness not built \
             (run `bash scripts/build-mlibc.sh` and `bash scripts/build-llvm-runtimes.sh x86_64`)",
            runtime.display()
        );
        return;
    };

    // Lane scripts export no-SSE CFLAGS/CXXFLAGS for ordinary Tier-1 C; cc-rs
    // would append them after the SSE2 contract below and break rule H1.
    for key in [
        "CFLAGS",
        "CXXFLAGS",
        "TARGET_CFLAGS",
        "TARGET_CXXFLAGS",
        "CXXFLAGS_x86_64-unknown-none",
        "CXXFLAGS_x86_64_unknown_none",
    ] {
        env::remove_var(key);
    }

    // The flags below are the whole `cpp-hosted` contract: clang++, the SSE2
    // Tier-2 row of tools/cellos-cc (rule H1), hosted C++17 with exceptions
    // and RTTI, libc++ headers before mlibc's, no host /usr/include.
    cc::Build::new()
        .cpp(true)
        .compiler("clang++")
        .cpp_link_stdlib(None)
        .no_default_flags(true)
        .inherit_rustflags(false)
        .flag("--target=x86_64-unknown-none-elf")
        .flag("-std=c++17")
        .flag("-O2")
        .flag("-msse2")
        .flag("-mfpmath=sse")
        .flag("-mno-red-zone")
        .flag("-mcmodel=small")
        .flag("-fPIC")
        .flag("-fno-stack-protector")
        .flag("-fexceptions")
        .flag("-frtti")
        .flag("-ffunction-sections")
        .flag("-fdata-sections")
        .flag("-D_GNU_SOURCE")
        .flag("-nostdinc++")
        .flag("-nostdlibinc")
        .flag(format!("-isystem{}", libcxx.display()))
        .flag(format!("-isystem{}", mlibc.display()))
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror")
        .file("cpp/witness.cpp")
        .compile("cpp_hosted_witness");

    println!("cargo:rustc-link-search=native={}", lib.display());
    for archive in ["c++", "c++abi", "unwind"] {
        println!("cargo:rustc-link-lib=static={archive}");
    }
    // rustc already links bins with --gc-sections, which the runtime's
    // per-function sections rely on.
    println!("cargo:rustc-link-arg=--build-id=sha1");
    println!("cargo:rustc-cfg=cpp_hosted_witness");
}
