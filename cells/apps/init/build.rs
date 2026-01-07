fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/app.ld");
    println!("cargo:rerun-if-changed=cells/apps/app.ld");
}
