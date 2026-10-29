//! The PTY escape (`~.`, `~~` at line start) and the local status of each ending.
use super::*;
use vissh::args::Host;
use vissh::client::HostKey;
use vissh::session::{Dispatch, Ending, Outcome, Trust};

fn feed(esc: &mut Escape, input: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let quit = esc.feed(input, &mut out);
    (out, quit)
}

#[test]
fn escape_only_at_line_start() {
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"a~.b"), (b"a~.b".to_vec(), false));
    assert_eq!(feed(&mut esc, b"\r~~x"), (b"\r~x".to_vec(), false));
    assert_eq!(feed(&mut esc, b"\n~x"), (b"\n~x".to_vec(), false));
    // A tilde split across reads is still the escape; `~.` stops before its own bytes.
    assert_eq!(feed(&mut esc, b"\n~"), (b"\n".to_vec(), false));
    assert_eq!(feed(&mut esc, b".rest"), (Vec::new(), true));
    // At the very start of input, and Ctrl-C passes through as a byte.
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"\x03~."), (b"\x03~.".to_vec(), false));
    let mut esc = Escape::default();
    assert_eq!(feed(&mut esc, b"~"), (Vec::new(), false));
    let mut out = Vec::new();
    esc.flush(&mut out);
    assert_eq!(out, b"~");
}

fn outcome(dispatch: Dispatch, ending: Ending, complete: bool) -> Outcome {
    Outcome { dispatch, ending, stdout_complete: complete, stderr_complete: complete, rekeys: 0, peaks: Default::default() }
}

#[test]
fn status_and_message_per_ending() {
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let host = Host::Name("h".into());
    let o = |ending| outcome(Dispatch::Accepted, ending, true);
    assert_eq!(report(&o(Ending::RemoteExit(7)), &host, 22), (7, None));
    assert_eq!(report(&o(Ending::RemoteExit(256)), &host, 22).0, 255);
    assert_eq!(report(&o(Ending::RemoteSignal("TERM".into())), &host, 22).0, 255);
    assert_eq!(report(&o(Ending::TimedOut), &host, 22).0, 124);
    let (st, msg) = report(&o(Ending::HostKey(Trust::Mismatch, key.clone())), &host, 2222);
    assert_eq!(st, 255);
    assert!(msg.unwrap().contains(&key.fingerprint()));
    for ending in [Ending::Auth, Ending::Unknown, Ending::Disconnected, Ending::RequestRefused, Ending::TransportLost] {
        assert_eq!(report(&o(ending), &host, 22).0, 255);
    }
    // Transport loss after dispatch never claims the command did not run.
    assert_eq!(report(&o(Ending::TransportLost), &host, 22).1.as_deref(), Some("connection lost; the command may have run"));
}

#[test]
fn completion_records_carry_dispatch_uncertainty_and_exact_remote_outcomes() {
    use super::{completion, settle, CANCELLED};
    use api::services::ssh_completion::{Dispatch as D, Mode, Outcome as O, Phase as P};
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let rec = |dispatch, ending, complete, phase| completion(&outcome(dispatch, ending, complete), Mode::Exec, phase, false);
    // Exact u32 status and bare signal name are kept; the phase is complete.
    let exit = rec(Dispatch::Accepted, Ending::RemoteExit(300), false, P::Running);
    assert_eq!((exit.phase, exit.outcome, exit.remote_status, exit.stdout_complete, exit.error_code), (P::Complete, O::RemoteExit, Some(300), false, None));
    let sig = rec(Dispatch::Accepted, Ending::RemoteSignal("TERM".into()), true, P::Running);
    assert_eq!((sig.outcome, sig.remote_signal.map(|s| String::from(s.as_str())), sig.remote_status), (O::RemoteSignal, Some("TERM".into()), None));
    // Transport loss: a local error before dispatch, unknown once the request may have run.
    for (dispatch, expect, d) in [
        (Dispatch::NotSent, O::LocalError, D::NotSent), (Dispatch::MayHaveRun, O::Unknown, D::MayHaveRun), (Dispatch::Accepted, O::Unknown, D::Accepted),
    ] {
        let r = rec(dispatch, Ending::TransportLost, false, P::Request);
        assert_eq!((r.outcome, r.dispatch, r.phase, r.error_code.map(|c| String::from(c.as_str()))), (expect, d, P::Request, Some("transport_lost".into())));
    }
    // Timeouts keep the phase that ran out; host-key and auth refusals name theirs.
    let t = rec(Dispatch::Accepted, Ending::TimedOut, false, P::Running);
    assert_eq!((t.outcome, t.phase, t.dispatch), (O::Timeout, P::Running, D::Accepted));
    let hk = rec(Dispatch::NotSent, Ending::HostKey(Trust::Mismatch, key), false, P::Trust);
    assert_eq!((hk.outcome, hk.phase, hk.error_code.map(|c| String::from(c.as_str()))), (O::LocalError, P::Trust, Some("host_key_mismatch".into())));
    // Local session loss wins over whatever the transport did.
    let lost = completion(&outcome(Dispatch::Accepted, Ending::Disconnected, false), Mode::Pty, P::Running, true);
    assert_eq!((lost.outcome, lost.phase, lost.mode), (O::SessionLost, P::Running, Mode::Pty));
    // The shell: a cancelled child keeps its last reported phase and dispatch.
    let snapshot = super::progress_record(Mode::Exec, P::Running, Dispatch::Accepted);
    let cancelled = settle(Some(snapshot), CANCELLED, Mode::Exec);
    assert_eq!((cancelled.outcome, cancelled.phase, cancelled.dispatch), (O::Cancelled, P::Running, D::Accepted));
    assert_eq!(settle(Some(exit), 255, Mode::Exec), exit);
    let none = settle(None, 127, Mode::Exec);
    assert_eq!((none.phase, none.outcome, none.dispatch), (P::Admission, O::LocalError, D::NotSent));
}

#[test]
fn first_trust_answers_map_to_session_outcomes_and_messages() {
    use super::{commit_trust, trust_added, NO_FRONTEND};
    use api::ipc::ssh_trust;
    // Only a stored key continues; an undecided wait that ran out is an expiry.
    assert_eq!(commit_trust(ssh_trust::KNOWN_EXACT), Trust::KnownExact);
    for (code, trust) in [
        (ssh_trust::MISMATCH, Trust::Mismatch), (ssh_trust::DENIED, Trust::Denied), (ssh_trust::REFUSED, Trust::Refused),
        (ssh_trust::EXPIRED, Trust::Expired), (ssh_trust::PENDING, Trust::Expired),
        (ssh_trust::UNAVAILABLE, Trust::Unavailable), (ssh_trust::UNKNOWN, Trust::Unavailable),
    ] {
        assert_eq!(commit_trust(code), trust, "code {code}");
    }
    assert_eq!(trust_added("gateway", 2222), "Host 'gateway' (2222) added to the trust store (synced, not crash-proof).");
    let key = HostKey { algorithm: "ssh-ed25519", key: [1; 32] };
    let host = Host::Name("gateway".into());
    let o = |trust| outcome(Dispatch::NotSent, Ending::HostKey(trust, key.clone()), false);
    assert_eq!(report(&o(Trust::NoConfirmation), &host, 22), (255, Some(String::from(NO_FRONTEND))));
    assert_eq!(report(&o(Trust::Expired), &host, 22).1.as_deref(), Some("host key confirmation for gateway:22 timed out; not trusted"));
    let refused = report(&o(Trust::Refused), &host, 22).1.unwrap();
    assert!(refused.contains("gateway:22") && refused.contains(&key.fingerprint()) && refused.ends_with("refused"), "{refused}");
}
