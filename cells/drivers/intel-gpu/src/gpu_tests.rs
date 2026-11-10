//! Host tests of the device half of the `xe` backend on a GPU brought up against the fake
//! MMIO and GuC: PPGTT leaves in device memory, GuC context registration and submission
//! dwords, bans from G2H, TLB invalidation and context teardown waits.

use crate::bring_up::{bring_up, PciIdentity, Platform, PINNED_GUC};
use crate::fake::{FakeClock, FakeDma, FakeExports, FakeRegs};
use crate::gpu::Gpu;
use alloc::vec;
use alloc::vec::Vec;
use intel_gpu_core::backend::{XeError, XE_ENGINE_CLASS_RENDER, XE_EXEC_QUEUE_BAN_FLAG_HANG};
use intel_gpu_core::guc_abi::*;
use intel_gpu_core::hw::GpuHw;
use intel_gpu_core::lrc;
use intel_gpu_core::pte::{self, PatIndex};
use types::ViError;

pub type TestGpu = Gpu<FakeRegs, FakeClock, FakeDma, FakeExports>;

/// A Lunar Lake GPU brought up on the fakes, with an empty export table.
pub fn test_gpu() -> TestGpu {
    struct Firmware(Vec<u8>);
    impl Platform for Firmware {
        fn read_firmware(&mut self, _: &str) -> Result<Option<usize>, ViError> {
            Ok(Some(self.0.len()))
        }
        fn with_firmware<T>(&mut self, f: impl FnOnce(&[u8]) -> T) -> T {
            f(&self.0)
        }
        fn map_firmware(&mut self) -> Result<u64, ViError> {
            Ok(0x2000_0000)
        }
    }
    let mut blob = vec![0u8; 128 + (16 + 64) * 4];
    for (off, v) in [(4, 96u32), (24, 112), (28, 64)] {
        blob[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    let sw = (70 << 16) | (u32::from(PINNED_GUC.minor) << 8) | u32::from(PINNED_GUC.patch);
    blob[64..68].copy_from_slice(&sw.to_le_bytes());
    let regs = FakeRegs::for_lunar_lake();
    let dma = regs.dma();
    let exports = FakeExports::new(regs.device.clone());
    let pci = PciIdentity {
        device_id: 0x6420,
        revision: 0,
    };
    let ready = bring_up(regs, FakeClock::default(), dma, pci, &mut Firmware(blob)).expect("ready");
    Gpu::new(
        ready.controller.regs,
        ready.controller.clock,
        ready.mem,
        exports,
        ready.ggtt,
        ready.pool,
        ready.ct,
    )
}

fn h2g(gpu: &TestGpu) -> Vec<Vec<u32>> {
    gpu.regs.device.borrow().h2g.clone()
}

/// The HXG action words (minus the CT `FAST_REQUEST` type bits) sent since `from`.
fn actions_since(gpu: &TestGpu, from: usize) -> Vec<Vec<u32>> {
    h2g(gpu)[from..]
        .iter()
        .map(|m| {
            let mut m = m.clone();
            assert_eq!(m[0] >> 28, 2, "sent as FAST_REQUEST");
            m[0] &= 0xffff;
            m
        })
        .collect()
}

const WINDOW: u64 = 1 << 38;

#[test]
fn leaves_point_into_the_window_and_honour_read_only() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    gpu.vm_map(vm, 0x10_0000, WINDOW + 0x2000, 0x2000, PatIndex::WB, true)
        .unwrap();
    gpu.vm_map(vm, 0x20_0000, WINDOW, 0x1000, PatIndex::WB, false)
        .unwrap();
    gpu.vm_map_null(vm, 0x30_0000, 0x1000, PatIndex::WB)
        .unwrap();
    assert_eq!(gpu.translate(vm, 0x10_1004), Some(WINDOW + 0x3004));
    let rw = gpu.leaf(vm, 0x10_0000).unwrap();
    let ro = gpu.leaf(vm, 0x20_0000).unwrap();
    let null = gpu.leaf(vm, 0x30_0000).unwrap();
    assert_eq!(
        rw,
        pte::encode_leaf(WINDOW + 0x2000, 0, PatIndex::WB, true, 46).unwrap()
    );
    assert_eq!(ro & pte::PAGE_RW, 0);
    assert_eq!(null, pte::encode_null_leaf(PatIndex::WB));
}

#[test]
fn unmap_clears_leaves_and_waits_for_the_tlb_invalidation() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    gpu.vm_map(vm, 0x10_0000, WINDOW, 0x2000, PatIndex::WB, true)
        .unwrap();
    let sent = h2g(&gpu).len();
    gpu.vm_unmap(vm, 0x10_0000, 0x2000);
    assert_eq!(gpu.translate(vm, 0x10_0000), None);
    assert_eq!(gpu.translate(vm, 0x10_1000), None);
    assert_eq!(actions_since(&gpu, sent), [vec![0x7000, 1, 0x8000_0000]]);
    assert_eq!(gpu.faults.tlb_timeouts, 0);

    // A GuC that never confirms: the wait ends at its bound and is counted.
    gpu.regs.device.borrow_mut().silent = true;
    let before = gpu.clock.now;
    gpu.vm_unmap(vm, 0x10_0000, 0x1000);
    assert_eq!(gpu.faults.tlb_timeouts, 1);
    assert!(gpu.clock.now - before >= crate::gpu::TLB_INVAL_TIMEOUT_US);
}

#[test]
fn queue_create_writes_the_lrc_and_registers_the_context() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    let sent = h2g(&gpu).len();
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    let msgs = actions_since(&gpu, sent);
    assert_eq!(msgs.len(), 1);
    let reg = &msgs[0];
    // `__register_exec_queue`: KMD normal context, RCS (GuC class 0), instance mask 1, no WQ.
    assert_eq!(&reg[..10], &[0x4502, 1, q.0, 0, 1, 0, 0, 0, 0, 0]);
    let desc = u64::from(reg[10]) | u64::from(reg[11]) << 32;
    assert_eq!(desc & 0xfff, 0x119);
    // The LRC the descriptor names: PPHWSP at desc, register state 4 KiB after it.
    let pphwsp = desc & !0xfff;
    let lrc_ggtt = pphwsp - u64::from(lrc::PPHWSP_OFFSET);
    let regs_iova = gpu.regs.ggtt_to_iova(pphwsp + 4096);
    let dev = gpu.regs.device.borrow();
    let reg_at = |i: usize| dev.read_u32(regs_iova + i as u64 * 4);
    assert_eq!(reg_at(lrc::CTX_CONTEXT_CONTROL), 0x0009_0009);
    assert_eq!(u64::from(reg_at(lrc::CTX_RING_START)), lrc_ggtt);
    assert_eq!(reg_at(lrc::CTX_RING_CTL), 0x3001);
    assert_eq!(reg_at(lrc::CTX_ASID), vm.0);
    let pdp = u64::from(reg_at(lrc::CTX_PDP0_UDW)) << 32 | u64::from(reg_at(lrc::CTX_PDP0_LDW));
    assert_eq!(
        pdp & 0xfff,
        pte::PAGE_PRESENT | pte::PAGE_RW | pte::PPGTT_PTE_PAT1
    );
    assert_ne!(pdp & pte::ADDR_MASK_4K, 0);
}

#[test]
fn submit_writes_the_job_moves_the_tail_and_kicks_the_guc() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    let reg = actions_since(&gpu, h2g(&gpu).len() - 1).remove(0);
    let pphwsp = (u64::from(reg[10]) | u64::from(reg[11]) << 32) & !0xfff;
    let lrc_ggtt = (pphwsp - u64::from(lrc::PPHWSP_OFFSET)) as u32;

    let sent = h2g(&gpu).len();
    gpu.queue_submit(q, 0x10_0000).unwrap();
    gpu.queue_submit(q, 0x20_0000).unwrap();
    // First submit enables scheduling, the next only tells the GuC the tail moved.
    assert_eq!(
        actions_since(&gpu, sent),
        [vec![0x1001, q.0, 1], vec![0x1000, q.0]]
    );

    let ring = gpu.regs.ggtt_to_iova(u64::from(lrc_ggtt));
    let regs_iova = gpu.regs.ggtt_to_iova(pphwsp + 4096);
    let dev = gpu.regs.device.borrow();
    let job: Vec<u32> = (0..16).map(|i| dev.read_u32(ring + 8 + i * 4)).collect();
    assert_eq!(job, lrc::job(lrc_ggtt, 0x10_0000, 1));
    assert_eq!(
        dev.read_u32(regs_iova + lrc::CTX_RING_TAIL as u64 * 4),
        8 + 2 * 64
    );
}

#[test]
fn a_ring_the_engine_never_drains_answers_eagain() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    let mut accepted = 0;
    loop {
        match gpu.queue_submit(q, 0x10_0000) {
            Ok(()) => accepted += 1,
            Err(e) => {
                assert_eq!(e, XeError::TryAgain);
                break;
            }
        }
    }
    // 16 KiB ring, 8 bytes of prologue, 64-byte jobs, never completely full.
    assert_eq!(accepted, (lrc::RING_SIZE - 8 - 1) / 64);
}

#[test]
fn context_reset_from_the_guc_bans_the_queue() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    assert_eq!(gpu.queue_ban(q), 0);
    gpu.regs
        .inject_g2h(ACTION_CONTEXT_RESET_NOTIFICATION, &[q.0]);
    assert_eq!(gpu.queue_ban(q), XE_EXEC_QUEUE_BAN_FLAG_HANG);
    assert_eq!(gpu.queue_submit(q, 0x10_0000), Err(XeError::Canceled));
    // Another queue is untouched.
    let other = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    assert_eq!(gpu.queue_ban(other), 0);
}

#[test]
fn queue_destroy_disables_deregisters_and_only_then_frees_the_lrc() {
    let mut gpu = test_gpu();
    let vm = gpu.vm_create().unwrap();
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    gpu.queue_submit(q, 0x10_0000).unwrap();
    let used = gpu.pool.used_pages();
    let sent = h2g(&gpu).len();
    gpu.queue_destroy(q);
    assert_eq!(
        actions_since(&gpu, sent),
        [vec![0x1001, q.0, 0], vec![0x4503, q.0]]
    );
    assert_eq!(gpu.pool.used_pages(), used - 10);
    assert_eq!(gpu.queue_count(), 0);

    // A GuC that never confirms: the LRC and its id are quarantined, not reused.
    let q = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    gpu.regs.device.borrow_mut().silent = true;
    let used = gpu.pool.used_pages();
    gpu.queue_destroy(q);
    assert_eq!(gpu.faults.stuck_contexts, 1);
    assert_eq!(gpu.pool.used_pages(), used);
    gpu.regs.device.borrow_mut().silent = false;
    let next = gpu.queue_create(vm, XE_ENGINE_CLASS_RENDER).unwrap();
    assert_ne!(next, q);
}

#[test]
fn vm_destroy_returns_every_table() {
    let mut gpu = test_gpu();
    let before = gpu.pool.used_pages();
    let vm = gpu.vm_create().unwrap();
    gpu.vm_map(vm, 0x10_0000, WINDOW, 0x2000, PatIndex::WB, true)
        .unwrap();
    gpu.vm_map(vm, 1 << 40, WINDOW, 0x1000, PatIndex::WB, true)
        .unwrap();
    gpu.vm_unmap(vm, 0x10_0000, 0x2000);
    gpu.vm_unmap(vm, 1 << 40, 0x1000);
    gpu.vm_destroy(vm);
    assert_eq!(gpu.pool.used_pages(), before);
    assert_eq!(gpu.vm_count(), 0);
}
