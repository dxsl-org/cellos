// SPDX-License-Identifier: MPL-2.0
//! Apple RTKit protocol, mailbox message packing, SART window management, and partition protection.
//!
//! Provides the communication layer for Apple Silicon coprocessors (ANS2 storage, SMC, DCP)
//! reached through the ASC mailbox interface (Phase 06, ADR-0032 §5).
//!
//! Clean-room implementation referencing OpenBSD `aplrtk.c`, `aplmbox.c`, and `aplsart.c` (ISC).

#![no_std]

#[cfg(test)]
extern crate std;

/// Endpoint numbers defined by RTKit.
pub mod endpoint {
    /// Management endpoint: handshake, endpoint map, power states.
    pub const MGMT: u8 = 0;
    /// Crash-log buffer endpoint.
    pub const CRASHLOG: u8 = 1;
    /// Syslog buffer endpoint.
    pub const SYSLOG: u8 = 2;
    /// Debug endpoint.
    pub const DEBUG: u8 = 3;
    /// IOReport buffer endpoint.
    pub const IOREPORT: u8 = 4;
    /// OSLog buffer endpoint.
    pub const OSLOG: u8 = 8;
    /// First application (coprocessor-specific) endpoint; lower numbers are RTKit system endpoints.
    pub const APP_START: u8 = 0x20;
}

/// Management message types on endpoint 0 (`data0` bits 59..52).
pub mod mgmt_type {
    /// Coprocessor → AP: supported protocol version range.
    pub const HELLO: u8 = 1;
    /// AP → coprocessor: chosen protocol version.
    pub const HELLO_REPLY: u8 = 2;
    /// AP → coprocessor: start an endpoint.
    pub const STARTEP: u8 = 5;
    /// AP → coprocessor: request an IOP power state.
    pub const SET_IOP_PWR_STATE: u8 = 6;
    /// Coprocessor → AP: IOP power state reached.
    pub const SET_IOP_PWR_STATE_ACK: u8 = 7;
    /// Coprocessor → AP: one 32-endpoint slice of the endpoint bitmap.
    pub const EPMAP: u8 = 8;
    /// AP → coprocessor: acknowledgement of one [`EPMAP`] slice (same code).
    pub const EPMAP_REPLY: u8 = 8;
    /// AP → coprocessor: AP power state.
    pub const SET_AP_PWR_STATE: u8 = 0xB;
    /// Coprocessor → AP: AP power state acknowledged (same code).
    pub const SET_AP_PWR_STATE_ACK: u8 = 0xB;
}

/// Lowest RTKit protocol version this client speaks.
pub const MIN_SUPPORTED_VERSION: u16 = 11;
/// Highest RTKit protocol version this client speaks.
pub const MAX_SUPPORTED_VERSION: u16 = 12;

const MGMT_TYPE_SHIFT: u32 = 52;
const HELLO_MAXVER_SHIFT: u32 = 16;
const EPMAP_BASE_SHIFT: u32 = 32;
const EPMAP_BASE_MASK: u64 = 0x7;
const EPMAP_LAST: u64 = 1 << 51;
const EPMAP_REPLY_MORE: u64 = 1 << 0;

/// Errors reported by the RTKit protocol layer, SART manager and partition guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtKitError {
    /// A management message arrived on a non-management endpoint.
    InvalidEndpoint,
    /// A message type that is not valid in the current handshake state.
    InvalidType,
    /// The coprocessor's protocol version range does not overlap ours.
    HandshakeFailed,
    /// A malformed message payload.
    InvalidPayload,
    /// A SART window index beyond the hardware's 16 entries.
    WindowOverflow,
    /// An I/O request outside the Cellos partition.
    PartitionViolation,
}

/// One RTKit message as carried by the ASC mailbox's two 64-bit words.
///
/// The first word is the 64-bit message; the endpoint travels in the low byte of
/// the second word. Management messages (endpoint 0) put their type in bits
/// 59..52 of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtKitMessage {
    /// Endpoint the message is addressed to or came from.
    pub endpoint: u8,
    /// The 64-bit message word.
    pub msg: u64,
}

impl RtKitMessage {
    /// A message `msg` on `endpoint`.
    pub const fn new(endpoint: u8, msg: u64) -> Self {
        Self { endpoint, msg }
    }

    /// A management message of type `msg_type` whose remaining fields are `fields`.
    pub const fn mgmt(msg_type: u8, fields: u64) -> Self {
        let msg = (fields & !(0xFFu64 << MGMT_TYPE_SHIFT)) | ((msg_type as u64) << MGMT_TYPE_SHIFT);
        Self::new(endpoint::MGMT, msg)
    }

    /// The management type field (bits 59..52); meaningful on endpoint 0 only.
    pub const fn mgmt_type(&self) -> u8 {
        ((self.msg >> MGMT_TYPE_SHIFT) & 0xFF) as u8
    }

    /// Pack into the mailbox's (data0, data1) register values.
    pub const fn to_raw(&self) -> (u64, u64) {
        (self.msg, self.endpoint as u64)
    }

    /// Unpack from the mailbox's (data0, data1) register values.
    pub const fn from_raw(data0: u64, data1: u64) -> Self {
        Self::new((data1 & 0xFF) as u8, data0)
    }
}

/// RTKit handshake state machine for endpoint 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeState {
    /// No HELLO received yet.
    Uninitialized,
    /// HELLO answered; the endpoint map is still arriving.
    Negotiating { version: u16 },
    /// The final EPMAP slice was acknowledged; system endpoints can be started.
    Running { version: u16 },
    /// The coprocessor's version range was refused.
    Failed,
}

/// Endpoint-0 management client: version negotiation and endpoint discovery.
pub struct RtKitClient {
    state: HandshakeState,
    /// One bit per endpoint the coprocessor announced in EPMAP (256 endpoints).
    endpoints: [u32; 8],
}

impl Default for RtKitClient {
    fn default() -> Self {
        Self::new()
    }
}

impl RtKitClient {
    /// A client that has not seen HELLO yet.
    pub const fn new() -> Self {
        Self {
            state: HandshakeState::Uninitialized,
            endpoints: [0; 8],
        }
    }

    /// Current handshake state.
    pub fn state(&self) -> HandshakeState {
        self.state
    }

    /// Whether the coprocessor announced `ep` in its endpoint map.
    pub fn endpoint_present(&self, ep: u8) -> bool {
        self.endpoints[usize::from(ep / 32)] & (1 << (ep % 32)) != 0
    }

    /// Handle an incoming message from the coprocessor on endpoint 0.
    /// Returns an optional reply message to transmit back to the coprocessor.
    pub fn handle_mgmt_message(
        &mut self,
        msg: &RtKitMessage,
    ) -> Result<Option<RtKitMessage>, RtKitError> {
        if msg.endpoint != endpoint::MGMT {
            return Err(RtKitError::InvalidEndpoint);
        }
        match msg.mgmt_type() {
            mgmt_type::HELLO => {
                let min_ver = (msg.msg & 0xFFFF) as u16;
                let max_ver = ((msg.msg >> HELLO_MAXVER_SHIFT) & 0xFFFF) as u16;
                if min_ver > MAX_SUPPORTED_VERSION || max_ver < MIN_SUPPORTED_VERSION {
                    self.state = HandshakeState::Failed;
                    return Err(RtKitError::HandshakeFailed);
                }
                let version = max_ver.min(MAX_SUPPORTED_VERSION);
                self.state = HandshakeState::Negotiating { version };
                self.endpoints = [0; 8];
                let fields = u64::from(version) | (u64::from(version) << HELLO_MAXVER_SHIFT);
                Ok(Some(RtKitMessage::mgmt(mgmt_type::HELLO_REPLY, fields)))
            }
            mgmt_type::EPMAP => {
                let HandshakeState::Negotiating { version } = self.state else {
                    return Err(RtKitError::InvalidType);
                };
                let base = (msg.msg >> EPMAP_BASE_SHIFT) & EPMAP_BASE_MASK;
                self.endpoints[base as usize] |= msg.msg as u32;
                let last = msg.msg & EPMAP_LAST != 0;
                let mut fields = base << EPMAP_BASE_SHIFT;
                if last {
                    fields |= EPMAP_LAST;
                    self.state = HandshakeState::Running { version };
                } else {
                    fields |= EPMAP_REPLY_MORE;
                }
                Ok(Some(RtKitMessage::mgmt(mgmt_type::EPMAP_REPLY, fields)))
            }
            // Power-state acknowledgements (and unknown types) need no reply.
            _ => Ok(None),
        }
    }
}

/// SART (Storage Address Resolution Table) window definition.
/// Filters physical DMA ranges permitted for coprocessor storage engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SartWindow {
    pub base: u64,
    pub size: u64,
    pub readable: bool,
    pub writable: bool,
}

impl SartWindow {
    pub const fn new(base: u64, size: u64, readable: bool, writable: bool) -> Self {
        Self {
            base,
            size,
            readable,
            writable,
        }
    }

    /// Whether `[addr, addr + len)` lies wholly inside this window; a range or
    /// window that wraps the address space is never contained.
    pub fn contains(&self, addr: u64, len: u64) -> bool {
        match (addr.checked_add(len), self.base.checked_add(self.size)) {
            (Some(end), Some(window_end)) => addr >= self.base && end <= window_end,
            _ => false,
        }
    }
}

/// SART Window Manager holding up to 16 hardware filter windows.
pub struct SartManager {
    windows: [Option<SartWindow>; 16],
}

impl Default for SartManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SartManager {
    pub const fn new() -> Self {
        Self {
            windows: [None; 16],
        }
    }

    pub fn set_window(&mut self, index: usize, window: SartWindow) -> Result<(), RtKitError> {
        if index >= 16 {
            return Err(RtKitError::WindowOverflow);
        }
        self.windows[index] = Some(window);
        Ok(())
    }

    pub fn clear_window(&mut self, index: usize) -> Result<(), RtKitError> {
        if index >= 16 {
            return Err(RtKitError::WindowOverflow);
        }
        self.windows[index] = None;
        Ok(())
    }

    /// Check whether a DMA buffer is permitted by any active SART window.
    pub fn is_dma_allowed(&self, addr: u64, len: u64, write: bool) -> bool {
        for w in self.windows.iter().flatten() {
            if w.contains(addr, len) {
                if write && !w.writable {
                    return false;
                }
                if !write && !w.readable {
                    return false;
                }
                return true;
            }
        }
        false
    }
}

/// Partition Guard preventing destructive writes outside the designated Cellos partition.
/// Protects macOS system, recovery, and iSC firmware volumes on Apple Silicon.
pub struct PartitionGuard {
    start_lba: u64,
    lba_count: u64,
}

impl PartitionGuard {
    pub const fn new(start_lba: u64, lba_count: u64) -> Self {
        Self {
            start_lba,
            lba_count,
        }
    }

    pub fn start_lba(&self) -> u64 {
        self.start_lba
    }

    pub fn lba_count(&self) -> u64 {
        self.lba_count
    }

    /// Validate an I/O request against the Cellos partition boundaries.
    pub fn validate_io(&self, lba: u64, count: u64, _is_write: bool) -> Result<(), RtKitError> {
        if count == 0 {
            return Err(RtKitError::PartitionViolation);
        }
        let Some(end_lba) = lba.checked_add(count) else {
            return Err(RtKitError::PartitionViolation);
        };
        let Some(partition_end) = self.start_lba.checked_add(self.lba_count) else {
            return Err(RtKitError::PartitionViolation);
        };
        if lba >= self.start_lba && end_lba <= partition_end {
            Ok(())
        } else {
            Err(RtKitError::PartitionViolation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_raw_round_trip() {
        let msg = RtKitMessage::new(endpoint::APP_START, 0x1234_5678_9ABC_DEF0);
        let (d0, d1) = msg.to_raw();
        assert_eq!((d0, d1), (0x1234_5678_9ABC_DEF0, 0x20));
        assert_eq!(RtKitMessage::from_raw(d0, d1), msg);
    }

    #[test]
    fn mgmt_type_lives_in_bits_59_to_52() {
        let msg = RtKitMessage::mgmt(mgmt_type::EPMAP, u64::MAX);
        assert_eq!(msg.endpoint, endpoint::MGMT);
        assert_eq!(msg.mgmt_type(), mgmt_type::EPMAP);
        assert_eq!((msg.msg >> 52) & 0xFF, 8);
        assert_eq!(msg.msg & !(0xFFu64 << 52), !(0xFFu64 << 52));
    }

    #[test]
    fn handshake_negotiates_version_and_acknowledges_epmap() {
        let mut client = RtKitClient::new();
        assert_eq!(client.state(), HandshakeState::Uninitialized);

        // Firmware offers versions 11..=13; the client picks its own maximum, 12.
        let hello = RtKitMessage::mgmt(mgmt_type::HELLO, 11 | (13 << 16));
        let reply = client.handle_mgmt_message(&hello).expect("valid hello");
        assert_eq!(client.state(), HandshakeState::Negotiating { version: 12 });
        assert_eq!(
            reply,
            Some(RtKitMessage::mgmt(mgmt_type::HELLO_REPLY, 12 | (12 << 16)))
        );

        // Slice 0 (endpoints 0, 1, 2) with more to come, then slice 1 (endpoint 0x20) as last.
        let first = RtKitMessage::mgmt(mgmt_type::EPMAP, 0b111);
        let reply = client.handle_mgmt_message(&first).expect("epmap");
        assert_eq!(reply, Some(RtKitMessage::mgmt(mgmt_type::EPMAP_REPLY, 1)));
        assert_eq!(client.state(), HandshakeState::Negotiating { version: 12 });

        let last = RtKitMessage::mgmt(mgmt_type::EPMAP, (1 << 51) | (1 << 32) | 1);
        let reply = client.handle_mgmt_message(&last).expect("last epmap");
        assert_eq!(
            reply,
            Some(RtKitMessage::mgmt(
                mgmt_type::EPMAP_REPLY,
                (1 << 51) | (1 << 32)
            ))
        );
        assert_eq!(client.state(), HandshakeState::Running { version: 12 });
        assert!(client.endpoint_present(endpoint::SYSLOG));
        assert!(client.endpoint_present(endpoint::APP_START));
        assert!(!client.endpoint_present(endpoint::DEBUG));
    }

    #[test]
    fn handshake_refuses_disjoint_version_range() {
        let mut client = RtKitClient::new();
        let hello = RtKitMessage::mgmt(mgmt_type::HELLO, 2 | (10 << 16));
        assert_eq!(
            client.handle_mgmt_message(&hello),
            Err(RtKitError::HandshakeFailed)
        );
        assert_eq!(client.state(), HandshakeState::Failed);
        let epmap = RtKitMessage::mgmt(mgmt_type::EPMAP, 1 << 51);
        assert_eq!(
            client.handle_mgmt_message(&epmap),
            Err(RtKitError::InvalidType)
        );
    }

    #[test]
    fn sart_window_filtering() {
        let mut sart = SartManager::new();
        let window = SartWindow::new(0x8_1000_0000, 0x10_0000, true, true);
        sart.set_window(0, window).expect("window 0 set");

        assert!(sart.is_dma_allowed(0x8_1000_0000, 0x1000, false));
        assert!(sart.is_dma_allowed(0x8_100F_0000, 0x1000, true));
        // Out of window bounds
        assert!(!sart.is_dma_allowed(0x8_1000_0000 - 0x1000, 0x1000, false));
        assert!(!sart.is_dma_allowed(0x8_1010_0000, 0x1000, true));
    }

    #[test]
    fn partition_guard_blocks_out_of_bounds_io() {
        // Cellos partition: LBA 100_000 .. 200_000
        let guard = PartitionGuard::new(100_000, 100_000);

        // Within partition
        assert_eq!(guard.validate_io(100_000, 10, true), Ok(()));
        assert_eq!(guard.validate_io(199_990, 10, true), Ok(()));

        // Outside partition (e.g. macOS APFS or EFI boot volume)
        assert_eq!(
            guard.validate_io(0, 10, true),
            Err(RtKitError::PartitionViolation)
        );
        assert_eq!(
            guard.validate_io(99_999, 10, true),
            Err(RtKitError::PartitionViolation)
        );
        assert_eq!(
            guard.validate_io(200_000, 1, true),
            Err(RtKitError::PartitionViolation)
        );
    }
}
