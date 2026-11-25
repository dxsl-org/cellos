// SPDX-License-Identifier: MIT
//! VMCS host-state area, kept equal to the executing CPU before every entry.
//!
//! A VM exit loads GS.base, TR (and through it the IST stacks), GDTR, IDTR
//! and CR3 from these fields. The vCPU's owner task may be scheduled on any
//! hart between `RunVcpu` calls, so values captured once at creation would
//! resume the host with another CPU's `CpuLocal` (`gs:`), TSS and IST stacks.
//! [`refresh`] therefore runs on every entry, inside the same IF=0 window as
//! the world switch, so the CPU it reads is the CPU that takes the exit; it
//! writes only when the CPU, the address space or the caller's FS moved.
//! HOST_RSP/HOST_RIP are written by the world-switch stub itself.

use types::ViResult;

use super::super::gdt;
use super::super::msr;
use super::super::vmcs::*;

const MSR_FS_BASE: u32 = 0xC000_0100;
const MSR_GS_BASE: u32 = 0xC000_0101;
const MSR_EFER: u32 = 0xC000_0080;

#[repr(C, packed)]
struct Dtr {
    limit: u16,
    base: u64,
}

/// Live CR3 of the executing CPU (PCID/flag bits included), the address space
/// the exit must return to.
///
/// # Safety
/// CPL 0.
#[inline]
pub(super) unsafe fn live_cr3() -> u64 {
    let cr3: u64;
    core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
    cr3
}

/// What the host-state area was last written from. The fields that differ
/// between entries are functions of these five values: GS.base is the CPU's
/// `CpuLocal`, so it names the CPU, and a CPU's selectors, TR, GDTR, IDTR and
/// EFER are loaded once when it is brought up.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp {
    cr0: u64,
    cr3: u64,
    cr4: u64,
    fs_base: u64,
    gs_base: u64,
}

impl Stamp {
    /// A value that differs between CPUs: the base of the CPU's `CpuLocal`.
    pub(super) fn cpu(&self) -> u64 {
        self.gs_base
    }
}

/// Make the host-state area of the current VMCS describe this CPU, with
/// `host_cr3` as the exit address space. `written` is the vCPU's record of
/// what the area holds (it survives `VMCLEAR`); when nothing it depends on
/// moved, no field is written: 17 `VMWRITE`s that each trap when CellOS itself
/// runs under a hypervisor.
///
/// # Safety
/// VMX root operation, the target VMCS current on this CPU, IF=0 until the
/// following world switch returns (no migration between refresh and entry).
pub(super) unsafe fn refresh(host_cr3: u64, written: &mut Option<Stamp>) -> ViResult<()> {
    let (cr0, cr4): (u64, u64);
    core::arch::asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack, preserves_flags));
    core::arch::asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack, preserves_flags));
    let now = Stamp {
        cr0,
        cr3: host_cr3,
        cr4,
        fs_base: msr::read(MSR_FS_BASE),
        gs_base: msr::read(MSR_GS_BASE),
    };
    if *written == Some(now) {
        return Ok(());
    }
    // A write that fails part-way must not pass for a complete one.
    *written = None;
    vmwrite(HOST_CR0, cr0)?;
    vmwrite(HOST_CR3, host_cr3)?;
    vmwrite(HOST_CR4, cr4)?;

    let (cs, ds, es, ss, tr): (u16, u16, u16, u16, u16);
    core::arch::asm!(
        "mov {cs:x}, cs",
        "mov {ds:x}, ds",
        "mov {es:x}, es",
        "mov {ss:x}, ss",
        "str {tr:x}",
        cs = out(reg) cs,
        ds = out(reg) ds,
        es = out(reg) es,
        ss = out(reg) ss,
        tr = out(reg) tr,
        options(nomem, nostack, preserves_flags),
    );
    vmwrite(HOST_CS_SELECTOR, u64::from(cs))?;
    vmwrite(HOST_DS_SELECTOR, u64::from(ds))?;
    vmwrite(HOST_ES_SELECTOR, u64::from(es))?;
    vmwrite(HOST_SS_SELECTOR, u64::from(ss))?;
    vmwrite(HOST_FS_SELECTOR, 0)?;
    vmwrite(HOST_GS_SELECTOR, 0)?;
    vmwrite(HOST_TR_SELECTOR, u64::from(tr))?;

    let mut gdtr = Dtr { limit: 0, base: 0 };
    let mut idtr = Dtr { limit: 0, base: 0 };
    core::arch::asm!("sgdt [{0}]", in(reg) &mut gdtr, options(nostack, preserves_flags));
    core::arch::asm!("sidt [{0}]", in(reg) &mut idtr, options(nostack, preserves_flags));
    vmwrite(HOST_GDTR_BASE, gdtr.base)?;
    vmwrite(HOST_IDTR_BASE, idtr.base)?;
    // Left 0, #DF/NMI/#MC IST lookups would read a TSS at linear 0.
    vmwrite(HOST_TR_BASE, gdt::tss_base())?;
    // The exit reloads both bases from here: GS is this CPU's kernel
    // `CpuLocal`, FS the calling cell's own value.
    vmwrite(HOST_FS_BASE, now.fs_base)?;
    vmwrite(HOST_GS_BASE, now.gs_base)?;
    // Loaded on exit (EXIT_LOAD_IA32_EFER); this CPU's value, like the rest.
    vmwrite(HOST_IA32_EFER, msr::read(MSR_EFER))?;
    *written = Some(now);
    Ok(())
}
