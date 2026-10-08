//! Local service-call lifecycle witness.
//!
//! Records the two observable outcomes when a provider task dies while a caller
//! waits for its reply, using only the shipped IPC surfaces:
//!
//! * **Synchronous leg** — `ostd::ipc::service_call_typed` (the path
//!   `LocalEndpoint::call` and `ServiceRef::call` use): `sys_send` completes as a
//!   handoff, then the caller parks in a **sender-masked** `Recv`. `exit_task`
//!   deliberately does not wake a plain reply waiter that has already left
//!   `Sending` (`kernel/src/task/scheduler.rs:1279-1284`), so the caller is
//!   stranded. The caller prints `SYNC-RESULT=RETURNED` only if it is ever woken;
//!   the absence of that line *is* the finding.
//! * **Asynchronous leg** — the bounded exact-operation path
//!   (`ostd::ipc::submit`/`wait`/`take`): the kernel binds the peer as
//!   `(cell_id, cell_generation)` and `async_ipc::peer_died` transitions the
//!   operation to a terminal, so the caller observes `PEER-GONE` instead of
//!   hanging.
//!
//! This is a witness, not a fix: it changes no ABI, no kernel path and no
//! production call site. See `docs/decisions/0023-local-service-generation-binding.md`.
//!
//! Roles: `local-service-lifecycle` (orchestrator, in `bench`) spawns the two
//! probe roles `c2c-provider` and `c2c-sync-caller:<tid>` (in `bench-probe`).

use alloc::format;
use api::ipc::IPC_BUF_SIZE;
use api::task::TaskPriority;
use ostd::{
    io::println,
    ipc::{submit, take, wait, IpcTakeResult, IpcTerminal},
    syscall::{
        sys_exit, sys_force_exit, sys_lookup_service, sys_lookup_service_bound, sys_recv,
        sys_set_spawn_args, sys_spawn_pinned, SyscallResult,
    },
    task::yield_now,
};

const PROBE_PATH: &str = "/bin/bench-probe";
const PROVIDER_ROLE: &str = "c2c-provider";
const SYNC_CALLER_PREFIX: &str = "c2c-sync-caller:";

/// First byte of the provider's request. Matches the postcard encoding of `(u8,)`.
const REQ_TAG: u8 = 0x5A;
/// Yields allowed for a spawned probe to reach its parked receive.
const SETTLE_YIELDS: usize = 60;
/// Per-call reply budget; 10 ms ticks, so 200 ticks ≈ 2 s per iteration.
const ASYNC_WAIT_TICKS: u64 = 200;
const ASYNC_WAIT_ROUNDS: usize = 10;

fn fail(reason: &str) -> ! {
    println(&format!("[local-lifecycle] FAIL — {reason}"));
    sys_exit(1)
}

/// Spawn `/bin/bench-probe` with one role argument.
fn spawn_probe(role: &str) -> Result<usize, ()> {
    if !sys_set_spawn_args(role) {
        return Err(());
    }
    match sys_spawn_pinned(PROBE_PATH, TaskPriority::Normal as u8, 0) {
        SyscallResult::Ok(tid) if tid != 0 => Ok(tid),
        _ => Err(()),
    }
}

// ── bench-probe roles ─────────────────────────────────────────────────────────

/// Provider role: consume exactly one tagged request, then die **without**
/// replying, so the caller's reply wait is the only thing left outstanding.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_provider() -> ! {
    println("[local-lifecycle] provider-ready");
    let mut buf = [0u8; 32];
    loop {
        match sys_recv(0, &mut buf) {
            SyscallResult::Ok(_sender) if buf[0] == REQ_TAG => {
                println("[local-lifecycle] provider-exiting-without-reply");
                sys_exit(0);
            }
            _ => yield_now(),
        }
    }
}

/// Synchronous caller role: exercise the real SDK call path against a provider
/// that has already been told to die. Reaching the print after the call means
/// the kernel produced a bounded terminal for this shape.
#[allow(dead_code)] // Shared module: dispatched by the bench-probe binary only.
pub fn run_sync_caller(role: &str) -> ! {
    let Some(provider_tid) = role
        .strip_prefix(SYNC_CALLER_PREFIX)
        .and_then(|tid| tid.parse::<usize>().ok())
    else {
        println("[local-lifecycle] FAIL — sync caller argument");
        sys_exit(1)
    };
    println(&format!(
        "[local-lifecycle] sync-caller-start provider={provider_tid}"
    ));
    let mut send_buf = [0u8; IPC_BUF_SIZE];
    let mut recv_buf = [0u8; IPC_BUF_SIZE];
    let request = (REQ_TAG,);
    let _ = ostd::ipc::service_call_typed::<(u8,), ()>(
        provider_tid,
        &request,
        &mut send_buf,
        &mut recv_buf,
    );
    println("[local-lifecycle] SYNC-RESULT=RETURNED");
    sys_exit(0)
}

// ── orchestrator role (bench) ─────────────────────────────────────────────────

fn settle() {
    for _ in 0..SETTLE_YIELDS {
        yield_now();
    }
}

/// Leg 0: the additive `LookupServiceBound = 429` resolves a real, init-registered
/// service to a full `(tid, cell_id, generation)` binding.
///
/// This is the end-to-end reachability proof for the opcode: allowlist bit 37, the
/// kernel dispatch, the registry's recorded identity, the fixed 24-byte write into a
/// caller buffer, and `ServiceBinding::from_bytes` all have to work for the anchors
/// below to appear. A capture bug (identity stored as 0/0) makes `lookup_bound`
/// report no binding, so this leg fails loudly instead of passing on a stub.
fn binding_leg() {
    let vfs_tid = sys_lookup_service(api::syscall::service::VFS);
    let mut buf = [0u8; api::service_binding::SERVICE_BINDING_LEN];
    match sys_lookup_service_bound(api::syscall::service::VFS, &mut buf) {
        Some(binding) => {
            println(&format!(
                "[local-lifecycle] VFS-BINDING tid={} cell={} gen={} matches_lookup={}",
                binding.tid,
                binding.cell_id,
                binding.generation,
                Some(binding.tid as usize) == vfs_tid
            ));
        }
        None => println("[local-lifecycle] VFS-BINDING=none"),
    }

    // No provider: this image neither builds nor launches the AI cell, so the id
    // resolves to nothing and the bound lookup must say so rather than write a
    // record. `0` from `LookupService` and "no binding" from
    // `LookupServiceBound` are the same absence, stated two ways.
    let absent_tid = sys_lookup_service(api::syscall::service::AI);
    let absent_binding = sys_lookup_service_bound(api::syscall::service::AI, &mut buf);
    println(&format!(
        "[local-lifecycle] ABSENT-BINDING lookup={:?} bound_is_none={}",
        absent_tid,
        absent_binding.is_none()
    ));
}

/// Leg A: the synchronous caller must not be woken by the provider's death.
fn sync_leg() {
    let provider = match spawn_probe(PROVIDER_ROLE) {
        Ok(tid) => tid,
        Err(()) => fail("sync-leg provider spawn"),
    };
    settle();

    let caller = match spawn_probe(&format!("{SYNC_CALLER_PREFIX}{provider}")) {
        Ok(tid) => tid,
        Err(()) => fail("sync-leg caller spawn"),
    };
    settle();

    // Reclaim the caller so the scenario can continue to the async leg. Its own
    // `SYNC-RESULT=RETURNED` line, if any, is the observable outcome.
    if !matches!(sys_force_exit(caller), SyscallResult::Ok(_)) {
        fail("sync-leg caller reclaim");
    }
    println("[local-lifecycle] SYNC-LEG=complete");
}

/// Leg B: the bounded exact-operation path reports a terminal instead of hanging.
fn async_leg() {
    let provider = match spawn_probe(PROVIDER_ROLE) {
        Ok(tid) => tid,
        Err(()) => fail("async-leg provider spawn"),
    };
    settle();

    let token = match submit(provider, &[REQ_TAG]) {
        Ok(token) => token,
        Err(_) => fail("async-leg submit"),
    };

    let mut reply = [0u8; IPC_BUF_SIZE];
    let mut terminal = None;
    for _ in 0..ASYNC_WAIT_ROUNDS {
        if !wait(ASYNC_WAIT_TICKS) {
            continue;
        }
        match take(token, &mut reply) {
            Ok(IpcTakeResult::Terminal { status, .. }) => {
                terminal = Some(status);
                break;
            }
            Ok(IpcTakeResult::Pending) => continue,
            Err(_) => break,
        }
    }

    match terminal {
        Some(IpcTerminal::PeerGone) => {
            println("[local-lifecycle] ASYNC-TERMINAL=PEER-GONE");
        }
        Some(other) => println(&format!("[local-lifecycle] ASYNC-TERMINAL={other:?}")),
        None => println("[local-lifecycle] ASYNC-TERMINAL=NONE"),
    }
}

pub fn run() -> ! {
    println("[local-lifecycle] START");
    binding_leg();
    sync_leg();
    async_leg();
    println("[local-lifecycle] PASS");
    sys_exit(0)
}
