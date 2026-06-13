fn main() {
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let ld = match target_arch.as_str() {
        "x86_64" => "cells/apps/app-x86_64.ld",
        _        => "cells/apps/app.ld",
    };
    println!("cargo:rustc-link-arg=-T{ld}");
    println!("cargo:rerun-if-changed={ld}");
}
