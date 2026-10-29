//! Native CellOS Poller implementation for async I/O runtimes (smol / async-io).
//!
//! Provides the core `Poller`, `Event`, and `Events` primitives backed by CellOS IPC
//! readiness and kernel-level reactor wakeup coalescing.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::time::Duration;
use spin::Mutex;

use api::ipc::{net_ready, encode_tcp_interests, NetRequest, NetResponse, TcpInterest, TcpReadyEvent, NET_READY_RECORD_BYTES, REACTOR_WAKE_OPCODE};
use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_try_recv, sys_try_send, SyscallResult};

/// An observed I/O readiness event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Opaque token / key assigned by the caller.
    pub key: usize,
    /// Whether the file/socket is readable.
    pub readable: bool,
    /// Whether the file/socket is writable.
    pub writable: bool,
}

impl Event {
    /// Create a new event with arbitrary readability and writability.
    pub const fn new(key: usize, readable: bool, writable: bool) -> Self {
        Self { key, readable, writable }
    }

    /// An event signaling readability.
    pub const fn readable(key: usize) -> Self {
        Self { key, readable: true, writable: false }
    }

    /// An event signaling writability.
    pub const fn writable(key: usize) -> Self {
        Self { key, readable: false, writable: true }
    }

    /// An event signaling both readability and writability.
    pub const fn all(key: usize) -> Self {
        Self { key, readable: true, writable: true }
    }

    /// An empty event.
    pub const fn none(key: usize) -> Self {
        Self { key, readable: false, writable: false }
    }

    /// Convert event flags into CellOS `net_ready` mask.
    pub fn to_net_mask(&self) -> u8 {
        let mut mask = 0;
        if self.readable {
            mask |= net_ready::READ | net_ready::ACCEPT;
        }
        if self.writable {
            mask |= net_ready::WRITE;
        }
        mask
    }
}

/// A container for returned events.
#[derive(Debug, Default)]
pub struct Events {
    list: Vec<Event>,
}

impl Events {
    /// Create an empty event list.
    pub fn new() -> Self {
        Self { list: Vec::new() }
    }

    /// Create an empty event list with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self { list: Vec::with_capacity(capacity) }
    }

    /// Clear all events in the list.
    pub fn clear(&mut self) {
        self.list.clear();
    }

    /// Check if the event list is empty.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Return the number of events.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Iterate over observed events.
    pub fn iter(&self) -> core::slice::Iter<'_, Event> {
        self.list.iter()
    }

    /// Push an event into the list.
    pub fn push(&mut self, event: Event) {
        self.list.push(event);
    }
}

/// Errors returned by `Poller` operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollerError {
    /// The requested resource was already registered.
    AlreadyExists,
    /// The requested resource was not found.
    NotFound,
    /// Network service is unavailable.
    NetServiceUnavailable,
    /// Invalid arguments or interest mask.
    InvalidInput,
    /// IPC transport or kernel error.
    IpcError,
}

/// Poller managing socket interest registrations and multiplexing readiness events.
pub struct Poller {
    /// Registered socket interests: socket_cap -> (key, net_mask).
    interests: Mutex<BTreeMap<u32, (usize, u8)>>,
}

impl Poller {
    /// Create a new Poller instance.
    pub fn new() -> Result<Self, PollerError> {
        Ok(Self {
            interests: Mutex::new(BTreeMap::new()),
        })
    }

    /// Register a socket capability and its desired interest.
    pub fn add(&self, socket_cap: u32, event: Event) -> Result<(), PollerError> {
        let mut lock = self.interests.lock();
        if lock.contains_key(&socket_cap) {
            return Err(PollerError::AlreadyExists);
        }
        let mask = event.to_net_mask();
        if mask == 0 {
            return Err(PollerError::InvalidInput);
        }
        lock.insert(socket_cap, (event.key, mask));
        Ok(())
    }

    /// Modify the interest or key for an already-registered socket capability.
    pub fn modify(&self, socket_cap: u32, event: Event) -> Result<(), PollerError> {
        let mut lock = self.interests.lock();
        let entry = lock.get_mut(&socket_cap).ok_or(PollerError::NotFound)?;
        let mask = event.to_net_mask();
        if mask == 0 {
            return Err(PollerError::InvalidInput);
        }
        *entry = (event.key, mask);
        Ok(())
    }

    /// Deregister a socket capability from the poller.
    pub fn delete(&self, socket_cap: u32) -> Result<(), PollerError> {
        let mut lock = self.interests.lock();
        lock.remove(&socket_cap).ok_or(PollerError::NotFound)?;
        Ok(())
    }

    /// Interrupt any ongoing or future `wait()` call on this reactor.
    ///
    /// Issues a kernel-coalesced `REACTOR_WAKE` (0x12) message to self (target=0).
    pub fn notify(&self) -> Result<(), PollerError> {
        let wake = [REACTOR_WAKE_OPCODE];
        match sys_try_send(0, &wake) {
            SyscallResult::Ok(_) => Ok(()),
            SyscallResult::Err(_) => Err(PollerError::IpcError),
        }
    }

    /// Wait for readiness events across registered sockets.
    pub fn wait(&self, events: &mut Events, timeout: Option<Duration>) -> Result<usize, PollerError> {
        events.clear();

        // 1. Drain any pending same-cell notify wakeup
        let mut drain_buf = [0u8; 64];
        if let SyscallResult::Ok(sender) = sys_try_recv(0, &mut drain_buf) {
            if sender > 0 && drain_buf.first() == Some(&REACTOR_WAKE_OPCODE) {
                // Notified: return empty events (interrupted wait)
                return Ok(0);
            }
        }

        // 2. Snapshot current interests
        let snapshot: Vec<TcpInterest> = {
            let lock = self.interests.lock();
            lock.iter()
                .map(|(&cap_id, &(_, mask))| TcpInterest { cap_id, mask })
                .collect()
        };

        if snapshot.is_empty() {
            // No sockets registered: park for timeout if any, otherwise return immediately
            if let Some(d) = timeout {
                if !d.is_zero() {
                    let ticks = (d.as_millis() / 10).max(1) as u64;
                    let mut dummy = [0u8; 8];
                    let _ = sys_recv_timeout(0, &mut dummy, ticks);
                }
            }
            return Ok(0);
        }

        // 3. Resolve Net Service
        let net_tid = sys_lookup_service(service::NET).ok_or(PollerError::NetServiceUnavailable)?;

        // 4. Encode interests into packed buffer
        let mut packed = [0u8; 256 * NET_READY_RECORD_BYTES];
        let Some(encoded_interests) = encode_tcp_interests(&snapshot, &mut packed) else {
            return Err(PollerError::InvalidInput);
        };

        let wait_flag = timeout != Some(Duration::ZERO);
        let request = NetRequest::TcpReady {
            interests: encoded_interests,
            cursor: 0,
            wait: wait_flag,
        };

        let mut req_buf = [0u8; 1536];
        let Ok(req_bytes) = api::ipc::encode(&request, &mut req_buf) else {
            return Err(PollerError::InvalidInput);
        };

        // 5. Send request to net service
        if let SyscallResult::Err(_) = ostd::syscall::sys_send(net_tid, req_bytes) {
            return Err(PollerError::IpcError);
        }

        // 6. Receive response with timeout
        let timeout_ticks = timeout
            .map(|d| (d.as_millis() / 10).max(1) as u64)
            .unwrap_or(3000); // Default 30s timeout if None

        let mut resp_buf = [0u8; 1536];
        let recv_res = sys_recv_timeout(0, &mut resp_buf, timeout_ticks);

        let sender = match recv_res {
            SyscallResult::Ok(0) => return Ok(0), // Timeout expired
            SyscallResult::Ok(s) => s,
            SyscallResult::Err(_) => return Err(PollerError::IpcError),
        };

        // Check if interrupted by self notify
        if sender != net_tid {
            if resp_buf.first() == Some(&REACTOR_WAKE_OPCODE) {
                return Ok(0); // Woken by notify
            }
            return Ok(0);
        }

        // Decode NetResponse
        let Ok(response) = api::ipc::decode::<NetResponse<'_>>(&resp_buf) else {
            return Err(PollerError::IpcError);
        };

        match response {
            NetResponse::TcpReady { events: ready_slice, .. } => {
                let lock = self.interests.lock();
                for chunk in ready_slice.chunks_exact(NET_READY_RECORD_BYTES) {
                    if let Some(ev) = TcpReadyEvent::decode(chunk) {
                        if let Some(&(key, _)) = lock.get(&ev.cap_id) {
                            let readable = ev.ready & (net_ready::READ | net_ready::ACCEPT) != 0;
                            let writable = ev.ready & net_ready::WRITE != 0;
                            if readable || writable {
                                events.push(Event::new(key, readable, writable));
                            }
                        }
                    }
                }
                Ok(events.len())
            }
            NetResponse::NotReady => Ok(0),
            _ => Ok(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_mask_conversions() {
        let r = Event::readable(10);
        assert!(r.readable);
        assert!(!r.writable);
        assert_eq!(r.to_net_mask(), net_ready::READ | net_ready::ACCEPT);

        let w = Event::writable(20);
        assert!(!w.readable);
        assert!(w.writable);
        assert_eq!(w.to_net_mask(), net_ready::WRITE);

        let all = Event::all(30);
        assert!(all.readable && all.writable);
        assert_eq!(all.to_net_mask(), net_ready::READ | net_ready::ACCEPT | net_ready::WRITE);
    }

    #[test]
    fn test_poller_registration_lifecycle() {
        let poller = Poller::new().expect("poller creation");
        let socket_cap = 1001;

        // Add socket interest
        assert_eq!(poller.add(socket_cap, Event::readable(1)), Ok(()));

        // Duplicate add must fail with AlreadyExists
        assert_eq!(poller.add(socket_cap, Event::writable(1)), Err(PollerError::AlreadyExists));

        // Modify existing interest
        assert_eq!(poller.modify(socket_cap, Event::all(2)), Ok(()));

        // Delete existing socket
        assert_eq!(poller.delete(socket_cap), Ok(()));

        // Delete non-existent socket must fail with NotFound
        assert_eq!(poller.delete(socket_cap), Err(PollerError::NotFound));
    }

    #[test]
    fn test_events_container() {
        let mut events = Events::with_capacity(16);
        assert!(events.is_empty());
        assert_eq!(events.len(), 0);

        events.push(Event::readable(1));
        events.push(Event::writable(2));
        assert_eq!(events.len(), 2);

        events.clear();
        assert!(events.is_empty());
    }
}
