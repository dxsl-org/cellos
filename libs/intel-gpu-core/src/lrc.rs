//! Logical ring context (LRC) of one render-engine exec queue on Xe2/Xe3 (graphics 20.xx
//! and 30.xx), and the ring commands of one job.
//!
//! Upstream reference (MIT): `drivers/gpu/drm/xe/xe_lrc.c` (`xe2_rcs_offsets`,
//! `set_offsets`, `set_context_control`, `xe_lrc_ctx_init`, `xe_lrc_init`,
//! `xe_lrc_descriptor`, `xe_lrc_write_ring`, `xe_lrc_ring_space`),
//! `regs/xe_lrc_layout.h` (`CTX_*`), `regs/xe_engine_regs.h` (`RING_CTL_SIZE`,
//! `CTX_CTRL_*`), `instructions/xe_mi_commands.h` and `xe_ring_ops.c`
//! (`emit_store_imm_ggtt`, `emit_bb_start`, `emit_user_interrupt`).
//!
//! The image is the one `xe` builds before it has recorded a golden context
//! (`empty_lrc_data`): register offsets and the ring, PPGTT, timestamp and ASID values,
//! with `CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT` so the hardware does not load the zeroed
//! engine state. Graphics 20.xx/30.0x have no indirect ring state (`graphics_xe2` in
//! `xe_pci.c`), so the ring registers live in this image.
//!
//! Allocation layout (byte offsets, one GGTT-contiguous driver allocation):
//! `[0, 16K)` ring (`xe_lrc_ring_size`), `[16K, 20K)` PPHWSP, `[20K, 32K)` register state
//! and engine image (`xe_gt_lrc_hang_replay_size`: 3 pages for render on graphics 20+),
//! `[32K, 36K)` the workaround batch (`LRC_WA_BB_SIZE`), `[36K, 40K)` the seqno page.
//! Upstream keeps the seqno in its own 4 KiB BO (`lrc->seqno_bo`); here it is the last
//! page of the same allocation, which changes no address the hardware is given.

use alloc::vec;
use alloc::vec::Vec;

/// `xe_lrc_ring_size()`.
pub const RING_SIZE: u32 = 16 * 1024;
/// `LRC_PPHWSP_SIZE`.
pub const PPHWSP_SIZE: u32 = 4096;
/// `xe_gt_lrc_hang_replay_size(gt, XE_ENGINE_CLASS_RENDER)` for graphics 20.xx-30.xx.
pub const RENDER_IMAGE_SIZE: u32 = 3 * 4096;
/// `LRC_WA_BB_SIZE`.
pub const WA_BB_SIZE: u32 = 4096;

pub const RING_OFFSET: u32 = 0;
pub const PPHWSP_OFFSET: u32 = RING_SIZE;
pub const REGS_OFFSET: u32 = PPHWSP_OFFSET + PPHWSP_SIZE;
pub const WA_BB_OFFSET: u32 = REGS_OFFSET + RENDER_IMAGE_SIZE;
pub const SEQNO_OFFSET: u32 = WA_BB_OFFSET + WA_BB_SIZE;
/// `LRC_START_SEQNO_OFFSET`: 8 bytes after the seqno.
pub const START_SEQNO_OFFSET: u32 = SEQNO_OFFSET + 8;
/// Bytes of the whole allocation.
pub const LRC_BYTES: u32 = SEQNO_OFFSET + 4096;

/// `RENDER_RING_BASE`: the engine MMIO base the register offsets are relative to.
pub const RENDER_RING_BASE: u32 = 0x2000;

// `regs/xe_lrc_layout.h`: dword indices into the register state.
pub const CTX_CONTEXT_CONTROL: usize = 0x02 + 1;
pub const CTX_RING_HEAD: usize = 0x04 + 1;
pub const CTX_RING_TAIL: usize = 0x06 + 1;
pub const CTX_RING_START: usize = 0x08 + 1;
pub const CTX_RING_CTL: usize = 0x0a + 1;
pub const CTX_BB_PER_CTX_PTR: usize = 0x12 + 1;
pub const CTX_TIMESTAMP: usize = 0x22 + 1;
pub const CTX_TIMESTAMP_UDW: usize = 0x24 + 1;
pub const CTX_ASID: usize = 0x2e + 1;
pub const CTX_PDP0_UDW: usize = 0x30 + 1;
pub const CTX_PDP0_LDW: usize = 0x32 + 1;

/// `HEAD_ADDR` (bits 20:2) of `RING_HEAD`.
pub const HEAD_ADDR: u32 = 0x001f_fffc;
/// `RING_VALID` in `RING_CTL`.
pub const RING_VALID: u32 = 1;
/// `CTX_CTRL_INHIBIT_SYN_CTX_SWITCH`.
pub const CTX_CTRL_INHIBIT_SYN_CTX_SWITCH: u32 = 1 << 3;
/// `CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT`.
pub const CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT: u32 = 1 << 0;

// LRC descriptor bits (`xe_lrc.c`).
pub const LRC_VALID: u64 = 1 << 0;
/// `LRC_ADDRESSING_MODE` = `LRC_LEGACY_64B_CONTEXT` (3) in bits 4:3.
pub const LRC_LEGACY_64B_CONTEXT: u64 = 3 << 3;
/// `LRC_PRIVILEGE`: set for every context with a VM ("really just means PPGTT mode").
pub const LRC_PRIVILEGE: u64 = 1 << 8;

// `instructions/xe_mi_commands.h`: `__MI_INSTR(op)` is `op << 23`.
pub const MI_NOOP: u32 = 0;
pub const MI_USER_INTERRUPT: u32 = 0x2 << 23;
pub const MI_ARB_CHECK: u32 = 0x5 << 23;
pub const MI_ARB_ON_OFF: u32 = 0x8 << 23;
pub const MI_ARB_ENABLE: u32 = 1;
pub const MI_ARB_DISABLE: u32 = 0;
pub const MI_BATCH_BUFFER_END: u32 = 0xa << 23;
pub const MI_STORE_DATA_IMM: u32 = 0x20 << 23;
pub const MI_SDI_GGTT: u32 = 1 << 22;
pub const MI_LOAD_REGISTER_IMM: u32 = 0x22 << 23;
pub const MI_LRI_LRM_CS_MMIO: u32 = 1 << 19;
pub const MI_LRI_FORCE_POSTED: u32 = 1 << 12;
pub const MI_BATCH_BUFFER_START: u32 = 0x31 << 23;
/// `BIT(8)` of `MI_BATCH_BUFFER_START` (`get_ppgtt_flag`): the address is a PPGTT VA.
pub const BB_PPGTT: u32 = 1 << 8;

/// `XE_INSTR_NUM_DW(x)`: the length field holds the dword count minus 2.
pub const fn instr_num_dw(dwords: u32) -> u32 {
    dwords - 2
}

/// `MI_SDI_NUM_DW(x)`.
pub const fn mi_sdi_num_dw(dwords: u32) -> u32 {
    dwords + 3 - 2
}

/// `MI_LRI_NUM_REGS(x)`.
pub const fn mi_lri_num_regs(count: u32) -> u32 {
    instr_num_dw(2 * count + 1)
}

/// `REG_MASKED_FIELD_ENABLE`: value bits plus their write-enable bits.
pub const fn masked_enable(bits: u32) -> u32 {
    bits << 16 | bits
}

/// `RING_CTL_SIZE(size)`.
pub const fn ring_ctl_size(size: u32) -> u32 {
    size - 4096
}

// The `xe_lrc.c` table encoding: one byte per command, register offsets in 7-bit groups.
const fn nop(count: u8) -> u8 {
    0x80 | count
}
const fn lri(count: u8, posted: bool) -> u8 {
    (posted as u8) << 6 | count
}
const fn reg(offset: u16) -> u8 {
    (offset >> 2) as u8
}
const fn reg16_hi(offset: u16) -> u8 {
    (offset >> 9) as u8 | 0x80
}
const fn reg16_lo(offset: u16) -> u8 {
    ((offset >> 2) & 0x7f) as u8
}

/// `xe2_rcs_offsets` (`XE2_CTX_COMMON` plus the render tail), byte for byte.
pub const XE2_RCS_OFFSETS: &[u8] = &[
    nop(1),
    lri(15, true),
    reg16_hi(0x244),
    reg16_lo(0x244), // CTXT_SR_CTL
    reg(0x034),      // RING_BUFFER_HEAD
    reg(0x030),      // RING_BUFFER_TAIL
    reg(0x038),      // RING_BUFFER_START
    reg(0x03c),      // RING_BUFFER_CONTROL
    reg(0x168),      // BB_ADDR_UDW
    reg(0x140),      // BB_ADDR
    reg(0x110),      // BB_STATE
    reg(0x1c0),      // BB_PER_CTX_PTR
    reg(0x1c4),      // RCS_INDIRECT_CTX
    reg(0x1c8),      // RCS_INDIRECT_CTX_OFFSET
    reg(0x180),      // CCID
    reg16_hi(0x2b4),
    reg16_lo(0x2b4), // SEMAPHORE_TOKEN
    reg(0x120),      // PRT_BB_STATE
    reg(0x124),      // PRT_BB_STATE_UDW
    nop(1),
    lri(9, true),
    reg16_hi(0x3a8),
    reg16_lo(0x3a8), // CTX_TIMESTAMP
    reg16_hi(0x3ac),
    reg16_lo(0x3ac), // CTX_TIMESTAMP_UDW
    reg(0x108),      // INDIRECT_RING_STATE
    reg16_hi(0x284),
    reg16_lo(0x284), // dummy reg
    reg16_hi(0x280),
    reg16_lo(0x280), // CS_ACC_CTR_THOLD
    reg16_hi(0x27c),
    reg16_lo(0x27c), // CS_CTX_SYS_PASID
    reg16_hi(0x278),
    reg16_lo(0x278), // CS_CTX_ASID
    reg16_hi(0x274),
    reg16_lo(0x274), // PTBP_UDW
    reg16_hi(0x270),
    reg16_lo(0x270), // PTBP_LDW
    nop(2),
    lri(2, true),
    reg16_hi(0x5a8),
    reg16_lo(0x5a8), // CONTEXT_SCHEDULING_ATTRIBUTES
    reg16_hi(0x5ac),
    reg16_lo(0x5ac), // PREEMPTION_STATUS
    nop(6),
    lri(1, false),
    reg(0x0c8), // R_PWR_CLK_STATE
    0,
];

/// `set_offsets`: write the `MI_LOAD_REGISTER_IMM` headers and register addresses a table
/// describes into `regs`, leaving the value slots untouched, then the closing
/// `MI_BATCH_BUFFER_END | BIT(0)`.
pub fn set_offsets(regs: &mut [u32], table: &[u8], mmio_base: u32) {
    let mut at = 0;
    let mut data = table.iter().copied();
    while let Some(byte) = data.next().filter(|&b| b != 0) {
        if byte & 0x80 != 0 {
            at += usize::from(byte & 0x7f);
            continue;
        }
        let count = u32::from(byte & 0x3f);
        let posted = byte >> 6 & 1 != 0;
        regs[at] = MI_LOAD_REGISTER_IMM | mi_lri_num_regs(count) | MI_LRI_LRM_CS_MMIO;
        if posted {
            regs[at] |= MI_LRI_FORCE_POSTED;
        }
        at += 1;
        for _ in 0..count {
            let mut offset = 0u32;
            loop {
                let v = data.next().unwrap_or(0);
                offset = offset << 7 | u32::from(v & 0x7f);
                if v & 0x80 == 0 {
                    break;
                }
            }
            regs[at] = mmio_base + (offset << 2);
            at += 2;
        }
    }
    regs[at] = MI_BATCH_BUFFER_END | 1;
}

/// Byte offset inside the allocation of register-state dword `index`.
pub const fn ctx_reg_offset(index: usize) -> u32 {
    REGS_OFFSET + (index as u32) * 4
}

/// What [`render_lrc`] needs to know about where things are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LrcParams {
    /// GGTT address of the allocation (its ring starts there).
    pub ggtt: u32,
    /// The VM's root page-directory entry (`xe_vm_pdp4_descriptor`: root table address,
    /// present, RW, WB PAT).
    pub pdp: u64,
    /// The VM's address-space id (`has_asid` is set for graphics 20.xx-30.xx).
    pub asid: u32,
    /// The seqno value already "completed" (`next_seqno - 1`).
    pub initial_seqno: u32,
}

/// A freshly initialized render LRC: the whole allocation's dwords and the CPU-side ring
/// tail (`lrc->ring.tail`) after the `MI_ARB_ON_OFF | MI_ARB_ENABLE` prologue
/// `xe_lrc_ctx_init` writes.
pub fn render_lrc(params: &LrcParams) -> (Vec<u32>, u32) {
    let mut image = vec![0u32; LRC_BYTES as usize / 4];
    let dw = |byte: u32| byte as usize / 4;

    let regs_len = RENDER_IMAGE_SIZE as usize / 4;
    let regs = &mut image[dw(REGS_OFFSET)..dw(REGS_OFFSET) + regs_len];
    set_offsets(regs, XE2_RCS_OFFSETS, RENDER_RING_BASE);
    // `set_context_control`.
    regs[CTX_CONTEXT_CONTROL] =
        masked_enable(CTX_CTRL_INHIBIT_SYN_CTX_SWITCH | CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT);
    // `xe_lrc_set_ppgtt`.
    regs[CTX_PDP0_UDW] = (params.pdp >> 32) as u32;
    regs[CTX_PDP0_LDW] = params.pdp as u32;
    // Ring registers without indirect ring state; head and tail start equal (0).
    regs[CTX_RING_START] = params.ggtt + RING_OFFSET;
    regs[CTX_RING_HEAD] = 0;
    regs[CTX_RING_TAIL] = 0;
    regs[CTX_RING_CTL] = ring_ctl_size(RING_SIZE) | RING_VALID;
    regs[CTX_TIMESTAMP] = 0;
    regs[CTX_TIMESTAMP_UDW] = 0;
    regs[CTX_ASID] = params.asid;
    // `xe_lrc_setup_wa_bb_with_scratch` with no workaround batch to emit: only the end,
    // and `BB_PER_CTX_PTR` = its GGTT address with the valid bit (bit 0).
    regs[CTX_BB_PER_CTX_PTR] = params.ggtt + WA_BB_OFFSET + 1;
    image[dw(WA_BB_OFFSET)] = MI_BATCH_BUFFER_END;

    image[dw(SEQNO_OFFSET)] = params.initial_seqno;
    image[dw(START_SEQNO_OFFSET)] = params.initial_seqno;

    // `xe_lrc_write_ring` of the arbitration enable, padded to a qword with a NOOP.
    image[dw(RING_OFFSET)] = MI_ARB_ON_OFF | MI_ARB_ENABLE;
    image[dw(RING_OFFSET) + 1] = MI_NOOP;
    (image, 8)
}

/// `xe_lrc_descriptor`: the descriptor flags plus the PPHWSP's GGTT address, as GuC
/// `REGISTER_CONTEXT` takes it.
pub const fn descriptor(ggtt: u32) -> u64 {
    LRC_VALID | LRC_LEGACY_64B_CONTEXT | LRC_PRIVILEGE | (ggtt + PPHWSP_OFFSET) as u64
}

/// Dwords one job takes in the ring, qword aligned.
pub const JOB_DWORDS: usize = 16;

/// One job: record the start seqno, run the batch at PPGTT VA `batch_va`, record the
/// completion seqno, raise the user interrupt (`emit_store_imm_ggtt`, `emit_bb_start`,
/// `emit_user_interrupt`). Upstream's render jobs also flush and invalidate the render
/// caches with `PIPE_CONTROL`s around the batch; this job does not, so the seqno may land
/// before the batch's writes leave the render caches.
pub fn job(ggtt: u32, batch_va: u64, seqno: u32) -> [u32; JOB_DWORDS] {
    let sdi = MI_STORE_DATA_IMM | MI_SDI_GGTT | mi_sdi_num_dw(1);
    [
        sdi,
        ggtt + START_SEQNO_OFFSET,
        0,
        seqno,
        MI_BATCH_BUFFER_START | BB_PPGTT | instr_num_dw(3),
        batch_va as u32,
        (batch_va >> 32) as u32,
        // Don't preempt fence signalling.
        MI_ARB_ON_OFF | MI_ARB_DISABLE,
        sdi,
        ggtt + SEQNO_OFFSET,
        0,
        seqno,
        MI_USER_INTERRUPT,
        MI_ARB_ON_OFF | MI_ARB_ENABLE,
        MI_ARB_CHECK,
        MI_NOOP,
    ]
}

/// `xe_lrc_ring_space`: free bytes between the CPU tail and the hardware head.
pub const fn ring_space(head: u32, tail: u32) -> u32 {
    (head.wrapping_sub(tail).wrapping_sub(1) & (RING_SIZE - 1)) + 1
}

#[cfg(test)]
#[path = "lrc/tests.rs"]
mod tests;
