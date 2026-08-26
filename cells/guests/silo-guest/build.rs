#[path = "src/layout.rs"]
mod layout;

fn main() {
    let script = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("aarch64-silo.ld");
    let source = std::fs::read_to_string(&script).expect("read Silo guest linker script");
    let load_base = format!(". = 0x{:08X};", layout::GUEST_IPA_BASE);
    let mailbox = format!("__mailbox_ipa = 0x{:08X};", layout::MAILBOX_IPA);
    assert!(source.contains(&load_base), "linker load base disagrees with shared layout");
    assert!(source.contains(&mailbox), "linker mailbox disagrees with shared layout");
    assert_eq!(
        layout::MAX_GUEST_BYTES + layout::PAGE_LEN,
        layout::GUEST_RAM_BYTES,
        "mailbox must occupy the final guest RAM page",
    );
    assert_eq!(
        layout::GUEST_RAM_BYTES,
        layout::GUEST_RAM_PAGES * layout::PAGE_LEN,
        "guest byte and page counts disagree",
    );
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed=src/layout.rs");
    println!("cargo:rustc-link-arg=-T{}", script.display());
}
