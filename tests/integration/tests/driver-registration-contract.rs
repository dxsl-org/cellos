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
        "if let Err(error) = sys_register_block_driver()",
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
        "if sys_register_block_driver().is_err()",
        "block driver registration failed",
    );
}

#[test]
fn kernel_rolls_back_driver_role_when_service_registry_rejects() {
    for (role, start_marker, end_marker, register, deregister) in [
        (
            "block",
            "        // 416: RegisterBlockDriver",
            "        // 417: RegisterNicDriver",
            "register_block_driver",
            "deregister_block_driver",
        ),
        (
            "NIC",
            "        // 417: RegisterNicDriver",
            "        // 418: FindPcieDevice",
            "register_nic_driver",
            "deregister_nic_driver",
        ),
    ] {
        let start = SYSCALL_SOURCE
            .find(start_marker)
            .unwrap_or_else(|| panic!("missing {role} registration handler"));
        let end = SYSCALL_SOURCE[start..]
            .find(end_marker)
            .map(|offset| start + offset)
            .unwrap_or_else(|| panic!("missing end of {role} registration handler"));
        let handler = &SYSCALL_SOURCE[start..end];

        assert!(handler.contains("publish_role_or_rollback("));
        assert!(handler.contains(register));
        assert!(handler.contains(deregister));
        assert!(
            handler.contains("Err(SyscallError::InvalidInput)"),
            "{role} service-registry rejection must reach the Driver Cell"
        );
    }
    assert!(DRIVER_CELL_SOURCE.contains("static ROLE_PUBLICATION: Spinlock<()>"));
    assert!(
        DRIVER_CELL_SOURCE
            .matches("let _publication = ROLE_PUBLICATION.lock();")
            .count()
            >= 2,
        "registration and teardown must share the publication transaction"
    );
    assert!(DRIVER_CELL_SOURCE.contains("service_registry::clear_tid(tid);"));
    assert!(DRIVER_CELL_SOURCE.contains("let scheduler = crate::task::SCHEDULER.lock();"));
    assert!(DRIVER_CELL_SOURCE.contains("crate::task::tcb::TaskState::Retiring"));
    assert!(DRIVER_CELL_SOURCE.contains("drop(scheduler);"));
    assert!(DRIVER_CELL_SOURCE.contains("if publish_service()"));
    assert!(DRIVER_CELL_SOURCE.contains("rollback_role(tid);"));
    assert!(
        DRIVER_CELL_SOURCE.contains("rejected_service_publication_rolls_back_only_the_exact_owner")
    );
}
