//! Shared VirtIO MMIO slot enumeration.
//!
//! `virtio_blk`, `input_irq_ack`, and Driver Cells use `virtio_slots()` to
//! iterate all VirtIO MMIO slots for the current platform.
//!
//! AArch64: scans all 32 slots at 0x0a000000, stride 0x200 (QEMU virt layout).
//! QEMU assigns devices to slots in an implementation-defined order so we must
//! probe all 32.  The identity map in paging.rs covers the full 0x0a004000 range.
//!
//! RISC-V: reads DTB-confirmed slots from `platform::PLATFORM`.
//! PCIe-only arches expose no VirtIO-MMIO slots.

extern crate alloc;
use alloc::vec::Vec;

/// A VirtIO MMIO slot with base address and IRQ.
pub struct VirtioSlot {
    pub base: usize,
    pub irq: u32,
}

/// Iterator over all VirtIO MMIO slots for the current platform.
pub fn virtio_slots() -> impl Iterator<Item = VirtioSlot> {
    #[cfg(target_arch = "aarch64")]
    {
        // QEMU ARM virt: 32 VirtIO MMIO slots at 0x0a000000, 512 bytes each, SPI 16+i.
        // All 32 slots are identity-mapped by init_kernel_paging (0x0a000000..0x0a004000).
        const BASE: usize = 0x0a00_0000;
        const STRIDE: usize = 0x200;
        let slots: Vec<VirtioSlot> = (0..32_usize)
            .map(|i| VirtioSlot {
                base: BASE + i * STRIDE,
                irq: 16 + i as u32,
            })
            .collect();
        slots.into_iter()
    }
    #[cfg(target_arch = "riscv64")]
    {
        let slots: Vec<VirtioSlot> = crate::platform::with(|p| {
            p.virtio_mmio
                .iter()
                .filter_map(|e| {
                    e.as_ref().map(|e| VirtioSlot {
                        base: e.base,
                        irq: e.irq,
                    })
                })
                .collect()
        });
        slots.into_iter()
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    {
        Vec::new().into_iter()
    }
}

/// Execute one VirtIO MMIO ACK sequence with RV64 SUM enabled only for that scope.
///
/// On RV64, supervisor-mode MMIO ACK helpers may touch user-accessible identity-mapped
/// device pages while trap handling runs with SUM clear. This helper snapshots whether
/// SUM was already set, raises it only for `ack`, and restores the prior state
/// immediately afterwards. Other architectures execute `ack` unchanged.
pub(crate) fn with_riscv_sum_for_virtio_ack<T>(ack: impl FnOnce() -> T) -> T {
    #[cfg(target_arch = "riscv64")]
    {
        const SUM_MASK: usize = 1usize << 18;
        let sstatus: usize;
        // SAFETY: this helper is used only for short-lived VirtIO MMIO ACK sequences.
        // `csrr/csrs/csrc sstatus` are privileged RV64 supervisor CSR operations; the
        // code records the prior SUM bit, sets only SUM for the duration of `ack`, and
        // restores the exact prior SUM state before returning. It does not widen trap
        // entry globally or alter SSIE/SEIE/STIE.
        unsafe {
            core::arch::asm!("csrr {value}, sstatus", value = out(reg) sstatus, options(nostack));
            if sstatus & SUM_MASK == 0 {
                core::arch::asm!("csrs sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
            }
        }
        let result = ack();
        unsafe {
            if sstatus & SUM_MASK == 0 {
                core::arch::asm!("csrc sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
            }
        }
        result
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        ack()
    }
}

/// Read VirtIO `InterruptStatus` and ACK the exact bits it reports.
///
/// Returns the observed status bits; `0` means there was nothing pending to ACK.
pub(crate) fn ack_virtio_interrupt_status(mmio_base: usize) -> u32 {
    let status = with_riscv_sum_for_virtio_ack(|| {
        // SAFETY: caller provides a live, identity-mapped VirtIO MMIO base. The
        // status register is read-only, and the scoped SUM helper contains the
        // RV64 privilege widening to this one MMIO access sequence only.
        unsafe { core::ptr::read_volatile((mmio_base + 0x60) as *const u32) }
    });
    if status != 0 {
        with_riscv_sum_for_virtio_ack(|| {
            // SAFETY: same MMIO contract as the status read above; the ACK register
            // consumes exactly the pending status bits returned by the device.
            unsafe { core::ptr::write_volatile((mmio_base + 0x64) as *mut u32, status) };
        });
    }
    status
}

/// Acknowledge an interrupt from an as-yet unclaimed VirtIO MMIO slot.
///
/// Driver Cells may need several synchronous queue transactions to initialise
/// before they can enter `sys_wait_irq`. Leaving those level-triggered
/// interrupts asserted starves the Cell before registration completes.
fn ack_unclaimed(irq: u32) -> bool {
    #[cfg(target_arch = "aarch64")]
    let base = (16..48)
        .contains(&irq)
        .then_some(0x0a00_0000usize + (irq as usize - 16) * 0x200);
    #[cfg(target_arch = "riscv64")]
    let base = (1..9)
        .contains(&irq)
        .then_some(0x1000_1000usize + (irq as usize - 1) * 0x1000);
    // x86_64 routes VirtIO through PCI MSI-X, not a fixed MMIO IRQ window, so
    // there is no slot base to derive and nothing to acknowledge here.
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    let base: Option<usize> = {
        let _ = irq;
        None
    };

    let Some(base) = base else {
        return false;
    };
    ack_virtio_interrupt_status(base) != 0
}

/// VirtIO MMIO IRQ dispatcher — called from the arch trap handlers (riscv64/aarch64)
/// when any VirtIO MMIO IRQ fires. Routes the IRQ to whichever Driver Cell registered
/// for it via `sys_wait_irq` (the block / net / gpu cells all rely on this), ACKs the
/// input slot when the input service Cell isn't up yet, and warns on an unclaimed slot.
///
/// Relocated here from the deleted kernel `virtio_blk` driver (G2 loader redesign
/// phase 06): it is VirtIO-common, not block-specific. The former kernel-block ACK
/// branch is gone — the virtio-blk Driver Cell now ACKs its own IRQ via the
/// `sys_wait_irq` path above.
#[no_mangle]
pub extern "Rust" fn vi_handle_virtio_irq(irq: u32) {
    let nic_irq = crate::task::drivers::driver_cell::owns_registered_nic_irq(irq);

    // Driver Cell IRQ routing: a Cell registered for this IRQ via sys_wait_irq —
    // signal it (sets IRQ_PENDING + writes VirtIO InterruptACK) and return.
    if crate::task::drivers::irq_wait::has_waiter(irq as u8) {
        crate::task::drivers::irq_wait::signal_irq(irq as u8);
        if nic_irq {
            crate::task::waker::signal_net_rx();
        }
        return;
    }
    // Input (keyboard) slot: ACK to prevent an interrupt storm before the input
    // service Cell is up; event routing lives entirely in that Cell.
    if crate::task::drivers::input_irq_ack::ack_if_input(irq) {
        return;
    }
    if ack_unclaimed(irq) {
        if nic_irq {
            crate::task::waker::signal_net_rx();
        }
        return;
    }
    // Unknown VirtIO slot — no device registered. InterruptStatus is already cleared
    // by plic_complete in the trap handler.
    log::warn!(
        "[virtio] unhandled IRQ {} — no registered device for this slot",
        irq
    );
}
