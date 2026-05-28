use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rustc-link-arg=-Tkernel/linker.ld");
    println!("cargo:rerun-if-changed=kernel/linker.ld");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let embedded_out = out_dir.join("embedded");
    fs::create_dir_all(&embedded_out).expect("create embedded OUT_DIR");

    let embedded_src = PathBuf::from("src/embedded");
    let cells = [
        "init", "vfs", "shell", "lua", "config", "cat", "echo", "hello", "ls",
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

fn try_strip(tool: &str, path: &PathBuf) -> bool {
    Command::new(tool)
        .arg("--strip-debug")
        .arg(path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
