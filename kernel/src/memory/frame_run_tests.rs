//! Host tests for every `FrameRun` transition and the pending list, driven by a
//! fake acknowledgement source over a synthetic allocator.

extern crate std;

use super::*;
use crate::memory::frame::{allocator_for_tests, FrameAllocator};
use crate::sync::Rank;
use std::cell::RefCell;
use std::vec;

const BASE: usize = 0x4000_0000;
const OWNER: RunOwner = RunOwner::Cell {
    cell: 7,
    generation: 2,
    tid: 31,
};

/// Fake hart set: hart 0 is the caller; `online` remotes answer requests only
/// when the test publishes their epoch.
struct Fake {
    allocator: FrameAllocator,
    online: Vec<usize>,
    requested: Epochs,
    completed: Epochs,
    reissues: usize,
    reissued_tags: Vec<Option<usize>>,
    normalised: Vec<PhysAddr>,
}

impl Fake {
    fn new(frames: usize) -> Self {
        Self {
            allocator: allocator_for_tests(&[(BASE, frames)]),
            online: vec![1, 2],
            requested: [0; MAX_HARTS],
            completed: [0; MAX_HARTS],
            reissues: 0,
            reissued_tags: Vec::new(),
            normalised: Vec::new(),
        }
    }

    /// Ask every online remote once, as a shootdown does.
    fn request(&mut self) -> Epochs {
        let mut epochs = [0; MAX_HARTS];
        for &hart in &self.online {
            self.requested[hart] += 1;
            epochs[hart] = self.requested[hart];
        }
        epochs
    }

    /// `hart` serves every request it has been sent.
    fn answer(&mut self, hart: usize) {
        self.completed[hart] = self.requested[hart];
    }

    fn unconfirmed(&mut self) -> FlushOutcome {
        FlushOutcome::Unconfirmed {
            epochs: self.request(),
            cause: UnconfirmedCause::Timeout { hart: 1 },
        }
    }

    /// A run of `pages` freshly allocated frames, `Mapped`.
    fn run(&mut self, pages: usize, alias: KernelAlias) -> FrameRun {
        let base = self
            .allocator
            .allocate_contiguous(pages)
            .expect("test frames");
        FrameRun::mapped(base, pages, OWNER, RunKind::Leaf, alias)
    }

    fn used(&self) -> usize {
        self.allocator.used_frames()
    }
}

impl Backend for Fake {
    fn completed(&self, hart: usize, epoch: usize) -> bool {
        epoch == 0 || self.completed[hart] >= epoch
    }

    fn reissue(&mut self, asid: Option<usize>) -> Epochs {
        self.reissues += 1;
        self.reissued_tags.push(asid);
        self.request()
    }

    fn normalise(&mut self, run: &FrameRun) {
        self.normalised.push(run.base());
    }

    fn free(&mut self, base: PhysAddr, pages: usize, hold: Option<VaHold>) {
        for page in 0..pages {
            self.allocator.deallocate_frame(base + page * PAGE_SIZE);
        }
        if let Some(hold) = hold {
            (hold.release)(hold.start, hold.end);
        }
    }
}

fn pending() -> Pending {
    Pending::new(Rank::UNRANKED)
}

std::thread_local!(static RELEASED_VA: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) });

fn record_va(start: usize, end: usize) {
    RELEASED_VA.with(|released| released.borrow_mut().push((start, end)));
}

fn released_va() -> Vec<(usize, usize)> {
    RELEASED_VA.with(|released| released.borrow().clone())
}

#[test]
fn mapped_unpublished_confirmed_is_freed_once() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut run = fake.run(3, KernelAlias::Intact);
    assert_eq!(run.state(), RunState::Mapped);
    assert_eq!(fake.used(), 3);

    run.unpublish().unwrap();
    assert_eq!(run.state(), RunState::Unpublished);
    assert_eq!(
        settle(run, FlushOutcome::Confirmed, &mut fake, &quarantine).map_err(|_| ()),
        Ok(3)
    );
    assert_eq!(fake.used(), 0, "every frame returned");
    assert!(
        fake.normalised.is_empty(),
        "an intact alias is not restored"
    );
    assert_eq!(quarantine.quarantined_pages(), 0);
}

#[test]
fn removed_or_user_alias_is_normalised_before_free() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut user = fake.run(1, KernelAlias::User);
    user.unpublish().unwrap();
    assert_eq!(
        user.alias(),
        KernelAlias::Removed,
        "unpublish removes the USER leaf"
    );
    let user_base = user.base();
    let mut removed = fake.run(2, KernelAlias::Removed);
    removed.unpublish().unwrap();
    let removed_base = removed.base();

    assert!(settle(user, FlushOutcome::Confirmed, &mut fake, &quarantine).is_ok());
    assert!(settle(removed, FlushOutcome::Confirmed, &mut fake, &quarantine).is_ok());
    assert_eq!(fake.normalised, [user_base, removed_base]);
    assert_eq!(fake.used(), 0);
}

#[test]
fn unconfirmed_flush_quarantines_and_never_frees() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut run = fake.run(4, KernelAlias::Intact);
    run.unpublish().unwrap();
    let outcome = fake.unconfirmed();

    assert_eq!(
        settle(run, outcome, &mut fake, &quarantine).map_err(|_| ()),
        Ok(0)
    );
    assert_eq!(fake.used(), 4, "nothing freed");
    assert_eq!(quarantine.quarantined_pages(), 4);
    let shared = Reason::SharedUnconfirmed.index();
    assert_eq!(quarantine.counters().quarantined_for(shared), 4);
    assert_eq!(quarantine.entries(), 1);

    // No hart answered: a reap frees nothing.
    assert_eq!(quarantine.reap(&mut fake), 0);
    assert_eq!(fake.used(), 4);
}

#[test]
fn quarantined_run_is_freed_once_every_epoch_is_published() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let hold = VaHold {
        release: record_va,
        start: 0x10_0000,
        end: 0x20_0000,
    };
    let mut run = fake.run(2, KernelAlias::Removed).with_va_hold(hold);
    run.unpublish().unwrap();
    let outcome = fake.unconfirmed();
    settle(run, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();

    // One of two harts answers: still held, VA still reserved.
    fake.answer(1);
    assert_eq!(quarantine.reap(&mut fake), 0);
    assert!(released_va().is_empty(), "VA kept until confirmed");
    assert!(fake.normalised.is_empty());

    fake.answer(2);
    assert_eq!(quarantine.reap(&mut fake), 2);
    assert_eq!(fake.used(), 0);
    assert_eq!(fake.normalised.len(), 1, "normalised before free");
    assert_eq!(released_va(), [(0x10_0000, 0x20_0000)]);
    assert_eq!(quarantine.quarantined_pages(), 0);
    assert_eq!(quarantine.entries(), 0);
    assert_eq!(quarantine.counters().released_after_retry(), 2);
}

#[test]
fn reaper_steps_every_entry_not_only_the_head() {
    let mut fake = Fake::new(16);
    let quarantine = pending();

    // Head: a tag entry for asid 9 on hart 1, which never answers.
    fake.online = vec![1];
    let mut tag_run = fake.run(1, KernelAlias::Intact);
    tag_run.unpublish().unwrap();
    tag_run
        .quarantine(Reason::TagUnconfirmed { asid: 9 })
        .unwrap();
    quarantine.push(tag_run, &fake.request());

    // Second: a shared entry on hart 2, which answers.
    fake.online = vec![2];
    let mut shared = fake.run(2, KernelAlias::Intact);
    shared.unpublish().unwrap();
    let outcome = fake.unconfirmed();
    settle(shared, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();
    fake.answer(2);

    assert_eq!(quarantine.entries(), 2);
    assert!(quarantine.tag_pending(9));
    assert_eq!(
        quarantine.reap(&mut fake),
        2,
        "the second entry is stepped too"
    );
    assert_eq!(fake.used(), 1, "the silent head keeps its frame");
    assert!(quarantine.tag_pending(9));
    assert!(!quarantine.tag_pending(8));
}

#[test]
fn reaper_reissues_every_32_attempts_and_waits_for_the_new_epoch() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    fake.online = vec![1];
    let mut run = fake.run(1, KernelAlias::Intact);
    run.unpublish().unwrap();
    run.quarantine(Reason::TagUnconfirmed { asid: 4 }).unwrap();
    quarantine.push(run, &fake.request());

    for _ in 0..REISSUE_EVERY - 1 {
        assert_eq!(quarantine.reap(&mut fake), 0);
    }
    assert_eq!(fake.reissues, 0);
    // The 32nd attempt reissues; the hart then publishes only the *first* epoch.
    let first = fake.requested[1];
    assert_eq!(quarantine.reap(&mut fake), 0);
    assert_eq!(fake.reissues, 1);
    assert_eq!(
        fake.reissued_tags,
        [Some(4)],
        "the tag is flushed locally again"
    );
    fake.completed[1] = first;
    assert_eq!(
        quarantine.reap(&mut fake),
        0,
        "the reissued epoch is awaited"
    );
    fake.answer(1);
    assert_eq!(quarantine.reap(&mut fake), 1);
}

#[test]
fn reaper_never_abandons_an_entry() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut run = fake.run(1, KernelAlias::Intact);
    run.unpublish().unwrap();
    let outcome = fake.unconfirmed();
    settle(run, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();

    for _ in 0..ESCALATE_AFTER + 2 * REISSUE_EVERY {
        assert_eq!(quarantine.reap(&mut fake), 0);
    }
    assert_eq!(
        quarantine.quarantined_pages(),
        1,
        "still held, never dropped"
    );
    fake.answer(1);
    fake.answer(2);
    assert_eq!(quarantine.reap(&mut fake), 1);
    assert_eq!(fake.used(), 0);
}

#[test]
fn shared_entries_merge_and_wait_for_the_newest_epoch() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    fake.online = vec![1];
    let mut first = fake.run(1, KernelAlias::Intact);
    first.unpublish().unwrap();
    let outcome = fake.unconfirmed();
    settle(first, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();
    let first_epoch = fake.requested[1];

    let mut second = fake.run(2, KernelAlias::Intact);
    second.unpublish().unwrap();
    let outcome = fake.unconfirmed();
    settle(second, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();

    assert_eq!(quarantine.entries(), 1, "one entry per wait");
    assert_eq!(quarantine.quarantined_pages(), 3);
    fake.completed[1] = first_epoch;
    assert_eq!(
        quarantine.reap(&mut fake),
        0,
        "merged epochs only wait longer"
    );
    fake.answer(1);
    assert_eq!(quarantine.reap(&mut fake), 3);
    assert_eq!(fake.used(), 0);
}

#[test]
fn rfence_failure_is_its_own_reason() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut run = fake.run(1, KernelAlias::Removed);
    run.unpublish().unwrap();
    let outcome = FlushOutcome::Unconfirmed {
        epochs: fake.request(),
        cause: UnconfirmedCause::RfenceFailed { hart: 1 },
    };
    settle(run, outcome, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();
    let rfence = Reason::RfenceFailed.index();
    assert_eq!(quarantine.counters().quarantined_for(rfence), 1);
    fake.answer(1);
    fake.answer(2);
    assert_eq!(quarantine.reap(&mut fake), 1);
}

#[test]
fn event_hold_keeps_the_mapping_until_its_event() {
    let mut fake = Fake::new(16);
    let quarantine = pending();
    let mut pinned = fake.run(2, KernelAlias::User);
    pinned
        .hold(Reason::Pinned(crate::memory::pin::ReleaseKey::Owner(5)))
        .unwrap();
    let mut export = fake.run(1, KernelAlias::Intact);
    export.hold(Reason::IotlbUnacked { export_id: 11 }).unwrap();
    quarantine.push(pinned, &[0; MAX_HARTS]);
    quarantine.push(export, &[0; MAX_HARTS]);
    assert_eq!(quarantine.quarantined_pages(), 3);

    // Epochs never end an event hold.
    fake.answer(1);
    fake.answer(2);
    assert_eq!(quarantine.reap(&mut fake), 0);
    assert_eq!(fake.used(), 3);

    let released =
        quarantine.release_event(|reason| *reason == Reason::IotlbUnacked { export_id: 11 });
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].state(), RunState::Mapped);
    assert_eq!(quarantine.quarantined_pages(), 2);

    // The released run resumes the normal path: unpublish, flush, free.
    for mut run in released {
        run.unpublish().unwrap();
        settle(run, FlushOutcome::Confirmed, &mut fake, &quarantine)
            .map_err(|_| ())
            .unwrap();
    }
    let mut pinned = quarantine.release_event(|reason| matches!(reason, Reason::Pinned(_)));
    let mut run = pinned.pop().unwrap();
    assert_eq!(run.alias(), KernelAlias::User, "still published");
    run.unpublish().unwrap();
    settle(run, FlushOutcome::Confirmed, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();
    assert_eq!(fake.used(), 0);
    assert_eq!(quarantine.quarantined_pages(), 0);
}

#[test]
fn invalid_transitions_are_refused_and_keep_the_frames() {
    let mut fake = Fake::new(16);
    let quarantine = pending();

    // A mapped run cannot be settled: a flush cannot cover a live mapping.
    let run = fake.run(1, KernelAlias::Intact);
    let mut run = settle(run, FlushOutcome::Confirmed, &mut fake, &quarantine)
        .expect_err("settle needs an unpublished run");
    assert_eq!(fake.used(), 1);

    // An invalidation reason is not an event hold.
    assert!(run.hold(Reason::SharedUnconfirmed).is_err());
    assert_eq!(run.state(), RunState::Mapped);

    run.unpublish().unwrap();
    assert_eq!(
        run.unpublish(),
        Err(StateError {
            state: RunState::Unpublished
        })
    );
    // An unpublished run cannot take an event hold, nor an event reason as
    // its quarantine.
    assert!(run.hold(Reason::VmLive).is_err());
    assert!(run.quarantine(Reason::DomainRevoking).is_err());
    assert!(run.end_hold().is_err());

    settle(run, FlushOutcome::Confirmed, &mut fake, &quarantine)
        .map_err(|_| ())
        .unwrap();
    assert_eq!(fake.used(), 0);

    // Only quarantined runs enter the pending list.
    let mut stray = fake.run(1, KernelAlias::Intact);
    stray.unpublish().unwrap();
    quarantine.push(stray, &[0; MAX_HARTS]);
    assert_eq!(quarantine.entries(), 0);
    assert_eq!(quarantine.counters().leaked(), 1, "kept, counted as leaked");
    assert_eq!(fake.used(), 1, "never freed");
}

#[test]
fn dropping_an_unreleased_run_leaks_instead_of_freeing() {
    let mut fake = Fake::new(16);
    let run = fake.run(2, KernelAlias::Intact);
    let leaked = PENDING.counters().leaked();
    drop(run);
    assert!(PENDING.counters().leaked() >= leaked + 2);
    assert_eq!(fake.used(), 2, "the allocator still owns them");
}

#[test]
fn outcomes_combine_to_the_strictest() {
    let mut fake = Fake::new(1);
    let a = fake.unconfirmed();
    let b = fake.unconfirmed();
    assert_eq!(
        FlushOutcome::Confirmed.and(FlushOutcome::Confirmed),
        FlushOutcome::Confirmed
    );
    assert_eq!(FlushOutcome::Confirmed.and(a), a);
    assert_eq!(a.and(FlushOutcome::Confirmed), a);
    let FlushOutcome::Unconfirmed { epochs, .. } = a.and(b) else {
        panic!("two unconfirmed flushes stay unconfirmed");
    };
    assert_eq!(epochs[1], 2);
    assert_eq!(epochs[2], 2);
    assert_eq!(FlushOutcome::Confirmed.reason(), None);
    assert_eq!(a.reason(), Some(Reason::SharedUnconfirmed));
}

#[test]
fn cap_is_one_sixty_fourth_of_managed_frames() {
    assert!(!cap_exceeded(0, 0));
    assert!(!cap_exceeded(16, 1024));
    assert!(cap_exceeded(17, 1024));
    assert!(!cap_exceeded(1, 64));
    assert!(cap_exceeded(2, 64));
    assert!(!cap_exceeded(usize::MAX / 128, usize::MAX), "no overflow");

    let quarantine = pending();
    quarantine.note_refused(1024);
    quarantine.note_refused(1024);
    assert_eq!(quarantine.counters().refused(), 2);
}
