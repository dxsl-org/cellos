#![no_std]
#![no_main]

//! Hypervisor Service Cell — boots an ARM64 Linux guest via EL2 Stage-2 MMU.
//!
//! Reads vmlinuz + initrd.gz from VIFS1, builds a minimal DTB, maps 128 MiB
//! of guest RAM, loads all images, and runs the VmExit dispatch loop.
#[cfg(all(feature = "volatile-disk", feature = "hostile-backend-recovery"))]
compile_error!("volatile-disk cannot provide hostile backend recovery");
#[cfg(all(feature = "volatile-disk", feature = "ubuntu-wide-guest"))]
compile_error!("ubuntu-wide-guest requires the persistent /mnt/sd/guest_disk.img backend");
#[cfg(all(feature = "ubuntu-wide-guest", not(target_arch = "x86_64")))]
compile_error!("ubuntu-wide-guest is qualified only on the x86_64 PVH path");

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
    VfsMutate,
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
mod boot_arm;
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
mod boot_x86_profile;
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
    boot_arm::boot_arm();
    #[cfg(target_arch = "x86_64")]
    boot_x86::run();
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    ostd::io::println("[hv] no VMM personality for this architecture");
    quiesce()
}

pub(crate) fn quiesce() -> ! {
    ostd::io::println("[hv] service quiesced");
    loop {
        let _ = ostd::syscall::sys_wait_for_event(0, 0);
    }
}
