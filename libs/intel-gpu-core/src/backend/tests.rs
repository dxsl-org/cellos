use crate::backend::*;
use crate::syncobj::*;

#[test]
fn gem_create_validations() {
    let mut session = XeSession::new();

    // Size must be non-zero and 4K aligned
    assert_eq!(
        session.gem_create(0, 100, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );
    assert_eq!(
        session.gem_create(4095, 100, 0, XE_GEM_CPU_CACHING_WB),
        Err(XeError::Invalid)
    );

    // Invalid caching
    assert_eq!(session.gem_create(4096, 100, 0, 99), Err(XeError::Invalid));

    // VM must exist if specified
    assert_eq!(
        session.gem_create(4096, 100, 42, XE_GEM_CPU_CACHING_WB),
        Err(XeError::NoEntry)
    );

    // Valid create
    let h1 = session
        .gem_create(8192, 100, 0, XE_GEM_CPU_CACHING_WB)
        .unwrap();
    assert_eq!(h1, 1);
    let offset = session.gem_mmap_offset(h1).unwrap();
    assert_eq!(offset, (1u64) << 32);

    // Close
    assert_eq!(session.gem_close(h1), Ok(()));
    assert_eq!(session.gem_close(h1), Err(XeError::NoEntry));
}

#[test]
fn vm_bind_bounds_and_oversize_refusal() {
    let mut session = XeSession::new();
    let vm_id = session.vm_create(0).unwrap();
    let bo = session
        .gem_create(16384, 101, 0, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    // Oversize 48-bit address space refusal
    let op_oversize_va = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo,
        obj_offset: 0,
        addr: MAX_VA - 4095,
        range: 8192,
        pat_index: 0,
    };
    assert_eq!(
        session.vm_bind(vm_id, &[op_oversize_va]),
        Err(XeError::Invalid)
    );

    // Oversize BO offset refusal
    let op_oversize_bo = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo,
        obj_offset: 12288,
        addr: 0x10000,
        range: 8192,
        pat_index: 0,
    };
    assert_eq!(
        session.vm_bind(vm_id, &[op_oversize_bo]),
        Err(XeError::Invalid)
    );

    // Valid map
    let op_valid = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo,
        obj_offset: 0,
        addr: 0x20000,
        range: 8192,
        pat_index: 0,
    };
    assert_eq!(session.vm_bind(vm_id, &[op_valid]), Ok(()));

    let vm = session.vms.get(&vm_id).unwrap();
    assert!(vm.is_mapped(0x20000, 8192));
    assert!(!vm.is_mapped(0x22000, 4096));

    // Unmap
    let op_unmap = BindOp {
        op: XE_VM_BIND_OP_UNMAP,
        flags: 0,
        gem_handle: 0,
        obj_offset: 0,
        addr: 0x20000,
        range: 8192,
        pat_index: 0,
    };
    assert_eq!(session.vm_bind(vm_id, &[op_unmap]), Ok(()));
    let vm = session.vms.get(&vm_id).unwrap();
    assert!(!vm.is_mapped(0x20000, 8192));
}

#[test]
fn vm_private_bo_isolation() {
    let mut session = XeSession::new();
    let vm1 = session.vm_create(0).unwrap();
    let vm2 = session.vm_create(0).unwrap();

    // Create BO bound specifically to vm1
    let bo1 = session
        .gem_create(4096, 201, vm1, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    // Binding bo1 to vm1 succeeds
    let op1 = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo1,
        obj_offset: 0,
        addr: 0x10000,
        range: 4096,
        pat_index: 0,
    };
    assert_eq!(session.vm_bind(vm1, &[op1]), Ok(()));

    // Binding bo1 to vm2 is refused with -EINVAL
    let op2 = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo1,
        obj_offset: 0,
        addr: 0x10000,
        range: 4096,
        pat_index: 0,
    };
    assert_eq!(session.vm_bind(vm2, &[op2]), Err(XeError::Invalid));
}

#[test]
fn exec_and_fence_cancellation_on_hang() {
    let mut session = XeSession::new();
    let mut syncobjs = Syncobjs::new();

    let vm_id = session.vm_create(0).unwrap();
    let bo = session
        .gem_create(4096, 301, vm_id, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    // Map batch buffer
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

    // Unmapped batch buffer address is refused
    assert_eq!(session.exec(q_id, 0x50000, 0, None), Err(XeError::Invalid));

    // Oversize sync count (> 1024) is refused
    assert_eq!(
        session.exec(q_id, 0x10000, 1025, None),
        Err(XeError::Invalid)
    );

    // Valid exec with out-fence
    let fence = syncobjs.new_fence();
    let job_id = session.exec(q_id, 0x10000, 1, Some(fence)).unwrap();
    assert_eq!(job_id, 1);

    // Fence is not yet signaled
    let sync_h = syncobjs.create(0).unwrap();
    syncobjs.attach(sync_h, 0, fence).unwrap();
    assert_eq!(
        syncobjs.poll_wait(&[sync_h], &[0], WAIT_ALL, false),
        Ok(WaitState::Pending)
    );

    // Inject GPU hang: queue becomes banned and fence is signaled (error completion)
    session.inject_hang(q_id, &mut syncobjs).unwrap();
    assert_eq!(
        session.exec_queue_get_property_ban(q_id),
        Ok(XE_EXEC_QUEUE_BAN_FLAG_HANG)
    );

    // Submitting on banned queue returns -ECANCELED
    assert_eq!(session.exec(q_id, 0x10000, 0, None), Err(XeError::Canceled));

    // Waiter discovers fence is completed
    assert_eq!(
        syncobjs.poll_wait(&[sync_h], &[0], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );
}

#[test]
fn client_death_revokes_gpu_mappings_before_frame_release() {
    let mut session = XeSession::new();
    let mut syncobjs = Syncobjs::new();

    let vm_id = session.vm_create(0).unwrap();
    let bo1 = session
        .gem_create(8192, 401, vm_id, XE_GEM_CPU_CACHING_WB)
        .unwrap();
    let bo2 = session
        .gem_create(4096, 402, vm_id, XE_GEM_CPU_CACHING_WB)
        .unwrap();

    let op1 = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo1,
        obj_offset: 0,
        addr: 0x10000,
        range: 8192,
        pat_index: 0,
    };
    let op2 = BindOp {
        op: XE_VM_BIND_OP_MAP,
        flags: 0,
        gem_handle: bo2,
        obj_offset: 0,
        addr: 0x30000,
        range: 4096,
        pat_index: 0,
    };
    session.vm_bind(vm_id, &[op1, op2]).unwrap();

    let q_id = session
        .exec_queue_create(
            vm_id,
            XE_ENGINE_CLASS_COMPUTE,
            XE_EXEC_QUEUE_PRIORITY_NORMAL,
        )
        .unwrap();

    let fence = syncobjs.new_fence();
    session.exec(q_id, 0x10000, 0, Some(fence)).unwrap();

    // Client dies: triggers on_client_death
    let revoked = session.on_client_death(&mut syncobjs);

    // Verify revoked export leases returned to caller for VT-d unmap
    assert_eq!(revoked.len(), 2);
    assert!(revoked.contains(&401));
    assert!(revoked.contains(&402));

    // Verify all GPU mappings in the VM are revoked
    let vm = session.vms.get(&vm_id).unwrap();
    assert_eq!(vm.mappings.len(), 0);
    assert!(!vm.is_mapped(0x10000, 8192));
    assert!(!vm.is_mapped(0x30000, 4096));

    // Verify queues are banned and in-flight fences signaled
    let q = session.exec_queues.get(&q_id).unwrap();
    assert!(q.banned);
    assert_eq!(q.in_flight.len(), 0);

    let sync_h = syncobjs.create(0).unwrap();
    syncobjs.attach(sync_h, 0, fence).unwrap();
    assert_eq!(
        syncobjs.poll_wait(&[sync_h], &[0], WAIT_ALL, false),
        Ok(WaitState::Ready { first: 0 })
    );

    // All GEM objects cleared
    assert_eq!(session.gem_objects.len(), 0);
}
