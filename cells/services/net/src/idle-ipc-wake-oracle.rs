use crate::service_runtime::{classify_idle_ipc_wake, IdleIpcWakeClassification};
use ostd::io::println;

struct ArmedWait {
    cycle: u64,
    started_ticks: u64,
    /// Ticks the wait *itself* burned before it returned recordless.
    ///
    /// This is the only number that separates a wake from a burned quantum. A
    /// deadline return lands at whole tick periods, so it can never fall below
    /// the exclusive ceiling; a refused park (IPC already queued) or an IPC wake
    /// lands wherever the message did. The arrival of the drained IPC is
    /// recorded next to it but cannot stand in for it: the ticks between the
    /// return and the drain are the waiter's own path back to `TryRecv`, which
    /// has nothing to do with why the wait ended.
    wait_return_ticks: u64,
    maintenance_budget_ticks: u64,
    proof_ceiling_ticks: u64,
}

pub(crate) struct IdleIpcWakeOracle {
    next_cycle: u64,
    armed: Option<ArmedWait>,
}

impl IdleIpcWakeOracle {
    pub(crate) const fn new() -> Self {
        Self {
            next_cycle: 0,
            armed: None,
        }
    }

    /// Observe a recordless return that landed `wait_return_ticks` after
    /// `start_ticks` was read immediately before the wait.
    pub(crate) fn arm(
        &mut self,
        start_ticks: u64,
        wait_return_ticks: u64,
        maintenance_budget_ticks: u64,
        proof_ceiling_ticks: u64,
    ) {
        self.next_cycle = self.next_cycle.wrapping_add(1);
        if self.next_cycle == 0 {
            self.next_cycle = 1;
        }
        let cycle = self.next_cycle;
        self.armed = Some(ArmedWait {
            cycle,
            started_ticks: start_ticks,
            wait_return_ticks,
            maintenance_budget_ticks,
            proof_ceiling_ticks,
        });
        println(&alloc::format!(
            "[c2c-broker-oracle] idle_ipc_wake status=ARMED cycle={cycle} raw_ret=0 start_ticks={start_ticks} return_ticks={wait_return_ticks} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
        ));
    }

    pub(crate) fn clear(&mut self) {
        self.armed = None;
    }

    pub(crate) fn record_ipc_miss(&mut self) {
        self.armed = None;
    }

    /// Close an observation with the IPC that followed its recordless return.
    ///
    /// The drained message is what makes the return meaningful — a recordless
    /// return with nothing behind it is cancelled by `record_ipc_miss` — while
    /// the classification stays on the wait's own burn, so a slow path back to
    /// `TryRecv` cannot turn a real wake into a late drain.
    pub(crate) fn record_ipc_drain(&mut self, now_ticks: u64, sender: usize) {
        let Some(armed) = self.armed.take() else {
            return;
        };

        let elapsed_ticks = now_ticks.wrapping_sub(armed.started_ticks);
        let cycle = armed.cycle;
        let wait_return_ticks = armed.wait_return_ticks;
        let maintenance_budget_ticks = armed.maintenance_budget_ticks;
        let proof_ceiling_ticks = armed.proof_ceiling_ticks;
        match classify_idle_ipc_wake(wait_return_ticks) {
            IdleIpcWakeClassification::Pass => {
                println(&alloc::format!(
                    "[c2c-broker-oracle] idle_ipc_wake status=PASS cycle={cycle} wake=recordless raw_ret=0 return_ticks={wait_return_ticks} elapsed_ticks={elapsed_ticks} sender={sender} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
                ));
            }
            IdleIpcWakeClassification::Inconclusive => {
                println(&alloc::format!(
                    "[c2c-broker-oracle] idle_ipc_wake status=INCONCLUSIVE wake=recordless raw_ret=0 reason=late-wait cycle={cycle} return_ticks={wait_return_ticks} elapsed_ticks={elapsed_ticks} sender={sender} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
                ));
            }
        }
    }
}
