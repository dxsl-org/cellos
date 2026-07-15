#![no_std]
#![no_main]

//! Hypervisor Service Cell — boots an ARM64 Linux guest via EL2 Stage-2 MMU.
//!
//! Reads vmlinuz + initrd.gz from VIFS1, builds a minimal DTB, maps 128 MiB
//! of guest RAM, loads all images, and runs the VmExit dispatch loop.

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
    Recv,
    Log,
    LookupService,
    // Kernel filesystem access (read vmlinuz + initrd)
    OpenCap,
    ReadCap,
    CloseCap,
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
];

mod dtb;
mod gicd;
mod loader_image;
mod net_backend;
mod pl011;
mod psci;
mod run_loop;
mod timer;
mod virtio_blk;
mod virtio_console;
mod virtio_mmio;
mod virtio_net;
mod virtqueue;
mod vmm;

use ostd::io::println;
use types::ViError;

/// Guest IPA base (1 GiB, must match registry.rs GUEST_IPA_BASE).
const GUEST_IPA_BASE: u64 = 0x4000_0000;
/// 128 MiB guest RAM.
const GUEST_RAM_SIZE: u64 = 128 * 1024 * 1024;
/// Page count for create_vm.
const GUEST_RAM_PAGES: usize = (GUEST_RAM_SIZE / 4096) as usize;

const VMLINUZ_PATH: &str = "/vmlinuz";
const INITRD_PATH: &str = "/initrd.gz";

#[no_mangle]
pub fn main() {
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
    guest.initrd_size = initrd_size;
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
    run_loop::run(vm_id, vcpu_id);

    println("[hv] guest exited");
}
