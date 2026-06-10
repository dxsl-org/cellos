fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/net-tools/net-tools.ld");
    println!("cargo:rerun-if-changed=cells/apps/net-tools/net-tools.ld");
}
