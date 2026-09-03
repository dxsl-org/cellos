use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{
    sys_get_time, sys_lookup_service, sys_notify_on_exit, sys_recv, sys_send, SyscallResult,
};

use crate::service_table::{self, RestartPolicy, Service};

const MAX_RESTARTS_PER_WINDOW: u32 = 5;
const RESTART_WINDOW_TICKS: u64 = 1000;

pub(crate) fn run(services: &mut [Service], mut hypervisor_tid: Option<usize>) -> ! {
    let mut buffer = [0u8; 16];
    let mut hypervisor_restarts = 0u32;
    let mut hypervisor_window_start = 0u64;
    loop {
        let dead = match sys_recv(0, &mut buffer) {
            SyscallResult::Ok(tid) => tid,
            _ => {
                ostd::task::yield_now();
                continue;
            }
        };
        #[cfg(not(feature = "hypervisor-min"))]
        let reason = u64::from_le_bytes(buffer[..8].try_into().unwrap());

        if hypervisor_tid == Some(dead) {
            relay_hypervisor_exit(dead);
            let now = sys_get_time();
            if now.wrapping_sub(hypervisor_window_start) > RESTART_WINDOW_TICKS {
                hypervisor_window_start = now;
                hypervisor_restarts = 0;
            }
            if hypervisor_restarts >= MAX_RESTARTS_PER_WINDOW {
                println("Init: hypervisor restart storm — giving up.");
                hypervisor_tid = None;
                continue;
            }
            hypervisor_restarts += 1;
            hypervisor_tid = crate::boot::spawn_hypervisor();
            if hypervisor_tid.is_some() {
                println("Init: hypervisor restarted.");
            } else {
                println("Init: hypervisor restart FAILED.");
            }
            continue;
        }

        #[cfg(feature = "development-silo-provider")]
        if services
            .iter()
            .any(|candidate| candidate.tid == Some(dead) && candidate.path == "/bin/kms")
        {
            let silo_ready = services
                .iter()
                .find(|candidate| candidate.path == "/bin/silo")
                .and_then(|silo| silo.tid)
                .is_some_and(|tid| service_table::wait_for_exact_registration(service::SILO, tid));
            if !silo_ready {
                if let Some(kms) = services
                    .iter_mut()
                    .find(|candidate| candidate.tid == Some(dead))
                {
                    kms.tid = None;
                }
                println("Init: KMS restart blocked — exact Silo instance not ready.");
                continue;
            }
        }

        let Some(service) = services
            .iter_mut()
            .find(|service| service.tid == Some(dead))
        else {
            continue;
        };
        let should_restart = match service.policy {
            RestartPolicy::Temporary => false,
            #[cfg(not(feature = "hypervisor-min"))]
            RestartPolicy::Transient => reason != 0,
            RestartPolicy::Permanent => true,
        };
        if !should_restart {
            println("Init: service exited cleanly — policy says no restart.");
            service.tid = None;
            continue;
        }

        let now = sys_get_time();
        if now.wrapping_sub(service.window_start) > RESTART_WINDOW_TICKS {
            service.window_start = now;
            service.restart_count = 0;
        }
        if service.restart_count >= MAX_RESTARTS_PER_WINDOW {
            println("Init: restart storm — giving up on this service (escalate).");
            service.tid = None;
            continue;
        }
        service.restart_count += 1;

        println("Init: service died — restarting...");
        if let Some(tid) = service_table::spawn(service) {
            let _ = sys_notify_on_exit(tid);
            println("Init: service restarted.");
        } else {
            println("Init: service restart FAILED.");
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
