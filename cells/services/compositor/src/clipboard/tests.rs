//! The clipboard rules against a fake kernel that keeps Grant owners and the
//! one receiver a Grant has.

extern crate std;

use std::collections::BTreeMap;
use std::vec;
use std::vec::Vec;

use wire::display::{ClipboardChanged, ClipboardGet, ClipboardSet, SurfaceRole, CLIPBOARD_MAX};

use super::{Clipboard, Focus, Kernel};
use crate::surface_table::SurfaceTable;

const APP: usize = 7;
const PEER: usize = 9;
/// Owner tid the fake kernel records for the compositor's own Grant.
const COMPOSITOR: usize = 1;
const COMPOSITOR_GRANT: usize = 0x9000_0000;

#[derive(Default)]
struct FakeKernel {
    /// Grant id to `(owner, bytes)`.
    grants: BTreeMap<usize, (usize, Vec<u8>)>,
    /// `(grant, tid)` of the last share.
    receiver: Option<(usize, usize)>,
    sent: Vec<(usize, ClipboardChanged)>,
    refuse_sends: bool,
}

impl FakeKernel {
    /// The text `tid` reads for `event`, which it may only as the receiver.
    fn read(&self, tid: usize, event: &ClipboardChanged) -> Vec<u8> {
        assert_eq!(self.receiver, Some((event.grant as usize, tid)));
        self.grants[&(event.grant as usize)].1[..event.len as usize].to_vec()
    }
}

impl Kernel for FakeKernel {
    fn copy_from(&mut self, grant: usize, owner: usize, dst: &mut [u8]) -> bool {
        match self.grants.get(&grant) {
            Some((holder, bytes)) if *holder == owner && bytes.len() >= dst.len() => {
                dst.copy_from_slice(&bytes[..dst.len()]);
                true
            }
            _ => false,
        }
    }

    fn register(&mut self) -> Option<usize> {
        let region = (COMPOSITOR, vec![0; CLIPBOARD_MAX]);
        self.grants.insert(COMPOSITOR_GRANT, region);
        Some(COMPOSITOR_GRANT)
    }

    fn write(&mut self, grant: usize, text: &[u8]) -> bool {
        match self.grants.get_mut(&grant) {
            Some((COMPOSITOR, bytes)) => {
                bytes[..text.len()].copy_from_slice(text);
                true
            }
            _ => false,
        }
    }

    fn share(&mut self, grant: usize, tid: usize) -> bool {
        let owned = self
            .grants
            .get(&grant)
            .is_some_and(|(owner, _)| *owner == COMPOSITOR);
        if owned {
            self.receiver = Some((grant, tid));
        }
        owned
    }

    fn send(&mut self, tid: usize, frame: &[u8]) -> bool {
        if self.refuse_sends {
            return false;
        }
        self.sent
            .push((tid, ClipboardChanged::decode(frame).unwrap()));
        true
    }
}

struct World {
    table: SurfaceTable,
    clipboard: Clipboard,
    kernel: FakeKernel,
    /// APP's surface and PEER's.
    a: u64,
    b: u64,
}

impl World {
    fn new() -> Self {
        let mut table = SurfaceTable::new();
        let a = table
            .create(0, 0, 8, 8, APP, SurfaceRole::Interactive)
            .unwrap();
        let b = table
            .create(0, 0, 8, 8, PEER, SurfaceRole::Interactive)
            .unwrap();
        Self {
            table,
            clipboard: Clipboard::new(),
            kernel: FakeKernel::default(),
            a,
            b,
        }
    }

    fn focus_a(&self) -> Focus {
        Some((APP, self.a))
    }

    fn focus_b(&self) -> Focus {
        Some((PEER, self.b))
    }

    /// `owner` writes `text` into a Grant of its own and sets it through `cap`.
    fn set(&mut self, owner: usize, cap: u64, focus: Focus, text: &[u8]) -> Option<usize> {
        let grant = 0x100 + owner;
        self.kernel.grants.insert(grant, (owner, text.to_vec()));
        self.set_from(owner, cap, focus, grant, text.len())
    }

    fn set_from(
        &mut self,
        owner: usize,
        cap: u64,
        focus: Focus,
        grant: usize,
        len: usize,
    ) -> Option<usize> {
        let request = ClipboardSet {
            cap: cap as u32,
            grant: grant as u32,
            len: len as u32,
        };
        let (table, kernel) = (&mut self.table, &mut self.kernel);
        self.clipboard.set(&request, owner, focus, table, kernel)
    }

    fn get(&mut self, owner: usize, cap: u64, focus: Focus) -> bool {
        let request = ClipboardGet { cap: cap as u32 };
        let (table, kernel) = (&mut self.table, &mut self.kernel);
        self.clipboard.get(&request, owner, focus, table, kernel)
    }

    fn offer_to_focus(&mut self, focus: Focus) {
        let (table, kernel) = (&mut self.table, &mut self.kernel);
        self.clipboard.offer_to_focus(focus, table, kernel);
    }

    fn take_sent(&mut self) -> Vec<(usize, ClipboardChanged)> {
        core::mem::take(&mut self.kernel.sent)
    }
}

#[test]
fn only_the_focused_surfaces_owner_sets_or_gets() {
    let mut world = World::new();
    let (a, b, focus_a) = (world.a, world.b, world.focus_a());
    assert_eq!(world.set(PEER, b, focus_a, b"x"), None, "unfocused");
    assert_eq!(world.set(PEER, a, focus_a, b"x"), None, "not the owner");
    assert_eq!(
        world.set(PEER, a, Some((PEER, a)), b"x"),
        None,
        "not the owner"
    );
    assert_eq!(world.set(APP, a, None, b"x"), None, "console has focus");
    assert!(!world.get(PEER, b, focus_a));
    assert!(!world.get(PEER, a, focus_a));
    assert!(!world.get(APP, a, None));
    assert!(world.take_sent().is_empty());
    assert_eq!(world.clipboard.serial, 0);

    // Nothing copied yet: the answer is empty, with no Grant.
    assert!(world.get(APP, a, focus_a));
    let empty = ClipboardChanged {
        cap: a as u32,
        grant: 0,
        len: 0,
        serial: 0,
    };
    assert_eq!(world.take_sent(), vec![(APP, empty)]);
}

#[test]
fn over_limit_unreadable_or_non_utf8_text_is_refused() {
    let mut world = World::new();
    let (a, focus_a) = (world.a, world.focus_a());
    let huge = vec![b'a'; CLIPBOARD_MAX + 1];
    assert_eq!(world.set(APP, a, focus_a, &huge), None, "over the limit");
    assert_eq!(world.set(APP, a, focus_a, &[0xff, 0xfe]), None, "not UTF-8");
    // PEER's Grant named by APP: the owner check fails.
    world.kernel.grants.insert(0x500, (PEER, b"hi".to_vec()));
    assert_eq!(
        world.set_from(APP, a, focus_a, 0x500, 2),
        None,
        "not its Grant"
    );
    assert_eq!(world.set_from(APP, a, focus_a, 0x501, 2), None, "no Grant");
    assert_eq!(world.clipboard.serial, 0);

    let full = vec![b'a'; CLIPBOARD_MAX];
    assert_eq!(world.set(APP, a, focus_a, &full), Some(CLIPBOARD_MAX));
}

#[test]
fn identical_text_does_not_bump_the_serial() {
    let mut world = World::new();
    let (a, b, focus_a, focus_b) = (world.a, world.b, world.focus_a(), world.focus_b());
    assert_eq!(world.set(APP, a, focus_a, b"hi"), Some(2));
    assert_eq!(world.set(APP, a, focus_a, b"hi"), None);
    assert_eq!(world.clipboard.serial, 1);
    assert_eq!(world.set(APP, a, focus_a, b"yo"), Some(2));
    assert_eq!(world.clipboard.serial, 2);

    // The peer gets the text and echoes it back: nothing changes, and APP is
    // offered nothing on its return.
    world.offer_to_focus(focus_b);
    assert_eq!(world.take_sent().len(), 1);
    assert_eq!(world.set(PEER, b, focus_b, b"yo"), None);
    assert_eq!(world.clipboard.serial, 2);
    world.offer_to_focus(focus_a);
    assert!(world.take_sent().is_empty());
}

#[test]
fn focus_gain_offers_each_change_once() {
    let mut world = World::new();
    let (a, b, focus_a, focus_b) = (world.a, world.b, world.focus_a(), world.focus_b());
    world.offer_to_focus(focus_b);
    assert!(world.take_sent().is_empty(), "nothing copied yet");
    assert_eq!(world.set(APP, a, focus_a, b"copied"), Some(6));
    world.offer_to_focus(focus_a);
    assert!(world.take_sent().is_empty(), "the setter saw it");

    world.offer_to_focus(focus_b);
    let sent = world.take_sent();
    let offer = ClipboardChanged {
        cap: b as u32,
        grant: COMPOSITOR_GRANT as u32,
        len: 6,
        serial: 1,
    };
    assert_eq!(sent, vec![(PEER, offer)]);
    assert_eq!(world.kernel.read(PEER, &offer), b"copied");
    for focus in [focus_b, focus_a, focus_b] {
        world.offer_to_focus(focus);
    }
    assert!(world.take_sent().is_empty(), "no repeat");

    assert_eq!(world.set(APP, a, focus_a, b"again"), Some(5));
    world.offer_to_focus(focus_b);
    let sent = world.take_sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1.serial, 2);
    assert_eq!(world.kernel.read(PEER, &sent[0].1), b"again");

    // A surface created later has seen nothing.
    let c = world
        .table
        .create(0, 0, 8, 8, PEER, SurfaceRole::Interactive)
        .unwrap();
    world.offer_to_focus(Some((PEER, c)));
    assert_eq!(world.take_sent().len(), 1);
}

#[test]
fn get_answers_the_focused_owner_and_undelivered_offers_retry() {
    let mut world = World::new();
    let (a, b, focus_a, focus_b) = (world.a, world.b, world.focus_a(), world.focus_b());
    assert_eq!(world.set(APP, a, focus_a, b"hi"), Some(2));

    world.kernel.refuse_sends = true;
    world.offer_to_focus(focus_b);
    assert!(!world.get(PEER, b, focus_b));
    world.kernel.refuse_sends = false;
    world.offer_to_focus(focus_b);
    assert_eq!(world.take_sent().len(), 1, "retried once delivered");
    world.offer_to_focus(focus_b);
    assert!(world.take_sent().is_empty());

    // A get is answered even when the surface has seen the text.
    assert!(world.get(PEER, b, focus_b));
    let sent = world.take_sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(world.kernel.read(PEER, &sent[0].1), b"hi");
    // APP asks too: the Grant now goes to APP.
    assert!(world.get(APP, a, focus_a));
    let sent = world.take_sent();
    assert_eq!(world.kernel.read(APP, &sent[0].1), b"hi");
}
