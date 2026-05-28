fn main() {
    // printf.c is not yet compiled into the crate (cc build is disabled).
    // These rerun-if directives ensure the build script re-runs if C sources change.
    println!("cargo:rerun-if-changed=src/c/printf.c");
    println!("cargo:rerun-if-changed=src/c/printf.h");
}
