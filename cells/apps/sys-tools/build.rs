fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld = if arch == "aarch64" {
        "cells/apps/sys-tools/sys-tools-arm64.ld"
    } else {
        // riscv64 and x86_64 share the same script (x86_64 linker ignores OUTPUT_ARCH(riscv))
        "cells/apps/sys-tools/sys-tools.ld"
    };
    println!("cargo:rustc-link-arg=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
