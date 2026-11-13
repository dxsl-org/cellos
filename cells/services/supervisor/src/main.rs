//! Supervisor Cell — Tier-1 Trusted Cell for hotswap/snapshot orchestration
//! and the orderly power path (`OP_POWER`, see `power.rs`).
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

#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
#![forbid(unsafe_code)]
extern crate alloc;

#[cfg(feature = "hostile-backend-recovery")]
mod hostile_backend_recovery;
mod hotswap;
mod power;
mod protocol;
mod sender_identity;
mod snapshot;
mod transfer;

use api::caller_identity::CallerIdentity;
#[cfg(feature = "hostile-backend-recovery")]
use api::hostile_backend_recovery::KILL_REQUEST_OPCODE;
use api::services::proto::app_envelope::{HEADER_LEN, KIND_SHUTDOWN, MAGIC};
use api::services::proto::supervisor::{
    KERNEL_SENDER, OP_HOTSWAP, OP_POWER, OP_SNAPSHOT, OWNER_EXITED_LEN, OWNER_EXITED_NOTICE,
    OWNER_EXITED_RELAY, PHASE_HOTSWAP_DONE, PHASE_HOTSWAP_ERROR, PHASE_POWER, PHASE_REFUSED,
    STATUS_INVALID_REQUEST, STATUS_OK, STATUS_REJECTED_CALLER, STATUS_SERVICE_NOT_FOUND,
    STATUS_UNAVAILABLE,
};
use api::syscall::service;
use ostd::ipc::Incoming;
use ostd::service::{provider_tid, Handler, Request, ServeConfig};
use ostd::syscall::sys_resolve_cell_image;
use protocol::{encode_status, parse_power_request, HotswapRequest, SnapshotRequest};

/// Signed program names (`__ViCell_image`) of the reviewed requesters. The
/// kernel attests the sender's Cell; 441 answers with its measured image only
/// when that name is the Cell's launch identity (a kernel-resolved `/bin`
/// install), so a task merely *named* `shutdown` is refused.
const HOTSWAP_PROGRAM: &str = "hotswap";
/// The retained power worker, launched only by the shell (`/bin/shutdown`).
const SHUTDOWN_PROGRAM: &str = "shutdown";
/// Signed programs reviewed to request a snapshot. Empty: the suite shell (`cellbox`)
/// no longer qualifies (S2c-min) and no retained snapshot worker exists, so a snapshot
/// is unavailable until a `SnapshotTarget` owner is added.
const SNAPSHOT_PROGRAMS: &[&str] = &[];

/// The supervisor's requests: App SDK envelope messages from the reviewed
/// tools, raw frames from init (the owner-exit notice) and, in hostile-backend
/// images, the kill request.
struct Supervisor;

impl Handler for Supervisor {
    fn on_request(&mut self, request: Request<'_>) {
        let payload = request.payload();
        if payload.first() != Some(&MAGIC) {
            raw_message(&request, payload);
            return;
        }
        if payload.get(1) == Some(&KIND_SHUTDOWN) {
            ostd::syscall::sys_exit(0);
        }
        message(&request, &payload[HEADER_LEN..]);
    }

    /// The kernel's power-button relay is the only message without a sender
    /// Cell the supervisor acts on.
    fn on_unattributed(&mut self, sender: usize, payload: &[u8]) {
        // The kernel's shutdown (watchdog, parent death), as AppContext obeyed it.
        if sender == KERNEL_SENDER && payload.starts_with(&[MAGIC, KIND_SHUTDOWN]) {
            ostd::syscall::sys_exit(0);
        }
        if sender == KERNEL_SENDER
            && payload.starts_with(&[MAGIC, api::services::proto::app_envelope::KIND_MESSAGE])
            && payload.get(HEADER_LEN) == Some(&OP_POWER)
        {
            handle_power(None, Incoming::current(sender), &payload[HEADER_LEN..]);
        }
    }
}

fn message(request: &Request<'_>, data: &[u8]) {
    let sender_tid = request.sender();
    let caller = Some(*request.caller());
    // Captured by `serve` before the swap flow receives anything else; replies
    // never park on a requester that does not receive them.
    let reply = request.incoming();

    if data[0] == OP_HOTSWAP {
        if !sender_is_program(caller, sender_tid, HOTSWAP_PROGRAM) {
            let _ = reply.reply(&encode_status(PHASE_REFUSED, STATUS_REJECTED_CALLER));
            return;
        }
        let Some(req) = HotswapRequest::parse(data) else {
            let _ = reply.reply(&encode_status(PHASE_REFUSED, STATUS_INVALID_REQUEST));
            return;
        };

        let Some(service_id) = service_id_for_name(req.service_name()) else {
            let _ = reply.reply(&encode_status(PHASE_REFUSED, STATUS_SERVICE_NOT_FOUND));
            return;
        };

        match hotswap::hotswap(service_id, req.elf_path()) {
            Ok(_new_tid) => {
                let _ = reply.reply(&encode_status(PHASE_HOTSWAP_DONE, STATUS_OK));
            }
            Err(e) => {
                let _ = reply.reply(&encode_status(PHASE_HOTSWAP_ERROR, e as u8));
            }
        }
        return;
    }

    if data[0] == OP_SNAPSHOT {
        if !SNAPSHOT_PROGRAMS
            .iter()
            .any(|program| sender_is_program(caller, sender_tid, program))
        {
            let _ = reply.reply(&encode_status(PHASE_REFUSED, STATUS_REJECTED_CALLER));
            return;
        }
        if SnapshotRequest::parse(data).is_none() {
            let _ = reply.reply(&encode_status(PHASE_REFUSED, STATUS_INVALID_REQUEST));
            return;
        }
        let _ = reply.reply(&snapshot::run());
        return;
    }

    if data[0] == OP_POWER {
        handle_power(caller, reply, data);
    }
}

fn raw_message(request: &Request<'_>, data: &[u8]) {
    let sender_tid = request.sender();
    #[cfg(feature = "hostile-backend-recovery")]
    if data.first() == Some(&KILL_REQUEST_OPCODE) {
        hostile_backend_recovery::handle(Some(*request.caller()), sender_tid, data);
        return;
    }
    // Init's owner-exit notice; payload windows are zero-padded, so the frame
    // is the prefix and the rest must be padding.
    if sender_tid == 1
        && data[0] == OWNER_EXITED_NOTICE
        && data[OWNER_EXITED_LEN..].iter().all(|&byte| byte == 0)
    {
        if let Some(compositor) = provider_tid(service::COMPOSITOR) {
            let mut relay = [0u8; OWNER_EXITED_LEN];
            relay[0] = OWNER_EXITED_RELAY;
            relay[1..].copy_from_slice(&data[1..OWNER_EXITED_LEN]);
            let _ = ostd::ipc::post(compositor, &relay);
        }
    }
}

/// `OP_POWER` from the `/bin/shutdown` worker (reboot or poweroff) or from the
/// kernel's power-button relay (poweroff only). Only a refused operator
/// request gets a reply; success resets or powers off the machine.
fn handle_power(caller: Option<CallerIdentity>, request: Incoming, data: &[u8]) {
    use ostd::syscall::SystemPowerAction;
    let sender_tid = request.sender();
    let origin = if sender_tid == KERNEL_SENDER {
        power::Origin::PowerButton
    } else if sender_is_program(caller, sender_tid, SHUTDOWN_PROGRAM) {
        power::Origin::Operator
    } else {
        reply_power(request, STATUS_REJECTED_CALLER);
        return;
    };
    let operator = origin == power::Origin::Operator;
    let Some(action) = parse_power_request(data)
        .filter(|&action| operator || action == SystemPowerAction::PowerOff)
    else {
        if operator {
            reply_power(request, STATUS_INVALID_REQUEST);
        }
        return;
    };
    let _ = power::shut_down(action, origin);
    if operator {
        reply_power(request, STATUS_UNAVAILABLE);
    }
}

fn reply_power(request: Incoming, status: u8) {
    let _ = request.reply(&encode_status(PHASE_POWER, status));
}

/// The services a hot-swap may target: those whose provider hands its state
/// over through `serve`'s swap hooks (config), and the demo lane's target.
fn service_id_for_name(name: &str) -> Option<u16> {
    match name {
        "config" => Some(service::CONFIG),
        "hotswap-demo" => Some(service::HOTSWAP_DEMO),
        _ => None,
    }
}

/// Whether the kernel attested the sender of the current message and its Cell
/// runs the signed program `program` from its reviewed install (441).
fn sender_is_program(caller: Option<CallerIdentity>, sender_tid: usize, program: &str) -> bool {
    sender_identity::caller_runs_program(caller, sender_tid, program, sys_resolve_cell_image)
}

ostd::cell_main!(supervisor_main);

fn supervisor_main() {
    // Service registration is handled by init after it spawns us; SupervisorCap
    // is granted by the kernel loader (path-based, see kernel/src/loader.rs).
    ostd::service::serve(ServeConfig::with_heartbeat(), &mut Supervisor);
}

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
