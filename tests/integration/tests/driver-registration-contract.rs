const SYSCALL_SOURCE: &str = include_str!("../../../kernel/src/task/syscall.rs");
const DRIVER_CELL_SOURCE: &str = include_str!("../../../kernel/src/task/drivers/driver_cell.rs");

fn assert_registration_contract(
    name: &str,
    source: &str,
    registration_branch: &str,
    failure_marker: &str,
) {
    assert!(
        !source.contains("let _ = sys_register_"),
        "{name} must not ignore driver-registration failures"
    );

    let state_ready = source
        .find("*STATE.lock() = Some(")
        .unwrap_or_else(|| panic!("{name} must publish initialized state"));
    let registration = source
        .find(registration_branch)
        .unwrap_or_else(|| panic!("{name} must check registration result"));
    assert!(
        state_ready < registration,
        "{name} must publish state before successful registration can route requests"
    );

    let failure = &source[registration..];
    let clear = failure
        .find("*STATE.lock() = None;")
        .unwrap_or_else(|| panic!("{name} must clear unpublished state on registration failure"));
    let diagnostic = failure
        .find(failure_marker)
        .unwrap_or_else(|| panic!("{name} must diagnose registration failure"));
    let exit = failure
        .find("sys_exit(1)")
        .unwrap_or_else(|| panic!("{name} must exit after registration failure"));
    assert!(
        clear < diagnostic && diagnostic < exit,
        "{name} registration failure must clear state, diagnose, then exit"
    );
}

#[test]
fn native_pcie_drivers_publish_ready_state_and_fail_closed() {
    assert_registration_contract(
        "e1000",
        include_str!("../../../cells/drivers/e1000/src/main.rs"),
        "if let Err(error) = sys_register_nic_driver()",
        "NIC driver registration failed",
    );
    assert_registration_contract(
        "nvme",
        include_str!("../../../cells/drivers/nvme/src/main.rs"),
        "if let Err(error) = sys_register_block_device(BlockDevice::Pci { bdf: bdf as u16 })",
        "block driver registration failed",
    );
}

#[test]
fn virtio_drivers_publish_ready_state_and_fail_closed() {
    assert_registration_contract(
        "virtio-net",
        include_str!("../../../cells/drivers/virtio-net/src/main.rs"),
        "if sys_register_nic_driver().is_err()",
        "NIC driver registration failed",
    );
    assert_registration_contract(
        "virtio-blk",
        include_str!("../../../cells/drivers/virtio-blk/src/main.rs"),
        "if let Err(error) = sys_register_block_device(BlockDevice::Mmio { base })",
        "block driver registration failed",
    );
}

/// The source text from `start` up to (not including) the next `end` after it.
fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let from = source.find(start).unwrap_or_else(|| panic!("missing `{start}`"));
    let to = source[from..]
        .find(end)
        .map(|offset| from + offset)
        .unwrap_or_else(|| panic!("missing `{end}` after `{start}`"));
    &source[from..to]
}

#[test]
fn kernel_rolls_back_nic_role_when_service_registry_rejects() {
    let handler = source_between(
        SYSCALL_SOURCE,
        "        // 417: RegisterNicDriver",
        "        // 418: FindPcieDevice",
    );
    assert!(handler.contains("register_driver_service("));
    assert!(handler.contains("register_nic_driver"));
    assert!(handler.contains("deregister_nic_driver"));

    assert!(DRIVER_CELL_SOURCE.contains("static ROLE_PUBLICATION: Spinlock<()>"));
    assert!(
        DRIVER_CELL_SOURCE
            .matches("let _publication = ROLE_PUBLICATION.lock();")
            .count()
            >= 2,
        "registration and teardown must share the publication transaction"
    );
    assert!(DRIVER_CELL_SOURCE.contains("service_registry::clear_tid(tid);"));
    let helper = source_between(
        SYSCALL_SOURCE,
        "fn register_driver_service",
        "fn caller_has_mmio_device",
    );
    assert!(helper.contains("let scheduler = crate::task::SCHEDULER.lock();"));
    assert!(helper.contains("task.pcie_driver_cap.is_some()"));
    assert!(helper.contains("crate::task::tcb::TaskState::Retiring"));
    assert!(helper.contains("publish_role_or_rollback("));
    assert!(helper.contains("drop(scheduler);"));
    assert!(
        helper.contains("Err(SyscallError::InvalidInput)"),
        "service-registry rejection must reach the Driver Cell"
    );
    assert!(DRIVER_CELL_SOURCE.contains("if publish_service()"));
    assert!(DRIVER_CELL_SOURCE.contains("rollback_role(tid);"));
    assert!(
        DRIVER_CELL_SOURCE.contains("rejected_service_publication_rolls_back_only_the_exact_owner")
    );
}

#[test]
fn kernel_binds_block_route_only_for_the_owned_device() {
    let handler = source_between(
        SYSCALL_SOURCE,
        "        // 416: RegisterBlockDriver",
        "        // 417: RegisterNicDriver",
    );
    assert!(handler.contains("register_block_device_route("));
    assert!(
        !handler.contains("register_driver_service("),
        "416 must not publish a singleton service"
    );

    let helper = source_between(
        SYSCALL_SOURCE,
        "fn register_block_device_route",
        "fn caller_has_platform",
    );
    let position = |needle: &str| {
        helper
            .find(needle)
            .unwrap_or_else(|| panic!("416 helper must contain `{needle}`"))
    };
    // Ownership is read outside SCHEDULER (the MMIO registry is taken before
    // SCHEDULER elsewhere), then the caller is re-checked live under SCHEDULER and
    // the route is published before that lock is dropped.
    position("task.pcie_driver_cap.is_some()");
    position("TaskState::Retiring");
    let pci_owner = position("owner_of_bdf(");
    let mmio_owner = position("lookup_mmio_owner(key.id as usize) == Some(caller_id)");
    let not_owner = position("Err(SyscallError::FileNotFound)");
    let locked = position("let scheduler = crate::task::SCHEDULER.lock();");
    let publish = position("publish_block_route(");
    let unlocked = position("drop(scheduler);");
    assert!(
        pci_owner < not_owner && mmio_owner < not_owner && not_owner < locked,
        "416 must prove device ownership before taking the scheduler lock for publication"
    );
    assert!(
        locked < publish && publish < unlocked,
        "416 must publish only an owned device's route, before dropping the scheduler lock"
    );

    let publication = source_between(
        DRIVER_CELL_SOURCE,
        "pub(crate) fn publish_block_route",
        "pub(crate) fn block_route_record",
    );
    assert!(
        publication.contains("let _publication = ROLE_PUBLICATION.lock();"),
        "route publication and exit teardown must share the publication transaction"
    );
    assert!(publication.contains("\"[driver_cell] block driver registered: tid={} device={} route={}\""));
    let teardown = source_between(DRIVER_CELL_SOURCE, "pub fn deregister_all_for", "\n}\n");
    assert!(teardown.contains("let _publication = ROLE_PUBLICATION.lock();"));
    assert!(teardown.contains("clear_block_route_provider(tid);"));
}
