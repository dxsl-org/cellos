fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/hello-cell/hello-cell.ld");
    println!("cargo:rerun-if-changed=cells/apps/hello-cell/hello-cell.ld");
}
