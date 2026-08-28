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
use core::sync::atomic::{AtomicU32, Ordering};

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
/// ASID 0 is reserved. Never reuse an ASID during a boot: stale nested-TLB
/// translations would otherwise let a new VM execute a previous VM's GPA map.
static NEXT_ASID: AtomicU32 = AtomicU32::new(1);

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
        if let Some(allocator) = FRAME_ALLOCATOR.lock().as_mut() {
            allocator.deallocate_frame(apic_frame);
        }
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

    let ncr3 = vm.npt.ncr3();
    let asid = NEXT_ASID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current < u32::MAX).then_some(current + 1)
        })
        .map_err(|_| ViError::OutOfMemory)?;
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
            asid,
            0, // gdt_gpa — smoke/PVH guest does not reload segments in the MVP
            iopm as u64,
            msrpm as u64,
            apic_va,
            phys_to_virt(vm.guest_pa as usize) as *const u8,
            vm.guest_pages * PAGE_SIZE,
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
    if matches!(exit, HalVmExit::Shutdown) {
        let (rip, rflags, cr0) = vcpu.shutdown_diagnostics();
        log::error!(
            "[hv-x86] guest triple-fault: rip={:#x} rflags={:#x} cr0={:#x}",
            rip,
            rflags,
            cr0
        );
    }
    if matches!(exit, HalVmExit::Unknown { ec: 0x400, .. }) {
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
    let mut copied = 0;
    while copied < len {
        let current_gpa = gpa + copied as u64;
        let page_remaining = PAGE_SIZE - current_gpa as usize % PAGE_SIZE;
        let chunk = (len - copied).min(page_remaining);
        let hpa = vm.npt.translate(current_gpa).ok_or(ViError::InvalidInput)?;
        // SAFETY: the software NPT walk resolves this GPA to kernel-owned guest
        // RAM; the syscall layer validated `src_ptr`; `chunk` stays in one page.
        unsafe {
            core::ptr::copy_nonoverlapping(
                (src_ptr + copied) as *const u8,
                phys_to_virt(hpa as usize) as *mut u8,
                chunk,
            );
        }
        copied += chunk;
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
    let mut copied = 0;
    while copied < len {
        let current_gpa = gpa + copied as u64;
        let page_remaining = PAGE_SIZE - current_gpa as usize % PAGE_SIZE;
        let chunk = (len - copied).min(page_remaining);
        let hpa = vm.npt.translate(current_gpa).ok_or(ViError::InvalidInput)?;
        // SAFETY: the software NPT walk resolves this GPA to kernel-owned guest
        // RAM; the syscall layer validated `dst_ptr`; `chunk` stays in one page.
        unsafe {
            core::ptr::copy_nonoverlapping(
                phys_to_virt(hpa as usize) as *const u8,
                (dst_ptr + copied) as *mut u8,
                chunk,
            );
        }
        copied += chunk;
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
    const BLOB: [u8; 8] = [0x66, 0xBA, 0xF8, 0x03, 0xB0, 0x4B, 0xEE, 0xF4];

    let vm_id = match create_vm(OWNER, GUEST_PAGES) {
        Ok(id) => id,
        Err(e) => {
            log::error!("[hv-x86] smoke: create_vm failed: {:?}", e);
            return;
        }
    };
    if write_guest_memory(OWNER, vm_id, ENTRY, BLOB.as_ptr() as usize, BLOB.len()).is_err() {
        log::error!("[hv-x86] smoke: fixture write failed");
        reap_vms_for_task(OWNER);
        return;
    }
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

/// Executable NPF contract: decode one VirtIO MMIO read, service its destination
/// register, then prove exactly-once RIP advancement through PortOut + HLT.
#[cfg(feature = "x86-mmio-smoke")]
pub fn x86_mmio_smoke() {
    const OWNER: usize = 0xF00E;
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
    if write_guest_memory(OWNER, vm_id, ENTRY, BLOB.as_ptr() as usize, BLOB.len()).is_err()
        || read_guest_memory(
            OWNER,
            vm_id,
            ENTRY,
            observed.as_mut_ptr() as usize,
            observed.len(),
        )
        .is_err()
        || observed != BLOB
        || create_vcpu(OWNER, vm_id, ENTRY).is_err()
    {
        log::error!("X86-MMIO-SMOKE: FAIL fixture setup/readback");
        reap_vms_for_task(OWNER);
        return;
    }

    use api::hypervisor::ViVmExit as ApiVmExit;
    let mut read = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut port = ApiVmExit::Unknown { ec: 0, iss: 0 };
    let mut hlt = ApiVmExit::Unknown { ec: 0, iss: 0 };
    run_mmio_smoke_until_exit(OWNER, vm_id, &mut read);
    let read_ok = match read {
        ApiVmExit::MmioRead {
            ipa: 0xd000_0000,
            size: 4,
            reg,
        } if reg < 16 => {
            let mut regs = [0u64; 32];
            vcpu_regs(OWNER, vm_id, 1, regs.as_mut_ptr() as usize, false).is_ok() && {
                regs[reg as usize] = MAGIC;
                vcpu_regs(OWNER, vm_id, 1, regs.as_mut_ptr() as usize, true).is_ok()
            }
        }
        _ => false,
    };
    if read_ok {
        run_mmio_smoke_until_exit(OWNER, vm_id, &mut port);
        run_mmio_smoke_until_exit(OWNER, vm_id, &mut hlt);
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
    reap_vms_for_task(OWNER);
}

#[cfg(feature = "x86-mmio-smoke")]
fn run_mmio_smoke_until_exit(owner: usize, vm_id: usize, exit: &mut api::hypervisor::ViVmExit) {
    let interrupts_were_enabled = interrupts_enabled();
    for _ in 0..1024 {
        // SAFETY: the local exit slot is valid; `owner` owns this single-vCPU VM.
        unsafe {
            let _ = super::registry::run_vcpu(owner, vm_id, 1, 0, exit);
        }
        if !matches!(exit, api::hypervisor::ViVmExit::Preempted) {
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
