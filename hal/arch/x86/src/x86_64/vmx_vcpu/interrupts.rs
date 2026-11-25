// SPDX-License-Identifier: MIT
//! Getting interrupts into the guest (SDM Vol. 3C 25.2, 27.6).
//!
//! Two sources feed an entry. The guest's local APIC (`lapic`: its timer, its
//! self-IPIs) requests vectors under the APIC's own priority rules, and a
//! delivered one stays in service until the guest's EOI. The VMM raises the
//! lines of its device model ([`VmxVcpu::raise_irq`]); they arrive like the
//! INTR of a legacy interrupt controller and need no APIC EOI.
//!
//! The guest can take either only with RFLAGS.IF set, outside an STI/MOV-SS
//! shadow and above its task priority. Entry injection bypasses all three
//! checks, so they are made here. Before every entry the next interrupt goes
//! in if the guest can take it, and otherwise the entry asks for an
//! interrupt-window exit, which the CPU makes at the first instruction
//! boundary where it can. Nothing requested or raised is dropped, and nothing
//! waits for the guest to exit for some other reason.
//!
//! A guest halted in the guest (`VmxVcpu::halts_in_guest`) is woken the same
//! way: an entry that injects makes it active again.

use types::ViResult;

use super::super::vmcs::*;
use super::VmxVcpu;

const RFLAGS_IF: u64 = 1 << 9;
/// Interruptibility state: blocking by STI (bit 0) or by MOV SS (bit 1).
const BLOCKED_BY_SHADOW: u64 = 0x3;
/// Interruption-information fields: bit 31 marks them valid.
const INFO_VALID: u64 = 1 << 31;

/// Where an interrupt about to be injected comes from.
enum Source {
    Apic,
    Vmm,
}

impl VmxVcpu {
    /// Raise external interrupt `vector`: it waits until the guest can take it.
    pub fn raise_irq(&mut self, vector: u8) {
        self.pending.raise(vector);
    }

    /// The interrupt the guest should get next: its APIC's first (the timer),
    /// then the VMM's lowest line above the task priority.
    fn next_interrupt(&self) -> Option<(u8, Source)> {
        self.lapic
            .deliverable()
            .map(|vector| (vector, Source::Apic))
            .or_else(|| {
                self.pending
                    .lowest_above(self.lapic.task_priority_class())
                    .map(|vector| (vector, Source::Vmm))
            })
    }

    /// Before an entry: put the next interrupt in if the guest can take it
    /// now, and ask for an interrupt-window exit while any still waits.
    ///
    /// # Safety
    /// This vCPU's VMCS is current.
    pub(super) unsafe fn offer_pending(&mut self) -> ViResult<()> {
        let Some((vector, source)) = self.next_interrupt() else {
            return self.set_proc_ctl(PROC_INTERRUPT_WINDOW, false);
        };
        let takes_it = vmread(GUEST_RFLAGS)? & RFLAGS_IF != 0
            && vmread(GUEST_INTERRUPTIBILITY_INFO)? & BLOCKED_BY_SHADOW == 0
            // A fault the kernel queued for this entry goes first.
            && vmread(VM_ENTRY_INTR_INFO_FIELD)? & INFO_VALID == 0;
        if takes_it {
            vmwrite(VM_ENTRY_INTR_INFO_FIELD, u64::from(vector) | INFO_VALID)?;
            match source {
                Source::Apic => self.lapic.accept(vector),
                Source::Vmm => self.pending.clear(vector),
            }
        }
        let waiting = !takes_it || self.next_interrupt().is_some();
        self.set_proc_ctl(PROC_INTERRUPT_WINDOW, waiting)
    }

    /// Set or clear a primary processor-based control, writing the VMCS only
    /// when the bit changes.
    ///
    /// # Safety
    /// This vCPU's VMCS is current.
    pub(super) unsafe fn set_proc_ctl(&mut self, bit: u32, on: bool) -> ViResult<()> {
        let wanted = if on {
            self.proc_ctls | bit
        } else {
            self.proc_ctls & !bit
        };
        if wanted != self.proc_ctls {
            vmwrite(CPU_BASED_VM_EXEC_CONTROL, u64::from(wanted))?;
            self.proc_ctls = wanted;
        }
        Ok(())
    }
}
