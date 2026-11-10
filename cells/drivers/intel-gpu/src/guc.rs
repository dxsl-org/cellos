//! Intel GuC firmware upload, DMA transfer, and status verification.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_guc.c`, `xe_uc_fw.c`, `regs/xe_guc_regs.h`,
//! `regs/xe_gt_regs.h`, `abi/guc_errors_abi.h` (MIT).
//!
//! [`load_guc`] follows `__xe_guc_upload` for the steps that need only the image: GuC reset,
//! RSA hand-off, uCode DMA and the boot wait. It does not yet write the GuC parameter block
//! (`guc_write_params`, which needs the ADS) or `guc_prepare_xfer`'s shim/PM registers; on
//! hardware a firmware that needs them reports its own load failure through `GUC_STATUS`.

use crate::regs::{masked_write, RegisterFile, GDRST, GRDOM_GUC};
use intel_gpu_core::guc_fw::{GucImage, CSS_HEADER_LEN};
use types::{ViError, ViResult};

// ── GuC Status Register ──────────────────────────────────────────────────────

pub const GUC_STATUS: usize = 0x00C000;
pub const GUC_STATUS_MIA_IN_RESET: u32 = 1 << 0;
/// Boot ROM progress code, bits 7:1.
pub const GUC_STATUS_BOOTROM_MASK: u32 = 0x7F << 1;
/// Microkernel load status, bits 15:8.
pub const GUC_STATUS_UKERNEL_MASK: u32 = 0xFF << 8;
/// `XE_GUC_LOAD_STATUS_READY` in the microkernel field.
pub const GUC_STATUS_UKERNEL_READY: u32 = 0xF0 << 8;

// ── GuC Firmware DMA Transfer Registers ───────────────────────────────────────

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

/// WOPCM offset the GuC image is copied to (`xe_guc_upload`).
pub const GUC_WOPCM_LOAD_OFFSET: u32 = 0x2000;

pub const GUC_POLL_TIMEOUT_TRIES: usize = 50_000;

/// Check that the GuC is held in reset, the only state from which it accepts an image.
pub fn check_guc_in_reset(regs: &impl RegisterFile) -> ViResult<()> {
    regs.poll_bit(
        GUC_STATUS,
        GUC_STATUS_MIA_IN_RESET,
        true,
        GUC_POLL_TIMEOUT_TRIES,
    )
}

/// DMA the CSS header plus uCode (`size` bytes at GGTT address `src_ggtt`) into WOPCM.
pub fn upload_guc_firmware(
    regs: &mut impl RegisterFile,
    src_ggtt: u64,
    dst_wopcm: u32,
    size: usize,
) -> ViResult<()> {
    if size == 0 || !size.is_multiple_of(4) {
        return Err(ViError::InvalidArgument);
    }
    let size = u32::try_from(size).map_err(|_| ViError::InvalidArgument)?;
    // The high dword carries the address-space selector in bits 20:16.
    let src_high = u32::try_from(src_ggtt >> 32)
        .ok()
        .filter(|high| *high <= 0xFFFF)
        .ok_or(ViError::InvalidArgument)?;

    regs.write32(GUC_DMA_ADDR_0_LOW, src_ggtt as u32)?;
    regs.write32(GUC_DMA_ADDR_0_HIGH, src_high | DMA_ADDRESS_SPACE_GGTT)?;
    regs.write32(GUC_DMA_ADDR_1_LOW, dst_wopcm)?;
    regs.write32(GUC_DMA_ADDR_1_HIGH, DMA_ADDRESS_SPACE_WOPCM)?;
    regs.write32(GUC_DMA_COPY_SIZE, size)?;

    let flags = GUC_DMA_CTRL_UOS_MOVE | GUC_DMA_CTRL_START;
    regs.write32(GUC_DMA_CTRL, masked_write(flags, flags))?;
    let done = regs.poll_bit(
        GUC_DMA_CTRL,
        u32::from(GUC_DMA_CTRL_START),
        false,
        GUC_POLL_TIMEOUT_TRIES,
    );
    // Upstream clears the move bit whether or not the copy finished.
    regs.write32(GUC_DMA_CTRL, masked_write(0, GUC_DMA_CTRL_UOS_MOVE))?;
    done
}

/// Wait until the GuC microkernel reports ready, failing fast on a terminal error code.
pub fn wait_guc_ukernel_running(regs: &impl RegisterFile) -> ViResult<()> {
    for _ in 0..GUC_POLL_TIMEOUT_TRIES {
        let status = regs.read32(GUC_STATUS)?;
        let ukernel = (status & GUC_STATUS_UKERNEL_MASK) >> 8;
        let bootrom = (status & GUC_STATUS_BOOTROM_MASK) >> 1;
        if status & GUC_STATUS_UKERNEL_MASK == GUC_STATUS_UKERNEL_READY {
            return Ok(());
        }
        // `guc_load_done`: terminal microkernel and boot ROM failure codes.
        let ukernel_failed = matches!(
            ukernel,
            0x02..=0x04 | 0x07 | 0x08 | 0x60 | 0x70 | 0x71 | 0x73..=0x76
        );
        let bootrom_failed = matches!(
            bootrom,
            0x13 | 0x2B | 0x50 | 0x73 | 0x74 | 0x75 | 0x77 | 0x79 | 0x7A | 0x7E
        );
        if ukernel_failed || bootrom_failed {
            return Err(ViError::IO);
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

/// `UOS_RSA_SCRATCH(0)`: the first of the registers that take an RSA signature of up to
/// [`UOS_RSA_SCRATCH_COUNT`] dwords, or the GGTT address of a larger one.
pub const UOS_RSA_SCRATCH_0: usize = 0x00C200;
pub const UOS_RSA_SCRATCH_COUNT: usize = 64;

/// `xe_guc_reset`: reset the GuC domain, then require the microcontroller held in reset.
pub fn reset_guc(regs: &mut impl RegisterFile) -> ViResult<()> {
    regs.write32(GDRST, GRDOM_GUC)?;
    regs.poll_bit(GDRST, GRDOM_GUC, false, GUC_POLL_TIMEOUT_TRIES)?;
    check_guc_in_reset(regs)
}

/// `guc_xfer_rsa`: the boot ROM reads a key larger than the scratch registers from GGTT;
/// a smaller one must fill all of them (`xe_uc_fw_copy_rsa` short copy is `-ENOMEM`).
pub fn transfer_rsa(regs: &mut impl RegisterFile, rsa: &[u8], rsa_ggtt: u64) -> ViResult<()> {
    const SCRATCH_BYTES: usize = UOS_RSA_SCRATCH_COUNT * 4;
    if rsa.len() > SCRATCH_BYTES {
        let addr = u32::try_from(rsa_ggtt).map_err(|_| ViError::InvalidArgument)?;
        return regs.write32(UOS_RSA_SCRATCH_0, addr);
    }
    if rsa.len() < SCRATCH_BYTES {
        return Err(ViError::InvalidArgument);
    }
    for (i, word) in rsa.chunks_exact(4).enumerate() {
        let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        regs.write32(UOS_RSA_SCRATCH_0 + i * 4, word)?;
    }
    Ok(())
}

/// Load the GuC from a parsed image whose blob is mapped at GGTT address `fw_ggtt`:
/// reset -> RSA -> DMA of the CSS header plus uCode into WOPCM -> wait for the microkernel.
pub fn load_guc(
    regs: &mut impl RegisterFile,
    image: &GucImage,
    rsa: &[u8],
    fw_ggtt: u64,
) -> ViResult<()> {
    reset_guc(regs)?;
    transfer_rsa(regs, rsa, fw_ggtt + image.rsa_offset as u64)?;
    upload_guc_firmware(
        regs,
        fw_ggtt,
        GUC_WOPCM_LOAD_OFFSET,
        CSS_HEADER_LEN + image.ucode_len,
    )?;
    wait_guc_ukernel_running(regs)
}
