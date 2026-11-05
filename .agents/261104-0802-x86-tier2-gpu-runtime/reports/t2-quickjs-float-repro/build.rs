//! Compile the Rust side of the boundary from the tree under test (`T2_TREE`).
//!
//! `libs/api`'s C99 math exports are compiled from their real source in both
//! states. The cell's own float boundary is `src/float_abi.rs` once T-2 is
//! fixed; before that, `lib.rs` carries the pre-fix `ffi.rs` declarations.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(t2_fixed)");
    println!("cargo:rerun-if-env-changed=T2_TREE");
    let tree = PathBuf::from(std::env::var("T2_TREE").expect("T2_TREE names the tree under test"));
    let math = tree.join("libs/api/src/services/posix/math.rs");
    let float_abi = tree.join("cells/services/ocel-quickjs/src/float_abi.rs");
    assert!(math.exists(), "{} missing", math.display());
    println!("cargo:rerun-if-changed={}", math.display());
    println!("cargo:rerun-if-changed={}", float_abi.display());

    let mut src = String::new();
    writeln!(
        src,
        "#[path = {:?}]\nmod api_math;",
        math.display().to_string()
    )
    .unwrap();
    if float_abi.exists() {
        println!("cargo:rustc-cfg=t2_fixed");
        writeln!(
            src,
            "#[path = {:?}]\nmod float_abi;",
            float_abi.display().to_string()
        )
        .unwrap();
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("tree.rs");
    std::fs::write(out, src).unwrap();
}
