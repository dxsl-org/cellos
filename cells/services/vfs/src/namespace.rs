//! Service-local lease accounting for canonical paths on `/srv`.
#![allow(dead_code)]

#[cfg(not(test))]
use alloc::{collections::BTreeMap, sync::Arc};
#[cfg(test)]
use std::{collections::BTreeMap, sync::Arc};

use core::cmp::Ordering;
#[cfg(not(test))]
use ostd::prelude::Mutex;
#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
#[path = "namespace/tests.rs"]
mod tests;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NamespaceKey(Arc<str>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidNamespaceKey;

impl NamespaceKey {
    pub fn parse(path: &str) -> Result<Self, InvalidNamespaceKey> {
        if path == "/srv" { return Ok(Self(Arc::from(path))); }
        let tail = path.strip_prefix("/srv/").ok_or(InvalidNamespaceKey)?;
        if tail.is_empty() || path.as_bytes().contains(&0) || tail.split('/').any(|p| p.is_empty() || matches!(p, "." | "..")) {
            return Err(InvalidNamespaceKey);
        }
        Ok(Self(Arc::from(path)))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcquireError { Conflict, CountOverflow }

#[derive(Default)]
struct State { transient: u32, service_handle: u32, exclusive: bool }

impl State {
    fn clear(&self) -> bool { self.transient == 0 && self.service_handle == 0 && !self.exclusive }
}

type Entries = Arc<Mutex<BTreeMap<NamespaceKey, State>>>;
type Acquire<T> = Result<T, AcquireError>;

pub struct NamespaceLedger { entries: Entries }

impl NamespaceLedger {
    pub fn new() -> Self { Self { entries: Arc::new(Mutex::new(BTreeMap::new())) } }

    pub fn acquire_transient(&self, key: &NamespaceKey) -> Acquire<Transient> {
        self.acquire_shared(key, SharedKind::Transient).map(Transient)
    }

    pub fn acquire_service_handle(&self, key: &NamespaceKey) -> Acquire<ServiceHandle> {
        self.acquire_shared(key, SharedKind::ServiceHandle).map(ServiceHandle)
    }

    fn acquire_shared(&self, key: &NamespaceKey, kind: SharedKind) -> Acquire<SharedLease> {
        let owned_key = key.clone();
        #[cfg(not(test))] let mut entries = self.entries.lock();
        #[cfg(test)] let mut entries = self.entries.lock().unwrap();

        let state = entries.entry(owned_key.clone()).or_default();
        if state.exclusive { return Err(AcquireError::Conflict); }
        let count = match kind {
            SharedKind::Transient => &mut state.transient,
            SharedKind::ServiceHandle => &mut state.service_handle,
        };
        *count = count.checked_add(1).ok_or(AcquireError::CountOverflow)?;
        drop(entries);
        Ok(SharedLease { entries: Arc::clone(&self.entries), key: Some(owned_key), kind })
    }

    pub fn reserve_one(&self, key: &NamespaceKey) -> Acquire<ExclusiveReservation> {
        self.reserve_two(key, key)
    }

    pub fn reserve_two(&self, left: &NamespaceKey, right: &NamespaceKey) -> Acquire<ExclusiveReservation> {
        let (first, second) = match left.cmp(right) {
            Ordering::Less => (left.clone(), Some(right.clone())),
            Ordering::Equal => (left.clone(), None),
            Ordering::Greater => (right.clone(), Some(left.clone())),
        };
        #[cfg(not(test))] let mut entries = self.entries.lock();
        #[cfg(test)] let mut entries = self.entries.lock().unwrap();

        if entries.get(&first).is_some_and(|s| !s.clear())
            || second.as_ref().is_some_and(|k| entries.get(k).is_some_and(|s| !s.clear()))
        {
            return Err(AcquireError::Conflict);
        }
        entries.entry(first.clone()).or_default().exclusive = true;
        if let Some(key) = &second { entries.entry(key.clone()).or_default().exclusive = true; }
        drop(entries);
        Ok(ExclusiveReservation { entries: Arc::clone(&self.entries), first: Some(first), second })
    }
}

impl Default for NamespaceLedger {
    fn default() -> Self { Self::new() }
}

enum SharedKind { Transient, ServiceHandle }

struct SharedLease { entries: Entries, key: Option<NamespaceKey>, kind: SharedKind }
pub struct Transient(SharedLease);
pub struct ServiceHandle(SharedLease);

impl Drop for SharedLease {
    fn drop(&mut self) {
        let Some(key) = self.key.take() else { return };
        #[cfg(not(test))] let mut entries = self.entries.lock();
        #[cfg(test)] let mut entries = self.entries.lock().unwrap();

        let state = entries.get_mut(&key).expect("lease entry must exist during drop");
        let count = match self.kind {
            SharedKind::Transient => &mut state.transient,
            SharedKind::ServiceHandle => &mut state.service_handle,
        };
        *count = count.checked_sub(1).expect("lease count underflow");
        if state.clear() { entries.remove(&key); }
    }
}

pub struct ExclusiveReservation {
    entries: Entries,
    first: Option<NamespaceKey>,
    second: Option<NamespaceKey>,
}

impl Drop for ExclusiveReservation {
    fn drop(&mut self) {
        #[cfg(not(test))] let mut entries = self.entries.lock();
        #[cfg(test)] let mut entries = self.entries.lock().unwrap();

        let mut release = |opt: &mut Option<NamespaceKey>| {
            if let Some(key) = opt.take() {
                let state = entries.get_mut(&key).expect("reservation entry missing on drop");
                assert!(state.exclusive, "must be exclusive on drop");
                state.exclusive = false;
                if state.clear() { entries.remove(&key); }
            }
        };
        release(&mut self.first);
        release(&mut self.second);
    }
}
