use alloc::collections::BTreeMap;

use crate::caller::Caller;

pub(super) fn key(owner: Caller) -> (u64, u64) {
    (owner.cell.0, owner.generation)
}

pub(super) fn decrement(counts: &mut BTreeMap<(u64, u64), usize>, owner: Caller) {
    let key = key(owner);
    if let Some(count) = counts.get_mut(&key) {
        *count = count.saturating_sub(1);
        if *count == 0 {
            counts.remove(&key);
        }
    }
}
