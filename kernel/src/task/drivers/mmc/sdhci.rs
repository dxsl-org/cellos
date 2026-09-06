use super::regs::*;
use hal_traits_mmc::{BusWidth, MmcCmd, MmcResponse, RespType, ViMmcHost};
use types::{ViError, ViResult};

/// Legacy iteration limit for short command and register polls.
const POLL_ITERATION_LIMIT: u32 = 500_000;
/// Maximum elapsed time for the write FIFO to become ready.
const BUFFER_READY_TIMEOUT_US: u64 = 500_000;
/// Maximum elapsed time for the card to finish a data transfer.
const DATA_TRANSFER_TIMEOUT_US: u64 = 10_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DataEventState {
    Pending,
    Ready,
    Error,
}

fn data_event_state(status: u32, event: u32) -> DataEventState {
    if status & INT_ERROR != 0 {
        DataEventState::Error
    } else if status & event != 0 {
        DataEventState::Ready
    } else {
        DataEventState::Pending
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SdhciAccessPolicy {
    pub word_access_only: bool,
    pub minimum_write_spacing_us: u32,
}

/// SDHCI host controller — PIO polling mode, no DMA, no interrupts.
///
/// The `base` field is the kernel-mapped virtual address of the SDHCI register block.
/// All MMIO accesses are `read_volatile` / `write_volatile` to prevent optimisation.
pub struct SdhciController {
    base: usize,
    /// SDHC = true (block-addressed); SDSC = false (byte-addressed).
    pub is_sdhc: bool,
    /// SDHCI spec version read from HOST_VERSION[2:0]; affects clock divider encoding.
    spec_ver: u8,
    policy: SdhciAccessPolicy,
    transfer_mode_shadow: u32,
    last_write_ticks: u64,
}

impl SdhciController {
    /// Construct a controller from `base`.
    ///
    /// # Safety
    /// `base` must be a valid kernel-mapped MMIO address for the SDHCI register block.
    /// The address must remain valid for the lifetime of `Self`.
    pub unsafe fn new(base: usize, policy: SdhciAccessPolicy) -> Self {
        let mut c = Self {
            base,
            is_sdhc: false,
            spec_ver: 0,
            policy,
            transfer_mode_shadow: 0,
            last_write_ticks: 0,
        };
        // SAFETY: base is the validated MMIO address passed by the caller.
        c.spec_ver = (c.read16(SDHCI_HOST_VERSION) & 0xFF) as u8;
        c
    }

    // --- volatile MMIO helpers ---

    #[inline]
    fn read8(&self, off: usize) -> u8 {
        if self.policy.word_access_only {
            let shift = (off & 3) * 8;
            return (self.read32(off & !3) >> shift) as u8;
        }
        // SAFETY: base + off is within the SDHCI MMIO block mapped by the kernel.
        unsafe { core::ptr::read_volatile((self.base + off) as *const u8) }
    }
    #[inline]
    fn read32(&self, off: usize) -> u32 {
        // SAFETY: base + off is within the SDHCI MMIO block mapped by the kernel.
        unsafe { core::ptr::read_volatile((self.base + off) as *const u32) }
    }
    #[inline]
    fn read16(&self, off: usize) -> u16 {
        if self.policy.word_access_only {
            let shift = (off & 2) * 8;
            return (self.read32(off & !3) >> shift) as u16;
        }
        // SAFETY: same as read32.
        unsafe { core::ptr::read_volatile((self.base + off) as *const u16) }
    }
    #[inline]
    fn write32(&mut self, off: usize, v: u32) {
        self.space_controller_write(off);
        // SAFETY: same as read32.
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u32, v) }
        self.last_write_ticks = Self::controller_timer_ticks();
    }
    #[inline]
    fn write16(&mut self, off: usize, v: u16) {
        if self.policy.word_access_only {
            let shift = (off & 2) * 8;
            let old = if off == SDHCI_COMMAND {
                self.transfer_mode_shadow
            } else {
                self.read32(off & !3)
            };
            let combined = (old & !(0xffff << shift)) | ((v as u32) << shift);
            if off == SDHCI_TRANSFER_MODE {
                self.transfer_mode_shadow = combined;
            } else {
                self.write32(off & !3, combined);
            }
            return;
        }
        // SAFETY: same as read32.
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u16, v) }
    }
    #[inline]
    fn write8(&mut self, off: usize, v: u8) {
        if self.policy.word_access_only {
            let shift = (off & 3) * 8;
            let old = self.read32(off & !3);
            let combined = (old & !(0xff << shift)) | ((v as u32) << shift);
            self.write32(off & !3, combined);
            return;
        }
        // SAFETY: same as read32.
        unsafe { core::ptr::write_volatile((self.base + off) as *mut u8, v) }
    }

    #[inline]
    fn controller_timer_ticks() -> u64 {
        #[cfg(target_arch = "aarch64")]
        {
            let ticks: u64;
            // SAFETY: CNTPCT_EL0 is a read-only architectural counter available at EL1/EL2.
            unsafe {
                core::arch::asm!("mrs {}, cntpct_el0", out(reg) ticks, options(nomem, nostack));
            }
            ticks
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            0
        }
    }

    #[cfg(target_arch = "aarch64")]
    fn controller_timer_frequency() -> u64 {
        let frequency: u64;
        // SAFETY: CNTFRQ_EL0 is a read-only architectural frequency register.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) frequency, options(nomem, nostack));
        }
        frequency
    }

    fn space_controller_write(&self, off: usize) {
        #[cfg(target_arch = "aarch64")]
        if off != SDHCI_BUFFER
            && self.last_write_ticks != 0
            && self.policy.minimum_write_spacing_us != 0
        {
            let frequency = Self::controller_timer_frequency();
            // BCM2835 Arasan may drop writes less than two 400 kHz SD-clock cycles apart.
            let spacing_us = u64::from(self.policy.minimum_write_spacing_us);
            let minimum_ticks = frequency.saturating_mul(spacing_us).div_ceil(1_000_000);
            while Self::controller_timer_ticks().wrapping_sub(self.last_write_ticks) < minimum_ticks
            {
                core::hint::spin_loop();
            }
        }

        #[cfg(not(target_arch = "aarch64"))]
        let _ = off;
    }

    /// Spin until `(read32(off) & mask) == 0`, up to `iteration_limit` polls.
    fn poll_clear32(&self, off: usize, mask: u32, iteration_limit: u32) -> ViResult<()> {
        let mut i = 0u32;
        while self.read32(off) & mask != 0 {
            if i >= iteration_limit {
                return Err(ViError::WouldBlock);
            }
            i += 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Spin until `(read8(off) & mask) == 0`, up to `iteration_limit` polls.
    fn poll_clear8(&self, off: usize, mask: u8, iteration_limit: u32) -> ViResult<()> {
        let mut i = 0u32;
        while self.read8(off) & mask != 0 {
            if i >= iteration_limit {
                return Err(ViError::WouldBlock);
            }
            i += 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Spin until `(read32(off) & mask) != 0`, up to `iteration_limit` polls.
    fn poll_set32(&self, off: usize, mask: u32, iteration_limit: u32) -> ViResult<()> {
        let mut i = 0u32;
        while self.read32(off) & mask == 0 {
            if i >= iteration_limit {
                return Err(ViError::WouldBlock);
            }
            i += 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Wait for one SDHCI data event while preserving controller error state.
    fn wait_data_event(
        &mut self,
        stage: &str,
        event: u32,
        _timeout_us: u64,
        _fallback_iteration_limit: u64,
    ) -> ViResult<()> {
        #[cfg(target_arch = "aarch64")]
        let started = Self::controller_timer_ticks();
        #[cfg(target_arch = "aarch64")]
        let timeout_ticks = Self::controller_timer_frequency()
            .saturating_mul(_timeout_us)
            .div_ceil(1_000_000);
        #[cfg(not(target_arch = "aarch64"))]
        let mut iterations = 0u64;

        loop {
            let status = self.read32(SDHCI_INT_STATUS);
            if data_event_state(status, event) == DataEventState::Error {
                log::warn!(
                    "[sdhci] {} error: INT_STATUS=0x{:08x} PRESENT_STATE=0x{:08x}",
                    stage,
                    status,
                    self.read32(SDHCI_PRESENT_STATE)
                );
                self.clear_int(INT_ALL_ERROR | event);
                self.write8(SDHCI_SOFT_RESET, RESET_DAT);
                if self
                    .poll_clear8(SDHCI_SOFT_RESET, RESET_DAT, POLL_ITERATION_LIMIT)
                    .is_err()
                {
                    log::warn!("[sdhci] {} DAT-line recovery timed out", stage);
                }
                return Err(ViError::IO);
            }
            if data_event_state(status, event) == DataEventState::Ready {
                return Ok(());
            }
            #[cfg(target_arch = "aarch64")]
            let timed_out = Self::controller_timer_ticks().wrapping_sub(started) >= timeout_ticks;
            #[cfg(not(target_arch = "aarch64"))]
            let timed_out = iterations >= _fallback_iteration_limit;

            if timed_out {
                #[cfg(target_arch = "aarch64")]
                log::warn!(
                    "[sdhci] {} timeout after {} us: INT_STATUS=0x{:08x} PRESENT_STATE=0x{:08x}",
                    stage,
                    _timeout_us,
                    status,
                    self.read32(SDHCI_PRESENT_STATE)
                );
                #[cfg(not(target_arch = "aarch64"))]
                log::warn!(
                    "[sdhci] {} timeout after {} polls: INT_STATUS=0x{:08x} PRESENT_STATE=0x{:08x}",
                    stage,
                    _fallback_iteration_limit,
                    status,
                    self.read32(SDHCI_PRESENT_STATE)
                );
                return Err(ViError::WouldBlock);
            }
            #[cfg(not(target_arch = "aarch64"))]
            {
                iterations += 1;
            }
            core::hint::spin_loop();
        }
    }

    /// Spin until `(read16(off) & mask) != 0`, up to `iteration_limit` polls.
    fn poll_set16(&self, off: usize, mask: u16, iteration_limit: u32) -> ViResult<()> {
        let mut i = 0u32;
        while self.read16(off) & mask == 0 {
            if i >= iteration_limit {
                return Err(ViError::WouldBlock);
            }
            i += 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Read the Present State register (offset 0x24) — single MMIO read, no poll.
    ///
    /// Used by callers for a fast card-present check before any lengthy poll sequences.
    pub fn read_present_state(&self) -> u32 {
        self.read32(SDHCI_PRESENT_STATE)
    }

    /// Emit one bounded register snapshot for real-board controller diagnosis.
    pub fn log_register_snapshot(&self, stage: &str) {
        log::info!(
            "[sdhci-diag] {} host=0x{:04x} caps=0x{:08x} clock=0x{:04x} power=0x{:02x} present=0x{:08x} int=0x{:08x}",
            stage,
            self.read16(SDHCI_HOST_VERSION),
            self.read32(SDHCI_CAPABILITIES),
            self.read16(SDHCI_CLOCK_CONTROL),
            self.read8(SDHCI_POWER_CONTROL),
            self.read32(SDHCI_PRESENT_STATE),
            self.read32(SDHCI_INT_STATUS),
        );
    }

    /// Reset the controller (all lines).
    pub fn reset_all(&mut self) -> ViResult<()> {
        self.write8(SDHCI_SOFT_RESET, RESET_ALL);
        self.poll_clear8(SDHCI_SOFT_RESET, RESET_ALL, POLL_ITERATION_LIMIT)?;
        Ok(())
    }

    /// Enable 3.3 V power to the card slot.
    pub fn power_on(&mut self) {
        self.write8(SDHCI_POWER_CONTROL, PWR_33V);
    }

    /// Set the SD clock to the requested divider.
    ///
    /// Uses spec-v3 10-bit divider encoding when `spec_ver >= SPEC_V3`.
    fn set_clock_div(&mut self, div: u16) {
        // Disable SD clock and internal clock first.
        self.write16(SDHCI_CLOCK_CONTROL, 0);

        let clk = if self.spec_ver >= SPEC_V3 {
            // 10-bit divider: bits[7:0] in bits[15:8], bits[9:8] in bits[7:6].
            let lo = div & 0xFF;
            let hi = (div >> 8) & 0x03;
            (lo << 8) | (hi << 6) | CLK_INT_EN
        } else {
            // 8-bit divider (spec v1/v2): bits[7:0] in bits[15:8].
            ((div & 0xFF) << 8) | CLK_INT_EN
        };

        self.write16(SDHCI_CLOCK_CONTROL, clk);
        // Wait for internal clock to stabilise.
        let _ = self.poll_set16(SDHCI_CLOCK_CONTROL, CLK_INT_STABLE, POLL_ITERATION_LIMIT);
        // Enable SD clock to card.
        self.write16(SDHCI_CLOCK_CONTROL, clk | CLK_SD_EN);
    }

    /// Read the INT_STATUS register and clear the given bits (w1c).
    fn clear_int(&mut self, bits: u32) {
        self.write32(SDHCI_INT_STATUS, bits);
    }

    /// Wait for CMD_INHIBIT and DAT_INHIBIT to clear before issuing a command.
    fn wait_cmd_ready(&self, needs_dat: bool) -> ViResult<()> {
        let mask = if needs_dat {
            PS_CMD_INHIBIT | PS_DAT_INHIBIT
        } else {
            PS_CMD_INHIBIT
        };
        self.poll_clear32(SDHCI_PRESENT_STATE, mask, POLL_ITERATION_LIMIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_event_reports_error_before_ready() {
        assert_eq!(
            data_event_state(INT_ERROR | INT_BUF_WRITE_READY, INT_BUF_WRITE_READY),
            DataEventState::Error
        );
    }

    #[test]
    fn data_event_distinguishes_pending_and_ready() {
        assert_eq!(
            data_event_state(0, INT_XFER_COMPLETE),
            DataEventState::Pending
        );
        assert_eq!(
            data_event_state(INT_XFER_COMPLETE, INT_XFER_COMPLETE),
            DataEventState::Ready
        );
    }
}

impl Drop for SdhciController {
    fn drop(&mut self) {
        // Power off the card slot on controller teardown.
        self.write8(SDHCI_POWER_CONTROL, PWR_OFF);
        self.write16(SDHCI_CLOCK_CONTROL, 0);
    }
}

impl ViMmcHost for SdhciController {
    fn send_cmd(&mut self, cmd: MmcCmd) -> ViResult<MmcResponse> {
        self.wait_cmd_ready(cmd.has_data)?;

        // Unmask normal+error interrupts in INT_ENABLE (no CPU IRQ — polling only).
        self.write32(SDHCI_INT_ENABLE, INT_ALL_NORMAL | INT_ALL_ERROR);
        self.write32(SDHCI_SIGNAL_ENABLE, 0); // no CPU interrupt

        // Build response flag bits for the COMMAND register.
        let resp_flags: u16 = match cmd.resp_type {
            RespType::None => RESP_NONE,
            RespType::R1 => RESP_R1,
            RespType::R1b => RESP_R1B,
            RespType::R2 => RESP_R2,
            RespType::R3 => RESP_R3,
            RespType::R6 => RESP_R6,
            RespType::R7 => RESP_R7,
        };

        self.write32(SDHCI_ARGUMENT, cmd.arg);
        // Writing COMMAND fires the command to the card.
        self.write16(SDHCI_COMMAND, cmd_reg(cmd.index, resp_flags, cmd.has_data));

        // Wait for CMD_COMPLETE (bit 0) or an error.
        self.poll_set32(
            SDHCI_INT_STATUS,
            INT_CMD_COMPLETE | INT_ERROR,
            POLL_ITERATION_LIMIT,
        )?;

        let status = self.read32(SDHCI_INT_STATUS);
        self.clear_int(INT_CMD_COMPLETE | INT_ALL_ERROR);

        if status & INT_ERROR != 0 {
            log::warn!(
                "[sdhci] cmd{} error, INT_STATUS=0x{:08x}",
                cmd.index,
                status
            );
            return Err(ViError::IO);
        }

        // Read response registers.
        let r = [
            self.read32(SDHCI_RESPONSE),
            self.read32(SDHCI_RESPONSE + 4),
            self.read32(SDHCI_RESPONSE + 8),
            self.read32(SDHCI_RESPONSE + 12),
        ];
        Ok(r)
    }

    fn read_block(&mut self, buf: &mut [u8]) -> ViResult<()> {
        // Caller must pass a 512-byte, 4-byte-aligned buffer (one SDHCI block).
        if buf.len() != 512 {
            return Err(ViError::InvalidArgument);
        }
        // Wait for BUFFER_READ_READY (data available in FIFO).
        self.wait_data_event(
            "buffer-read-ready",
            INT_BUF_READ_READY,
            BUFFER_READY_TIMEOUT_US,
            u64::from(POLL_ITERATION_LIMIT),
        )?;
        self.clear_int(INT_BUF_READ_READY);
        // Read 4 bytes at a time from the BUFFER port.
        let chunks = buf.len() / 4;
        for i in 0..chunks {
            let word = self.read32(SDHCI_BUFFER);
            let off = i * 4;
            buf[off] = (word & 0xFF) as u8;
            buf[off + 1] = ((word >> 8) & 0xFF) as u8;
            buf[off + 2] = ((word >> 16) & 0xFF) as u8;
            buf[off + 3] = ((word >> 24) & 0xFF) as u8;
        }

        // Wait for TRANSFER_COMPLETE.
        self.wait_data_event(
            "read-transfer-complete",
            INT_XFER_COMPLETE,
            DATA_TRANSFER_TIMEOUT_US,
            u64::from(POLL_ITERATION_LIMIT),
        )?;
        self.clear_int(INT_XFER_COMPLETE);
        Ok(())
    }

    fn write_block(&mut self, buf: &[u8]) -> ViResult<()> {
        if buf.len() != 512 {
            return Err(ViError::InvalidArgument);
        }

        // Wait for BUFFER_WRITE_READY (FIFO has space).
        self.wait_data_event(
            "buffer-write-ready",
            INT_BUF_WRITE_READY,
            BUFFER_READY_TIMEOUT_US,
            u64::from(POLL_ITERATION_LIMIT),
        )?;
        self.clear_int(INT_BUF_WRITE_READY);

        let chunks = buf.len() / 4;
        for i in 0..chunks {
            let off = i * 4;
            let word = (buf[off] as u32)
                | ((buf[off + 1] as u32) << 8)
                | ((buf[off + 2] as u32) << 16)
                | ((buf[off + 3] as u32) << 24);
            self.write32(SDHCI_BUFFER, word);
        }

        self.wait_data_event(
            "write-transfer-complete",
            INT_XFER_COMPLETE,
            DATA_TRANSFER_TIMEOUT_US,
            u64::from(POLL_ITERATION_LIMIT),
        )?;
        self.clear_int(INT_XFER_COMPLETE);
        Ok(())
    }

    fn set_clock_hz(&mut self, hz: u32) -> ViResult<()> {
        // Base clock frequency (typically 200 MHz on Arasan, 50 MHz on some others).
        // We assume 200 MHz; the divider is rounded up to the nearest power-of-2 (spec v1/v2)
        // or any value (spec v3). For boot-time use we target either 400 kHz (ID) or 25 MHz (DS).
        const BASE_HZ: u32 = 200_000_000;
        let div = BASE_HZ.checked_div(hz).map_or(0, |q| (q / 2).max(1) as u16);
        self.set_clock_div(div);
        Ok(())
    }

    fn set_bus_width(&mut self, width: BusWidth) -> ViResult<()> {
        let mut hc = self.read32(SDHCI_HOST_CONTROL) as u8;
        hc &= !0x26; // clear 4-bit (bit1) and 8-bit (bit5) fields
        match width {
            BusWidth::One => {}
            BusWidth::Four => hc |= 1 << 1,
            BusWidth::Eight => hc |= 1 << 5,
        }
        self.write8(SDHCI_HOST_CONTROL, hc);
        Ok(())
    }

    fn card_present(&self) -> bool {
        self.read32(SDHCI_PRESENT_STATE) & PS_CARD_PRESENT != 0
    }
}

impl SdhciController {
    /// Configure BLOCK_SIZE, BLOCK_COUNT, and TRANSFER_MODE for an upcoming data command.
    pub(super) fn setup_data_transfer(
        &mut self,
        block_size: u16,
        block_count: u16,
        transfer_mode: u16,
    ) {
        // Reset leaves the timeout exponent at its minimum on Arasan. Program the
        // conservative SDHCI maximum before every data command, as U-Boot does.
        self.write8(SDHCI_TIMEOUT_CONTROL, TIMEOUT_MAX);
        self.write16(SDHCI_BLOCK_SIZE, block_size);
        self.write16(SDHCI_BLOCK_COUNT, block_count);
        self.write16(SDHCI_TRANSFER_MODE, transfer_mode);
    }
}
