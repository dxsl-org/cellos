const SYSCALL_SOURCE: &str = include_str!("../../../kernel/src/task/syscall.rs");
const TASK_SOURCE: &str = include_str!("../../../kernel/src/task.rs");
const IOMMU_SOURCE: &str = include_str!("../../../kernel/src/task/drivers/iommu.rs");
const RISCV_IOMMU_SOURCE: &str = include_str!("../../../kernel/src/task/drivers/iommu_riscv.rs");

fn handler_section(start: &str, end: &str) -> &'static str {
    let start = SYSCALL_SOURCE
        .find(start)
        .unwrap_or_else(|| panic!("missing handler start: {start}"));
    let end = SYSCALL_SOURCE[start..]
        .find(end)
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing handler end: {end}"));
    &SYSCALL_SOURCE[start..end]
}

#[test]
fn driver_discovery_claims_bdf_ownership() {
    let find = handler_section("        // 418: FindPcieDevice", "        // 234: WaitIrq");

    assert!(
        find.contains("if !crate::resource_registry::claim_bdf_owner(bdf, caller_id)"),
        "the selected Driver Cell must atomically claim the BDF used by GrantDma"
    );
    assert!(
        find.contains("return Ok(0);"),
        "a competing Driver Cell must observe the live device as unavailable"
    );
}

#[test]
fn platform_bar_enumeration_cannot_overwrite_driver_ownership() {
    let register_bar = handler_section(
        "        // 235: RegisterPcieBar",
        "        // 236: RegisterPciDevice",
    );

    assert!(
        register_bar.contains("register_pcie_bar(base, len)"),
        "Platform registration must still publish the BAR allowlist"
    );
    assert!(
        !register_bar.contains("bdf_owner"),
        "Platform enumeration must not steal BDF ownership from a Driver Cell"
    );
}

#[test]
fn bdf_release_waits_for_iommu_teardown_acknowledgement() {
    let start = TASK_SOURCE
        .find("fn release_retired_dma_after_ack")
        .expect("missing acknowledged DMA resource release helper");
    let end = TASK_SOURCE[start..]
        .find("pub fn spawn(")
        .map(|offset| start + offset)
        .expect("missing end of resource-reaper section");
    let reap = &TASK_SOURCE[start..end];

    let cleanup = reap
        .find("crate::task::drivers::iommu::cleanup_cell")
        .expect("IOMMU cleanup acknowledgement must gate resource release");
    let release = reap
        .find("release_bdfs_for(tid)")
        .expect("acknowledged teardown must release BDF ownership");
    assert!(cleanup < release);
    assert_eq!(
        reap.matches("release_bdfs_for(tid)").count(),
        1,
        "BDF ownership must have no pre-acknowledgement release path"
    );
    assert!(
        reap.contains("queue_retired_dma_cleanup(tid)"),
        "unacknowledged teardown must remain queued"
    );
    assert!(
        reap.contains("retry_retired_dma_cleanup"),
        "scheduler retirement must retry deferred hardware acknowledgement"
    );
    assert!(reap.contains("let _cleanup = IOMMU_CLEANUP_SERIAL.lock();"));
    assert!(
        !reap.contains("retries.contains("),
        "one retry must not scan every quarantined TID"
    );
}

#[test]
fn riscv_cleanup_propagates_hardware_acknowledgement() {
    assert!(
        IOMMU_SOURCE.contains("super::iommu_riscv::unmap_cell(tid)"),
        "common cleanup must return the RISC-V teardown result"
    );
    assert!(
        !IOMMU_SOURCE.contains("super::iommu_riscv::unmap_cell(tid);\n        true"),
        "RISC-V cleanup must not claim unconditional acknowledgement"
    );

    let start = RISCV_IOMMU_SOURCE
        .find("pub(super) fn unmap_cell")
        .expect("missing RISC-V Cell teardown");
    let end = RISCV_IOMMU_SOURCE[start..]
        .find("// ── Phase 3")
        .map(|offset| start + offset)
        .expect("missing end of RISC-V teardown section");
    let teardown = &RISCV_IOMMU_SOURCE[start..end];
    let directory = teardown
        .find("invalidate_dc")
        .expect("missing IODIR command");
    let translations = teardown
        .find("invalidate_pscid_tlb")
        .expect("missing IOTINVAL command");
    let fence = teardown
        .find("issue_iofence")
        .expect("missing IOFENCE command");
    assert!(
        directory < translations && translations < fence,
        "teardown must enqueue IODIR then IOTINVAL before IOFENCE"
    );
    assert!(
        teardown
            .matches("RISCV_DOMAINS.lock().insert(tid, domain)")
            .count()
            >= 2,
        "failed teardown must restore the quarantined domain"
    );
    assert!(
        teardown.contains("return false;"),
        "unacknowledged teardown must propagate failure"
    );
}

#[test]
fn riscv_mapping_reports_unconfirmed_publication() {
    let start = RISCV_IOMMU_SOURCE
        .find("pub(super) fn map_range_for_cell")
        .expect("missing RISC-V DMA mapping path");
    let end = RISCV_IOMMU_SOURCE[start..]
        .find("/// Backward-compat")
        .map(|offset| start + offset)
        .expect("missing end of RISC-V mapping section");
    let mapping = &RISCV_IOMMU_SOURCE[start..end];

    assert!(
        mapping.contains("return classify_dma_publication("),
        "mapping must classify command publication and IOFENCE acknowledgement"
    );
    let directory = mapping
        .find("invalidate_dc")
        .expect("missing IODIR command");
    let translations = mapping
        .find("invalidate_pscid_tlb")
        .expect("missing IOTINVAL command");
    let fence = mapping
        .find("issue_iofence")
        .expect("missing IOFENCE command");
    assert!(
        directory < translations && translations < fence,
        "mapping must enqueue IODIR then IOTINVAL before IOFENCE"
    );
    assert!(!mapping.contains("let _ = invalidate_dc"));
    assert!(!mapping.contains("let _ = invalidate_pscid_tlb"));
    assert!(!mapping.contains("let _ = issue_iofence"));
}

#[test]
fn riscv_command_queue_uses_v101_register_protocol() {
    for register in [
        "const REG_CQT: usize = 0x24;",
        "const REG_CQCSR: usize = 0x48;",
        "const REG_IPSR: usize = 0x54;",
    ] {
        assert!(RISCV_IOMMU_SOURCE.contains(register), "missing {register}");
    }
    assert!(RISCV_IOMMU_SOURCE.contains("encode_cqb(cq_virt as u64, CQ_LOG2)"));
    assert!(RISCV_IOMMU_SOURCE.contains("write32(bar0, REG_CQT, 0)"));
    assert!(RISCV_IOMMU_SOURCE.contains("write32(bar0, REG_CQCSR, CQCSR_CQEN)"));
    assert!(RISCV_IOMMU_SOURCE.contains("wait_cq_state(bar0, true)"));

    let enqueue = &RISCV_IOMMU_SOURCE[RISCV_IOMMU_SOURCE
        .find("fn enqueue_cmd")
        .expect("missing CQ producer")..];
    let release = enqueue
        .find("fence(Ordering::Release)")
        .expect("missing release fence before tail publication");
    let publish = enqueue
        .find("write32(bar0, REG_CQT")
        .expect("CQT must use a 32-bit MMIO write");
    assert!(release < publish);

    assert!(
        RISCV_IOMMU_SOURCE
            .matches("let _transaction = CQ_TRANSACTION.lock()")
            .count()
            >= 2,
        "mapping and teardown must serialize complete CQ transactions"
    );

    let activation = &RISCV_IOMMU_SOURCE[RISCV_IOMMU_SOURCE
        .find("pub(super) fn activate")
        .expect("missing IOMMU activation")..];
    let ddtp_write = activation.find("write64(bar0, REG_DDTP").unwrap();
    let busy_poll = activation.find("wait_ddtp_ready(bar0)").unwrap();
    let claim_active = activation.find("super::iommu::set_active()").unwrap();
    assert!(ddtp_write < busy_poll && busy_poll < claim_active);
}
