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
//!
//! Pipeline stage (`--features guest-pipe`, `261110-1200-cellbox-followups/guest-pipeline-cp1.md`):
//! `pipe_checks` replaces the password scenarios above. The guest's sshd accepts one throw-away
//! ed25519 key (publickey only); the driver imports that key into the KMS with the K7 admin cell
//! (read with `cat`, pasted at the echo-off prompt, never printed) and runs the unchanged
//! `cellbox ssh --auth publickey --identity` as a plain exec and as pipeline stages carrying a
//! 512 KiB blob (NUL, 0xFF, invalid UTF-8, terminal-control bytes), plus the refusals that are the
//! retained password-mode limitation.

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

#[cfg(feature = "guest-pipe")]
pub(super) use pipe::pipe_checks;

#[cfg(feature = "guest-pipe")]
mod pipe {
    use super::*;

    /// One pipeline transfer (512 KiB through nested TCG in both directions).
    const PIPE_MS: u64 = 480_000;
    /// Total wait for the KMS service to answer the admin cell.
    const SERVICE_MS: u64 = 300_000;
    const ARMOR_BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
    const ARMOR_END: &str = "-----END OPENSSH PRIVATE KEY-----";
    const HOST: &str = "10.0.2.2";
    const BLOB: &str = "/t3p/blob.bin";
    const STDIN_FILE: &str = "/tmp/p09-pipe-in";
    const RT_FILE: &str = "/tmp/p09-rt.bin";
    const MARKERS: [&str; 5] = [
        "ssh-guest-pk-import", "ssh-guest-pk-exec", "ssh-guest-pipe-refusals", "ssh-guest-pipe-stdin-exact",
        "ssh-guest-pipe-roundtrip",
    ];
    const POSTURE_VOLATILE: &str = "  posture: DEV_POSTURE volatile: lost when the key service restarts";

    /// `/t3p/pipe.conf` (public values only): `t3pipe-conf 1`, `blob sha256=HEX len=N`,
    /// `keyfp SHA256:FP`, `name NAME`, `nonce HEX`.
    struct Conf {
        name: String,
        nonce: String,
        keyfp: String,
        sha: String,
        len: String,
    }

    /// Secret canaries (the key's body lines) and the number of transcript hits.
    struct Ctx {
        needles: Vec<String>,
        hits: u32,
    }

    struct Out {
        st: String,
        out: Vec<String>,
        err: String,
        ms: u64,
        rec: Result<Record, String>,
        withheld: bool,
    }

    impl Out {
        fn describe(&self) -> String {
            let err = if self.withheld { String::from("<withheld>") } else { clip(self.err.trim()) };
            format!("{} ms={} out_lines={} err={err}", self.st, self.ms, self.out.len())
        }

        /// The stage's `completion` record is a clean exec exit.
        fn rec_ok(&self) -> bool {
            self.rec.as_ref().is_ok_and(|r| {
                has(r, &[("mode", "exec"), ("outcome", "remote_exit"), ("remote_status", "0"), ("error_code", "null")])
            })
        }

        fn rec_code(&self) -> &str {
            self.rec.as_ref().ok().and_then(|r| field(r, "error_code")).unwrap_or("-")
        }

        /// No password, passphrase or first-trust prompt anywhere.
        fn quiet(&self) -> bool {
            !self.err.contains("password: ")
                && !self.err.contains("passphrase")
                && !self.err.contains("Type 'yes'")
                && !self.out.iter().any(|l| l.contains("Type 'yes'"))
        }
    }

    fn kv<'a>(lines: &'a [String], prefix: &str) -> Option<&'a str> {
        lines.iter().find_map(|l| l.strip_prefix(prefix))
    }

    fn has_line(lines: &[String], line: &str) -> bool {
        lines.iter().any(|l| l == line)
    }

    /// Stdout lines after the echoed command line, without the trailing prompt line.
    fn out_lines(a: &Term) -> Vec<String> {
        let text = strip_ansi(after_echo(&a.out));
        let mut lines = lines_of(&text);
        if lines.last().is_some_and(|l| l.starts_with(PROMPT.trim_end())) {
            lines.pop();
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }

    /// Look for every secret in this step's transcripts. Prints the step, never any text.
    fn scan(a: &Term, c: &mut Ctx, step: &str) -> bool {
        let (out, err) = (strip_ansi(&a.out), strip_ansi(&a.err));
        let mut hit = false;
        for needle in &c.needles {
            let n = needle.as_bytes();
            if find(&a.out, n).is_some() || find(&a.err, n).is_some() || out.contains(needle.as_str()) || err.contains(needle.as_str()) {
                hit = true;
                c.hits += 1;
                println(&format!("P04 INFO ssh-guest-pipe-scan-hit step={step}"));
            }
        }
        hit
    }

    /// An error that may quote a transcript is replaced when the transcript holds a secret.
    fn guard<T>(a: &Term, c: &mut Ctx, step: &str, r: Result<T, String>) -> Result<T, String> {
        match r {
            Ok(v) => Ok(v),
            Err(e) => {
                if scan(a, c, step) {
                    Err(String::from("withheld: transcript holds a secret"))
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Wait for `what` on stderr (`Ok(true)`) or for the shell prompt (`Ok(false)`).
    fn wait_stderr_or_prompt(t: &mut Term, what: &str, ms: u64) -> Result<bool, String> {
        let end = now_ms() + ms;
        loop {
            t.pump();
            if t.stderr_text().contains(what) {
                return Ok(true);
            }
            if t.at_prompt() {
                return Ok(false);
            }
            if now_ms() >= end {
                return Err(format!("neither `{what}` on stderr nor a prompt"));
            }
            let _ = sys_set_timer(1);
        }
    }

    fn read_conf(a: &mut Term) -> Result<Conf, String> {
        let lines = a.run("cat /t3p/pipe.conf", 30_000)?;
        if !has_line(&lines, "t3pipe-conf 1") {
            return Err(format!("pipe.conf: no header in {} lines", lines.len()));
        }
        let blob = kv(&lines, "blob ").ok_or_else(|| String::from("pipe.conf: no blob line"))?;
        let (sha, len) = blob.split_once(" len=").ok_or_else(|| String::from("pipe.conf: blob line shape"))?;
        let sha = sha.strip_prefix("sha256=").ok_or_else(|| String::from("pipe.conf: blob sha256"))?;
        let keyfp = kv(&lines, "keyfp ").ok_or_else(|| String::from("pipe.conf: no keyfp"))?;
        let name = kv(&lines, "name ").ok_or_else(|| String::from("pipe.conf: no name"))?;
        let nonce = kv(&lines, "nonce ").ok_or_else(|| String::from("pipe.conf: no nonce"))?;
        let shaped = sha.len() == 64
            && sha.bytes().all(|b| b.is_ascii_hexdigit())
            && keyfp.starts_with("SHA256:")
            && !name.is_empty()
            && nonce.len() >= 8
            && len.bytes().all(|b| b.is_ascii_digit())
            && !len.is_empty();
        if !shaped {
            return Err(String::from("pipe.conf: malformed values"));
        }
        Ok(Conf {
            name: String::from(name),
            nonce: String::from(nonce),
            keyfp: String::from(keyfp),
            sha: String::from(sha),
            len: String::from(len),
        })
    }

    /// The armored text of `/t3p/pipe.key`, read once with `cat` (never `run`: the transcript is
    /// cleared before returning, errors are fixed texts). Its body lines become scan canaries.
    fn read_key(a: &mut Term, c: &mut Ctx) -> Result<String, &'static str> {
        let cmd = "cat /t3p/pipe.key";
        a.send(cmd).map_err(|_| "send")?;
        a.wait_prompt(SSH_MS).map_err(|_| "no prompt after cat")?;
        let mut lines = a.result(cmd);
        a.out.clear();
        a.err.clear();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let shaped = lines.len() >= 3
            && lines.first().map(String::as_str) == Some(ARMOR_BEGIN)
            && lines.last().map(String::as_str) == Some(ARMOR_END);
        if !shaped {
            return Err("key file: not an armored block");
        }
        for line in &lines[1..lines.len() - 1] {
            if line.len() >= 8 {
                c.needles.push(line.clone());
            }
        }
        let mut text = lines.join("\n");
        text.push('\n');
        Ok(text)
    }

    /// Collect what the step just ran left behind and read `$?` (and optionally `completion`).
    fn finish(a: &mut Term, c: &mut Ctx, id: &str, start: u64, rec: bool) -> Result<Out, String> {
        let end = now_ms();
        let out = out_lines(a);
        let err = a.stderr_text();
        let hits = c.hits;
        scan(a, c, id);
        let withheld = c.hits != hits;
        let st = status(a)?;
        let rec = if rec { completion(a) } else { Err(String::from("not read")) };
        Ok(Out { st, out, err, ms: end.saturating_sub(start), rec, withheld })
    }

    /// One shell line to the prompt, then `$?` and optionally the `completion` record.
    fn stage(a: &mut Term, c: &mut Ctx, id: &str, line: &str, ms: u64, rec: bool) -> Result<Out, String> {
        println(&format!("P04 INFO ssh-guest-pipe-case {id}"));
        if line.len() > SHELL_LINE_MAX {
            return Err(format!("command line of {} bytes exceeds the shell's {SHELL_LINE_MAX}", line.len()));
        }
        let start = now_ms();
        let r = (|| {
            a.send(line)?;
            a.wait_prompt(ms)
        })();
        guard(a, c, id, r)?;
        let r = finish(a, c, id, start, rec);
        guard(a, c, id, r)
    }

    fn pk_line(f: &Fixture, key: &str, host: &str, command: &str) -> String {
        format!("ssh --user tester --auth publickey --identity {key} -p {} {host} --command '{command}'", f.port)
    }

    fn fin(name: &str, conds: &[(&str, bool)], detail: String) {
        let failed: Vec<&str> = conds.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
        check(name, failed.is_empty(), &format!("failed={failed:?} {detail}"));
    }

    fn fin_err(name: &str, why: &str) {
        check(name, false, why);
    }

    /// `kms-ssh-admin status` until the key service answers (st=0); `Some(attempts)`.
    fn wait_service(a: &mut Term) -> Option<u32> {
        let end = now_ms() + SERVICE_MS;
        let mut attempts = 0u32;
        while now_ms() < end {
            attempts += 1;
            let ok = (|| {
                a.send("kms-ssh-admin status")?;
                a.wait_prompt(SSH_MS)?;
                status(a)
            })();
            if ok.as_deref() == Ok("st=0") {
                return Some(attempts);
            }
            sleep_ms(3_000);
        }
        None
    }

    /// `kms-ssh-admin import NAME` with the key pasted at the echo-off prompt.
    fn import(a: &mut Term, c: &mut Ctx, name: &str, text: &str) -> Result<Out, String> {
        let id = "import";
        let start = now_ms();
        let r = (|| {
            a.send(&format!("kms-ssh-admin import {name}"))?;
            if wait_stderr_or_prompt(a, "Paste the OpenSSH private key now", SSH_MS)? {
                // The reader takes the echo-off lease right after the prompt: let it.
                sleep_ms(500);
                a.host.submit_paste(text).map_err(|s| format!("paste {s:?}"))?;
            }
            a.wait_prompt(SSH_MS)
        })();
        guard(a, c, id, r)?;
        let r = finish(a, c, id, start, false);
        guard(a, c, id, r)
    }

    pub(crate) fn pipe_checks(a: &mut Term, f: &Fixture) {
        let mut c = Ctx { needles: Vec::new(), hits: 0 };
        let conf = match read_conf(a) {
            Ok(conf) => conf,
            Err(why) => return skip(&MARKERS, &why),
        };
        let key = match read_key(a, &mut c) {
            Ok(text) => text,
            Err(why) => return skip(&MARKERS, why),
        };
        let name = conf.name.as_str();

        // 3. The key goes into the KMS with the K7 admin cell.
        let r = match wait_service(a) {
            Some(attempts) => {
                println(&format!("P04 INFO ssh-guest-pk-status attempts={attempts}"));
                import(a, &mut c, name, &key)
            }
            None => Err(String::from("the key service never answered `kms-ssh-admin status`")),
        };
        drop(key);
        match &r {
            Ok(imp) => fin(
                "ssh-guest-pk-import",
                &[
                    ("st0", imp.st == "st=0"),
                    ("imported", has_line(&imp.out, &format!("kms-ssh-admin: imported key '{name}'"))),
                    ("plain", has_line(&imp.out, "  custody: plain")),
                    (
                        "warning",
                        has_line(&imp.out, "  WARNING: unencrypted key held by the key service (development posture)"),
                    ),
                    ("posture", has_line(&imp.out, POSTURE_VOLATILE)),
                    ("fingerprint", kv(&imp.out, "  fingerprint: ") == Some(conf.keyfp.as_str())),
                    ("no-secret-in-transcript", !imp.withheld),
                ],
                imp.describe(),
            ),
            Err(why) => fin_err("ssh-guest-pk-import", why),
        }

        // 4. Foreground publickey exec: isolates publickey-to-guest failures from pipeline failures.
        let r = stage(a, &mut c, "pk-exec", &pk_line(f, name, HOST, &format!("echo {}", conf.nonce)), SSH_MS, true);
        match &r {
            Ok(o) => fin(
                "ssh-guest-pk-exec",
                &[
                    ("st0", o.st == "st=0"),
                    ("stdout", o.out == [conf.nonce.clone()]),
                    ("no-prompt", o.quiet()),
                    ("completion", o.rec_ok()),
                ],
                o.describe(),
            ),
            Err(why) => fin_err("ssh-guest-pk-exec", why),
        }

        // 5. The retained password limitation and the stage refusals, before or at key exchange.
        let feed = format!("cat {BLOB} | ");
        println("P04 INFO ssh-guest-pipe-mark neg-ab-begin");
        let pw = stage(
            a,
            &mut c,
            "refuse-password-pipeline",
            &format!("{feed}ssh --user tester --auth password -p {} {HOST} --command cat", f.port),
            SSH_MS,
            false,
        );
        let unknown = stage(a, &mut c, "refuse-unknown-identity", &format!("{feed}{}", pk_line(f, "nope", HOST, "cat")), SSH_MS, true);
        println("P04 INFO ssh-guest-pipe-mark neg-ab-end");
        println("P04 INFO ssh-guest-pipe-mark neg-c-begin");
        let host = stage(a, &mut c, "refuse-unknown-host", &format!("{feed}{}", pk_line(f, name, "gateway", "true")), SSH_MS, true);
        println("P04 INFO ssh-guest-pipe-mark neg-c-end");
        let mut conds: Vec<(&str, bool)> = Vec::new();
        conds.push(("a-st2", pw.as_ref().is_ok_and(|o| o.st == "st=2")));
        conds.push((
            "a-message",
            pw.as_ref().is_ok_and(|o| {
                o.err.contains("interactive session") && !o.err.contains("password: ") && !o.err.contains("SHA256:")
            }),
        ));
        conds.push(("b-st255", unknown.as_ref().is_ok_and(|o| o.st == "st=255")));
        conds.push(("b-message", unknown.as_ref().is_ok_and(|o| o.err.contains("no such identity"))));
        conds.push(("b-code", unknown.as_ref().is_ok_and(|o| o.rec_code() == "identity_unknown")));
        conds.push(("c-st255", host.as_ref().is_ok_and(|o| o.st == "st=255")));
        conds.push((
            "c-message",
            host.as_ref()
                .is_ok_and(|o| o.err.contains("host key not trusted; confirmation needs an interactive frontend")),
        ));
        conds.push(("c-no-prompt", host.as_ref().is_ok_and(|o| o.quiet())));
        let detail = |r: &Result<Out, String>| match r {
            Ok(o) => o.describe(),
            Err(why) => why.clone(),
        };
        fin("ssh-guest-pipe-refusals", &conds, format!("a: {} | b: {} | c: {}", detail(&pw), detail(&unknown), detail(&host)));

        // 6. Exact stdin: the blob reaches the guest unchanged, one EOF.
        let line = format!(
            "{feed}{}",
            pk_line(f, name, HOST, &format!("cat > {STDIN_FILE}; sha256sum {STDIN_FILE}; wc -c < {STDIN_FILE}"))
        );
        let r = stage(a, &mut c, "pipe-stdin-exact", &line, PIPE_MS, true);
        match &r {
            Ok(o) => {
                let sha = o.out.first().and_then(|l| sha256_word(l)).unwrap_or("none");
                let len = o.out.get(1).map(|l| l.trim()).unwrap_or("none");
                println(&format!("P04 INFO ssh-guest-pipe-digest which=stdin sha={sha} len={len}"));
                fin(
                    "ssh-guest-pipe-stdin-exact",
                    &[
                        ("st0", o.st == "st=0"),
                        ("no-prompt", o.quiet()),
                        ("stderr-empty", o.err.trim().is_empty()),
                        ("two-lines", o.out.len() == 2),
                        ("sha", o.out.first().map(String::as_str) == Some(format!("{}  {STDIN_FILE}", conf.sha).as_str())),
                        ("len", o.out.get(1).map(String::as_str) == Some(conf.len.as_str())),
                        ("completion", o.rec_ok()),
                    ],
                    o.describe(),
                );
            }
            Err(why) => fin_err("ssh-guest-pipe-stdin-exact", why),
        }

        // 7. Full duplex: the blob through `cat` on the guest into a local file, then that file
        // back through `sha256sum` (stdout of a stage is byte-exact too).
        let line = format!("{feed}{} > {RT_FILE}", pk_line(f, name, HOST, "cat"));
        let up = stage(a, &mut c, "pipe-roundtrip-up", &line, PIPE_MS, true);
        let line = format!("cat {RT_FILE} | {}", pk_line(f, name, HOST, "sha256sum"));
        let back = stage(a, &mut c, "pipe-roundtrip-sum", &line, PIPE_MS, true);
        if let Ok(o) = &back {
            let sha = o.out.first().and_then(|l| sha256_word(l)).unwrap_or("none");
            println(&format!("P04 INFO ssh-guest-pipe-digest which=roundtrip sha={sha} len=-"));
        }
        let want = format!("{}  -", conf.sha);
        let conds = [
            ("up-st0", up.as_ref().is_ok_and(|o| o.st == "st=0")),
            ("up-no-prompt", up.as_ref().is_ok_and(|o| o.quiet())),
            ("up-stderr-empty", up.as_ref().is_ok_and(|o| o.err.trim().is_empty())),
            ("up-stdout-empty", up.as_ref().is_ok_and(|o| o.out.is_empty())),
            ("up-completion", up.as_ref().is_ok_and(|o| o.rec_ok())),
            ("sum-st0", back.as_ref().is_ok_and(|o| o.st == "st=0")),
            ("sum-no-prompt", back.as_ref().is_ok_and(|o| o.quiet())),
            ("sum-stderr-empty", back.as_ref().is_ok_and(|o| o.err.trim().is_empty())),
            ("sum-digest", back.as_ref().is_ok_and(|o| o.out == [want.clone()])),
            ("sum-completion", back.as_ref().is_ok_and(|o| o.rec_ok())),
        ];
        fin("ssh-guest-pipe-roundtrip", &conds, format!("up: {} | sum: {}", detail(&up), detail(&back)));

        println("P04 INFO ssh-guest-pipe-logins 4");
    }
}
