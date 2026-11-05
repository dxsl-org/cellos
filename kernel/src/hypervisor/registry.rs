//! VM registry — per-owner RAII store for Stage-2 tables, guest RAM, and vCPUs.
//!
//! # Lock order
//! `VM_REGISTRY` → `FRAME_ALLOCATOR` (same order as grant reaper; never reverse).
//!
//! # Non-aarch64
//! All public functions return `Err(ViError::NotSupported)` so the compiler
//! produces a complete match for the hypervisor Syscall arms on every target.

extern crate alloc;
#[cfg(target_arch = "aarch64")]
use crate::sync::Spinlock;
#[cfg(target_arch = "aarch64")]
use alloc::{collections::BTreeMap, vec::Vec};
// reason: ViError is used only by the aarch64 branch; x86_64/other arches
// delegate to svm_registry and reference only ViResult here.
#[allow(unused_imports)]
use types::{ViError, ViResult};

#[cfg(target_arch = "aarch64")]
use super::pending_irqs::PendingIrqs;

/// Byte images of the records the hypervisor syscalls copy to the cell.
#[path = "vmm_wire.rs"]
pub(crate) mod vmm_wire;

// ── AArch64-only concrete types ───────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
use crate::memory::stage2::Stage2Table;
#[cfg(target_arch = "aarch64")]
use api::hypervisor::ViVmExit as ApiVmExit;
#[cfg(target_arch = "aarch64")]
use hal::aarch64::backend::{ArmEl2Monitor, ArmResources, Board, El2Monitor, VgicWindows};
#[cfg(target_arch = "aarch64")]
use hal::aarch64::{
    id_regs::read_trapped_id_reg,
    stage2_regs::{disable_stage2, enable_stage2},
    vcpu::AArch64Vcpu,
};
#[cfg(target_arch = "aarch64")]
use hal::aarch64::{vcpu::run_vcpu_impl, vgic};
#[cfg(target_arch = "aarch64")]
use hal::ViVmExit as HalVmExit;

/// The host reaches EL2 through the board's EL2 monitor (the Pi: guest IRQs
/// via `HCR_EL2.VI`, no GICV) instead of running at EL2 itself (guest IRQs via
/// the GICH list registers, GICV passed through).
#[cfg(target_arch = "aarch64")]
const PI_MONITOR: bool = El2Monitor::PRESENT;

/// The board's GICv2 virtualization windows; `None` leaves guests without GICH
/// list-register injection or GICV passthrough (the Pi 3 monitor uses
/// `HCR_EL2.VI` instead).
#[cfg(target_arch = "aarch64")]
const VGIC: Option<VgicWindows> = <Board as ArmResources>::VGIC;

/// Guest IPA of the guest's GICC, as the hypervisor cell's DTB declares it.
#[cfg(target_arch = "aarch64")]
const GUEST_GICC_IPA: u64 = 0x0801_0000;

/// Guest virtual-timer PPI (the guest DTB's `arm,armv8-timer` virtual entry).
/// Level-sensitive: it is sampled before every entry, never queued.
#[cfg(target_arch = "aarch64")]
const GUEST_VTIMER_PPI: u32 = 27;

/// The guest's virtual timer line, against the virtual count with the current
/// `CNTVOFF_EL2` applied (see [`vgic::vtimer_line_asserted`]).
#[cfg(target_arch = "aarch64")]
fn guest_vtimer_asserted(ctl: u64, cval: u64) -> bool {
    vgic::vtimer_line_asserted(ctl, cval, hal::aarch64::timer::virtual_count_now())
}

// ── VM entry ──────────────────────────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
struct Vm {
    stage2: Stage2Table,
    guest_pa: u64,
    guest_pages: usize,
    vcpus: Vec<AArch64Vcpu>,
    /// Per-vCPU pending virtual IRQ set; intids set by inject_irq, drained into
    /// GICH LRs just before each run_vcpu_impl call (Phase 09). Fixed-size
    /// coalescing bitset — see `pending_irqs::PendingIrqs` for why this isn't
    /// a queue.
    vcpu_irqs: Vec<PendingIrqs>,
    /// Per-vCPU GICv2 virtual CPU interface state (VMCR, APR, Active LRs),
    /// installed in GICH around each guest run on hosts without the Pi monitor.
    vcpu_vgic: Vec<vgic::CpuIfState>,
    // reason: the Pi monitor's guest entry passes this to `monitor::run`; on the
    // other AArch64 hosts the local `vmid` still carries the Stage-2 activation,
    // so the field stays reserved for VM introspection until they read it too.
    #[allow(dead_code)]
    vmid: u16,
    /// Set once the guest RAM window has been cleaned to PoC and the I-cache
    /// invalidated, before the first vCPU entry.
    entry_flushed: bool,
}

// VM_REGISTRY is keyed by (owner_tid, vm_id).
// vm_id is assigned sequentially per owner; starts at 1.
#[cfg(target_arch = "aarch64")]
static VM_REGISTRY: Spinlock<Option<BTreeMap<(usize, usize), Vm>>> = Spinlock::new(None);

#[cfg(target_arch = "aarch64")]
static NEXT_VMID: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(1);

#[cfg(target_arch = "aarch64")]
fn registry_lock() -> &'static Spinlock<Option<BTreeMap<(usize, usize), Vm>>> {
    &VM_REGISTRY
}

// ── Sequential vm_id counter per owner ───────────────────────────────────────

/// Per-owner sequential VM-id counter, stored alongside each owner's first VM.
/// Simple: we just use the total registered VM count + 1 as the next id.
// reason: kept for near-future VM lifecycle refactor (currently `create_vm`
// inlines equivalent logic); not yet wired up as a callable helper.
#[allow(dead_code)]
#[cfg(target_arch = "aarch64")]
fn next_vm_id_for(owner: usize) -> usize {
    let guard = registry_lock().lock();
    let count = guard
        .as_ref()
        .map_or(0, |m| m.keys().filter(|(o, _)| *o == owner).count());
    count + 1
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Allocate guest RAM + Stage-2 table; return opaque `vm_id`.
pub fn create_vm(owner: usize, guest_pages: usize) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        if PI_MONITOR && !El2Monitor::is_verified() {
            // The cell can only print one sentence for every failure, so the
            // reason has to be here: a refused boot monitor and a failed carve
            // are different bugs with the same symptom on the console.
            log::warn!(
                "[hv] create_vm refused: EL2 monitor not verified (ready={})",
                El2Monitor::is_ready()
            );
            return Err(ViError::NotSupported);
        }
        use crate::memory::paging::PAGE_SIZE;

        let mut table = Stage2Table::new().ok_or_else(|| {
            log::error!("[hv] create_vm: stage-2 root allocation failed");
            ViError::OutOfMemory
        })?;
        let guest_pa = table
            .carve_guest_ram(guest_pages)
            .ok_or_else(|| {
                let (total_mib, used_mib, largest_mib) = {
                    let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
                    frames
                        .as_ref()
                        .map(|allocator| {
                            (
                                allocator.total_memory() / (1024 * 1024),
                                allocator.used_memory() / (1024 * 1024),
                                allocator.largest_free_run() / 256,
                            )
                        })
                        .unwrap_or((0, 0, 0))
                };
                log::error!(
                    "[hv] create_vm: no contiguous guest run ({} pages, {} MiB; allocator {} MiB total, {} MiB used, largest free run {} MiB)",
                    guest_pages,
                    guest_pages / 256,
                    total_mib,
                    used_mib,
                    largest_mib
                );
                ViError::OutOfMemory
            })?;
        // Map all guest RAM at IPA 0x40000000.
        table
            .map(0x4000_0000, guest_pa, guest_pages, true)
            .map_err(|error| {
                log::error!("[hv] create_vm: guest RAM stage-2 map failed: {:?}", error);
                ViError::OutOfMemory
            })?;
        // GICV is passed through only on boards that declare it (QEMU virt).
        // Pi GICC/GICD are software-MMIO devices and must remain unmapped
        // Stage-2 holes. GICV is writable: the guest enables its interface
        // (GICC_CTLR, GICC_PMR) and completes every interrupt (GICC_EOIR)
        // through it, and those writes reach only the running vCPU's GICH
        // state, which `vgic::CpuIfState` switches per vCPU.
        if let Some(windows) = VGIC {
            table
                .map_mmio_passthrough(
                    GUEST_GICC_IPA,
                    windows.gicv.base as u64,
                    windows.gicv.size / PAGE_SIZE,
                    true,
                )
                .map_err(|error| {
                    log::error!("[hv] create_vm: GICV stage-2 map failed: {:?}", error);
                    ViError::OutOfMemory
                })?;
        }

        let vmid = NEXT_VMID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: table built and flushed; vmid ≥ 1; not yet active (enable later).
        unsafe {
            enable_stage2(vmid, table.root_pa());
        }
        if let Some(windows) = VGIC {
            // SAFETY: EL2 host; the board's GICH window is kernel-mapped.
            unsafe {
                vgic::enable(windows.gich_base);
            }
        }

        let vm_id = {
            let mut guard = registry_lock().lock();
            if guard.is_none() {
                *guard = Some(BTreeMap::new());
            }
            let map = guard.as_mut().unwrap();
            let id = map.keys().filter(|(o, _)| *o == owner).count() + 1;
            map.insert(
                (owner, id),
                Vm {
                    stage2: table,
                    guest_pa,
                    guest_pages,
                    vcpus: Vec::new(),
                    vcpu_irqs: Vec::new(),
                    vcpu_vgic: Vec::new(),
                    vmid,
                    entry_flushed: false,
                },
            );
            id
        };
        Ok(vm_id)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::create_vm(owner, guest_pages)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, guest_pages);
        Err(ViError::NotSupported)
    }
}

/// Create a vCPU in `vm_id` with initial PC `entry_pc`; return `vcpu_id` (1-based).
///
/// Under `test-hooks`, writes a P04 HVC smoke blob (`MOVZ X0,#42; HVC #0; B .`)
/// to the page containing `entry_pc` so the test cell does not need userspace
/// memory access to guest RAM.
pub fn create_vcpu(owner: usize, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        let mut guard = registry_lock().lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
        let vcpu_id = vm.vcpus.len() + 1;

        // test-hooks: write P04 HVC smoke blob so the test cell can verify Hvc exit.
        #[cfg(feature = "test-hooks")]
        {
            const MOVZ_X0_42: u32 = 0xD280_0540; // MOVZ X0, #42
            const HVC_0: u32 = 0xD400_0002; // HVC #0
            const B_DOT: u32 = 0x1400_0000; // B .
            const GUEST_IPA_BASE: u64 = 0x4000_0000;
            let offset = (entry_pc - GUEST_IPA_BASE) as usize;
            let blob_pa = vm.guest_pa as usize + offset;
            // SAFETY: guest RAM is kernel-allocated identity-mapped memory; no active vCPU yet.
            unsafe {
                let ptr = blob_pa as *mut u32;
                ptr.write(MOVZ_X0_42);
                ptr.add(1).write(HVC_0);
                ptr.add(2).write(B_DOT);
            }
        }

        vm.vcpus.push(AArch64Vcpu::new(entry_pc));
        vm.vcpu_irqs.push(PendingIrqs::new());
        vm.vcpu_vgic.push(vgic::CpuIfState::new());
        Ok(vcpu_id)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::create_vcpu(owner, vm_id, entry_pc)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, entry_pc);
        Err(ViError::NotSupported)
    }
}

/// Map guest IPA range in `vm_id`'s Stage-2.
pub fn map_guest_memory(
    owner: usize,
    vm_id: usize,
    ipa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    #[cfg(target_arch = "aarch64")]
    {
        use crate::memory::paging::PAGE_SIZE;
        let pages = size.div_ceil(PAGE_SIZE);
        let mut guard = registry_lock().lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
        // Extend guest RAM mapping to cover the requested IPA range.
        vm.stage2
            .map(ipa, vm.guest_pa, pages, writable)
            .map_err(|_| ViError::OutOfMemory)?;
        Ok(())
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::map_guest_memory(owner, vm_id, ipa, size, writable)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, ipa, size, writable);
        Err(ViError::NotSupported)
    }
}

/// The guest-RAM window of `vm_id`: its first guest-physical address and its
/// size in bytes. The syscall layer bounds every guest-memory request with it
/// before staging any data.
pub fn guest_window(owner: usize, vm_id: usize) -> ViResult<(u64, usize)> {
    #[cfg(target_arch = "aarch64")]
    {
        const GUEST_IPA_BASE: u64 = 0x4000_0000;
        let guard = registry_lock().lock();
        let map = guard.as_ref().ok_or(ViError::NotFound)?;
        let vm = map.get(&(owner, vm_id)).ok_or(ViError::NotFound)?;
        let bytes = vm
            .guest_pages
            .checked_mul(crate::memory::paging::PAGE_SIZE)
            .ok_or(ViError::InvalidInput)?;
        Ok((GUEST_IPA_BASE, bytes))
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::guest_window(owner, vm_id)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id);
        Err(ViError::NotSupported)
    }
}

/// World-switch into the vCPU and return the exit, converted to the API ABI.
/// The syscall layer copies it to the cell; this never touches cell memory.
pub fn run_vcpu(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
) -> ViResult<api::hypervisor::ViVmExit> {
    #[cfg(target_arch = "aarch64")]
    {
        let hal_exit = {
            let mut guard = registry_lock().lock();
            let map = guard.as_mut().ok_or(ViError::NotFound)?;
            let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
            let vcpu_idx = vcpu_id.saturating_sub(1);

            // Pi's software GICC requests at most one edge per entry. Consume
            // one request now, assert HCR_EL2.VI only during this guest run and
            // clear VI in the EL2 exit trampoline before the host resumes.
            let mut virtual_irq = PI_MONITOR
                && vm
                    .vcpu_irqs
                    .get_mut(vcpu_idx)
                    .and_then(PendingIrqs::take_lowest)
                    .is_some();
            let mut num_loaded = 0usize;
            if let Some(windows) = VGIC {
                if let (Some(q), Some(cpu_if), Some(vcpu)) = (
                    vm.vcpu_irqs.get_mut(vcpu_idx),
                    vm.vcpu_vgic.get_mut(vcpu_idx),
                    vm.vcpus.get(vcpu_idx),
                ) {
                    // The guest's timer is level-triggered and only its saved
                    // CNTV state says whether the line is up, so sample it at
                    // every entry. Injecting it only on WFI starved a CPU-bound
                    // guest of ticks: it never waited, so it never got one.
                    if guest_vtimer_asserted(vcpu.g_cntv_ctl, vcpu.g_cntv_cval) {
                        q.set(GUEST_VTIMER_PPI);
                    }
                    // SAFETY: EL2; the board's GICH window is kernel-mapped; no
                    // vCPU running.
                    num_loaded = unsafe { cpu_if.load(windows.gich_base, || q.take_lowest()) };
                }
            }

            // ── Guest image: make it visible to the guest's own view ────────────
            //
            // The cell streams the guest kernel/initrd/DTB into guest RAM through
            // `write_guest_memory`, which copies through the kernel's mapping; the
            // guest fetches those pages through Stage-2, a different VA and ASID.
            // Cache maintenance by VA does not cover that alias, so the first
            // entry cleans the whole window to the point of coherency and drops
            // the instruction cache. Once per VM: the streaming happens before
            // the first run, and the guest owns the window afterwards.
            if !vm.entry_flushed {
                #[cfg(target_arch = "aarch64")]
                {
                    let start = vm.guest_pa as usize;
                    let len = vm.guest_pages * crate::memory::paging::PAGE_SIZE;
                    hal::aarch64::cache::clean_data_cache_range(start, len);
                    hal::aarch64::cache::invalidate_instruction_cache_all();
                }
                vm.entry_flushed = true;
            }

            // ── World-switch into guest ──────────────────────────────────────────
            let exit = {
                let vcpu = vm.vcpus.get_mut(vcpu_idx).ok_or(ViError::NotFound)?;

                // Resolve guest ID_AA64* reads (trapped by HCR_EL2.TID3) here:
                // `ViVmExit::SysReg` cannot carry a return value. When the
                // bounded batch ends, retry the faulting MRS on the next run
                // instead of fabricating a CPU feature value.
                const MAX_ID_REG_RESOLVES: u32 = 64;
                let mut resolved = 0u32;
                let exit = loop {
                    let exit = if PI_MONITOR {
                        let vi = core::mem::replace(&mut virtual_irq, false);
                        unsafe {
                            El2Monitor::run(vcpu, vm.vmid, vm.stage2.root_pa(), vi);
                        }
                        vcpu.decode_exit()
                    } else {
                        unsafe { run_vcpu_impl(vcpu) }
                    };
                    if let HalVmExit::SysReg {
                        op0,
                        op1,
                        crn,
                        crm,
                        op2,
                        rt,
                        is_write,
                    } = exit
                    {
                        if !is_write {
                            if let Some(val) = read_trapped_id_reg(op0, op1, crn, crm, op2) {
                                if resolved == MAX_ID_REG_RESOLVES {
                                    break HalVmExit::Preempted;
                                }
                                if (rt as usize) < 31 {
                                    vcpu.gp[rt as usize] = val;
                                }
                                vcpu.g_elr_el2 = vcpu.exit_elr.wrapping_add(4);
                                resolved += 1;
                                continue;
                            }
                        }
                    }
                    break exit;
                };
                // Unhandled guest trap: dump the guest's own EL1 exception bank.
                // After a guest-internal exception these carry the ORIGINAL
                // syndrome (the EL2 exit only sees the follow-on vector-fetch
                // fault), which is the difference between a diagnosable failure
                // and "unknown vmexit".
                if let HalVmExit::Unknown { ec, iss } = exit {
                    log::warn!(
                        "[hv] unhandled guest trap ec={:#x} iss={:#x} | guest ELR_EL1={:#x} ESR_EL1={:#x} FAR_EL1={:#x} VBAR_EL1={:#x} SCTLR_EL1={:#x} SPSR_EL1={:#x}",
                        ec, iss,
                        vcpu.g_elr_el1, vcpu.g_esr_el1, vcpu.g_far_el1,
                        vcpu.g_vbar_el1, vcpu.g_sctlr_el1, vcpu.g_spsr_el1,
                    );
                    log::warn!(
                        "[hv]   guest TCR_EL1={:#x} TTBR0_EL1={:#x} TTBR1_EL1={:#x} MAIR_EL1={:#x}",
                        vcpu.g_tcr_el1,
                        vcpu.g_ttbr0_el1,
                        vcpu.g_ttbr1_el1,
                        vcpu.g_mair_el1,
                    );
                }
                exit
                // vcpu borrow ends here (NLL + nested block)
            };

            // ── Phase 09: take the vCPU's GICH state back after exit ────────────
            // Unacknowledged edge interrupts return to the pending set; the timer
            // line is re-sampled at the next entry instead. Active ones (guest
            // preempted mid-handler) stay with the vCPU's interface state.
            // SAFETY: no vCPU running; EL2; the board's GICH window is mapped.
            if let Some(windows) = VGIC {
                if let (Some(q), Some(cpu_if)) = (
                    vm.vcpu_irqs.get_mut(vcpu_idx),
                    vm.vcpu_vgic.get_mut(vcpu_idx),
                ) {
                    unsafe {
                        cpu_if.save(
                            windows.gich_base,
                            num_loaded,
                            |intid| intid == GUEST_VTIMER_PPI,
                            |intid| q.set(intid),
                        )
                    };
                }
            }

            exit
        };

        // Convert HAL ViVmExit → API ViVmExit (same fields, different crate paths).
        let api_exit = match hal_exit {
            HalVmExit::MmioRead { ipa, size, reg } => ApiVmExit::MmioRead { ipa, size, reg },
            HalVmExit::MmioWrite { ipa, size, val } => ApiVmExit::MmioWrite { ipa, size, val },
            HalVmExit::Hvc { imm, regs } => ApiVmExit::Hvc { imm, regs },
            HalVmExit::Wfi => ApiVmExit::Wfi,
            HalVmExit::SysReg {
                op0,
                op1,
                crn,
                crm,
                op2,
                rt,
                is_write,
            } => ApiVmExit::SysReg {
                op0,
                op1,
                crn,
                crm,
                op2,
                rt,
                is_write,
            },
            HalVmExit::Preempted => ApiVmExit::Preempted,
            HalVmExit::Shutdown => ApiVmExit::Shutdown,
            HalVmExit::Unknown { ec, iss } => ApiVmExit::Unknown { ec, iss },
            // x86-only exits (SVM/VT-x) never arise on the aarch64 world-switch.
            HalVmExit::PortIn { .. }
            | HalVmExit::PortOut { .. }
            | HalVmExit::Hlt
            | HalVmExit::Msr { .. } => ApiVmExit::Unknown { ec: 0, iss: 0 },
        };
        Ok(api_exit)
    }
    #[cfg(target_arch = "x86_64")]
    {
        use api::hypervisor::ViVmExit as ApiVmExit;
        use hal::ViVmExit as HalVmExit;
        let hal_exit = super::svm_registry::run_vcpu_hal(owner, vm_id, vcpu_id)?;
        // HAL → API conversion (VERSION 2 ABI — the x86 variants are frozen at
        // discriminants 8-11). `reg` is not decoded for PortIn (guest `IN`
        // always targets (E)AX) → 0.
        let api_exit = match hal_exit {
            HalVmExit::MmioRead { ipa, size, reg } => ApiVmExit::MmioRead { ipa, size, reg },
            HalVmExit::MmioWrite { ipa, size, val } => ApiVmExit::MmioWrite { ipa, size, val },
            HalVmExit::Preempted => ApiVmExit::Preempted,
            HalVmExit::Shutdown => ApiVmExit::Shutdown,
            HalVmExit::Unknown { ec, iss } => ApiVmExit::Unknown { ec, iss },
            HalVmExit::PortIn { port, size } => ApiVmExit::PortIn { port, size, reg: 0 },
            HalVmExit::PortOut { port, size, val } => ApiVmExit::PortOut { port, size, val },
            HalVmExit::Hlt => ApiVmExit::Hlt,
            HalVmExit::Msr {
                index,
                is_write,
                value,
            } => ApiVmExit::Msr {
                index,
                is_write,
                val: value,
            },
            // ARM-only HAL variants — unreachable on x86 (no aarch64 exits here).
            HalVmExit::Hvc { .. } | HalVmExit::Wfi | HalVmExit::SysReg { .. } => {
                ApiVmExit::Unknown { ec: 0, iss: 0 }
            }
        };
        Ok(api_exit)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, vcpu_id, _budget_ns);
        Err(ViError::NotSupported)
    }
}

/// Modes 0/1 read/write 32 guest GP-register words. On Pi mode 2 reads a
/// 32-word timer snapshot (CNTV_CTL_EL0, CNTV_CVAL_EL0, then thirty zeros).
///
/// `regs` is a kernel copy of the cell's buffer: read modes fill it, mode 1
/// consumes it. Words a backend does not model are left as they were.
pub fn vcpu_regs(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    mode: usize,
) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        let mut guard = registry_lock().lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
        let vcpu = vm
            .vcpus
            .get_mut(vcpu_id.saturating_sub(1))
            .ok_or(ViError::NotFound)?;
        let buf = regs;
        match mode {
            1 => {
                for (i, v) in buf[..31].iter().enumerate() {
                    vcpu.gp[i] = *v;
                }
                vcpu.g_elr_el2 = buf[31];
            }
            0 => {
                for (i, v) in vcpu.gp.iter().enumerate() {
                    buf[i] = *v;
                }
                buf[31] = vcpu.g_elr_el2;
            }
            2 if <hal::aarch64::vcpu::VcpuExt as hal::aarch64::backend::ArmVcpuExt>::REGS_MAX_MODE
                >= 2 =>
            {
                buf.fill(0);
                buf[0] = vcpu.g_cntv_ctl;
                buf[1] = vcpu.g_cntv_cval;
            }
            _ => return Err(ViError::InvalidArgument),
        }
        Ok(0)
    }
    #[cfg(target_arch = "x86_64")]
    {
        if mode > 1 {
            return Err(ViError::InvalidArgument);
        }
        super::svm_registry::vcpu_regs(owner, vm_id, vcpu_id, regs, mode == 1)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, vcpu_id, regs, mode);
        Err(ViError::NotSupported)
    }
}

/// Copy `src` into guest physical RAM at `gpa`.
///
/// `src` is kernel memory (the syscall layer stages cell data through the
/// caller's copy view); the whole `[gpa, gpa + len)` range must lie inside the
/// guest-RAM window or nothing is written.
pub fn write_guest_memory(owner: usize, vm_id: usize, gpa: u64, src: &[u8]) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        use crate::memory::paging::PAGE_SIZE;
        const GUEST_IPA_BASE: u64 = 0x4000_0000;
        let len = src.len();

        let guard = registry_lock().lock();
        let map = guard.as_ref().ok_or(ViError::NotFound)?;
        let vm = map.get(&(owner, vm_id)).ok_or(ViError::NotFound)?;

        // Validate gpa is within the mapped guest-RAM window.
        let offset = gpa
            .checked_sub(GUEST_IPA_BASE)
            .ok_or(ViError::InvalidInput)? as usize;
        let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
        if end > vm.guest_pages * PAGE_SIZE {
            return Err(ViError::InvalidInput);
        }

        // SAFETY: guest RAM is kernel-allocated identity-mapped memory and
        // `offset + len` lies inside it (checked above); `src` is a kernel
        // slice, disjoint from guest RAM.
        unsafe {
            let dst = (vm.guest_pa as usize + offset) as *mut u8;
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len);
        }
        Ok(len)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::write_guest_memory(owner, vm_id, gpa, src)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, gpa, src);
        Err(ViError::NotSupported)
    }
}

/// Copy guest physical RAM at `gpa` into `dst`.
///
/// `dst` is kernel memory (the syscall layer copies it out through the
/// caller's copy view); the whole `[gpa, gpa + len)` range must lie inside the
/// guest-RAM window or nothing is read.
pub fn read_guest_memory(owner: usize, vm_id: usize, gpa: u64, dst: &mut [u8]) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        use crate::memory::paging::PAGE_SIZE;
        const GUEST_IPA_BASE: u64 = 0x4000_0000;
        let len = dst.len();

        let guard = registry_lock().lock();
        let map = guard.as_ref().ok_or(ViError::NotFound)?;
        let vm = map.get(&(owner, vm_id)).ok_or(ViError::NotFound)?;

        // Validate gpa is within the mapped guest-RAM window.
        let offset = gpa
            .checked_sub(GUEST_IPA_BASE)
            .ok_or(ViError::InvalidInput)? as usize;
        let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
        if end > vm.guest_pages * PAGE_SIZE {
            return Err(ViError::InvalidInput);
        }

        // SAFETY: guest RAM is kernel-allocated identity-mapped memory and
        // `offset + len` lies inside it (checked above); `dst` is a kernel
        // slice, disjoint from guest RAM.
        unsafe {
            let src = (vm.guest_pa as usize + offset) as *const u8;
            core::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), len);
        }
        Ok(len)
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::read_guest_memory(owner, vm_id, gpa, dst)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, gpa, dst);
        Err(ViError::NotSupported)
    }
}

/// Queue one virtual IRQ request for the next vCPU run. On Pi the monitor
/// asserts HCR_EL2.VI for that run only; its exit path clears VI before returning
/// to the EL1 host. The software GICC owns enabled/pending/active state.
pub fn inject_irq(owner: usize, vm_id: usize, vcpu_id: usize, intid: u32) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        let mut guard = registry_lock().lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
        let idx = vcpu_id.saturating_sub(1);
        if let Some(q) = vm.vcpu_irqs.get_mut(idx) {
            q.set(intid);
        }
        Ok(0)
    }
    #[cfg(target_arch = "x86_64")]
    {
        // `intid` is reinterpreted as an x86 interrupt vector (8259 line remap).
        super::svm_registry::inject_irq(owner, vm_id, vcpu_id, intid)?;
        Ok(0)
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, vcpu_id, intid);
        Ok(0)
    }
}

// ── Teardown — called on every task-exit path ─────────────────────────────────

/// Reclaim all VMs and guest RAM owned by `dead_tid`.
///
/// Called alongside `reap_grants_for_task` on task exit, fault, and watchdog kill.
/// Lock order: VM_REGISTRY → FRAME_ALLOCATOR (same as grant reaper).
pub fn reap_vms_for_task(dead_tid: usize) {
    #[cfg(target_arch = "aarch64")]
    {
        // Collect entries to drop outside the lock (Stage2Table::drop frees frames).
        let dead_vms: alloc::vec::Vec<Vm> = {
            let mut guard = registry_lock().lock();
            let Some(map) = guard.as_mut() else { return };
            let dead_keys: alloc::vec::Vec<(usize, usize)> = map
                .keys()
                .filter(|(o, _)| *o == dead_tid)
                .copied()
                .collect();
            dead_keys.iter().filter_map(|k| map.remove(k)).collect()
        };
        // Disable Stage-2 for each dying VM before dropping the table.
        for vm in dead_vms {
            // SAFETY: no vCPU is running (task is dead); safe to disable Stage-2.
            unsafe {
                disable_stage2();
            }
            drop(vm); // Stage2Table::drop frees all frames
        }
    }
    #[cfg(target_arch = "x86_64")]
    {
        super::svm_registry::reap_vms_for_task(dead_tid);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = dead_tid;
    }
}

/// Pi's EL1-host capability gate: an actual monitor round-trip must run guest
/// HVC, WFI, MMIO and software-injected virtual IRQ before any VM is admitted.
/// Called only when the board has an EL2 monitor (`El2Monitor::PRESENT`).
#[cfg(target_arch = "aarch64")]
pub fn pi_monitor_smoke() {
    use crate::memory::frame::phys_to_virt;
    use hal::aarch64::cache::{
        clean_data_cache_range, invalidate_instruction_cache_all, sync_instruction_cache,
    };

    if !El2Monitor::is_ready() {
        log::warn!(
            "[pi-monitor] entry EL={} HVC init unavailable; HypervisorCap closed",
            El2Monitor::entry_el()
        );
        return;
    }
    const IPA: u64 = 0x4000_0000;
    const VMID: u16 = 0xfffe;
    let Some(mut table) = Stage2Table::new() else {
        log::warn!("[pi-monitor] smoke: no Stage-2 root; HypervisorCap closed");
        return;
    };
    let Some(guest_pa) = table.carve_guest_ram(1) else {
        log::warn!("[pi-monitor] smoke: no guest page; HypervisorCap closed");
        return;
    };
    if table.map(IPA, guest_pa, 1, true).is_err() {
        log::warn!("[pi-monitor] smoke: guest map failed; HypervisorCap closed");
        return;
    }
    let page = phys_to_virt(guest_pa as usize) as *mut u32;
    // MOVZ X0,#42; HVC #0; WFI; MOVZ X0,#0x900,LSL#16;
    // STR X1,[X0]; HVC #0. The final HVC bounds a failure if MMIO did not
    // trap instead of leaving an unbounded guest loop.
    let blob = [
        0xD280_0540,
        0xD400_0002,
        0xD503_201F,
        0xD2A1_2000,
        0xF900_0001,
        0xD400_0002,
    ];
    unsafe {
        core::ptr::copy_nonoverlapping(blob.as_ptr(), page, blob.len());
        // EL1h IRQ vector = VBAR_EL1+0x280; HVC reports that vector was reached.
        page.add(0x280 / 4).write(0xD400_0002);
        // +0x300: `B .` — a guest that never yields, for the preemption step.
        page.add(0x300 / 4).write(0x1400_0000);
        sync_instruction_cache(page as usize, page as usize, 4096);
        // The guest fetches this page through Stage-2, i.e. a different VA and
        // ASID, so the per-VA invalidate above does not cover the alias. Clean
        // the page to the point of coherency and drop the whole instruction
        // cache: on real hardware (not in TCG, which has no caches) a guest that
        // fetched a stale alias faulted at its first instruction while EL2's own
        // read of the same page showed the correct blob.
        clean_data_cache_range(page as usize, 4096);
        invalidate_instruction_cache_all();
    }
    let mut vcpu = AArch64Vcpu::new(IPA);
    // These markers distinguish an unobserved EL2 write from a genuine trap
    // whose syndrome and PC happen to be zero.
    vcpu.exit_esr = 0x0053_4d4f_4b45_5352;
    vcpu.exit_elr = 0x0053_4d4f_4b45_5043;
    // Boot has DAIF.I set: defer the existing, still-routed physical timers
    // for a bounded 100 ms guest smoke window. Otherwise their pre-boot
    // pending IRQs can turn a conditional WFI into a NOP. Production vCPU
    // runs retain the regular 10 ms host-tick preemption.
    let prepare = El2Monitor::prepare_smoke_window;
    // A physical tick can still race guest entry. Retry only genuine
    // preemptions, bounded: a permanently asserted unrelated IRQ fails closed.
    let run_step = |cpu: &mut AArch64Vcpu, vi: bool| {
        for _ in 0..16 {
            prepare();
            unsafe {
                El2Monitor::run(cpu, VMID, table.root_pa(), vi);
            }
            if !matches!(cpu.decode_exit(), HalVmExit::Preempted) {
                return;
            }
        }
    };
    let passed = {
        log::info!(
            "[pi-monitor] first-run EL1 entry={:#x} insn={:#x} esr-marker={:#x} elr-marker={:#x}",
            vcpu.g_elr_el2,
            unsafe { core::ptr::read_volatile(page) },
            vcpu.exit_esr,
            vcpu.exit_elr
        );
        prepare();
        let first = unsafe {
            El2Monitor::run_probed(&mut vcpu, VMID, table.root_pa(), false, IPA, guest_pa)
        };
        log::info!(
            "[pi-monitor] first-run root={:#x} page={:#x} EL2 entry={:#x} s2=[{:#x},{:#x},{:#x}] insn={:#x} exit=[{:#x},{:#x},irq={}] EL1 exit=[{:#x},{:#x},irq={}]",
            table.root_pa(), guest_pa, first.entry_pc, first.stage2_l1,
            first.stage2_l2, first.stage2_l3, first.instruction,
            first.exit_esr, first.exit_elr, first.exit_is_irq,
            vcpu.exit_esr, vcpu.exit_elr, vcpu.exit_is_irq
        );
        if matches!(vcpu.decode_exit(), HalVmExit::Preempted) {
            run_step(&mut vcpu, false);
        }
        let hvc = matches!(vcpu.decode_exit(), HalVmExit::Hvc { imm: 0, regs } if regs[0] == 42);
        if !hvc {
            // EL1's own view of the same words EL2 reported: if they differ the
            // two regimes disagree about memory; if they agree, the divergence is
            // in what the *hardware* walk/fetch saw, not in what software reads.
            let el1_root_leaf = unsafe {
                core::ptr::read_volatile(phys_to_virt(table.root_pa() as usize) as *const u64)
            };
            let el1_insn = unsafe { core::ptr::read_volatile(page) };
            log::warn!(
                "[pi-monitor] HVC exit={:?} pc={:#x} esr={:#x} EL1 leaf0={:#x} EL1 insn={:#x}",
                vcpu.decode_exit(),
                vcpu.exit_elr,
                vcpu.exit_esr,
                el1_root_leaf,
                el1_insn
            );
            false
        } else {
            // Step 2 runs the guest's WFI. Whether it produces a `Wfi` trap is
            // emulator/hardware dependent: QEMU's AArch64 WFI helper returns
            // before the HCR_EL2.TWI trap whenever `cpu_has_work()` is true
            // (target/arm/tcg/op_helper.c), so under QEMU-TCG the WFI can be an
            // in-block NOP. Measured on raspi3b: the WFI is a NOP and the guest
            // runs straight into the MMIO store. The gate therefore requires
            // forward progress past the WFI — either exit — and logs which one.
            run_step(&mut vcpu, false);
            let (mmio_ok, wfi_trapped) = match vcpu.decode_exit() {
                HalVmExit::Wfi => {
                    run_step(&mut vcpu, false);
                    (
                        matches!(
                            vcpu.decode_exit(),
                            HalVmExit::MmioWrite {
                                ipa: 0x0900_0000,
                                size: 8,
                                ..
                            }
                        ),
                        true,
                    )
                }
                HalVmExit::MmioWrite {
                    ipa: 0x0900_0000,
                    size: 8,
                    ..
                } => (true, false),
                _ => (false, false),
            };
            if !mmio_ok {
                log::warn!(
                    "[pi-monitor] WFI/MMIO exit={:?} pc={:#x} esr={:#x} far={:#x} hpfar={:#x}",
                    vcpu.decode_exit(),
                    vcpu.exit_elr,
                    vcpu.exit_esr,
                    vcpu.exit_far,
                    vcpu.exit_hpfar
                );
                let soc = hal_soc_bcm27xx::BCM2837;
                // SAFETY: both controller apertures are identity-mapped before
                // IRQs are enabled and the smoke runs after paging activation.
                let (
                    basic,
                    pending1,
                    pending2,
                    enable1,
                    enable2,
                    core_src,
                    core_fiq,
                    core_timers,
                    cntp_ctl,
                ) = unsafe {
                    (
                        core::ptr::read_volatile(soc.mmio.legacy_irq_base as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x04) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x08) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x10) as *const u32),
                        core::ptr::read_volatile((soc.mmio.legacy_irq_base + 0x14) as *const u32),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x60) as *const u32,
                        ),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x70) as *const u32,
                        ),
                        core::ptr::read_volatile(
                            (soc.mmio.local_controller_base + 0x40) as *const u32,
                        ),
                        {
                            let v: u64;
                            core::arch::asm!("mrs {v}, cntp_ctl_el0",
                                             v = out(reg) v, options(nomem, nostack));
                            v
                        },
                    )
                };
                log::warn!(
                    "[pi-monitor]   irq: basic={:#x} pend1={:#x} pend2={:#x} en1={:#x} en2={:#x}",
                    basic,
                    pending1,
                    pending2,
                    enable1,
                    enable2
                );
                log::warn!(
                    "[pi-monitor]   core: irq_src={:#x} fiq_src={:#x} timers={:#x} cntp_ctl={:#x}",
                    core_src,
                    core_fiq,
                    core_timers,
                    cntp_ctl
                );
                false
            } else {
                log::info!(
                    "[pi-monitor] WFI {}",
                    if wfi_trapped {
                        "trapped (TWI honoured)"
                    } else {
                        "NOP (emulator helper)"
                    }
                );
                let mut irq_cpu = AArch64Vcpu::new(IPA);
                irq_cpu.g_vbar_el1 = IPA;
                irq_cpu.g_spsr_el2 &= !(1 << 7); // unmask guest IRQ
                run_step(&mut irq_cpu, true);
                // The vIRQ must be taken before the guest's first instruction:
                // PC becomes VBAR_EL1 + 0x280 and that vector's `HVC #0` runs.
                // ELR_EL2 for a natively executed HVC is HVC+4, so the exit PC
                // is IPA + 0x284 — proof the guest entered its own IRQ vector.
                let irq = matches!(irq_cpu.decode_exit(), HalVmExit::Hvc { imm: 0, .. })
                    && irq_cpu.exit_elr == IPA + 0x284;
                if !irq {
                    log::warn!(
                        "[pi-monitor] VI exit={:?} pc={:#x} esr={:#x} spsr={:#x}",
                        irq_cpu.decode_exit(),
                        irq_cpu.exit_elr,
                        irq_cpu.exit_esr,
                        irq_cpu.g_spsr_el2
                    );
                    false
                } else {
                    // A guest that never yields must still be preempted by the
                    // host tick; otherwise a runaway guest owns the CPU.
                    let mut spin_cpu = AArch64Vcpu::new(IPA);
                    spin_cpu.g_elr_el2 = IPA + 0x300;
                    El2Monitor::arm_smoke_preemption();
                    unsafe {
                        El2Monitor::run(&mut spin_cpu, VMID, table.root_pa(), false);
                    }
                    let preempted = matches!(spin_cpu.decode_exit(), HalVmExit::Preempted);
                    if !preempted {
                        log::warn!(
                            "[pi-monitor] PREEMPT exit={:?} pc={:#x} esr={:#x}",
                            spin_cpu.decode_exit(),
                            spin_cpu.exit_elr,
                            spin_cpu.exit_esr
                        );
                    }
                    preempted
                }
            }
        }
    };
    // Flush before freeing the Stage-2 frames even on a failed smoke.
    unsafe {
        disable_stage2();
    }
    // Restore the ordinary host tick cadence before Cells can run.
    El2Monitor::restore_host_tick();
    if passed {
        El2Monitor::mark_verified();
        log::info!("[pi-monitor] HVC/MMIO/VI/PREEMPT smoke PASS; HypervisorCap open");
    } else {
        log::warn!("[pi-monitor] HVC/MMIO/VI/PREEMPT smoke FAILED; HypervisorCap closed");
    }
}
