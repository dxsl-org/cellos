#![allow(dead_code, unused_imports)]
//! GPU batch buffer instructions and execution simulator for First Light testing.
//!
//! Upstream reference: Intel PRMs (Command Reference: Instructions) and
//! Linux `drivers/gpu/drm/xe/xe_ring.c` (MIT).

use crate::ppgtt_store::ClientVm;
use alloc::vec::Vec;
use intel_gpu_core::ppgtt::TableStore;
use types::{ViError, ViResult};

pub const MI_NOOP: u32 = 0x0000_0000;
pub const MI_BATCH_BUFFER_END: u32 = 0x0500_0000;
pub const MI_STORE_DATA_IMM: u32 = 0x1000_0002; // Opcode 0x20 << 23 | length 2 (4 dwords total)

/// Append an `MI_STORE_DATA_IMM` instruction writing 32-bit `value` to 64-bit `target_va`.
pub fn emit_store_data_imm(buf: &mut Vec<u32>, target_va: u64, value: u32) {
    buf.push(MI_STORE_DATA_IMM);
    buf.push((target_va & 0xFFFF_FFFF) as u32);
    buf.push((target_va >> 32) as u32);
    buf.push(value);
}

/// Append an `MI_BATCH_BUFFER_END` instruction terminating execution.
pub fn emit_batch_buffer_end(buf: &mut Vec<u32>) {
    buf.push(MI_BATCH_BUFFER_END);
}

/// Helper to build a complete First Light batch that writes `marker` to `target_va`.
pub fn build_store_marker_batch(target_va: u64, marker: u32) -> Vec<u32> {
    let mut batch = Vec::with_capacity(5);
    emit_store_data_imm(&mut batch, target_va, marker);
    emit_batch_buffer_end(&mut batch);
    batch
}

/// Simulated GPU execution engine for First Light host tests.
///
/// Reads batch buffer instructions starting at `batch_va` through the VM's PPGTT,
/// executes them against simulated physical memory, and writes output markers.
pub fn simulate_gpu_execution<S: TableStore>(
    vm: &mut ClientVm<S>,
    batch_va: u64,
    phys_mem: &mut [u8],
    phys_base: u64,
) -> ViResult<u32> {
    let mut current_va = batch_va;
    let mut instructions_executed = 0;

    loop {
        // Read instruction dword through VM translation
        let pa = vm.translate(current_va)?.ok_or(ViError::NotFound)?;
        let mem_offset = pa.checked_sub(phys_base).ok_or(ViError::InvalidArgument)? as usize;

        if mem_offset + 4 > phys_mem.len() {
            return Err(ViError::InvalidArgument);
        }

        let dw0 = u32::from_le_bytes([
            phys_mem[mem_offset],
            phys_mem[mem_offset + 1],
            phys_mem[mem_offset + 2],
            phys_mem[mem_offset + 3],
        ]);

        instructions_executed += 1;

        if dw0 == MI_BATCH_BUFFER_END {
            break;
        } else if dw0 == MI_NOOP {
            current_va += 4;
        } else if dw0 == MI_STORE_DATA_IMM {
            // Read target address and immediate value
            let dw1 = read_u32_va(vm, current_va + 4, phys_mem, phys_base)?;
            let dw2 = read_u32_va(vm, current_va + 8, phys_mem, phys_base)?;
            let data = read_u32_va(vm, current_va + 12, phys_mem, phys_base)?;

            let target_va = (dw1 as u64) | ((dw2 as u64) << 32);

            // Translate destination VA and write marker to simulated physical memory
            let target_pa = vm.translate(target_va)?.ok_or(ViError::NotFound)?;
            let target_offset = target_pa
                .checked_sub(phys_base)
                .ok_or(ViError::InvalidArgument)? as usize;

            if target_offset + 4 > phys_mem.len() {
                return Err(ViError::InvalidArgument);
            }

            phys_mem[target_offset..target_offset + 4].copy_from_slice(&data.to_le_bytes());
            current_va += 16;
        } else {
            // Unknown or unsupported GPU instruction
            return Err(ViError::NotSupported);
        }
    }

    Ok(instructions_executed)
}

fn read_u32_va<S: TableStore>(
    vm: &mut ClientVm<S>,
    va: u64,
    phys_mem: &[u8],
    phys_base: u64,
) -> ViResult<u32> {
    let pa = vm.translate(va)?.ok_or(ViError::NotFound)?;
    let offset = pa.checked_sub(phys_base).ok_or(ViError::InvalidArgument)? as usize;
    if offset + 4 > phys_mem.len() {
        return Err(ViError::InvalidArgument);
    }
    Ok(u32::from_le_bytes([
        phys_mem[offset],
        phys_mem[offset + 1],
        phys_mem[offset + 2],
        phys_mem[offset + 3],
    ]))
}
