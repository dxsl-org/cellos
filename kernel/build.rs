use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Emit the Git short SHA as the snapshot invalidation key.
    // Any git commit changes this value, causing warm-boot snapshots taken before
    // that commit to be rejected (stale snapshot → cold boot).
    emit_git_sha();
    // Choose linker script based on target architecture.
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let (ld_script, rerun_path) = match target_arch.as_str() {
        "aarch64" => ("kernel/linker-aarch64.ld", "kernel/linker-aarch64.ld"),
        "x86_64"  => ("kernel/linker-x86-64.ld",  "kernel/linker-x86-64.ld"),
        _         => ("kernel/linker.ld",          "kernel/linker.ld"),
    };
    println!("cargo:rustc-link-arg=-T{ld_script}");
    println!("cargo:rerun-if-changed={rerun_path}");
    println!("cargo:rerun-if-changed=kernel/linker-x86-64.ld");

    // Produce a position-independent (ET_DYN) kernel binary so Limine can apply
    // KASLR — randomizing the physical load address each boot. With ORIGIN=0x80200000
    // in the linker script and slide=0, direct QEMU -kernel boot still works
    // (all R_RISCV_RELATIVE addends are identity-transformed). Only apply to riscv64;
    // aarch64/x86_64 use their own linker scripts and may need different flags.
    if target_arch == "riscv64" {
        println!("cargo:rustc-link-arg=-pie");
        println!("cargo:rustc-link-arg=--no-dynamic-linker");
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let embedded_out = out_dir.join("embedded");
    fs::create_dir_all(&embedded_out).expect("create embedded OUT_DIR");

    let embedded_src = PathBuf::from("src/embedded");
    // kernel_fs.img is the embedded FAT32 image (~8 MB release cells).
    // The others are kept for reference but kernel_fs.img is what ramdisk.rs embeds.
    let cells = [
        "init", "vfs", "shell", "lua", "config", "cat", "echo", "hello", "ls",
        "kernel_fs.img",
    ];

    for cell in &cells {
        let src = embedded_src.join(cell);
        if !src.exists() {
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
    let git = vergen_gitcl::GitclBuilder::default()
        .sha(true)
        .build()
        .ok();
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
