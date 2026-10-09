//! Tier-2 provider of the Phase-02 cross-tier fixture.
//!
//! Runs in a **private root** (Tier 2 Paged Domain), launched by `init` under the
//! `tier2-rpc-entry` feature, and is capability-free: its manifest declares
//! `PROTECTION_CLASS_UNTRUSTED` and no capability bits, so admission rests on the
//! class alone. It serves exactly the requests the driver sends and then exits, so
//! the driver can witness that a descriptor naming it is refused once it is gone.
//!
//! Two things it proves by existing:
//!
//! 1. A private-root Cell can receive a copied typed request from a Tier-1 Cell and
//!    answer it — the Tier-2 side of the exchange, over the same `Send`/`Recv`
//!    surface every other Cell uses (`copy_to_user` into this root, `copy_from_user`
//!    out of it).
//! 2. A private-root Cell can call a **named Tier-1 service** through the shipped
//!    SDK: on the echo request it resolves VFS with `ServiceRef` (which since
//!    Phase-02 slice A resolves the `LookupServiceBound` binding) and performs a
//!    typed `Stat("/")`, reporting the result back inside its reply.
//!
//! It does **not** register a service: `RegisterService` is `SpawnCap`-gated and a
//! private-root Cell holds no capabilities, so the fixture is tid-addressed and the
//! driver witnesses that the service id resolves to nothing.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use api::ipc::IPC_BUF_SIZE;
use api::services::ipc::{VfsRequest, VfsResponse};
use ostd::io::println;
use ostd::service::{service, ServiceRef};
use ostd::syscall::{sys_exit, sys_recv, sys_send, SyscallResult};
use tier2_rpc_proto::{
    checksum, decode_request, encode_response, EchoResponse, METHOD_ECHO, METHOD_UNAUTHORIZED,
    UNAUTHORIZED,
};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_UNTRUSTED
);

// No capability bit is declared: `LookupService` also authorizes
// `LookupServiceBound` (allowlist bit 37), and `Send`/`Recv` are ordinary IPC.
api::declare_syscalls![Log, Exit, Send, Recv, LookupService];

ostd::cell_main!(cell_main);

/// How many requests this provider serves before exiting. The driver sends this
/// many typed requests, in this order, and nothing else can reach it.
const SERVED_BUDGET: usize = 2;

fn cell_main() {
    println("[tier2-rpc] provider-start tier2");
    let mut request_buf = [0u8; IPC_BUF_SIZE];
    let mut reply_buf = [0u8; IPC_BUF_SIZE];
    let mut served = 0usize;

    while served < SERVED_BUDGET {
        // Blocking masked receive: the driver is the only cell that sends here.
        let sender = match sys_recv(0, &mut request_buf) {
            SyscallResult::Ok(sender) => sender,
            SyscallResult::Err(_) => {
                println("[tier2-rpc] provider-recv-failed");
                sys_exit(1);
            }
        };
        let Some(request) = decode_request(&request_buf) else {
            println("[tier2-rpc] provider-undecodable-request");
            sys_exit(1);
        };
        served += 1;

        let response = match request.method {
            METHOD_ECHO => {
                // The Tier-2→Tier-1 half: a private-root Cell calling the named VFS
                // service through the shipped SDK, over copied typed IPC.
                let (vfs_stat_ok, vfs_root_is_dir) = stat_root_through_sdk();
                println(&format!(
                    "[tier2-rpc] provider-echo nonce={} len={} vfs_ok={}",
                    request.nonce,
                    request.payload.len(),
                    vfs_stat_ok
                ));
                EchoResponse {
                    method: METHOD_ECHO,
                    nonce: request.nonce,
                    payload_len: request.payload.len() as u32,
                    payload_checksum: checksum(request.payload),
                    vfs_stat_ok,
                    vfs_root_is_dir,
                    error: None,
                }
            }
            METHOD_UNAUTHORIZED => {
                // Refused without a side effect, and the refusal names itself.
                println("[tier2-rpc] provider-refused-unauthorized-method");
                EchoResponse {
                    method: METHOD_UNAUTHORIZED,
                    nonce: request.nonce,
                    payload_len: 0,
                    payload_checksum: 0,
                    vfs_stat_ok: false,
                    vfs_root_is_dir: false,
                    error: Some(UNAUTHORIZED),
                }
            }
            other => {
                println(&format!("[tier2-rpc] provider-refused-method {other}"));
                EchoResponse {
                    method: other,
                    nonce: request.nonce,
                    payload_len: 0,
                    payload_checksum: 0,
                    vfs_stat_ok: false,
                    vfs_root_is_dir: false,
                    error: Some(UNAUTHORIZED),
                }
            }
        };

        let Some(len) = encode_response(&response, &mut reply_buf) else {
            println("[tier2-rpc] provider-response-does-not-fit");
            sys_exit(1);
        };
        if !matches!(sys_send(sender, &reply_buf[..len]), SyscallResult::Ok(_)) {
            println("[tier2-rpc] provider-reply-failed");
            sys_exit(1);
        }
    }

    println(&format!("[tier2-rpc] provider-served {served}"));
    println("[tier2-rpc] provider-exiting");
    sys_exit(0)
}

/// Ask the named Tier-1 VFS service about `/` through the SDK and report whether the
/// call completed and what it said. A refusal is not fatal to the fixture: it is
/// reported in the reply, so the driver sees which half of the exchange moved.
fn stat_root_through_sdk() -> (bool, bool) {
    let mut vfs: ServiceRef<{ service::VFS }> = ServiceRef::new();
    let mut response_buffer = [0u8; IPC_BUF_SIZE];
    match vfs.call::<VfsRequest, VfsResponse>(&VfsRequest::Stat("/"), &mut response_buffer) {
        Ok(VfsResponse::Stat { is_dir, .. }) => {
            println(&format!("[tier2-rpc] provider-vfs-stat ok is_dir={is_dir}"));
            (true, is_dir)
        }
        Ok(other) => {
            println(&format!(
                "[tier2-rpc] provider-vfs-stat unexpected {other:?}"
            ));
            (true, false)
        }
        Err(error) => {
            println(&format!("[tier2-rpc] provider-vfs-stat failed {error:?}"));
            (false, false)
        }
    }
}
