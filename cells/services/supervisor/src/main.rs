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
extern crate alloc;

mod error;
mod hotswap;
mod protocol;

use ostd::app::{AppContext, AppEvent};
use ostd::syscall::sys_send;
use protocol::{HotswapRequest, encode_status, OP_HOTSWAP};
use api::syscall::service;

fn handler(_ctx: &mut AppContext, event: AppEvent) {
    match event {
        AppEvent::Init => {
            // Service registration is handled by init after it spawns us.
            // SupervisorCap is granted by the kernel loader (path-based, see
            // kernel/src/loader.rs) — no self-grant needed here.
        }

        AppEvent::Message { sender_tid, data } => {
            let data: &[u8] = data.as_ref();
            if data.is_empty() { return; }

            match data[0] {
                OP_HOTSWAP => {
                    let Some(req) = HotswapRequest::parse(data) else {
                        let _ = sys_send(sender_tid, &encode_status(0, 0xFF));
                        return;
                    };

                    let service_id = service_id_for_name(req.service_name());
                    if service_id == 0 {
                        let _ = sys_send(sender_tid, &encode_status(0, 0xFE));
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
                }
                _ => {}
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
        "vfs"        => service::VFS,
        "net"        => service::NET,
        "compositor" => service::COMPOSITOR,
        "input"      => service::INPUT,
        _            => 0,
    }
}

ostd::run_app!(handler);

// Supervisor Cell capabilities:
// - spawn = true  → SpawnCap (for sys_spawn_from_path + sys_register_service)
// - SupervisorCap is granted by the kernel loader via path match "/bin/supervisor"
//   (not a manifest flag — v1 manifest is full; v2 requires a Law-1 bump)
api::declare_manifest!(
    block_io = false, network = false, spawn = true,
    gpio = false, uart = false, hypervisor = false
);
