#[path = "../../../cells/services/hypervisor/src/virtio_gpu/rules.rs"]
mod rules;

#[test]
fn resource_ids_must_be_nonzero_and_unique() {
    assert!(!rules::valid_new_resource_id(0, false));
    assert!(!rules::valid_new_resource_id(7, true));
    assert!(rules::valid_new_resource_id(7, false));
}
