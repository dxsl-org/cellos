#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

// Host test reminder: the repo default target is bare-metal, so run
// `cargo test -p service-net-broker --lib --target x86_64-unknown-linux-gnu`.

pub mod export_registry;
mod peer_config;

#[cfg(target_os = "none")]
pub mod identity;
