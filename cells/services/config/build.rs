fn main() {
    println!("cargo:rustc-link-arg=-Tcells/services/config/config.ld");
    println!("cargo:rerun-if-changed=cells/services/config/config.ld");
}
