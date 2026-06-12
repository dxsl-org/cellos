use std::env;
use std::path::PathBuf;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld_name = if arch == "aarch64" { "shell-arm64.ld" } else { "shell.ld" };

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut path = PathBuf::from(manifest_dir);
    path.pop(); // Go up to apps/
    path.push(ld_name);

    println!("cargo:rustc-link-arg=-T{}", path.display());
    println!("cargo:rerun-if-changed={}", path.display());
}
