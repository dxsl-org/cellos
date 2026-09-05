#[path = "../../../kernel/src/task/drivers/iommu_riscv_cmd.rs"]
mod command;

#[test]
fn encodes_command_queue_base_fields() {
    assert_eq!(command::encode_cqb(0x8123_4000, 6), 0x0000_0000_2048_D005);
}

#[test]
fn encodes_iotinval_vma_pscid_fields() {
    assert_eq!(
        command::encode_iotinval_vma(0xA_BCDE),
        (0x0000_0001_ABCD_E001, 0)
    );
}

#[test]
fn encodes_iofence_c_opcode() {
    assert_eq!(command::encode_iofence_c(), (0x0000_0000_0000_0002, 0));
}

#[test]
fn encodes_iodir_inval_ddt_device_fields() {
    assert_eq!(
        command::encode_iodir_inval_ddt(0x12_3456),
        (0x1234_5602_0000_0003, 0)
    );
}

#[test]
fn masks_out_of_range_command_operands() {
    assert_eq!(
        command::encode_iotinval_vma(0xFF_A_BCDE),
        command::encode_iotinval_vma(0xA_BCDE)
    );
    assert_eq!(
        command::encode_iodir_inval_ddt(0xFF_12_3456),
        command::encode_iodir_inval_ddt(0x12_3456)
    );
}
