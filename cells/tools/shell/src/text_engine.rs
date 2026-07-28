//! Re-export of the pure argv/pattern/record engine.
//!
//! The implementation lives in `libs/text-engine` so it can be host-tested;
//! app-shell links `ostd`, whose `#[panic_handler]`/`#[global_allocator]`
//! collide with the std test harness. This shim keeps the in-crate
//! `crate::text_engine::…` paths stable for the built-in command modules.

pub use text_engine::{args, records};
