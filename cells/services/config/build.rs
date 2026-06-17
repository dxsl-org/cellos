fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld = if arch == "x86_64" {
        "cells/services/config/config-x86_64.ld"
    } else if arch == "aarch64" {
        // aarch64 reuses the RISC-V script; OUTPUT_ARCH is advisory, not enforced by LLD.
        // A dedicated arm64 script can be added if section differences arise.
        "cells/services/config/config.ld"
    } else {
        "cells/services/config/config.ld"
    };
    println!("cargo:rustc-link-arg=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
    println!("cargo:rerun-if-changed=cells/services/config/config.ld");
}
