//! Path authorization for the VFS service.
//!
//! Path-prefix rules rather than POSIX mode bits: in a Single Address Space OS a
//! cell is the only principal, so there is no uid/gid to carry. The rule data and
//! the reasoning behind its shape live in [`rules`].
//!
//! Deny by default. A path matching no rule is refused, and so is a caller whose
//! identity the kernel did not vouch for — that check happens at the request
//! boundary in `dispatch`, before a path is ever looked at, because a caller with
//! no identity has no business reaching a policy table at all.
//!
//! These rules are VFS-internal and deliberately NOT driven by the cell manifest
//! the kernel reads at spawn (`__ViCell_manifest`, source of `BlockIoCap` /
//! `NetworkCap` / `SpawnCap`). Per-cell VFS path grants are a separate concern.

mod guest_disk;
#[cfg(test)]
mod guest_disk_tests;
mod kms;
mod rules;
#[cfg(feature = "test-hooks")]
pub(crate) mod selftest;
#[cfg(feature = "test-hooks")]
pub(crate) mod stub;
#[cfg(test)]
mod tests;

use crate::caller::Caller;
use guest_disk::live_hypervisor_matches;
pub(crate) use guest_disk::{contains_guest_disk, is_guest_disk_path, GUEST_DISK_PATH};
use kms::{
    contains_kms_namespace, is_canonical_policy_path, is_kms_namespace_path, is_kms_store_rule,
    live_kms_matches,
};
pub use rules::PathRule;

/// Path rules, evaluated whole-path first and by prefix second.
pub struct AccessTable {
    exact: &'static [PathRule],
    prefixes: &'static [PathRule],
    service_lookup: fn(u16) -> Option<usize>,
}

impl AccessTable {
    /// Initialize with the shipped rule tables.
    pub fn new() -> Self {
        Self::with_service_lookup(default_service_lookup)
    }

    pub(crate) fn with_service_lookup(service_lookup: fn(u16) -> Option<usize>) -> Self {
        Self {
            exact: rules::EXACT_RULES,
            prefixes: rules::PREFIX_RULES,
            service_lookup,
        }
    }

    /// Whether `caller` may write to `path`.
    ///
    /// `/srv/cellos/kms` is narrower than the broad `/srv/` prefix: only the
    /// live KMS service instance may touch it.
    pub fn can_write(&self, caller: Caller, path: &str) -> bool {
        caller.may_mutate() && self.decide(caller, path, AccessKind::Write)
    }

    /// Whether `caller` may write to the guest system disk image.
    ///
    /// Restricted to the live registered hypervisor provider; other cells cannot
    /// touch the VM disk backend directly.
    pub fn is_live_hypervisor(&self, caller: Caller) -> bool {
        live_hypervisor_matches(caller, self.service_lookup)
    }

    /// Whether `caller` may read `path`.
    pub fn can_read(&self, caller: Caller, path: &str) -> bool {
        self.decide(caller, path, AccessKind::Read)
    }

    /// Interrupt-disabled fast path: never performs service lookup.
    ///
    /// The protected KMS prefix is denied here so the ecall path remains the
    /// only place that can prove "live service::KMS" with a normal lookup.
    pub fn can_read_fast(&self, caller: Caller, path: &str) -> bool {
        if !is_canonical_policy_path(path) || is_kms_namespace_path(path) {
            return false;
        }
        self.decide(caller, path, AccessKind::Read)
    }

    /// Whether `caller` may recursively remove `path` and its descendants.
    pub fn can_remove_tree(&self, caller: Caller, path: &str) -> bool {
        caller.may_mutate()
            && is_canonical_policy_path(path)
            && self.can_write(caller, path)
            && !contains_kms_namespace(path)
            && !contains_guest_disk(path)
    }

    /// Whether `caller` may remove `path` as a directory.
    pub fn can_remove_dir(&self, caller: Caller, path: &str) -> bool {
        caller.may_mutate()
            && is_canonical_policy_path(path)
            && self.can_write(caller, path)
            && !contains_kms_namespace(path)
            && !contains_guest_disk(path)
    }

    fn decide(&self, caller: Caller, path: &str, kind: AccessKind) -> bool {
        if !is_canonical_policy_path(path) {
            return false;
        }
        if is_guest_disk_path(path) {
            return match kind {
                AccessKind::Read => true,
                AccessKind::Write => self.is_live_hypervisor(caller),
            };
        }
        match self.rule_for(path) {
            Some(rule) if is_kms_store_rule(rule.prefix) => {
                live_kms_matches(caller, self.service_lookup)
            }
            Some(rule) => match kind {
                AccessKind::Read => rule.allow_read_all,
                AccessKind::Write => rule.allow_write_all,
            },
            None => false,
        }
    }

    fn rule_for(&self, path: &str) -> Option<&'static PathRule> {
        self.exact
            .iter()
            .find(|rule| rule.prefix == path)
            .or_else(|| {
                self.prefixes
                    .iter()
                    .find(|rule| path.starts_with(rule.prefix))
            })
    }
}

impl Default for AccessTable {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum AccessKind {
    Read,
    Write,
}

#[cfg(target_os = "none")]
fn default_service_lookup(service_id: u16) -> Option<usize> {
    ostd::syscall::sys_lookup_service(service_id)
}

#[cfg(not(target_os = "none"))]
fn default_service_lookup(_service_id: u16) -> Option<usize> {
    None
}
