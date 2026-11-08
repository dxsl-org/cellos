#![allow(dead_code, unused_imports)]
//! Intel GuC firmware upload, DMA transfer, and status verification.
//!
//! Upstream reference: `drivers/gpu/drm/xe/xe_guc.c`, `xe_uc_fw.c`, `xe_guc_regs.h` (MIT).

use crate::regs::RegisterFile;
use types::{ViError, ViResult};

// ── GuC Status Registers and Bitmasks ─────────────────────────────────────────

pub const GUC_STATUS: usize = 0x00C000;
pub const GUC_STATUS_MIA_IN_RESET: u32 = 1 << 0;
pub const GUC_STATUS_BOOTLOADER_READY: u32 = 1 << 1;
pub const GUC_STATUS_UKERNEL_MASK: u32 = 0xF << 4;
pub const GUC_STATUS_UKERNEL_READY: u32 = 0x5 << 4; // 0x50

// ── GuC Firmware DMA Transfer Registers ───────────────────────────────────────

pub const GUC_DMA_ADDR_0_LOW: usize = 0x00C2C0;
pub const GUC_DMA_ADDR_0_HIGH: usize = 0x00C2C4;
pub const GUC_DMA_ADDR_1_LOW: usize = 0x00C2C8;
pub const GUC_DMA_ADDR_1_HIGH: usize = 0x00C2CC;
pub const GUC_DMA_COPY_SIZE: usize = 0x00C2D0;
pub const GUC_DMA_CTRL: usize = 0x00C2D4;
pub const GUC_DMA_CTRL_START: u32 = 1 << 0;

pub const GUC_POLL_TIMEOUT_TRIES: usize = 50_000;

/// Check that the GuC hardware bootloader is out of reset and ready for firmware.
pub fn check_guc_bootloader_ready(regs: &impl RegisterFile) -> ViResult<()> {
    regs.poll_bit(
        GUC_STATUS,
        GUC_STATUS_BOOTLOADER_READY,
        true,
        GUC_POLL_TIMEOUT_TRIES,
    )
}

/// Transfer GuC firmware from host memory to GuC SRAM using the dedicated DMA engine.
pub fn upload_guc_firmware(
    regs: &mut impl RegisterFile,
    src_phys: u64,
    dst_sram: u32,
    size: usize,
) -> ViResult<()> {
    if size == 0 || size % 4 != 0 {
        return Err(ViError::InvalidArgument);
    }

    // Source host memory address (64-bit)
    regs.write32(GUC_DMA_ADDR_0_LOW, (src_phys & 0xFFFF_FFFF) as u32)?;
    regs.write32(GUC_DMA_ADDR_0_HIGH, (src_phys >> 32) as u32)?;

    // Destination in GuC SRAM
    regs.write32(GUC_DMA_ADDR_1_LOW, dst_sram)?;
    regs.write32(GUC_DMA_ADDR_1_HIGH, 0)?;

    // Size in bytes
    regs.write32(GUC_DMA_COPY_SIZE, size as u32)?;

    // Start DMA
    regs.write32(GUC_DMA_CTRL, GUC_DMA_CTRL_START)?;

    // Poll until DMA completes (START bit cleared by hardware)
    regs.poll_bit(
        GUC_DMA_CTRL,
        GUC_DMA_CTRL_START,
        false,
        GUC_POLL_TIMEOUT_TRIES,
    )
}

/// Wait until the GuC microkernel is running and ready.
pub fn wait_guc_ukernel_running(regs: &impl RegisterFile) -> ViResult<()> {
    for _ in 0..GUC_POLL_TIMEOUT_TRIES {
        let status = regs.read32(GUC_STATUS)?;
        if status & GUC_STATUS_UKERNEL_MASK == GUC_STATUS_UKERNEL_READY {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err(ViError::IO)
}

/// Full GuC upload sequence: verify bootloader -> DMA firmware -> wait running.
pub fn bring_up_guc(regs: &mut impl RegisterFile, fw_phys: u64, fw_size: usize) -> ViResult<()> {
    check_guc_bootloader_ready(regs)?;
    upload_guc_firmware(regs, fw_phys, 0, fw_size)?;
    wait_guc_ukernel_running(regs)
}
