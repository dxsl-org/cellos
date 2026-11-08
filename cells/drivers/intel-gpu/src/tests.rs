//! Host unit tests for Intel iGPU driver probe, forcewake, and identity parsing.

use crate::controller::IntelGpuController;
use crate::fake::FakeRegs;
use crate::forcewake::{forcewake_gt_acquire, forcewake_gt_release};
use crate::identity::{parse_gmd_id, GfxArch};
use crate::regs::{FORCEWAKE_ACK_BIT, FORCEWAKE_ACK_GT, FORCEWAKE_GT, GMD_ID};
use types::ViError;

#[test]
fn forcewake_handshake_success() {
    let mut regs = FakeRegs::new();
    assert_eq!(forcewake_gt_acquire(&mut regs), Ok(()));

    // Check register state
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0001);
    assert_eq!(
        regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT,
        FORCEWAKE_ACK_BIT
    );

    // Release forcewake
    assert_eq!(forcewake_gt_release(&mut regs), Ok(()));
    assert_eq!(regs.get(FORCEWAKE_GT), 0x0001_0000);
    assert_eq!(regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn forcewake_timeout_when_unresponsive() {
    let mut regs = FakeRegs::new();
    regs.auto_ack_forcewake = false; // Never sets ACK bit

    assert_eq!(forcewake_gt_acquire(&mut regs), Err(ViError::IO));
}

#[test]
fn gmd_id_decoding_panther_lake() {
    let raw = (30 << 22) | (0 << 14) | (2 << 6);
    let (arch, major, minor, rev) = parse_gmd_id(raw).unwrap();
    assert_eq!(arch, GfxArch::Xe3);
    assert_eq!((major, minor, rev), (30, 0, 2));
}

#[test]
fn gmd_id_decoding_lunar_lake() {
    let raw = (20 << 22) | (4 << 14) | (0 << 6);
    let (arch, major, minor, rev) = parse_gmd_id(raw).unwrap();
    assert_eq!(arch, GfxArch::Xe2);
    assert_eq!((major, minor, rev), (20, 4, 0));
}

#[test]
fn gmd_id_unsupported_version_refused() {
    let raw = (12 << 22) | (0 << 14) | (0 << 6); // Xe-LP
    assert_eq!(parse_gmd_id(raw), Err(ViError::NotSupported));
}

#[test]
fn controller_probe_panther_lake_success() {
    let regs = FakeRegs::for_panther_lake();
    let mut controller = IntelGpuController::probe(regs).unwrap();

    assert_eq!(controller.identity.arch, GfxArch::Xe3);
    assert_eq!(controller.identity.name(), "Intel Xe3 (PTL/WCL)");
    assert_eq!(controller.identity.ip_ver_major, 30);
    assert_eq!(controller.identity.dss_geometry_mask, 0x0FFF);
    assert_eq!(controller.identity.eu_enable_mask, 0xFF);
    assert_eq!(controller.identity.ref_clock_hz, 24_000_000);
    assert!(controller.forcewake_active);

    // Shutdown releases forcewake
    controller.shutdown().unwrap();
    assert!(!controller.forcewake_active);
    assert_eq!(controller.regs.get(FORCEWAKE_ACK_GT) & FORCEWAKE_ACK_BIT, 0);
}

#[test]
fn controller_probe_fails_on_unsupported_hardware() {
    let regs = FakeRegs::for_panther_lake();
    // Overwrite with unsupported GMD_ID
    regs.set(GMD_ID, 9 << 22);

    assert_eq!(IntelGpuController::probe(regs).is_err(), true);
}
#[test]
fn ggtt_pte_encoding() {
    use crate::ggtt::encode_ggtt_pte;
    use intel_gpu_core::pte::PatIndex;

    // 4K aligned address with writeable and WB PAT
    let pte = encode_ggtt_pte(0x1000, true, PatIndex::WB).unwrap();
    assert_eq!(pte & 0xFFF, 0x013); // PRESENT (1) | RW (2) | PAT1 (16)
    assert_eq!(pte & !0xFFF, 0x1000);

    // Misaligned address rejected
    assert_eq!(
        encode_ggtt_pte(0x1001, true, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
}

#[test]
fn ggtt_allocator_bump_and_alignment() {
    use crate::ggtt::GgttAllocator;

    let mut alloc = GgttAllocator::new(0x10000, 0x10000); // 64 KiB space
    let a1 = alloc.allocate(4000).unwrap(); // rounds up to 4K
    assert_eq!(a1, 0x10000);

    let a2 = alloc.allocate(8192).unwrap();
    assert_eq!(a2, 0x11000);

    // Oversize allocation rejected
    assert_eq!(alloc.allocate(0x100000), Err(ViError::OutOfMemory));
}

#[test]
fn guc_bootloader_and_firmware_upload() {
    use crate::guc::bring_up_guc;

    let mut regs = FakeRegs::for_panther_lake();
    let fw_phys = 0x200000;
    let fw_size = 64 * 1024; // 64 KiB

    assert_eq!(bring_up_guc(&mut regs, fw_phys, fw_size), Ok(()));

    // Verify registers updated after DMA transfer
    assert_eq!(
        regs.get(crate::guc::GUC_STATUS),
        crate::guc::GUC_STATUS_UKERNEL_READY
    );
}

#[test]
fn guc_upload_invalid_size_rejected() {
    use crate::guc::upload_guc_firmware;

    let mut regs = FakeRegs::for_panther_lake();
    assert_eq!(
        upload_guc_firmware(&mut regs, 0x1000, 0, 0),
        Err(ViError::InvalidArgument)
    );
    assert_eq!(
        upload_guc_firmware(&mut regs, 0x1000, 0, 3),
        Err(ViError::InvalidArgument)
    );
}
#[test]
fn client_vm_ppgtt_bind_and_translate() {
    use crate::ppgtt_store::ClientVm;
    use intel_gpu_core::pte::PatIndex;

    let mut vm = ClientVm::new_ptl(1).unwrap();
    assert_ne!(vm.root_table(), 0);

    // Bind 16 KiB (4 pages) at VA 0x100000 to PA 0x200000
    let va_base = 0x100000;
    let pa_base = 0x200000;
    let size = 16384;
    assert_eq!(
        vm.bind_range(va_base, pa_base, size, true, PatIndex::WB),
        Ok(())
    );

    // Verify all 4 pages translate correctly
    for i in 0..4 {
        let va = va_base + i * 4096;
        let expected_pa = pa_base + i * 4096;
        assert_eq!(vm.translate(va), Ok(Some(expected_pa)));
        // Byte offset within page preserved
        assert_eq!(vm.translate(va + 0x123), Ok(Some(expected_pa + 0x123)));
    }

    // Unmapped address translates to None
    assert_eq!(vm.translate(0x500000), Ok(None));

    // Double mapping returns AlreadyExists
    assert_eq!(
        vm.bind_range(va_base, pa_base, 4096, true, PatIndex::WB),
        Err(ViError::AlreadyExists)
    );

    // Unbind range
    assert_eq!(vm.unbind_range(va_base, size), Ok(()));
    assert_eq!(vm.translate(va_base), Ok(None));
}

#[test]
fn client_vm_misaligned_bind_rejected() {
    use crate::ppgtt_store::ClientVm;
    use intel_gpu_core::pte::PatIndex;

    let mut vm = ClientVm::new_ptl(2).unwrap();
    // Misaligned VA
    assert_eq!(
        vm.bind_range(0x1001, 0x2000, 4096, true, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
    // Misaligned PA
    assert_eq!(
        vm.bind_range(0x1000, 0x2001, 4096, true, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
    // Misaligned size
    assert_eq!(
        vm.bind_range(0x1000, 0x2000, 4095, true, PatIndex::WB),
        Err(ViError::InvalidArgument)
    );
}
#[test]
fn first_light_marker_write_and_readback() {
    use crate::batch::{build_store_marker_batch, simulate_gpu_execution};
    use crate::ppgtt_store::ClientVm;
    use intel_gpu_core::pte::PatIndex;

    // 1. Create client VM with 48-bit PTL PPGTT
    let mut vm = ClientVm::new_ptl(1).unwrap();

    // 2. Simulated physical memory buffer (64 KiB starting at PA 0x200000)
    let phys_base: u64 = 0x200000;
    let mut phys_mem = alloc::vec![0u8; 64 * 1024];

    // 3. Map batch buffer at VA 0x100000 (PA 0x200000)
    let batch_va: u64 = 0x100000;
    let batch_pa: u64 = 0x200000;
    vm.bind_range(batch_va, batch_pa, 4096, true, PatIndex::WB)
        .unwrap();

    // 4. Map target output buffer at VA 0x110000 (PA 0x204000)
    let target_va: u64 = 0x110000;
    let target_pa: u64 = 0x204000;
    vm.bind_range(target_va, target_pa, 4096, true, PatIndex::WB)
        .unwrap();

    // 5. Construct batch buffer: MI_STORE_DATA_IMM writing 0xDEAD_BEEF to target_va
    let marker: u32 = 0xDEAD_BEEF;
    let batch_words = build_store_marker_batch(target_va, marker);

    // Copy batch words into simulated physical memory at batch_pa offset
    let batch_mem_offset = (batch_pa - phys_base) as usize;
    for (i, &word) in batch_words.iter().enumerate() {
        let off = batch_mem_offset + i * 4;
        phys_mem[off..off + 4].copy_from_slice(&word.to_le_bytes());
    }

    // Target memory is initially zero
    let target_mem_offset = (target_pa - phys_base) as usize;
    assert_eq!(
        &phys_mem[target_mem_offset..target_mem_offset + 4],
        &[0, 0, 0, 0]
    );

    // 6. Simulate GPU execution of the batch buffer
    let insn_count = simulate_gpu_execution(&mut vm, batch_va, &mut phys_mem, phys_base).unwrap();
    assert_eq!(insn_count, 2); // 1 MI_STORE_DATA_IMM + 1 MI_BATCH_BUFFER_END

    // 7. Verify the marker was written by the GPU to target memory and read back
    let read_back = u32::from_le_bytes([
        phys_mem[target_mem_offset],
        phys_mem[target_mem_offset + 1],
        phys_mem[target_mem_offset + 2],
        phys_mem[target_mem_offset + 3],
    ]);
    assert_eq!(read_back, marker);
}

#[test]
fn stress_10000_submits_with_fences_without_loss() {
    use intel_gpu_core::backend::*;
    use intel_gpu_core::syncobj::*;

    let mut session = XeSession::new();
    let mut syncobjs = Syncobjs::new();

    let vm_id = session.vm_create(0).unwrap();
    let bo = session
        .gem_create(4096, 501, vm_id, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    let op = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo,
        obj_offset: 0,
        addr: 0x10000,
        range: 4096,
        pat_index: 0,
    };
    session.vm_bind(vm_id, &[op]).unwrap();

    let q_id = session
        .exec_queue_create(vm_id, XE_ENGINE_CLASS_RENDER, XE_EXEC_QUEUE_PRIORITY_NORMAL)
        .unwrap();

    // 10,000 submit and complete cycle
    for i in 0..10_000 {
        let fence = syncobjs.new_fence();
        let sync_h = syncobjs.create(0).unwrap();
        syncobjs.attach(sync_h, 0, fence).unwrap();

        let job_id = session.exec(q_id, 0x10000, 0, Some(fence)).unwrap();
        assert_eq!(
            syncobjs.poll_wait(&[sync_h], &[0], WAIT_ALL, false),
            Ok(WaitState::Pending)
        );

        // Complete job
        session.complete_job(q_id, job_id, &mut syncobjs).unwrap();
        assert_eq!(
            syncobjs.poll_wait(&[sync_h], &[0], WAIT_ALL, false),
            Ok(WaitState::Ready { first: 0 })
        );

        // Clean up syncobj handle
        syncobjs.destroy(sync_h).unwrap();
    }
}

#[test]
fn injected_hang_reset_and_client_error() {
    use intel_gpu_core::backend::*;
    use intel_gpu_core::syncobj::*;

    let mut session = XeSession::new();
    let mut syncobjs = Syncobjs::new();

    let vm_id = session.vm_create(0).unwrap();
    let bo = session
        .gem_create(4096, 601, vm_id, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    let op = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo,
        obj_offset: 0,
        addr: 0x10000,
        range: 4096,
        pat_index: 0,
    };
    session.vm_bind(vm_id, &[op]).unwrap();

    let q_id = session
        .exec_queue_create(vm_id, XE_ENGINE_CLASS_RENDER, XE_EXEC_QUEUE_PRIORITY_NORMAL)
        .unwrap();

    let fence1 = syncobjs.new_fence();
    let fence2 = syncobjs.new_fence();
    let job1 = session.exec(q_id, 0x10000, 0, Some(fence1)).unwrap();
    let _job2 = session.exec(q_id, 0x10000, 0, Some(fence2)).unwrap();

    // Inject GPU hang
    session.inject_hang(q_id, &mut syncobjs).unwrap();
    assert_eq!(
        session.exec_queue_get_property_ban(q_id),
        Ok(XE_EXEC_QUEUE_BAN_FLAG_HANG)
    );

    // Banned queue rejects new submits with -ECANCELED
    assert_eq!(session.exec(q_id, 0x10000, 0, None), Err(XeError::Canceled));

    // Trying to complete jobs on banned queue fails
    assert_eq!(
        session.complete_job(q_id, job1, &mut syncobjs),
        Err(XeError::NoEntry)
    );
}
