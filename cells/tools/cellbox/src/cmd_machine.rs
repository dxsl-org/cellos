//! `machine pin [--confirm]`: the console client of init's machine pin (ADR-0029 §3,
//! the ADR-0028 CLI pattern). The shell holds no pin authority: it asks init over
//! `service::MACHINE_PIN` with the [`api::services::machine_pin`] frame and prints the
//! answer. Init honours `Confirm` only in safe mode and only from the console shell it
//! spawned, identified by the kernel-attested sender tid, so the request is sent from
//! this task: the builtin runs in process, never as a spawned child.
//!
//! Output: init's answers on stdout (a refusal too, so it follows the `current=` line);
//! usage and transport failures on stderr. Exit status: 0 on a status report or a
//! recorded pin, 1 on a refusal or a failure, 2 on a usage error.

use alloc::format;
use alloc::string::String;

use api::services::machine_pin::{MachinePinRequest, MachinePinResponse, PinState, Refusal};
use ostd::ipc::{IpcTerminal, PendingCall};

pub const USAGE: &str = "usage: machine pin [--confirm]";
const USAGE_STATUS: i32 = 2;
const FAILED_STATUS: i32 = 1;
/// Caller budget per request: 4 rounds of 50 scheduler ticks (2 s in total).
const ROUND_TICKS: u64 = 50;
const ROUNDS: usize = 4;

/// One accepted command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Invocation {
    /// `machine pin`: report the gate state and both digests.
    Status,
    /// `machine pin --confirm`: re-pin the current facts.
    Confirm,
}

/// The words after `machine`; `None` for every other shape (a usage error).
fn parse<S: AsRef<str>>(args: &[S]) -> Option<Invocation> {
    let mut words = args.iter().map(AsRef::<str>::as_ref);
    let invocation = match (words.next(), words.next()) {
        (Some("pin"), None) => Invocation::Status,
        (Some("pin"), Some("--confirm")) => Invocation::Confirm,
        _ => return None,
    };
    words.next().is_none().then_some(invocation)
}

/// Why no answer from init can be printed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// No live `MACHINE_PIN` provider.
    Unavailable,
    /// Submission failed, the call did not settle with a reply, or the budget ran out.
    NoReply,
    /// The reply is not a valid frame, or not the kind this request expects.
    Malformed,
}

impl Failure {
    const fn line(self) -> &'static str {
        match self {
            Self::Unavailable => "machine pin: init endpoint unavailable",
            Self::NoReply => "machine pin: no reply from init",
            Self::Malformed => "machine pin: malformed reply",
        }
    }
}

/// 64 lowercase hex digits.
fn hex(digest: &[u8; 32]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

/// The pinned digest, or `none` when init has no readable pin (all zero).
fn pinned_text(pinned: &[u8; 32]) -> String {
    if pinned.iter().all(|byte| *byte == 0) {
        String::from("none")
    } else {
        hex(pinned)
    }
}

fn status_line(state: PinState, current: &[u8; 32], pinned: &[u8; 32]) -> String {
    format!(
        "machine pin: state={} current={} pinned={}",
        state.name(),
        hex(current),
        pinned_text(pinned)
    )
}

fn current_line(current: &[u8; 32]) -> String {
    format!("machine pin: current={}", hex(current))
}

fn recorded_line(digest: &[u8; 32]) -> String {
    format!(
        "machine pin: recorded {}; reboot to start services",
        hex(digest)
    )
}

fn refused_line(refusal: Refusal) -> String {
    format!("machine pin: refused {}", refusal.name())
}

/// `machine pin [--confirm]` (see the module docs for output and status).
pub fn cmd_machine(args: &[String]) -> i32 {
    let Some(invocation) = parse(args) else {
        crate::executor::shell_stderrln(USAGE);
        return USAGE_STATUS;
    };
    match run(invocation) {
        Ok(status) => status,
        Err(failure) => {
            crate::executor::shell_stderrln(failure.line());
            FAILED_STATUS
        }
    }
}

fn run(invocation: Invocation) -> Result<i32, Failure> {
    let init = ostd::syscall::sys_lookup_service(api::syscall::service::MACHINE_PIN)
        .ok_or(Failure::Unavailable)?;
    let MachinePinResponse::Status {
        state,
        current,
        pinned,
    } = call(init, MachinePinRequest::Status)?
    else {
        return Err(Failure::Malformed);
    };
    if invocation == Invocation::Status {
        crate::executor::shell_println(&status_line(state, &current, &pinned));
        return Ok(0);
    }
    crate::executor::shell_println(&current_line(&current));
    match call(init, MachinePinRequest::Confirm { digest: current })? {
        MachinePinResponse::Recorded { digest } => {
            crate::executor::shell_println(&recorded_line(&digest));
            Ok(0)
        }
        MachinePinResponse::Refused(refusal) => {
            crate::executor::shell_println(&refused_line(refusal));
            Ok(FAILED_STATUS)
        }
        MachinePinResponse::Status { .. } => Err(Failure::Malformed),
    }
}

/// One bounded exchange with init from this task. The reply buffer holds any reply the
/// kernel can return, so a terminal is never retained for want of space; the frame
/// decoder refuses one longer than a response. An unsettled call is cancelled and its
/// terminal taken, releasing the slot; a reply that wins the cancel is still used.
fn call(init: usize, request: MachinePinRequest) -> Result<MachinePinResponse, Failure> {
    let pending = PendingCall::submit(init, &request.encode()).map_err(|_| Failure::NoReply)?;
    let mut reply = [0u8; api::ipc::IPC_BUF_SIZE];
    let completion = match pending.wait_and_take(&mut reply, ROUND_TICKS, ROUNDS) {
        Ok(Some(completion)) => completion,
        Ok(None) | Err(_) => {
            let _ = pending.cancel();
            match pending.try_take(&mut reply) {
                Ok(Some(completion)) => completion,
                Ok(None) | Err(_) => return Err(Failure::NoReply),
            }
        }
    };
    if completion.terminal != IpcTerminal::Reply {
        return Err(Failure::NoReply);
    }
    MachinePinResponse::decode(&reply[..completion.len]).ok_or(Failure::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_and_confirm() {
        assert_eq!(parse(&["pin"]), Some(Invocation::Status));
        assert_eq!(parse(&["pin", "--confirm"]), Some(Invocation::Confirm));
    }

    #[test]
    fn every_other_shape_is_a_usage_error() {
        let bad: [&[&str]; 8] = [
            &[],
            &["foo"],
            &["--confirm"],
            &["pin", "--bogus"],
            &["pin", "confirm"],
            &["pin", "--confirm", "extra"],
            &["pin", "extra"],
            &["PIN"],
        ];
        for args in bad {
            assert_eq!(parse(args), None, "{args:?}");
        }
        assert_eq!(USAGE, "usage: machine pin [--confirm]");
    }

    #[test]
    fn hex_is_64_lowercase_digits() {
        let mut digest = [0u8; 32];
        digest[0] = 0xAB;
        digest[31] = 0x0F;
        let text = hex(&digest);
        assert_eq!(text.len(), 64);
        assert!(text.starts_with("ab00"));
        assert!(text.ends_with("000f"));
        assert_eq!(hex(&[0xff; 32]), "f".repeat(64));
    }

    #[test]
    fn zero_pin_prints_none() {
        assert_eq!(pinned_text(&[0; 32]), "none");
        let mut pinned = [0u8; 32];
        pinned[31] = 1;
        assert_eq!(pinned_text(&pinned), format!("{}01", "0".repeat(62)));
    }

    #[test]
    fn lines_are_exact() {
        let current = [0x11; 32];
        let ones = "11".repeat(32);
        assert_eq!(
            status_line(PinState::Mismatch, &current, &[0; 32]),
            format!("machine pin: state=mismatch current={ones} pinned=none")
        );
        assert_eq!(
            status_line(PinState::Match, &current, &current),
            format!("machine pin: state=match current={ones} pinned={ones}")
        );
        assert_eq!(
            current_line(&current),
            format!("machine pin: current={ones}")
        );
        assert_eq!(
            recorded_line(&current),
            format!("machine pin: recorded {ones}; reboot to start services")
        );
        assert_eq!(
            refused_line(Refusal::NotSafeMode),
            "machine pin: refused NotSafeMode"
        );
        assert_eq!(
            Failure::Unavailable.line(),
            "machine pin: init endpoint unavailable"
        );
        assert_eq!(Failure::NoReply.line(), "machine pin: no reply from init");
        assert_eq!(Failure::Malformed.line(), "machine pin: malformed reply");
    }
}
