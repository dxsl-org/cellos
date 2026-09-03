//! aarch64 Linux guest boot via EL2 Stage-2 MMU.

#[cfg(target_arch = "aarch64")]
use crate::{dtb, loader_image, persistent_disk, run_loop, vmm};

/// Guest IPA base (1 GiB, must match registry.rs GUEST_IPA_BASE).
#[cfg(target_arch = "aarch64")]
const GUEST_IPA_BASE: u64 = 0x4000_0000;
/// 128 MiB guest RAM.
#[cfg(target_arch = "aarch64")]
const GUEST_RAM_SIZE: u64 = 128 * 1024 * 1024;
/// Page count for create_vm.
#[cfg(target_arch = "aarch64")]
const GUEST_RAM_PAGES: usize = (GUEST_RAM_SIZE / 4096) as usize;

#[cfg(target_arch = "aarch64")]
const VMLINUZ_PATH: &str = "/vmlinuz";
#[cfg(target_arch = "aarch64")]
const INITRD_PATH: &str = "/initrd.gz";

#[cfg(target_arch = "aarch64")]
pub fn boot_arm() {
    use ostd::io::println;
    use types::ViError;
    println("[hv] hypervisor service cell starting");

    let vm_id = vmm::create_vm(GUEST_RAM_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        println("[hv] create_vm failed — not at EL2 or OOM");
        return;
    }
    println(&alloc::format!("[hv] VM created vm_id={}", vm_id));

    let ret = vmm::map_guest_memory(vm_id, GUEST_IPA_BASE, GUEST_RAM_SIZE as usize, true);
    if ret == usize::MAX {
        println("[hv] map_guest_memory failed");
        return;
    }

    let (text_offset, image_size) = match loader_image::read_image_header(VMLINUZ_PATH) {
        Ok(h) => h,
        Err(e) => {
            println(&alloc::format!(
                "[hv] read {} header failed: {:?}",
                VMLINUZ_PATH,
                e
            ));
            return;
        }
    };
    let mut guest = loader_image::compute_layout(text_offset, image_size, GUEST_IPA_BASE);

    let write_guest = |gpa: u64, bytes: &[u8]| -> types::ViResult<()> {
        let r = vmm::write_guest_memory(vm_id, gpa, bytes);
        if r == usize::MAX {
            Err(ViError::IO)
        } else {
            Ok(())
        }
    };
    let kernel_size =
        match loader_image::stream_file_to_guest(VMLINUZ_PATH, guest.kernel_entry_gpa, write_guest)
        {
            Ok(n) => n,
            Err(e) => {
                println(&alloc::format!(
                    "[hv] stream {} failed: {:?}",
                    VMLINUZ_PATH,
                    e
                ));
                return;
            }
        };
    let initrd_size =
        match loader_image::stream_file_to_guest(INITRD_PATH, guest.initrd_gpa, write_guest) {
            Ok(n) => n,
            Err(e) => {
                println(&alloc::format!(
                    "[hv] stream {} failed: {:?}",
                    INITRD_PATH,
                    e
                ));
                return;
            }
        };
    guest.finalize_dtb_gpa(initrd_size);
    println(&alloc::format!(
        "[hv] kernel={} B  initrd={} B (streamed)",
        kernel_size,
        initrd_size
    ));

    let dtb_bytes = match dtb::build_dtb(
        GUEST_IPA_BASE,
        GUEST_RAM_SIZE,
        guest.initrd_gpa,
        guest.initrd_gpa + guest.initrd_size,
    ) {
        Ok(b) => b,
        Err(_) => {
            println("[hv] build_dtb failed");
            return;
        }
    };
    if vmm::write_guest_memory(vm_id, guest.dtb_gpa, &dtb_bytes) == usize::MAX {
        println("[hv] write DTB failed");
        return;
    }
    println(&alloc::format!(
        "[hv] DTB @ 0x{:x} ({} B)",
        guest.dtb_gpa,
        dtb_bytes.len()
    ));
    println(&alloc::format!(
        "[hv] kernel entry @ 0x{:x}",
        guest.kernel_entry_gpa
    ));

    let vcpu_id = vmm::create_vcpu(vm_id, guest.kernel_entry_gpa);
    if vcpu_id == 0 || vcpu_id == usize::MAX {
        println("[hv] create_vcpu failed");
        return;
    }

    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false);
    rb[0] = guest.dtb_gpa;
    rb[1] = 0;
    rb[2] = 0;
    rb[3] = 0;
    rb[31] = guest.kernel_entry_gpa;
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true);

    println("[hv] vCPU ready — entering run loop");
    let persistent_disk = match persistent_disk::open() {
        Ok(Some(disk)) => {
            println("[hv] persistent disk: /mnt/sd/guest_disk.img");
            Some(disk)
        }
        Ok(None) => {
            println("[hv] volatile disk fallback");
            None
        }
        Err(()) => {
            println("[hv] persistent disk unavailable");
            return;
        }
    };
    run_loop::run(vm_id, vcpu_id, persistent_disk);

    println("[hv] guest exited");
    crate::quiesce()
}
