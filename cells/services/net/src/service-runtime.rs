use alloc::{collections::BTreeMap, string::String};
#[cfg(not(feature = "hypervisor-bridge"))]
use api::syscall::events::NET_RX;
use core::sync::atomic::{AtomicU16, Ordering};
#[cfg(feature = "hypervisor-bridge")]
use ostd::syscall::sys_recv_attested;
#[cfg(all(not(feature = "hypervisor-bridge"), not(feature = "ipc-wake-oracle")))]
use ostd::syscall::sys_wait_completion;
#[cfg(not(feature = "hypervisor-bridge"))]
use ostd::syscall::{sys_try_recv_attested, sys_yield};
#[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
use ostd::syscall::{sys_wait_completion_detailed, WaitCompletionResult};
use ostd::{
    io::println,
    syscall::{sys_get_time, SyscallResult},
};
use smoltcp::{
    iface::{Config, Interface, SocketSet, SocketStorage},
    time::Instant,
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};

use crate::{
    dhcp::{add_dhcp_socket, poll_dhcp, DhcpState},
    handlers,
    interface::VirtioNetDevice,
    socket_table::{SocketOwner, SocketTable, MAX_SOCKETS},
    tls::socket::TlsSocketEntry,
};

const IPC_BUF_SIZE: usize = 4096;
const MAC: EthernetAddress = EthernetAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS: u64 = 10;
#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
const SCHEDULER_TICK_MS: u64 = 10;
const IPC_BURST_GRACE_YIELDS: u8 = 1;
const SMOLTCP_MAINTENANCE_INTERVAL_MS: u64 = 100;
const MTIME_TICKS_PER_MS: u64 = 10_000;
const SMOLTCP_MAINTENANCE_TICKS: u64 = SMOLTCP_MAINTENANCE_INTERVAL_MS * MTIME_TICKS_PER_MS;
// A 10-tick deadline can fire just after 9 complete tick periods when the
// submission lands immediately before a scheduler tick. The exclusive ceiling
// therefore has to be the preceding 9 periods, not the 100 ms maintenance budget.
#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
const IDLE_IPC_WAKE_PROOF_CEILING_TICKS: u64 =
    (NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS - 1) * SCHEDULER_TICK_MS * MTIME_TICKS_PER_MS;

#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdleIpcWakeClassification {
    Pass,
    Inconclusive,
}

#[cfg(any(
    test,
    all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge"))
))]
pub(crate) const fn classify_idle_ipc_wake(elapsed_ticks: u64) -> IdleIpcWakeClassification {
    if elapsed_ticks < IDLE_IPC_WAKE_PROOF_CEILING_TICKS {
        IdleIpcWakeClassification::Pass
    } else {
        IdleIpcWakeClassification::Inconclusive
    }
}
static NEXT_PORT: AtomicU16 = AtomicU16::new(49152);

pub(crate) fn next_ephemeral_port() -> u16 {
    let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    if port >= 65534 {
        NEXT_PORT.store(49152, Ordering::Relaxed);
    }
    port
}

pub(crate) fn now_instant() -> Instant {
    Instant::from_micros((sys_get_time() / 10) as i64)
}

fn consume_ipc_burst_grace(remaining: &mut u8) -> bool {
    if *remaining == 0 {
        return false;
    }
    *remaining -= 1;
    true
}

#[cfg(target_os = "none")]
pub(crate) fn run() {
    println("[net] Network Service v0.1: smoltcp + NIC Driver Cell + DHCP");

    let mut device = VirtioNetDevice::new();
    let config = Config::new(HardwareAddress::Ethernet(MAC));
    let mut iface = Interface::new(config, &mut device, now_instant());
    iface.update_ip_addrs(|addresses| {
        let _ = addresses.push(IpCidr::new(IpAddress::v4(0, 0, 0, 0), 0));
    });

    let mut socket_storage = [SocketStorage::EMPTY; MAX_SOCKETS];
    let mut sockets = SocketSet::new(&mut socket_storage[..]);
    let mut table = SocketTable::new();
    let mut tls_table: BTreeMap<u64, TlsSocketEntry> = BTreeMap::new();

    #[cfg(not(feature = "hypervisor-bridge"))]
    let dhcp_handle = add_dhcp_socket(&mut sockets);
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut dhcp_state = DhcpState::Pending;

    let mut buffer = [0u8; IPC_BUF_SIZE];
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut last_poll_ticks = sys_get_time();
    let mut local_ip = [0u8; 4];
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut net_rx_producer_proved = false;
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut pending_net_rx_proof = false;
    #[cfg(not(feature = "hypervisor-bridge"))]
    let mut ipc_burst_grace = 0;
    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
    let mut idle_ipc_wake_oracle = crate::idle_ipc_wake_oracle::IdleIpcWakeOracle::new();

    #[cfg(not(feature = "hypervisor-bridge"))]
    println("[net] Starting DHCP...");

    loop {
        ostd::syscall::sys_heartbeat(500);
        #[cfg(not(feature = "hypervisor-bridge"))]
        {
            let drained = device.pump_rx_split();
            if pending_net_rx_proof {
                if !net_rx_producer_proved && drained > 0 {
                    println("[net-rx-producer] irq->completion PASS");
                    net_rx_producer_proved = true;
                }
                pending_net_rx_proof = false;
            }

            if dhcp_state == DhcpState::Pending {
                dhcp_state = poll_dhcp(
                    dhcp_handle,
                    &mut iface,
                    &mut sockets,
                    &mut device,
                    now_instant(),
                );
                if dhcp_state == DhcpState::Acquired {
                    if let Some(IpCidr::Ipv4(cidr)) = iface
                        .ip_addrs()
                        .iter()
                        .find(|address| matches!(address, IpCidr::Ipv4(_)))
                    {
                        local_ip.copy_from_slice(cidr.address().as_bytes());
                        let mut message = String::from("[net] IP address: ");
                        for (index, octet) in local_ip.iter().enumerate() {
                            if index > 0 {
                                message.push('.');
                            }
                            let mut number = *octet as u32;
                            let mut digits = [0u8; 3];
                            let mut digit_index = 3;
                            loop {
                                digit_index -= 1;
                                digits[digit_index] = b'0' + (number % 10) as u8;
                                number /= 10;
                                if number == 0 {
                                    break;
                                }
                            }
                            for digit in &digits[digit_index..] {
                                message.push(*digit as char);
                            }
                        }
                        println(&message);
                    }
                }
            }

            let now = sys_get_time();
            if now.wrapping_sub(last_poll_ticks) >= SMOLTCP_MAINTENANCE_TICKS {
                iface.poll(now_instant(), &mut device, &mut sockets);
                last_poll_ticks = now;
            }
        }

        buffer.fill(0);
        #[cfg(feature = "hypervisor-bridge")]
        let receive_result = {
            // An idle blocking receiver is healthy; disable its progress deadline.
            ostd::syscall::sys_heartbeat(0);
            sys_recv_attested(0, &mut buffer)
        };
        #[cfg(not(feature = "hypervisor-bridge"))]
        let receive_result = sys_try_recv_attested(0, &mut buffer);
        match receive_result {
            SyscallResult::Ok(sender) if sender > 0 => {
                let Some(identity) = api::caller_identity::CallerIdentity::from_recv_buf(&buffer)
                else {
                    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                    idle_ipc_wake_oracle.record_ipc_miss();
                    continue;
                };
                if identity.cell_id == 0 || identity.generation == 0 {
                    #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                    idle_ipc_wake_oracle.record_ipc_miss();
                    continue;
                }
                #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                idle_ipc_wake_oracle.record_ipc_drain(sys_get_time());
                handlers::handle_request(
                    &buffer,
                    sender,
                    SocketOwner {
                        cell_id: identity.cell_id,
                        generation: identity.generation,
                    },
                    &mut iface,
                    &mut device,
                    &mut sockets,
                    &mut table,
                    &mut tls_table,
                    &local_ip,
                );
                #[cfg(not(feature = "hypervisor-bridge"))]
                {
                    ipc_burst_grace = IPC_BURST_GRACE_YIELDS;
                }
            }
            _ => {
                #[cfg(all(feature = "ipc-wake-oracle", not(feature = "hypervisor-bridge")))]
                idle_ipc_wake_oracle.record_ipc_miss();
                #[cfg(not(feature = "hypervisor-bridge"))]
                if consume_ipc_burst_grace(&mut ipc_burst_grace) {
                    // Let the caller run after a reply before parking on NET_RX;
                    // sequential IPC bursts then avoid one timer quantum per call.
                    sys_yield();
                } else {
                    #[cfg(feature = "ipc-wake-oracle")]
                    let wait_started_ticks = sys_get_time();
                    #[cfg(feature = "ipc-wake-oracle")]
                    let completion = match sys_wait_completion_detailed(
                        NET_RX,
                        NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS,
                    ) {
                        WaitCompletionResult::NoRecord => {
                            idle_ipc_wake_oracle.arm(
                                wait_started_ticks,
                                SMOLTCP_MAINTENANCE_TICKS,
                                IDLE_IPC_WAKE_PROOF_CEILING_TICKS,
                            );
                            None
                        }
                        WaitCompletionResult::Completion(completion) => {
                            idle_ipc_wake_oracle.clear();
                            Some(completion)
                        }
                        WaitCompletionResult::ErrorOrInvalid(_) => {
                            idle_ipc_wake_oracle.clear();
                            None
                        }
                    };
                    #[cfg(not(feature = "ipc-wake-oracle"))]
                    let completion =
                        sys_wait_completion(NET_RX, NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS);
                    if let Some(completion) = completion {
                        pending_net_rx_proof =
                            completion.source == NET_RX && completion.result == NET_RX as i64;
                    }
                }
                // A recordless return means queued IPC interrupted the wait early or
                // the finite maintenance budget elapsed. Either way, retry the loop
                // without claiming NET_RX producer proof.
            }
        }
    }
}

#[cfg(test)]
#[path = "service-runtime-tests.rs"]
mod tests;
