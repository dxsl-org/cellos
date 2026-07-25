#[path = "../../../cells/services/hypervisor/src/virtqueue_guard.rs"]
mod guard;

#[test]
fn pending_entries_must_fit_the_queue() {
    assert_eq!(guard::pending_count(8, 10, 18), Some(8));
    assert_eq!(guard::pending_count(8, 10, 19), None);
    assert_eq!(guard::pending_count(0, 0, 0), None);
}

#[test]
fn pending_count_handles_u16_wrap() {
    assert_eq!(guard::pending_count(8, u16::MAX - 2, 3), Some(6));
}

#[test]
fn descriptor_indices_are_strictly_bounded() {
    assert!(guard::valid_descriptor(0, 8));
    assert!(guard::valid_descriptor(7, 8));
    assert!(!guard::valid_descriptor(8, 8));
    assert!(!guard::valid_descriptor(usize::MAX, 8));
}
