#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

// Host test reminder: the repo default target is bare-metal, so run
// `cargo test -p service-net-broker --lib --target x86_64-unknown-linux-gnu`.

pub mod bench_oracle;
pub mod export_registry;
pub mod local_ingress;
pub mod local_queue;
pub mod local_runtime_metrics;
mod peer_config;
pub mod reply_pump;
pub mod runtime_roles;

#[cfg(target_os = "none")]
pub mod identity;
