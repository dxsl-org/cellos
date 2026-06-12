fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/posix-shim-test/posix-shim-test.ld");
    println!("cargo:rerun-if-changed=cells/apps/posix-shim-test/posix-shim-test.ld");
}
