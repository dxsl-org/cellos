fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DEVELOPMENT_SILO_PROVIDER");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");
    println!("cargo:rerun-if-env-changed=CELLOS_PRODUCTION");
    if std::env::var_os("CARGO_FEATURE_DEVELOPMENT_SILO_PROVIDER").is_some() {
        assert!(
            std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64"),
            "development-silo-provider init selection requires AArch64 QEMU"
        );
        assert!(
            std::env::var_os("CELLOS_PRODUCTION").is_none(),
            "development-silo-provider init selection is forbidden in production"
        );
    }
    cell_build::emit_linker_script();
}
