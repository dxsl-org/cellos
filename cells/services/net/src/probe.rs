//! The loop's evidence lines behind one type, so the loop body carries no
//! feature gates: the NET_RX producer proof (every image), the idle IPC wake
//! oracle (`ipc-wake-oracle`) and the `[net-loop]` trace (`loop-trace`). With
//! both features off the oracle and trace methods compile to nothing.

#[cfg(feature = "loop-trace")]
use core::sync::atomic::Ordering;

use abi::syscall::events::NET_RX;
use ostd::io::println;
#[cfg(not(feature = "ipc-wake-oracle"))]
use ostd::syscall::sys_wait_completion;
#[cfg(feature = "ipc-wake-oracle")]
use ostd::syscall::{sys_wait_completion_detailed, WaitCompletionResult};

#[cfg(feature = "ipc-wake-oracle")]
use super::{
    sys_get_time, IDLE_IPC_WAKE_PROOF_CEILING_TICKS, NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS,
    SMOLTCP_MAINTENANCE_TICKS,
};
use crate::interface::VirtioNetDevice;

/// One `[net-loop]` line per this many raw timer ticks (`loop-trace` images).
///
/// It used to be ten times shorter (`200 * MTIME_TICKS_PER_MS`), which made the
/// trace print about once per loop turn — the noise an operator notices first,
/// and enough UART time to change the thing it was measuring. The tick rate is
/// per-arch, and `MTIME_TICKS_PER_MS` is only a 10 MHz assumption, so this is a
/// nominal 2 s: measured on QEMU raspi3b it lands at about one line per four
/// turns (8962 → 2245 lines over the same 900 s window, same ~8960 turns).
#[cfg(feature = "loop-trace")]
const LOOP_TRACE_INTERVAL_TICKS: u64 = 2_000 * super::MTIME_TICKS_PER_MS;

pub(super) struct Probe {
    net_rx_proof: super::net_rx_proof::NetRxProof,
    #[cfg(feature = "ipc-wake-oracle")]
    oracle: crate::idle_ipc_wake_oracle::IdleIpcWakeOracle,
    #[cfg(feature = "loop-trace")]
    turns: u64,
    #[cfg(feature = "loop-trace")]
    recv: u64,
    #[cfg(feature = "loop-trace")]
    last: u64,
}

impl Probe {
    pub(super) fn new() -> Self {
        Self {
            net_rx_proof: super::net_rx_proof::NetRxProof::default(),
            #[cfg(feature = "ipc-wake-oracle")]
            oracle: crate::idle_ipc_wake_oracle::IdleIpcWakeOracle::new(),
            #[cfg(feature = "loop-trace")]
            turns: 0,
            #[cfg(feature = "loop-trace")]
            recv: 0,
            #[cfg(feature = "loop-trace")]
            last: super::sys_get_time(),
        }
    }

    /// One loop turn. `loop-trace` image: one line every ~2 s with what the
    /// turn is spending its time on. A stopped heartbeat is the signal that the
    /// service is parked inside a driver command rather than looping.
    #[cfg_attr(not(feature = "loop-trace"), allow(unused_variables))]
    pub(super) fn turn(&mut self, device: &VirtioNetDevice) {
        #[cfg(feature = "loop-trace")]
        {
            self.turns += 1;
            let now = super::sys_get_time();
            if now.wrapping_sub(self.last) >= LOOP_TRACE_INTERVAL_TICKS {
                self.last = now;
                println(&alloc::format!(
                    "[net-loop] turns={} recv={} l2send={} l2recv={} drv_cmd={} drv_to={} rx_q={} guest_q={}",
                    self.turns,
                    self.recv,
                    crate::handlers::L2_SEND_REQUESTS.load(Ordering::Relaxed),
                    crate::handlers::L2_RECV_REQUESTS.load(Ordering::Relaxed),
                    crate::interface::DRV_COMMANDS.load(Ordering::Relaxed),
                    crate::interface::DRV_TIMEOUTS.load(Ordering::Relaxed),
                    device.rx_queue_len(),
                    device.bridge_queued(),
                ));
            }
        }
    }

    /// A message was received (attested or not).
    pub(super) fn received(&mut self) {
        #[cfg(feature = "loop-trace")]
        {
            self.recv += 1;
        }
    }

    /// A receive turn that drained no client request closes no wake observation.
    pub(super) fn ipc_miss(&mut self) {
        #[cfg(feature = "ipc-wake-oracle")]
        self.oracle.record_ipc_miss();
    }

    /// A client request drained after a recordless wait closes its observation.
    #[cfg_attr(not(feature = "ipc-wake-oracle"), allow(unused_variables))]
    pub(super) fn ipc_drain(&mut self, sender: usize) {
        #[cfg(feature = "ipc-wake-oracle")]
        self.oracle.record_ipc_drain(sys_get_time(), sender);
    }

    /// `drained` frames were pumped this turn; see [`NetRxProof`] for when a
    /// frame and a NET_RX completion together prove the IRQ→completion producer.
    ///
    /// [`NetRxProof`]: super::net_rx_proof::NetRxProof
    pub(super) fn rx_drained(&mut self, drained: usize) {
        if self.net_rx_proof.drained(drained) {
            println("[net-rx-producer] irq->completion PASS");
        }
    }

    /// Park on NET_RX for at most `wait_ticks`. A recordless return means
    /// queued IPC interrupted the wait early or the finite maintenance budget
    /// elapsed. Either way, the loop retries without claiming NET_RX producer
    /// proof.
    pub(super) fn wait_net_rx(&mut self, wait_ticks: u64) {
        #[cfg(feature = "ipc-wake-oracle")]
        let wait_started_ticks = sys_get_time();
        #[cfg(feature = "ipc-wake-oracle")]
        let completion = match sys_wait_completion_detailed(NET_RX, wait_ticks) {
            // The proof ceiling assumes the full maintenance
            // deadline: a shorter smoltcp deadline returns below it
            // with no IPC behind the wake, which would read as PASS.
            WaitCompletionResult::NoRecord
                if wait_ticks == NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS =>
            {
                self.oracle.arm(
                    wait_started_ticks,
                    sys_get_time().wrapping_sub(wait_started_ticks),
                    SMOLTCP_MAINTENANCE_TICKS,
                    IDLE_IPC_WAKE_PROOF_CEILING_TICKS,
                );
                None
            }
            WaitCompletionResult::NoRecord => {
                self.oracle.clear();
                None
            }
            WaitCompletionResult::Completion(completion) => {
                self.oracle.clear();
                Some(completion)
            }
            WaitCompletionResult::ErrorOrInvalid(_) => {
                self.oracle.clear();
                None
            }
        };
        #[cfg(not(feature = "ipc-wake-oracle"))]
        let completion = sys_wait_completion(NET_RX, wait_ticks);
        if let Some(completion) = completion {
            if completion.source == NET_RX
                && completion.result == NET_RX as i64
                && self.net_rx_proof.completion()
            {
                println("[net-rx-producer] irq->completion PASS");
            }
        }
    }
}
