// SPDX-License-Identifier: Apache-2.0
//! Reply admission for the POSIX socket shim's net-service round trips (H-32).
//!
//! `Recv` returns the **sender tid**, never a byte count: the payload length is
//! not reported. Two mistakes followed from reading it as a length — slicing the
//! reply to `tid` bytes (a tid above the buffer size panicked the cell, a small
//! tid truncated the frame) and decoding whatever arrived, so any task that could
//! `Send` to the socket cell between its request and its `Recv` forged the
//! answer (`Data` payload, `CapId`, resolved `Addr`).
//!
//! The rule here: a reply counts only when the kernel stamped it with the tid of
//! the live NET provider the request went to, and it is decoded from the whole
//! receive buffer. Postcard frames are self-delimiting and `decode` tolerates the
//! buffer's zero tail, so no length is needed.

use crate::ipc::{decode, NetResponse};
use abi::service_binding::ServiceBinding;

/// Decode `buf` as the NET provider's reply, or refuse it.
///
/// `provider` is the binding the request was sent to, read from
/// `LookupServiceBound` for this round trip; `recv_ret` is `Recv`'s raw return.
/// Refuses a non-live binding, a failed `Recv`, any other sender, and a frame
/// that does not decode.
pub fn admit_net_reply<'b>(
    provider: &ServiceBinding,
    recv_ret: isize,
    buf: &'b [u8],
) -> Option<NetResponse<'b>> {
    if !provider.is_live() || recv_ret <= 0 || recv_ret as u64 != provider.tid {
        return None;
    }
    decode::<NetResponse>(buf).ok()
}

#[cfg(test)]
#[path = "net-reply-tests.rs"]
mod tests;
