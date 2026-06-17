fn main() {
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld = match arch.as_str() {
        "aarch64" => "cells/apps/sys-tools/sys-tools-arm64.ld",
        "x86_64"  => "cells/apps/sys-tools/sys-tools-x86_64.ld",
        _         => "cells/apps/sys-tools/sys-tools.ld",
    };
    println!("cargo:rustc-link-arg=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
