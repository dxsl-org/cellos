//! Compiles `c/witness.c` against mlibc's installed headers
//! (`third_party/mlibc/build-x86_64/sysroot/include`, from `scripts/build-mlibc.sh`)
//! as hard-float SSE2 C (rule H1), and links the Cell with a GNU build-id note
//! (owner decision H-6: each mlibc Cell's own build script, not `cell-build`).
//!
//! Workspace builds without mlibc (CI, other targets) still succeed: the C part
//! is skipped with a Cargo warning, `cfg(mlibc_witness)` stays unset, and the
//! binary prints `MLIBC-POSIX: NOT-BUILT` and exits 1 if it is ever run.

use std::{env, path::PathBuf, process::Command};

fn main() {
    cell_build::emit_linker_script();
    println!("cargo:rustc-check-cfg=cfg(mlibc_witness)");
    println!("cargo:rerun-if-changed=c/witness.c");

    let target = env::var("TARGET").unwrap_or_default();
    if !target.starts_with("x86_64-") || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none") {
        return;
    }

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let sysroot = manifest.join("../../../third_party/mlibc/build-x86_64/sysroot/include");
    println!("cargo:rerun-if-changed={}", sysroot.display());
    let Ok(sysroot) = sysroot.canonicalize() else {
        println!(
            "cargo:warning=mlibc headers missing at {}; witness not built (run `bash scripts/build-mlibc.sh`)",
            sysroot.display()
        );
        return;
    };

    // Lane scripts export no-SSE CFLAGS for ordinary Tier-1 C
    // (`-mno-sse -mno-mmx`); cc-rs would append them after the SSE2 contract
    // below and break rule H1, so this build ignores the environment flags.
    for key in [
        "CFLAGS",
        "TARGET_CFLAGS",
        "CFLAGS_x86_64-unknown-none",
        "CFLAGS_x86_64_unknown_none",
    ] {
        env::remove_var(key);
    }

    // Freestanding GCC still needs its own <stddef.h>/<stdarg.h>.
    let gcc_include = Command::new("gcc")
        .arg("-print-file-name=include")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .expect("gcc is required to build the x86_64 mlibc witness");

    // The flags below are the whole contract; rustc's kernel code model and
    // soft-float target features must not leak into this SSE2 user code.
    cc::Build::new()
        .compiler("gcc")
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
        .flag(format!("-isystem{}", sysroot.display()))
        .flag(format!("-isystem{gcc_include}"))
        .file("c/witness.c")
        .compile("mlibc_posix_witness");

    println!("cargo:rustc-link-arg=--build-id=sha1");
    println!("cargo:rustc-cfg=mlibc_witness");
}
