#![no_std]
#![no_main]

//! Hypervisor Service Cell — boots an ARM64 Linux guest via EL2 Stage-2 MMU.
//!
//! Reads vmlinuz + initrd.gz from VIFS1, builds a minimal DTB, maps 128 MiB
//! of guest RAM, loads all images, and runs the VmExit dispatch loop.
#[cfg(all(feature = "volatile-disk", feature = "hostile-backend-recovery"))]
compile_error!("volatile-disk cannot provide hostile backend recovery");

extern crate alloc;

// Manifest: requires HypervisorCap (allowlist bit 44).
api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = true
);

// Narrow syscall allowlist enforced by the kernel.
api::declare_syscalls![
    // IPC / service discovery
    Send,
    TrySend,
    Recv,
    RecvTimeout,
    Log,
    LookupService,
    // Kernel filesystem access (read vmlinuz + initrd)
    OpenCap,
    ReadCap,
    CloseCap,
    // Guest console input: drain the kernel UART RX ring (fd 0) into the
    // emulated 16550 RX FIFO (x86) / future PL011 RX (aarch64)
    Read,
    // Timer emulation
    GetTime,
    // VMM syscalls 220-227
    CreateVm,
    CreateVcpu,
    MapGuestMemory,
    WriteGuestMemory,
    RunVcpu,
    VcpuRegs,
    InjectIrq,
    ReadGuestMemory,
    // Scanout backing shared read-only with the compositor.
    GrantRegister,
    GrantShare,
    GrantSlice,
    GrantUnregister,
    GpuGetResolution,
    WaitForEvent,
];

// VMM syscall wrappers — only the two arches with a kernel VMM backend have a
// caller; on any other target every wrapper would be dead code.
#[cfg(all(
    feature = "hostile-backend-recovery",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[path = "backend-fault-control.rs"]
mod backend_fault_control;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod vmm;

// ── aarch64 (EL2) personality ─────────────────────────────────────────────────
#[cfg(target_arch = "aarch64")]
mod dtb;
#[cfg(target_arch = "aarch64")]
mod gicd;
#[cfg(target_arch = "aarch64")]
mod loader_image;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod net_backend;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod persistent_disk;
#[cfg(target_arch = "aarch64")]
mod pl011;
#[cfg(target_arch = "aarch64")]
mod psci;
#[cfg(target_arch = "aarch64")]
mod run_loop;
#[cfg(target_arch = "aarch64")]
mod timer;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtio_blk;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtio_console;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtio_gpu;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtio_mmio;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtio_net;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtqueue;
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
mod virtqueue_guard;
// ── x86_64 (SVM/VT-x) personality ──────────────────────────────────────────────
#[cfg(target_arch = "x86_64")]
mod acpi;
#[cfg(target_arch = "x86_64")]
mod boot_info;
#[cfg(target_arch = "x86_64")]
mod boot_x86;
#[cfg(target_arch = "x86_64")]
mod cmos_rtc;
#[cfg(target_arch = "x86_64")]
mod loader_image_x86;
#[cfg(target_arch = "x86_64")]
mod pic_8259;
#[cfg(target_arch = "x86_64")]
mod pit_8253;
#[cfg(target_arch = "x86_64")]
mod run_loop_x86;
#[cfg(target_arch = "x86_64")]
mod uart_16550;

/// Entry: dispatch to the arch personality that has a VMM backend.
#[no_mangle]
pub fn main() -> ! {
    #[cfg(target_arch = "aarch64")]
    boot_arm();
    #[cfg(target_arch = "x86_64")]
    boot_x86::run();
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    ostd::io::println("[hv] no VMM personality for this architecture");
    quiesce()
}

fn quiesce() -> ! {
    ostd::io::println("[hv] service quiesced");
    loop {
        let _ = ostd::syscall::sys_wait_for_event(0, 0);
    }
}

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
fn boot_arm() {
    use ostd::io::println;
    use types::ViError;
    println("[hv] hypervisor service cell starting");

    // ── 1. Allocate guest VM ──────────────────────────────────────────────────
    let vm_id = vmm::create_vm(GUEST_RAM_PAGES);
    if vm_id == 0 || vm_id == usize::MAX {
        println("[hv] create_vm failed — not at EL2 or OOM");
        return;
    }
    println(&alloc::format!("[hv] VM created vm_id={}", vm_id));

    // ── 2. Map guest RAM (IPA 0x4000_0000 .. +128 MiB) ───────────────────────
    let ret = vmm::map_guest_memory(vm_id, GUEST_IPA_BASE, GUEST_RAM_SIZE as usize, true);
    if ret == usize::MAX {
        println("[hv] map_guest_memory failed");
        return;
    }

    // ── 3. Parse the ARM64 Image header + compute guest RAM layout ──────────
    // Layout math needs only the header, so the images are streamed straight
    // into guest RAM afterwards — buffering either file whole exceeds the
    // 8 MiB cell heap and OOM-kills the cell.
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

    // ── 4. Stream kernel + initramfs into guest RAM ──────────────────────────
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

    // Build the DTB now that initrd_gpa/size are known and write it in place.
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

    // ── 5. Create vCPU at kernel entry ───────────────────────────────────────
    let vcpu_id = vmm::create_vcpu(vm_id, guest.kernel_entry_gpa);
    if vcpu_id == 0 || vcpu_id == usize::MAX {
        println("[hv] create_vcpu failed");
        return;
    }

    // ── 6. Set initial vCPU state (ARM64 boot protocol) ──────────────────────
    // x0 = DTB GPA, x1-x3 = 0, PC = kernel_entry_gpa.
    let mut rb = [0u64; 32];
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, false); // read current state
    rb[0] = guest.dtb_gpa; // x0 = DTB physical address
    rb[1] = 0; // x1 reserved
    rb[2] = 0; // x2 reserved
    rb[3] = 0; // x3 reserved
    rb[31] = guest.kernel_entry_gpa; // PC
    vmm::vcpu_regs(vm_id, vcpu_id, &mut rb, true); // write back

    println("[hv] vCPU ready — entering run loop");
    // ── 7. Run ───────────────────────────────────────────────────────────────
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
    quiesce()
}
