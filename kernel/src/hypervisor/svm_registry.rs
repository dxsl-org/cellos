//! x86 SVM VM store + run loop (Tier 3b P03) — the x86 twin of the aarch64
//! branch in [`super::registry`].
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

use crate::memory::ept::{NestedFormat, NestedPageTable};
use crate::memory::frame::{phys_to_virt, FRAME_ALLOCATOR};
use crate::memory::paging::PAGE_SIZE;
use crate::sync::Spinlock;
use hal::x86_64::svm_vcpu::SvmVcpu;
use hal::ViVmExit as HalVmExit;
use types::{ViError, ViResult};

/// Physical frames backing one vCPU's SVM structures, freed on VM teardown.
struct VcpuFrames {
    vmcb: usize,
    host: usize,
    msrpm: usize, // 2 contiguous pages
    iopm: usize,  // 3 contiguous pages
}

/// GPA of the guest LAPIC MMIO window (x86 architectural default).
const APIC_GPA: u64 = 0xFEE0_0000;

struct SvmVm {
    npt: NestedPageTable,
    guest_pa: u64,
    guest_pages: usize,
    /// Kernel frame backing the RAM-backed xAPIC window at [`APIC_GPA`]. Guest
    /// LAPIC MMIO hits this frame directly (no trap); the timer is polled from
    /// it on HLT. Freed here in `Drop` (it is not tracked by the NPT, which only
    /// frees its own tree + guest RAM).
    apic_frame: usize,
    vcpus: Vec<SvmVcpu>,
    vcpu_frames: Vec<VcpuFrames>,
}

impl Drop for SvmVm {
    fn drop(&mut self) {
        let mut g = FRAME_ALLOCATOR.lock();
        if let Some(a) = g.as_mut() {
            for f in &self.vcpu_frames {
                a.deallocate_frame(f.vmcb);
                a.deallocate_frame(f.host);
                a.deallocate_frame(f.msrpm);
                a.deallocate_frame(f.msrpm + PAGE_SIZE);
                a.deallocate_frame(f.iopm);
                a.deallocate_frame(f.iopm + PAGE_SIZE);
                a.deallocate_frame(f.iopm + 2 * PAGE_SIZE);
            }
            if self.apic_frame != 0 {
                a.deallocate_frame(self.apic_frame);
            }
        }
        // `npt` frees its own tree + guest RAM in its Drop.
    }
}

static VM_REGISTRY: Spinlock<Option<BTreeMap<(usize, usize), SvmVm>>> = Spinlock::new(None);

/// The nested-paging format for this CPU (NPT on AMD/SVM, EPT on Intel/VMX).
fn nested_format() -> NestedFormat {
    match crate::cpu_features::x86_virt_kind() {
        Some(crate::cpu_features::X86Virt::Vmx) => NestedFormat::Ept,
        _ => NestedFormat::Npt,
    }
}

/// Allocate `n` contiguous frames and return the base PA (no zeroing).
fn alloc_contig(n: usize) -> Option<usize> {
    FRAME_ALLOCATOR.lock().as_mut()?.allocate_contiguous(n)
}

/// Allocate guest RAM + NPT, mapped at GPA 0; return the opaque `vm_id`.
pub fn create_vm(owner: usize, guest_pages: usize) -> ViResult<usize> {
    let mut npt = NestedPageTable::new(nested_format()).ok_or_else(|| {
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
    let apic_frame = alloc_contig(1).ok_or_else(|| {
        log::error!("[hv-x86] create_vm: xAPIC frame OOM");
        ViError::OutOfMemory
    })?;
    // SAFETY: freshly allocated, exclusively owned frame; HHDM-mapped VA. u32
    // stores are within the 4 KiB frame and naturally aligned (0x20 / 0x30).
    unsafe {
        let va = phys_to_virt(apic_frame) as *mut u8;
        core::ptr::write_bytes(va, 0, PAGE_SIZE);
        (va.add(0x20) as *mut u32).write_volatile(0); // APIC ID = 0
        (va.add(0x30) as *mut u32).write_volatile(0x0005_0014); // version 0x14, max-LVT 5
    }
    if npt
        .map_device_frame(APIC_GPA, apic_frame as u64, true)
        .is_err()
    {
        log::error!("[hv-x86] create_vm: xAPIC NPT map OOM");
        FRAME_ALLOCATOR
            .lock()
            .as_mut()
            .map(|a| a.deallocate_frame(apic_frame));
        return Err(ViError::OutOfMemory);
    }

    let mut guard = VM_REGISTRY.lock();
    if guard.is_none() {
        *guard = Some(BTreeMap::new());
    }
    let map = guard.as_mut().unwrap();
    let id = map.keys().filter(|(o, _)| *o == owner).count() + 1;
    map.insert(
        (owner, id),
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
/// Under `test-hooks`, writes an M1 smoke blob (`mov dx,0x3f8; mov al,'K';
/// out dx,al; hlt`) at the entry GPA so the test needs no cell-side guest RAM.
pub fn create_vcpu(owner: usize, vm_id: usize, entry_pc: u64) -> ViResult<usize> {
    // Allocate the SVM backing frames (VMCB + host-save + IOPM + MSRPM).
    let vmcb = alloc_contig(1).ok_or(ViError::OutOfMemory)?;
    let host = alloc_contig(1).ok_or(ViError::OutOfMemory)?;
    let msrpm = alloc_contig(2).ok_or(ViError::OutOfMemory)?;
    let iopm = alloc_contig(3).ok_or(ViError::OutOfMemory)?;

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
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    let vcpu_id = vm.vcpus.len() + 1;

    #[cfg(feature = "test-hooks")]
    {
        // GPA base is 0, so the guest-RAM offset equals entry_pc.
        const SMOKE_BLOB: [u8; 8] = [0x66, 0xBA, 0xF8, 0x03, 0xB0, 0x4B, 0xEE, 0xF4];
        let off = entry_pc as usize;
        if off + SMOKE_BLOB.len() <= vm.guest_pages * PAGE_SIZE {
            // SAFETY: guest RAM is kernel-allocated; no vCPU runs yet. guest_pa
            // is PHYSICAL — map through HHDM to a kernel VA before writing (on
            // x86 phys ≠ virt, unlike ARM's identity map).
            unsafe {
                let dst = phys_to_virt(vm.guest_pa as usize + off) as *mut u8;
                core::ptr::copy_nonoverlapping(SMOKE_BLOB.as_ptr(), dst, SMOKE_BLOB.len());
            }
        }
    }

    let ncr3 = vm.npt.ncr3();
    let vmcb_va = phys_to_virt(vmcb) as *mut u8;
    let apic_va = phys_to_virt(vm.apic_frame) as *mut u8;
    // SAFETY: frames freshly allocated + zeroed/filled above; all outlive the
    // vCPU (owned by this VM entry, freed only in SvmVm::drop). apic_va is the
    // live VA of the RAM-backed xAPIC frame (create_vm maps + pre-populates it).
    let vcpu = unsafe {
        SvmVcpu::new(
            vmcb_va,
            vmcb as u64,
            host as u64,
            entry_pc,
            ncr3,
            0, // gdt_gpa — smoke/PVH guest does not reload segments in the MVP
            iopm as u64,
            msrpm as u64,
            apic_va,
        )
    };
    vm.vcpus.push(vcpu);
    vm.vcpu_frames.push(VcpuFrames {
        vmcb,
        host,
        msrpm,
        iopm,
    });
    Ok(vcpu_id)
}

/// Extend the guest NPT mapping to cover `[gpa, gpa+size)`.
pub fn map_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    size: usize,
    writable: bool,
) -> ViResult<()> {
    let pages = size.div_ceil(PAGE_SIZE);
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    vm.npt
        .map(gpa, vm.guest_pa, pages, writable)
        .map_err(|_| ViError::OutOfMemory)?;
    Ok(())
}

/// World-switch into a vCPU and return the HAL exit. Runs with interrupts
/// masked (the `svm_vmrun` GS.base window requires IF=0).
pub fn run_vcpu_hal(owner: usize, vm_id: usize, vcpu_id: usize) -> ViResult<HalVmExit> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
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
    // SAFETY: SVM root operation is active (P01 latched has_x86_virt); IF=0.
    let exit = unsafe { vcpu.run() };
    if if_was_set {
        // SAFETY: restore the caller's interrupt-enable state.
        unsafe {
            core::arch::asm!("sti", options(nomem, nostack));
        }
    }
    Ok(exit)
}

/// Copy `len` bytes from the caller's `src_ptr` into guest RAM at GPA `gpa`
/// (GPA base 0). Bounds-checked against the carved guest-RAM window.
pub fn write_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    src_ptr: usize,
    len: usize,
) -> ViResult<usize> {
    let guard = VM_REGISTRY.lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map.get(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    let offset = gpa as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }
    // SAFETY: guest RAM is kernel-allocated; src_ptr validated by the syscall
    // layer; no vCPU runs concurrently in a single-task cell. guest_pa is
    // PHYSICAL — map through HHDM (phys ≠ virt on x86).
    unsafe {
        core::ptr::copy_nonoverlapping(
            src_ptr as *const u8,
            phys_to_virt(vm.guest_pa as usize + offset) as *mut u8,
            len,
        );
    }
    Ok(len)
}

/// Copy `len` bytes from guest RAM at GPA `gpa` into the caller's `dst_ptr`.
pub fn read_guest_memory(
    owner: usize,
    vm_id: usize,
    gpa: u64,
    dst_ptr: usize,
    len: usize,
) -> ViResult<usize> {
    let guard = VM_REGISTRY.lock();
    let map = guard.as_ref().ok_or(ViError::NotFound)?;
    let vm = map.get(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    let offset = gpa as usize;
    let end = offset.checked_add(len).ok_or(ViError::InvalidInput)?;
    if end > vm.guest_pages * PAGE_SIZE {
        return Err(ViError::InvalidInput);
    }
    // SAFETY: guest RAM is kernel-allocated; dst_ptr validated by the syscall
    // layer; no vCPU runs concurrently in a single-task cell. guest_pa is
    // PHYSICAL — map through HHDM (phys ≠ virt on x86).
    unsafe {
        core::ptr::copy_nonoverlapping(
            phys_to_virt(vm.guest_pa as usize + offset) as *const u8,
            dst_ptr as *mut u8,
            len,
        );
    }
    Ok(len)
}

/// Read/write vCPU GPRs (16×u64, x86 register-number indexed).
pub fn vcpu_regs(
    owner: usize,
    vm_id: usize,
    vcpu_id: usize,
    buf_ptr: usize,
    write: bool,
) -> ViResult<usize> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;
    // SAFETY: buf_ptr validated by the syscall layer; SAS — same VA in kernel.
    let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u64, 16) };
    if write {
        vcpu.gpr.copy_from_slice(&buf[..16]);
    } else {
        buf[..16].copy_from_slice(&vcpu.gpr);
    }
    Ok(0)
}

/// Queue an external-interrupt `vector` for delivery into the vCPU on its next
/// `VMRUN` (SVM EVENTINJ). Used for the emulated 8259 IRQ lines (e.g. PIT IRQ0).
/// Gated on the guest interrupt window: a Preempted exit can catch the guest in
/// an IRQ-off section (or pre-IDT boot), where a forced EVENTINJ — which skips
/// the hardware IF check — would corrupt it. A closed window drops the tick.
pub fn inject_irq(owner: usize, vm_id: usize, vcpu_id: usize, vector: u32) -> ViResult<()> {
    let mut guard = VM_REGISTRY.lock();
    let map = guard.as_mut().ok_or(ViError::NotFound)?;
    let vm = map.get_mut(&(owner, vm_id)).ok_or(ViError::NotFound)?;
    let vcpu = vm
        .vcpus
        .get_mut(vcpu_id.saturating_sub(1))
        .ok_or(ViError::NotFound)?;
    vcpu.inject_ext_irq_gated(vector as u8);
    Ok(())
}

/// Reclaim all x86 VMs owned by `dead_tid` (task-exit / fault / watchdog).
pub fn reap_vms_for_task(dead_tid: usize) {
    let dead: Vec<SvmVm> = {
        let mut guard = VM_REGISTRY.lock();
        let Some(map) = guard.as_mut() else { return };
        let keys: Vec<(usize, usize)> = map
            .keys()
            .filter(|(o, _)| *o == dead_tid)
            .copied()
            .collect();
        keys.iter().filter_map(|k| map.remove(k)).collect()
    };
    drop(dead); // SvmVm::drop + NestedPageTable::drop free all frames
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
    const OWNER: usize = 0xF00D;
    const ENTRY: u64 = 0x1000; // GPA (base 0)
    const GUEST_PAGES: usize = 256; // 1 MiB

    let vm_id = match create_vm(OWNER, GUEST_PAGES) {
        Ok(id) => id,
        Err(e) => {
            log::error!("[hv-x86] smoke: create_vm failed: {:?}", e);
            return;
        }
    };
    if let Err(e) = create_vcpu(OWNER, vm_id, ENTRY) {
        log::error!("[hv-x86] smoke: create_vcpu failed: {:?}", e);
        reap_vms_for_task(OWNER);
        return;
    }

    // Drive the vCPU through the PUBLIC syscall-level path so the P04 HAL→API
    // ViVmExit conversion (VERSION 2) is exercised, not just the HAL decode.
    use api::hypervisor::ViVmExit as ApiVmExit;
    let mut e1 = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut e2 = ApiVmExit::Unknown { ec: 0, iss: 0 };
    // SAFETY: e1/e2 are valid local ViVmExit slots; OWNER owns this VM; single
    // task, no concurrent run. run_vcpu masks interrupts internally.
    unsafe {
        let _ = super::registry::run_vcpu(OWNER, vm_id, 1, 0, &mut e1 as *mut ApiVmExit);
        let _ = super::registry::run_vcpu(OWNER, vm_id, 1, 0, &mut e2 as *mut ApiVmExit);
    }

    let ok_port = matches!(e1, ApiVmExit::PortOut { port: 0x3F8, val, .. } if val == 0x4B);
    let ok_hlt = matches!(e2, ApiVmExit::Hlt);

    if ok_port && ok_hlt {
        log::info!("X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)");
    } else {
        log::error!("X86-VMM-SMOKE: FAIL e1={:?} e2={:?}", e1, e2);
    }

    reap_vms_for_task(OWNER);
}
