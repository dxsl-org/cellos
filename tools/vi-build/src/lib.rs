//! Build script helper for ViUI `.vi` DSL compilation.
//!
//! Add to your app's `Cargo.toml`:
//! ```toml
//! [build-dependencies]
//! vi-build = { path = "path/to/tools/vi-build" }
//! ```
//!
//! Then in `build.rs`:
//! ```rust,ignore
//! fn main() {
//!     vi_build::compile_vi_dir("src/ui/");
//! }
//! ```
//!
//! Include the generated code in your app:
//! ```rust,ignore
//! mod counter {
//!     include!(concat!(env!("OUT_DIR"), "/vi_generated/counter.rs"));
//! }
//! ```
//!
//! # Generated output
//!
//! Each `.vi` file in the input directory is compiled to a `.rs` file placed
//! at `$OUT_DIR/vi_generated/<stem>.rs`.  The generated file contains one Rust
//! struct per `component` block with a `build() -> (Self, RootWidget)` method.

use std::path::{Path, PathBuf};

/// Compile all `.vi` files in `input_dir` to `$OUT_DIR/vi_generated/`.
///
/// Emits `cargo:rerun-if-changed=<path>` for every `.vi` file found so that
/// cargo only reruns the build script when a DSL source changes.
///
/// Non-`.vi` files in the directory are silently ignored.
///
/// # Panics
///
/// Panics with a descriptive message if:
/// - `OUT_DIR` env var is not set (not running inside a cargo build script)
/// - `input_dir` cannot be read as a directory
/// - Any `.vi` file fails to parse or generate Rust code
pub fn compile_vi_dir(input_dir: &str) {
    let out_dir = std::env::var("OUT_DIR")
        .expect("vi-build: OUT_DIR not set — compile_vi_dir must be called from build.rs");
    let gen_dir = PathBuf::from(&out_dir).join("vi_generated");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("vi-build: cannot create output dir '{}': {}", gen_dir.display(), e));

    let entries = std::fs::read_dir(input_dir)
        .unwrap_or_else(|e| panic!("vi-build: cannot read input dir '{}': {}", input_dir, e));

    for entry in entries {
        let path = entry.expect("vi-build: directory entry I/O error").path();
        if path.extension().and_then(|e| e.to_str()) != Some("vi") {
            continue;
        }
        compile_file_to(&path, &gen_dir);
    }
}

/// Compile a single `.vi` file at `input_path`.
///
/// Output is written to `$OUT_DIR/vi_generated/<stem>.rs`.
///
/// # Panics
///
/// Same conditions as [`compile_vi_dir`].
pub fn compile_vi_file(input_path: &str) {
    let out_dir = std::env::var("OUT_DIR")
        .expect("vi-build: OUT_DIR not set — compile_vi_file must be called from build.rs");
    let gen_dir = PathBuf::from(&out_dir).join("vi_generated");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("vi-build: cannot create output dir '{}': {}", gen_dir.display(), e));
    compile_file_to(Path::new(input_path), &gen_dir);
}

// ── Internal ──────────────────────────────────────────────────────────────────

/// Compile one `.vi` file and write the result to `gen_dir/<stem>.rs`.
///
/// Emits the `cargo:rerun-if-changed` directive before reading the file so
/// that even a failing build correctly tracks the source for the next run.
fn compile_file_to(path: &Path, gen_dir: &Path) {
    // Tell cargo to rerun build.rs when this source file changes.
    println!("cargo:rerun-if-changed={}", path.display());

    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("vi-build: cannot read '{}': {}", path.display(), e));

    let rust_code = vi_compiler::compile(&source)
        .unwrap_or_else(|e| panic!("vi-build: compile error in '{}': {}", path.display(), e));

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("generated");
    let out_path = gen_dir.join(format!("{stem}.rs"));

    std::fs::write(&out_path, &rust_code)
        .unwrap_or_else(|e| panic!("vi-build: cannot write '{}': {}", out_path.display(), e));
}
