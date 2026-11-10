//! Host-test model of a GPU running a first-light batch: `MI_STORE_DATA_IMM` and
//! `MI_BATCH_BUFFER_END`, fetched and executed through a client PPGTT.
//!
//! Upstream reference: `drivers/gpu/drm/xe/instructions/xe_mi_commands.h` (MIT). This is
//! test equipment: the real engine executes batches, the driver never interprets them.

use crate::ppgtt_store::ClientVm;
use alloc::vec::Vec;
use intel_gpu_core::lrc::{MI_BATCH_BUFFER_END, MI_NOOP, MI_STORE_DATA_IMM};
use intel_gpu_core::ppgtt::TableStore;
use types::{ViError, ViResult};

/// `MI_STORE_DATA_IMM | MI_SDI_NUM_DW(1)` with a PPGTT address: 4 dwords.
pub const STORE_DWORD_IMM: u32 = MI_STORE_DATA_IMM | 2;

/// Append an `MI_STORE_DATA_IMM` writing 32-bit `value` to PPGTT VA `target_va`.
pub fn emit_store_data_imm(buf: &mut Vec<u32>, target_va: u64, value: u32) {
    buf.extend_from_slice(&[
        STORE_DWORD_IMM,
        target_va as u32,
        (target_va >> 32) as u32,
        value,
    ]);
}

/// The first-light batch: write `marker` to `target_va`, then end.
pub fn build_store_marker_batch(target_va: u64, marker: u32) -> Vec<u32> {
    let mut batch = Vec::with_capacity(5);
    emit_store_data_imm(&mut batch, target_va, marker);
    batch.push(MI_BATCH_BUFFER_END);
    batch
}

/// Execute the batch at `batch_va` against `memory`, a byte image of device addresses
/// `[memory_base, memory_base + memory.len())`. Returns the instructions executed.
pub fn simulate_gpu_execution(
    vm: &ClientVm,
    store: &mut impl TableStore,
    batch_va: u64,
    memory: &mut [u8],
    memory_base: u64,
) -> ViResult<u32> {
    let mut va = batch_va;
    let mut executed = 0;
    loop {
        let dw0 = read_u32(vm, store, va, memory, memory_base)?;
        executed += 1;
        match dw0 {
            MI_BATCH_BUFFER_END => return Ok(executed),
            MI_NOOP => va += 4,
            STORE_DWORD_IMM => {
                let lo = read_u32(vm, store, va + 4, memory, memory_base)?;
                let hi = read_u32(vm, store, va + 8, memory, memory_base)?;
                let data = read_u32(vm, store, va + 12, memory, memory_base)?;
                let at = offset(
                    vm,
                    store,
                    u64::from(hi) << 32 | u64::from(lo),
                    memory,
                    memory_base,
                )?;
                memory[at..at + 4].copy_from_slice(&data.to_le_bytes());
                va += 16;
            }
            _ => return Err(ViError::NotSupported),
        }
    }
}

fn offset(
    vm: &ClientVm,
    store: &mut impl TableStore,
    va: u64,
    memory: &[u8],
    memory_base: u64,
) -> ViResult<usize> {
    let pa = vm.translate(store, va)?.ok_or(ViError::NotFound)?;
    let at = pa
        .checked_sub(memory_base)
        .ok_or(ViError::InvalidArgument)? as usize;
    if at + 4 > memory.len() {
        return Err(ViError::InvalidArgument);
    }
    Ok(at)
}

fn read_u32(
    vm: &ClientVm,
    store: &mut impl TableStore,
    va: u64,
    memory: &[u8],
    memory_base: u64,
) -> ViResult<u32> {
    let at = offset(vm, store, va, memory, memory_base)?;
    Ok(u32::from_le_bytes([
        memory[at],
        memory[at + 1],
        memory[at + 2],
        memory[at + 3],
    ]))
}
