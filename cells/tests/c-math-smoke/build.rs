//! Builds the cell's C setjmp/longjmp probe and emits the cell linker script.
//!
//! The compiler comes from cc-rs's usual `CC_<target>` / `CC` lookup — CI sets
//! `CC_<target>` per row (.github/workflows/ci.yml), as for every C-bearing cell.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    // Freestanding: the probe uses no libc, and a bare-metal cell link cannot
    // resolve the stack-protector or libc builtin calls a hosted default emits.
    build
        .flag("-ffreestanding")
        .flag_if_supported("-fno-stack-protector");
    if target.contains("aarch64") {
        // The softfloat posix setjmp does not save d8–d15, so the C side must
        // not keep live values in FP/SIMD registers across it.
        build.flag("-mgeneral-regs-only");
    } else if target.contains("x86_64") {
        build.flag("-mno-red-zone");
    }
    build
        .warnings(true)
        .file("src/setjmp_probe.c")
        .compile("c_math_smoke_setjmp_probe");
    println!("cargo:rerun-if-changed=src/setjmp_probe.c");

    cell_build::emit_linker_script();
}
