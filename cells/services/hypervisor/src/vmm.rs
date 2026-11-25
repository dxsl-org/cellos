//! The hypervisor's VMM call surface: the kernel VMM wrappers from
//! [`ostd::vmm`] plus this cell's interrupt-routing policy. How long a vCPU
//! may run per call is each run loop's decision.
//!
//! Compiled only for the two arches with a kernel VMM backend — aarch64 and
//! x86_64; `main.rs` gates the module accordingly. The syscall `unsafe` lives
//! in `ostd::vmm`; this module is safe code.

#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub use ostd::vmm::guest_timer_regs;
pub use ostd::vmm::{
    create_vcpu, create_vm, map_guest_memory, read_guest_memory, run_vcpu, vcpu_regs,
    write_guest_memory, SCHED_TICK_BUDGET_NS,
};

/// DTB SPI specifiers are relative to the GIC SPI base (INTID 32). x86
/// devices raise their PIC lines through `x86_irq_dispatch` instead.
#[cfg(target_arch = "aarch64")]
pub const fn device_irq(spi: u32) -> u32 {
    spi + 32
}

/// Signal a device interrupt. On Pi the software GIC owns the pending state;
/// the run loop requests HCR_EL2.VI only for a deliverable interrupt.
pub fn inject_irq(vm_id: usize, vcpu_id: usize, intid: u32) -> usize {
    #[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
    {
        let _ = (vm_id, vcpu_id);
        crate::gicd::queue_device_irq(intid);
        0
    }
    #[cfg(not(all(target_arch = "aarch64", feature = "board-rpi3")))]
    ostd::vmm::inject_irq(vm_id, vcpu_id, intid)
}

/// Pi only: ask the kernel to raise the virtual IRQ line for the interrupt
/// the software GIC selected as deliverable.
#[cfg(all(target_arch = "aarch64", feature = "board-rpi3"))]
pub fn request_virtual_irq(vm_id: usize, vcpu_id: usize, intid: u32) -> usize {
    ostd::vmm::inject_irq(vm_id, vcpu_id, intid)
}
