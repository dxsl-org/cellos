//! Tier-3 guest endpoint lane (`261110-0900-tier3-guest-endpoint`, S10-11): the `ssh` checks of
//! the parent module run against OpenSSH inside the nested Linux guest, reached through the
//! one loopback `hostfwd` of `scripts/qemu-tier3-ssh-guest-x86.sh` at `10.0.2.2:PORT`.
//! Built only with `--features guest`.
//!
//! Readiness (T3-C2): the guest's sshd comes up long after the shell does (nested TCG, no
//! virtio-rng), so the lane waits for the route before any scenario. The probe is the
//! client's own changed-key refusal: `host` has a deliberately wrong stored key for
//! `host:PORT`, so the client connects, reads the server's host key, and refuses before any
//! credential with a message that names the key it was shown. The route is ready when that
//! message carries exactly the fingerprint the lane recorded from the guest's host key
//! (`/p09/ssh.conf`). No password is ever typed here.
//!
//! Scenarios (T3-D, after the reused password scenarios of the parent module; the S9-6 group
//! needs the Docker fixture's hooks and sinks and is not run). They use the guest-side helper
//! `p9` of `scripts/prepare-tier3-sshd-initramfs-x86.sh` and the guest's own sshd as the only
//! fault injector, so the cell client is the unmodified product `cellbox`:
//! - `ssh-guest-cancel-producer`: an endless guest producer (`p9 produce`) is cancelled from the
//!   client; a second login proves the guest process is gone ("producer traffic stops on
//!   consumer close").
//! - `ssh-guest-fault-may-have-run`: `p9 selfkill` writes one ledger line and kills its own sshd
//!   session process; the client reports exit 255 and a `completion` record with outcome
//!   `unknown`, and does not retry.
//! - `ssh-guest-no-replay-ledger`: a later login reads `/tmp/p09-ledger`: exactly one
//!   `p09x-selfkill` line.

use super::*;

/// Total wait for the guest's sshd (the T3-B window: 60 minutes).
const READY_MS: u64 = 3_600_000;
/// Pause between probes.
const RETRY_MS: u64 = 10_000;

/// Wait until a client connection through `10.0.2.2:PORT` shows the guest's host key.
pub(super) fn wait_ready(a: &mut Term, f: &Fixture) -> bool {
    let end = now_ms() + READY_MS;
    let mut attempts: u32 = 0;
    while now_ms() < end {
        attempts += 1;
        let probe = attempt(a, &format!("ssh --user tester --auth password -p {} host", f.port));
        if let Ok((st, err)) = &probe {
            if st == "st=255" && err.contains("HOST KEY CHANGED") && err.contains(&f.fp) {
                println(&format!("P04 INFO ssh-guest-ready attempts={attempts}"));
                return true;
            }
        }
        if attempts % 6 == 1 {
            println(&format!("P04 INFO ssh-guest-wait attempts={attempts}"));
        }
        sleep_ms(RETRY_MS);
    }
    println(&format!("P04 INFO ssh-guest-ready never attempts={attempts}"));
    false
}

/// Run `cmd` on the guest to the prompt (a fresh login each time); retried up to twice when the
/// command did not exit 0 (a follow-up read must not fail the scenario on a transient). Counts
/// every attempt in `logins`.
fn follow_up(t: &mut Term, f: &Fixture, cmd: &str, logins: &mut u32) -> Result<Run, String> {
    let mut last = Err(String::from("no attempt"));
    for _ in 0..3 {
        *logins += 1;
        last = case(t, "ssh-guest-follow-up", &ssh_line(f.port, &format!("10.0.2.2 --command '{cmd}'")), &f.password, SSH_MS,
            |_| Ok(()));
        if last.as_ref().is_ok_and(|run| run.st == "st=0") {
            break;
        }
        sleep_ms(2000);
    }
    last
}

/// The three guest-only scenarios, in order. Prints `P04 INFO ssh-guest-logins N` (every
/// password login the driver made here) for the judge's accounting of the guest's sshd log.
#[cfg_attr(feature = "guest-diag", allow(dead_code))]
pub(super) fn guest_checks(a: &mut Term, f: &Fixture) {
    let pw = f.password.as_str();
    let mut logins: u32 = 0;

    // 1. Producer traffic stops on consumer close.
    logins += 1;
    let r = case(a, "ssh-guest-cancel-producer", &ssh_line(f.port, "10.0.2.2 --command 'echo p09-started; p9 produce'"), pw,
        SSH_MS, |t| {
            wait_stdout(t, b"p09-started")?;
            sleep_ms(500);
            t.host.submit_key(0x63, CTRL, 1).map_err(|s| format!("ctrl-c {s:?}"))
        });
    let cancelled = r.as_ref().is_ok_and(|run| {
        run.st == "st=130"
            && run.rec_has(&[("mode", "exec"), ("phase", "running"), ("dispatch", "accepted"), ("outcome", "cancelled"),
                ("remote_status", "null"), ("remote_signal", "null")])
    });
    let gone = follow_up(a, f, "p9 gone", &mut logins);
    let gone_ok = gone.as_ref().is_ok_and(|run| run.st == "st=0" && run.out.windows(8).any(|w| w == b"p09-gone"));
    check(
        "ssh-guest-cancel-producer",
        cancelled && gone_ok,
        &format!("cancel: {} | follow-up: {}", describe(&r), describe(&gone)),
    );

    // 2. A session lost after dispatch: unknown outcome, exit 255, no automatic retry.
    logins += 1;
    let r = case(a, "ssh-guest-fault-may-have-run", &ssh_line(f.port, "10.0.2.2 --command 'p9 selfkill'"), pw, SSH_MS, |_| Ok(()));
    if let Some(Ok(rec)) = r.as_ref().ok().map(|run| &run.rec) {
        println(&format!("P04 INFO ssh-guest-selfkill dispatch={}", field(rec, "dispatch").unwrap_or("?")));
    }
    judge(
        "ssh-guest-fault-may-have-run",
        &r,
        "st=255",
        &[("mode", "exec"), ("outcome", "unknown"), ("remote_status", "null"), ("remote_signal", "null")],
        |run| {
            // The command's own `sleep 20` after the kill must not be what ended the session.
            run.rec.as_ref().is_ok_and(|rec| matches!(field(rec, "dispatch"), Some("accepted" | "may_have_run"))) && run.ms < 18_000
        },
    );

    // 3. The ledger: the dispatched command ran once; the client did not replay it.
    let ledger = follow_up(a, f, "cat /tmp/p09-ledger", &mut logins);
    let (selfkill, lines) = match &ledger {
        Ok(run) if run.st == "st=0" => {
            let text = String::from_utf8_lossy(&run.out).into_owned();
            let all: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("p09x-")).collect();
            (all.iter().filter(|l| **l == "p09x-selfkill").count(), all.len())
        }
        _ => (usize::MAX, 0),
    };
    println(&format!("P04 INFO ssh-guest-ledger selfkill={selfkill} lines={lines}"));
    check("ssh-guest-no-replay-ledger", selfkill == 1, &describe(&ledger));
    println(&format!("P04 INFO ssh-guest-logins {logins}"));
}

/// Observation offsets (ms after the `cancel-done` mark): the lane takes a host `ss` snapshot on
/// each mark, the driver then runs one guest `p9 snap` login (cp1-diag-draft.md section 3.1).
#[cfg(feature = "guest-diag")]
const SNAP_A_MS: u64 = 3_000;
#[cfg(feature = "guest-diag")]
const SNAP_B_MS: u64 = 25_000;
#[cfg(feature = "guest-diag")]
const SNAP_C_MS: u64 = 70_000;
/// Pause between a mark and the guest login that follows it: the lane's `ss` snapshot must see
/// only the connections that exist before the snap login adds its own.
#[cfg(feature = "guest-diag")]
const MARK_SETTLE_MS: u64 = 1_500;

/// A long description as several short serial lines (the `check` line is clipped at 200 chars).
#[cfg(feature = "guest-diag")]
fn detail(name: &str, text: &str) {
    let clean: Vec<char> = text.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '.' }).collect();
    for (i, part) in clean.chunks(160).enumerate() {
        println(&format!("P04 INFO ssh-guest-diag-detail {name} part={i} {}", part.iter().collect::<String>()));
    }
}

/// Print the mark, let the lane react, run `p9 snap` on a fresh guest login and print its output
/// line by line. Waits until `at_ms` after `t0` first (a mark that is already late is printed at
/// once; its real offset is in the line).
#[cfg(feature = "guest-diag")]
fn snap(a: &mut Term, f: &Fixture, name: &str, t0: u64, at_ms: u64, logins: &mut u32) {
    let target = t0 + at_ms;
    if now_ms() < target {
        sleep_ms(target - now_ms());
    }
    println(&format!("P04 INFO ssh-guest-diag-mark {name} off_ms={}", now_ms().saturating_sub(t0)));
    sleep_ms(MARK_SETTLE_MS);
    let r = follow_up(a, f, "p9 snap", logins);
    match &r {
        Ok(run) => {
            println(&format!("P04 INFO ssh-guest-snap {name} st={} ms={}", run.st, run.ms));
            for line in String::from_utf8_lossy(&run.out).lines() {
                let line = line.trim();
                if !line.is_empty() {
                    println(&format!("P04 INFO ssh-guest-snap {name} | {}", clip(line)));
                }
            }
        }
        Err(why) => println(&format!("P04 INFO ssh-guest-snap {name} error {}", clip(why))),
    }
    println(&format!("P04 INFO ssh-guest-snap {name} end"));
}

/// The T3-D2 diagnostic stage: the guest scenarios of `guest_checks` plus a positive control for a
/// cooperative close, with the cancel kept as an observation (a FAIL there is recorded data) and
/// guest snapshots after it. Prints `P04 INFO ssh-guest-logins N` as `guest_checks` does.
#[cfg(feature = "guest-diag")]
pub(super) fn diag_checks(a: &mut Term, f: &Fixture) {
    let pw = f.password.as_str();
    let mut logins: u32 = 0;

    // 1. Positive control: the client ends the session itself (`--timeout`, graceful close path);
    // the guest producer must stop. The producer's start is not checked from the client's output
    // (the flood overwrites the terminal buffer); the judge checks it from the guest's own
    // `p9 snap` (the `diag pid=` line `produce` appends) and from the process table.
    logins += 1;
    let r = case(a, "ssh-guest-consumer-close-producer",
        &ssh_line(f.port, "--timeout 8 10.0.2.2 --command 'echo p09-started; p9 produce'"), pw, SSH_MS, |_| Ok(()));
    let timed_out = r.as_ref().is_ok_and(|run| {
        run.st == "st=124"
            && run.rec_has(&[("mode", "exec"), ("phase", "running"), ("dispatch", "accepted"), ("outcome", "timeout")])
    });
    let gone = follow_up(a, f, "p9 gone", &mut logins);
    let gone_ok = gone.as_ref().is_ok_and(|run| run.st == "st=0" && run.out.windows(8).any(|w| w == b"p09-gone"));
    detail("control-client", &describe(&r));
    detail("control-follow-up", &describe(&gone));
    check("ssh-guest-consumer-close-producer", timed_out && gone_ok, &format!("timed_out={timed_out} gone_ok={gone_ok}"));

    // 2. The cancel scenario, unchanged, as an observation.
    logins += 1;
    let r = case(a, "ssh-guest-cancel-producer", &ssh_line(f.port, "10.0.2.2 --command 'echo p09-started; p9 produce'"), pw,
        SSH_MS, |t| {
            wait_stdout(t, b"p09-started")?;
            sleep_ms(500);
            t.host.submit_key(0x63, CTRL, 1).map_err(|s| format!("ctrl-c {s:?}"))
        });
    let t0 = now_ms();
    println("P04 INFO ssh-guest-diag-mark cancel-done off_ms=0");
    let cancelled = r.as_ref().is_ok_and(|run| {
        run.st == "st=130"
            && run.rec_has(&[("mode", "exec"), ("phase", "running"), ("dispatch", "accepted"), ("outcome", "cancelled"),
                ("remote_status", "null"), ("remote_signal", "null")])
    });
    // `p9 gone` retries up to three times (as in T3-D, ~60 s when the producer stays alive), so
    // snap-a and snap-b are taken before it and snap-c after it; each mark prints its real offset.
    snap(a, f, "snap-a", t0, SNAP_A_MS, &mut logins);
    snap(a, f, "snap-b", t0, SNAP_B_MS, &mut logins);
    let gone = follow_up(a, f, "p9 gone", &mut logins);
    let gone_ok = gone.as_ref().is_ok_and(|run| run.st == "st=0" && run.out.windows(8).any(|w| w == b"p09-gone"));
    detail("cancel-client", &describe(&r));
    detail("cancel-follow-up", &describe(&gone));
    check("ssh-guest-cancel-producer", cancelled && gone_ok, &format!("cancelled={cancelled} gone_ok={gone_ok}"));
    snap(a, f, "snap-c", t0, SNAP_C_MS, &mut logins);

    // 3. A session lost after dispatch: unknown outcome, exit 255, no automatic retry.
    logins += 1;
    let r = case(a, "ssh-guest-fault-may-have-run", &ssh_line(f.port, "10.0.2.2 --command 'p9 selfkill'"), pw, SSH_MS, |_| Ok(()));
    if let Some(Ok(rec)) = r.as_ref().ok().map(|run| &run.rec) {
        println(&format!("P04 INFO ssh-guest-selfkill dispatch={}", field(rec, "dispatch").unwrap_or("?")));
    }
    judge(
        "ssh-guest-fault-may-have-run",
        &r,
        "st=255",
        &[("mode", "exec"), ("outcome", "unknown"), ("remote_status", "null"), ("remote_signal", "null")],
        |run| {
            // The command's own `sleep 20` after the kill must not be what ended the session.
            run.rec.as_ref().is_ok_and(|rec| matches!(field(rec, "dispatch"), Some("accepted" | "may_have_run"))) && run.ms < 18_000
        },
    );

    // 4. The ledger: the dispatched command ran once; the client did not replay it.
    let ledger = follow_up(a, f, "cat /tmp/p09-ledger", &mut logins);
    let (selfkill, lines) = match &ledger {
        Ok(run) if run.st == "st=0" => {
            let text = String::from_utf8_lossy(&run.out).into_owned();
            let all: Vec<&str> = text.lines().map(str::trim).filter(|l| l.starts_with("p09x-")).collect();
            (all.iter().filter(|l| **l == "p09x-selfkill").count(), all.len())
        }
        _ => (usize::MAX, 0),
    };
    println(&format!("P04 INFO ssh-guest-ledger selfkill={selfkill} lines={lines}"));
    check("ssh-guest-no-replay-ledger", selfkill == 1, &describe(&ledger));
    println(&format!("P04 INFO ssh-guest-logins {logins}"));
}
