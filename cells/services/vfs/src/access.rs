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
mod machine_pin;
mod rules;
#[cfg(feature = "test-hooks")]
pub(crate) mod selftest;
#[path = "access/srv-owner.rs"]
mod srv_owner;
#[cfg(feature = "test-hooks")]
pub(crate) mod stub;
#[cfg(test)]
mod tests;

use crate::caller::Caller;
use guest_disk::live_hypervisor_matches;
pub(crate) use guest_disk::{contains_guest_disk, is_guest_disk_path, GUEST_DISK_PATH};
use kms::{contains_kms_namespace, is_canonical_policy_path, is_kms_store_rule, live_kms_matches};
use machine_pin::{
    contains_machine_pin_namespace, is_machine_pin_store_rule, live_machine_pin_matches,
};
pub use rules::PathRule;
use srv_owner::{
    contains_srv_owner_namespace, is_srv_owner_store_rule, srv_owner_matches, ImageLookup,
};

/// Path rules, evaluated whole-path first and by prefix second.
pub struct AccessTable {
    exact: &'static [PathRule],
    prefixes: &'static [PathRule],
    service_lookup: fn(u16) -> Option<usize>,
    image_lookup: ImageLookup,
}

impl AccessTable {
    /// Initialize with the shipped rule tables.
    pub fn new() -> Self {
        Self::with_service_lookup(default_service_lookup)
    }

    pub(crate) fn with_service_lookup(service_lookup: fn(u16) -> Option<usize>) -> Self {
        Self::with_lookups(service_lookup, default_image_lookup)
    }

    /// Both kernel queries replaceable, so a host test can stand in for the
    /// service registry and for `ResolveCellImage`.
    pub(crate) fn with_lookups(
        service_lookup: fn(u16) -> Option<usize>,
        image_lookup: ImageLookup,
    ) -> Self {
        Self {
            exact: rules::EXACT_RULES,
            prefixes: rules::PREFIX_RULES,
            service_lookup,
            image_lookup,
        }
    }

    /// A table with replacement rule data, so a test can change policy under
    /// a live handle.
    #[cfg(test)]
    pub(crate) fn with_rules(exact: &'static [PathRule], prefixes: &'static [PathRule]) -> Self {
        Self {
            exact,
            prefixes,
            service_lookup: default_service_lookup,
            image_lookup: default_image_lookup,
        }
    }

    /// Whether `caller` may write to `path`.
    ///
    /// `/srv/cellos/kms` and `/srv/.cellos-machine` are narrower than the broad
    /// `/srv/` prefix: only the live KMS, respectively `MACHINE_PIN`, service
    /// instance may touch them. The mutate flag is required everywhere, the
    /// pin store included: the `machine-pin` Cell declares `VfsMutate` and
    /// holds W on its boot edge (Phase 13), so no caller is exempt.
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

    /// Whether `caller` may recursively remove `path` and its descendants.
    pub fn can_remove_tree(&self, caller: Caller, path: &str) -> bool {
        caller.may_mutate()
            && is_canonical_policy_path(path)
            && self.can_write(caller, path)
            && !contains_provider_namespace(path)
            && !contains_guest_disk(path)
    }

    /// Whether `caller` may remove `path` as a directory.
    pub fn can_remove_dir(&self, caller: Caller, path: &str) -> bool {
        caller.may_mutate()
            && is_canonical_policy_path(path)
            && self.can_write(caller, path)
            && !contains_provider_namespace(path)
            && !contains_guest_disk(path)
    }

    fn decide(&self, caller: Caller, path: &str, kind: AccessKind) -> bool {
        // Runtime boot configuration is operator input, not a writable mailbox.
        // FAT names are case-insensitive; protect both the subtree and ancestors
        // against replacement, rename and recursive removal by ordinary cells.
        if matches!(kind, AccessKind::Write) && touches_boot_configuration(path) {
            return false;
        }
        if !is_canonical_policy_path(path) {
            return false;
        }
        // Unpublished save stages belong to the transaction that made them and
        // are reached only through `save`, which goes around this table. A
        // path-addressed read, write or delete there would read or tamper with
        // another cell's pending save.
        if touches_save_staging(path) {
            return false;
        }
        // The guest's whole disk is the guest's data: only the live hypervisor
        // reads or writes it.
        if is_guest_disk_path(path) {
            return self.is_live_hypervisor(caller);
        }
        match self.rule_for(path) {
            Some(rule) if is_kms_store_rule(rule.prefix) => {
                live_kms_matches(caller, self.service_lookup)
            }
            Some(rule) if is_machine_pin_store_rule(rule.prefix) => {
                live_machine_pin_matches(caller, self.service_lookup)
            }
            Some(rule) if is_srv_owner_store_rule(rule.prefix) => {
                srv_owner_matches(caller, self.image_lookup)
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

/// Whether removing `path` would remove a provider-held namespace: `path` is
/// inside one or is an ancestor of one. No caller, the provider included, may.
fn contains_provider_namespace(path: &str) -> bool {
    contains_kms_namespace(path)
        || contains_machine_pin_namespace(path)
        || contains_srv_owner_namespace(path)
}

/// Whether any component of `path` is a backend's save-staging directory.
/// ASCII case is ignored so a FAT alias of the name cannot slip past.
fn touches_save_staging(path: &str) -> bool {
    let staging = crate::backend_cellosfs::STAGING_DIR.trim_start_matches('/');
    path.split('/')
        .any(|component| component.eq_ignore_ascii_case(staging))
}

fn touches_boot_configuration(path: &str) -> bool {
    const ROOT: &str = "/mnt/sd/etc/cellos";
    if path == "/" {
        return true;
    }
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.len() <= ROOT.len() {
        ROOT.get(..path.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(path))
            && (path.len() == ROOT.len() || ROOT.as_bytes()[path.len()] == b'/')
    } else {
        path.get(..ROOT.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(ROOT))
            && path.as_bytes()[ROOT.len()] == b'/'
    }
}

#[derive(Clone, Copy)]
enum AccessKind {
    Read,
    Write,
}

#[cfg(target_os = "none")]
fn default_service_lookup(service_id: u16) -> Option<usize> {
    ostd::service::provider_tid(service_id)
}

#[cfg(not(target_os = "none"))]
fn default_service_lookup(_service_id: u16) -> Option<usize> {
    None
}

#[cfg(target_os = "none")]
fn default_image_lookup(cell: u64, generation: u64) -> Option<abi::cell_image::CellImage> {
    ostd::syscall::sys_resolve_cell_image(cell, generation)
}

#[cfg(not(target_os = "none"))]
fn default_image_lookup(_cell: u64, _generation: u64) -> Option<abi::cell_image::CellImage> {
    None
}
