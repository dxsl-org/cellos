//! Host tests for how a ring channel is placed over a Grant mapping (audit H-30):
//! only a mapping that holds a whole, aligned channel becomes a view.

use super::{channel_in_grant, BiRingChannel, FastpathEndpoint};
use core::mem::{align_of, size_of};

/// Backing that is aligned for the channel and one channel plus a spare word long.
fn backing() -> Vec<u64> {
    assert!(align_of::<BiRingChannel>() <= align_of::<u64>());
    vec![0u64; size_of::<BiRingChannel>() / 8 + 1]
}

#[test]
fn a_whole_aligned_mapping_becomes_a_working_channel() {
    let mut mem = backing();
    let ptr = mem.as_mut_ptr().cast::<u8>();
    let channel = channel_in_grant(ptr, size_of::<BiRingChannel>()).expect("whole channel");
    assert_eq!(channel.as_ptr().cast::<u8>(), ptr);

    // SAFETY: `mem` is zeroed (a valid all-atomics channel) and outlives the view.
    let channel = unsafe { channel.as_ref() };
    let host = FastpathEndpoint::new_endpoint_a(channel);
    let client = FastpathEndpoint::new_endpoint_b(channel);
    host.tx.try_send(b"ping", 1, 0).expect("ring has room");
    let mut out = [0u8; 8];
    let meta = client
        .rx
        .try_recv(&mut out)
        .expect("valid slot")
        .expect("one message");
    assert_eq!(&out[..meta.len], b"ping");
}

#[test]
fn refuses_a_mapping_shorter_than_a_channel() {
    // The old token check accepted any non-null aligned address; a short Grant
    // would let ring stores run past the peer's mapping.
    let mut mem = backing();
    let ptr = mem.as_mut_ptr().cast::<u8>();
    assert!(channel_in_grant(ptr, size_of::<BiRingChannel>() - 1).is_none());
    assert!(channel_in_grant(ptr, 0).is_none());
}

#[test]
fn refuses_a_misaligned_mapping() {
    let mut mem = backing();
    // SAFETY: one byte into a buffer longer than a channel stays in bounds.
    let ptr = unsafe { mem.as_mut_ptr().cast::<u8>().add(1) };
    assert!(channel_in_grant(ptr, size_of::<BiRingChannel>()).is_none());
}

#[test]
fn refuses_a_null_mapping() {
    assert!(channel_in_grant(core::ptr::null_mut(), size_of::<BiRingChannel>()).is_none());
}
