// SPDX-License-Identifier: MPL-2.0
//! 32-bit RISC-V.

pub mod user_access {
    //! `sstatus.SUM` window (same CSR bit as riscv64).
    // Each function below is one SUM (sstatus bit 18) asm block moved verbatim
    // from its call site; blocks whose text or options differ stay separate.

    /// Syscall dispatch: set SUM for the duration of the handler.
    pub(crate) fn syscall_open() {
        // SAFETY: csrs sstatus SUM from S-mode enables access to user pages.
        unsafe {
            core::arch::asm!("csrs sstatus, {0}", in(reg) 0x40000usize);
        }
    }

    /// Syscall dispatch: clear SUM after the handler.
    pub(crate) fn syscall_close() {
        // SAFETY: csrc sstatus SUM from S-mode.
        unsafe {
            core::arch::asm!("csrc sstatus, {0}", in(reg) 0x40000usize);
        }
    }

    /// Test dispatch: return `sstatus` and set SUM, as real trap entry does.
    #[cfg(feature = "getrandom-sas-test")]
    pub(crate) fn test_open_saved() -> usize {
        let saved_sstatus: usize;
        unsafe {
            // SAFETY: test-only dispatch must reproduce the real trap entry's SUM state.
            core::arch::asm!(
                "csrr {saved_sstatus}, sstatus",
                "csrs sstatus, {sum}",
                saved_sstatus = out(reg) saved_sstatus,
                sum = in(reg) 0x40000_usize,
                options(nostack)
            );
        }
        saved_sstatus
    }

    /// Test dispatch: clear SUM again.
    #[cfg(feature = "getrandom-sas-test")]
    pub(crate) fn test_close() {
        unsafe {
            // SAFETY: restore exactly the caller hart's prior SUM bit.
            core::arch::asm!(
                "csrc sstatus, {sum}",
                sum = in(reg) 0x40000_usize,
                options(nostack)
            );
        }
    }

    /// Boot hart: SUM stays set for the kernel lifetime (see kmain).
    pub(crate) fn boot_open() {
        // SAFETY: csrs modifies sstatus.SUM (bit 18). Safe to set for kernel S-mode code.
        unsafe {
            core::arch::asm!("csrs sstatus, {sum}", sum = in(reg) 0x40000_usize, options(nostack));
        }
    }

    /// Secondary hart: SUM set right after the shared root is activated.
    pub(crate) fn secondary_open() {
        // SAFETY: csrs sstatus SUM from S-mode.
        unsafe {
            core::arch::asm!(
                "csrs sstatus, {sum}",
                sum = in(reg) 0x40000usize,
                options(nostack)
            );
        }
    }

    /// User-copy guard window: set SUM once the window is armed.
    #[cfg(feature = "native-domains")]
    pub(crate) fn guard_open() {
        // SAFETY: csrs sstatus SUM from S-mode.
        unsafe {
            let sum_bit: usize = 1 << 18;
            core::arch::asm!(
                "csrs sstatus, {sum}",
                sum = in(reg) sum_bit,
                options(nomem, nostack),
            );
        }
    }

    /// `sstatus`, read by the test-hooks stack sampler.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn stack_sample_status() -> usize {
        let status: usize;
        // SAFETY: reading sstatus from S-mode has no side effect.
        unsafe {
            core::arch::asm!("csrr {status}, sstatus", status = out(reg) status, options(nostack));
        }
        status
    }

    /// `sstatus`, read around a VirtIO MMIO ACK.
    pub(crate) fn virtio_ack_status() -> usize {
        let sstatus: usize;
        // SAFETY: reading sstatus from S-mode has no side effect.
        unsafe {
            core::arch::asm!("csrr {value}, sstatus", value = out(reg) sstatus, options(nostack));
        }
        sstatus
    }

    /// Set SUM (`csrs sstatus, SUM_MASK`), for a window the caller restores.
    pub(crate) fn sum_mask_set() {
        const SUM_MASK: usize = 1usize << 18;
        // SAFETY: csrs sstatus SUM from S-mode; only SUM changes.
        unsafe {
            core::arch::asm!("csrs sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
        }
    }

    /// Clear SUM (`csrc sstatus, SUM_MASK`), closing a window `sum_mask_set` opened.
    pub(crate) fn sum_mask_clear() {
        const SUM_MASK: usize = 1usize << 18;
        // SAFETY: csrc sstatus SUM from S-mode; only SUM changes.
        unsafe {
            core::arch::asm!("csrc sstatus, {mask}", mask = in(reg) SUM_MASK, options(nostack));
        }
    }
}
