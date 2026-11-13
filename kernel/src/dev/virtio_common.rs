//! Shared VirtIO MMIO slot enumeration.
//!
//! `virtio_blk`, `input_irq_ack`, and Driver Cells use `virtio_slots()` to
//! iterate all VirtIO MMIO slots for the current platform.
//!
//! AArch64: the board backend's VirtIO MMIO window, if any. QEMU virt scans all
//! 32 slots at 0x0a000000, stride 0x200; the Raspberry Pi boards expose no
//! VirtIO MMIO window and yield no slots. QEMU assigns devices to slots in an
//! implementation-defined order so we must probe all 32 there.  The identity
//! map in paging.rs covers the window wherever the backend declares one.
//!
//! RISC-V: reads DTB-confirmed slots from `platform::PLATFORM`.
//! PCIe-only arches expose no VirtIO-MMIO slots.

/// A VirtIO MMIO slot with base address and IRQ.
pub struct VirtioSlot {
    pub base: usize,
    pub irq: u32,
}

/// Iterator over all VirtIO MMIO slots for the current platform.
pub fn virtio_slots() -> impl Iterator<Item = VirtioSlot> {
    crate::arch::virtio_mmio::slots()
}

/// Size of one VirtIO MMIO transport's register block.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64", test))]
const SLOT_REGS_LEN: usize = 0x200;
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64", test))]
const PAGE: usize = 0x1000;

/// `base` is exactly the base of one of `slot_bases`.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64", test))]
fn is_slot_base(base: usize, mut slot_bases: impl Iterator<Item = usize>) -> bool {
    slot_bases.any(|slot| slot == base)
}

/// Length of the window a Driver Cell may claim for the slot at `base`.
///
/// A slot that occupies its page alone (RISC-V QEMU virt spaces transports a
/// page apart) gets the whole page, so the claim maps it for U-mode. Slots
/// sharing a page (AArch64 QEMU virt packs eight per page) keep their register
/// block: that page stays kernel-only, the kernel mediates the owner's
/// accesses, and no Cell can reach a sibling slot's registers.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64", test))]
pub(crate) fn slot_window(base: usize, mut slot_bases: impl Iterator<Item = usize>) -> usize {
    let alone_in_page =
        base.is_multiple_of(PAGE) && !slot_bases.any(|other| other > base && other - base < PAGE);
    if alone_in_page {
        PAGE
    } else {
        SLOT_REGS_LEN
    }
}

/// Outcome of reading one slot's identity registers for `ProbeVirtioMmio`.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) enum SlotProbe {
    /// `base` is not one of this board's slot bases.
    NotASlot,
    /// No VirtIO transport a Cell can name answers there.
    NoTransport,
    /// [`abi::syscall::virtio_probe::encode`]d DeviceID and Status.
    Identity(usize),
}

/// Read MagicValue, DeviceID and Status of the slot at `base` through the
/// kernel mapping, without claiming the slot.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub(crate) fn probe_slot(base: usize) -> SlotProbe {
    use abi::syscall::virtio_probe::{
        encode, DEVICE_ID_OFFSET, MAGIC, MAGIC_OFFSET, STATUS_OFFSET,
    };
    if !is_slot_base(base, virtio_slots().map(|slot| slot.base)) {
        return SlotProbe::NotASlot;
    }
    // A slot a Cell already claimed is user-mapped on RISC-V; S-mode reads it
    // only with SUM set.
    let (magic, device_id, status) = with_riscv_sum_for_virtio_ack(|| {
        // SAFETY: `base` is one of the board's VirtIO slots, which the boot
        // identity map covers as device memory. MagicValue, DeviceID and Status
        // are plain 32-bit registers inside the slot whose reads have no side
        // effect on the device.
        unsafe {
            (
                core::ptr::read_volatile((base + MAGIC_OFFSET) as *const u32),
                core::ptr::read_volatile((base + DEVICE_ID_OFFSET) as *const u32),
                core::ptr::read_volatile((base + STATUS_OFFSET) as *const u32),
            )
        }
    });
    if magic != MAGIC {
        return SlotProbe::NoTransport;
    }
    match encode(device_id, status) {
        Some(identity) => SlotProbe::Identity(identity),
        None => SlotProbe::NoTransport,
    }
}

/// Execute one VirtIO MMIO ACK sequence with RV64 SUM enabled only for that scope.
///
/// On RV64, supervisor-mode MMIO ACK helpers may touch user-accessible identity-mapped
/// device pages while trap handling runs with SUM clear. This helper snapshots whether
/// SUM was already set, raises it only for `ack`, and restores the prior state
/// immediately afterwards. Other architectures execute `ack` unchanged.
pub(crate) fn with_riscv_sum_for_virtio_ack<T>(ack: impl FnOnce() -> T) -> T {
    crate::arch::virtio_mmio::with_ack_access(ack)
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
    let base = crate::arch::virtio_mmio::slot_base_for_irq(irq);

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
    let nic_irq = crate::dev::driver_cell::owns_registered_nic_irq(irq);

    // Driver Cell IRQ routing: a Cell registered for this IRQ via sys_wait_irq —
    // signal it (sets IRQ_PENDING + writes VirtIO InterruptACK) and return.
    if crate::dev::irq_wait::has_waiter(irq as u8) {
        crate::dev::irq_wait::signal_irq(irq as u8);
        if nic_irq {
            crate::task::waker::signal_net_rx();
        }
        return;
    }
    // Input (keyboard) slot: ACK to prevent an interrupt storm before the input
    // service Cell is up; event routing lives entirely in that Cell.
    if crate::dev::input_irq_ack::ack_if_input(irq) {
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

#[cfg(test)]
mod tests {
    use super::{is_slot_base, slot_window, PAGE, SLOT_REGS_LEN};

    const RISCV_SLOTS: [usize; 8] = [
        0x1000_1000,
        0x1000_2000,
        0x1000_3000,
        0x1000_4000,
        0x1000_5000,
        0x1000_6000,
        0x1000_7000,
        0x1000_8000,
    ];

    fn arm_slots() -> impl Iterator<Item = usize> + Clone {
        (0..32).map(|i| 0x0a00_0000 + i * SLOT_REGS_LEN)
    }

    #[test]
    fn only_exact_slot_bases_are_slots() {
        let slots = || RISCV_SLOTS.iter().copied();
        assert!(RISCV_SLOTS.iter().all(|&base| is_slot_base(base, slots())));
        for base in [0x1000_0000, 0x1000_1004, 0x1000_1200, 0x1000_9000, 0] {
            assert!(!is_slot_base(base, slots()), "{base:#x}");
        }
        assert!(is_slot_base(0x0a00_3e00, arm_slots()));
        assert!(!is_slot_base(0x0a00_3e08, arm_slots()));
        assert!(!is_slot_base(0x0a00_4000, arm_slots()));
    }

    /// A page is claimable only by a slot that is alone in it; otherwise the
    /// claim would make sibling slots' registers reachable.
    #[test]
    fn page_windows_only_for_slots_alone_in_their_page() {
        for &base in &RISCV_SLOTS {
            assert_eq!(slot_window(base, RISCV_SLOTS.iter().copied()), PAGE);
        }
        for base in arm_slots() {
            assert_eq!(slot_window(base, arm_slots()), SLOT_REGS_LEN, "{base:#x}");
        }
        // Page-aligned but sharing the page with the next slot.
        assert_eq!(
            slot_window(0x2000_0000, [0x2000_0000, 0x2000_0800].into_iter()),
            SLOT_REGS_LEN
        );
        // Alone but not page-aligned.
        assert_eq!(
            slot_window(0x2000_0200, [0x2000_0200].into_iter()),
            SLOT_REGS_LEN
        );
        // A slot in the previous page does not share this one.
        assert_eq!(
            slot_window(0x2000_1000, [0x2000_0e00, 0x2000_1000].into_iter()),
            PAGE
        );
    }
}
