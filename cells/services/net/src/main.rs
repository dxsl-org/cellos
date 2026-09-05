#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

//! Net Service Cell.
//!
//! Drives a smoltcp TCP/IPv4 stack backed by the registered NIC Driver Cell.
//! Provides BSD-style socket IPC for consumer cells via typed postcard messages
//! (`api::ipc::NetRequest`/`NetResponse`).  Legacy TLS raw opcodes (0x30–0x32)
//! from `ostd::tls` are handled by the raw fallback in `handlers`.

#[cfg(all(
    feature = "verified-tls",
    any(
        feature = "tls-insecure",
        feature = "raw-relay",
        feature = "k1-fallback"
    )
))]
compile_error!("service-net: verified TLS excludes insecure, raw relay, and K1 fallback paths");

#[cfg(all(feature = "verified-tls", not(feature = "tls-roots-embedded")))]
compile_error!("service-net: verified TLS requires tls-roots-embedded");

extern crate alloc;

// Declares network capability; the kernel grants NetworkCap at spawn.
api::declare_manifest!(block_io = false, network = true, spawn = false);

// Narrow syscall allowlist -- kernel enforces this at dispatch (Phase 27).
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Heartbeat,
    LookupService,
    NetTx,
    NetRx,
    GetTime,
    StateStash,
    StateRestore,
    GetRandom,
    WaitCompletion,
];

mod dhcp;
mod handlers;
#[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
#[path = "idle-ipc-wake-oracle.rs"]
mod idle_ipc_wake_oracle;
mod interface;
#[path = "service-runtime.rs"]
mod service_runtime;
mod socket_state;
mod socket_table;
mod tls;
mod tls_handler;
mod tls_wire;
#[cfg(target_os = "none")]
#[no_mangle]
pub fn main() {
    service_runtime::run();
}
