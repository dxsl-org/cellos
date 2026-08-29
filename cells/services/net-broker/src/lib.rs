#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

// Host test reminder: the repo default target is bare-metal, so run
// `cargo test -p service-net-broker --lib --target x86_64-unknown-linux-gnu`.

pub mod bench_oracle;
pub mod c2c_deadline;
pub mod c2c_dedup;
pub mod c2c_envelope;
pub mod c2c_receive;
pub mod export_registry;
pub mod kms_dh;
pub mod local_ingress;
pub mod local_queue;
pub mod local_runtime_metrics;
mod peer_config;
pub mod reply_pump;
pub mod runtime_roles;
pub mod server_epoch;
pub mod session_pool;

#[cfg(target_os = "none")]
pub mod identity;
