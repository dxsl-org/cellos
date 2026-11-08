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
    pub const MGMT: u8 = 0;
    pub const CRASHLOG: u8 = 1;
    pub const SYSLOG: u8 = 2;
    pub const IO_STORAGE: u8 = 32;
}

/// Management message types on endpoint 0.
pub mod mgmt_type {
    pub const HELLO: u8 = 1;
    pub const HELLO_ACK: u8 = 2;
    pub const RESTART: u8 = 3;
    pub const SLEEP: u8 = 4;
    pub const WAKE: u8 = 5;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtKitError {
    InvalidEndpoint,
    InvalidType,
    HandshakeFailed,
    InvalidPayload,
    WindowOverflow,
    PartitionViolation,
}

/// An RTKit 128-bit mailbox message (two 64-bit words).
///
/// In `data0`:
/// - Bits 0..31: 32-bit payload
/// - Bits 32..39: 8-bit endpoint ID
/// - Bits 40..47: 8-bit message type
/// - Bits 48..63: reserved / sequence flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtKitMessage {
    pub endpoint: u8,
    pub msg_type: u8,
    pub payload: u32,
    pub data1: u64,
}

impl RtKitMessage {
    pub const fn new(endpoint: u8, msg_type: u8, payload: u32, data1: u64) -> Self {
        Self {
            endpoint,
            msg_type,
            payload,
            data1,
        }
    }

    /// Pack message into (data0, data1) 64-bit register values.
    pub const fn to_raw(&self) -> (u64, u64) {
        let data0 =
            (self.payload as u64) | ((self.endpoint as u64) << 32) | ((self.msg_type as u64) << 40);
        (data0, self.data1)
    }

    /// Unpack message from (data0, data1) 64-bit register values.
    pub const fn from_raw(data0: u64, data1: u64) -> Self {
        let payload = (data0 & 0xFFFF_FFFF) as u32;
        let endpoint = ((data0 >> 32) & 0xFF) as u8;
        let msg_type = ((data0 >> 40) & 0xFF) as u8;
        Self {
            endpoint,
            msg_type,
            payload,
            data1,
        }
    }
}

/// RTKit handshake state machine for endpoint 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeState {
    Uninitialized,
    Negotiating,
    Running { version: u32 },
    Failed,
}

pub struct RtKitClient {
    state: HandshakeState,
    supported_version: u32,
}

impl RtKitClient {
    pub const fn new(supported_version: u32) -> Self {
        Self {
            state: HandshakeState::Uninitialized,
            supported_version,
        }
    }

    pub fn state(&self) -> HandshakeState {
        self.state
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
        match msg.msg_type {
            mgmt_type::HELLO => {
                let coproc_version = msg.payload;
                if coproc_version == 0 {
                    self.state = HandshakeState::Failed;
                    return Err(RtKitError::HandshakeFailed);
                }
                let agreed_version = if coproc_version < self.supported_version {
                    coproc_version
                } else {
                    self.supported_version
                };
                self.state = HandshakeState::Running {
                    version: agreed_version,
                };
                Ok(Some(RtKitMessage::new(
                    endpoint::MGMT,
                    mgmt_type::HELLO_ACK,
                    agreed_version,
                    0,
                )))
            }
            mgmt_type::SLEEP => Ok(Some(RtKitMessage::new(
                endpoint::MGMT,
                mgmt_type::SLEEP,
                0,
                0,
            ))),
            mgmt_type::WAKE => Ok(Some(RtKitMessage::new(
                endpoint::MGMT,
                mgmt_type::WAKE,
                0,
                0,
            ))),
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

    pub fn contains(&self, addr: u64, len: u64) -> bool {
        if let Some(end) = addr.checked_add(len) {
            addr >= self.base && end <= self.base + self.size
        } else {
            false
        }
    }
}

/// SART Window Manager holding up to 16 hardware filter windows.
pub struct SartManager {
    windows: [Option<SartWindow>; 16],
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
        let partition_end = self.start_lba + self.lba_count;
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
        let msg = RtKitMessage::new(endpoint::MGMT, mgmt_type::HELLO, 2, 0x1234_5678_9ABC_DEF0);
        let (d0, d1) = msg.to_raw();
        let unpacked = RtKitMessage::from_raw(d0, d1);
        assert_eq!(msg, unpacked);
        assert_eq!(unpacked.endpoint, endpoint::MGMT);
        assert_eq!(unpacked.msg_type, mgmt_type::HELLO);
        assert_eq!(unpacked.payload, 2);
        assert_eq!(unpacked.data1, 0x1234_5678_9ABC_DEF0);
    }

    #[test]
    fn handshake_state_machine() {
        let mut client = RtKitClient::new(2);
        assert_eq!(client.state(), HandshakeState::Uninitialized);

        let hello = RtKitMessage::new(endpoint::MGMT, mgmt_type::HELLO, 2, 0);
        let reply = client.handle_mgmt_message(&hello).expect("valid hello");
        assert_eq!(client.state(), HandshakeState::Running { version: 2 });
        assert_eq!(
            reply,
            Some(RtKitMessage::new(
                endpoint::MGMT,
                mgmt_type::HELLO_ACK,
                2,
                0
            ))
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
