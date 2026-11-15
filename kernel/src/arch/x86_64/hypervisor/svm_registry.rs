//! x86 SVM VM store + run loop (Tier 3 P03) — the x86 twin of the aarch64
//! branch in [`crate::hypervisor::registry`].
//!
//! The kernel owns all frame allocation (VMCB + IOPM/MSRPM + guest RAM + NPT);
//! the HAL ([`hal::x86_64::svm_vcpu::SvmVcpu`]) owns the world-switch and exit
//! decode. Guest physical address base is **0** (x86/PVH convention).
//!
//! # Lock order
//! `VM_REGISTRY` → `FRAME_ALLOCATOR` (same as the aarch64 branch + grant reaper).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::arch::memory::ept::{EptMapError, NestedFormat, NestedPageTable};
use crate::hypervisor::registry::{next_vm_id, VmCreateError, VmKey};
use crate::memory::cell_quota::{Account, Charge, Reservation};
use crate::memory::frame::phys_to_virt;
use crate::memory::frame_run::{PrivateRun, RunKind};
use crate::memory::paging::PAGE_SIZE;
use crate::memory::tag_pool::{TagLease, TagPool, TagRefusal};
use crate::sync::Spinlock;
use hal::x86_64::svm_vcpu::SvmVcpu;
use hal::x86_64::vmx_vcpu::VmxVcpu;
use hal::ViVmExit as HalVmExit;
use types::{ViError, ViResult};

/// Physical frames backing one vCPU's hardware structures, freed when the VM
/// (or a vCPU creation that failed part-way) drops them.
// reason: the runs are held for their `Drop`, which frees the frames; the vCPU
// reads the addresses it captured at creation, not these fields. One value per
// vCPU sits in a `Vec`, so the SVM variant's size costs nothing worth a box.
#[allow(dead_code, clippy::large_enum_variant)]
enum VcpuFrames {
    Svm {
        vmcb: PrivateRun,
        host: PrivateRun,
        msrpm: PrivateRun,
        iopm: PrivateRun,
        /// The vCPU's ASID, held for the VM's life. [`SVM_ASIDS`] never
        /// reissues a value, so the lease is dropped with the VM, not
        /// released.
        asid: TagLease,
    },
    Vmx {
        vmcs: Vmcs,
    },
}

/// A vCPU's VMCS frame. A VMCS still active on this CPU may be written back
/// into its frame after the frame is reused, so the drop clears it before the
/// run frees the frame.
struct Vmcs(PrivateRun);

impl Drop for Vmcs {
    fn drop(&mut self) {
        if crate::cpu_features::has_x86_virt() {
            // SAFETY: VMX backend with every online CPU in VMX operation;
            // `self.0` is this vCPU's VMCS frame.
            if let Err(error) = unsafe { hal::x86_64::vmcs::vmclear(self.0.base() as u64) } {
                // A VMCS VMCLEAR did not take may still be active on a CPU,
                // which can write it back: never reuse the frame.
                log::error!(
                    "[hv] VMCLEAR of VMCS {:#x} failed ({:?}): the frame is leaked",
                    self.0.base(),
                    error
                );
                self.0.leak("VMCLEAR failed");
            }
        }
    }
}

/// `pages` fresh frames for one VM control structure, drawn from `budget`
/// before they are allocated.
fn vm_structure(pages: usize, budget: &mut Reservation) -> ViResult<PrivateRun> {
    let charge = budget
        .split(pages * PAGE_SIZE)
        .ok_or(ViError::OutOfMemory)?;
    PrivateRun::allocate(pages, charge, RunKind::VmStructure).ok_or(ViError::OutOfMemory)
}

enum X86Vcpu {
    Svm(SvmVcpu),
    Vmx(VmxVcpu),
}

impl X86Vcpu {
    fn gpr(&self) -> &[u64; 16] {
        match self {
            Self::Svm(v) => &v.gpr,
            Self::Vmx(v) => &v.gpr,
        }
    }

    fn gpr_mut(&mut self) -> &mut [u64; 16] {
        match self {
            Self::Svm(v) => &mut v.gpr,
            Self::Vmx(v) => &mut v.gpr,
        }
    }

    unsafe fn run(&mut self) -> HalVmExit {
        match self {
            Self::Svm(v) => v.run(),
            Self::Vmx(v) => v.run().unwrap_or(HalVmExit::Shutdown),
        }
    }

    fn inject_ext_irq_gated(&mut self, vector: u8) -> bool {
        match self {
            Self::Svm(v) => v.inject_ext_irq_gated(vector),
            Self::Vmx(v) => v.inject_ext_irq_gated(vector),
        }
    }

    fn shutdown_diagnostics(&self) -> (u64, u64, u64) {
        match self {
            Self::Svm(v) => v.shutdown_diagnostics(),
            Self::Vmx(v) => v.shutdown_diagnostics(),
        }
    }

    fn npf_diagnostics(&self) -> (u64, u64, u64) {
        match self {
            Self::Svm(v) => v.npf_diagnostics(),
            Self::Vmx(v) => v.npf_diagnostics(),
        }
    }
}

/// GPA of the guest LAPIC MMIO window (x86 architectural default).
const APIC_GPA: u64 = 0xFEE0_0000;
/// Frames the xAPIC window adds to a VM: its page, and the PD and PT that map
/// [`APIC_GPA`] (the fourth GiB, beyond what the guest-RAM bound counts).
const APIC_PAGES: usize = 1 + 2;
/// Frames of one vCPU's control structures: the VMCS on VMX; on SVM the
/// VMCB, the host-save area, the MSRPM (2) and the IOPM (3).
const VMX_VCPU_PAGES: usize = 1;
const SVM_VCPU_PAGES: usize = 1 + 1 + 2 + 3;

/// The guest charge `create_vm` takes for a VM of `guest_pages`, before any
/// frame: the guest RAM, the PML4 root, the most nested tables the guest-RAM
/// window can need, and the xAPIC window. Each vCPU charges its control
/// structures separately (`create_vcpu`). `None` when the sum overflows,
/// which no budget covers.
pub(crate) fn guest_bytes(guest_pages: usize) -> Option<usize> {
    guest_pages
        .checked_add(NestedPageTable::table_bound(guest_pages))?
        .checked_add(NestedPageTable::ROOT_PAGES + APIC_PAGES)?
        .checked_mul(PAGE_SIZE)
}

/// SVM ASIDs not yet issued (the `VM-GUEST-BUDGET` fixture's tag snapshot);
/// 0 on VMX, which uses no VPID.
#[cfg(feature = "test-hooks")]
pub(crate) fn free_vm_tags() -> usize {
    if crate::arch::boot::cpu_features::x86_virt_kind()
        != Some(crate::arch::boot::cpu_features::X86Virt::Svm)
    {
        return 0;
    }
    SVM_ASIDS.free_count()
}

/// Whether `owner` could take an SVM ASID now (the `TAG-POOL-OWNER-LIMIT`
/// fixture). A taken ASID is cancelled: never used, and never reissued.
#[cfg(feature = "test-hooks")]
pub(crate) fn vm_tag_probe(owner: Charge) -> Result<(), TagRefusal> {
    SVM_ASIDS.acquire(owner).map(TagLease::cancel)
}

/// SVM ASIDs `1..=max_asid` (CPUID `8000_000A.EBX` − 1; ASID 0 is the
/// host's), one per vCPU, at most [`SVM_ASIDS_PER_OWNER`] per cell
/// incarnation and any number for the kernel. Never reused during a boot:
/// stale nested-TLB translations would otherwise let a new VM execute a
/// previous VM's GPA map. Exhaustion is permanent for the boot.
static SVM_ASIDS: TagPool =
    TagPool::never_reused(1, hal::x86_64::svm::max_asid, SVM_ASIDS_PER_OWNER);

/// SVM vCPUs one cell incarnation may create (census §8.5).
pub(crate) const SVM_ASIDS_PER_OWNER: u32 = 4;

/// One VM. Dropped only after it left [`VM_REGISTRY`], so no vCPU can enter
/// it: `run_vcpu_hal` holds the registry across every guest entry. Each field
/// frees its own frames as it drops — the NPT/EPT its tree and guest RAM
/// (after the teardown INVEPT), then the xAPIC page, then each vCPU's control
/// structures (a VMCS after its VMCLEAR), after the vCPUs that point at them.
struct SvmVm {
    npt: NestedPageTable,
    guest_pa: u64,
    guest_pages: usize,
    /// Kernel frame backing the RAM-backed xAPIC window at [`APIC_GPA`]. Guest
    /// LAPIC MMIO hits this frame directly (no trap); the timer is polled from
    /// it on HLT. Not tracked by the NPT, which owns only its own tree + guest
    /// RAM.
    apic_frame: PrivateRun,
    vcpus: Vec<X86Vcpu>,
    vcpu_frames: Vec<VcpuFrames>,
}

static VM_REGISTRY: Spinlock<Option<BTreeMap<VmKey, SvmVm>>> =
    Spinlock::ranked(None, crate::sync::rank::SVM_VM_REGISTRY);

/// Registry references do not reconstruct root birth or exclusive ownership.
#[cfg(feature = "p01-trace")]
pub(crate) fn p01_visit_roots(mut visit: impl FnMut(u64, u64, u64)) {
    let registry = VM_REGISTRY.lock();
    if let Some(registry) = registry.as_ref() {
        for (key, vm) in registry.iter() {
            let (holder, object) = key.parts();
            visit(vm.npt.root_pa(), holder, object as u64);
        }
    }
}

/// The nested-paging format for this CPU (NPT on AMD/SVM, EPT on Intel/VMX).
fn nested_format() -> NestedFormat {
    match crate::arch::boot::cpu_features::x86_virt_kind() {
        Some(crate::arch::boot::cpu_features::X86Virt::Vmx) => NestedFormat::Ept,
        _ => NestedFormat::Npt,
    }
}

/// Allocate guest RAM + NPT, mapped at GPA 0; return the opaque `vm_id`.
pub(crate) fn create_vm(owner: Charge, guest_pages: usize) -> Result<usize, VmCreateError> {
    let format = nested_format();
    // SAFETY: the EPT format is selected only on a VMX CPU, so the VMX
    // capability MSRs exist.
    if format == NestedFormat::Ept && !unsafe { hal::x86_64::invept::supported() } {
        // Without INVEPT an unmap or a reused frame could never be made
        // visible to the CPU: refuse EPT guests outright.
        log::error!("[hv-x86] create_vm: CPU lacks INVEPT; EPT guests refused");
        return Err(ViError::NotSupported.into());
    }
    // The first effect: the owner's guest charge for every frame the VM can
    // take. A refusal allocates nothing; every later frame is drawn from it,
    // and an error below frees what was drawn and refunds the whole charge.
    let mut budget = guest_bytes(guest_pages)
        .and_then(|bytes| Reservation::take(owner, Account::Guest, bytes).ok())
        .ok_or_else(|| {
            log::warn!(
                "[hv-x86] create_vm refused: {} guest pages exceed cell {}'s guest budget",
                guest_pages,
                owner.cell().0
            );
            VmCreateError::Budget
        })?;
    let mut apic_budget = budget.split(PAGE_SIZE).ok_or(ViError::OutOfMemory)?;
    let mut npt = NestedPageTable::new(format, budget).ok_or_else(|| {
        log::error!("[hv-x86] create_vm: NestedPageTable::new OOM");
        ViError::OutOfMemory
    })?;
    let guest_pa = npt.carve_guest_ram(guest_pages).ok_or_else(|| {
        log::error!(
            "[hv-x86] create_vm: carve_guest_ram({} pages = {} MiB) — no contiguous run",
            guest_pages,
            guest_pages / 256
        );
        ViError::OutOfMemory
    })?;
    npt.map(0, guest_pa, guest_pages, true).map_err(|_| {
        log::error!("[hv-x86] create_vm: npt.map OOM (guest_pa={:#x})", guest_pa);
        ViError::OutOfMemory
    })?;

    // RAM-backed xAPIC window: a kernel frame mapped at 0xFEE00000 so guest
    // LAPIC MMIO hits memory (QEMU TCG has no DecodeAssist → per-access
    // emulation is impossible). Zeroed, then ID(0x20)=0 / VERSION(0x30)=0x50014
    // pre-populated; the timer is polled kernel-side on HLT.
    let apic_frame = vm_structure(1, &mut apic_budget).inspect_err(|_| {
        log::error!("[hv-x86] create_vm: xAPIC frame OOM");
    })?;
    // SAFETY: freshly allocated, exclusively owned frame; HHDM-mapped VA. u32
    // stores are within the 4 KiB frame and naturally aligned (0x20 / 0x30).
    unsafe {
        let va = phys_to_virt(apic_frame.base()) as *mut u8;
        core::ptr::write_bytes(va, 0, PAGE_SIZE);
        (va.add(0x20) as *mut u32).write_volatile(0); // APIC ID = 0
        (va.add(0x30) as *mut u32).write_volatile(0x0005_0014); // version 0x14, max-LVT 5
    }
    if npt
        .map_device_frame(APIC_GPA, apic_frame.base() as u64, true)
        .is_err()
    {
        // No leaf was written; the frame and the table go back on return.
        log::error!("[hv-x86] create_vm: xAPIC NPT map OOM");
        return Err(ViError::OutOfMemory.into());
    }

    let mut guard = VM_REGISTRY.lock();
    if guard.is_none() {
        *guard = Some(BTreeMap::new());
    }
    let map = guard.as_mut().unwrap();
    let id = next_vm_id();
    #[cfg(feature = "p01-trace")]
    let _semantic = crate::memory::heap::p01_owner::creation(
        None,
        crate::memory::heap::p01_owner::SERVICE_SHARED,
    );
    map.insert(
        VmKey::new(owner, id),
        SvmVm {
            npt,
            guest_pa,
            guest_pages,
            apic_frame,
            vcpus: Vec::new(),
            vcpu_frames: Vec::new(),
        },
    );
    Ok(id)
}

/// Create a vCPU entering at GPA `entry_pc`; returns the 1-based `vcpu_id`.
///
/// The vCPU's control structures are the owner's guest charge, taken after
/// the VM is found and before the ASID or any frame: a refusal
/// (`OutOfMemory`) spends neither.
pub(crate) fn create_vcpu(owner: Charge, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    let key = VmKey::new(owner, vm_id);
    // Refuse an unknown VM before charging or consuming a non-reusable ASID.
    VM_REGISTRY
        .lock()
        .as_ref()
        .and_then(|map| map.get(&key))
        .ok_or(ViError::NotFound)?;
    let vmx = crate::arch::boot::cpu_features::x86_virt_kind()
        == Some(crate::arch::boot::cpu_features::X86Virt::Vmx);
    let pages = if vmx { VMX_VCPU_PAGES } else { SVM_VCPU_PAGES };
    let mut budget = Reservation::take(owner, Account::Guest, pages * PAGE_SIZE).map_err(|_| {
        log::warn!(
            "[hv-x86] create_vcpu refused: cell {}'s guest budget cannot cover {} pages",
            owner.cell().0,
            pages
        );
        ViError::OutOfMemory
    })?;
    if vmx {
        // Every early return below drops `vmcs`: VMCLEAR, then the frame
        // is freed (it leaked on these paths before, census §5).
        let vmcs = Vmcs(vm_structure(VMX_VCPU_PAGES, &mut budget)?);
        let mut guard = VM_REGISTRY.lock();
        let map = guard.as_mut().ok_or(ViError::NotFound)?;
        let vm = map.get_mut(&key).ok_or(ViError::NotFound)?;
        let vcpu_id = vm.vcpus.len() + 1;
        let host_cr3 = unsafe { hal::x86_64::paging::read_cr3() & !0xFFF };
        let vmcs_pa = vmcs.0.base();
        let mut vcpu = match unsafe {
            VmxVcpu::new(
                vmcs_pa as u64,
                phys_to_virt(vmcs_pa) as *mut u8,
                phys_to_virt(vm.guest_pa as usize) as *const u8,
                vm.guest_pages * PAGE_SIZE,
            )
        } {
            Ok(v) => v,
            Err(e) => {
                log::error!("[vmx] VmxVcpu::new failed: {:?}", e);
                return Err(e);
            }
        };
        if let Err(e) = unsafe { vcpu.init_pvh(entry_pc, 0, vm.npt.eptp(), host_cr3) } {
            log::error!("[vmx] init_pvh failed: {:?}", e);
            return Err(e);
        }
        vm.vcpus.push(X86Vcpu::Vmx(vcpu));
        vm.vcpu_frames.push(VcpuFrames::Vmx { vmcs });
        return Ok(vcpu_id);
    }

    // Reserve the ASID before any frame: exhaustion or an owner at its limit
    // refuses without allocating. A failure below cancels the lease, so the
    // owner's count returns; the ASID itself stays spent.
    let asid = SVM_ASIDS.acquire(owner).map_err(|refusal| {
        match refusal {
            TagRefusal::Exhausted => log::warn!(
                "[hv-x86] create_vcpu: ASIDs exhausted (max {})",
                SVM_ASIDS.capacity()
            ),
            TagRefusal::OwnerLimit => log::warn!(
                "[hv-x86] create_vcpu refused: cell {} generation {} already holds {} ASIDs",
                owner.cell().0,
                owner.generation(),
                SVM_ASIDS_PER_OWNER
            ),
        }
        ViError::OutOfMemory
    })?;

    // Allocate the SVM backing frames (VMCB + host-save + IOPM + MSRPM). An
    // early return below frees the ones already allocated (they leaked on
    // these paths before, census §5).
    let frames = vm_structure(1, &mut budget).and_then(|vmcb| {
        Ok((
            vmcb,
            vm_structure(1, &mut budget)?,
            vm_structure(2, &mut budget)?,
            vm_structure(3, &mut budget)?,
        ))
    });
    let (vmcb_run, host_run, msrpm_run, iopm_run) = match frames {
        Ok(frames) => frames,
        Err(error) => {
            asid.cancel();
            return Err(error);
        }
    };
    let (vmcb, host, msrpm, iopm) = (
        vmcb_run.base(),
        host_run.base(),
        msrpm_run.base(),
        iopm_run.base(),
    );

    // Zero the VMCB + host-save frames; fill the permission bitmaps with 0xFF
    // (intercept every MSR/port).
    // SAFETY: freshly allocated, exclusively owned frames; HHDM-mapped VAs.
    unsafe {
        core::ptr::write_bytes(phys_to_virt(vmcb) as *mut u8, 0, PAGE_SIZE);
        core::ptr::write_bytes(phys_to_virt(host) as *mut u8, 0, PAGE_SIZE);
        core::ptr::write_bytes(phys_to_virt(msrpm) as *mut u8, 0xFF, 2 * PAGE_SIZE);
        core::ptr::write_bytes(phys_to_virt(iopm) as *mut u8, 0xFF, 3 * PAGE_SIZE);
        // Passthrough the guest-context MSRs (SYSCALL/SYSENTER targets, segment
        // bases) so a booting Linux guest sets its own syscall entry without a
        // VMM round-trip; every other MSR stays intercepted + stubbed.
        hal::x86_64::svm::msrpm_passthrough_boot(phys_to_virt(msrpm) as *mut u8);
    }

    let mut guard = VM_REGISTRY.lock();
    let Some(vm) = guard.as_mut().and_then(|map| map.get_mut(&key)) else {
        asid.cancel();
        return Err(ViError::NotFound);
    };
    let vcpu_id = vm.vcpus.len() + 1;

    let ncr3 = vm.npt.ncr3();
    let vmcb_va = phys_to_virt(vmcb) as *mut u8;
    let apic_va = phys_to_virt(vm.apic_frame.base()) as *mut u8;
    // SAFETY: frames freshly allocated + zeroed/filled above; all outlive the
    // vCPU (owned by this VM entry's `vcpu_frames`, which drop after `vcpus`,
    // and freed only when the VM drops). apic_va is the
    // live VA of the RAM-backed xAPIC frame (create_vm maps + pre-populates it).
    let vcpu = unsafe {
        SvmVcpu::new(
            vmcb_va,
            vmcb as u64,
            host as u64,
            entry_pc,
            ncr3,
            asid.value(),
            0, // gdt_gpa — smoke/PVH guest does not reload segments in the MVP
            iopm as u64,
            msrpm as u64,
            apic_va,
            phys_to_virt(vm.guest_pa as usize) as *const u8,
            vm.guest_pages * PAGE_SIZE,
        )
    };
    vm.vcpus.push(X86Vcpu::Svm(vcpu));
    vm.vcpu_frames.push(VcpuFrames::Svm {
        vmcb: vmcb_run,
        host: host_run,
        msrpm: msrpm_run,
        iopm: iopm_run,
        asid,
    });
    Ok(vcpu_id)
}

/// Guest-RAM window: GPA 0 and the carve size in bytes.
pub(crate) fn guest_window(owner: Charge, vm_id: usize) -> ViResult<(u64, usize)> {
    let guard = VM_REGISTRY.lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let bytes = vm
        .guest_pages
        .checked_mul(PAGE_SIZE)
        .ok_or(ViError::InvalidInput)?;
    Ok((0, bytes))
}

/// Extend the guest NPT mapping to cover `[gpa, gpa+size)`, backed by the
/// start of the guest carve. The table refuses any range larger than the
/// carve, and a refused map changes nothing.
pub(crate) fn map_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    let pages = size.div_ceil(PAGE_SIZE);
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    vm.npt
        .map(gpa, vm.guest_pa, pages, writable)
        .map_err(|error| match error {
            EptMapError::OutOfMemory => ViError::OutOfMemory,
            _ => ViError::InvalidInput,
        })
}

/// Make this CPU's EPT translations for `eptp` current before VM entry.
///
/// `INVEPT` is CPU-local and the VMM task may migrate between runs, so an
/// unmap or a freed-and-reused root invalidated elsewhere could still be
/// cached here. Invalidating on the entering CPU, with interrupts masked up to
/// the entry, closes that for every CPU (the SVM path flushes its ASID on
/// every VMRUN for the same reason).
fn invalidate_ept_on_this_cpu(eptp: u64) -> ViResult<()> {
    if !crate::cpu_features::has_x86_virt() {
        return Err(ViError::NotSupported);
    }
    // SAFETY: `has_x86_virt` means every online CPU completed VMXON (an AP that
    // fails it never comes online), and only the VMX backend builds VMX vCPUs.
    unsafe { hal::x86_64::invept::invalidate_context(eptp) }
}

/// World-switch into a vCPU and return the HAL exit. Runs with interrupts
/// masked (the `svm_vmrun` GS.base window requires IF=0).
pub(crate) fn run_vcpu_hal(owner: Charge, vm_id: usize, vcpu_id: usize) -> ViResult<HalVmExit> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let eptp = vm.npt.eptp();
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;

    let if_was_set = interrupts_enabled();
    // SAFETY: cli masks IRQs for the whole VMRUN→VMLOAD window (svm_vmrun
    // contract — no host gs: access while the guest GS.base is loaded).
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
    if matches!(vcpu, X86Vcpu::Vmx(_)) {
        if let Err(error) = invalidate_ept_on_this_cpu(eptp) {
            if if_was_set {
                // SAFETY: restore the caller's interrupt-enable state.
                unsafe {
                    core::arch::asm!("sti", options(nomem, nostack));
                }
            }
            log::error!("[hv-x86] INVEPT before entry failed: {:?}", error);
            return Err(error);
        }
    }
    // SAFETY: SVM root operation is active (P01 latched has_x86_virt); IF=0.
    let exit = unsafe { vcpu.run() };
    if matches!(exit, HalVmExit::Shutdown) {
        let (rip, rflags, cr0) = vcpu.shutdown_diagnostics();
        log::error!(
            "[hv-x86] guest triple-fault: rip={:#x} rflags={:#x} cr0={:#x}",
            rip,
            rflags,
            cr0
        );
    }
    // 0x400: SVM NPF; 48: VMX EPT violation (info1 is then the exit qualification).
    if matches!(exit, HalVmExit::Unknown { ec: 0x400 | 48, .. }) {
        let (info1, gpa, rip) = vcpu.npf_diagnostics();
        log::error!(
            "[hv-x86] unsupported NPF: info1={:#x} gpa={:#x} rip={:#x}",
            info1,
            gpa,
            rip
        );
    }
    if if_was_set {
        // SAFETY: restore the caller's interrupt-enable state.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
    Ok(exit)
}

/// Copy `src` into guest RAM at GPA `gpa` (GPA base 0). Bounds-checked
/// against the carved guest-RAM window before any byte is written.
pub(crate) fn write_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    src: &[u8],
) -> ViResult<usize> {
    let guard = VM_REGISTRY.lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let len = src.len();
    let offset = gpa as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }
    let mut copied = 0;
    while copied < len {
        let current_gpa = gpa + copied as u64;
        let page_remaining = PAGE_SIZE - current_gpa as usize % PAGE_SIZE;
        let chunk = (len - copied).min(page_remaining);
        let hpa = vm.npt.translate(current_gpa).ok_or(ViError::InvalidInput)?;
        // SAFETY: the software NPT walk resolves this GPA to kernel-owned guest
        // RAM; `chunk` stays in one page; `src` is a kernel slice.
        unsafe {
            core::ptr::copy_nonoverlapping(
                src[copied..].as_ptr(),
                phys_to_virt(hpa as usize) as *mut u8,
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(len)
}

/// Copy guest RAM at GPA `gpa` into `dst`. Bounds-checked like
/// [`write_guest_memory`].
pub(crate) fn read_guest_memory(
    owner: Charge,
    vm_id: usize,
    gpa: u64,
    dst: &mut [u8],
) -> ViResult<usize> {
    let guard = VM_REGISTRY.lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map
        .get(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let len = dst.len();
    let offset = gpa as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }
    let mut copied = 0;
    while copied < len {
        let current_gpa = gpa + copied as u64;
        let page_remaining = PAGE_SIZE - current_gpa as usize % PAGE_SIZE;
        let chunk = (len - copied).min(page_remaining);
        let hpa = vm.npt.translate(current_gpa).ok_or(ViError::InvalidInput)?;
        // SAFETY: the software NPT walk resolves this GPA to kernel-owned guest
        // RAM; `chunk` stays in one page; `dst` is a kernel slice.
        unsafe {
            core::ptr::copy_nonoverlapping(
                phys_to_virt(hpa as usize) as *const u8,
                dst[copied..].as_mut_ptr(),
                chunk,
            );
        }
        copied += chunk;
    }
    Ok(len)
}

/// Read/write vCPU GPRs (16×u64, x86 register-number indexed) in the first 16
/// words of `regs`; the remaining words are not modelled and left untouched.
pub(crate) fn vcpu_regs(
    owner: Charge,
    vm_id: usize,
    vcpu_id: usize,
    regs: &mut [u64; 32],
    write: bool,
) -> ViResult<usize> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;
    if write {
        vcpu.gpr_mut().copy_from_slice(&regs[..16]);
    } else {
        regs[..16].copy_from_slice(vcpu.gpr());
    }
    Ok(0)
}

/// Queue an external-interrupt `vector` for delivery into the vCPU on its next
/// `VMRUN` (SVM EVENTINJ). Used for the emulated 8259 IRQ lines (e.g. PIT IRQ0).
/// Gated on the guest interrupt window: a Preempted exit can catch the guest in
/// an IRQ-off section (or pre-IDT boot), where a forced EVENTINJ — which skips
/// the hardware IF check — would corrupt it. A closed window drops the tick.
pub(crate) fn inject_irq(owner: Charge, vm_id: usize, vcpu_id: usize, vector: u32) -> ViResult<()> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map
        .get_mut(&VmKey::new(owner, vm_id))
        .ok_or(ViError::NotFound)?;
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;
    vcpu.inject_ext_irq_gated(vector as u8);
    Ok(())
}

/// Reclaim every x86 VM `owner` holds: its tables, guest RAM, control
/// structures and guest charge. Called once per incarnation from the
/// root-retirement funnel, after every member quiesced.
pub(crate) fn reap_vms_for(owner: Charge) {
    let dead: Vec<SvmVm> = {
        let mut guard = VM_REGISTRY.lock();
        let Some(map) = guard.as_mut() else { return };
        let keys: Vec<VmKey> = map
            .keys()
            .filter(|key| key.owned_by(owner))
            .copied()
            .collect();
        keys.iter().filter_map(|k| map.remove(k)).collect()
    };
    drop(dead); // each SvmVm field frees its own frames and refunds their charge
}

fn interrupts_enabled() -> bool {
    let rflags: u64;
    // SAFETY: pushfq/pop reads RFLAGS without modifying visible state.
    unsafe {
        core::arch::asm!("pushfq", "pop {}", out(reg) rflags, options(nomem));
    }
    rflags & (1 << 9) != 0
}

/// M1 bare-metal smoke: create a 1 MiB guest running the port-out-'K'+HLT blob,
/// world-switch it, and assert the decoded exits. Logs `X86-VMM-SMOKE: PASS`.
#[cfg(feature = "test-hooks")]
pub fn x86_smoke() {
    /// The kernel: its guest charge is never limited or counted.
    const OWNER: Charge = Charge::KERNEL;
    const ENTRY: u64 = 0x1000; // GPA (base 0)
    const GUEST_PAGES: usize = 256; // 1 MiB
    const BLOB: [u8; 8] = [0x66, 0xBA, 0xF8, 0x03, 0xB0, 0x4B, 0xEE, 0xF4];

    let vm_id = match create_vm(OWNER, GUEST_PAGES) {
        Ok(id) => id,
        Err(e) => {
            log::error!("[hv-x86] smoke: create_vm failed: {:?}", e);
            return;
        }
    };
    if write_guest_memory(OWNER, vm_id, ENTRY, &BLOB).is_err() {
        log::error!("[hv-x86] smoke: fixture write failed");
        reap_vms_for(OWNER);
        return;
    }
    if let Err(e) = create_vcpu(OWNER, vm_id, ENTRY) {
        log::error!("[hv-x86] smoke: create_vcpu failed: {:?}", e);
        reap_vms_for(OWNER);
        return;
    }

    // Drive the vCPU through the PUBLIC syscall-level path so the P04 HAL→API
    // ViVmExit conversion (VERSION 2) is exercised, not just the HAL decode.
    use abi::hypervisor::ViVmExit as ApiVmExit;
    let mut e = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut got_port = false;
    let mut got_hlt = false;
    for _ in 0..10 {
        run_smoke_until_exit(OWNER, vm_id, &mut e);
        match e {
            ApiVmExit::PortOut {
                port: 0x3F8, val, ..
            } if val == 0x4B => {
                got_port = true;
            }
            ApiVmExit::Hlt => {
                got_hlt = true;
                break;
            }
            _ => break,
        }
    }

    if got_port && got_hlt {
        log::info!("X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)");
    } else {
        log::error!(
            "X86-VMM-SMOKE: FAIL got_port={} got_hlt={} last_exit={:?}",
            got_port,
            got_hlt,
            e
        );
    }
    reap_vms_for(OWNER);
}

/// Executable NPF contract: decode one VirtIO MMIO read, service its destination
/// register, then prove exactly-once RIP advancement through PortOut + HLT.
#[cfg(feature = "x86-mmio-smoke")]
pub fn x86_mmio_smoke() {
    /// The kernel: its guest charge is never limited or counted.
    const OWNER: Charge = Charge::KERNEL;
    const ENTRY: u64 = 0x1000;
    const GUEST_PAGES: usize = 256;
    const MAGIC: u64 = 0x7472_6976;
    const BLOB: [u8; 15] = [
        0xBB, 0x00, 0x00, 0x00, 0xD0, // mov ebx, 0xd0000000
        0x8B, 0x0B, // mov ecx, [ebx]
        0x88, 0xC8, // mov al, cl
        0x66, 0xBA, 0xF8, 0x03, // mov dx, 0x3f8
        0xEE, // out dx, al
        0xF4, // hlt
    ];

    let Ok(vm_id) = create_vm(OWNER, GUEST_PAGES) else {
        log::error!("X86-MMIO-SMOKE: FAIL create_vm");
        return;
    };
    let mut observed = [0u8; BLOB.len()];
    if write_guest_memory(OWNER, vm_id, ENTRY, &BLOB).is_err()
        || read_guest_memory(OWNER, vm_id, ENTRY, &mut observed).is_err()
        || observed != BLOB
        || create_vcpu(OWNER, vm_id, ENTRY).is_err()
    {
        log::error!("X86-MMIO-SMOKE: FAIL fixture setup/readback");
        reap_vms_for(OWNER);
        return;
    }

    use abi::hypervisor::ViVmExit as ApiVmExit;
    let mut read = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut port = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut hlt = ApiVmExit::Unknown { ec: 0, iss: 0 };
    run_smoke_until_exit(OWNER, vm_id, &mut read);
    let read_ok = match read {
        ApiVmExit::MmioRead {
            ipa: 0xd000_0000,
            size: 4,
            reg,
        } if reg < 16 => {
            let mut regs = [0u64; 32];
            vcpu_regs(OWNER, vm_id, 1, &mut regs, false).is_ok() && {
                regs[reg as usize] = MAGIC;
                vcpu_regs(OWNER, vm_id, 1, &mut regs, true).is_ok()
            }
        }
        _ => false,
    };
    if read_ok {
        run_smoke_until_exit(OWNER, vm_id, &mut port);
        run_smoke_until_exit(OWNER, vm_id, &mut hlt);
    }

    let port_ok = matches!(
        port,
        ApiVmExit::PortOut {
            port: 0x3f8,
            val: 0x76,
            ..
        }
    );
    if read_ok && port_ok && matches!(hlt, ApiVmExit::Hlt) {
        log::info!("X86-MMIO-SMOKE: PASS (read+gpr+RIP)");
    } else {
        log::error!(
            "X86-MMIO-SMOKE: FAIL read={:?} port={:?} hlt={:?}",
            read,
            port,
            hlt
        );
    }
    reap_vms_for(OWNER);
}

/// Run the smoke vCPU until a non-`Preempted` exit. An external-interrupt exit
/// leaves the host IRQ pending, so each `Preempted` opens a one-instruction IF
/// window to service it; without that the next entry exits on it again.
#[cfg(any(feature = "test-hooks", feature = "x86-mmio-smoke"))]
fn run_smoke_until_exit(owner: Charge, vm_id: usize, exit: &mut abi::hypervisor::ViVmExit) {
    let interrupts_were_enabled = interrupts_enabled();
    for _ in 0..1024 {
        // `owner` owns this single-vCPU VM; a failed run keeps the last exit.
        if let Ok(next) = crate::hypervisor::registry::run_vcpu(owner, vm_id, 1, 0) {
            *exit = next;
        }
        if !matches!(exit, abi::hypervisor::ViVmExit::Preempted) {
            break;
        }
        // Service the pending host IRQ without parking or entering the scheduler;
        // STI defers delivery until after the following NOP.
        unsafe {
            core::arch::asm!("sti", "nop", "cli", options(nomem, nostack));
        }
    }
    if interrupts_were_enabled {
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
}
