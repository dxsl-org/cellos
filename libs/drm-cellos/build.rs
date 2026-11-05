//! Builds libdrm-lite for x86_64 Tier-2 Cells against mlibc's installed headers
//! (`third_party/mlibc/build-x86_64/sysroot/include`, from
//! `scripts/build-mlibc.sh`), as hard-float SSE2 C (rule H1):
//! - `libdrm_lite.a`: the pinned libdrm subset (`third_party/libdrm`), the
//!   Cellos enumeration backend (`c/device.c`) and `ioctl` (`c/ioctl.c`);
//! - `libdrm_cellos_fake.a`, with the `fake-provider` feature: the test-only
//!   provider (`c/fake_provider.c`).
//!
//! Dependents get the include directories as `DEP_DRM_LITE_INCLUDE` (a
//! `:`-separated list, compat headers first), set only when the archives were
//! built. Other targets, or a missing mlibc build, compile nothing.

use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    for dir in ["c", "include", "compat", "../../third_party/libdrm"] {
        println!("cargo:rerun-if-changed={}", manifest.join(dir).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        root.join("third_party/mlibc/sysdeps/vicell/include/vicell").display()
    );

    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("x86_64-") || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }
    let sysroot = root.join("third_party/mlibc/build-x86_64/sysroot/include");
    println!("cargo:rerun-if-changed={}", sysroot.display());
    let Ok(sysroot) = sysroot.canonicalize() else {
        println!(
            "cargo:warning=mlibc headers missing at {}; libdrm-lite not built (run `bash scripts/build-mlibc.sh`)",
            sysroot.display()
        );
        return;
    };
    let canon = |p: PathBuf| p.canonicalize().unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let compat = canon(manifest.join("compat"));
    let include = canon(manifest.join("include"));
    let libdrm = canon(root.join("third_party/libdrm"));
    let uapi = canon(root.join("third_party/libdrm/include/drm"));
    let vicell = canon(root.join("third_party/mlibc/sysdeps/vicell/include"));

    // Lane scripts export no-SSE CFLAGS for ordinary Tier-1 C; cc-rs would
    // append them after the SSE2 contract below and break rule H1.
    for key in ["CFLAGS", "TARGET_CFLAGS", "CFLAGS_x86_64-unknown-none", "CFLAGS_x86_64_unknown_none"] {
        env::remove_var(key);
    }
    let gcc_include = Command::new("gcc")
        .arg("-print-file-name=include")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .expect("gcc is required to build libdrm-lite for x86_64");

    // The flags are the whole contract (as cells/tests/mlibc-posix-x86): no
    // rustc code model or soft-float feature reaches this SSE2 user code. The
    // host gcc predefines __linux__, so drm.h takes its Linux uAPI branch and
    // finds compat/linux/types.h and compat/asm/ioctl.h. The vicell headers
    // come after the sysroot so their abi-bits copy never shadows it.
    let build = |name: &str, files: &[PathBuf]| {
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
            .flag("-Werror")
            .flag(format!("-I{}", compat.display()))
            .flag(format!("-I{}", include.display()))
            .flag(format!("-I{}", libdrm.display()))
            .flag(format!("-I{}", uapi.display()))
            .flag(format!("-isystem{}", sysroot.display()))
            .flag(format!("-isystem{gcc_include}"))
            .flag(format!("-idirafter{}", vicell.display()));
        cc.files(files).compile(name);
    };

    build(
        "drm_lite",
        &[libdrm.join("xf86drm.c"), manifest.join("c/device.c"), manifest.join("c/ioctl.c")],
    );
    if env::var_os("CARGO_FEATURE_FAKE_PROVIDER").is_some() {
        build("drm_cellos_fake", &[manifest.join("c/fake_provider.c")]);
    }

    let dirs = [&compat, &include, &libdrm, &uapi, &vicell];
    let joined: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    println!("cargo:include={}", joined.join(":"));
}
