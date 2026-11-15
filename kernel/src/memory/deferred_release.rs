//! The private-root half of the release contract: frames a private root no
//! longer maps, retained until that root's tag invalidation is confirmed.
//!
//! A release path must never wait synchronously for a remote acknowledgement:
//! a peer hart can be held non-preemptible for seconds. A path whose bounded
//! probe did not confirm its tag hands the frames (and, for a root teardown,
//! the tag's pool slot) to [`defer_frames`] / [`defer_frames_and_tag`]. They
//! become `Quarantined(TagUnconfirmed)` runs in the one frame-run quarantine
//! (`memory::frame_run::PENDING`), whose hart-0 reaper frees them once every
//! remote hart has published an epoch asked for after the unmap.
//!
//! Fail-closed, in this order:
//!
//! 1. a frame is released **only** after the invalidation that covers it is
//!    acknowledged — never on a timeout, never silently;
//! 2. nothing is abandoned: a hart that never answers keeps the frames
//!    quarantined (counted, and logged once on escalation) and the tag reads
//!    unconfirmed for as long as it does (census §7.5);
//! 3. a wait that cannot be recorded (no heap) leaks its frames, counted, and
//!    makes every tag read unconfirmed for the rest of the boot.
//!
//! [`tag_invalidation_unconfirmed`] is the one answer every dependent path asks
//! (a grant drain, a re-publish over a retired VA): `false` is the only proof
//! that no hart can still resolve a stale translation under the tag.

use super::frame::OwnedFrame;
use super::frame_run::{FlushOutcome, KernelBackend, RunKind, RunOwner, TagWait, PENDING};
use alloc::vec::Vec;

/// Retain `frames` until `asid`'s invalidation is confirmed.
///
/// The frames must already be detached from every structure that could hand
/// them back to the allocator: this is the last owner. The tag stays live (the
/// caller keeps its lease), so nothing can reuse it while the wait is pending.
pub fn defer_frames(asid: usize, domain: u64, frames: Vec<OwnedFrame>, reason: &'static str) {
    defer(asid, domain, frames, reason, false)
}

/// Retain `frames` **and** the tag's pool slot until the invalidation is
/// confirmed. Used by a root teardown, whose `AsidLease::release` kept its
/// slot reserved: the slot is reissued only when the same wait completes.
pub fn defer_frames_and_tag(
    asid: usize,
    domain: u64,
    frames: Vec<OwnedFrame>,
    reason: &'static str,
) {
    defer(asid, domain, frames, reason, true)
}

fn defer(
    asid: usize,
    domain: u64,
    frames: Vec<OwnedFrame>,
    reason: &'static str,
    release_tag_slot: bool,
) {
    #[cfg(feature = "p01-trace")]
    let frames = {
        let owner = crate::p01_resources::owner_for_domain(domain);
        if owner.is_none()
            && crate::memory::heap::p01_owner::current_attempt().is_some()
            && !frames.is_empty()
        {
            // A selected prepublication retention has no exact domain binding.
            crate::p01_retained::fault();
        }
        if let Some(owner) = owner {
            // One wait per tag holds every retention behind it (stacks
            // included), so no single retention's completion is observable.
            crate::p01_resources::unknown(owner, 6);
        }
        let mut frames = frames;
        for frame in &mut frames {
            frame.p01_retain(owner, reason);
        }
        frames
    };
    let count = frames.len();
    let mut runs = Vec::new();
    if runs.try_reserve_exact(count).is_err() {
        // No heap to carry the frames: never free them, and fail closed on
        // every tag, as for any wait the quarantine cannot record.
        for frame in frames {
            core::mem::forget(frame);
        }
        PENDING.lose_frames(count, "no heap for the tag wait");
        return;
    }
    runs.extend(
        frames
            .into_iter()
            .map(|frame| frame.into_run(RunOwner::Kernel, RunKind::PageTable)),
    );
    // Every online remote is asked again after the unmap: each serves its
    // epoch with a full flush, which drops this tag's entries too.
    let epochs = super::frame_run::tag_epochs(asid, FlushOutcome::Confirmed, &mut KernelBackend);
    // The decision and the log are separated so the console never nests inside
    // the quarantine lock.
    match PENDING.defer_tag(asid, runs, release_tag_slot.then_some(domain), &epochs) {
        TagWait::Queued => log::warn!(
            "[aspace] deferred release queued: tag={} domain={} frames={} reason={} — the tag stays unconfirmed and every frame behind it is retained until an acknowledgement lands",
            asid,
            domain,
            count,
            reason
        ),
        TagWait::Extended { pages } => log::warn!(
            "[aspace] deferred release extended: tag={} total_frames={} reason={}",
            asid,
            pages,
            reason
        ),
        TagWait::Lost => {}
    }
}

/// Is `asid`'s invalidation still unconfirmed?
///
/// True while a tag wait is pending in the frame-run quarantine, and for every
/// tag once a tag wait could not be recorded. Every path that gates a reuse of
/// the address space on a confirmed invalidation must ask this instead of
/// waiting: a `false` answer is the only proof that no hart can still resolve
/// a stale translation, and a missing acknowledgement must never look like one.
pub fn tag_invalidation_unconfirmed(asid: usize) -> bool {
    PENDING.tag_unconfirmed(asid, &KernelBackend)
}

/// Test-hooks view: frames retained behind tag `asid`.
///
/// A deferred release withholds its frames from the allocator, and no
/// allocator-side count can see them. A fixture that accounts a deferred revoke
/// by its grant backing alone (the `grant-gate` frames property) therefore
/// needs this to tell a correct deferral from a leak.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(any(cellos_domain_grants, target_arch = "riscv64")),
    allow(dead_code) // reason: the grant fixtures and the RV64 aspace fixture assert on it
)]
pub(crate) fn deferred_release_frames_for(asid: usize) -> usize {
    PENDING.tag_pages(asid)
}

/// Test-hooks view of every tag wait: `(tag, frames, attempts)`, so a fixture
/// that does not add up can name what is holding a frame.
#[cfg(feature = "test-hooks")]
#[cfg_attr(
    not(any(cellos_domain_grants, target_arch = "riscv64")),
    allow(dead_code) // reason: the grant fixtures and the RV64 aspace fixture read it
)]
pub(crate) fn deferred_release_snapshot() -> Vec<(usize, usize, usize)> {
    PENDING.tag_snapshot()
}
