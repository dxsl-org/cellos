//! SSH-2.0 server daemon for Cellos.
//!
//! Listens on TCP port 22 and serves one connection at a time:
//! curve25519-sha256 key exchange with a fresh per-connection scalar, the
//! persisted Ed25519 host key (`host_key`), chacha20-poly1305, and `publickey`
//! (`ssh-ed25519`) login against `/srv/sshd/authorized_keys` (`auth`).
//! A connection that has not logged in within [`LOGIN_GRACE_MS`] is dropped so
//! one idle peer cannot hold the single slot.

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

mod auth;
mod authorized_keys;
mod base64;
mod cipher;
mod connection;
mod handshake;
mod host_key;
mod kex;
mod session;
mod shell;
mod ssh_packet;

// GetRandom: per-connection KEX secrets and the first host key.
// VfsMutate: creating /srv/sshd and the host key seed on first start.
api::declare_syscalls![
    Send,
    Recv,
    Log,
    LookupServiceBound,
    GetTime,
    GetProcs,
    GetRandom,
    VfsMutate
];

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(6 * 1024 * 1024);

/// Time from accept to `USERAUTH_SUCCESS` before the connection is dropped.
pub const LOGIN_GRACE_MS: u64 = 60_000;

/// Serial log line. Host unit tests have no kernel to log to.
pub fn log(msg: &str) {
    #[cfg(target_os = "none")]
    ostd::io::println(msg);
    #[cfg(not(target_os = "none"))]
    let _ = msg;
}

pub fn log2(prefix: &str, msg: &str) {
    #[cfg(target_os = "none")]
    {
        ostd::io::print(prefix);
        ostd::io::println(msg);
    }
    #[cfg(not(target_os = "none"))]
    let _ = (prefix, msg);
}

#[cfg(target_os = "none")]
ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
fn cell_main() {
    init_custom_heap();
    log("[sshd] Cellos SSH-2.0 daemon starting...");

    while ostd::service::provider_tid(api::syscall::service::VFS).is_none() {
        ostd::syscall::sys_yield();
    }
    let host_key = match host_key::load_or_create(kex::kernel_entropy) {
        Ok(keypair) => keypair,
        Err(why) => {
            log2("[sshd] refusing to start: ", why);
            return;
        }
    };
    log2(
        "[sshd] host key ssh-ed25519 ",
        &host_key::fingerprint(&host_key),
    );
    reactor::run(host_key);
}

#[cfg(target_os = "none")]
mod reactor;

#[cfg(not(target_os = "none"))]
fn main() {}
