//! One call to the audio service: an exact operation submitted at one host
//! boundary and taken at a later one, abandoned after [`CALL_TIMEOUT_NS`].
//!
//! The reply lands in the operation's slot, never in the mailbox the launcher
//! session drains: the kernel routes a provider's reply to the operation it
//! serves and quarantines a late one instead of delivering it as a message.

use ostd::ipc::{IpcSubmitError, IpcTerminal, PendingCall};
use wire::audio::AudioReply;

/// How long a call may take before it is abandoned: two scheduler ticks,
/// since the driver may wait one for a CPU.
const CALL_TIMEOUT_NS: u64 = 20_000_000;

/// What a call asked for.
pub enum Call {
    Open,
    Write(u32),
}

pub enum Submitted {
    Accepted(Outstanding),
    /// Nothing was queued; try again at the next boundary.
    Busy,
    /// The provider is gone.
    Gone,
}

/// Where a call stands.
pub enum Outcome {
    Pending(Outstanding),
    /// The status the service answered with; `None` when it did not answer
    /// in time or the operation was cancelled or indeterminate.
    Answered(Call, Option<u32>),
    /// The provider died with the call outstanding.
    Gone(Call),
}

pub struct Outstanding {
    pending: PendingCall,
    call: Call,
    deadline_ns: u64,
}

impl Outstanding {
    pub fn submit(provider: usize, request: &[u8], call: Call, now_ns: u64) -> Submitted {
        match PendingCall::submit(provider, request) {
            Ok(pending) => Submitted::Accepted(Self {
                pending,
                call,
                deadline_ns: now_ns.saturating_add(CALL_TIMEOUT_NS),
            }),
            Err(IpcSubmitError::Busy) => Submitted::Busy,
            Err(IpcSubmitError::PeerGone | IpcSubmitError::InvalidRequest) => Submitted::Gone,
        }
    }

    /// The call's outcome by `now_ns`. Dropping an unanswered call cancels
    /// its operation.
    pub fn take(self, now_ns: u64) -> Outcome {
        let mut buf = [0u8; AudioReply::LEN];
        match self.pending.try_take(&mut buf) {
            Ok(None) if now_ns < self.deadline_ns => Outcome::Pending(self),
            Ok(Some(done)) if done.terminal == IpcTerminal::Reply => {
                let reply = AudioReply::decode(&buf[..done.len.min(buf.len())]);
                Outcome::Answered(self.call, reply.map(|reply| reply.status))
            }
            Ok(Some(done)) if done.terminal == IpcTerminal::PeerGone => Outcome::Gone(self.call),
            _ => Outcome::Answered(self.call, None),
        }
    }

    /// The bytes a write carries, 0 for an open.
    pub fn write_len(&self) -> u32 {
        match self.call {
            Call::Write(len) => len,
            Call::Open => 0,
        }
    }
}
