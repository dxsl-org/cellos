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

// ── AArch64-only concrete types ───────────────────────────────────────────────

#[cfg(target_arch = "aarch64")]
use crate::memory::stage2::Stage2Table;
#[cfg(target_arch = "aarch64")]
use api::hypervisor::ViVmExit as ApiVmExit;
#[cfg(target_arch = "aarch64")]
use hal::aarch64::{
    id_regs::read_trapped_id_reg,
    stage2_regs::{disable_stage2, enable_stage2},
    vcpu::{run_vcpu_impl, AArch64Vcpu},
    vgic,
};
#[cfg(target_arch = "aarch64")]
use hal::ViVmExit as HalVmExit;

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
    // reason: retained for future VM introspection/debug tooling (e.g. listing
    // active Stage-2 VMIDs); written at creation, not yet consumed by any reader.
    #[allow(dead_code)]
    vmid: u16,
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
        use crate::memory::paging::PAGE_SIZE;

        let mut table = Stage2Table::new().ok_or_else(|| {
            log::error!("[hv] create_vm: stage-2 root allocation failed");
            ViError::OutOfMemory
        })?;
        let guest_pa = table
            .carve_guest_ram(guest_pages)
            .ok_or_else(|| {
                let (total_mib, used_mib) = {
                    let frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
                    frames
                        .as_ref()
                        .map(|allocator| {
                            (
                                allocator.total_memory() / (1024 * 1024),
                                allocator.used_memory() / (1024 * 1024),
                            )
                        })
                        .unwrap_or((0, 0))
                };
                log::error!(
                    "[hv] create_vm: no contiguous guest run ({} pages, {} MiB; allocator {} MiB total, {} MiB used)",
                    guest_pages,
                    guest_pages / 256,
                    total_mib,
                    used_mib
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
        // Phase 09: GICV Stage-2 passthrough — map GICC IPA (0x0801_0000) → GICV HPA
        // (0x0804_0000) so guest GICC accesses hit real GICV hardware, removing the
        // GICC trap path.  64 KiB = 16 pages.  Read-only from guest (CPU interface
        // writes go via GICC_EOIR which GICV handles natively).
        table
            .map_mmio_passthrough(0x0801_0000, 0x0804_0000, 16, false)
            .map_err(|error| {
                log::error!("[hv] create_vm: GICV stage-2 map failed: {:?}", error);
                ViError::OutOfMemory
            })?;

        let vmid = NEXT_VMID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        // SAFETY: table built and flushed; vmid ≥ 1; not yet active (enable later).
        unsafe {
            enable_stage2(vmid, table.root_pa());
        }
        // Phase 09: enable GICH (virtual CPU interface control) for LR-based injection.
        // SAFETY: kernel runs at EL2; GICH MMIO at 0x0803_0000 is EL2-accessible.
        unsafe {
            vgic::enable();
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
                    vmid,
                },
            );
            let _ = PAGE_SIZE; // suppress unused warning
            id
        };
        Ok(vm_id)
    }
    #[cfg(target_arch = "x86_64")]
    {
        return super::svm_registry::create_vm(owner, guest_pages);
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
        Ok(vcpu_id)
    }
    #[cfg(target_arch = "x86_64")]
    {
        return super::svm_registry::create_vcpu(owner, vm_id, entry_pc);
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
        return super::svm_registry::map_guest_memory(owner, vm_id, ipa, size, writable);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, ipa, size, writable);
        Err(ViError::NotSupported)
    }
}

/// World-switch into vCPU; write `ViVmExit` to `exit_out`.
///
/// # Safety
/// `exit_out` must point to a valid, writable `ViVmExit`-sized buffer in the
/// caller's address space.  Validated by the syscall layer before this call.
pub unsafe fn run_vcpu(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    _budget_ns: u64,
    exit_out: *mut api::hypervisor::ViVmExit,
) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        let hal_exit = {
            let mut guard = registry_lock().lock();
            let map = guard.as_mut().ok_or(ViError::NotFound)?;
            let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
            let vcpu_idx = vcpu_id.saturating_sub(1);

            // ── Phase 09: drain pending-IRQ set → load into GICH LRs ────────────
            // Ascending-INTID order is used as the load order (not a GIC-mandated
            // priority — arrival order isn't guaranteed either). Bits still
            // pending once LRs run out stay set and are picked up on the next
            // entry, same as the old queue's overflow behavior.
            let mut num_loaded = 0usize;
            if let Some(q) = vm.vcpu_irqs.get_mut(vcpu_idx) {
                while num_loaded < vgic::MAX_LRS {
                    let Some(intid) = q.take_lowest() else {
                        break;
                    };
                    // SAFETY: EL2; GICH MMIO at 0x0803_0000; num_loaded < MAX_LRS.
                    unsafe {
                        vgic::load_lr(num_loaded, intid);
                    }
                    num_loaded += 1;
                }
            }

            // ── World-switch into guest ──────────────────────────────────────────
            let exit = {
                let vcpu = vm.vcpus.get_mut(vcpu_idx).ok_or(ViError::NotFound)?;

                // Resolve guest ID_AA64* reads (trapped by HCR_EL2.TID3) entirely
                // in-kernel: `ViVmExit::SysReg` carries no value field and
                // `libs/api` is frozen (Law 1), so there is no ABI-compatible way
                // to hand a resolved read back through the Cell — the kernel must
                // write the guest GPR and resume here instead. Capped so a guest
                // cannot spin this loop forever inside one syscall even though PC
                // is always advanced past the trapping instruction below.
                const MAX_ID_REG_RESOLVES: u32 = 64;
                let mut resolved = 0u32;
                let exit = loop {
                    // SAFETY: Stage-2 is enabled for this VMID; vcpu exclusively owned.
                    let exit = unsafe { run_vcpu_impl(vcpu) };
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
                        if !is_write && resolved < MAX_ID_REG_RESOLVES {
                            if let Some(val) = read_trapped_id_reg(op0, op1, crn, crm, op2) {
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

            // ── Phase 09: drain GICH LRs after exit ─────────────────────────────
            // Re-mark pending any LRs still in Active state (guest was preempted
            // mid-handling). SAFETY: no vCPU running; EL2; GICH MMIO accessible.
            if num_loaded > 0 {
                let elrsr = unsafe { vgic::read_elrsr() };
                for n in 0..num_loaded {
                    if (elrsr >> n) & 1 == 0 {
                        // LR occupied — re-mark pending if Active or Pending+Active.
                        let lr_val = unsafe { vgic::read_lr(n) };
                        if (lr_val >> 28) & 3 != 0 {
                            if let Some(q) = vm.vcpu_irqs.get_mut(vcpu_idx) {
                                q.set(lr_val & 0x3FF);
                            }
                        }
                    }
                    unsafe {
                        vgic::clear_lr(n);
                    }
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
        // SAFETY: exit_out validated by syscall layer.
        unsafe {
            core::ptr::write(exit_out, api_exit);
        }
        Ok(0)
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
        // SAFETY: exit_out validated by the syscall layer.
        unsafe {
            core::ptr::write(exit_out, api_exit);
        }
        return Ok(0);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, vcpu_id, _budget_ns, exit_out);
        Err(ViError::NotSupported)
    }
}

/// Read or write vCPU general-purpose registers (x0-x30 + sp + pc = 32×u64).
pub fn vcpu_regs(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    buf_ptr: usize,
    write: bool,
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
        // buf_ptr points to 32×u64 (256 bytes), validated by syscall layer.
        // SAFETY: buf_ptr validated; SAS — same VA in kernel and cell.
        let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u64, 32) };
        if write {
            // Write x0-x30 from buf[0..31]; buf[31] = pc (g_elr_el2).
            for (i, v) in buf[..31].iter().enumerate() {
                vcpu.gp[i] = *v;
            }
            vcpu.g_elr_el2 = buf[31];
        } else {
            for (i, v) in vcpu.gp.iter().enumerate() {
                buf[i] = *v;
            }
            buf[31] = vcpu.g_elr_el2;
        }
        Ok(0)
    }
    #[cfg(target_arch = "x86_64")]
    {
        return super::svm_registry::vcpu_regs(owner, vm_id, vcpu_id, buf_ptr, write);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, vcpu_id, buf_ptr, write);
        Err(ViError::NotSupported)
    }
}

/// Copy `len` bytes from caller's `src_ptr` into guest physical RAM at `gpa`.
///
/// # Preconditions (enforced by caller / syscall layer)
/// - `src_ptr + len` is within the caller cell's valid address range (via `validate_user_buf`).
/// - `gpa + len` does not wrap (overflow guard in syscall layer).
///
/// # Safety (kernel-internal)
/// `src_ptr` is a valid cell VA; in SAS, VA == PA for kernel-managed regions, but
/// the copy uses `copy_nonoverlapping` which only reads the source — no guest access.
pub fn write_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    src_ptr: usize,
    len: usize,
) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        use crate::memory::paging::PAGE_SIZE;
        const GUEST_IPA_BASE: u64 = 0x4000_0000;

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

        // SAFETY: guest RAM is kernel-allocated identity-mapped memory.
        // src_ptr validated by syscall layer (validate_user_buf); SAS means it's
        // also accessible here. No active vCPU reads this region while we copy
        // (the caller holds no vcpu run in progress — that would require RunVcpu,
        // which cannot be concurrent in a single-task cell).
        unsafe {
            let dst = (vm.guest_pa as usize + offset) as *mut u8;
            let src = src_ptr as *const u8;
            core::ptr::copy_nonoverlapping(src, dst, len);
        }
        Ok(len)
    }
    #[cfg(target_arch = "x86_64")]
    {
        return super::svm_registry::write_guest_memory(owner, vm_id, gpa, src_ptr, len);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, gpa, src_ptr, len);
        Err(ViError::NotSupported)
    }
}

/// Copy `len` bytes from guest physical RAM at `gpa` into caller's `dst_ptr`.
///
/// # Preconditions (enforced by caller / syscall layer)
/// - `dst_ptr + len` is within the caller cell's valid address range (via `validate_user_buf`).
/// - `gpa + len` does not wrap (overflow guard in syscall layer).
///
/// # Safety (kernel-internal)
/// `dst_ptr` is a valid cell VA; in SAS, VA == PA for kernel-managed regions.
/// Guest RAM is kernel-allocated identity-mapped memory — never freed while a vCPU
/// is alive (teardown requires the VM to be destroyed first).
pub fn read_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    dst_ptr: usize,
    len: usize,
) -> ViResult<usize> {
    #[cfg(target_arch = "aarch64")]
    {
        use crate::memory::paging::PAGE_SIZE;
        const GUEST_IPA_BASE: u64 = 0x4000_0000;

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

        // SAFETY: guest RAM is kernel-allocated identity-mapped memory.
        // dst_ptr validated by syscall layer (validate_user_buf); SAS means it's
        // also accessible here. No active vCPU writes this region while we copy
        // (the caller holds no vcpu run in progress — that would require RunVcpu,
        // which cannot be concurrent in a single-task cell).
        unsafe {
            let src = (vm.guest_pa as usize + offset) as *const u8;
            let dst = dst_ptr as *mut u8;
            core::ptr::copy_nonoverlapping(src, dst, len);
        }
        Ok(len)
    }
    #[cfg(target_arch = "x86_64")]
    {
        return super::svm_registry::read_guest_memory(owner, vm_id, gpa, dst_ptr, len);
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = (owner, vm_id, gpa, dst_ptr, len);
        Err(ViError::NotSupported)
    }
}

/// Mark a GICv2 virtual interrupt pending for delivery into vCPU via GICH LR on
/// next entry.
///
/// `intid` must be ≤ 1019 (validated by the syscall layer, m3). Pending state is
/// a coalescing bitset (`PendingIrqs`), not a queue: re-injecting an intid that's
/// already pending is a no-op, so a guest cannot grow kernel memory by masking an
/// IRQ at the vGIC and repeatedly triggering this call for the same intid. The
/// set bit is cleared and loaded into a GICH List Register in `run_vcpu` before
/// the next `run_vcpu_impl` call (Phase 09 GICH LR injection path).
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
        return Ok(0);
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
