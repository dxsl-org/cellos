use crate::backend::*;
use crate::hw::fake::{Event, RecordingHw};
use crate::hw::{ExportRecord, HwQueue, HwVm};
use crate::syncobj::*;

const CELL: u64 = 9;
const GEN: u64 = 1;
const IOVA: u64 = 1 << 38;

fn export(id: u64, len: u64, write: bool) -> ExportRecord {
    ExportRecord {
        export_id: id,
        owner_cell_id: CELL,
        owner_generation: GEN,
        iova: IOVA + id * (1 << 24),
        len,
        device_write: write,
    }
}

/// A session with VM 1 and an object of `pages` pages over export `id`.
fn with_object(hw: &mut RecordingHw, id: u64, pages: u64, write: bool) -> (XeSession, u32, u32) {
    let mut s = XeSession::new();
    let vm = s.vm_create(0, hw).unwrap();
    let rec = export(id, pages * 4096, write);
    hw.exports.insert(id, rec);
    let bo = s
        .gem_create(pages as usize * 4096, &rec, 0, XE_GEM_CPU_CACHING_WB)
        .unwrap();
    (s, vm, bo)
}

fn map_op(gem_handle: u32, obj_offset: u64, addr: u64, range: u64) -> BindOp {
    BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle,
        obj_offset,
        addr,
        range,
        pat_index: 2,
    }
}

fn unmap_op(addr: u64, range: u64) -> BindOp {
    BindOp {
        op: XE_VM_BIND_OP_UNMAP,
        flags: 0,
        gem_handle: 0,
        obj_offset: 0,
        addr,
        range,
        pat_index: 0,
    }
}

#[test]
fn gem_create_validations() {
    let mut hw = RecordingHw::default();
    let mut s = XeSession::new();
    let rec = export(1, 8192, true);
    assert_eq!(
        s.gem_create(0, &rec, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );
    assert_eq!(
        s.gem_create(4095, &rec, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );
    assert_eq!(s.gem_create(4096, &rec, 0, 99), Err(XeError::Invalid));
    assert_eq!(
        s.gem_create(4096, &rec, 7, XE_GEM_CPU_CACHING_WB),
        Err(XeError::NoEntry)
    );
    // The window must back the whole object.
    assert_eq!(
        s.gem_create(3 * 4096, &rec, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );

    let h1 = s.gem_create(8192, &rec, 0, XE_GEM_CPU_CACHING_WB).unwrap();
    assert_eq!(h1, 1);
    assert_eq!(s.gem_mmap_offset(h1), Ok(1u64 << 32));
    let bo = &s.gem_objects[&h1];
    assert_eq!((bo.iova, bo.writable, bo.export_id), (rec.iova, true, 1));
    // One lease backs one object.
    assert_eq!(
        s.gem_create(4096, &rec, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );

    hw.exports.insert(1, rec);
    assert_eq!(s.gem_close(h1, &mut hw), Ok(()));
    assert_eq!(hw.events, [Event::Revoke(1)]);
    assert_eq!(s.gem_close(h1, &mut hw), Err(XeError::NoEntry));
}

#[test]
fn map_writes_leaves_at_window_plus_object_offset() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 3, 4, true);
    s.vm_bind(vm, &[map_op(bo, 0x2000, 0x10_0000, 0x2000)], &mut hw)
        .unwrap();
    assert_eq!(
        hw.events.last(),
        Some(&Event::Map {
            vm: HwVm(1),
            va: 0x10_0000,
            dma: export(3, 0, true).iova + 0x2000,
            len: 0x2000,
            writable: true,
        })
    );
    // READONLY is honoured.
    let mut ro = map_op(bo, 0, 0x20_0000, 0x1000);
    ro.flags = XE_VM_BIND_FLAG_READONLY;
    s.vm_bind(vm, &[ro], &mut hw).unwrap();
    assert!(matches!(
        hw.events.last(),
        Some(Event::Map {
            writable: false,
            ..
        })
    ));
}

#[test]
fn device_read_only_export_forces_read_only_leaves() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 4, 2, false);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x2000)], &mut hw)
        .unwrap();
    assert!(matches!(
        hw.events.last(),
        Some(Event::Map {
            writable: false,
            ..
        })
    ));
    assert!(!s.vms[&vm].mappings[0].writable);
}

#[test]
fn vm_bind_bounds_and_oversize_refusal() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 4, true);
    let before = hw.events.len();
    // Past the object, misaligned, past 48 bits, zero length, unknown VM, unknown object.
    for op in [
        map_op(bo, 0x2000, 0x10_0000, 0x3000),
        map_op(bo, 0, 0x10_0800, 0x1000),
        map_op(bo, 0, MAX_VA + 1 - 0x1000, 0x2000),
        map_op(bo, 0, 0x10_0000, 0),
    ] {
        assert_eq!(s.vm_bind(vm, &[op], &mut hw), Err(XeError::Invalid));
    }
    assert_eq!(
        s.vm_bind(vm, &[map_op(77, 0, 0x10_0000, 0x1000)], &mut hw),
        Err(XeError::NoEntry)
    );
    assert_eq!(
        s.vm_bind(9, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw),
        Err(XeError::NoEntry)
    );
    // An unmap naming an object, an out-of-table PAT index.
    let mut bad_unmap = unmap_op(0x10_0000, 0x1000);
    bad_unmap.gem_handle = bo;
    assert_eq!(s.vm_bind(vm, &[bad_unmap], &mut hw), Err(XeError::Invalid));
    let mut bad_pat = map_op(bo, 0, 0x10_0000, 0x1000);
    bad_pat.pat_index = 32;
    assert_eq!(s.vm_bind(vm, &[bad_pat], &mut hw), Err(XeError::Invalid));
    assert_eq!(hw.events.len(), before, "refusals touch no hardware");
}

#[test]
fn userptr_and_prefetch_are_refused_as_xe_refuses_them() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, _) = with_object(&mut hw, 1, 1, true);
    for op in [XE_VM_BIND_OP_MAP_USERPTR, XE_VM_BIND_OP_PREFETCH, 9] {
        let mut b = unmap_op(0x10_0000, 0x1000);
        b.op = op;
        assert_eq!(s.vm_bind(vm, &[b], &mut hw), Err(XeError::Invalid));
    }
}

#[test]
fn null_binding_writes_null_leaves() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, _) = with_object(&mut hw, 1, 1, true);
    let mut null = map_op(0, 0, 0x40_0000, 0x4000);
    null.flags = XE_VM_BIND_FLAG_NULL;
    s.vm_bind(vm, &[null], &mut hw).unwrap();
    assert_eq!(
        hw.events.last(),
        Some(&Event::MapNull {
            vm: HwVm(1),
            va: 0x40_0000,
            len: 0x4000
        })
    );
    null.gem_handle = 1;
    assert_eq!(s.vm_bind(vm, &[null], &mut hw), Err(XeError::Invalid));
}

#[test]
fn vm_private_bo_isolation() {
    let mut hw = RecordingHw::default();
    let mut s = XeSession::new();
    let vm1 = s.vm_create(0, &mut hw).unwrap();
    let vm2 = s.vm_create(0, &mut hw).unwrap();
    let rec = export(5, 4096, true);
    let bo = s
        .gem_create(4096, &rec, vm1, XE_GEM_CPU_CACHING_WB)
        .unwrap();
    assert_eq!(
        s.vm_bind(vm2, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw),
        Err(XeError::Invalid)
    );
    assert_eq!(
        s.vm_bind(vm1, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw),
        Ok(())
    );
}

#[test]
fn remap_clears_the_old_leaves_first_and_partial_unmap_splits() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 4, true);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x4000)], &mut hw)
        .unwrap();
    s.vm_bind(vm, &[unmap_op(0x10_1000, 0x2000)], &mut hw)
        .unwrap();
    assert_eq!(
        hw.events.last(),
        Some(&Event::Unmap {
            vm: HwVm(1),
            va: 0x10_1000,
            len: 0x2000
        })
    );
    let mut spans: Vec<_> = s.vms[&vm]
        .mappings
        .iter()
        .map(|m| (m.addr, m.range, m.obj_offset))
        .collect();
    spans.sort_unstable();
    assert_eq!(spans, [(0x10_0000, 0x1000, 0), (0x10_3000, 0x1000, 0x3000)]);
    // Mapping over a live range unmaps it before writing the new leaves.
    let before = hw.events.len();
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    assert!(matches!(
        hw.events[before],
        Event::Unmap { va: 0x10_0000, .. }
    ));
    assert!(matches!(
        hw.events[before + 1],
        Event::Map { va: 0x10_0000, .. }
    ));
    // Unmapping a hole touches nothing.
    let before = hw.events.len();
    s.vm_bind(vm, &[unmap_op(0x80_0000, 0x1000)], &mut hw)
        .unwrap();
    assert_eq!(hw.events.len(), before);
}

#[test]
fn refused_bind_array_leaves_vm_unchanged() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    let before = hw.events.len();
    let ops = [
        map_op(bo, 0, 0x10_0000, 0x1000),
        map_op(bo, 0, 0x10_0001, 0x1000),
    ];
    assert_eq!(s.vm_bind(vm, &ops, &mut hw), Err(XeError::Invalid));
    assert!(s.vms[&vm].mappings.is_empty());
    assert_eq!(hw.events.len(), before);
}

#[test]
fn table_exhaustion_is_reported_not_recorded() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 2, true);
    hw.map_budget = Some(1);
    let ops = [
        map_op(bo, 0, 0x10_0000, 0x1000),
        map_op(bo, 0x1000, 0x20_0000, 0x1000),
    ];
    assert_eq!(s.vm_bind(vm, &ops, &mut hw), Err(XeError::OutOfMemory));
    assert_eq!(s.vms[&vm].mappings.len(), 1);
}

#[test]
fn last_page_below_48_bits_is_mappable() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    assert_eq!(
        s.vm_bind(vm, &[map_op(bo, 0, MAX_VA + 1 - 0x1000, 0x1000)], &mut hw),
        Ok(())
    );
}

#[test]
fn gem_close_unmaps_every_vm_then_revokes() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 6, 2, true);
    let vm2 = s.vm_create(0, &mut hw).unwrap();
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x2000)], &mut hw)
        .unwrap();
    s.vm_bind(vm2, &[map_op(bo, 0, 0x30_0000, 0x1000)], &mut hw)
        .unwrap();
    s.gem_close(bo, &mut hw).unwrap();
    let revoke = hw.position(|e| *e == Event::Revoke(6)).unwrap();
    let unmaps: Vec<usize> = hw
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Event::Unmap { .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(unmaps.len(), 2);
    assert!(unmaps.iter().all(|&i| i < revoke));
    assert!(s.vms.values().all(|vm| vm.mappings.is_empty()));
}

#[test]
fn unmap_all_requires_an_object() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    let mut op = unmap_op(0, 0);
    op.op = XE_VM_BIND_OP_UNMAP_ALL;
    assert_eq!(s.vm_bind(vm, &[op], &mut hw), Err(XeError::Invalid));
    op.gem_handle = bo;
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    assert_eq!(s.vm_bind(vm, &[op], &mut hw), Ok(()));
    assert!(s.vms[&vm].mappings.is_empty());
}

#[test]
fn exec_submits_mapped_batches_and_refuses_the_rest() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    assert_eq!(s.exec(q, 0x10_0000, None, &mut hw), Err(XeError::Invalid));
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    s.exec(q, 0x10_0000, None, &mut hw).unwrap();
    assert_eq!(
        hw.events.last(),
        Some(&Event::Submit(HwQueue(1), 0x10_0000))
    );
    assert!(s.exec_queues[&q].in_flight.is_empty());
    // A full ring is EAGAIN, the error anv retries on.
    hw.full.insert(HwQueue(1));
    assert_eq!(s.exec(q, 0x10_0000, None, &mut hw), Err(XeError::TryAgain));
    assert_eq!(s.exec(77, 0x10_0000, None, &mut hw), Err(XeError::NoEntry));
}

#[test]
fn exec_on_a_banned_queue_is_canceled() {
    let mut hw = RecordingHw::default();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    assert_eq!(s.exec_queue_ban(q, &mut hw), Ok(0));
    // The GuC reset the context (CONTEXT_RESET_NOTIFICATION).
    hw.bans.insert(HwQueue(1), XE_EXEC_QUEUE_BAN_FLAG_HANG);
    let submits = hw.count(|e| matches!(e, Event::Submit(..)));
    assert_eq!(s.exec(q, 0x10_0000, None, &mut hw), Err(XeError::Canceled));
    assert_eq!(hw.count(|e| matches!(e, Event::Submit(..))), submits);
    assert_eq!(
        s.exec_queue_ban(q, &mut hw),
        Ok(XE_EXEC_QUEUE_BAN_FLAG_HANG)
    );
}

#[test]
fn exec_and_fence_cancellation_on_hang() {
    let mut hw = RecordingHw::default();
    let mut sync = Syncobjs::new();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    let (f1, f2) = (sync.new_fence(), sync.new_fence());
    let h1 = sync.create(0).unwrap();
    sync.attach(h1, 0, f1).unwrap();
    let j1 = s.exec(q, 0x10_0000, Some(f1), &mut hw).unwrap();
    s.exec(q, 0x10_0000, Some(f2), &mut hw).unwrap();
    s.inject_hang(q, &mut sync).unwrap();
    // Waiters woke; new work is refused; the hung job is gone.
    assert_eq!(
        sync.poll_wait(&[h1], &[0], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );
    assert_eq!(s.exec(q, 0x10_0000, None, &mut hw), Err(XeError::Canceled));
    assert_eq!(s.complete_job(q, j1, &mut sync), Err(XeError::NoEntry));
}

#[test]
fn ten_thousand_fenced_submits_complete_without_loss() {
    let mut hw = RecordingHw::default();
    let mut sync = Syncobjs::new();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    for _ in 0..10_000 {
        let fence = sync.new_fence();
        let h = sync.create(0).unwrap();
        sync.attach(h, 0, fence).unwrap();
        let job = s.exec(q, 0x10_0000, Some(fence), &mut hw).unwrap();
        assert_eq!(
            sync.poll_wait(&[h], &[0], WAIT_ALL, false),
            Ok(WaitState::Pending)
        );
        s.complete_job(q, job, &mut sync).unwrap();
        assert_eq!(
            sync.poll_wait(&[h], &[0], WAIT_ALL, false),
            Ok(WaitState::Ready { first: 0 })
        );
        sync.destroy(h).unwrap();
    }
    assert_eq!(hw.count(|e| matches!(e, Event::Submit(..))), 10_000);
    assert!(s.exec_queues[&q].in_flight.is_empty());
}

#[test]
fn vm_destroy_waits_for_its_queues_before_freeing_tables() {
    let mut hw = RecordingHw::default();
    let mut sync = Syncobjs::new();
    let (mut s, vm, bo) = with_object(&mut hw, 1, 1, true);
    s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw)
        .unwrap();
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    s.vm_destroy(vm, &mut hw).unwrap();
    // Leaves gone at once, tables kept for the live context, VM invisible to the uAPI.
    assert!(matches!(hw.events.last(), Some(Event::Unmap { .. })));
    assert_eq!(hw.count(|e| matches!(e, Event::VmDestroy(_))), 0);
    assert_eq!(s.vm_destroy(vm, &mut hw), Err(XeError::NoEntry));
    assert_eq!(
        s.vm_bind(vm, &[map_op(bo, 0, 0x10_0000, 0x1000)], &mut hw),
        Err(XeError::NoEntry)
    );
    s.exec_queue_destroy(q, &mut sync, &mut hw).unwrap();
    assert_eq!(
        &hw.events[hw.events.len() - 2..],
        &[Event::QueueDestroy(HwQueue(1)), Event::VmDestroy(HwVm(1))]
    );
}

#[test]
fn client_death_stops_queues_zaps_ppgtt_and_only_then_revokes() {
    let mut hw = RecordingHw::default();
    let mut sync = Syncobjs::new();
    let mut s = XeSession::new();
    let vm = s.vm_create(0, &mut hw).unwrap();
    let mut handles = Vec::new();
    for id in [11, 12] {
        let rec = export(id, 0x2000, true);
        hw.exports.insert(id, rec);
        handles.push(
            s.gem_create(0x2000, &rec, 0, XE_GEM_CPU_CACHING_WB)
                .unwrap(),
        );
    }
    s.vm_bind(
        vm,
        &[
            map_op(handles[0], 0, 0x10_0000, 0x2000),
            map_op(handles[1], 0, 0x20_0000, 0x2000),
        ],
        &mut hw,
    )
    .unwrap();
    let q = s
        .exec_queue_create(
            vm,
            XE_ENGINE_CLASS_RENDER,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
            &mut hw,
        )
        .unwrap();
    let fence = sync.new_fence();
    let h = sync.create(0).unwrap();
    sync.attach(h, 0, fence).unwrap();
    s.exec(q, 0x10_0000, Some(fence), &mut hw).unwrap();

    let start = hw.events.len();
    let mut revoked = s.on_client_death(&mut sync, &mut hw);
    revoked.sort_unstable();
    assert_eq!(revoked, [11, 12]);
    let tail = &hw.events[start..];
    let at = |pred: &dyn Fn(&Event) -> bool| tail.iter().position(pred).unwrap();
    let queue_gone = at(&|e| matches!(e, Event::QueueDestroy(_)));
    let last_unmap = tail
        .iter()
        .rposition(|e| matches!(e, Event::Unmap { .. }))
        .unwrap();
    let vm_gone = at(&|e| matches!(e, Event::VmDestroy(_)));
    let first_revoke = at(&|e| matches!(e, Event::Revoke(_)));
    assert!(queue_gone < last_unmap && last_unmap < vm_gone && vm_gone < first_revoke);
    assert_eq!(
        tail.iter()
            .filter(|e| matches!(e, Event::Unmap { .. }))
            .count(),
        2
    );
    // The waiter woke, nothing is left.
    assert_eq!(
        sync.poll_wait(&[h], &[0], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );
    assert!(s.gem_objects.is_empty() && s.vms.is_empty() && s.exec_queues.is_empty());
}
