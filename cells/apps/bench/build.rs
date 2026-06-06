fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/bench/bench.ld");
    println!("cargo:rerun-if-changed=cells/apps/bench/bench.ld");
}
