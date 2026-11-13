// SPDX-License-Identifier: Apache-2.0
//! Non-production control protocol for hostile backend recovery images.
//!
//! This module is available only through the `hostile-backend-recovery` feature.

/// Opcode for a request to terminate a supervised hypervisor backend.
pub const KILL_REQUEST_OPCODE: u8 = 0x04;
/// Opcode for the bounded response to a backend kill request.
pub const KILL_RESPONSE_OPCODE: u8 = 0x05;
/// Exact encoded size of a backend kill request.
pub const KILL_REQUEST_LEN: usize = 3;
/// Exact encoded size of a backend kill response.
pub const KILL_RESPONSE_LEN: usize = 2;

/// The requested backend was terminated.
pub const KILL_STATUS_OK: u8 = 0x00;
/// The request did not match the exact wire contract.
pub const KILL_STATUS_INVALID_REQUEST: u8 = 0x01;
/// The request sender was not the hypervisor Cell.
pub const KILL_STATUS_REJECTED_CALLER: u8 = 0x02;
/// The requested service is not a killable hostile-test backend.
pub const KILL_STATUS_SERVICE_NOT_ALLOWED: u8 = 0x03;
/// No current provider is registered for the requested service.
pub const KILL_STATUS_SERVICE_NOT_FOUND: u8 = 0x04;
/// The SupervisorCap-backed kill operation failed.
pub const KILL_STATUS_KILL_FAILED: u8 = 0x05;

/// Stable prefix for host-authored backend disconnect evidence.
pub const DISCONNECT_LOG_MARKER: &str = "HOSTILE_BACKEND_DISCONNECT";

/// Encode a strict backend kill request as `[opcode, service_id_le]`.
pub const fn encode_kill_request(service_id: u16) -> [u8; KILL_REQUEST_LEN] {
    let service_id = service_id.to_le_bytes();
    [KILL_REQUEST_OPCODE, service_id[0], service_id[1]]
}

/// Parse a canonical backend kill request and return its service ID.
///
/// IPC receive buffers may be larger than the three-byte frame, so canonical
/// zero padding is accepted. A missing prefix, mismatched opcode, or any
/// non-zero trailing byte is rejected.
pub fn parse_kill_request(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < KILL_REQUEST_LEN
        || bytes[0] != KILL_REQUEST_OPCODE
        || bytes[KILL_REQUEST_LEN..].iter().any(|&byte| byte != 0)
    {
        return None;
    }
    Some(u16::from_le_bytes([bytes[1], bytes[2]]))
}

/// Encode a strict backend kill response as `[opcode, status]`.
pub const fn encode_kill_response(status: u8) -> [u8; KILL_RESPONSE_LEN] {
    [KILL_RESPONSE_OPCODE, status]
}

/// Parse a canonical backend kill response and return its status byte.
///
/// IPC receive buffers may be larger than the two-byte frame, so canonical
/// zero padding is accepted. A missing prefix, mismatched opcode, or any
/// non-zero trailing byte is rejected.
pub fn parse_kill_response(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < KILL_RESPONSE_LEN
        || bytes[0] != KILL_RESPONSE_OPCODE
        || bytes[KILL_RESPONSE_LEN..].iter().any(|&byte| byte != 0)
    {
        return None;
    }
    Some(bytes[1])
}

#[cfg(test)]
mod tests {
    use super::{
        encode_kill_request, encode_kill_response, parse_kill_request, parse_kill_response,
        KILL_REQUEST_OPCODE, KILL_RESPONSE_OPCODE,
    };

    #[test]
    fn request_is_strict_little_endian_with_canonical_padding() {
        assert_eq!(
            encode_kill_request(0x1234),
            [KILL_REQUEST_OPCODE, 0x34, 0x12]
        );
        let mut padded = [0u8; 4096];
        padded[..3].copy_from_slice(&encode_kill_request(0x1234));
        assert_eq!(parse_kill_request(&padded), Some(0x1234));
        assert_eq!(parse_kill_request(&[KILL_REQUEST_OPCODE, 0x34]), None);
        padded[3] = 1;
        assert_eq!(parse_kill_request(&padded), None);
    }

    #[test]
    fn response_is_strict_with_canonical_padding() {
        assert_eq!(encode_kill_response(7), [KILL_RESPONSE_OPCODE, 7]);
        let mut padded = [0u8; 8];
        padded[..2].copy_from_slice(&encode_kill_response(7));
        assert_eq!(parse_kill_response(&padded), Some(7));
        assert_eq!(parse_kill_response(&[KILL_RESPONSE_OPCODE]), None);
        padded[2] = 1;
        assert_eq!(parse_kill_response(&padded), None);
    }
}
