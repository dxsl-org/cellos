// Smoke test: verifies the vi-build pipeline produced counter.rs at build time.
// The generated file uses viui types (Signal, etc.) which are no_std — we
// don't include!() it here because this host binary doesn't depend on viui.
// Success criterion: `cargo build` completes without error (the build.rs ran
// vi_build::compile_vi_dir which would panic on any .vi parse error).

fn main() {
    println!("vi-build pipeline: OK");
    println!("Generated file is at: $OUT_DIR/vi_generated/counter.rs");
}
