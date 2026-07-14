use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Emit the Git short SHA as the snapshot invalidation key.
    // Any git commit changes this value, causing warm-boot snapshots taken before
    // that commit to be rejected (stale snapshot → cold boot).
    emit_git_sha();
    // Choose linker script based on target architecture (and board feature).
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let board_rpi3 = std::env::var("CARGO_FEATURE_BOARD_RPI3").is_ok();
    let (ld_script, rerun_path) = match target_arch.as_str() {
        // board-rpi3: VideoCore loads at 0x80000; use dedicated linker script.
        "aarch64" if board_rpi3 => ("kernel/linker-rpi3.ld", "kernel/linker-rpi3.ld"),
        "aarch64" => ("kernel/linker-aarch64.ld", "kernel/linker-aarch64.ld"),
        "x86_64" => ("kernel/linker-x86-64.ld", "kernel/linker-x86-64.ld"),
        "riscv32" => ("kernel/linker-riscv32.ld", "kernel/linker-riscv32.ld"),
        "arm" => ("kernel/linker-aarch32.ld", "kernel/linker-aarch32.ld"),
        "x86" => ("kernel/linker-x86-32.ld", "kernel/linker-x86-32.ld"),
        _ => ("kernel/linker.ld", "kernel/linker.ld"),
    };
    println!("cargo:rustc-link-arg=-T{ld_script}");
    println!("cargo:rerun-if-changed={rerun_path}");
    println!("cargo:rerun-if-changed=kernel/linker-rpi3.ld");
    println!("cargo:rerun-if-changed=kernel/linker-x86-64.ld");

    // PIE: only riscv64 (Limine KASLR). riscv32 is non-PIE (direct -kernel boot,
    // OpenSBI loads kernel at ORIGIN=0x80200000 with no relocation).
    if target_arch == "riscv64" {
        // Removed -pie and --no-dynamic-linker because they break rust-lld with static libcore.
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let embedded_out = out_dir.join("embedded");
    fs::create_dir_all(&embedded_out).expect("create embedded OUT_DIR");

    // Use arch-specific embedded directory when available, fall back to default.
    // EMBEDDED_OVERRIDE lets CI test-hooks builds point at a different directory
    // (e.g. src/embedded-test-hooks/) without touching committed source files.
    let arch_embedded = PathBuf::from(format!("src/embedded-{}", target_arch));
    println!("cargo:rerun-if-env-changed=EMBEDDED_OVERRIDE");
    let embedded_src = if let Ok(ov) = std::env::var("EMBEDDED_OVERRIDE") {
        // Build scripts run with CWD = kernel/, but callers (CI scripts,
        // run.ps1) usually pass a workspace-root-relative path like
        // "kernel/src/embedded-test-hooks" — try both. A misresolved override
        // must FAIL the build: the old silent fallback shipped kernels with an
        // empty VIFS1 stub that booted to "bootstrap not in VIFS1".
        let p = PathBuf::from(&ov);
        let from_workspace = PathBuf::from("..").join(&p);
        if p.exists() {
            p
        } else if from_workspace.exists() {
            from_workspace
        } else {
            panic!(
                "EMBEDDED_OVERRIDE={ov} does not exist (checked relative to \
                 kernel/ and the workspace root)"
            );
        }
    } else if arch_embedded.exists() {
        arch_embedded
    } else {
        PathBuf::from("src/embedded")
    };
    // Only two artifacts are actually embedded: `init` (kernel/src/main.rs
    // INIT_ELF) and `kernel_fs.img` (ramdisk.rs VIFS1 — bootstrap cells only,
    // G2 kernel-shrink). Everything else ships in the disk cell-store.
    let cells = ["init", "kernel_fs.img"];

    for cell in &cells {
        let src = embedded_src.join(cell);
        if !src.exists() {
            // kernel_fs.img is a build artifact (gitignored: 4-36 MB) that
            // gen_disk.ps1 / build-*-ci.sh assemble before a bootable build.
            // For compile-only contexts (clippy, CI lint) emit an empty stub so
            // include_bytes! in ramdisk.rs resolves — a kernel built from the
            // stub compiles but has no VIFS1, so it must never be booted.
            if *cell == "kernel_fs.img" {
                fs::write(embedded_out.join(cell), []).expect("write kernel_fs.img stub");
                println!(
                    "cargo:warning={} missing — embedded an EMPTY VIFS1 stub. \
                     This kernel is compile-only; assemble the real image \
                     (gen_disk.ps1 / scripts/build-*-ci.sh) before booting.",
                    src.display()
                );
            }
            continue;
        }
        let dst = embedded_out.join(cell);
        println!("cargo:rerun-if-changed={}", src.display());

        fs::copy(&src, &dst).expect("copy embedded cell");

        // Strip debug sections to reduce kernel image size.
        // Try llvm-strip first (matches LLVM-based cross toolchain), then rust-strip,
        // then host strip. If none succeed, fall back silently — the kernel will still build.
        let stripped = try_strip("llvm-strip", &dst)
            || try_strip("rust-strip", &dst)
            || try_strip("strip", &dst);

        if !stripped {
            println!(
                "cargo:warning=Could not strip {} (no strip tool available)",
                cell
            );
        }
    }

    // Expose stripped embedded dir to source via env! macro.
    println!(
        "cargo:rustc-env=EMBEDDED_OUT_DIR={}",
        embedded_out.display()
    );
}

/// Emit the git short SHA via cargo:rustc-env for snapshot invalidation.
/// Falls back to a placeholder ("00000000") when not in a git repository.
fn emit_git_sha() {
    // Use vergen-gitcl to read the git SHA; ignore errors gracefully.
    let git = vergen_gitcl::GitclBuilder::default().sha(true).build().ok();
    let mut emitter = vergen_gitcl::Emitter::default();
    if let Some(g) = git {
        let _ = emitter.add_instructions(&g);
    }
    if emitter.emit().is_err() || std::env::var("VERGEN_GIT_SHA").is_err() {
        // Not a git repo or vergen failed — emit a stable placeholder.
        // Any non-zero placeholder is fine; it will match itself on rebuild.
        println!("cargo:rustc-env=VERGEN_GIT_SHA=00000000");
    }
}

fn try_strip(tool: &str, path: &PathBuf) -> bool {
    Command::new(tool)
        .arg("--strip-debug")
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
