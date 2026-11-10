fn main() {
    cell_build::emit_linker_script();

    // `cfi_landing_pads`: this build was compiled with forward-edge landing
    // pads, so the linker emits the GNU property note the kernel loader reads
    // as the CFI opt-in (`kernel/src/loader/gnu_property.rs`). The flag must be
    // in the RUSTFLAGS of the whole invocation — with `-Z build-std` that
    // includes core/alloc/compiler_builtins, and the linker drops the property
    // if any one object lacks it — so the cell cannot set it for itself; it
    // only reports which kind of image it is. See `scripts/qemu-cfi-test.sh`.
    println!("cargo:rustc-check-cfg=cfg(cfi_landing_pads)");
    let flags = std::env::var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let opted_in = flags.split('\x1f').any(|flag| {
        // `-Z flag=value` arrives as two words, `-Zflag=value` as one.
        let flag = flag.strip_prefix("-Z").unwrap_or(flag);
        match arch.as_str() {
            "aarch64" => flag.starts_with("branch-protection=") && flag.contains("bti"),
            "x86_64" => matches!(flag, "cf-protection=branch" | "cf-protection=full"),
            _ => false,
        }
    });
    if opted_in {
        println!("cargo:rustc-cfg=cfi_landing_pads");
    }
}
