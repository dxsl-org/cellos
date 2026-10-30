//! Host-only read-back of wiped heap buffers: the one test in this crate that needs `unsafe`.
//!
//! `Secret`/`Event`/`Paste::wipe` delegate to `ostd::session::zeroize_vec`, which zeroes the
//! vector's whole allocation (spare capacity included) before truncating it. Observing that
//! the bytes behind a cleared `Vec` are zero requires reading the allocation through a raw
//! pointer, so the test lives in its own file: this is the only file of the crate (apart from
//! the driver `virtio_device.rs`) that `scripts/unsafe-allowlist.toml` lets contain `unsafe`,
//! which keeps `session.rs` itself under the token rule.

use super::*;

const SECRET: &[u8] = b"hunter2-correct-horse";
fn who() -> Principal { Principal { cell_id: 7, generation: 1 } }
fn operation() -> Ref { Ref { id: 9, generation: 1 } }

#[test]
fn staging_and_event_buffers_are_zeroed_but_keep_their_allocation_until_dropped() {
    let mut secret = Secret(Vec::with_capacity(64)); secret.extend_from_slice(SECRET);
    let (pointer, capacity) = (secret.as_ptr(), secret.capacity());
    secret.wipe();
    assert!(secret.is_empty());
    // SAFETY: the allocation is still owned by `secret`; every byte was just overwritten.
    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));

    let mut event = Event { sequence: 1, bytes: SECRET.to_vec(), sent: 0 };
    let (pointer, capacity) = (event.bytes.as_ptr(), event.bytes.capacity());
    event.wipe();
    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));

    let mut paste = Paste { reference: operation(), frontend: who(), attachment: operation(), epoch: 1, expires: 0, total: SECRET.len(), bytes: SECRET.to_vec() };
    let (pointer, capacity) = (paste.bytes.as_ptr(), paste.bytes.capacity());
    paste.wipe();
    assert!(paste.bytes.is_empty());
    assert!(unsafe { core::slice::from_raw_parts(pointer, capacity) }.iter().all(|b| *b == 0));
}
