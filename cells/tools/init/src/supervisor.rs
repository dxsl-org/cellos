use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_lookup_service, sys_send};
use cellos_boot_config::RestartPolicy;

use crate::service_table::{self, now_ticks, Service};

const MAX_RESTARTS_PER_WINDOW: u32 = 5;
const RESTART_WINDOW_TICKS: u64 = 1000;

pub(crate) fn run(services: &mut [Service]) -> ! {
    let mut activator = crate::activation::Activator::new();
    loop {
        restart_pending(services);
        let Some(message) = activator.next_message() else {
            ostd::task::yield_now();
            continue;
        };
        if activator.handle(message) {
            continue;
        }
        // Ordinary attested caller messages are never exit notifications.
        // Even a missing trailer is insufficient without independent liveness
        // evidence for a watched service/root/provider.
        if message.identity.is_some()
            || crate::activation::task_alive(message.sender) != Some(false)
        {
            continue;
        }
        let dead = message.sender;
        // Exit reasons retain their existing clean/crash restart semantics.
        let reason = message.reason();

        let Some(index) = services.iter().position(|service| service.tid == Some(dead)) else {
            continue;
        };
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
        // Reserve the existing crash budget once for this observed death.
        // Dependency recovery must neither discard this restart nor give a
        // crash-storm victim a new budget merely because time has passed.
        service.restart_pending = true;
        if !service_table::dependencies_available(services, index) {
            println("Init: service restart blocked — exact dependency instance unavailable.");
        }
    }
}

fn restart_pending(services: &mut [Service]) {
    // Choose the first eligible entry afresh after each attempt. A pending
    // provider is unavailable until its exact new instance is ready, so this
    // yields a stable dependency order even for forward TOML references.
    while let Some(index) = (0..services.len()).find(|&index| {
        services[index].restart_pending
            && service_table::dependencies_available(services, index)
    }) {
        let service = &mut services[index];
        // Consume before attempting: a real spawn/registration failure must
        // not become an idle-loop retry. Only a new watched death can rearm it.
        service.restart_pending = false;
        println("Init: service died — restarting...");
        if service_table::spawn(service).is_some() {
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
}

fn relay_hypervisor_exit(dead: usize) {
    let mut message = [0u8; 1 + core::mem::size_of::<usize>()];
    message[0] = 0xE1;
    message[1..].copy_from_slice(&dead.to_le_bytes());
    if let Some(supervisor) = sys_lookup_service(service::SUPERVISOR) {
        let _ = sys_send(supervisor, &message);
    }
    println("Init: hypervisor exited — scanout cleanup relayed.");
}
