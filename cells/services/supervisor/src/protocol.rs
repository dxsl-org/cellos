//! IPC message types for the Supervisor Cell.
//!
//! Wire format (buf[0] = discriminant):
//!   0x01 — HotswapRequest  { target_service: [u8; 64], new_elf: [u8; 128] }  → total 193 B
//!   0x02 — SnapshotRequest { }                                                → total   1 B
//!   0x03 — StatusReply     { phase: u8, result: u8 }                          → total   3 B
//!
//! Snapshot is global whole-RAM state, so the request is opcode-only. The App
//! SDK hands the supervisor a zero-padded receive buffer; snapshot parsing is
//! deliberately strict and accepts only an opcode followed by all-zero padding.

pub const OP_HOTSWAP: u8 = 0x01;
pub const OP_SNAPSHOT: u8 = 0x02;
pub const OP_STATUS: u8 = 0x03;
pub const SNAPSHOT_STATUS_PHASE: u8 = 1;
pub const STATUS_OK: u8 = 0x00;
pub const STATUS_UNAVAILABLE: u8 = 0x01;
pub const STATUS_REJECTED_CALLER: u8 = 0xFD;
pub const STATUS_SERVICE_NOT_FOUND: u8 = 0xFE;
pub const STATUS_INVALID_REQUEST: u8 = 0xFF;

pub const SVC_NAME_LEN: usize = 64;
pub const ELF_PATH_LEN: usize = 128;
pub const HOTSWAP_REQUEST_LEN: usize = 1 + SVC_NAME_LEN + ELF_PATH_LEN;
pub const SNAPSHOT_REQUEST_LEN: usize = 1;

/// Hotswap request from an authorized `hotswap` CLI caller.
pub struct HotswapRequest<'a> {
    service_name: &'a str,
    elf_path: &'a str,
}

/// Opcode-only snapshot request from the shell.
pub struct SnapshotRequest;

impl<'a> HotswapRequest<'a> {
    /// Parse the fixed-size request prefix from an App SDK receive buffer.
    ///
    /// `AppContext` owns a larger zero-padded receive buffer and does not expose
    /// the sender's original byte count, so trailing bytes are intentionally
    /// ignored after both fixed fields have been validated.
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < HOTSWAP_REQUEST_LEN {
            return None;
        }
        if buf[0] != OP_HOTSWAP {
            return None;
        }
        let service_name = parse_service_name(&buf[1..1 + SVC_NAME_LEN])?;
        let elf_path = parse_elf_path(&buf[1 + SVC_NAME_LEN..HOTSWAP_REQUEST_LEN])?;
        Some(Self {
            service_name,
            elf_path,
        })
    }

    /// Return the validated service name.
    pub fn service_name(&self) -> &str {
        self.service_name
    }

    /// Return the validated replacement ELF path.
    pub fn elf_path(&self) -> &str {
        self.elf_path
    }
}

impl SnapshotRequest {
    /// Parse an opcode-only request from the App SDK receive buffer.
    ///
    /// `AppContext` retains a zero-padded buffer after the caller's original
    /// bytes, so snapshot accepts only the opcode followed by all-zero padding.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.is_empty() || buf[0] != OP_SNAPSHOT {
            return None;
        }
        if buf[SNAPSHOT_REQUEST_LEN..].iter().any(|&byte| byte != 0) {
            return None;
        }
        Some(Self)
    }
}

fn parse_service_name(field: &[u8]) -> Option<&str> {
    let value = parse_nul_terminated_ascii(field)?;
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Some(value)
    } else {
        None
    }
}

fn parse_elf_path(field: &[u8]) -> Option<&str> {
    let value = parse_nul_terminated_ascii(field)?;
    let suffix = value.strip_prefix("/bin/")?;
    if suffix.is_empty() || suffix.contains('/') || suffix.contains("..") || value.contains("//") {
        return None;
    }
    Some(value)
}

fn parse_nul_terminated_ascii(field: &[u8]) -> Option<&str> {
    let nul = field.iter().position(|&byte| byte == 0)?;
    if nul == 0 || field[nul + 1..].iter().any(|&byte| byte != 0) {
        return None;
    }
    if !field[..nul].is_ascii() {
        return None;
    }
    core::str::from_utf8(&field[..nul]).ok()
}

/// Status reply sent back to the requester.
pub fn encode_status(phase: u8, result: u8) -> [u8; 3] {
    [OP_STATUS, phase, result]
}

/// Encode a bounded status reply for snapshot success.
pub fn encode_snapshot_ok() -> [u8; 3] {
    encode_status(SNAPSHOT_STATUS_PHASE, STATUS_OK)
}

/// Encode a bounded status reply for snapshot mechanism unavailability.
pub fn encode_snapshot_unavailable() -> [u8; 3] {
    encode_status(SNAPSHOT_STATUS_PHASE, STATUS_UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::{HotswapRequest, SnapshotRequest, HOTSWAP_REQUEST_LEN, OP_HOTSWAP, OP_SNAPSHOT};

    #[test]
    fn parse_accepts_canonical_service_request() {
        let request = request_bytes("hotswap-demo", "/bin/hotswap-demo-v2");
        let parsed = HotswapRequest::parse(&request).expect("canonical request parses");
        assert_eq!(parsed.service_name(), "hotswap-demo");
        assert_eq!(parsed.elf_path(), "/bin/hotswap-demo-v2");
    }

    #[test]
    fn parse_rejects_non_zero_padding_after_service_nul() {
        let mut request = request_bytes("hotswap-demo", "/bin/hotswap-demo-v2");
        request[1 + "hotswap-demo".len() + 1] = b'x';
        assert!(HotswapRequest::parse(&request).is_none());
    }

    #[test]
    fn parse_rejects_non_service_names() {
        let request = request_bytes("hotswap/demo", "/bin/hotswap-demo-v2");
        assert!(HotswapRequest::parse(&request).is_none());
    }

    #[test]
    fn parse_rejects_non_bin_paths() {
        for path in ["/bin/demo/next", "/bin/../demo", "/bin//demo", "/sbin/demo"] {
            let request = request_bytes("hotswap-demo", path);
            assert!(
                HotswapRequest::parse(&request).is_none(),
                "{path} must fail"
            );
        }
    }

    #[test]
    fn snapshot_parse_accepts_opcode_and_zero_padding() {
        let mut request = [0u8; 8];
        request[0] = OP_SNAPSHOT;
        assert!(SnapshotRequest::parse(&request).is_some());
    }

    #[test]
    fn snapshot_parse_rejects_non_zero_payload() {
        let mut request = [0u8; 8];
        request[0] = OP_SNAPSHOT;
        request[3] = 1;
        assert!(SnapshotRequest::parse(&request).is_none());
    }

    #[test]
    fn snapshot_parse_rejects_wrong_opcode() {
        assert!(SnapshotRequest::parse(&[OP_HOTSWAP]).is_none());
    }

    fn request_bytes(service_name: &str, elf_path: &str) -> [u8; HOTSWAP_REQUEST_LEN] {
        let mut request = [0u8; HOTSWAP_REQUEST_LEN];
        request[0] = OP_HOTSWAP;
        request[1..1 + service_name.len()].copy_from_slice(service_name.as_bytes());
        let path_offset = 1 + super::SVC_NAME_LEN;
        request[path_offset..path_offset + elf_path.len()].copy_from_slice(elf_path.as_bytes());
        request
    }
}
