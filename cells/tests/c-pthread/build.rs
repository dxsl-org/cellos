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

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
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
    } else if target.contains("riscv") {
        if have("riscv64-unknown-elf-gcc") {
            build.compiler("riscv64-unknown-elf-gcc");
        } else if have("riscv-none-elf-gcc") {
            build.compiler("riscv-none-elf-gcc");
        }
        if have("riscv64-unknown-elf-ar") {
            build.archiver("riscv64-unknown-elf-ar");
        } else if have("riscv-none-elf-ar") {
            build.archiver("riscv-none-elf-ar");
        }
    } else if target.contains("aarch64") {
        if have("aarch64-linux-gnu-gcc") {
            build.compiler("aarch64-linux-gnu-gcc");
        } else if have("clang") {
            build.compiler("clang");
            build.flag("--target=aarch64-unknown-none-elf");
        }
    } else if target.contains("x86_64") {
        if have("gcc") {
            build.compiler("gcc");
        } else if have("clang") {
            build.compiler("clang");
            build.flag("--target=x86_64-unknown-none-elf");
        }
    }

    if let Some(ar) = custom_ar {
        build.archiver(ar);
    }

    if target.contains("riscv") {
        build.flag("-mabi=lp64d");
    } else if target.contains("x86_64") {
        build.flag("-mno-red-zone").flag("-mcmodel=small");
    } else if target.contains("aarch64") {
        build.flag("-mgeneral-regs-only").flag("-mcmodel=small");
    }
    build
        .warnings(true)
        .flag_if_supported("-std=gnu11")
        .include("../../../libs/port-platform/include")
        .file("../../../libs/port-platform/cellos_pthread.c")
        .file("src/witness.c")
        .compile("cellos_pthread_witness");

    cell_build::emit_linker_script();
    for path in [
        "../../../libs/port-platform/include/cellos_pthread.h",
        "../../../libs/port-platform/include/cellos_syscall.h",
        "../../../libs/port-platform/cellos_pthread.c",
        "src/witness.c",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    assert!(Path::new("src/witness.c").exists());
}
