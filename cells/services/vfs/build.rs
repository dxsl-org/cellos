fn main() {
    println!("cargo:rustc-link-arg=-Tcells/services/vfs/vfs.ld");
    println!("cargo:rerun-if-changed=cells/services/vfs/vfs.ld");
}
