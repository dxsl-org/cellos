use super::media::*;
use super::support::*;
use crate::*;

fn reboot_storage(old: &[u8], next: &[u8]) -> FakeStorage {
    FakeStorage {
        slots: [old.to_vec(), next.to_vec()],
        fault: StorageFault::None,
        events: std::vec::Vec::new(),
    }
}

fn reboot_counter() -> FakeCounter {
    FakeCounter {
        value: 2,
        fail_increment: false,
        sealed: false,
        events: std::vec::Vec::new(),
    }
}

#[test]
fn every_precompletion_write_cut_seals_on_reboot_without_commit_returning() {
    let current = full_record(SlotRole::A);
    let next = successor(&current);
    let old_bytes = encode_full(&current);
    let next_bytes = encode_full(&next);

    // Prefix length zero covers cuts immediately after counter increment and
    // after erasing the inactive slot. Every other prefix is a torn write.
    for prefix_len in 0..next_bytes.len() {
        let storage = reboot_storage(&old_bytes, &next_bytes[..prefix_len]);
        let mut rebooted = Journal::new(reboot_counter(), storage, TestAuth, identity());
        assert_eq!(rebooted.recover(), Err(JournalError::Sealed));
        let (counter, _, _) = rebooted.into_parts();
        assert!(counter.sealed, "unsealed cut at prefix {prefix_len}");
        assert!(counter.events.contains(&Event::CounterSeal));
    }
}

#[test]
fn full_write_or_readback_cut_recovers_exact_new_record() {
    let current = full_record(SlotRole::A);
    let next = successor(&current);
    let storage = reboot_storage(&encode_full(&current), &encode_full(&next));
    let mut rebooted = Journal::new(reboot_counter(), storage, TestAuth, identity());
    let recovered = rebooted.recover().unwrap();
    assert_eq!(recovered.record(), &next);
    let (counter, _, _) = rebooted.into_parts();
    assert!(!counter.sealed);
}
