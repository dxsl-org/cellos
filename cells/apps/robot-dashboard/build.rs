fn main() {
    println!("cargo:rustc-link-arg=-Tcells/apps/robot-dashboard/robot-dashboard.ld");
    println!("cargo:rerun-if-changed=cells/apps/robot-dashboard/robot-dashboard.ld");
}
