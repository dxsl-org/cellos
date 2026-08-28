//! x86 PVH guest bring-up: load `vmlinux` + initramfs, build boot info, run.
//!
//! Mirrors the aarch64 boot flow in [`crate::main`] but for the PVH protocol:
//! guest RAM is GPA-base-0, the entry point comes from the ELF PVH note, and
//! the guest receives `hvm_start_info` in RBX rather than a DTB in x0.

extern crate alloc;

use crate::{
    acpi,
    boot_info::{self, BootInfoParams},
    loader_image_x86, run_loop_x86, vmm,
};
use ostd::io::println;
use types::{ViError, ViResult};

/// 128 MiB guest RAM (contiguous carve; matches the proven ARM cell size and
/// fits Alpine's initramfs boot). Larger carves fail the frame allocator's
/// contiguous request on the x86 memory map.
const GUEST_RAM_SIZE: u64 = 128 * 1024 * 1024;
const GUEST_RAM_PAGES: usize = (GUEST_RAM_SIZE / 4096) as usize;

const VMLINUX_PATH: &str = "/vmlinux";
const INITRD_PATH: &str = "/initrd.gz";
// `nox2apic`: keep the guest in xAPIC mode. x2APIC needs IRQ remapping (VT-d IR)
// this VMM does not emulate; instead the guest drives the LAPIC through the
// RAM-backed 0xFEE00000 MMIO window (kernel-mapped) and the timer is polled
// kernel-side on HLT. CPUID also clears ECX[21] so this is belt-and-braces.
//
// `lpj=4000000`: preset loops_per_jiffy so `calibrate_delay()` is skipped. Its
// `calibrate_delay_converge()` busy-waits for jiffies to advance, but the
// periodic tick is not wired until AFTER calibrate_delay in the x86 boot order,
// so without a preset it spins forever on this minimal timer platform.
//
// `nohz=off highres=off`: force the legacy PERIODIC tick via the boot
// clockevent (i8253 PIT) instead of tickless/high-res one-shot mode. In
// periodic mode the guest arms PIT channel 0, which the cell's real-timebase
// PIT + idle IRQ0 injection can feed; one-shot mode never arms a timer we can
// drive, so the idle loop HLTs forever waiting for a tick that never comes.
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic lpj=4000000 nohz=off highres=off rdinit=/bin/sh panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6";

/// 2 MiB alignment for the initramfs placement.
const ALIGN_2M: u64 = 0x20_0000;

/// Write `bytes` into guest RAM at `gpa`, mapping the VMM error sentinel to an
/// `Err` so the `?`-based boot flow can bail.
fn wg(vm_id: usize, gpa: u64, bytes: &[u8]) -> ViResult<()> {
    if vmm::write_guest_memory(vm_id, gpa, bytes) == usize::MAX {
        Err(ViError::IO)
    } else {
        Ok(())
    }
}

/// Stream a VIFS1 file sequentially into guest RAM at `gpa`; returns byte count.
fn stream_file(path: &str, vm_id: usize, gpa: u64) -> ViResult<u64> {
    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| ViError::NotFound)?;
    let mut chunk = alloc::vec![0u8; 256 * 1024];
    let mut off = 0u64;
    loop {
        match ostd::syscall::sys_read_cap(cap, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                wg(vm_id, gpa + off, &chunk[..n]).inspect_err(|_| {
                    ostd::syscall::sys_close_cap(cap);
                })?;
                off += n as u64;
            }
            Err(_) => {
                ostd::syscall::sys_close_cap(cap);
                return Err(ViError::IO);
            }
        }
    }
    ostd::syscall::sys_close_cap(cap);
    Ok(off)
}

pub fn run() {
    println("[hv-x86] hypervisor cell starting (SVM PVH)");

    let vm_id = vmm::create_vm(GUEST_RAM_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        // Causes, in likelihood order: HypervisorCap not granted to this cell
        // (CreateVm → PermissionDenied), SVM root not active, or no contiguous
        // guest-RAM run. The kernel logs the precise reason (`[hv-x86] …`).
        println("[hv-x86] create_vm failed — see kernel log (cap / SVM / OOM)");
        return;
    }
    if vmm::map_guest_memory(vm_id, 0, GUEST_RAM_SIZE as usize, true) == usize::MAX {
        println("[hv-x86] map_guest_memory failed");
        return;
    }

    // ── Parse vmlinux ELF + PVH entry note ────────────────────────────────────
    let info = match loader_image_x86::parse_headers(VMLINUX_PATH) {
        Ok(i) => i,
        Err(e) => {
            println(&alloc::format!(
                "[hv-x86] parse {} failed: {:?} — need an uncompressed vmlinux with a PHYS32_ENTRY note",
                VMLINUX_PATH, e
            ));
            return;
        }
    };

    // ── Load PT_LOAD segments (and resolve the PVH entry from the note in the
    //    same pass), then place the initramfs above the kernel image ──────────
    let entry32 = match loader_image_x86::load_segments(VMLINUX_PATH, &info, |gpa, bytes| {
        wg(vm_id, gpa, bytes)
    }) {
        Ok(e) => e,
        Err(e) => {
            println(&alloc::format!(
                "[hv-x86] load_segments failed: {:?} (no PHYS32_ENTRY note? need uncompressed vmlinux)",
                e
            ));
            return;
        }
    };
    let kernel_end = info
        .loads
        .iter()
        .map(|s| s.paddr + s.memsz)
        .max()
        .unwrap_or(0);
    let initrd_gpa = (kernel_end + ALIGN_2M - 1) & !(ALIGN_2M - 1);
    let initrd_size = match stream_file(INITRD_PATH, vm_id, initrd_gpa) {
        Ok(n) => n,
        Err(e) => {
            println(&alloc::format!(
                "[hv-x86] stream {} failed: {:?}",
                INITRD_PATH,
                e
            ));
            return;
        }
    };
    println(&alloc::format!(
        "[hv-x86] entry=0x{:x} kernel_end=0x{:x} initrd@0x{:x} ({} B)",
        entry32,
        kernel_end,
        initrd_gpa,
        initrd_size
    ));

    // ── ACPI tables (Alpine requires them; PVH passes rsdp via start_info) ────
    let (acpi_blob, rsdp_paddr) = acpi::build();
    if wg(vm_id, acpi::ACPI_BASE_GPA, &acpi_blob).is_err() {
        println("[hv-x86] write ACPI tables failed");
        return;
    }

    // ── Boot-info blob (hvm_start_info + e820 + initramfs module + cmdline) ───
    let (blob, start_info_gpa) = boot_info::build(&BootInfoParams {
        ram_size: GUEST_RAM_SIZE,
        initrd_gpa,
        initrd_size,
        cmdline: CMDLINE,
        rsdp_paddr,
    });
    if wg(vm_id, start_info_gpa, &blob).is_err() {
        println("[hv-x86] write boot_info failed");
        return;
    }

    // ── vCPU at the PVH entry; RBX = hvm_start_info GPA ───────────────────────
    let vcpu_id = vmm::create_vcpu(vm_id, entry32);
    if vcpu_id == 0 || vcpu_id == usize::MAX {
        println("[hv-x86] create_vcpu failed");
        return;
    }
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
    rb[3] = start_info_gpa; // RBX (x86 gpr index 3)
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);

    let persistent_disk = crate::persistent_disk::open();
    if persistent_disk.is_some() {
        println("[hv-x86] persistent disk: /mnt/sd/guest_disk.img");
    } else {
        println("[hv-x86] volatile disk fallback");
    }
    println("[hv-x86] vCPU ready — entering run loop");
    run_loop_x86::run(vm_id, vcpu_id, persistent_disk);
    println("[hv-x86] guest exited");
}
