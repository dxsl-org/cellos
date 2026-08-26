use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FEATURE: &str = "CARGO_FEATURE_DEVELOPMENT_SILO_PROVIDER";
const TARGET: &str = "aarch64-unknown-none-softfloat";

fn main() {
    cell_build::emit_linker_script();
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={FEATURE}");
    println!("cargo:rerun-if-env-changed=LLVM_OBJCOPY");
    if env::var_os(FEATURE).is_none() {
        return;
    }
    require_development_target();
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let guest_dir = manifest_dir.join("../../guests/silo-guest");
    println!(
        "cargo:rerun-if-changed={}",
        guest_dir.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        guest_dir.join("Cargo.lock").display()
    );
    println!("cargo:rerun-if-changed={}", guest_dir.join("src").display());
    println!(
        "cargo:rerun-if-changed={}",
        guest_dir.join("aarch64-silo.ld").display()
    );
    package_guest(&guest_dir, &PathBuf::from(env::var_os("OUT_DIR").unwrap()));
}

fn require_development_target() {
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64")
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none")
    {
        panic!("development-silo-provider requires the AArch64 bare-metal QEMU target");
    }
    if env::var_os("CELLOS_PRODUCTION").is_some() {
        panic!("development-silo-provider is forbidden in production builds");
    }
}

fn package_guest(guest_dir: &Path, out_dir: &Path) {
    let target_dir = out_dir.join("guest-target");
    let status = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([
            "build",
            "--locked",
            "--release",
            "--target",
            TARGET,
            "--manifest-path",
        ])
        .arg(guest_dir.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .env("SOURCE_DATE_EPOCH", "1")
        .env("CARGO_INCREMENTAL", "0")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .expect("run deterministic silo guest build");
    assert!(status.success(), "silo guest build failed");
    let elf = target_dir.join(TARGET).join("release/silo-guest");
    let binary = out_dir.join("silo-guest.bin");
    let objcopy = env::var_os("LLVM_OBJCOPY").unwrap_or_else(|| "llvm-objcopy".into());
    let status = Command::new(objcopy)
        .args(["--strip-all", "-O", "binary"])
        .arg(&elf)
        .arg(&binary)
        .status()
        .expect("run llvm-objcopy for silo guest");
    assert!(status.success(), "silo guest objcopy failed");
    let bytes = fs::read(&binary).expect("read packaged silo guest");
    assert!(!bytes.is_empty(), "packaged silo guest is empty");
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let digest_source = format!("pub const GUEST_SHA256: [u8; 32] = {digest:?};\n");
    fs::write(out_dir.join("silo-guest-digest.rs"), digest_source)
        .expect("write silo guest integrity metadata");
}
