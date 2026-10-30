//! Phase 09 (`phase-09-s0-proposal.md` §3 S9-2/S9-3/S9-6): `cellbox ssh` against the
//! disposable Docker OpenSSH fixture reached through unrestricted SLIRP at 10.0.2.2.
//! Built only with the `phase09-ssh` feature, for `scripts/qemu-phase09-ssh-test.sh`.
//!
//! `/p09/ssh.conf` (`PORT SHA256:... PASSWORD [REKEY_PORT]`, written by the runner) names
//! the fixture; REKEY_PORT is the second server (`RekeyLimit 64K`, same key).
//! The fixture disk's protected trust store holds the real key for `10.0.2.2:PORT` (and
//! `10.0.2.2:REKEY_PORT`), a different key for `host:PORT` and nothing for `gateway` (both
//! names resolve to 10.0.2.2). Each check judges `$?`, exact output bytes and the stable
//! fragments the client prints. The password is typed only after its prompt and never
//! printed here.
//!
//! S9-6 runs against the lane-only `p09-faults` cellbox build. The driver cannot read the
//! kernel log, so it opens each case's serial segment with `P04 INFO p09-case NAME`; the
//! runner acts on the client's `p09-hook` lines inside it (kill the live session, pause the
//! container), undoes a pause on `P04 INFO p09-restore`, and judges the client's
//! `ssh-rekeys`/`ssh-bytes` lines and the remote ledger `/tmp/p09-ledger` (one `p09x-TAG`
//! line per remote execution: no replay). The driver judges `$?` and the shell's
//! `completion` record.

use super::*;
#[cfg(feature = "kssh")] pub(crate) mod kssh;
#[cfg(feature = "guest")] pub(crate) mod guest;

/// The guest build talks to OpenSSH inside nested TCG: the per-case deadline is raised from the
/// 120 s of the Docker fixture to 300 s. T3-B measured a full SSH exchange at 1-2 s, so this is a
/// margin of two orders of magnitude, not an expected duration.
#[cfg(not(feature = "guest"))]
const SSH_MS: u64 = 120_000;
#[cfg(feature = "guest")]
const SSH_MS: u64 = 300_000;
/// Bulk sink cases (MiB through the throttled consumer under TCG).
const BULK_MS: u64 = 900_000;
/// Request/KEX deadline cases: 30 s budgets plus the 5 s hook hold and login.
const DEADLINE_MS: u64 = 240_000;
/// The cellbox line editor accepts at most this many bytes of a typed line and refuses longer
/// ones whole (`executor::SHELL_LINE_MAX`).
const SHELL_LINE_MAX: usize = 4096;
pub(crate) const SSH_MARKERS: [&str; 34] = [
    "ssh-conf", "ssh-refusals", "ssh-changed-host-key", "ssh-first-trust-no", "ssh-first-trust-paste-refused",
    "ssh-first-trust-yes", "ssh-first-trust-known-after", "ssh-wrong-password",
    "ssh-exec-bytes", "ssh-exec-stdin-eof", "ssh-exec-signal", "ssh-pty-shell", "ssh-pty-resize",
    "ssh-pty-escape-disconnect", "ssh-shell-alive", "shell-long-line",
    "ssh-completion-auth", "ssh-completion-trust", "ssh-completion-exec", "ssh-completion-signal",
    "ssh-completion-pty", "ssh-fault-before-request", "ssh-fault-may-have-run", "ssh-fault-accepted",
    "ssh-fault-exit-before-drain", "ssh-rekey-bytes", "ssh-rekey-time", "ssh-rekey-peer",
    "ssh-backpressure-bytes", "ssh-timeout-overall", "ssh-timeout-request", "ssh-timeout-kex",
    "ssh-cancel-exec", "ssh-fault-restores",
];

struct Fixture {
    port: u16,
    fp: String,
    password: String,
    /// The `RekeyLimit 64K` server (S9-6 peer-initiated rekey).
    rekey_port: Option<u16>,
}

fn status(t: &mut Term) -> Result<String, String> {
    Ok(t.run("echo st=$?", 30_000)?.join("|"))
}

/// Run `cmd` to the prompt; report (`$?`, stderr text).
fn attempt(t: &mut Term, cmd: &str) -> Result<(String, String), String> {
    t.send(cmd)?;
    t.wait_prompt(SSH_MS)?;
    let err = t.stderr_text();
    Ok((status(t)?, err))
}

/// The shell runs a line longer than the old 128-byte limit in full, and refuses a line past
/// `SHELL_LINE_MAX` whole: nothing of it runs, and the next line runs normally.
fn shell_long_line(t: &mut Term) {
    let word = "w".repeat(300);
    let long = t.run(&format!("echo {word}"), 30_000);
    let too_long = format!("echo p09-overflow-ran {}", "v".repeat(SHELL_LINE_MAX));
    let refused = (|| {
        // One submission is bounded by the frontend; type the line in 256-byte pieces.
        t.pump();
        t.out.clear();
        t.err.clear();
        for piece in too_long.as_bytes().chunks(256) {
            let piece = core::str::from_utf8(piece).map_err(|_| String::from("ascii"))?;
            t.host.submit_text(piece).map_err(|s| format!("submit_text {s:?}"))?;
            t.pump();
        }
        t.host.submit_key(ENTER, 0, 1).map_err(|s| format!("submit_key {s:?}"))?;
        t.wait_prompt(SSH_MS)?;
        // The echoed input contains the token too; only an output line starting with it ran.
        let ran = String::from_utf8_lossy(&t.out).split(['\n', '\r']).any(|l| l.starts_with("p09-overflow-ran"));
        Ok::<_, String>((t.stderr_text(), ran))
    })();
    let after = t.run("echo p09-after-long", 30_000);
    let ok = long.as_deref() == Ok(&[word.clone()][..])
        && matches!(&refused, Ok((err, false)) if err.contains("line longer than 4096 bytes; discarded"))
        && after.as_deref() == Ok(&[String::from("p09-after-long")][..]);
    check(
        "shell-long-line",
        ok,
        &format!("long_ok={} refused={refused:?} after={after:?}", long.as_deref() == Ok(&[word][..])),
    );
}

fn wait_stderr(t: &mut Term, what: &str) -> Result<(), String> {
    let end = now_ms() + SSH_MS;
    while now_ms() < end {
        t.pump();
        if t.stderr_text().contains(what) {
            return Ok(());
        }
        let _ = sys_set_timer(1);
    }
    Err(format!("no `{what}` on stderr"))
}

fn wait_stdout(t: &mut Term, what: &[u8]) -> Result<(), String> {
    let end = now_ms() + SSH_MS;
    while now_ms() < end {
        t.pump();
        if t.out.windows(what.len()).any(|w| w == what) {
            return Ok(());
        }
        let _ = sys_set_timer(1);
    }
    Err(format!("no {:?} on stdout", String::from_utf8_lossy(what)))
}

/// The remote side wrote something (a PTY shell's prompt) after the echoed command line.
fn wait_remote_output(t: &mut Term) -> Result<(), String> {
    let end = now_ms() + SSH_MS;
    while now_ms() < end {
        t.pump();
        if !after_echo(&t.out).is_empty() {
            return Ok(());
        }
        let _ = sys_set_timer(1);
    }
    Err(String::from("no remote output"))
}

/// Command output: the transcript after the echoed command line.
fn after_echo(out: &[u8]) -> &[u8] {
    out.iter().position(|&b| b == b'\n').map_or(&[][..], |i| &out[i + 1..])
}

/// Start `ssh ... ARGS`, answer the password prompt, return without waiting further.
fn login(t: &mut Term, f: &Fixture, args: &str, password: &str) -> Result<(), String> {
    t.send(&format!("ssh --user tester --auth password -p {} {args}", f.port))?;
    wait_stderr(t, "password: ")?;
    type_keys(t, password)?;
    press(t, ENTER)
}

/// The lane's network comes up concurrently with the shell (DHCP may also restart once
/// after an early NIC timeout). Wait until the shell's `ifconfig` shows the SLIRP lease
/// before any connection; this is lane readiness, not an ssh check.
fn wait_network(a: &mut Term) -> bool {
    let end = now_ms() + 180_000;
    while now_ms() < end {
        if let Ok(lines) = a.run("ifconfig", 30_000) {
            if lines.iter().any(|l| l.contains("10.0.2.15")) {
                println(&format!("P04 INFO ssh-network-ready lines={}", lines.len()));
                return true;
            }
        }
        let _ = sys_set_timer(100);
    }
    false
}

fn read_conf(a: &mut Term) -> Option<Fixture> {
    let lines = a.run("cat /p09/ssh.conf", 30_000).ok()?;
    let mut words = lines.first()?.split(' ');
    let port: u16 = words.next()?.parse().ok()?;
    let fp = String::from(words.next()?);
    let password = String::from(words.next()?);
    let rekey_port = match words.next() {
        None => None,
        Some(word) => Some(word.parse().ok()?),
    };
    (fp.starts_with("SHA256:") && !password.is_empty()).then_some(Fixture { port, fp, password, rekey_port })
}

pub(crate) fn ssh_checks(a: &mut Term) {
    if !wait_network(a) {
        println("P04 INFO ssh-network-ready never");
    }
    let fixture = read_conf(a);
    check("ssh-conf", fixture.is_some(), "conf unreadable");
    let Some(f) = fixture else {
        return skip(&SSH_MARKERS[1..], "no fixture conf");
    };
    // Tier-3 guest lane: the guest's sshd comes up long after the shell; this is lane
    // readiness (route proof T3-C2), not an ssh check.
    #[cfg(feature = "guest")]
    if !guest::wait_ready(a, &f) {
        return skip(&SSH_MARKERS[1..], "guest route never ready");
    }

    let refusals = (|| {
        let mut out = Vec::new();
        for cmd in [
            "ssh -i key --user tester --auth password 10.0.2.2",
            "ssh --user tester 10.0.2.2",
            "ssh --user tester --auth password tester@10.0.2.2",
            "ssh --user tester --auth password 10.0.2.2 --command 'true' | cat",
        ] {
            let (st, err) = attempt(a, cmd)?;
            // No host key, no password prompt: refused before any connection.
            out.push((st == "st=2" || st == "st=1") && !err.contains("SHA256:") && !err.contains("password: "));
        }
        Ok::<Vec<bool>, String>(out)
    })();
    check("ssh-refusals", refusals.as_ref().is_ok_and(|v| v.iter().all(|ok| *ok)), &format!("{refusals:?}"));

    // A changed key is refused before any credential and never prompts.
    let r = attempt(a, &format!("ssh --user tester --auth password -p {} host", f.port));
    let ok = r.as_ref().is_ok_and(|(st, err)| {
        st == "st=255" && err.contains("HOST KEY CHANGED") && err.contains(&f.fp) && !err.contains("password")
    });
    check("ssh-changed-host-key", ok, &format!("{r:?}"));

    // First trust (S9-4): `gateway` has no store entry. The shell draws the input owner's
    // prompt with the fixture fingerprint; `no` and a pasted `yes` refuse without writing
    // anything, a typed `yes` stores it and the session continues to authentication.
    let unknown = format!("ssh --user tester --auth password -p {} gateway --command 'echo trusted-ok'", f.port);
    let prompt = |a: &mut Term| -> Result<bool, String> {
        a.send(&unknown)?;
        wait_stdout(a, b"Type 'yes' and Enter")?;
        Ok(String::from_utf8_lossy(&a.out).contains(&f.fp) && !a.stderr_text().contains("password: "))
    };
    let r = (|| {
        let shown = prompt(a)?;
        type_keys(a, "no")?;
        press(a, ENTER)?;
        a.wait_prompt(SSH_MS)?;
        let err = a.stderr_text();
        Ok::<_, String>((shown, status(a)?, err))
    })();
    let ok = r.as_ref().is_ok_and(|(shown, st, err)| *shown && st == "st=255" && err.contains("refused") && !err.contains("password: "));
    check("ssh-first-trust-no", ok, &format!("{r:?}"));
    let r = (|| {
        let shown = prompt(a)?;
        // Input refuses a paste during the capture (Cancelled) and rejects the confirmation.
        match a.host.submit_paste("yes\n") {
            Ok(()) | Err(Status::Cancelled) => {}
            Err(s) => return Err(format!("paste {s:?}")),
        }
        a.wait_prompt(SSH_MS)?;
        let err = a.stderr_text();
        Ok::<_, String>((shown, status(a)?, err))
    })();
    let ok = r.as_ref().is_ok_and(|(shown, st, err)| *shown && st == "st=255" && err.contains("refused"));
    check("ssh-first-trust-paste-refused", ok, &format!("{r:?}"));
    let r = (|| {
        let shown = prompt(a)?;
        type_keys(a, "yes")?;
        press(a, ENTER)?;
        wait_stderr(a, "password: ")?;
        type_keys(a, &f.password)?;
        press(a, ENTER)?;
        a.wait_prompt(SSH_MS)?;
        let (out, err) = (after_echo(&a.out).to_vec(), a.stderr_text());
        Ok::<_, String>((shown, status(a)?, out, err))
    })();
    let ok = r.as_ref().is_ok_and(|(shown, st, out, err)| {
        *shown && st == "st=0" && out.windows(10).any(|w| w == b"trusted-ok") && err.contains("added to the trust store")
    });
    check("ssh-first-trust-yes", ok, &format!("{:?}", r.as_ref().map(|(s, st, _, e)| (s, st, e))));
    // The stored key is now known: no prompt, straight to the password.
    let r = (|| {
        login(a, &f, "gateway --command 'echo again-ok'", &f.password)?;
        a.wait_prompt(SSH_MS)?;
        let out = after_echo(&a.out).to_vec();
        Ok::<_, String>((status(a)?, out))
    })();
    let ok = r.as_ref().is_ok_and(|(st, out)| {
        st == "st=0" && out.windows(8).any(|w| w == b"again-ok") && !out.windows(5).any(|w| w == b"Type ")
    });
    check("ssh-first-trust-known-after", ok, &format!("{:?}", r.as_ref().map(|(s, o)| (s, String::from_utf8_lossy(o).into_owned()))));

    let r = (|| {
        login(a, &f, "10.0.2.2 --command 'echo never'", "p09-wrong-password")?;
        a.wait_prompt(SSH_MS)?;
        let err = a.stderr_text();
        let out = after_echo(&a.out).to_vec();
        Ok::<_, String>((status(a)?, err, out))
    })();
    let ok = r.as_ref().is_ok_and(|(st, err, out)| {
        st == "st=255" && err.contains("authentication failed") && !out.windows(5).any(|w| w == b"never")
    });
    check("ssh-wrong-password", ok, &format!("{:?}", r.as_ref().map(|(s, e, _)| (s, e))));

    // Exact remote bytes (NUL, 0xFF), stderr kept apart, exact exit status.
    let r = (|| {
        login(a, &f, "10.0.2.2 --command 'printf \"a\\000b\\377c\"; printf E >&2; exit 7'", &f.password)?;
        a.wait_prompt(SSH_MS)?;
        let (out, err) = (after_echo(&a.out).to_vec(), a.stderr_text());
        Ok::<_, String>((status(a)?, out, err))
    })();
    let ok = r.as_ref().is_ok_and(|(st, out, err)| {
        st == "st=7" && out.windows(5).any(|w| w == b"a\0b\xffc") && !out.contains(&b'E') && err.ends_with('E')
    });
    check("ssh-exec-bytes", ok, &format!("{:?}", r.as_ref().map(|(s, o, e)| (s, String::from_utf8_lossy(o).into_owned(), e))));

    // Typed stdin reaches the remote command; Ctrl-D is one EOF; the output drains after it.
    let r = (|| {
        login(a, &f, "10.0.2.2 --command 'cat; echo done; exit 3'", &f.password)?;
        wait_stderr(a, "\n")?;
        let _ = sys_set_timer(300);
        type_keys(a, "p09-typed")?;
        press(a, ENTER)?;
        // The remote `cat` sends the typed line back (the foreground child gets the
        // text as events; nothing echoes it locally).
        wait_stdout(a, b"p09-typed\n")?;
        a.host.submit_key('d' as u32, CTRL, 1).map_err(|s| format!("ctrl-d {s:?}"))?;
        a.wait_prompt(SSH_MS)?;
        let out = a.out.clone();
        Ok::<_, String>((status(a)?, out))
    })();
    let ok = r.as_ref().is_ok_and(|(st, out)| st == "st=3" && out.windows(5).any(|w| w == b"done\n"));
    check("ssh-exec-stdin-eof", ok, &format!("{:?}", r.as_ref().map(|(s, o)| (s, String::from_utf8_lossy(o).into_owned()))));

    let r = (|| {
        login(a, &f, "10.0.2.2 --command 'kill -TERM $$'", &f.password)?;
        a.wait_prompt(SSH_MS)?;
        let err = a.stderr_text();
        Ok::<_, String>((status(a)?, err))
    })();
    let ok = r.as_ref().is_ok_and(|(st, err)| st == "st=255" && err.contains("signal TERM"));
    check("ssh-exec-signal", ok, &format!("{r:?}"));

    // PTY shell: TERM and size reach the server, a resize is sent, `~.` disconnects.
    let pty = (|| {
        login(a, &f, "10.0.2.2", &f.password)?;
        let _ = sys_set_timer(500);
        type_keys(a, "echo T=$TERM; stty size")?;
        press(a, ENTER)?;
        wait_stdout(a, b"T=cellos-vt1")?;
        wait_stdout(a, b"24 80")?;
        Ok::<(), String>(())
    })();
    check("ssh-pty-shell", pty.is_ok(), &format!("{pty:?} out={:?}", clip(&String::from_utf8_lossy(&a.out))));
    let resized = pty.clone().and_then(|()| {
        a.host.resize(30, 100).map_err(|s| format!("resize {s:?}"))?;
        let _ = sys_set_timer(500);
        type_keys(a, "stty size")?;
        press(a, ENTER)?;
        wait_stdout(a, b"30 100")?;
        a.host.resize(24, 80).map_err(|s| format!("resize {s:?}"))
    });
    check("ssh-pty-resize", resized.is_ok(), &format!("{resized:?}"));
    let quit = pty.and_then(|()| {
        press(a, ENTER)?;
        type_keys(a, "~.")?;
        a.wait_prompt(SSH_MS)?;
        let err = a.stderr_text();
        Ok::<_, String>((status(a)?, err))
    });
    let ok = quit.as_ref().is_ok_and(|(st, err)| st == "st=255" && err.contains("disconnected"));
    check("ssh-pty-escape-disconnect", ok, &format!("{quit:?}"));

    let alive = a.run("echo p09-alive", 30_000);
    check("ssh-shell-alive", alive.as_deref() == Ok(&[String::from("p09-alive")][..]), &format!("{alive:?}"));
    shell_long_line(a);

    // The S9-6 group needs the Docker fixture's hooks and sinks; the guest build has none and
    // runs its own three guest-only scenarios instead.
    #[cfg(not(feature = "guest"))]
    s96_checks(a, &f);
    #[cfg(feature = "guest")]
    guest::guest_checks(a, &f);
}

/// Field names of the shell's `completion` line, in order (C2).
const RECORD_KEYS: [&str; 11] = [
    "schema", "schema_version", "mode", "phase", "dispatch", "outcome", "remote_status", "remote_signal",
    "error_code", "stdout_complete", "stderr_complete",
];

type Record = Vec<(String, String)>;

fn field<'a>(rec: &'a Record, key: &str) -> Option<&'a str> {
    rec.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn has(rec: &Record, want: &[(&str, &str)]) -> bool {
    want.iter().all(|(k, v)| field(rec, k) == Some(*v))
}

/// The shell's `completion` builtin: the last ssh child's record, shape-checked.
fn completion(t: &mut Term) -> Result<Record, String> {
    let lines = t.run("completion", 30_000)?;
    let line = lines
        .iter()
        .find(|l| l.starts_with("schema=cellbox.ssh.completion "))
        .ok_or_else(|| format!("no record in {lines:?}"))?;
    println(&format!("P04 INFO p09-completion {line}"));
    let rec: Record = line
        .split(' ')
        .map(|kv| kv.split_once('=').map(|(k, v)| (String::from(k), String::from(v))))
        .collect::<Option<_>>()
        .ok_or_else(|| format!("malformed {line}"))?;
    let shaped = rec.len() >= RECORD_KEYS.len()
        && rec.iter().zip(RECORD_KEYS).all(|((k, _), want)| k == want)
        && field(&rec, "schema_version") == Some("1");
    shaped.then_some(rec).ok_or_else(|| format!("shape {line}"))
}

/// What one S9-6 case left behind.
struct Run {
    st: String,
    out: Vec<u8>,
    err: String,
    ms: u64,
    rec: Result<Record, String>,
}

impl Run {
    fn rec_has(&self, want: &[(&str, &str)]) -> bool {
        self.rec.as_ref().is_ok_and(|rec| has(rec, want))
    }
}

fn describe(r: &Result<Run, String>) -> String {
    match r {
        Ok(run) => {
            let rec = match &run.rec {
                Ok(rec) => rec.iter().skip(2).map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "),
                Err(why) => format!("<{why}>"),
            };
            format!("{} ms={} {rec} err={}", run.st, run.ms, run.err.trim())
        }
        Err(why) => why.clone(),
    }
}

/// Answer the password prompt; returns early (Ok) when ssh ends before prompting.
fn answer_password(t: &mut Term, password: &str) -> Result<(), String> {
    let end = now_ms() + SSH_MS;
    while now_ms() < end {
        t.pump();
        if t.stderr_text().contains("password: ") {
            type_keys(t, password)?;
            return press(t, ENTER);
        }
        if t.at_prompt() {
            return Ok(());
        }
        let _ = sys_set_timer(1);
    }
    Err(String::from("no password prompt"))
}

fn ssh_line(port: u16, args: &str) -> String {
    format!("ssh --user tester --auth password -p {port} {args}")
}

/// One S9-6 case: open its serial segment, run `cmd` (answering the password prompt),
/// run `during` while ssh is live, wait for the prompt, then read `$?` and `completion`.
fn case(
    t: &mut Term,
    name: &str,
    cmd: &str,
    password: &str,
    ms: u64,
    during: impl FnOnce(&mut Term) -> Result<(), String>,
) -> Result<Run, String> {
    println(&format!("P04 INFO p09-case {name}"));
    // A longer line is refused whole by the shell, so fail it here with a clearer message.
    if cmd.len() > SHELL_LINE_MAX {
        return Err(format!("command line of {} bytes exceeds the shell's {SHELL_LINE_MAX}", cmd.len()));
    }
    let start = now_ms();
    t.send(cmd)?;
    answer_password(t, password)?;
    during(t).map_err(|e| format!("{e} err={}", clip(t.stderr_text().trim())))?;
    t.wait_prompt(ms)?;
    let ms = now_ms().saturating_sub(start);
    let (out, err) = (after_echo(&t.out).to_vec(), t.stderr_text());
    let st = status(t)?;
    let rec = completion(t);
    Ok(Run { st, out, err, ms, rec })
}

fn judge(name: &str, r: &Result<Run, String>, st: &str, want: &[(&str, &str)], extra: impl Fn(&Run) -> bool) {
    let ok = r.as_ref().is_ok_and(|run| run.st == st && run.rec_has(want) && extra(run));
    check(name, ok, &describe(r));
}

/// After a fault case: the runner unpauses the container on `p09-restore`; a bounded
/// retry of `--command true` proves the server answers again before the next case.
fn restore(t: &mut Term, f: &Fixture, after: &str) -> bool {
    println("P04 INFO p09-restore");
    println(&format!("P04 INFO p09-case {after}-restore"));
    for attempt in 1..=8 {
        let r = (|| {
            t.send(&ssh_line(f.port, "10.0.2.2 --command true"))?;
            answer_password(t, &f.password)?;
            t.wait_prompt(SSH_MS)?;
            status(t)
        })();
        if r.as_deref() == Ok("st=0") {
            println(&format!("P04 INFO p09-restored {after} attempts={attempt}"));
            return true;
        }
        println(&format!("P04 INFO p09-restore-retry {after} {}", clip(&format!("{r:?}"))));
        sleep_ms(2000);
    }
    false
}

/// The first 64-hex-digit word of `text` (a `sha256sum` line).
fn sha256_word(text: &str) -> Option<&str> {
    text.split_whitespace().find(|w| w.len() == 64 && w.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// S9-6: completion records, interruption at each dispatch point, rekey triggers,
/// backpressure, timeouts and cancellation (contracts C2/C3).
fn s96_checks(a: &mut Term, f: &Fixture) {
    let pw = f.password.as_str();
    let none = |_: &mut Term| -> Result<(), String> { Ok(()) };
    let exec_done = |status: &'static str| {
        [
            ("mode", "exec"), ("phase", "complete"), ("dispatch", "accepted"), ("outcome", "remote_exit"),
            ("remote_status", status), ("remote_signal", "null"), ("error_code", "null"),
            ("stdout_complete", "true"), ("stderr_complete", "true"),
        ]
    };
    let mut restores: Vec<(&str, bool)> = Vec::new();

    // First S9-6 connection: the unknown user splits the sshd log (`Invalid user p09split`)
    // so the runner judges the S9-3 sessions apart from these. Refused before any channel.
    let r = case(a, "ssh-completion-auth", &format!("ssh --user p09split --auth password -p {} 10.0.2.2 --command true", f.port),
        "p09-wrong-password", SSH_MS, none);
    judge("ssh-completion-auth", &r, "st=255", &[
        ("mode", "exec"), ("phase", "auth"), ("dispatch", "not_sent"), ("outcome", "local_error"),
        ("remote_status", "null"), ("remote_signal", "null"),
    ], |run| run.rec.as_ref().is_ok_and(|rec| field(rec, "error_code") != Some("null")));

    let r = case(a, "ssh-completion-trust",
        &ssh_line(f.port, "host --command 'p9 completion-trust'"), pw, SSH_MS, none);
    judge("ssh-completion-trust", &r, "st=255", &[
        ("mode", "exec"), ("phase", "trust"), ("dispatch", "not_sent"), ("outcome", "local_error"),
        ("remote_status", "null"), ("remote_signal", "null"),
    ], |run| {
        !run.err.contains("password: ") && run.rec.as_ref().is_ok_and(|rec| field(rec, "error_code") != Some("null"))
    });

    let r = case(a, "ssh-completion-exec",
        &ssh_line(f.port, "10.0.2.2 --command 'p9 completion-exec; printf p09out; printf p09err >&2; exit 4'"),
        pw, SSH_MS, none);
    judge("ssh-completion-exec", &r, "st=4", &exec_done("4"), |run| {
        run.out.windows(6).any(|w| w == b"p09out") && run.err.ends_with("p09err")
    });

    let r = case(a, "ssh-completion-signal",
        &ssh_line(f.port, "10.0.2.2 --command 'p9 completion-signal; kill -TERM $$'"), pw, SSH_MS, none);
    judge("ssh-completion-signal", &r, "st=255", &[
        ("mode", "exec"), ("phase", "complete"), ("dispatch", "accepted"), ("outcome", "remote_signal"),
        ("remote_status", "null"), ("remote_signal", "TERM"), ("error_code", "null"),
    ], |_| true);

    let r = case(a, "ssh-completion-pty", &ssh_line(f.port, "10.0.2.2"), pw, SSH_MS, |t| {
        // Typed input before the client enters raw mode is dropped: wait for the remote
        // shell's first output. The remote shell expands the arithmetic, so the echoed
        // input never matches.
        wait_remote_output(t)?;
        sleep_ms(500);
        type_keys(t, "echo p09-pty-$((40+2))")?;
        press(t, ENTER)?;
        wait_stdout(t, b"p09-pty-42")?;
        type_keys(t, "exit 6")?;
        press(t, ENTER)
    });
    judge("ssh-completion-pty", &r, "st=6", &[
        ("mode", "pty"), ("phase", "complete"), ("dispatch", "accepted"), ("outcome", "remote_exit"),
        ("remote_status", "6"), ("remote_signal", "null"), ("error_code", "null"),
    ], |_| true);

    // Interruption at each dispatch point: the runner kills the live session process when
    // the client reports the point (the client holds it up to 5 s).
    let lost = [("remote_status", "null"), ("remote_signal", "null")];
    for (name, point, dispatch, outcome) in [
        ("ssh-fault-before-request", "before-request", "not_sent", "local_error"),
        ("ssh-fault-may-have-run", "may-have-run", "may_have_run", "unknown"),
        ("ssh-fault-accepted", "accepted", "accepted", "unknown"),
    ] {
        let tag = &name["ssh-fault-".len()..];
        let cmd = format!("10.0.2.2 --command ': p09-fault {point} kill; p9 {tag}; sleep 20'");
        let r = case(a, name, &ssh_line(f.port, &cmd), pw, SSH_MS, none);
        judge(name, &r, "st=255", &[("mode", "exec"), ("dispatch", dispatch), ("outcome", outcome), lost[0], lost[1]],
            |_| true);
        restores.push((name, restore(a, f, name)));
    }
    // Exit status received while the output is still to come (`p9 drain` exits 3 at once;
    // a background job writes 256 KiB 2 s later): the session dies before the drain, the
    // status is still the remote one.
    let r = case(a, "ssh-fault-exit-before-drain", &ssh_line(f.port,
        "10.0.2.2 --command ': p09-fault exit-before-drain kill; p9 drain'"),
        pw, SSH_MS, none);
    judge("ssh-fault-exit-before-drain", &r, "st=3", &[
        ("mode", "exec"), ("dispatch", "accepted"), ("outcome", "remote_exit"), ("remote_status", "3"),
        ("remote_signal", "null"), ("stdout_complete", "false"),
    ], |_| true);
    restores.push(("ssh-fault-exit-before-drain", restore(a, f, "ssh-fault-exit-before-drain")));

    // Rekeys (lane policy: 2 MiB / 20 s per direction; the second server 64 KiB). The
    // runner counts `ssh-rekeys` and checks the sink's byte count and digest.
    let r = case(a, "ssh-rekey-bytes", &ssh_line(f.port,
        "10.0.2.2 --command ': p09-sink; p9 rekey-bytes; head -c 4194304 /dev/zero'"),
        pw, BULK_MS, none);
    judge("ssh-rekey-bytes", &r, "st=0", &exec_done("0"), |_| true);
    let r = case(a, "ssh-rekey-time", &ssh_line(f.port,
        "10.0.2.2 --command 'p9 rekey-time; sleep 25'"), pw, SSH_MS, none);
    judge("ssh-rekey-time", &r, "st=0", &exec_done("0"), |run| run.ms >= 25_000);
    let r = match f.rekey_port {
        Some(port) => case(a, "ssh-rekey-peer", &ssh_line(port,
            "10.0.2.2 --command ': p09-sink; p9 rekey-peer; head -c 1048576 /dev/zero'"),
            pw, BULK_MS, none),
        None => Err(String::from("no REKEY_PORT in /p09/ssh.conf")),
    };
    judge("ssh-rekey-peer", &r, "st=0", &exec_done("0"), |_| true);

    // 8 MiB of random bytes through the throttled sink; the runner compares the client's
    // digest with the remote one reported here.
    let r = case(a, "ssh-backpressure-bytes", &ssh_line(f.port,
        "10.0.2.2 --command ': p09-sink; p9 backpressure; p9 bulk'"),
        pw, BULK_MS, none);
    let remote = r.as_ref().ok().and_then(|run| sha256_word(&run.err).map(String::from));
    if let Some(sha) = &remote {
        println(&format!("P04 INFO ssh-remote-sha256 {sha}"));
    }
    judge("ssh-backpressure-bytes", &r, "st=0", &exec_done("0"), |_| remote.is_some());

    // `--timeout` is absolute: 124 long before the remote `sleep 30` ends.
    let r = case(a, "ssh-timeout-overall", &ssh_line(f.port,
        "--timeout 5 10.0.2.2 --command 'p9 timeout-overall; sleep 30'"), pw, SSH_MS, none);
    judge("ssh-timeout-overall", &r, "st=124", &[("mode", "exec"), ("outcome", "timeout"), lost[0], lost[1]],
        |run| (5_000..25_000).contains(&run.ms));
    // The container is paused at Authenticated: the channel is never confirmed, the
    // request phase's 30 s budget ends it without sending the command.
    let r = case(a, "ssh-timeout-request", &ssh_line(f.port,
        "10.0.2.2 --command ': p09-fault before-request pause; p9 timeout-request'"),
        pw, DEADLINE_MS, none);
    judge("ssh-timeout-request", &r, "st=124", &[
        ("mode", "exec"), ("phase", "request"), ("dispatch", "not_sent"), ("outcome", "timeout"), lost[0], lost[1],
    ], |run| (30_000..120_000).contains(&run.ms));
    restores.push(("ssh-timeout-request", restore(a, f, "ssh-timeout-request")));
    // The age rekey (20 s) starts during `sleep 60`; the paused server never answers it,
    // the KEX's 30 s budget ends the session before the sleep would.
    let r = case(a, "ssh-timeout-kex", &ssh_line(f.port,
        "10.0.2.2 --command ': p09-fault rekey-start pause; p9 timeout-kex; sleep 60'"),
        pw, DEADLINE_MS, none);
    judge("ssh-timeout-kex", &r, "st=124", &[
        ("mode", "exec"), ("phase", "running"), ("dispatch", "accepted"), ("outcome", "timeout"), lost[0], lost[1],
    ], |run| (30_000..120_000).contains(&run.ms));
    restores.push(("ssh-timeout-kex", restore(a, f, "ssh-timeout-kex")));

    let r = case(a, "ssh-cancel-exec", &ssh_line(f.port,
        "10.0.2.2 --command 'echo p09-started; p9 cancel-exec; sleep 30'"), pw, SSH_MS, |t| {
        wait_stdout(t, b"p09-started")?;
        sleep_ms(500);
        t.host.submit_key(0x63, CTRL, 1).map_err(|s| format!("ctrl-c {s:?}"))
    });
    judge("ssh-cancel-exec", &r, "st=130", &[
        ("mode", "exec"), ("phase", "running"), ("dispatch", "accepted"), ("outcome", "cancelled"), lost[0], lost[1],
    ], |run| run.ms < 30_000);

    let failed: Vec<&str> = restores.iter().filter(|(_, ok)| !ok).map(|(name, _)| *name).collect();
    check("ssh-fault-restores", restores.len() == 6 && failed.is_empty(), &format!("not restored after {failed:?}"));
}
