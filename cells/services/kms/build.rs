fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DEVELOPMENT_SILO_PROVIDER");
    println!("cargo:rerun-if-env-changed=CELLOS_PRODUCTION");
    let development_silo = std::env::var_os("CARGO_FEATURE_DEVELOPMENT_SILO_PROVIDER").is_some();
    if development_silo && std::env::var_os("CELLOS_PRODUCTION").is_some() {
        panic!("development-silo-provider is forbidden in production builds");
    }
    if development_silo
        && (std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() != Ok("aarch64")
            || std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("none"))
    {
        panic!("development-silo-provider requires the AArch64 bare-metal QEMU target");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        cell_build::emit_linker_script();
    }
}
