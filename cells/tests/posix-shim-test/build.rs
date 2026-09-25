use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest path"));
    let root = manifest
        .join("../../..")
        .canonicalize()
        .expect("Cellos root");
    let build = PathBuf::from(env::var("OUT_DIR").expect("output path")).join("porting-cmake");
    let toolchain = format!(
        "-DCMAKE_TOOLCHAIN_FILE={}",
        root.join("cmake/cellos-riscv64.cmake").display()
    );
    let status = Command::new("cmake")
        .args([
            "-S",
            root.join("tests/porting-contract/smoke").to_str().unwrap(),
            "-B",
            build.to_str().unwrap(),
            toolchain.as_str(),
        ])
        .status()
        .expect("run cmake");
    assert!(status.success(), "configure external C porting smoke");
    let status = Command::new("cmake")
        .args(["--build", build.to_str().unwrap()])
        .status()
        .expect("build CMake smoke");
    assert!(status.success(), "build external C porting smoke");
    println!("cargo:rustc-link-search=native={}", build.display());
    println!("cargo:rustc-link-lib=static=cellos_porting_smoke");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("tests/porting-contract/smoke/smoke.c").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("libs/port-platform/include/cellos_platform.h")
            .display()
    );
    cell_build::emit_linker_script();
}
