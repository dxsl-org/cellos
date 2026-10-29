//! CellOS Mio Subsystem & Architecture Backend.
//!
//! Provides `Selector`, `Waker`, `Registry`, `event::Source`, `Token`, and `Interest`
//! matching the standard `mio` architecture contract for driving Tokio and Axum on CellOS.

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

/// An opaque token used to identify an event source.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Token(pub usize);

/// Readiness interest flags.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Interest(u8);

impl Interest {
    /// Read interest.
    pub const READABLE: Interest = Interest(1 << 0);
    /// Write interest.
    pub const WRITABLE: Interest = Interest(1 << 1);

    /// Combine interests.
    pub const fn add(self, other: Interest) -> Interest {
        Interest(self.0 | other.0)
    }

    /// Check if read interest is set.
    pub const fn is_readable(self) -> bool {
        (self.0 & Self::READABLE.0) != 0
    }

    /// Check if write interest is set.
    pub const fn is_writable(self) -> bool {
        (self.0 & Self::WRITABLE.0) != 0
    }

    /// Convert to CellOS net_ready mask.
    pub fn to_net_mask(self) -> u8 {
        let mut mask = 0;
        if self.is_readable() {
            mask |= net_ready::READ | net_ready::ACCEPT;
        }
        if self.is_writable() {
            mask |= net_ready::WRITE;
        }
        mask
    }
}

/// An observed readiness event.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub token: Token,
    pub readable: bool,
    pub writable: bool,
}

/// A container of events returned by `select`.
#[derive(Debug, Default)]
pub struct Events {
    list: Vec<Event>,
}

impl Events {
    pub fn new() -> Self {
        Self { list: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self { list: Vec::with_capacity(capacity) }
    }

    pub fn clear(&mut self) {
        self.list.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn iter(&self) -> core::slice::Iter<'_, Event> {
        self.list.iter()
    }

    pub fn push(&mut self, event: Event) {
        self.list.push(event);
    }
}

/// Errors returned by Mio operations on CellOS.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MioError {
    AlreadyExists,
    NotFound,
    NetServiceUnavailable,
    InvalidInput,
    IpcError,
}

/// Registry providing registration access to the selector.
pub struct Registry<'a> {
    selector: &'a Selector,
}

impl<'a> Registry<'a> {
    pub fn new(selector: &'a Selector) -> Self {
        Self { selector }
    }

    pub fn register(&self, socket_cap: u32, token: Token, interests: Interest) -> Result<(), MioError> {
        self.selector.register(socket_cap, token, interests)
    }

    pub fn reregister(&self, socket_cap: u32, token: Token, interests: Interest) -> Result<(), MioError> {
        self.selector.reregister(socket_cap, token, interests)
    }

    pub fn deregister(&self, socket_cap: u32) -> Result<(), MioError> {
        self.selector.deregister(socket_cap)
    }
}

pub mod event {
    use super::*;

    /// Trait implemented by event sources (e.g. TcpStream, TcpListener).
    pub trait Source {
        fn register(&mut self, registry: &Registry<'_>, token: Token, interests: Interest) -> Result<(), MioError>;
        fn reregister(&mut self, registry: &Registry<'_>, token: Token, interests: Interest) -> Result<(), MioError>;
        fn deregister(&mut self, registry: &Registry<'_>) -> Result<(), MioError>;
    }
}

/// The core I/O multiplexer backend for Tokio.
pub struct Selector {
    interests: Mutex<BTreeMap<u32, (Token, u8)>>,
    waker_token: Mutex<Option<Token>>,
}

impl Selector {
    pub fn new() -> Result<Self, MioError> {
        Ok(Self {
            interests: Mutex::new(BTreeMap::new()),
            waker_token: Mutex::new(None),
        })
    }

    pub fn register(&self, socket_cap: u32, token: Token, interests: Interest) -> Result<(), MioError> {
        let mut lock = self.interests.lock();
        if lock.contains_key(&socket_cap) {
            return Err(MioError::AlreadyExists);
        }
        let mask = interests.to_net_mask();
        if mask == 0 {
            return Err(MioError::InvalidInput);
        }
        lock.insert(socket_cap, (token, mask));
        Ok(())
    }

    pub fn reregister(&self, socket_cap: u32, token: Token, interests: Interest) -> Result<(), MioError> {
        let mut lock = self.interests.lock();
        let entry = lock.get_mut(&socket_cap).ok_or(MioError::NotFound)?;
        let mask = interests.to_net_mask();
        if mask == 0 {
            return Err(MioError::InvalidInput);
        }
        *entry = (token, mask);
        Ok(())
    }

    pub fn deregister(&self, socket_cap: u32) -> Result<(), MioError> {
        let mut lock = self.interests.lock();
        lock.remove(&socket_cap).ok_or(MioError::NotFound)?;
        Ok(())
    }

    pub fn select(&self, events: &mut Events, timeout: Option<Duration>) -> Result<(), MioError> {
        events.clear();

        // 1. Drain any pending same-cell notify wakeup
        let mut drain_buf = [0u8; 64];
        if let SyscallResult::Ok(sender) = sys_try_recv(0, &mut drain_buf) {
            if sender > 0 && drain_buf.first() == Some(&REACTOR_WAKE_OPCODE) {
                if let Some(token) = *self.waker_token.lock() {
                    events.push(Event { token, readable: true, writable: false });
                }
                return Ok(());
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
            if let Some(d) = timeout {
                if !d.is_zero() {
                    let ticks = (d.as_millis() / 10).max(1) as u64;
                    let mut dummy = [0u8; 8];
                    let _ = sys_recv_timeout(0, &mut dummy, ticks);
                }
            }
            return Ok(());
        }

        // 3. Resolve Net Service
        let net_tid = sys_lookup_service(service::NET).ok_or(MioError::NetServiceUnavailable)?;

        // 4. Encode interests
        let mut packed = [0u8; 256 * NET_READY_RECORD_BYTES];
        let Some(encoded) = encode_tcp_interests(&snapshot, &mut packed) else {
            return Err(MioError::InvalidInput);
        };

        let wait_flag = timeout != Some(Duration::ZERO);
        let req = NetRequest::TcpReady {
            interests: encoded,
            cursor: 0,
            wait: wait_flag,
        };

        let mut req_buf = [0u8; 1536];
        let Ok(req_bytes) = api::ipc::encode(&req, &mut req_buf) else {
            return Err(MioError::InvalidInput);
        };

        // 5. Send request
        if let SyscallResult::Err(_) = ostd::syscall::sys_send(net_tid, req_bytes) {
            return Err(MioError::IpcError);
        }

        // 6. Receive response
        let timeout_ticks = timeout
            .map(|d| (d.as_millis() / 10).max(1) as u64)
            .unwrap_or(3000);

        let mut resp_buf = [0u8; 1536];
        let recv_res = sys_recv_timeout(0, &mut resp_buf, timeout_ticks);

        let sender = match recv_res {
            SyscallResult::Ok(0) => return Ok(()),
            SyscallResult::Ok(s) => s,
            SyscallResult::Err(_) => return Err(MioError::IpcError),
        };

        if sender != net_tid {
            if resp_buf.first() == Some(&REACTOR_WAKE_OPCODE) {
                if let Some(token) = *self.waker_token.lock() {
                    events.push(Event { token, readable: true, writable: false });
                }
            }
            return Ok(());
        }

        let Ok(response) = api::ipc::decode::<NetResponse<'_>>(&resp_buf) else {
            return Err(MioError::IpcError);
        };

        if let NetResponse::TcpReady { events: ready_slice, .. } = response {
            let lock = self.interests.lock();
            for chunk in ready_slice.chunks_exact(NET_READY_RECORD_BYTES) {
                if let Some(ev) = TcpReadyEvent::decode(chunk) {
                    if let Some(&(token, _)) = lock.get(&ev.cap_id) {
                        let readable = ev.ready & (net_ready::READ | net_ready::ACCEPT) != 0;
                        let writable = ev.ready & net_ready::WRITE != 0;
                        if readable || writable {
                            events.push(Event { token, readable, writable });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// A Waker used to awaken a blocked Selector.
pub struct Waker {
    token: Token,
}

impl Waker {
    pub fn new(selector: &Selector, token: Token) -> Result<Self, MioError> {
        *selector.waker_token.lock() = Some(token);
        Ok(Self { token })
    }

    pub fn wake(&self) -> Result<(), MioError> {
        let wake_msg = [REACTOR_WAKE_OPCODE];
        match sys_try_send(0, &wake_msg) {
            SyscallResult::Ok(_) => Ok(()),
            SyscallResult::Err(_) => Err(MioError::IpcError),
        }
    }

    pub fn token(&self) -> Token {
        self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interest_and_token() {
        let t = Token(100);
        assert_eq!(t.0, 100);

        let read = Interest::READABLE;
        assert!(read.is_readable());
        assert!(!read.is_writable());

        let both = Interest::READABLE.add(Interest::WRITABLE);
        assert!(both.is_readable());
        assert!(both.is_writable());
    }

    #[test]
    fn test_selector_lifecycle() {
        let selector = Selector::new().unwrap();
        let cap = 55;
        let token = Token(1);

        assert_eq!(selector.register(cap, token, Interest::READABLE), Ok(()));
        assert_eq!(selector.register(cap, token, Interest::WRITABLE), Err(MioError::AlreadyExists));
        assert_eq!(selector.reregister(cap, token, Interest::WRITABLE), Ok(()));
        assert_eq!(selector.deregister(cap), Ok(()));
        assert_eq!(selector.deregister(cap), Err(MioError::NotFound));
    }

    #[test]
    fn test_waker() {
        let selector = Selector::new().unwrap();
        let waker = Waker::new(&selector, Token(999)).unwrap();
        assert_eq!(waker.token(), Token(999));
    }
}
