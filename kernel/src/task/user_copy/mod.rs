//! Recoverable domain-aware user copy boundary (Spec 22, phase 03).
//!
//! This is the ONLY module allowed to touch user bytes. Every syscall-facing
//! dereference goes through [`copy_from_user`] / [`copy_to_user`], which run a
//! uniform two-pass transaction:
//!
//! 1. PROBE PASS (non-mutating): arithmetic range validation (null / overflow /
//!    non-canonical / kernel-half), then a per-page permission check — private
//!    roots consult their mapping ledger AND the live Sv39 PTEs; the shared
//!    address space walks the currently active root. No bytes move, so any
//!    failure here leaves both destinations byte-identical.
//! 2. COMMIT PASS: the byte copy runs inside a per-hart recoverable fault
//!    guard. For `CopyView::Domain` the CopyReader pin is held across the
//!    whole commit and every page's PTE is re-walked under the pin before any
//!    byte moves; because `unmap_private_page`/`unmap_grant_page` drain copy
//!    readers BEFORE tearing down any PTE, an in-window fault is unreachable
//!    through the public API and remains a recoverable `CopyError` (never a
//!    panic). For `CopyView::Sas` the commit copies through user VAs under the
//!    active shared root, retaining today's SUM-based shared-space semantics;
//!    the guard converts a racing revocation into the same recoverable error.
//!
//! The guard itself lives in `hart_local` (`user_copy_guard_*`). The RV64 trap
//! handler routes guard-owned page faults to the helper's error label by
//! rewinding `sepc`; everything else keeps its existing fatal path. No
//! scheduler lock, no allocation, and no callbacks run inside the guarded
//! window, and the window masks interrupts so a context switch can never carry
//! a stale guard into another task (`clear_guard_for_context_switch` is the
//! defense-in-depth backstop for that invariant).
//!
//! Layout: [`range`] owns the validated slice/view vocabulary,
//! [`sv39_probe`] owns ledger + live-PTE validation, [`copy`] orchestrates
//! the two-pass transaction, and [`guard`] arms the per-hart recoverable
//! fault window around every byte movement.

mod copy;
mod guard;
mod range;
mod scatter;
mod sv39_probe;

pub(crate) use copy::{copy_from_user, copy_to_user, probe_writable};
pub(crate) use guard::clear_guard_for_context_switch;
#[cfg(feature = "test-hooks")]
pub(crate) use guard::forced_guard_fault_recovers_for_test;
#[cfg(feature = "test-hooks")]
pub(crate) use range::CopyError;
pub(crate) use range::{CopyView, UserReadSlice, UserWriteSlice};
pub(crate) use scatter::copy_to_user_scatter;
#[cfg(feature = "test-hooks")]
pub(crate) use sv39_probe::stage_domain_for_test;
pub(crate) use sv39_probe::{current_satp_root, sv39_leaf};
