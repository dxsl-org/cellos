// SPDX-License-Identifier: MIT
//! What a vCPU has cost so far: `run` calls, exits by cause, and time in and
//! around the guest. Plain fields of the vCPU, written only by the task inside
//! `run`; the kernel reports them when the VM is torn down.

use super::super::vmcs::*;

/// Why the guest stopped. `Msr`, `Cpuid` and `Cr` are served in the kernel and
/// the guest resumed; every other class ends the `run` call.
#[derive(Clone, Copy)]
pub enum ExitClass {
    Msr,
    Cpuid,
    Cr,
    Hlt,
    HostInterrupt,
    /// The run reached its deadline (VMX-preemption timer).
    Deadline,
    Io,
    Ept,
    Other,
}

impl ExitClass {
    const COUNT: usize = 9;

    fn of(exit_reason: u32) -> Self {
        match exit_reason {
            EXIT_REASON_MSR_READ | EXIT_REASON_MSR_WRITE => Self::Msr,
            EXIT_REASON_CPUID => Self::Cpuid,
            EXIT_REASON_CR_ACCESS => Self::Cr,
            EXIT_REASON_HLT => Self::Hlt,
            EXIT_REASON_EXTERNAL_INTERRUPT => Self::HostInterrupt,
            EXIT_REASON_VMX_PREEMPTION_TIMER => Self::Deadline,
            EXIT_REASON_IO_INSTRUCTION => Self::Io,
            EXIT_REASON_EPT_VIOLATION => Self::Ept,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct VcpuStats {
    /// `run` calls.
    pub runs: u64,
    /// Nanoseconds inside `run`, from VMCS load to clear.
    pub run_ns: u64,
    /// The part of `run_ns` between VM entry and VM exit, both transitions
    /// included. The rest of `run_ns` is VMX bookkeeping and in-kernel exits.
    pub guest_ns: u64,
    /// Nanoseconds between one `run` returning and the next starting: the
    /// VMM's own work, and any wait for a CPU.
    pub away_ns: u64,
    /// The part of `away_ns` that followed a host-interrupt exit. The VMM has
    /// little to do then, so this is mostly the vCPU waiting for its CPU.
    pub preempted_away_ns: u64,
    exits: [u64; ExitClass::COUNT],
    /// When the latest `run` returned, and whether on a host interrupt.
    left: Option<(u64, bool)>,
}

impl VcpuStats {
    pub fn exits(&self, class: ExitClass) -> u64 {
        self.exits[class as usize]
    }

    pub(super) fn count_exit(&mut self, exit_reason: u32) {
        self.exits[ExitClass::of(exit_reason) as usize] += 1;
    }

    /// Account one `run` call that lasted from `started` to `ended`.
    pub(super) fn count_run(&mut self, started: u64, ended: u64, preempted: bool) {
        if let Some((left_at, was_preempted)) = self.left {
            let away = started.saturating_sub(left_at);
            self.away_ns += away;
            if was_preempted {
                self.preempted_away_ns += away;
            }
        }
        self.runs += 1;
        self.run_ns += ended.saturating_sub(started);
        self.left = Some((ended, preempted));
    }
}
