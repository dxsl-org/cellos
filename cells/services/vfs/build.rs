use std::env;
use std::fs;
use std::path::PathBuf;

const ORACLE_K1_ENV: &str = "CELLOS_C2C_ORACLE_K1_FILE";
const ORACLE_K1_LEN: usize = 32;
const ORACLE_K1_OUT: &str = "c2c-oracle-cluster.key";

fn main() {
    cell_build::emit_linker_script();
    // littlefs2-sys vendors a freestanding string.c (strlen/strchr/strspn/strcspn)
    // that collides with the identical symbols in api's POSIX shim (posix.rs) —
    // the shim must keep them for Tier-1b cells that don't link littlefs.
    // muldefs keeps the first definition (the Rust shim's), which is equivalent.
    println!("cargo:rustc-link-arg=-zmuldefs");

    if env::var_os("CARGO_FEATURE_C2C_ORACLE_K1_FIXTURE").is_some() {
        install_oracle_k1();
    }
}

fn install_oracle_k1() {
    println!("cargo:rerun-if-env-changed={ORACLE_K1_ENV}");
    let source = env::var_os(ORACLE_K1_ENV).unwrap_or_else(|| {
        panic!("feature c2c-oracle-k1-fixture requires {ORACLE_K1_ENV} to name a 32-byte file")
    });
    println!(
        "cargo:rerun-if-changed={}",
        PathBuf::from(&source).display()
    );

    let key =
        fs::read(&source).unwrap_or_else(|error| panic!("failed to read {ORACLE_K1_ENV}: {error}"));
    assert_eq!(
        key.len(),
        ORACLE_K1_LEN,
        "{ORACLE_K1_ENV} must contain exactly {ORACLE_K1_LEN} bytes (found {})",
        key.len()
    );

    let destination =
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR")).join(ORACLE_K1_OUT);
    fs::write(&destination, &key)
        .unwrap_or_else(|error| panic!("failed to copy oracle K1 into OUT_DIR: {error}"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("failed to restrict oracle K1 permissions: {error}"));
    }
}
