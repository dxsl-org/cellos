//! Intel GuC firmware load: WOPCM partition, reset, parameter block, transfer setup,
//! uCode DMA, boot wait, and the MMIO host-to-GuC channel.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_guc.c` (`xe_guc_reset`,
//! `guc_write_params`, `guc_prepare_xfer`, `guc_xfer_rsa`, `guc_load_done`,
//! `guc_wait_ucode`, `__xe_guc_upload`, `xe_guc_mmio_send_recv`, `xe_guc_notify`),
//! `xe_uc_fw.c` (`uc_fw_xfer`), `xe_wopcm.c` (`xe_wopcm_init`), `xe_uc.c` (order:
//! WOPCM at init, then sanitize/reset, then upload), `regs/xe_guc_regs.h` and
//! `abi/guc_errors_abi.h`.
//!
//! Every wait is bounded by time through [`Clock`], with upstream's limits: GDRST 5 ms,
//! uCode DMA 100 ms, GuC boot polled every 10 ms for up to 3 s (`GUC_LOAD_TIMEOUT_SEC` of a
//! non-debug build; a load above 200 ms is reported as slow), MMIO replies 50 ms.

use crate::clock::Clock;
use crate::regs::{
    masked_write, wait32, RegisterFile, WaitError, ARAT_EXPIRED_INTRMSK, GDRST, GRDOM_GUC,
    PMINTRMSK,
};
use intel_gpu_core::guc_abi::{decode_mmio_reply, MmioReply};
use intel_gpu_core::guc_init::{
    check_layout, compute_layout, locked_layout, WopcmError, WopcmLayout, GUC_CTL_MAX_DWORDS,
    GUC_WOPCM_OFFSET_MASK, GUC_WOPCM_OFFSET_VALID, GUC_WOPCM_SIZE_LOCKED, GUC_WOPCM_SIZE_MASK,
    LNL_WOPCM_SIZE, MAX_WOPCM_SIZE,
};

// ── Registers (`regs/xe_guc_regs.h`) ─────────────────────────────────────────

/// `DIST_DBS_POPULATED`: doorbells per SQIDI in bits 23:16 (read for the ADS).
pub const DIST_DBS_POPULATED: usize = 0x000D08;
pub const GUC_STATUS: usize = 0x00C000;
/// `GS_MIA_IN_RESET`.
pub const GUC_STATUS_MIA_IN_RESET: u32 = 1 << 0;
/// `GS_BOOTROM_MASK`, bits 7:1.
pub const GUC_STATUS_BOOTROM_MASK: u32 = 0x7F << 1;
/// `GS_UKERNEL_MASK`, bits 15:8.
pub const GUC_STATUS_UKERNEL_MASK: u32 = 0xFF << 8;
/// `XE_GUC_LOAD_STATUS_READY` in the microkernel field.
pub const GUC_STATUS_UKERNEL_READY: u32 = 0xF0 << 8;
/// `GUC_WOPCM_SIZE`: size in bits 31:12, `GUC_WOPCM_SIZE_LOCKED` in bit 0.
pub const GUC_WOPCM_SIZE: usize = 0x00C050;
/// `GUC_SHIM_CONTROL`, programmed before the uCode DMA.
pub const GUC_SHIM_CONTROL: usize = 0x00C064;
pub const GUC_ENABLE_READ_CACHE_LOGIC: u32 = 1 << 1;
pub const GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA: u32 = 1 << 9;
pub const GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA: u32 = 1 << 10;
pub const GUC_ENABLE_MIA_CLOCK_GATING: u32 = 1 << 15;
/// `GUC_MOCS_INDEX_MASK` shift (bits 27:24).
pub const GUC_MOCS_INDEX_SHIFT: u32 = 24;
/// `gt->mocs.uc_index` on Lunar Lake and Panther Lake (`xe2_mocs_table`, `xe_mocs.c`).
pub const XE2_MOCS_UC_INDEX: u32 = 3;
/// `SOFT_SCRATCH(0)`; the parameter block occupies `SOFT_SCRATCH(1..=14)`.
pub const SOFT_SCRATCH_0: usize = 0x00C180;
/// `UOS_RSA_SCRATCH(0)`: the first of the registers that take an RSA signature of up to
/// [`UOS_RSA_SCRATCH_COUNT`] dwords, or the GGTT address of a larger one.
pub const UOS_RSA_SCRATCH_0: usize = 0x00C200;
pub const UOS_RSA_SCRATCH_COUNT: usize = 64;
pub const GUC_DMA_ADDR_0_LOW: usize = 0x00C300;
pub const GUC_DMA_ADDR_0_HIGH: usize = 0x00C304;
pub const GUC_DMA_ADDR_1_LOW: usize = 0x00C308;
pub const GUC_DMA_ADDR_1_HIGH: usize = 0x00C30C;
pub const GUC_DMA_COPY_SIZE: usize = 0x00C310;
/// Masked register: bits 15:0 value, bits 31:16 write enable.
pub const GUC_DMA_CTRL: usize = 0x00C314;
pub const GUC_DMA_CTRL_START: u16 = 1 << 0;
pub const GUC_DMA_CTRL_UOS_MOVE: u16 = 1 << 4;
pub const DMA_ADDRESS_SPACE_WOPCM: u32 = 7 << 16;
pub const DMA_ADDRESS_SPACE_GGTT: u32 = 8 << 16;
/// `DMA_GUC_WOPCM_OFFSET`: base in bits 31:14, `GUC_WOPCM_OFFSET_VALID` in bit 0.
pub const DMA_GUC_WOPCM_OFFSET: usize = 0x00C340;
/// `GT_PM_CONFIG` and `GT_DOORBELL_ENABLE`.
pub const GT_PM_CONFIG: usize = 0x13816C;
pub const GT_DOORBELL_ENABLE: u32 = 1 << 0;
/// `GUC_HOST_INTERRUPT`: any write notifies the GuC (`xe_guc_notify` writes 0).
pub const GUC_HOST_INTERRUPT: usize = 0x1901F0;
/// `VF_SW_FLAG(0)`: MMIO request dwords go to `VF_SW_FLAG(0..4)`, the reply comes back
/// in `VF_SW_FLAG(0)`.
pub const VF_SW_FLAG_0: usize = 0x190240;
pub const VF_SW_FLAG_COUNT: usize = 4;

/// WOPCM offset the GuC image is copied to: "current uCode expects the code to be loaded
/// at 8k; locations below this are used for the stack" (`__xe_guc_upload`).
pub const GUC_WOPCM_LOAD_OFFSET: u32 = 0x2000;

/// `xe_guc_reset`: `GDRST` wait.
pub const GDRST_TIMEOUT_US: u64 = 5_000;
/// `uc_fw_xfer`: DMA completion wait.
pub const DMA_TIMEOUT_US: u64 = 100_000;
/// `guc_wait_ucode`: one `GUC_STATUS` read every 10 ms...
pub const LOAD_POLL_US: u64 = 10_000;
/// ...for at most `GUC_LOAD_TIMEOUT_SEC` (3 s outside debug builds).
pub const LOAD_TIMEOUT_US: u64 = 3_000_000;
/// `GUC_LOAD_TIME_WARN_MSEC`.
pub const LOAD_WARN_US: u64 = 200_000;
/// `xe_guc_mmio_send_recv`: wait for the GuC's answer...
pub const MMIO_REPLY_TIMEOUT_US: u64 = 50_000;
/// ...and, after a `NO_RESPONSE_BUSY`, for the final one.
pub const MMIO_BUSY_TIMEOUT_US: u64 = 2_000_000;
/// Upstream retries a `NO_RESPONSE_RETRY` answer without limit (backing off to 256 ms);
/// this driver gives up after this long so a wedged GuC cannot stall its only thread.
pub const MMIO_RETRY_LIMIT_US: u64 = 5_000_000;

/// Why the GuC did not come up, or the MMIO channel failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GucError {
    /// An MMIO access failed.
    Mmio,
    /// The WOPCM cannot hold the image.
    Wopcm(WopcmError),
    /// The WOPCM registers did not read back locked with the written values
    /// (`xe_mmio_write32_and_verify`); the value read.
    WopcmNotLocked(u32),
    /// `GRDOM_GUC` did not clear within 5 ms; the last `GDRST`.
    ResetTimeout(u32),
    /// The reset finished but the MIA core is not held in reset; `GUC_STATUS`.
    NotInReset(u32),
    /// An RSA key shorter than the scratch registers (`xe_uc_fw_copy_rsa` short copy).
    RsaTooShort,
    /// The image or its GGTT address does not fit the DMA registers.
    BadImage,
    /// `START_DMA` did not clear within 100 ms; `DMA_CTRL`.
    DmaTimeout(u32),
    /// `guc_load_done` saw a terminal failure code; `GUC_STATUS`.
    LoadFailed(u32),
    /// No terminal state within 3 s; the last `GUC_STATUS`.
    LoadTimeout(u32),
    /// No MMIO reply in time; the last `VF_SW_FLAG(0)`.
    MmioTimeout(u32),
    /// The GuC answered `RESPONSE_FAILURE`.
    MmioFailure { hint: u32, error: u32 },
    /// The GuC answered with something no request expects (`-EPROTO`).
    MmioProtocol(u32),
}

impl From<WaitError> for GucError {
    fn from(e: WaitError) -> Self {
        match e {
            WaitError::Mmio => GucError::Mmio,
            WaitError::TimedOut(v) => GucError::MmioTimeout(v),
        }
    }
}

fn mmio<T>(r: types::ViResult<T>) -> Result<T, GucError> {
    r.map_err(|_| GucError::Mmio)
}

/// `xe_wopcm_init` for a platform without HuC firmware: keep a partition firmware already
/// locked if it fits the image, else compute one, write it, and require the hardware to
/// report both registers locked with exactly those values (`__wopcm_init_regs`).
pub fn init_wopcm(regs: &mut impl RegisterFile, guc_fw_size: u32) -> Result<WopcmLayout, GucError> {
    let reg_offset = mmio(regs.read32(DMA_GUC_WOPCM_OFFSET))?;
    let reg_size = mmio(regs.read32(GUC_WOPCM_SIZE))?;
    if let Some(layout) = locked_layout(reg_offset, reg_size) {
        // Preprogrammed by BIOS/IFWI: check against the largest WOPCM (`max_wopcm_size`).
        check_layout(MAX_WOPCM_SIZE, layout, guc_fw_size, 0).map_err(GucError::Wopcm)?;
        return Ok(layout);
    }
    let layout = compute_layout(LNL_WOPCM_SIZE, guc_fw_size, 0).map_err(GucError::Wopcm)?;

    mmio(regs.write32(GUC_WOPCM_SIZE, layout.size))?;
    let size = mmio(regs.read32(GUC_WOPCM_SIZE))?;
    if size & (GUC_WOPCM_SIZE_MASK | GUC_WOPCM_SIZE_LOCKED) != layout.size | GUC_WOPCM_SIZE_LOCKED {
        return Err(GucError::WopcmNotLocked(size));
    }
    // No HuC image is loaded, so `HUC_LOADING_AGENT_GUC` stays clear.
    mmio(regs.write32(DMA_GUC_WOPCM_OFFSET, layout.base))?;
    let base = mmio(regs.read32(DMA_GUC_WOPCM_OFFSET))?;
    if base & (GUC_WOPCM_OFFSET_MASK | GUC_WOPCM_OFFSET_VALID)
        != layout.base | GUC_WOPCM_OFFSET_VALID
    {
        return Err(GucError::WopcmNotLocked(base));
    }
    Ok(layout)
}

/// `xe_guc_reset`: reset the GuC domain, wait up to 5 ms for the bit to self-clear, then
/// require the microcontroller held in reset.
pub fn reset_guc(regs: &mut impl RegisterFile, clock: &mut impl Clock) -> Result<(), GucError> {
    mmio(regs.write32(GDRST, GRDOM_GUC))?;
    wait32(regs, clock, GDRST, GRDOM_GUC, 0, GDRST_TIMEOUT_US).map_err(|e| match e {
        WaitError::TimedOut(v) => GucError::ResetTimeout(v),
        WaitError::Mmio => GucError::Mmio,
    })?;
    let status = mmio(regs.read32(GUC_STATUS))?;
    if status & GUC_STATUS_MIA_IN_RESET == 0 {
        return Err(GucError::NotInReset(status));
    }
    Ok(())
}

/// `guc_write_params`: `SOFT_SCRATCH(0)` cleared, the block in `SOFT_SCRATCH(1..=14)`.
pub fn write_params(
    regs: &mut impl RegisterFile,
    params: &[u32; GUC_CTL_MAX_DWORDS],
) -> Result<(), GucError> {
    mmio(regs.write32(SOFT_SCRATCH_0, 0))?;
    for (i, &p) in params.iter().enumerate() {
        mmio(regs.write32(SOFT_SCRATCH_0 + 4 * (1 + i), p))?;
    }
    Ok(())
}

/// `guc_prepare_xfer` for graphics 20+: shim caching and MOCS index, doorbells on, ARAT
/// interrupts unmasked.
pub fn prepare_xfer(regs: &mut impl RegisterFile) -> Result<(), GucError> {
    let shim = GUC_ENABLE_READ_CACHE_LOGIC
        | GUC_ENABLE_READ_CACHE_FOR_SRAM_DATA
        | GUC_ENABLE_READ_CACHE_FOR_WOPCM_DATA
        | GUC_ENABLE_MIA_CLOCK_GATING
        | XE2_MOCS_UC_INDEX << GUC_MOCS_INDEX_SHIFT;
    mmio(regs.write32(GUC_SHIM_CONTROL, shim))?;
    mmio(regs.write32(GT_PM_CONFIG, GT_DOORBELL_ENABLE))?;
    let mask = mmio(regs.read32(PMINTRMSK))?;
    mmio(regs.write32(PMINTRMSK, mask & !ARAT_EXPIRED_INTRMSK))
}

/// `guc_xfer_rsa`: the boot ROM reads a key larger than the scratch registers from GGTT;
/// a smaller one must fill all of them (`xe_uc_fw_copy_rsa` short copy is `-ENOMEM`).
pub fn transfer_rsa(
    regs: &mut impl RegisterFile,
    rsa: &[u8],
    rsa_ggtt: u64,
) -> Result<(), GucError> {
    const SCRATCH_BYTES: usize = UOS_RSA_SCRATCH_COUNT * 4;
    if rsa.len() > SCRATCH_BYTES {
        let addr = u32::try_from(rsa_ggtt).map_err(|_| GucError::BadImage)?;
        return mmio(regs.write32(UOS_RSA_SCRATCH_0, addr));
    }
    if rsa.len() < SCRATCH_BYTES {
        return Err(GucError::RsaTooShort);
    }
    for (i, word) in rsa.as_chunks::<4>().0.iter().enumerate() {
        let word = u32::from_le_bytes(*word);
        mmio(regs.write32(UOS_RSA_SCRATCH_0 + i * 4, word))?;
    }
    Ok(())
}

/// `uc_fw_xfer`: DMA the CSS header plus uCode (`size` bytes at GGTT address `src_ggtt`)
/// into WOPCM at `dst_wopcm`, waiting up to 100 ms for the engine.
pub fn upload_guc_firmware(
    regs: &mut impl RegisterFile,
    clock: &mut impl Clock,
    src_ggtt: u64,
    dst_wopcm: u32,
    size: usize,
) -> Result<(), GucError> {
    if size == 0 || !size.is_multiple_of(4) {
        return Err(GucError::BadImage);
    }
    let size = u32::try_from(size).map_err(|_| GucError::BadImage)?;
    // The high dword carries the address-space selector in bits 20:16.
    let src_high = u32::try_from(src_ggtt >> 32)
        .ok()
        .filter(|high| *high <= 0xFFFF)
        .ok_or(GucError::BadImage)?;

    mmio(regs.write32(GUC_DMA_ADDR_0_LOW, src_ggtt as u32))?;
    mmio(regs.write32(GUC_DMA_ADDR_0_HIGH, src_high | DMA_ADDRESS_SPACE_GGTT))?;
    mmio(regs.write32(GUC_DMA_ADDR_1_LOW, dst_wopcm))?;
    mmio(regs.write32(GUC_DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM))?;
    mmio(regs.write32(GUC_DMA_COPY_SIZE, size))?;

    let flags = GUC_DMA_CTRL_UOS_MOVE | GUC_DMA_CTRL_START;
    mmio(regs.write32(GUC_DMA_CTRL, masked_write(flags, flags)))?;
    let start = u32::from(GUC_DMA_CTRL_START);
    let done = wait32(regs, clock, GUC_DMA_CTRL, start, 0, DMA_TIMEOUT_US);
    // Upstream clears the move bit whether or not the copy finished.
    mmio(regs.write32(GUC_DMA_CTRL, masked_write(0, GUC_DMA_CTRL_UOS_MOVE)))?;
    done.map(|_| ()).map_err(|e| match e {
        WaitError::TimedOut(v) => GucError::DmaTimeout(v),
        WaitError::Mmio => GucError::Mmio,
    })
}

/// What `guc_load_done` makes of one `GUC_STATUS` value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    Running,
    Failed,
    InProgress,
}

/// `guc_load_done`: terminal microkernel codes (`XE_GUC_LOAD_STATUS_*`), then terminal
/// boot ROM codes (`XE_BOOTROM_STATUS_*`), else still loading.
pub fn guc_load_done(status: u32) -> LoadState {
    let ukernel = (status & GUC_STATUS_UKERNEL_MASK) >> 8;
    let bootrom = (status & GUC_STATUS_BOOTROM_MASK) >> 1;
    if status & GUC_STATUS_UKERNEL_MASK == GUC_STATUS_UKERNEL_READY {
        return LoadState::Running;
    }
    let ukernel_failed = matches!(
        ukernel,
        0x02..=0x04 | 0x07 | 0x08 | 0x60 | 0x70 | 0x71 | 0x73..=0x76
    );
    let bootrom_failed = matches!(
        bootrom,
        0x13 | 0x2B | 0x50 | 0x73 | 0x74 | 0x75 | 0x77 | 0x79 | 0x7A | 0x7E
    );
    if ukernel_failed || bootrom_failed {
        LoadState::Failed
    } else {
        LoadState::InProgress
    }
}

/// `guc_wait_ucode`: read `GUC_STATUS` every 10 ms until `guc_load_done` reports a terminal
/// state, for at most 3 s. Returns how long the load took in microseconds.
pub fn wait_guc_ukernel_running(
    regs: &impl RegisterFile,
    clock: &mut impl Clock,
) -> Result<u64, GucError> {
    let start = clock.now_us();
    loop {
        let status = mmio(regs.read32(GUC_STATUS))?;
        match guc_load_done(status) {
            LoadState::Running => return Ok(clock.now_us() - start),
            LoadState::Failed => return Err(GucError::LoadFailed(status)),
            LoadState::InProgress => {}
        }
        let elapsed = clock.now_us() - start;
        if elapsed >= LOAD_TIMEOUT_US {
            return Err(GucError::LoadTimeout(status));
        }
        clock.relax(LOAD_POLL_US.min(LOAD_TIMEOUT_US - elapsed));
    }
}

/// `xe_guc_mmio_send_recv` on the primary GT: request dwords into `VF_SW_FLAG(0..n)`,
/// a posting read of the last flag, notify, then wait for a reply with the GuC origin bit.
/// Returns the reply's `DATA0`.
pub fn mmio_send(
    regs: &mut impl RegisterFile,
    clock: &mut impl Clock,
    request: &[u32],
) -> Result<u32, GucError> {
    debug_assert!(!request.is_empty() && request.len() <= VF_SW_FLAG_COUNT);
    let started = clock.now_us();
    let mut sleep = 1_000u64;
    loop {
        for (i, &word) in request.iter().enumerate() {
            mmio(regs.write32(VF_SW_FLAG_0 + 4 * i, word))?;
        }
        mmio(regs.read32(VF_SW_FLAG_0 + 4 * (VF_SW_FLAG_COUNT - 1)))?;
        notify(regs)?;

        let origin = intel_gpu_core::guc_abi::HXG_ORIGIN_GUC;
        let mut header = wait32(
            regs,
            clock,
            VF_SW_FLAG_0,
            origin,
            origin,
            MMIO_REPLY_TIMEOUT_US,
        )?;
        if decode_mmio_reply(header) == MmioReply::Busy {
            // SUCCESS (7) and FAILURE (6) differ in one bit; wait for both upper type bits.
            let final_bits = 6 << 28;
            header = wait32(
                regs,
                clock,
                VF_SW_FLAG_0,
                final_bits,
                final_bits,
                MMIO_BUSY_TIMEOUT_US,
            )?;
        }
        match decode_mmio_reply(header) {
            MmioReply::Success(data0) => return Ok(data0),
            MmioReply::Retry if clock.now_us() - started < MMIO_RETRY_LIMIT_US => {
                clock.relax(sleep);
                sleep = (sleep * 2).min(256_000);
            }
            MmioReply::Retry => return Err(GucError::MmioTimeout(header)),
            MmioReply::Failure { hint, error } => {
                return Err(GucError::MmioFailure { hint, error })
            }
            _ => return Err(GucError::MmioProtocol(header)),
        }
    }
}

/// `xe_guc_notify`: the firmware ignores the payload so far, so upstream writes 0.
pub fn notify(regs: &mut impl RegisterFile) -> Result<(), GucError> {
    mmio(regs.write32(GUC_HOST_INTERRUPT, 0))
}
