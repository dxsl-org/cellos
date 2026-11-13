//! `/srv/sshd/**`: the inbound sshd's host-key seed and `authorized_keys`.
//!
//! Only a Cell whose **kernel-measured image** is a signature-verified image
//! named `sshd` may read, stat, list, create, write, rename or unlink anything
//! here; every other Cell is refused, and no caller may remove the namespace
//! or an ancestor of it.
//!
//! Why the image and not a service id or a path: a well-known service id is
//! bound by whoever registers it, and the path a Cell was spawned from is
//! chosen by its spawner, so neither says which *program* is calling. The
//! kernel measures the image at admission and reports, per live `(cell,
//! generation)`, the anchor that verified its signature and the name in its
//! signed `__ViCell_image` section (`abi::cell_image`). An unsigned image
//! carries no name, so it never matches, whatever path it came from.
//!
//! Anchors: the release anchor always qualifies; the dev anchor qualifies
//! because only a dev-anchor kernel can report it (`kernel/src/signing.rs`
//! refuses to build a release kernel that would). Under the dev posture the
//! rule is spoofable: the dev seed is public, so any Cell that can spawn from
//! bytes can sign an image named `sshd`. The private profile anchor is not
//! admitted.
//!
//! The lookup is always for the exact `(cell, generation)` of the request; a
//! respawned Cell under the same `CellId` is asked about afresh, and a stale
//! generation resolves to nothing. Nothing is cached. The fast path cannot make
//! the syscall (it runs in the caller's context), so it refuses outright.
//! A raw-block holder or an offline attacker reads the volume directly; this
//! rule stops in-system Cells only.

use crate::caller::Caller;
use abi::cell_image::{CellImage, ANCHOR_DEV, ANCHOR_RELEASE};

const NAMESPACE: &str = "/srv/sshd";
const PREFIX: &str = "/srv/sshd/";
/// The name `scripts/sign-cell.py` emits for the `service-sshd` artifact.
const PROGRAM: &str = "sshd";

/// Kernel query for the image identity of a `(cell, generation)`.
pub(crate) type ImageLookup = fn(u64, u64) -> Option<CellImage>;

/// Whether `prefix` names one of the two rule rows that guard the namespace.
pub(super) fn is_srv_owner_store_rule(prefix: &str) -> bool {
    matches!(prefix, NAMESPACE | PREFIX)
}

/// Whether `path` is the namespace root or lies below it.
pub(super) fn is_srv_owner_namespace_path(path: &str) -> bool {
    path == NAMESPACE || path.starts_with(PREFIX)
}

/// Whether removing `path` would remove the namespace: `path` is inside it or
/// is one of its ancestors.
pub(super) fn contains_srv_owner_namespace(path: &str) -> bool {
    if is_srv_owner_namespace_path(path) {
        return true;
    }
    let ancestor = path.strip_suffix('/').unwrap_or(path);
    NAMESPACE
        .strip_prefix(ancestor)
        .is_some_and(|remainder| remainder.starts_with('/'))
}

/// Whether the kernel vouches that `caller`'s exact generation runs a signed
/// `sshd` image under an admitted anchor. The record must echo the principal
/// asked about.
pub(super) fn srv_owner_matches(caller: Caller, lookup: ImageLookup) -> bool {
    if caller.cell.0 == 0 || caller.generation == 0 {
        return false;
    }
    lookup(caller.cell.0, caller.generation)
        .filter(|image| image.cell_id == caller.cell.0 && image.generation == caller.generation)
        .is_some_and(|image| {
            matches!(image.identity.anchor(), ANCHOR_RELEASE | ANCHOR_DEV)
                && image.identity.is_signed_program(PROGRAM)
        })
}

#[cfg(test)]
#[path = "srv-owner-dispatch-tests.rs"]
mod dispatch_tests;
#[cfg(test)]
#[path = "srv-owner-tests.rs"]
mod tests;
