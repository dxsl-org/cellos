use std::{env, path::PathBuf, process::Command};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest path"));
    let root = manifest
        .join("../../..")
        .canonicalize()
        .expect("Cellos root");
    let build = PathBuf::from(env::var("OUT_DIR").expect("output path")).join("porting-cmake");
    // The external smoke is compiled by a per-architecture CMake toolchain. Selecting
    // it from cargo's TARGET is what keeps the aarch64 leg from linking an RV64 object
    // (`smoke.c.obj is incompatible with symbols.o`) — the toolchain file also pins
    // `CELLOS_C_TARGET` for `tools/cellos-cc`, whose default is RV64.
    let target = env::var("TARGET").expect("cargo target triple");
    let toolchain_file = match target.as_str() {
        "riscv64gc-unknown-none-elf" => "cmake/cellos-riscv64.cmake",
        "aarch64-unknown-none-softfloat" => "cmake/cellos-aarch64.cmake",
        other => panic!(
            "the external C porting smoke has no toolchain for target {other}; \
             supported targets are riscv64gc-unknown-none-elf and \
             aarch64-unknown-none-softfloat"
        ),
    };
    let toolchain = format!(
        "-DCMAKE_TOOLCHAIN_FILE={}",
        root.join(toolchain_file).display()
    );
    let status = Command::new("cmake")
        .args([
            "-S",
            root.join("tests/porting-contract/smoke").to_str().unwrap(),
            "-B",
            build.to_str().unwrap(),
            toolchain.as_str(),
        ])
        // The toolchain file's `set(ENV{...})` only reaches the configure process;
        // `cmake --build` runs the generated Makefiles as a separate process, so the
        // variable has to be in the environment of both invocations or the wrapper
        // falls back to RV64 and the arch flags no longer match the compiler.
        .env("CELLOS_C_TARGET", &target)
        .status()
        .expect("run cmake");
    assert!(status.success(), "configure external C porting smoke");
    let status = Command::new("cmake")
        .args(["--build", build.to_str().unwrap()])
        .env("CELLOS_C_TARGET", &target)
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
    // The toolchain selection lives in these files; without this, switching or
    // fixing a toolchain leaves a stale object in OUT_DIR/porting-cmake and the
    // link fails with an "incompatible" or undefined-symbol error that names the
    // wrong cause.
    println!(
        "cargo:rerun-if-changed={}",
        root.join(toolchain_file).display()
    );
    println!("cargo:rerun-if-env-changed=TARGET");
    cell_build::emit_linker_script();
}
