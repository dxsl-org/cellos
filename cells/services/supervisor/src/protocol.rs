//! Parsing and encoding of the Supervisor Cell's requests and status replies.
//! The bytes and frame layouts are owned by [`wire::proto::supervisor`].
//!
//! Snapshot is global whole-RAM state, so the request is opcode-only. The App
//! SDK hands the supervisor a zero-padded receive buffer; snapshot and power
//! parsing is deliberately strict and accepts only the fixed bytes followed
//! by all-zero padding.
//!
//! `PowerRequest` is supervisor-private wire (owner decision H-S2-7), not
//! Law-1 ABI. Its action byte is the `SystemPower` (436) a0 value. Two senders
//! exist: the `/bin/shutdown` worker and the kernel's ACPI power-button relay
//! (sender 0, `kernel/src/acpi/sci.rs`, poweroff only).

use ostd::syscall::SystemPowerAction;
use wire::proto::supervisor::{
    HOTSWAP_REQUEST_LEN, OP_HOTSWAP, OP_POWER, OP_SNAPSHOT, OP_STATUS, PHASE_SNAPSHOT,
    POWER_ACTION_POWEROFF, POWER_ACTION_REBOOT, POWER_REQUEST_LEN, SNAPSHOT_REQUEST_LEN, STATUS_OK,
    STATUS_REPLY_LEN, STATUS_UNAVAILABLE, SVC_NAME_LEN,
};

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

/// Parse a `PowerRequest`: opcode, action byte, then all-zero padding.
pub fn parse_power_request(buf: &[u8]) -> Option<SystemPowerAction> {
    if buf.len() < POWER_REQUEST_LEN || buf[0] != OP_POWER {
        return None;
    }
    if buf[POWER_REQUEST_LEN..].iter().any(|&byte| byte != 0) {
        return None;
    }
    match buf[1] {
        POWER_ACTION_REBOOT => Some(SystemPowerAction::Reboot),
        POWER_ACTION_POWEROFF => Some(SystemPowerAction::PowerOff),
        _ => None,
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
pub fn encode_status(phase: u8, result: u8) -> [u8; STATUS_REPLY_LEN] {
    [OP_STATUS, phase, result]
}

/// Encode a bounded status reply for snapshot success.
pub fn encode_snapshot_ok() -> [u8; STATUS_REPLY_LEN] {
    encode_status(PHASE_SNAPSHOT, STATUS_OK)
}

/// Encode a bounded status reply for snapshot mechanism unavailability.
pub fn encode_snapshot_unavailable() -> [u8; STATUS_REPLY_LEN] {
    encode_status(PHASE_SNAPSHOT, STATUS_UNAVAILABLE)
}

#[cfg(test)]
mod tests {
    use super::{parse_power_request, HotswapRequest, SnapshotRequest};
    use ostd::syscall::SystemPowerAction;
    use wire::proto::app_envelope::HEADER_LEN;
    use wire::proto::supervisor::{
        HOTSWAP_REQUEST_LEN, OP_HOTSWAP, OP_POWER, OP_SNAPSHOT, POWER_ACTION_POWEROFF,
        POWER_ACTION_REBOOT, POWER_BUTTON_REQUEST, SVC_NAME_LEN,
    };

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

    #[test]
    fn power_request_carries_the_436_action_and_nothing_else() {
        let mut request = [0u8; 8];
        request[0] = OP_POWER;
        request[1] = POWER_ACTION_REBOOT;
        assert_eq!(
            parse_power_request(&request),
            Some(SystemPowerAction::Reboot)
        );
        request[1] = POWER_ACTION_POWEROFF;
        assert_eq!(
            parse_power_request(&request),
            Some(SystemPowerAction::PowerOff)
        );
        for action in [0, 3, 0xFF] {
            request[1] = action;
            assert_eq!(parse_power_request(&request), None);
        }
        request[1] = POWER_ACTION_POWEROFF;
        request[5] = 1;
        assert_eq!(parse_power_request(&request), None);
        assert_eq!(parse_power_request(&[OP_POWER]), None);
        assert_eq!(
            parse_power_request(&[OP_SNAPSHOT, POWER_ACTION_POWEROFF]),
            None
        );
    }

    /// The kernel's power-button relay sends these bytes (`acpi/sci.rs`).
    #[test]
    fn kernel_power_button_bytes_parse_as_poweroff() {
        assert_eq!(
            parse_power_request(&POWER_BUTTON_REQUEST[HEADER_LEN..]),
            Some(SystemPowerAction::PowerOff)
        );
    }

    fn request_bytes(service_name: &str, elf_path: &str) -> [u8; HOTSWAP_REQUEST_LEN] {
        let mut request = [0u8; HOTSWAP_REQUEST_LEN];
        request[0] = OP_HOTSWAP;
        request[1..1 + service_name.len()].copy_from_slice(service_name.as_bytes());
        let path_offset = 1 + SVC_NAME_LEN;
        request[path_offset..path_offset + elf_path.len()].copy_from_slice(elf_path.as_bytes());
        request
    }
}
