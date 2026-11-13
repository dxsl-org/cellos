use abi::caller_identity::{CallerIdentity, CALLER_IDENTITY_LEN};
use abi::retirement::{reason, RetirementState, ViRetirementV1};
use abi::service_binding::ServiceBinding;
use abi::syscall::service;
use alloc::string::String;
use cellos_boot_config::RestartPolicy;
use ostd::io::println;
use ostd::service::provider_tid;
use ostd::syscall::{sys_notify_on_exit, sys_query_retirement, SyscallError, SyscallResult};
use wire::proto::supervisor::{OWNER_EXITED_LEN, OWNER_EXITED_NOTICE};

use crate::pin_gate::SafeConsole;
use crate::service_table::{self, now_ticks, task_alive, Service};

const MAX_RESTARTS_PER_WINDOW: u32 = 5;
const RESTART_WINDOW_TICKS: u64 = 1000;
/// Scheduler ticks init waits for a dead incarnation's retirement verdict to
/// leave `Pending` before it decides; a verdict still pending then is unproven.
const VERDICT_WAIT_TICKS: u64 = 200;
/// Poll backstop for a scheduler-tick clock that never advances.
const VERDICT_POLL_LIMIT: usize = 5_000_000;
/// The exit reason the kernel gives an incarnation the supervisor retired after
/// a committed hot-swap (`KillCell`, `kernel/src/cell/hotswap.rs`).
const HOTSWAP_EXIT_REASON: u64 = 0xAAAA_AAAA;

/// What init does with a service whose death passed the policy and the budget.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    /// The verdict is `Contained`, or `Quarantined` without `DMA_DEFERRED`.
    Restart(ViRetirementV1),
    /// `Quarantined` with `DMA_DEFERRED`: the kernel retains the PCI function and
    /// the record will not change this boot, so no replacement can claim it.
    DeviceUnavailable,
    /// `Unproven`, no record, refused, or still `Pending` after the bound.
    NodeRecoveryRequired,
}

/// The restart policy table, identical for every supervised service.
/// `WRITABLE_BORROW` is reported in the line but never changes the decision.
fn restart_decision(verdict: Result<ViRetirementV1, SyscallError>) -> Decision {
    let Ok(record) = verdict else {
        return Decision::NodeRecoveryRequired;
    };
    match record.retirement_state() {
        Some(RetirementState::Contained) => Decision::Restart(record),
        Some(RetirementState::Quarantined) if record.reasons & reason::DMA_DEFERRED != 0 => {
            Decision::DeviceUnavailable
        }
        Some(RetirementState::Quarantined) => Decision::Restart(record),
        Some(RetirementState::Pending | RetirementState::Unproven) | None => {
            Decision::NodeRecoveryRequired
        }
    }
}

/// The one consumer-visible line for a decision.
fn decision_line(name: &str, decision: &Decision) -> String {
    match decision {
        Decision::Restart(record) => alloc::format!(
            "Init: {name} retirement {} (reasons=0x{:x} dma={} qpages={} leaked={}) — restarting",
            if record.retirement_state() == Some(RetirementState::Contained) {
                "Contained"
            } else {
                "Quarantined"
            },
            record.reasons,
            record.dma_deferred,
            record.quarantined_pages,
            record.leaked_pages,
        ),
        Decision::DeviceUnavailable => {
            alloc::format!(
                "Init: {name} device unavailable: DMA teardown unacknowledged (BDF retained)"
            )
        }
        Decision::NodeRecoveryRequired => {
            alloc::format!("Init: {name} node recovery required: containment unproven, no restart")
        }
    }
}

/// Bounded verdict wait: poll while `Pending`, yielding between polls, until
/// `VERDICT_WAIT_TICKS` have passed. Returns the last answer, which is still a
/// `Pending` record when the bound ran out.
pub(crate) fn await_verdict(dead: usize) -> Result<ViRetirementV1, SyscallError> {
    let started = now_ticks();
    let mut polls = 0usize;
    loop {
        let verdict = sys_query_retirement(dead);
        let pending = matches!(
            verdict,
            Ok(record) if record.retirement_state() == Some(RetirementState::Pending)
        );
        if !pending
            || now_ticks().wrapping_sub(started) >= VERDICT_WAIT_TICKS
            || polls >= VERDICT_POLL_LIMIT
        {
            return verdict;
        }
        polls += 1;
        ostd::task::yield_now();
    }
}

/// One death record: the dead task and its exit reason. Init answers no
/// request (Phase 13: the activation and machine-pin endpoints are Cells), so
/// every other message is dropped. Only the kernel-written ABSENCE of a caller
/// trailer plus independently observed death makes a record: an ordinary
/// caller cannot forge an exit.
fn next_death() -> Option<(usize, u64)> {
    let mut bytes = [0u8; 64 + CALLER_IDENTITY_LEN];
    let SyscallResult::Ok(sender) = ostd::syscall::sys_recv_attested(0, &mut bytes) else {
        return None;
    };
    if sender == 0
        || sender == usize::MAX
        || CallerIdentity::from_recv_buf(&bytes).is_some()
        || task_alive(sender) != Some(false)
    {
        return None;
    }
    Some((sender, u64::from_le_bytes(bytes[..8].try_into().unwrap())))
}

/// Supervise `services` forever. `console` is the safe-mode console the
/// machine-pin Cell is told about; it is re-announced after a restart.
pub(crate) fn run(services: &mut [Service], console: Option<SafeConsole>) -> ! {
    loop {
        if restart_pending(services) {
            if let Some(console) = console.as_ref() {
                console.announce(services);
            }
        }
        let Some((dead, reason)) = next_death() else {
            ostd::task::yield_now();
            continue;
        };
        let Some(index) = services
            .iter()
            .position(|service| service.tid == Some(dead))
        else {
            continue;
        };
        // A committed hot-swap: the supervisor already published the successor,
        // so watch it instead of restarting over it.
        if let Some(successor) = services[index]
            .spec
            .service_id
            .and_then(|id| swap_successor(reason, dead, ostd::service::lookup(id)))
            .filter(|&successor| task_alive(successor) == Some(true))
        {
            adopt(&mut services[index], dead, successor);
            continue;
        }
        // Exit reasons retain their existing clean/crash restart semantics.
        if services[index].spec.path == "/bin/hypervisor" {
            relay_hypervisor_exit(dead);
        }
        let service = &mut services[index];
        service.ready = false;
        service.tid = None;
        service.restart_pending = false;
        let should_restart = match service.spec.restart {
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure => reason != 0,
            RestartPolicy::Always => true,
        };
        if !should_restart {
            println("Init: service exited cleanly — policy says no restart.");
            continue;
        }

        let now = now_ticks();
        let window_age = now.wrapping_sub(service.window_start);
        // The only place that can explain the budget's decision: how many
        // restarts it has already paid for and how far into its window it is.
        // `init_gives_up_after_a_crash_storm` failed once in CI (2026-09-26)
        // with six restarts in ~150 ticks and no give-up, and the guest log had
        // no way to say whether a window roll or a missing count caused it.
        ostd::io::print("Init: storm state — death tid=");
        ostd::io::print_usize(dead);
        ostd::io::print(" count=");
        ostd::io::print_usize(service.restart_count as usize);
        ostd::io::print(" window_age=");
        ostd::io::print_usize(window_age as usize);
        println("/1000 ticks");

        if window_age > RESTART_WINDOW_TICKS {
            service.window_start = now;
            service.restart_count = 0;
        }
        if service.restart_count >= MAX_RESTARTS_PER_WINDOW {
            println(if service.spec.path == "/bin/hypervisor" {
                "Init: hypervisor restart storm — giving up."
            } else {
                "Init: restart storm — giving up on this service (escalate)."
            });
            continue;
        }
        service.restart_count += 1;
        // The verdict gate: a replacement starts only after the dead
        // incarnation's retirement verdict allows it. A refusal keeps the
        // budget already charged and leaves `restart_pending` unarmed; only a
        // new watched death can arm it again.
        let decision = restart_decision(await_verdict(dead));
        println(&decision_line(&service.spec.name, &decision));
        if !matches!(decision, Decision::Restart(_)) {
            if service.spec.required {
                println("Init: required service unavailable — recovery failed.");
            }
            continue;
        }
        // Reserve the existing crash budget once for this observed death.
        // Dependency recovery must neither discard this restart nor give a
        // crash-storm victim a new budget merely because time has passed.
        service.restart_pending = true;
        if !service_table::dependencies_available(services, index) {
            println("Init: service restart blocked — exact dependency instance unavailable.");
        }
    }
}

/// Restart every pending service whose dependencies are available; `true`
/// when one was restarted.
fn restart_pending(services: &mut [Service]) -> bool {
    let mut restarted = false;
    // Choose the first eligible entry afresh after each attempt. A pending
    // provider is unavailable until its exact new instance is ready, so this
    // yields a stable dependency order even for forward TOML references.
    while let Some(index) = (0..services.len()).find(|&index| {
        services[index].restart_pending && service_table::dependencies_available(services, index)
    }) {
        let service = &mut services[index];
        // Consume before attempting: a real spawn/registration failure must
        // not become an idle-loop retry. Only a new watched death can rearm it.
        service.restart_pending = false;
        println("Init: service died — restarting...");
        if service_table::spawn(service).is_some() {
            restarted = true;
            println(if service.spec.path == "/bin/hypervisor" {
                "Init: hypervisor restarted."
            } else {
                "Init: service restarted."
            });
        } else {
            println(if service.spec.path == "/bin/hypervisor" {
                "Init: hypervisor restart FAILED."
            } else {
                "Init: service restart FAILED."
            });
            if service.spec.required {
                println("Init: required service unavailable — recovery failed.");
            }
        }
    }
    restarted
}

/// The provider that replaced `dead` in a committed hot-swap: the death carries
/// the swap reason and the registry names a live, different provider.
fn swap_successor(reason: u64, dead: usize, live: Option<ServiceBinding>) -> Option<usize> {
    let binding = live.filter(|binding| reason == HOTSWAP_EXIT_REASON && binding.is_live())?;
    usize::try_from(binding.tid)
        .ok()
        .filter(|&successor| successor != dead)
}

/// Take over `successor` as the service's incarnation: no restart, no new
/// registration (the supervisor's commit published it), and its death is
/// watched from now on. Its crash budget carries over.
fn adopt(service: &mut Service, dead: usize, successor: usize) {
    service.tid = Some(successor);
    service.restart_pending = false;
    service.ready = matches!(sys_notify_on_exit(successor), SyscallResult::Ok(0));
    println(&alloc::format!(
        "Init: {} hot-swapped: adopted tid {successor} (was {dead}){}",
        service.spec.name,
        if service.ready {
            ""
        } else {
            ", death unwatched"
        },
    ));
}

fn relay_hypervisor_exit(dead: usize) {
    let mut message = [0u8; OWNER_EXITED_LEN];
    message[0] = OWNER_EXITED_NOTICE;
    message[1..].copy_from_slice(&dead.to_le_bytes());
    if let Some(supervisor) = provider_tid(service::SUPERVISOR) {
        let _ = ostd::ipc::post(supervisor, &message);
    }
    println("Init: hypervisor exited — scanout cleanup relayed.");
}

#[cfg(test)]
mod restart_decision_tests {
    use super::{decision_line, restart_decision, Decision};
    use abi::retirement::{reason, RetirementState, ViRetirementV1, RETIREMENT_RECORD_VERSION};
    use ostd::syscall::SyscallError;

    fn record(state: RetirementState, reasons: u32) -> ViRetirementV1 {
        ViRetirementV1 {
            version: RETIREMENT_RECORD_VERSION,
            state: state as u16,
            reasons,
            root_tid: 42,
            dma_deferred: u32::from(reasons & reason::DMA_DEFERRED != 0),
            quarantined_pages: 3,
            leaked_pages: 0,
            ..ViRetirementV1::default()
        }
    }

    #[test]
    fn contained_restarts() {
        let contained = record(RetirementState::Contained, 0);
        assert_eq!(
            restart_decision(Ok(contained)),
            Decision::Restart(contained)
        );
        let borrowed = record(RetirementState::Contained, reason::WRITABLE_BORROW);
        assert_eq!(restart_decision(Ok(borrowed)), Decision::Restart(borrowed));
    }

    #[test]
    fn quarantined_without_deferred_dma_restarts() {
        for reasons in [
            reason::TRACKED_QUARANTINE,
            reason::TRACKED_QUARANTINE | reason::WRITABLE_BORROW,
        ] {
            let quarantined = record(RetirementState::Quarantined, reasons);
            assert_eq!(
                restart_decision(Ok(quarantined)),
                Decision::Restart(quarantined)
            );
        }
    }

    #[test]
    fn quarantined_with_deferred_dma_leaves_the_device_unavailable() {
        for reasons in [
            reason::DMA_DEFERRED,
            reason::DMA_DEFERRED | reason::TRACKED_QUARANTINE,
        ] {
            let retained = record(RetirementState::Quarantined, reasons);
            assert_eq!(restart_decision(Ok(retained)), Decision::DeviceUnavailable);
        }
    }

    #[test]
    fn unproven_pending_and_every_error_require_node_recovery() {
        let unproven = record(RetirementState::Unproven, reason::SHM_UNTRACKED);
        assert_eq!(
            restart_decision(Ok(unproven)),
            Decision::NodeRecoveryRequired
        );
        // A record still pending once the wait bound ran out.
        let pending = record(RetirementState::Pending, 0);
        assert_eq!(
            restart_decision(Ok(pending)),
            Decision::NodeRecoveryRequired
        );
        for error in [
            SyscallError::FileNotFound,
            SyscallError::PermissionDenied,
            SyscallError::Unknown,
            SyscallError::InvalidCommand,
        ] {
            assert_eq!(restart_decision(Err(error)), Decision::NodeRecoveryRequired);
        }
    }

    #[test]
    fn decision_lines_are_exact() {
        let contained = record(RetirementState::Contained, reason::WRITABLE_BORROW);
        assert_eq!(
            decision_line("ahci", &Decision::Restart(contained)),
            "Init: ahci retirement Contained (reasons=0x4 dma=0 qpages=3 leaked=0) — restarting"
        );
        let quarantined = record(RetirementState::Quarantined, reason::TRACKED_QUARANTINE);
        assert_eq!(
            decision_line("vfs", &Decision::Restart(quarantined)),
            "Init: vfs retirement Quarantined (reasons=0x10 dma=0 qpages=3 leaked=0) — restarting"
        );
        assert_eq!(
            decision_line("nvme", &Decision::DeviceUnavailable),
            "Init: nvme device unavailable: DMA teardown unacknowledged (BDF retained)"
        );
        assert_eq!(
            decision_line("config", &Decision::NodeRecoveryRequired),
            "Init: config node recovery required: containment unproven, no restart"
        );
    }
}

#[cfg(test)]
mod swap_adoption_tests {
    use super::{swap_successor, HOTSWAP_EXIT_REASON};
    use abi::service_binding::ServiceBinding;

    const LIVE_41: ServiceBinding = ServiceBinding::new(41, 7, 2);

    #[test]
    fn a_swap_death_with_a_live_new_provider_is_adopted() {
        assert_eq!(
            swap_successor(HOTSWAP_EXIT_REASON, 30, Some(LIVE_41)),
            Some(41)
        );
    }

    #[test]
    fn any_other_death_is_not_a_swap() {
        for reason in [0, 1, u64::MAX, HOTSWAP_EXIT_REASON + 1] {
            assert_eq!(swap_successor(reason, 30, Some(LIVE_41)), None);
        }
    }

    #[test]
    fn a_swap_death_without_a_different_live_provider_restarts() {
        // Still the dead tid: the commit never published a successor.
        assert_eq!(swap_successor(HOTSWAP_EXIT_REASON, 41, Some(LIVE_41)), None);
        assert_eq!(swap_successor(HOTSWAP_EXIT_REASON, 30, None), None);
        for unbound in [
            ServiceBinding::new(0, 7, 2),
            ServiceBinding::new(41, 0, 2),
            ServiceBinding::new(41, 7, 0),
        ] {
            assert_eq!(swap_successor(HOTSWAP_EXIT_REASON, 30, Some(unbound)), None);
        }
    }
}
