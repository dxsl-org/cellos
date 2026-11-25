//! x86 PVH guest bring-up: load `vmlinux` + initramfs, build boot info, run.
//!
//! Mirrors the aarch64 boot flow in [`crate::main`] but for the PVH protocol:
//! guest RAM is GPA-base-0, the entry point comes from the ELF PVH note, and
//! the guest receives `hvm_start_info` in RBX rather than a DTB in x0.

extern crate alloc;

use crate::boot_x86_profile::{GuestProfile, CHROME_DISK_NAME, VMLINUX_PATH};
use crate::persistent_disk::{DiskSpec, PersistentDisk};
use crate::{
    acpi,
    boot_info::{self, BootInfoParams},
    loader_image_x86, run_loop_x86, vmm,
};
use alloc::vec::Vec;
use ostd::io::println;
use types::{ViError, ViResult};
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
    // A launcher-spawned hypervisor boots the guest at its window's size and
    // shows it there; without a launcher it fills the screen.
    let app_mode = crate::app_session_x86::app_mode();
    let session = if app_mode {
        match crate::app_session_x86::wait_for_launch() {
            Some(bound) => Some(bound),
            None => return,
        }
    } else {
        None
    };
    let profile = GuestProfile::select(app_mode);

    match boot(profile) {
        Some((vm_id, vcpus, disk)) => {
            println("[hv-x86] vCPU ready — entering run loop");
            run_loop_x86::run(vm_id, &vcpus, disk, session);
            println("[hv-x86] guest exited");
        }
        None => {
            if let Some((session, _, _)) = &session {
                session.report_exit(1);
            }
        }
    }
    if app_mode {
        // The launcher's window is gone or the guest stopped: this Cell has
        // nothing left to serve, and its exit reclaims the VM.
        ostd::syscall::sys_exit(0);
    }
}

/// Everything before the run loop: guest RAM, kernel, initramfs, vCPUs, ACPI,
/// boot info and disk. Returns the VM and its vCPU ids (the first one's
/// first); `None` after logging the failed step.
#[allow(clippy::type_complexity)]
fn boot(profile: GuestProfile) -> Option<(usize, Vec<usize>, Option<(PersistentDisk, DiskSpec)>)> {
    // Open the disk first: a missing Chromium image must fail before the
    // large guest-RAM carve.
    let disk = open_disk(profile)?;
    // The largest size the kernel can carve; a smaller one is said out loud,
    // since the guest then has less than its profile asks for.
    let sizes = profile.ram_sizes();
    let Some((vm_id, ram_size)) = sizes.iter().find_map(|&size| {
        let vm_id = vmm::create_vm((size / 4096) as usize);
        (vm_id != 0 && vm_id != usize::MAX).then_some((vm_id, size))
    }) else {
        // Causes, in likelihood order: HypervisorCap not granted to this cell
        // (CreateVm → PermissionDenied), SVM root not active, or no contiguous
        // guest-RAM run. The kernel logs the precise reason (`[hv-x86] …`).
        println("[hv-x86] create_vm failed — see kernel log (cap / SVM / OOM)");
        return None;
    };
    if ram_size != sizes[0] {
        println(&alloc::format!(
            "[hv-x86] guest RAM: {} MiB ({} MiB did not fit)",
            ram_size >> 20,
            sizes[0] >> 20
        ));
    }
    if vmm::map_guest_memory(vm_id, 0, ram_size as usize, true) == usize::MAX {
        println("[hv-x86] map_guest_memory failed");
        return None;
    }

    // ── Parse vmlinux ELF + PVH entry note ────────────────────────────────────
    let info = match loader_image_x86::parse_headers(VMLINUX_PATH) {
        Ok(i) => i,
        Err(e) => {
            println(&alloc::format!(
                "[hv-x86] parse {} failed: {:?} — need an uncompressed vmlinux with a PHYS32_ENTRY note",
                VMLINUX_PATH, e
            ));
            return None;
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
            return None;
        }
    };
    let kernel_end = info
        .loads
        .iter()
        .map(|s| s.paddr + s.memsz)
        .max()
        .unwrap_or(0);
    let initrd_gpa = (kernel_end + ALIGN_2M - 1) & !(ALIGN_2M - 1);
    let initrd_path = profile.initrd_path();
    let initrd_size = match stream_file(initrd_path, vm_id, initrd_gpa) {
        Ok(n) => n,
        Err(e) => {
            println(&alloc::format!(
                "[hv-x86] stream {} failed: {:?}",
                initrd_path,
                e
            ));
            return None;
        }
    };
    println(&alloc::format!(
        "[hv-x86] entry=0x{:x} kernel_end=0x{:x} initrd@0x{:x} ({} B)",
        entry32,
        kernel_end,
        initrd_gpa,
        initrd_size
    ));

    // ── vCPUs: the first at the PVH entry (RBX = hvm_start_info GPA); the
    //    others wait for the guest to start them. A kernel that gives fewer
    //    (SVM: one) settles the count before the MADT names them. ─────────────
    let first = vmm::create_vcpu(vm_id, entry32);
    if first == 0 || first == usize::MAX {
        println("[hv-x86] create_vcpu failed");
        return None;
    }
    let asked = usize::from(profile.vcpus());
    let mut vcpus = alloc::vec![first];
    while vcpus.len() < asked {
        let vcpu_id = vmm::create_vcpu(vm_id, 0);
        if vcpu_id == 0 || vcpu_id == usize::MAX {
            break;
        }
        vcpus.push(vcpu_id);
    }
    println(&alloc::format!(
        "[hv-x86] guest vCPUs: {} (asked {})",
        vcpus.len(),
        asked
    ));

    // ── ACPI tables (Alpine requires them; PVH passes rsdp via start_info) ────
    let (acpi_blob, rsdp_paddr) = acpi::build(vcpus.len() as u8);
    if wg(vm_id, acpi::ACPI_BASE_GPA, &acpi_blob).is_err() {
        println("[hv-x86] write ACPI tables failed");
        return None;
    }

    // ── Boot-info blob (hvm_start_info + e820 + initramfs module + cmdline) ───
    let (blob, start_info_gpa) = boot_info::build(&BootInfoParams {
        ram_size,
        initrd_gpa,
        initrd_size,
        cmdline: &profile.cmdline(),
        rsdp_paddr,
    });
    if wg(vm_id, start_info_gpa, &blob).is_err() {
        println("[hv-x86] write boot_info failed");
        return None;
    }
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, first, &mut rb, false);
    rb[3] = start_info_gpa; // RBX (x86 gpr index 3)
    vmm::vcpu_regs(vm_id, first, &mut rb, true);
    Some((vm_id, vcpus, disk))
}

/// The guest's only disk. Chrome always runs from its read-only image; the
/// other profiles follow the build's disk policy.
#[allow(clippy::type_complexity)]
fn open_disk(profile: GuestProfile) -> Option<Option<(PersistentDisk, DiskSpec)>> {
    let spec = match profile {
        GuestProfile::Chrome => DiskSpec {
            name: CHROME_DISK_NAME,
            read_only: true,
        },
        GuestProfile::Console | GuestProfile::Fixture => {
            #[cfg(feature = "volatile-disk")]
            {
                println("[hv-x86] volatile disk selected by build policy");
                return Some(None);
            }
            #[cfg(not(feature = "volatile-disk"))]
            crate::persistent_disk::GUEST_DISK
        }
    };
    match crate::persistent_disk::open(spec) {
        Ok(Some(disk)) => {
            println(&alloc::format!(
                "[hv-x86] persistent disk: /mnt/sd/{} (size={}{})",
                spec.name,
                disk.2,
                if spec.read_only { ", read-only" } else { "" }
            ));
            Some(Some((disk, spec)))
        }
        Ok(None) => {
            println("[hv-x86] persistent disk unavailable: VFS absent");
            None
        }
        Err(()) => {
            println(&alloc::format!(
                "[hv-x86] persistent disk open failed: /mnt/sd/{}",
                spec.name
            ));
            None
        }
    }
}
