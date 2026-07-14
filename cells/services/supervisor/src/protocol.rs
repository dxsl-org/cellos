//! IPC message types for the Supervisor Cell.
//!
//! Wire format (buf[0] = discriminant):
//!   0x01 — HotswapRequest  { target_service: [u8; 64], new_elf: [u8; 128] }  → total 193 B
//!   0x02 — SnapshotRequest { target_service: [u8; 64] }                       → total  65 B
//!   0x03 — StatusReply     { phase: u8, result: u8 }                          → total   3 B

pub const OP_HOTSWAP: u8 = 0x01;
// reason: SnapshotRequest is documented in the wire-format table above but its
// handler is not yet implemented in this cell (snapshot orchestration is still
// kernel-side per docs/specs/15-kernel-boundary.md tracked tech debt).
#[allow(dead_code)]
pub const OP_SNAPSHOT: u8 = 0x02;
pub const OP_STATUS: u8 = 0x03;

pub const SVC_NAME_LEN: usize = 64;
pub const ELF_PATH_LEN: usize = 128;

/// Hotswap request from any authorized caller.
///
/// `target_service` is the null-terminated ASCII name that maps to a service ID.
/// `new_elf` is the VFS path of the replacement ELF (null-terminated).
pub struct HotswapRequest<'a> {
    pub target_service: &'a [u8; SVC_NAME_LEN],
    pub new_elf: &'a [u8; ELF_PATH_LEN],
}

impl<'a> HotswapRequest<'a> {
    /// Parse from raw IPC bytes.  Returns `None` if the buffer is too short.
    pub fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < 1 + SVC_NAME_LEN + ELF_PATH_LEN {
            return None;
        }
        if buf[0] != OP_HOTSWAP {
            return None;
        }
        // SAFETY: slices are within bounds and correctly sized.
        let target_service = buf[1..1 + SVC_NAME_LEN].try_into().ok()?;
        let new_elf = buf[1 + SVC_NAME_LEN..1 + SVC_NAME_LEN + ELF_PATH_LEN]
            .try_into()
            .ok()?;
        Some(Self {
            target_service,
            new_elf,
        })
    }

    /// Return the null-terminated service name as a &str (up to first NUL).
    pub fn service_name(&self) -> &str {
        let end = self
            .target_service
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(SVC_NAME_LEN);
        core::str::from_utf8(&self.target_service[..end]).unwrap_or("")
    }

    /// Return the VFS path as a &str (up to first NUL).
    pub fn elf_path(&self) -> &str {
        let end = self
            .new_elf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(ELF_PATH_LEN);
        core::str::from_utf8(&self.new_elf[..end]).unwrap_or("")
    }
}

/// Status reply sent back to the requester.
pub fn encode_status(phase: u8, result: u8) -> [u8; 3] {
    [OP_STATUS, phase, result]
}
