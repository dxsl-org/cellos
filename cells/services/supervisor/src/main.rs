//! Supervisor Cell — Tier-1 Trusted Cell for hotswap/snapshot orchestration.
//!
//! Holds `SupervisorCap` (granted by the kernel loader at spawn — path-based grant
//! since all 8 manifest flag bits are occupied in v1) and `SpawnCap` (declared in
//! manifest so init sets the bit via intersection).
//!
//! Service registration (`service::SUPERVISOR`) is handled by init after spawning;
//! the Supervisor Cell does not need to self-register.
//!
//! On crash: init restarts the Supervisor Cell (never-die). Frozen target cells
//! survive the restart because `sys_freeze_cell` state persists in the kernel.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;

mod error;
#[cfg(feature = "hostile-backend-recovery")]
mod hostile_backend_recovery;
mod hotswap;
mod protocol;
mod snapshot;
mod transfer;

#[cfg(feature = "hostile-backend-recovery")]
use api::hostile_backend_recovery::KILL_REQUEST_OPCODE;
use api::syscall::service;
use ostd::app::{AppContext, AppEvent};
use ostd::syscall::{sys_get_procs, sys_lookup_service, sys_send};
use protocol::{
    encode_status, HotswapRequest, SnapshotRequest, OP_HOTSWAP, OP_SNAPSHOT,
    STATUS_INVALID_REQUEST, STATUS_REJECTED_CALLER, STATUS_SERVICE_NOT_FOUND,
};

const HOTSWAP_TASK_NAME: &str = "hotswap";
const SHELL_TASK_NAME: &str = "shell";
const PROCESS_TABLE_ROWS: usize = 64;

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // Service registration is handled by init after it spawns us.
            // SupervisorCap is granted by the kernel loader (path-based, see
            // kernel/src/loader.rs) — no self-grant needed here.
        }

        AppEvent::Message { sender_tid, data } => {
            let data: &[u8] = data.as_ref();
            if data.is_empty() {
                return;
            }

            if data[0] == OP_HOTSWAP {
                if !sender_has_exact_name(sender_tid, HOTSWAP_TASK_NAME)
                    && !sender_has_exact_name(sender_tid, "bench")
                {
                    let _ = sys_send(sender_tid, &encode_status(0, STATUS_REJECTED_CALLER));
                    return;
                }
                let Some(req) = HotswapRequest::parse(data) else {
                    let _ = sys_send(sender_tid, &encode_status(0, STATUS_INVALID_REQUEST));
                    return;
                };

                let service_id = service_id_for_name(req.service_name());
                if service_id == 0 {
                    let _ = sys_send(sender_tid, &encode_status(0, STATUS_SERVICE_NOT_FOUND));
                    return;
                }

                match hotswap::hotswap(service_id, req.elf_path()) {
                    Ok(new_tid) => {
                        let _ = sys_send(sender_tid, &encode_status(6, 0x00));
                        let _ = new_tid;
                    }
                    Err(e) => {
                        let _ = sys_send(sender_tid, &encode_status(0xFF, e.as_code()));
                    }
                }
                return;
            }

            if data[0] == OP_SNAPSHOT {
                if !sender_has_exact_name(sender_tid, SHELL_TASK_NAME) {
                    let _ = sys_send(sender_tid, &encode_status(0, STATUS_REJECTED_CALLER));
                    return;
                }
                if SnapshotRequest::parse(data).is_none() {
                    let _ = sys_send(sender_tid, &encode_status(0, STATUS_INVALID_REQUEST));
                    return;
                }
                let _ = sys_send(sender_tid, &snapshot::run());
            }
        }

        #[cfg(feature = "hostile-backend-recovery")]
        AppEvent::RawMessage { sender_tid, data } if data.first() == Some(&KILL_REQUEST_OPCODE) => {
            hostile_backend_recovery::handle(sender_tid, &data);
        }

        AppEvent::RawMessage { sender_tid, data }
            if sender_tid == 1
                && data.len() == 1 + core::mem::size_of::<usize>()
                && data[0] == 0xE1 =>
        {
            if let Some(compositor) = sys_lookup_service(service::COMPOSITOR) {
                let mut relay = [0u8; 1 + core::mem::size_of::<usize>()];
                relay[0] = 0xE2;
                relay[1..].copy_from_slice(&data[1..]);
                let _ = sys_send(compositor, &relay);
            }
        }

        AppEvent::Shutdown | AppEvent::ShutdownWith { .. } => {
            ostd::syscall::sys_exit(0);
        }
        _ => {}
    }
}

/// Map a well-known ASCII service name to its numeric `service::*` constant.
fn service_id_for_name(name: &str) -> u16 {
    match name {
        "vfs" => service::VFS,
        "net" => service::NET,
        "compositor" => service::COMPOSITOR,
        "input" => service::INPUT,
        "hotswap-demo" => service::HOTSWAP_DEMO,
        _ => 0,
    }
}

fn sender_has_exact_name(sender_tid: usize, expected_name: &str) -> bool {
    // `GetProcs` snapshots up to the kernel's current 64-row diagnostics bound,
    // which gives the supervisor a full, bounded inventory for sender attestation.
    let mut rows = [api::syscall::ProcessInfo::default(); PROCESS_TABLE_ROWS];
    let Ok(count) = sys_get_procs(&mut rows) else {
        return false;
    };
    rows.iter()
        .take(count)
        .find(|info| info.id == sender_tid)
        .and_then(process_name)
        == Some(expected_name)
}

fn process_name(info: &api::syscall::ProcessInfo) -> Option<&str> {
    let end = info
        .name
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(info.name.len());
    core::str::from_utf8(&info.name[..end]).ok()
}

ostd::run_app!(handler);

// Supervisor Cell capabilities:
// - spawn = true  → SpawnCap (for sys_spawn_from_path + sys_register_service)
// - SupervisorCap is granted by the kernel loader via path match "/bin/supervisor"
//   (not a manifest flag — v1 manifest is full; v2 requires a Law-1 bump)
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = true,
    gpio = false,
    uart = false,
    hypervisor = false
);
