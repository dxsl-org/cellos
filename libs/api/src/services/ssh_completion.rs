// SPDX-License-Identifier: Apache-2.0
//! `cellbox.ssh.completion` v1 (command-contracts §4.3): the `cellbox ssh` lifecycle
//! record, carried from the ssh Cell to the shell over the foreground child's write-only
//! completion endpoint (child-context slot 5).
//!
//! The child writes one fixed [`FRAME_LEN`]-byte frame at every phase or dispatch change
//! (write-ahead: `may_have_run` before the request frame exists) and a final one; the
//! newest whole frame wins ([`last_frame`]). Nonsecret lifecycle metadata only: no
//! credential, key byte or command text.
//!
//! Frame: `version:u8=1, mode:u8, phase:u8, dispatch:u8, outcome:u8, flags:u8`
//! (bit 0 stdout_complete, bit 1 stderr_complete, bit 2 remote_status present),
//! `signal_len:u8, error_len:u8, remote_status:u32 LE, signal:[u8;32], error:[u8;32],
//! reserved:[u8;4]=0`. A text field of length 0 is null; text is graphic ASCII.
use alloc::format;
use alloc::string::String;

pub const SCHEMA: &str = "cellbox.ssh.completion";
pub const VERSION: u8 = 1;
pub const FRAME_LEN: usize = 80;
/// Longest signal name or error code.
pub const TEXT_MAX: usize = 32;

macro_rules! wire_names {
    ($name:ident { $($variant:ident = $value:literal => $text:literal),+ $(,)? }) => {
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($variant = $value),+ }
        impl $name {
            pub fn name(self) -> &'static str { match self { $(Self::$variant => $text),+ } }
            fn from_wire(value: u8) -> Option<Self> { match value { $($value => Some(Self::$variant),)+ _ => None } }
        }
    };
}

wire_names!(Mode { Pty = 0 => "pty", Exec = 1 => "exec" });
wire_names!(Phase {
    Admission = 0 => "admission", Connect = 1 => "connect", Trust = 2 => "trust", Auth = 3 => "auth",
    Request = 4 => "request", Running = 5 => "running", Complete = 6 => "complete",
});
wire_names!(Dispatch { NotSent = 0 => "not_sent", MayHaveRun = 1 => "may_have_run", Accepted = 2 => "accepted" });
wire_names!(Outcome {
    RemoteExit = 0 => "remote_exit", RemoteSignal = 1 => "remote_signal", LocalError = 2 => "local_error",
    Timeout = 3 => "timeout", Cancelled = 4 => "cancelled", SessionLost = 5 => "session_lost", Unknown = 6 => "unknown",
});

/// A bounded graphic-ASCII text (SSH signal name or error code).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    len: u8,
    bytes: [u8; TEXT_MAX],
}

impl Text {
    /// `None` for an empty, oversized or non-graphic-ASCII value.
    pub fn new(text: &str) -> Option<Self> {
        let raw = text.as_bytes();
        if raw.is_empty() || raw.len() > TEXT_MAX || !raw.iter().all(u8::is_ascii_graphic) {
            return None;
        }
        let mut bytes = [0; TEXT_MAX];
        bytes[..raw.len()].copy_from_slice(raw);
        Some(Self { len: raw.len() as u8, bytes })
    }
    pub fn as_str(&self) -> &str {
        // Validated graphic ASCII on construction and decode.
        core::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompletionV1 {
    pub mode: Mode,
    pub phase: Phase,
    pub dispatch: Dispatch,
    pub outcome: Outcome,
    pub remote_status: Option<u32>,
    pub remote_signal: Option<Text>,
    pub error_code: Option<Text>,
    pub stdout_complete: bool,
    pub stderr_complete: bool,
}

fn text_field(text: Option<Text>, out: &mut [u8]) -> u8 {
    let Some(text) = text else { return 0 };
    out[..text.len as usize].copy_from_slice(&text.bytes[..text.len as usize]);
    text.len
}

fn text_decode(len: u8, field: &[u8]) -> Result<Option<Text>, ()> {
    let len = len as usize;
    if len > TEXT_MAX || field[len..].iter().any(|b| *b != 0) {
        return Err(());
    }
    if len == 0 {
        return Ok(None);
    }
    let text = core::str::from_utf8(&field[..len]).map_err(|_| ())?;
    Text::new(text).map(Some).ok_or(())
}

impl CompletionV1 {
    pub fn encode(&self) -> [u8; FRAME_LEN] {
        let mut f = [0u8; FRAME_LEN];
        f[0] = VERSION;
        f[1] = self.mode as u8;
        f[2] = self.phase as u8;
        f[3] = self.dispatch as u8;
        f[4] = self.outcome as u8;
        f[5] = u8::from(self.stdout_complete) | u8::from(self.stderr_complete) << 1 | u8::from(self.remote_status.is_some()) << 2;
        f[6] = text_field(self.remote_signal, &mut f[12..44]);
        f[7] = text_field(self.error_code, &mut f[44..76]);
        f[8..12].copy_from_slice(&self.remote_status.unwrap_or(0).to_le_bytes());
        f
    }

    pub fn decode(f: &[u8]) -> Result<Self, ()> {
        if f.len() != FRAME_LEN || f[0] != VERSION || f[5] & !7 != 0 || f[76..].iter().any(|b| *b != 0) {
            return Err(());
        }
        let status = u32::from_le_bytes([f[8], f[9], f[10], f[11]]);
        if f[5] & 4 == 0 && status != 0 {
            return Err(());
        }
        Ok(Self {
            mode: Mode::from_wire(f[1]).ok_or(())?,
            phase: Phase::from_wire(f[2]).ok_or(())?,
            dispatch: Dispatch::from_wire(f[3]).ok_or(())?,
            outcome: Outcome::from_wire(f[4]).ok_or(())?,
            remote_status: (f[5] & 4 != 0).then_some(status),
            remote_signal: text_decode(f[6], &f[12..44])?,
            error_code: text_decode(f[7], &f[44..76])?,
            stdout_complete: f[5] & 1 != 0,
            stderr_complete: f[5] & 2 != 0,
        })
    }

    /// The record as one `key=value` line, fields in schema order.
    pub fn render(&self) -> String {
        fn text(t: &Option<Text>) -> &str { t.as_ref().map_or("null", Text::as_str) }
        let status = self.remote_status.map_or(String::from("null"), |n| format!("{n}"));
        format!(
            "schema={SCHEMA} schema_version={VERSION} mode={} phase={} dispatch={} outcome={} remote_status={status} \
remote_signal={} error_code={} stdout_complete={} stderr_complete={}",
            self.mode.name(), self.phase.name(), self.dispatch.name(), self.outcome.name(),
            text(&self.remote_signal), text(&self.error_code), self.stdout_complete, self.stderr_complete,
        )
    }
}

/// The newest whole, valid frame in `bytes` (a partial trailing frame is ignored).
pub fn last_frame(bytes: &[u8]) -> Option<CompletionV1> {
    bytes.chunks_exact(FRAME_LEN).rev().find_map(|f| CompletionV1::decode(f).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> CompletionV1 {
        CompletionV1 {
            mode: Mode::Exec, phase: Phase::Running, dispatch: Dispatch::Accepted, outcome: Outcome::RemoteExit,
            remote_status: Some(300), remote_signal: None, error_code: Text::new("transport_lost"),
            stdout_complete: false, stderr_complete: true,
        }
    }

    #[test]
    fn frames_round_trip_and_the_newest_whole_frame_wins() {
        let first = CompletionV1 { phase: Phase::Request, dispatch: Dispatch::MayHaveRun, outcome: Outcome::Unknown, remote_status: None, error_code: None, ..record() };
        let mut ring = alloc::vec::Vec::new();
        ring.extend_from_slice(&first.encode());
        assert_eq!(last_frame(&ring), Some(first));
        ring.extend_from_slice(&record().encode());
        // A torn trailing frame (the writer died mid-write) never replaces a whole one.
        ring.extend_from_slice(&record().encode()[..40]);
        assert_eq!(last_frame(&ring), Some(record()));
        assert_eq!(last_frame(&[]), None);
        assert_eq!(
            record().render(),
            "schema=cellbox.ssh.completion schema_version=1 mode=exec phase=running dispatch=accepted outcome=remote_exit \
remote_status=300 remote_signal=null error_code=transport_lost stdout_complete=false stderr_complete=true"
        );
    }

    #[test]
    fn malformed_frames_are_refused() {
        let good = record().encode();
        let mut cases = alloc::vec::Vec::new();
        for (at, value) in [(0, 2), (1, 2), (2, 7), (3, 3), (4, 7), (5, 8), (7, 33), (79, 1)] {
            let mut f = good;
            f[at] = value;
            cases.push(f);
        }
        let mut control = good;
        control[44] = 0x1b; // ESC inside the error code
        cases.push(control);
        let mut stale_status = good;
        stale_status[5] &= !4; // a status value without its presence bit
        cases.push(stale_status);
        for (n, f) in cases.iter().enumerate() {
            assert_eq!(CompletionV1::decode(f), Err(()), "case {n}");
        }
        assert_eq!(Text::new("SIG TERM"), None);
        assert_eq!(Text::new(&"a".repeat(TEXT_MAX + 1)), None);
    }
}
