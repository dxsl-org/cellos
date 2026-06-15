fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld = if arch == "aarch64" {
        "cells/apps/hypervisor-test/hypervisor-test-arm64.ld"
    } else {
        "cells/apps/hypervisor-test/hypervisor-test.ld"
    };
    println!("cargo:rustc-link-arg=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
