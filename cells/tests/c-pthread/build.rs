use std::path::Path;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    if target.contains("riscv") {
        if std::env::var("CC_riscv64gc_unknown_none_elf").is_err() {
            build.compiler("riscv-none-elf-gcc");
        }
        build.flag("-mabi=lp64d");
        if std::env::var("AR_riscv64gc_unknown_none_elf").is_err()
            && std::env::var("AR_riscv64gc-unknown-none-elf").is_err()
            && std::env::var("TARGET_AR").is_err()
            && std::env::var("AR").is_err()
        {
            build.archiver("riscv-none-elf-ar");
        }
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
