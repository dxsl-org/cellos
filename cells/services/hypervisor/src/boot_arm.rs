//! aarch64 Linux guest boot via EL2 Stage-2 MMU.

#[cfg(all(target_arch = "aarch64", not(feature = "volatile-disk")))]
use crate::persistent_disk;
#[cfg(target_arch = "aarch64")]
use crate::{dtb, loader_image, run_loop, vmm};

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
fn guest_file_size(path: &str) -> types::ViResult<u64> {
    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| types::ViError::NotFound)?;
    let size = ostd::syscall::sys_stat_cap(cap).map_err(|_| types::ViError::IO);
    ostd::syscall::sys_close_cap(cap);
    size
}

#[cfg(target_arch = "aarch64")]
pub fn boot_arm() {
    use ostd::io::println;
    use types::ViError;
    println("[hv] hypervisor service cell starting");

    // The embedded VIFS1 files are required. Check both paths before carving
    // 128 MiB from the host: a malformed/incomplete image must not look like
    // an EL2 or host-memory failure.
    let (text_offset, image_size) = match loader_image::read_image_header(VMLINUZ_PATH) {
        Ok(header) => header,
        Err(e) => {
            println(&alloc::format!(
                "[hv] guest image {} missing or invalid: {:?}",
                VMLINUZ_PATH,
                e
            ));
            return;
        }
    };
    let kernel_size = match guest_file_size(VMLINUZ_PATH) {
        Ok(size) => size,
        Err(e) => {
            println(&alloc::format!(
                "[hv] guest image {} unavailable: {:?}",
                VMLINUZ_PATH,
                e
            ));
            return;
        }
    };
    let initrd_size = match guest_file_size(INITRD_PATH) {
        Ok(size) if size > 0 => size,
        Ok(_) => {
            println("[hv] guest image /initrd.gz is empty");
            return;
        }
        Err(e) => {
            println(&alloc::format!(
                "[hv] guest image {} missing: {:?}",
                INITRD_PATH,
                e
            ));
            return;
        }
    };
    if text_offset > GUEST_RAM_SIZE || image_size.max(kernel_size) > GUEST_RAM_SIZE - text_offset {
        println("[hv] guest kernel does not fit in 128 MiB RAM");
        return;
    }
    let mut guest =
        loader_image::compute_layout(text_offset, image_size.max(kernel_size), GUEST_IPA_BASE);
    if initrd_size > GUEST_RAM_SIZE
        || guest
            .initrd_gpa
            .checked_add(initrd_size + 2 * 1024 * 1024)
            .is_none_or(|end| end > GUEST_IPA_BASE + GUEST_RAM_SIZE)
    {
        println("[hv] guest kernel/initrd/DTB do not fit in 128 MiB RAM");
        return;
    }

    let vm_id = vmm::create_vm(GUEST_RAM_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        println("[hv] create_vm failed — EL2 unavailable or host OOM allocating 128 MiB guest RAM");
        return;
    }
    println(&alloc::format!("[hv] VM created vm_id={}", vm_id));

    let ret = vmm::map_guest_memory(vm_id, GUEST_IPA_BASE, GUEST_RAM_SIZE as usize, true);
    if ret == usize::MAX {
        println("[hv] map_guest_memory failed — host OOM mapping guest RAM");
        return;
    }

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

    #[cfg(feature = "volatile-disk")]
    let persistent_disk = {
        println("[hv] volatile disk selected by build policy — no persistent guest disk");
        None
    };
    #[cfg(not(feature = "volatile-disk"))]
    let persistent_disk = match persistent_disk::open() {
        Ok(Some(disk)) => {
            println("[hv] persistent disk: /mnt/sd/guest_disk.img");
            Some(disk)
        }
        Ok(None) => {
            #[cfg(feature = "board-rpi3")]
            {
                println(
                    "[hv] persistent guest disk missing: VFS or /mnt/sd/guest_disk.img unavailable",
                );
                return;
            }
            #[cfg(not(feature = "board-rpi3"))]
            {
                println("[hv] volatile disk fallback");
                None
            }
        }
        Err(()) => {
            println("[hv] persistent guest disk unavailable: /mnt/sd/guest_disk.img");
            return;
        }
    };
    println("[hv] vCPU ready — entering run loop");
    run_loop::run(vm_id, vcpu_id, persistent_disk);

    println("[hv] guest exited");
    crate::quiesce()
}
