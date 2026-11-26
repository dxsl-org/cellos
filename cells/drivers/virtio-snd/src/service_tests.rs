//! `AUDIO` request handling against fake Grants: ownership, takeover,
//! queue bounds and copy failures.

use crate::pcm::PcmQueue;
use crate::service::{DeviceView, Event, Grants, Outcome, Service, Totals};
use std::collections::HashMap;
use wire::audio::{
    AudioOpen, AudioReply, AudioWrite, CLOSE, MAX_CHUNK, STATUS_BUSY, STATUS_DEVICE,
    STATUS_INVALID, STATUS_IN_USE, STATUS_NOT_OPEN, STATUS_OK,
};

const A: usize = 41;
const B: usize = 42;
const OK: DeviceView = DeviceView {
    healthy: true,
    in_flight: 0,
};
const FAILED: DeviceView = DeviceView {
    healthy: false,
    in_flight: 0,
};

/// Grant id -> (owner tid, contents).
#[derive(Default)]
struct FakeGrants(HashMap<u32, (usize, Vec<u8>)>);

impl Grants for FakeGrants {
    fn len_owned_by(&mut self, grant: u32, owner: usize) -> Option<usize> {
        let (tid, bytes) = self.0.get(&grant)?;
        (*tid == owner).then_some(bytes.len())
    }

    fn copy(&mut self, grant: u32, owner: usize, offset: usize, dst: &mut [u8]) -> bool {
        match self.0.get(&grant) {
            Some((tid, bytes)) if *tid == owner && offset + dst.len() <= bytes.len() => {
                dst.copy_from_slice(&bytes[offset..offset + dst.len()]);
                true
            }
            _ => false,
        }
    }
}

/// A service with queue `capacity`, and Grant 1 of A and 2 of B, each a
/// whole chunk.
struct Harness {
    svc: Service,
    grants: FakeGrants,
}

impl Harness {
    fn new(capacity: usize) -> Self {
        let mut grants = FakeGrants::default();
        grants.give(1, A, MAX_CHUNK as usize, 7);
        grants.give(2, B, MAX_CHUNK as usize, 9);
        Self {
            svc: Service::new(PcmQueue::new(capacity)),
            grants,
        }
    }

    fn send(&mut self, msg: &[u8], tid: usize, view: DeviceView) -> Outcome {
        self.svc.handle(msg, tid, &mut self.grants, view)
    }

    /// The reply status of a call on a healthy device.
    fn call(&mut self, msg: &[u8], tid: usize) -> u32 {
        self.send(msg, tid, OK).reply.unwrap().status
    }
}

impl FakeGrants {
    fn give(&mut self, grant: u32, owner: usize, len: usize, fill: u8) {
        self.0.insert(grant, (owner, vec![fill; len]));
    }
}

fn open(grant: u32) -> [u8; 8] {
    AudioOpen { grant }.encode()
}

fn write(len: u32) -> [u8; 8] {
    AudioWrite { len }.encode()
}

fn closed(tid: usize, played: u64, dropped: u64) -> Option<Event> {
    let totals = Totals { played, dropped };
    Some(Event::Closed { tid, totals })
}

#[test]
fn first_open_wins_and_a_live_holder_keeps_the_stream() {
    let mut h = Harness::new(38_400);
    let first = h.send(&open(1), A, OK);
    assert_eq!(first.reply.unwrap().status, STATUS_OK);
    assert_eq!(first.events, [None, Some(Event::Opened { tid: A })]);
    assert_eq!(h.call(&open(2), B), STATUS_IN_USE);
    assert_eq!(h.call(&write(4), B), STATUS_NOT_OPEN);
    // B's CLOSE is ignored and unanswered.
    assert_eq!(h.send(&[CLOSE], B, OK), Outcome::default());
    assert!(h.svc.held());
}

#[test]
fn close_releases_and_reports_the_session_totals() {
    let mut h = Harness::new(38_400);
    h.call(&open(1), A);
    assert_eq!(h.call(&write(1920), A), STATUS_OK);
    let close = h.send(&[CLOSE], A, OK);
    assert_eq!(close.reply, None);
    assert_eq!(close.events, [closed(A, 1920, 0), None]);
    assert!(!h.svc.held());
    assert_eq!(h.call(&open(2), B), STATUS_OK);
}

#[test]
fn a_gone_holder_is_taken_over() {
    let mut h = Harness::new(38_400);
    h.call(&open(1), A);
    h.grants.0.remove(&1); // the kernel dropped the dead holder's Grants
    let taken = h.send(&open(2), B, OK);
    assert_eq!(taken.reply.unwrap().status, STATUS_OK);
    let opened = Some(Event::Opened { tid: B });
    assert_eq!(taken.events, [closed(A, 0, 0), opened]);
}

#[test]
fn open_needs_an_owned_grant_of_a_whole_chunk() {
    let mut h = Harness::new(38_400);
    h.grants.give(3, A, MAX_CHUNK as usize - 4, 0);
    assert_eq!(h.call(&open(3), A), STATUS_INVALID, "short Grant");
    assert_eq!(h.call(&open(2), A), STATUS_INVALID, "B's Grant");
    assert_eq!(h.call(&open(9), A), STATUS_INVALID, "unknown Grant");
    let mut bad = open(1);
    bad[2] = 1;
    assert_eq!(h.call(&bad, A), STATUS_INVALID, "reserved byte");
    assert!(!h.svc.held());
}

#[test]
fn a_full_queue_answers_busy_and_counts_the_drop() {
    let mut h = Harness::new(8_000);
    h.call(&open(1), A);
    assert_eq!(h.call(&write(6_000), A), STATUS_OK);
    let busy = h.send(&write(4_000), A, OK).reply;
    let queued = 6_000;
    let status = STATUS_BUSY;
    assert_eq!(busy, Some(AudioReply { status, queued }));
    let view = DeviceView {
        healthy: true,
        in_flight: 1920,
    };
    let fits = h.send(&write(2_000), A, view).reply;
    let queued = 8_000 + 1920;
    let status = STATUS_OK;
    assert_eq!(fits, Some(AudioReply { status, queued }));
    let close = h.send(&[CLOSE], A, OK);
    assert_eq!(close.events[0], closed(A, 8_000, 4_000));
}

#[test]
fn copied_bytes_land_in_the_queue_in_order() {
    let mut h = Harness::new(38_400);
    h.call(&open(1), A);
    h.call(&write(8), A);
    let mut out = [0u8; 16];
    assert_eq!(h.svc.queue.pop_into(&mut out), 8);
    assert_eq!(&out[..8], &[7; 8]);
}

#[test]
fn a_withdrawn_grant_or_failed_device_drops_the_write() {
    let mut h = Harness::new(38_400);
    h.call(&open(1), A);
    let status = |outcome: Outcome| outcome.reply.unwrap().status;
    assert_eq!(status(h.send(&write(4), A, FAILED)), STATUS_DEVICE);
    assert_eq!(status(h.send(&open(1), B, FAILED)), STATUS_DEVICE);
    h.grants.0.remove(&1);
    assert_eq!(h.call(&write(4), A), STATUS_INVALID);
    assert!(h.svc.queue.is_empty());
    assert_eq!(h.call(&write(2), A), STATUS_INVALID, "not whole frames");
    assert_eq!(h.call(&[0x7F], A), STATUS_INVALID, "unknown opcode");
}
