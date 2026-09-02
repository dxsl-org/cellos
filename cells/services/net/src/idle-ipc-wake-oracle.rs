use crate::service_runtime::{classify_idle_ipc_wake, IdleIpcWakeClassification};
use ostd::io::println;

struct ArmedWait {
    cycle: u64,
    start_ticks: u64,
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

    pub(crate) fn arm(
        &mut self,
        start_ticks: u64,
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
            start_ticks,
            maintenance_budget_ticks,
            proof_ceiling_ticks,
        });
        println(&alloc::format!(
            "[c2c-broker-oracle] idle_ipc_wake status=ARMED cycle={cycle} raw_ret=0 start_ticks={start_ticks} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
        ));
    }

    pub(crate) fn clear(&mut self) {
        self.armed = None;
    }

    pub(crate) fn record_ipc_miss(&mut self) {
        self.armed = None;
    }

    pub(crate) fn record_ipc_drain(&mut self, now_ticks: u64) {
        let Some(armed) = self.armed.take() else {
            return;
        };

        let elapsed_ticks = now_ticks.wrapping_sub(armed.start_ticks);
        let cycle = armed.cycle;
        let maintenance_budget_ticks = armed.maintenance_budget_ticks;
        let proof_ceiling_ticks = armed.proof_ceiling_ticks;
        match classify_idle_ipc_wake(elapsed_ticks) {
            IdleIpcWakeClassification::Pass => {
                println(&alloc::format!(
                    "[c2c-broker-oracle] idle_ipc_wake status=PASS cycle={cycle} wake=recordless raw_ret=0 elapsed_ticks={elapsed_ticks} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
                ));
            }
            IdleIpcWakeClassification::Inconclusive => {
                println(&alloc::format!(
                    "[c2c-broker-oracle] idle_ipc_wake status=INCONCLUSIVE wake=recordless raw_ret=0 reason=late-drain cycle={cycle} elapsed_ticks={elapsed_ticks} budget_ticks={maintenance_budget_ticks} proof_ceiling_ticks={proof_ceiling_ticks}"
                ));
            }
        }
    }
}
