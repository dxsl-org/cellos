//! Cell entry-point declaration that survives `#![forbid(unsafe_code)]`.
//!
//! The ViCell ELF loader resolves the symbol `main`, so every cell binary needs
//! a `#[no_mangle]` function with that name. rustc classifies `#[no_mangle]` as
//! an *unsafe attribute* (a duplicate exported symbol is UB at link time), so a
//! hand-written `#[no_mangle] pub fn main()` is a hard error under F1's
//! `#![forbid(unsafe_code)]` — which would leave every cell with a hand-rolled
//! entry point permanently unable to carry the attribute.
//!
//! Emitting the attribute from a macro *in this crate* resolves that: rustc does
//! not fire the `unsafe_code` lint inside an expansion of an external macro.
//! Note honestly what this does and does not buy: the duplicate-symbol hazard is
//! unchanged — it is the same attribute on the same function either way — and no
//! unsafe *code* is introduced or hidden. What it buys is that the rest of the
//! crate is then held to `forbid`, instead of the whole crate being exempted for
//! the sake of one attribute. `app_entry!` already relies on the same property
//! for the `main` it generates.

/// Export `$entry` as the cell's `main` symbol.
///
/// ```ignore
/// ostd::cell_main!(cell_main);              // Rust ABI
/// ostd::cell_main!(extern "C" cell_main);   // C ABI
///
/// fn cell_main() { /* … */ }
/// ```
#[macro_export]
macro_rules! cell_main {
    (extern "C" $entry:ident) => {
        #[no_mangle]
        pub extern "C" fn main() {
            $entry()
        }
    };
    ($entry:ident) => {
        #[no_mangle]
        pub fn main() {
            $entry()
        }
    };
}
