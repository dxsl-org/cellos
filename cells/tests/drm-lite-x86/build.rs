//! Compiles `c/witness.c` against mlibc's installed headers and libdrm-lite's
//! include directories (`DEP_DRM_LITE_INCLUDE`, from `libs/drm-cellos`) as
//! hard-float SSE2 C (rule H1), and links the Cell with a GNU build-id note
//! (owner decision H-6).
//!
//! Without mlibc (CI, other targets) libdrm-lite is not built, the C part is
//! skipped with a Cargo warning, `cfg(drm_lite_witness)` stays unset, and the
//! binary prints `DRM-LITE: NOT-BUILT` and exits 1 if it is ever run.

use std::{env, path::PathBuf, process::Command};

fn main() {
    cell_build::emit_linker_script();
    println!("cargo:rustc-check-cfg=cfg(drm_lite_witness)");
    println!("cargo:rerun-if-changed=c/witness.c");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("x86_64-") || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    let Ok(drm_include) = env::var("DEP_DRM_LITE_INCLUDE") else {
        println!("cargo:warning=libdrm-lite not built (mlibc headers missing); witness not built");
        return;
    };
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let sysroot = manifest
        .join("../../../third_party/mlibc/build-x86_64/sysroot/include")
        .canonicalize()
        .expect("mlibc sysroot (libdrm-lite was built against it)");

    // Lane scripts export no-SSE CFLAGS for ordinary Tier-1 C; cc-rs would
    // append them after the SSE2 contract below and break rule H1.
    for key in [
        "CFLAGS",
        "TARGET_CFLAGS",
        "CFLAGS_x86_64-unknown-none",
        "CFLAGS_x86_64_unknown_none",
    ] {
        env::remove_var(key);
    }
    let gcc_include = Command::new("gcc")
        .arg("-print-file-name=include")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .expect("gcc is required to build the x86_64 libdrm-lite witness");

    // Same contract as libs/drm-cellos/build.rs; the last libdrm-lite
    // directory (vicell headers) goes after the sysroot.
    let dirs: Vec<&str> = drm_include.split(':').collect();
    let (vicell, user) = dirs.split_last().expect("DEP_DRM_LITE_INCLUDE is empty");
    let mut cc = cc::Build::new();
    cc.compiler("gcc")
        .no_default_flags(true)
        .inherit_rustflags(false)
        .flag("-std=gnu11")
        .flag("-O2")
        .flag("-ffreestanding")
        .flag("-nostdinc")
        .flag("-fPIE")
        .flag("-fno-stack-protector")
        .flag("-msse2")
        .flag("-mfpmath=sse")
        .flag("-mno-red-zone")
        .flag("-mcmodel=small")
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror");
    for dir in user {
        cc.flag(format!("-I{dir}"));
    }
    cc.flag(format!("-isystem{}", sysroot.display()))
        .flag(format!("-isystem{gcc_include}"))
        .flag(format!("-idirafter{vicell}"))
        .file("c/witness.c")
        .compile("drm_lite_witness");

    println!("cargo:rustc-link-arg=--build-id=sha1");
    println!("cargo:rustc-cfg=drm_lite_witness");
}
