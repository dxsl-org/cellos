//! KSSH K8 lane (`k8-cp1.md` sections 7-8, interfaces: `k8-contract.md`): `cellbox ssh
//! --auth publickey --identity KEY_ID` with an ed25519 key held by the key service, driven
//! through the real shell. Built only with `--features kssh`; the host judge correlates the
//! `P04` lines below with the fixture's sshd log.
//!
//! `/kssh/kssh.conf` (written by the runner, TEST secrets, never printed) selects the mode:
//! `t0` runs S00, S01a, S01b, S01, S09, S10, S04a/b, S06a-d, S02, S02p, S03, S03b, S02',
//! S08, S15; `f` (the fault cellbox) runs S00, S01, S05a-c, S02, S11. `calibrate 1` adds the
//! KDF timing attempts after S01.
//!
//! Secrecy: no key text, passphrase, WRONG value, password or conf line is ever formatted
//! into a `P04` line. Key-bearing steps use `send` + `wait_prompt` (never `Term::run`, whose
//! diagnostics echo stderr), their errors are withheld when the transcript scan hits, and
//! every secret-bearing transcript is scanned for the secrets right after the step. Only
//! counts reach the `kssh-transcripts` marker.

use super::*;
use ostd::clients::kms::{KmsClient, KmsClientError, KmsErrorCode};
use ostd::syscall::{sys_lookup_service, sys_recv_timeout, sys_send, sys_try_recv, SyscallResult};

/// Markers in the order the judge expects them (`k8-contract.md` section 3).
const T0_MARKERS: [&str; 21] = [
    "kssh-conf", "kssh-s00", "kssh-s01a", "kssh-s01b", "kssh-s01", "kssh-s09", "kssh-s10", "kssh-s04a",
    "kssh-s04b", "kssh-s06a", "kssh-s06b", "kssh-s06c", "kssh-s06d", "kssh-s02", "kssh-s02p", "kssh-s03",
    "kssh-s03b", "kssh-s02x", "kssh-s08", "kssh-s15", "kssh-transcripts",
];
const F_MARKERS: [&str; 9] = [
    "kssh-conf", "kssh-s00", "kssh-s01", "kssh-s05a", "kssh-s05b", "kssh-s05c", "kssh-s02", "kssh-s11",
    "kssh-transcripts",
];

/// KDF-bearing attempts: the KDF under TCG is unmeasured, so the bound is generous (the
/// client's own sign bound is 60 s and its auth budget 120 s; the driver must outlast both).
const KDF_MS: u64 = 600_000;
/// Connect + key exchange + server probe until the passphrase prompt.
const PROMPT_MS: u64 = 300_000;
const ARMOR_BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const ARMOR_END: &str = "-----END OPENSSH PRIVATE KEY-----";
const HOST: &str = "10.0.2.2";

type Res = Result<(bool, String), String>;

// ---------------------------------------------------------------------------------------
// kssh.conf
// ---------------------------------------------------------------------------------------

struct KeyConf {
    name: String,
    fp: String,
    pass: Option<String>,
    wrong: Option<String>,
    prefix: String,
    rounds: u32,
}

struct Conf {
    f_mode: bool,
    calibrate: bool,
    main: u16,
    rekey: u16,
    b: u16,
    hostfp: String,
    password: String,
    nonce: String,
    raw_sha: String,
    raw_len: usize,
    keys: Vec<KeyConf>,
    keyb: String,
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn dash(word: &str) -> Option<String> {
    (word != "-").then(|| String::from(word))
}

/// Parse the conf lines. Errors are fixed texts: a conf line is never formatted.
fn parse_conf(lines: &[String]) -> Result<Conf, &'static str> {
    if lines.first().map(String::as_str) != Some("kssh-conf 1") {
        return Err("header");
    }
    let mut f_mode = None;
    let mut calibrate = None;
    let mut ports = None;
    let (mut hostfp, mut password, mut nonce, mut raw, mut keyb) = (None, None, None, None, None);
    let mut keys: Vec<KeyConf> = Vec::new();
    for line in &lines[1..] {
        let words: Vec<&str> = line.split(' ').collect();
        match words.as_slice() {
            ["mode", "t0"] => f_mode = Some(false),
            ["mode", "f"] => f_mode = Some(true),
            ["calibrate", "0"] => calibrate = Some(false),
            ["calibrate", "1"] => calibrate = Some(true),
            ["ports", a, b, c] => {
                ports = Some((
                    a.parse::<u16>().map_err(|_| "ports")?,
                    b.parse::<u16>().map_err(|_| "ports")?,
                    c.parse::<u16>().map_err(|_| "ports")?,
                ));
            }
            ["hostfp", fp] => hostfp = Some(String::from(*fp)),
            ["password", pw] => password = Some(String::from(*pw)),
            ["nonce", n] => nonce = Some(String::from(*n)),
            ["raw", sha, len] => {
                let sha = sha.strip_prefix("sha256=").ok_or("raw")?;
                let len = len.strip_prefix("len=").and_then(|l| l.parse::<usize>().ok()).ok_or("raw")?;
                raw = Some((String::from(sha), len));
            }
            ["key", name, fp, pass, wrong, prefix, rounds] => keys.push(KeyConf {
                name: String::from(*name),
                fp: String::from(*fp),
                pass: dash(pass),
                wrong: dash(wrong),
                prefix: String::from(*prefix),
                rounds: rounds.parse().map_err(|_| "key rounds")?,
            }),
            ["keyb", fp] => keyb = Some(String::from(*fp)),
            _ => {}
        }
    }
    let (main, rekey, b) = ports.ok_or("ports")?;
    let (raw_sha, raw_len) = raw.ok_or("raw")?;
    let conf = Conf {
        f_mode: f_mode.ok_or("mode")?,
        calibrate: calibrate.ok_or("calibrate")?,
        main,
        rekey,
        b,
        hostfp: hostfp.filter(|fp| fp.starts_with("SHA256:")).ok_or("hostfp")?,
        password: password.filter(|p| !p.is_empty()).ok_or("password")?,
        nonce: nonce.filter(|n| is_hex(n, 16)).ok_or("nonce")?,
        raw_sha,
        raw_len,
        keys,
        keyb: keyb.unwrap_or_default(),
    };
    if !is_hex(&conf.raw_sha, 64) || conf.raw_len == 0 || main == 0 || rekey == 0 || b == 0 {
        return Err("raw/ports");
    }
    let mut want: Vec<&str> = alloc::vec!["ka16"];
    if !conf.f_mode {
        want.extend(["kl4", "kp0"]);
    }
    if conf.calibrate {
        want.extend(["kr1", "kr64"]);
    }
    for name in want {
        let key = conf.keys.iter().find(|k| k.name == name).ok_or("missing key")?;
        let wrapped = name != "kp0";
        let ok = key.fp.starts_with("SHA256:")
            && is_hex(&key.prefix, 16)
            && (!wrapped || (key.pass.is_some() && key.wrong.is_some()));
        if !ok {
            return Err("key fields");
        }
    }
    Ok(conf)
}

/// Whether the conf names mode `f`, even when the rest of it did not parse (picks the
/// marker list to skip).
fn mode_hint_f(lines: &[String]) -> bool {
    lines.iter().any(|l| l == "mode f")
}

// ---------------------------------------------------------------------------------------
// Secret scan
// ---------------------------------------------------------------------------------------

struct Ctx {
    conf: Conf,
    /// (label, needle): passphrases, WRONG values, the fixture password, key body lines.
    needles: Vec<(String, String)>,
    scans: u32,
    hits: u32,
    /// Key files already read (stem, armored text). Kept in driver memory only.
    texts: Vec<(String, String)>,
}

impl Ctx {
    fn new(conf: Conf) -> Ctx {
        let mut needles = Vec::new();
        for key in &conf.keys {
            if let Some(p) = &key.pass {
                needles.push((format!("pass:{}", key.name), p.clone()));
            }
            if let Some(w) = &key.wrong {
                needles.push((format!("wrong:{}", key.name), w.clone()));
            }
        }
        needles.push((String::from("password"), conf.password.clone()));
        Ctx { conf, needles, scans: 0, hits: 0, texts: Vec::new() }
    }

    fn key(&self, name: &str) -> Result<&KeyConf, String> {
        self.conf.keys.iter().find(|k| k.name == name).ok_or_else(|| format!("no key {name} in conf"))
    }

    fn pass(&self, name: &str) -> Result<String, String> {
        self.key(name)?.pass.clone().ok_or_else(|| format!("no passphrase for {name}"))
    }

    fn wrong(&self, name: &str) -> Result<String, String> {
        self.key(name)?.wrong.clone().ok_or_else(|| format!("no WRONG value for {name}"))
    }

    fn token(&self) -> String {
        format!("kssh-ok-{}", self.conf.nonce)
    }
}

/// Look for every secret in this step's transcripts (raw and ANSI-stripped). Prints labels
/// of hits, never any text.
fn scan(a: &Term, c: &mut Ctx, step: &str) -> bool {
    c.scans += 1;
    let (out, err) = (strip_ansi(&a.out), strip_ansi(&a.err));
    let mut hit = false;
    for (label, needle) in &c.needles {
        let n = needle.as_bytes();
        if find(&a.out, n).is_some() || find(&a.err, n).is_some() || out.contains(needle.as_str()) || err.contains(needle.as_str()) {
            hit = true;
            c.hits += 1;
            println(&format!("P04 INFO kssh-obs scan-hit step={step} label={label}"));
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

// ---------------------------------------------------------------------------------------
// Transcript helpers
// ---------------------------------------------------------------------------------------

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

/// Wait for `what` on stderr (`Ok(true)`) or for the shell prompt (`Ok(false)`: the command
/// ended without showing it).
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

fn prompt_text(key: &str) -> String {
    format!("Enter passphrase for key '{key}': ")
}

fn count_in(text: &str, what: &str) -> usize {
    text.matches(what).count()
}

fn kv<'a>(lines: &'a [String], prefix: &str) -> Option<&'a str> {
    lines.iter().find_map(|l| l.strip_prefix(prefix))
}

fn has_line(lines: &[String], line: &str) -> bool {
    lines.iter().any(|l| l == line)
}

fn rec_code(rec: &Result<Record, String>) -> &str {
    rec.as_ref().ok().and_then(|r| field(r, "error_code")).unwrap_or("-")
}

fn verdict(conds: &[(&str, bool)], detail: String) -> Res {
    let failed: Vec<&str> = conds.iter().filter(|(_, ok)| !ok).map(|(n, _)| *n).collect();
    Ok((failed.is_empty(), format!("failed={failed:?} {detail}")))
}

fn fin(name: &str, r: Res) {
    match r {
        Ok((ok, why)) => check(name, ok, &why),
        Err(why) => check(name, false, &why),
    }
}

// ---------------------------------------------------------------------------------------
// Files on the image
// ---------------------------------------------------------------------------------------

/// `cat PATH` to a line list. Never `run`; errors are fixed texts (the transcript may hold
/// secrets). The driver's transcripts are cleared before returning.
fn read_lines(a: &mut Term, path: &str) -> Result<Vec<String>, &'static str> {
    let cmd = format!("cat {path}");
    a.send(&cmd).map_err(|_| "send")?;
    a.wait_prompt(SSH_MS).map_err(|_| "no prompt after cat")?;
    let mut lines = a.result(&cmd);
    a.out.clear();
    a.err.clear();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Ok(lines)
}

/// The armored text of `/kssh/STEM.key`, read once. Its body lines become scan canaries.
fn key_text(a: &mut Term, c: &mut Ctx, stem: &str) -> Result<String, String> {
    if let Some((_, text)) = c.texts.iter().find(|(s, _)| s == stem) {
        return Ok(text.clone());
    }
    let lines = read_lines(a, &format!("/kssh/{stem}.key")).map_err(|why| format!("key file {stem}: {why}"))?;
    let shaped = lines.len() >= 3
        && lines.first().map(String::as_str) == Some(ARMOR_BEGIN)
        && lines.last().map(String::as_str) == Some(ARMOR_END);
    if !shaped {
        return Err(format!("key file {stem}: not an armored block"));
    }
    for line in &lines[1..lines.len() - 1] {
        if line.len() >= 8 {
            c.needles.push((format!("body:{stem}"), line.clone()));
        }
    }
    let mut text = lines.join("\n");
    text.push('\n');
    c.texts.push((String::from(stem), text.clone()));
    Ok(text)
}

// ---------------------------------------------------------------------------------------
// ssh attempts
// ---------------------------------------------------------------------------------------

/// What the driver does when the passphrase prompt shows.
#[derive(Clone, Copy)]
enum Typed<'a> {
    /// Type this passphrase and Enter.
    Pass(&'a str),
    /// Press Escape.
    Escape,
    /// No prompt is expected; Escape if one shows (nothing is ever typed).
    Never,
}

struct Spec<'a> {
    id: &'a str,
    cmd: String,
    key: &'a str,
    secret: Typed<'a>,
    /// Answer to the first-trust prompt (`Type 'yes' and Enter`), when one is expected.
    trust: Option<&'a str>,
    /// Read the shell's `completion` record afterwards.
    rec: bool,
    ms: u64,
    during: Option<fn(&mut Term) -> Result<(), String>>,
}

fn spec<'a>(id: &'a str, cmd: String, key: &'a str, secret: Typed<'a>) -> Spec<'a> {
    let ms = if matches!(secret, Typed::Pass(_)) { KDF_MS } else { SSH_MS };
    Spec { id, cmd, key, secret, trust: None, rec: true, ms, during: None }
}

/// What one ssh attempt left behind.
struct Pk {
    st: String,
    out: Vec<String>,
    err: String,
    /// The passphrase prompt appeared.
    seen: bool,
    /// A passphrase was typed.
    typed: bool,
    /// Occurrences of the passphrase prompt text on stderr.
    prompts: usize,
    /// First-trust prompt shown with the host fingerprint (None: no trust answer given).
    shown: Option<bool>,
    ms: u64,
    /// Enter at the prompt to the shell's prompt: stage + KDF + sign + failure + exit.
    enter_ms: Option<u64>,
    rec: Result<Record, String>,
    /// The step's transcript held a secret: `err` is not to be printed.
    withheld: bool,
}

impl Pk {
    fn describe(&self) -> String {
        let err = if self.withheld { String::from("<withheld>") } else { clip(self.err.trim()) };
        format!(
            "{} seen={} prompts={} ms={} code={} err={err}",
            self.st,
            self.seen,
            self.prompts,
            self.ms,
            rec_code(&self.rec)
        )
    }

    fn rec_has(&self, want: &[(&str, &str)]) -> bool {
        self.rec.as_ref().is_ok_and(|r| has(r, want))
    }

    fn err_has(&self, what: &str) -> bool {
        self.err.contains(what)
    }
}

fn attempt_pk(a: &mut Term, c: &mut Ctx, s: &Spec) -> Result<Pk, String> {
    println(&format!("P04 INFO kssh-attempt {}", s.id));
    let r = attempt_pk_inner(a, c, s);
    guard(a, c, s.id, r)
}

fn attempt_pk_inner(a: &mut Term, c: &mut Ctx, s: &Spec) -> Result<Pk, String> {
    let start = now_ms();
    a.send(&s.cmd)?;
    let mut shown = None;
    if let Some(answer) = s.trust {
        wait_stdout(a, b"Type 'yes' and Enter")?;
        shown = Some(String::from_utf8_lossy(&a.out).contains(c.conf.hostfp.as_str()));
        type_keys(a, answer)?;
        press(a, ENTER)?;
    }
    let seen = wait_stderr_or_prompt(a, &prompt_text(s.key), PROMPT_MS)?;
    let (mut typed, mut enter_at) = (false, None);
    // A prompt that is already followed by the shell prompt has been answered by the end of
    // the command (timeout); nothing is typed into the shell.
    if seen {
        // The reader takes the echo-off lease right after printing the prompt: let it.
        sleep_ms(300);
        a.pump();
    }
    if seen && !a.at_prompt() {
        match s.secret {
            Typed::Pass(pass) => {
                type_keys(a, pass)?;
                press(a, ENTER)?;
                enter_at = Some(now_ms());
                typed = true;
            }
            Typed::Escape | Typed::Never => press(a, ESCAPE)?,
        }
    }
    if let Some(during) = s.during {
        during(a)?;
    }
    a.wait_prompt(s.ms)?;
    let end = now_ms();
    let out = out_lines(a);
    let err = a.stderr_text();
    let hits = c.hits;
    scan(a, c, s.id);
    let withheld = c.hits != hits;
    let prompts = count_in(&err, &prompt_text(s.key));
    let st = status(a)?;
    let rec = if s.rec { completion(a) } else { Err(String::from("not read")) };
    let enter_ms = enter_at.map(|t| end.saturating_sub(t));
    if typed {
        println(&format!(
            "P04 INFO kssh-obs {} ms={} enter-ms={}",
            s.id,
            end.saturating_sub(start),
            enter_ms.unwrap_or(0)
        ));
    }
    Ok(Pk { st, out, err, seen, typed, prompts, shown, ms: end.saturating_sub(start), enter_ms, rec, withheld })
}

fn pk_cmd(port: u16, key: &str, host: &str, command: &str) -> String {
    format!("ssh --user tester --auth publickey --identity {key} -p {port} {host} --command '{command}'")
}

/// Conditions of a correct login: one prompt, the typed passphrase, exact stdout, exit 0.
fn login_ok(c: &Ctx, p: &Pk) -> Vec<(&'static str, bool)> {
    alloc::vec![
        ("st0", p.st == "st=0"),
        ("stdout", p.out == [c.token()]),
        ("prompt-once", p.seen && p.prompts == 1 && p.typed),
        ("no-password-prompt", !p.err_has("password: ")),
        ("rec", p.rec_has(&[("mode", "exec"), ("outcome", "remote_exit"), ("remote_status", "0"), ("error_code", "null")])),
    ]
}

/// Conditions of a refusal after the prompt: exit 255, the stable message, no second prompt.
fn refused_after_prompt(p: &Pk, message: &str, code: &str) -> Vec<(&'static str, bool)> {
    alloc::vec![
        ("st255", p.st == "st=255"),
        ("message", p.err_has(&format!("ssh: {message}"))),
        ("prompt-once", p.seen && p.prompts == 1),
        ("no-password-prompt", !p.err_has("password: ")),
        ("no-stdout", p.out.is_empty()),
        ("code", rec_code(&p.rec) == code),
    ]
}

// ---------------------------------------------------------------------------------------
// admin cell
// ---------------------------------------------------------------------------------------

struct Adm {
    st: String,
    out: Vec<String>,
    err: String,
    ms: u64,
    withheld: bool,
}

impl Adm {
    fn describe(&self) -> String {
        let err = if self.withheld { String::from("<withheld>") } else { clip(self.err.trim()) };
        format!("{} ms={} err={err}", self.st, self.ms)
    }
}

fn adm_finish(a: &mut Term, c: &mut Ctx, id: &str, start: u64) -> Result<Adm, String> {
    let end = now_ms();
    let out = out_lines(a);
    let err = a.stderr_text();
    let hits = c.hits;
    scan(a, c, id);
    let withheld = c.hits != hits;
    let st = status(a)?;
    Ok(Adm { st, out, err, ms: end.saturating_sub(start), withheld })
}

/// `kms-ssh-admin status [NAME]` and `delete NAME` refusals before any prompt.
fn admin(a: &mut Term, c: &mut Ctx, cmd: &str) -> Result<Adm, String> {
    let start = now_ms();
    let r = (|| {
        a.send(cmd)?;
        a.wait_prompt(SSH_MS)
    })();
    guard(a, c, cmd, r)?;
    let r = adm_finish(a, c, cmd, start);
    guard(a, c, cmd, r)
}

/// `kms-ssh-admin import NAME` with `text` pasted at the prompt (nothing is echoed).
fn admin_import(a: &mut Term, c: &mut Ctx, name: &str, text: &str) -> Result<Adm, String> {
    let id = format!("import {name}");
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
    guard(a, c, &id, r)?;
    let r = adm_finish(a, c, &id, start);
    guard(a, c, &id, r)
}

/// `kms-ssh-admin delete NAME`, typing `word` at the confirmation prompt. The second value
/// is whether the typed word was echoed (None: no prompt came).
fn admin_delete(a: &mut Term, c: &mut Ctx, name: &str, word: &str) -> Result<(Adm, Option<bool>), String> {
    let id = format!("delete {name}");
    let start = now_ms();
    let r = (|| {
        a.send(&format!("kms-ssh-admin delete {name}"))?;
        let mut echoed = None;
        if wait_stderr_or_prompt(a, "Type 'delete' and press Enter", SSH_MS)? {
            sleep_ms(300);
            a.pump();
            let marks = (a.out.len(), a.err.len());
            type_keys(a, word)?;
            sleep_ms(500);
            a.pump();
            echoed = Some(find(&a.out[marks.0..], word.as_bytes()).is_some() || find(&a.err[marks.1..], word.as_bytes()).is_some());
            press(a, ENTER)?;
        }
        a.wait_prompt(SSH_MS)?;
        Ok::<_, String>(echoed)
    })();
    let echoed = guard(a, c, &id, r)?;
    let r = adm_finish(a, c, &id, start);
    Ok((guard(a, c, &id, r)?, echoed))
}

/// `import` outcome facts from the admin's stdout (public values only).
fn imported_facts(out: &[String]) -> (Option<String>, Option<String>) {
    let fp = kv(out, "  fingerprint: ").map(String::from);
    let prefix = kv(out, "  blob: ").and_then(|l| l.rsplit(' ').next()).map(String::from);
    (fp, prefix)
}

// ---------------------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------------------

/// S00: the control login with the fixture password.
fn s00(a: &mut Term, c: &mut Ctx) -> Res {
    println("P04 INFO kssh-attempt s00");
    let f = Fixture {
        port: c.conf.main,
        fp: c.conf.hostfp.clone(),
        password: c.conf.password.clone(),
        rekey_port: Some(c.conf.rekey),
    };
    let r = (|| {
        login(a, &f, &format!("{HOST} --command 'echo kssh-s00'"), &f.password)?;
        a.wait_prompt(SSH_MS)
    })();
    guard(a, c, "s00", r)?;
    let out = out_lines(a);
    let err = a.stderr_text();
    scan(a, c, "s00");
    let st = status(a)?;
    verdict(
        &[("st0", st == "st=0"), ("stdout", out == ["kssh-s00"])],
        format!("{st} out={} err={}", out.len(), clip(err.trim())),
    )
}

/// S01a: `kms-ssh-admin status` on the empty store.
fn s01a(a: &mut Term, c: &mut Ctx) -> Res {
    let r = admin(a, c, "kms-ssh-admin status")?;
    verdict(
        &[
            ("st0", r.st == "st=0"),
            ("leaf", has_line(&r.out, "kms-ssh-admin: key service ssh leaf")),
            ("ready", has_line(&r.out, "  readiness: Ready")),
            ("keys0", has_line(&r.out, "  keys: 0")),
            ("posture", has_line(&r.out, "  posture: DEV_POSTURE volatile: lost when the key service restarts")),
        ],
        r.describe(),
    )
}

/// S01b: the import refusals on the empty store.
fn s01b(a: &mut Term, c: &mut Ctx) -> Res {
    let not_a_key = format!("kssh-not-a-key\n{ARMOR_END}\n");
    let first = admin_import(a, c, "kbad", &not_a_key)?;
    let aes = key_text(a, c, "bad-aes128")?;
    let second = admin_import(a, c, "kbad", &aes)?;
    let chacha = key_text(a, c, "bad-chacha")?;
    let third = admin_import(a, c, "kbad", &chacha)?;
    let after = admin(a, c, "kms-ssh-admin status")?;
    let format = "kms-ssh-admin: key format refused (openssh-key-v1 ed25519; none, or bcrypt + aes256-ctr only)";
    println(&format!("P04 INFO kssh-obs s01b fmt-ms aes128={} chacha={}", second.ms, third.ms));
    verdict(
        &[
            ("text-st2", first.st == "st=2"),
            (
                "text-msg",
                first.err.contains("kms-ssh-admin: the key text must start with the BEGIN OPENSSH PRIVATE KEY line"),
            ),
            ("aes-st1", second.st == "st=1"),
            ("aes-msg", second.err.contains(format)),
            ("chacha-st1", third.st == "st=1"),
            ("chacha-msg", third.err.contains(format)),
            ("still-empty", after.st == "st=0" && has_line(&after.out, "  keys: 0")),
        ],
        format!("{} | {} | {} | {}", first.describe(), second.describe(), third.describe(), after.describe()),
    )
}

/// S01: import `ka16` and compare the admin's public facts with the host's.
fn s01(a: &mut Term, c: &mut Ctx) -> Res {
    let text = key_text(a, c, "ka16")?;
    let (fp, prefix) = {
        let key = c.key("ka16")?;
        (key.fp.clone(), key.prefix.clone())
    };
    let imp = admin_import(a, c, "ka16", &text)?;
    let (got_fp, got_prefix) = imported_facts(&imp.out);
    println(&format!(
        "P04 INFO kssh-obs s01 fp={} prefix={}",
        got_fp.as_deref().map(clip).unwrap_or_default(),
        got_prefix.as_deref().map(clip).unwrap_or_default()
    ));
    let one = admin(a, c, "kms-ssh-admin status ka16")?;
    let leaf = admin(a, c, "kms-ssh-admin status")?;
    verdict(
        &[
            ("st0", imp.st == "st=0"),
            ("imported", has_line(&imp.out, "kms-ssh-admin: imported key 'ka16'")),
            ("wrapped", has_line(&imp.out, "  custody: passphrase-wrapped")),
            ("fp", got_fp.as_deref() == Some(fp.as_str())),
            ("blob-prefix", got_prefix.as_deref() == Some(prefix.as_str())),
            ("posture", has_line(&imp.out, "  posture: DEV_POSTURE volatile: lost when the key service restarts")),
            ("status-ka16", one.st == "st=0" && kv(&one.out, "  fingerprint: ") == Some(fp.as_str())),
            ("keys1", leaf.st == "st=0" && has_line(&leaf.out, "  keys: 1")),
        ],
        format!("{} | {} | {}", imp.describe(), one.describe(), leaf.describe()),
    )
}

/// S09: admin refusals and a cancelled delete leave the store unchanged.
fn s09(a: &mut Term, c: &mut Ctx) -> Res {
    let fp = c.key("ka16")?.fp.clone();
    let ka16 = key_text(a, c, "ka16")?;
    let kl4 = key_text(a, c, "kl4")?;
    let same = admin_import(a, c, "ka16", &ka16)?;
    let other = admin_import(a, c, "kx", &kl4)?;
    let nope = admin(a, c, "kms-ssh-admin delete nope")?;
    let (cancelled, echoed) = admin_delete(a, c, "ka16", "nope")?;
    println(&format!(
        "P04 INFO kssh-obs delete-word-echo echoed={}",
        echoed.map_or("no-prompt", |e| if e { "1" } else { "0" })
    ));
    let one = admin(a, c, "kms-ssh-admin status ka16")?;
    let leaf = admin(a, c, "kms-ssh-admin status")?;
    verdict(
        &[
            ("same-st1", same.st == "st=1"),
            (
                "same-msg",
                same.err.contains(
                    "kms-ssh-admin: an identity with this name or public key already exists; delete the existing identity first",
                ),
            ),
            ("other-st1", other.st == "st=1"),
            (
                "other-msg",
                other.err.contains(
                    "kms-ssh-admin: the key store is full: delete the existing identity first, or the key exceeds the store budget",
                ),
            ),
            ("nope-st1", nope.st == "st=1"),
            ("nope-msg", nope.err.contains("kms-ssh-admin: no such key")),
            ("cancel-prompted", echoed.is_some()),
            ("cancel-st130", cancelled.st == "st=130"),
            ("cancel-msg", cancelled.err.contains("kms-ssh-admin: cancelled; nothing deleted")),
            ("intact", one.st == "st=0" && kv(&one.out, "  fingerprint: ") == Some(fp.as_str())),
            ("keys1", leaf.st == "st=0" && has_line(&leaf.out, "  keys: 1")),
        ],
        format!("{} | {} | {} | {}", same.describe(), other.describe(), nope.describe(), cancelled.describe()),
    )
}

/// S10: a changed or untrusted host key is refused before any credential.
fn s10(a: &mut Term, c: &mut Ctx) -> Res {
    let main = c.conf.main;
    let changed = attempt_pk(a, c, &spec("s10-changed", pk_cmd(main, "ka16", "host", "true"), "ka16", Typed::Never))?;
    let mut untrusted = spec("s10-untrusted", pk_cmd(main, "ka16", "gateway", "true"), "ka16", Typed::Never);
    untrusted.trust = Some("no");
    let untrusted = attempt_pk(a, c, &untrusted)?;
    verdict(
        &[
            ("changed-st255", changed.st == "st=255"),
            ("changed-msg", changed.err_has(&format!("HOST KEY CHANGED for host:{main}")) && changed.err_has("refused")),
            ("changed-no-prompt", !changed.seen && !changed.err_has("password: ")),
            ("changed-not-sent", changed.rec_has(&[("dispatch", "not_sent")])),
            ("untrusted-shown", untrusted.shown == Some(true)),
            ("untrusted-st255", untrusted.st == "st=255"),
            (
                "untrusted-msg",
                untrusted.err_has(&format!("host key for gateway:{main} was not trusted")) && untrusted.err_has("refused"),
            ),
            ("untrusted-no-prompt", !untrusted.seen && !untrusted.err_has("password: ")),
            ("untrusted-not-sent", untrusted.rec_has(&[("dispatch", "not_sent")])),
        ],
        format!("{} | {}", changed.describe(), untrusted.describe()),
    )
}

/// S04a: an unknown identity is refused before any network traffic.
fn s04a(a: &mut Term, c: &mut Ctx) -> Res {
    let p = attempt_pk(a, c, &spec("s04a", pk_cmd(c.conf.main, "NOPE0", HOST, "true"), "NOPE0", Typed::Never))?;
    verdict(
        &[
            ("st255", p.st == "st=255"),
            ("msg", p.err_has("ssh: no such identity 'NOPE0'")),
            ("no-prompt", !p.seen && !p.err_has("password: ")),
            ("code", rec_code(&p.rec) == "identity_unknown"),
        ],
        p.describe(),
    )
}

/// S04b: a key the server does not authorize (port B, first trust answered `yes`).
fn s04b(a: &mut Term, c: &mut Ctx) -> Res {
    let mut s = spec("s04b", pk_cmd(c.conf.b, "ka16", HOST, "true"), "ka16", Typed::Never);
    s.trust = Some("yes");
    let p = attempt_pk(a, c, &s)?;
    verdict(
        &[
            ("trust-shown", p.shown.is_some()),
            ("st255", p.st == "st=255"),
            ("trust-stored", p.err_has("added to the trust store")),
            ("msg", p.err_has("ssh: authentication failed")),
            ("no-prompt", !p.seen && !p.err_has("password: ")),
        ],
        p.describe(),
    )
}

fn denied(r: Result<(), KmsClientError>) -> (bool, String) {
    match r {
        Err(KmsClientError::Service(KmsErrorCode::PermissionDenied)) => (true, String::from("PermissionDenied")),
        other => (false, format!("{other:?}")),
    }
}

/// S06a: the driver is an attested caller without a KMS SSH flag: every SSH opcode of the
/// public client is `PermissionDenied`.
fn s06a() -> Res {
    let kms = KmsClient::connect().map_err(|e| format!("connect {e:?}"))?;
    let zero = [0u8; 32];
    let results = [
        ("status", denied(kms.ssh_key_status(b"").map(|_| ()))),
        ("unlock_begin", denied(kms.ssh_unlock_begin(&zero, 1).map(|_| ()))),
        ("sign", denied(kms.ssh_sign_userauth(&zero, &[1u8; 32], b"tester").map(|_| ()))),
        ("import_begin", denied(kms.ssh_import_begin(b"kx", 1, &[0u8; 32]).map(|_| ()))),
        ("delete", denied(kms.ssh_delete(b"kx", &zero).map(|_| ()))),
    ];
    let conds: Vec<(&str, bool)> = results.iter().map(|(n, (ok, _))| (*n, *ok)).collect();
    let detail = results.iter().map(|(n, (_, d))| format!("{n}={d}")).collect::<Vec<_>>().join(" ");
    verdict(&conds, detail)
}

const KMS_FRAME: usize = 128;
const RAW_TICKS: u64 = 500;

/// A canonical 128-byte request header with an empty payload.
fn kms_frame(opcode: u8, flags: u16, request_id: u32) -> [u8; KMS_FRAME] {
    let mut f = [0u8; KMS_FRAME];
    f[0] = 1;
    f[1] = opcode;
    f[2..4].copy_from_slice(&flags.to_le_bytes());
    f[4..8].copy_from_slice(&request_id.to_le_bytes());
    f
}

/// Bytes shaped as a caller-identity trailer: tag, flag bit 6 (the sign right), a nonzero
/// cell id. The kernel writes the real trailer after the payload copy.
fn forged_trailer() -> [u8; 32] {
    let mut t = [0u8; 32];
    t[0..4].copy_from_slice(&0x5649_4349u32.to_le_bytes());
    t[4..8].copy_from_slice(&(1u32 << 6).to_le_bytes());
    t[8..16].copy_from_slice(&1u64.to_le_bytes());
    t[16..24].copy_from_slice(&1u64.to_le_bytes());
    t[24..32].copy_from_slice(&1u64.to_le_bytes());
    t
}

enum Raw {
    Reply([u8; KMS_FRAME]),
    Silent,
    SendFailed,
    Other(&'static str),
}

/// One raw frame out, one reply or silence within `ticks` scheduler ticks (the receive
/// shape of the public client: mask = the service's tid).
fn kms_raw(tid: usize, msg: &[u8], ticks: u64) -> Raw {
    match sys_send(tid, msg) {
        SyscallResult::Ok(_) => {}
        SyscallResult::Err(_) => return Raw::SendFailed,
    }
    let mut buf = [0u8; KMS_FRAME];
    match sys_recv_timeout(tid, &mut buf, ticks) {
        SyscallResult::Ok(0) => {
            // One non-blocking drain, as the client does: a reply that raced the bound.
            let mut late = [0u8; KMS_FRAME];
            let _ = sys_try_recv(tid, &mut late);
            Raw::Silent
        }
        SyscallResult::Ok(sender) if sender == tid => Raw::Reply(buf),
        SyscallResult::Ok(_) => Raw::Other("wrong sender"),
        SyscallResult::Err(_) => Raw::Other("recv failed"),
    }
}

/// The error code of a reply frame that echoes `opcode` and `request_id`.
fn reply_code(frame: &[u8; KMS_FRAME], opcode: u8, request_id: u32) -> Result<u16, &'static str> {
    if frame[0] != 1 || frame[1] != opcode || frame[4..8] != request_id.to_le_bytes() {
        return Err("reply does not echo the request");
    }
    if frame[2] != 1 {
        return Err("reply is not an error");
    }
    Ok(u16::from_le_bytes([frame[8], frame[9]]))
}

const PERMISSION_DENIED: u16 = 2;
const SIGN_OPCODE: u8 = 21;

fn describe_raw(r: &Raw, opcode: u8, id: u32) -> String {
    match r {
        Raw::Reply(frame) => match reply_code(frame, opcode, id) {
            Ok(code) => format!("code={code}"),
            Err(why) => String::from(why),
        },
        Raw::Silent => String::from("silent"),
        Raw::SendFailed => String::from("send-failed"),
        Raw::Other(why) => String::from(*why),
    }
}

/// S06b: a frame padded to the whole IPC buffer whose last 32 bytes look like a trailer with
/// flag bit 6 cannot pass admission: the kernel writes the real trailer last.
fn s06b() -> Res {
    const ID: u32 = 0x0510_6b00;
    let tid = sys_lookup_service(api::syscall::service::KMS).ok_or("no key service")?;
    let mut sent = None;
    let mut outcome = Raw::SendFailed;
    for len in [api::ipc::IPC_BUF_SIZE, api::ipc::IPC_BUF_SIZE - 32] {
        let mut msg = alloc::vec![0u8; len];
        msg[..KMS_FRAME].copy_from_slice(&kms_frame(SIGN_OPCODE, 0, ID));
        msg[len - 32..].copy_from_slice(&forged_trailer());
        outcome = kms_raw(tid, &msg, RAW_TICKS);
        if !matches!(outcome, Raw::SendFailed) {
            sent = Some(len);
            break;
        }
    }
    let sent_len = sent.unwrap_or(0);
    // Reaching the trailer region needs the whole buffer; shorter, the claim degrades to
    // "the kernel-written trailer is not forgeable at this length".
    let reach = if sent_len == api::ipc::IPC_BUF_SIZE { "full" } else { "short" };
    println(&format!("P04 INFO kssh-obs s06b-sent={sent_len}"));
    println(&format!("P04 INFO kssh-obs s06b reach={reach} {}", describe_raw(&outcome, SIGN_OPCODE, ID)));
    let code = match &outcome {
        Raw::Reply(frame) => reply_code(frame, SIGN_OPCODE, ID).ok(),
        _ => None,
    };
    verdict(&[("sent", sent.is_some()), ("permission-denied", code == Some(PERMISSION_DENIED))], format!("reach={reach} {}", describe_raw(&outcome, SIGN_OPCODE, ID)))
}

/// S06c: noncanonical frames (nonzero header flags, undefined opcode 25) get no reply, and
/// the service still answers the next canonical frame.
fn s06c() -> Res {
    const CONTROL: u32 = 0x0510_6c03;
    let tid = sys_lookup_service(api::syscall::service::KMS).ok_or("no key service")?;
    let flags_op25 = kms_raw(tid, &kms_frame(25, 1, 0x0510_6c01), RAW_TICKS);
    let flags_sign = kms_raw(tid, &kms_frame(SIGN_OPCODE, 1, 0x0510_6c02), RAW_TICKS);
    let control = kms_raw(tid, &kms_frame(SIGN_OPCODE, 0, CONTROL), RAW_TICKS);
    let code = match &control {
        Raw::Reply(frame) => reply_code(frame, SIGN_OPCODE, CONTROL).ok(),
        _ => None,
    };
    println(&format!(
        "P04 INFO kssh-obs s06c flags-op25={} flags-sign={} control={}",
        describe_raw(&flags_op25, 25, 0x0510_6c01),
        describe_raw(&flags_sign, SIGN_OPCODE, 0x0510_6c02),
        describe_raw(&control, SIGN_OPCODE, CONTROL)
    ));
    verdict(
        &[
            ("op25-silent", matches!(flags_op25, Raw::Silent)),
            ("flags-silent", matches!(flags_sign, Raw::Silent)),
            ("control-denied", code == Some(PERMISSION_DENIED)),
        ],
        String::from("see kssh-obs s06c"),
    )
}

/// S06d: the same ELF launched as `/bin/cellbox ssh ...` (not the `/bin/cellbox-ssh` edge)
/// must not reach a signature: kms_denied, a spawn refusal, or any refusal with no prompt.
fn s06d(a: &mut Term, c: &mut Ctx) -> Res {
    let cmd = format!(
        "/bin/cellbox ssh --user tester --auth publickey --identity ka16 -p {} {HOST} --command true",
        c.conf.main
    );
    let mut s = spec("s06d", cmd, "ka16", Typed::Never);
    s.rec = false;
    let p = attempt_pk(a, c, &s)?;
    let form = if p.seen {
        String::from("prompt-shown")
    } else if p.err_has("ssh: not permitted to use the key service") {
        String::from("kms_denied")
    } else if p.st == "st=0" {
        String::from("ran")
    } else if p.err.trim().is_empty() {
        String::from("silent-refusal")
    } else {
        let line = p.err.trim().lines().next().unwrap_or("");
        format!("refused:{}", clip(line).replace(' ', "_"))
    };
    println(&format!("P04 INFO kssh-obs s06d-form={form}"));
    verdict(
        &[("no-prompt", !p.seen), ("refused", p.st != "st=0"), ("no-stdout", p.out.is_empty())],
        format!("form={form} {}", p.describe()),
    )
}

/// S02 (and S02', S08's final login): a login with the stored key.
fn s02(a: &mut Term, c: &mut Ctx, id: &str, key: &str) -> Res {
    let pass = c.pass(key)?;
    let cmd = pk_cmd(c.conf.main, key, HOST, &format!("p9 kssh-login; echo {}", c.token()));
    let p = attempt_pk(a, c, &spec(id, cmd, key, Typed::Pass(&pass)))?;
    verdict(&login_ok(c, &p), p.describe())
}

fn pty_session(t: &mut Term) -> Result<(), String> {
    // Typed input before the client enters raw mode is dropped: wait for the remote
    // shell's first output.
    wait_remote_output(t)?;
    sleep_ms(500);
    type_keys(t, "echo T=$TERM")?;
    press(t, ENTER)?;
    wait_stdout(t, b"T=cellos-vt1")?;
    type_keys(t, "exit")?;
    press(t, ENTER)
}

/// S02p: the same login in a PTY session.
fn s02p(a: &mut Term, c: &mut Ctx) -> Res {
    let pass = c.pass("ka16")?;
    let cmd = format!("ssh --user tester --auth publickey --identity ka16 -p {} {HOST}", c.conf.main);
    let mut s = spec("s02p", cmd, "ka16", Typed::Pass(&pass));
    s.during = Some(pty_session);
    let p = attempt_pk(a, c, &s)?;
    verdict(
        &[
            ("st0", p.st == "st=0"),
            ("prompt-once", p.seen && p.prompts == 1 && p.typed),
            ("term", p.out.iter().any(|l| l.contains("T=cellos-vt1"))),
            ("rec", p.rec_has(&[("mode", "pty"), ("outcome", "remote_exit"), ("remote_status", "0")])),
        ],
        p.describe(),
    )
}

/// S03: the wrong passphrase.
fn s03(a: &mut Term, c: &mut Ctx) -> Res {
    let wrong = c.wrong("ka16")?;
    let cmd = pk_cmd(c.conf.main, "ka16", HOST, &format!("p9 kssh-login; echo {}", c.token()));
    let p = attempt_pk(a, c, &spec("s03", cmd, "ka16", Typed::Pass(&wrong)))?;
    println(&format!("P04 INFO kssh-obs unlock-ms ms={}", p.enter_ms.unwrap_or(0)));
    verdict(&refused_after_prompt(&p, "unlock failed", "unlock_failed"), p.describe())
}

/// S03b: Escape at the passphrase prompt.
fn s03b(a: &mut Term, c: &mut Ctx) -> Res {
    let cmd = pk_cmd(c.conf.main, "ka16", HOST, &format!("p9 kssh-login; echo {}", c.token()));
    let mut s = spec("s03b", cmd, "ka16", Typed::Escape);
    s.ms = PROMPT_MS;
    let p = attempt_pk(a, c, &s)?;
    verdict(&refused_after_prompt(&p, "unlock cancelled", "unlock_cancelled"), p.describe())
}

/// S08: the limiter. Five wrong passphrases, then the correct one is refused as locked;
/// a delete and re-import resets it.
fn s08(a: &mut Term, c: &mut Ctx) -> Res {
    let mut conds: Vec<(&str, bool)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let (pass, wrong) = (c.pass("kl4")?, c.wrong("kl4")?);
    let kl4 = key_text(a, c, "kl4")?;
    let cmd = pk_cmd(c.conf.main, "kl4", HOST, &format!("p9 kssh-login; echo {}", c.token()));

    let (del, _) = admin_delete(a, c, "ka16", "delete")?;
    let imp = admin_import(a, c, "kl4", &kl4)?;
    conds.push(("delete-ka16", del.st == "st=0" && has_line(&del.out, "kms-ssh-admin: deleted key 'ka16'")));
    conds.push(("import-kl4", imp.st == "st=0" && has_line(&imp.out, "kms-ssh-admin: imported key 'kl4'")));

    let mut fail_ms = Vec::new();
    for n in 1..=5 {
        let id = format!("s08-w{n}");
        let p = attempt_pk(a, c, &spec(&id, cmd.clone(), "kl4", Typed::Pass(&wrong)))?;
        fail_ms.push(p.enter_ms.unwrap_or(0));
        conds.push(("wrong-refused", refused_after_prompt(&p, "unlock failed", "unlock_failed").iter().all(|(_, ok)| *ok)));
        notes.push(format!("w{n}:{}", p.describe()));
    }
    let locked = attempt_pk(a, c, &spec("s08-locked", cmd.clone(), "kl4", Typed::Pass(&pass)))?;
    println(&format!(
        "P04 INFO kssh-obs s08 fail-ms={} locked-ms={}",
        fail_ms.iter().map(|m| format!("{m}")).collect::<Vec<_>>().join(","),
        locked.enter_ms.unwrap_or(0)
    ));
    conds.push((
        "sixth-locked",
        refused_after_prompt(&locked, "too many failed unlocks; key locked", "unlock_locked").iter().all(|(_, ok)| *ok),
    ));

    let (del, _) = admin_delete(a, c, "kl4", "delete")?;
    let imp = admin_import(a, c, "kl4", &kl4)?;
    conds.push(("delete-kl4", del.st == "st=0" && has_line(&del.out, "kms-ssh-admin: deleted key 'kl4'")));
    conds.push(("reimport-kl4", imp.st == "st=0" && has_line(&imp.out, "kms-ssh-admin: imported key 'kl4'")));
    let ok = attempt_pk(a, c, &spec("s08-ok", cmd, "kl4", Typed::Pass(&pass)))?;
    conds.extend(login_ok(c, &ok).into_iter().map(|(n, v)| (if n == "st0" { "ok-st0" } else { n }, v)));
    notes.push(format!("locked:{}", locked.describe()));
    notes.push(format!("ok:{}", ok.describe()));
    verdict(&conds, notes.join(" | "))
}

/// S15: the pipeline stage (K6b): no prompt ever, plain keys only.
fn s15(a: &mut Term, c: &mut Ctx) -> Res {
    let main = c.conf.main;
    let kp0 = key_text(a, c, "kp0")?;
    let feed = "cat /kssh/raw.bin | ";

    let one = attempt_pk(
        a,
        c,
        &spec("s15-i", format!("{feed}{}", pk_cmd(main, "kl4", HOST, "cat > /tmp/kssh-in")), "kl4", Typed::Never),
    )?;

    let (del, _) = admin_delete(a, c, "kl4", "delete")?;
    let imp = admin_import(a, c, "kp0", &kp0)?;

    let three = attempt_pk(
        a,
        c,
        &spec("s15-iii", format!("{feed}{}", pk_cmd(main, "kp0", "gateway", "true")), "kp0", Typed::Never),
    )?;

    let four = attempt_pk(
        a,
        c,
        &spec(
            "s15-iv",
            format!("{feed}{}", pk_cmd(main, "kp0", HOST, "cat > /tmp/kssh-in; sha256sum /tmp/kssh-in")),
            "kp0",
            Typed::Never,
        ),
    )?;
    let sha = sha256_word(&four.out.join(" ")).map(String::from);
    println(&format!("P04 INFO kssh-obs s15 raw-len={}", c.conf.raw_len));
    verdict(
        &[
            ("i-st255", one.st == "st=255"),
            ("i-msg", one.err_has("ssh: encrypted key needs an interactive terminal")),
            ("i-code", rec_code(&one.rec) == "unlock_needs_terminal"),
            ("i-no-prompt", !one.seen && !one.err_has("password: ")),
            ("delete-kl4", del.st == "st=0"),
            ("import-kp0", imp.st == "st=0" && has_line(&imp.out, "  custody: plain")),
            (
                "plain-warning",
                has_line(&imp.out, "  WARNING: unencrypted key held by the key service (development posture)"),
            ),
            ("iii-st255", three.st == "st=255"),
            ("iii-msg", three.err_has("ssh: host key not trusted; confirmation needs an interactive frontend")),
            ("iii-no-prompt", !three.seen && !three.err_has("password: ")),
            ("iv-st0", four.st == "st=0"),
            ("iv-no-prompt", !four.seen && four.prompts == 0 && !four.err_has("password: ")),
            ("iv-sha", sha.as_deref() == Some(c.conf.raw_sha.as_str())),
            ("iv-rec", four.rec.is_ok()),
        ],
        format!("{} | {} | {}", one.describe(), three.describe(), four.describe()),
    )
}

// ---------------------------------------------------------------------------------------
// F mode (fault cellbox)
// ---------------------------------------------------------------------------------------

/// S05: one lane-only fault in the key-service request or reply; the client refuses and no
/// signature leaves it.
fn s05(a: &mut Term, c: &mut Ctx, id: &str, fault: &str, message: &str, code: &str) -> Res {
    let pass = c.pass("ka16")?;
    let cmd = pk_cmd(c.conf.main, "ka16", HOST, &format!(": kssh-fault {fault}; true"));
    let p = attempt_pk(a, c, &spec(id, cmd, "ka16", Typed::Pass(&pass)))?;
    verdict(&refused_after_prompt(&p, message, code), p.describe())
}

/// S11: a rekey after a publickey login on the `RekeyLimit 64K` port. The client's
/// `ssh-rekeys`/`ssh-bytes` lines inside the `p09-case` segment are judged on the host.
fn s11(a: &mut Term, c: &mut Ctx) -> Res {
    let pass = c.pass("ka16")?;
    println("P04 INFO p09-case kssh-s11");
    let cmd = pk_cmd(c.conf.rekey, "ka16", HOST, ": p09-sink; p9 kssh-rekey; head -c 1048576 /dev/zero");
    let mut s = spec("s11", cmd, "ka16", Typed::Pass(&pass));
    s.ms = BULK_MS + KDF_MS;
    let p = attempt_pk(a, c, &s)?;
    verdict(
        &[
            ("st0", p.st == "st=0"),
            ("prompt-once", p.seen && p.prompts == 1 && p.typed),
            ("no-password-prompt", !p.err_has("password: ")),
            (
                "rec",
                p.rec_has(&[
                    ("mode", "exec"),
                    ("phase", "complete"),
                    ("dispatch", "accepted"),
                    ("outcome", "remote_exit"),
                    ("remote_status", "0"),
                    ("remote_signal", "null"),
                    ("error_code", "null"),
                    ("stdout_complete", "true"),
                    ("stderr_complete", "true"),
                ]),
            ),
        ],
        p.describe(),
    )
}

// ---------------------------------------------------------------------------------------
// KDF calibration (B0)
// ---------------------------------------------------------------------------------------

/// Time one wrong-passphrase attempt (Enter to the shell's prompt: stage + KDF + sign +
/// failure + exit) for the keys of bcrypt rounds 1, 64 and 16, one at a time in the store.
/// Ends with `ka16` imported again, as after S01.
fn calibrate(a: &mut Term, c: &mut Ctx) {
    let r = calibrate_run(a, c);
    if let Err(why) = r {
        println(&format!("P04 INFO kssh-obs kdf-error {}", clip(&why)));
        restore_ka16(a, c);
    }
}

fn calibrate_run(a: &mut Term, c: &mut Ctx) -> Result<(), String> {
    let mut previous = "ka16";
    for name in ["kr1", "kr64", "ka16"] {
        let (rounds, wrong) = {
            let key = c.key(name)?;
            (key.rounds, c.wrong(name)?)
        };
        let (del, _) = admin_delete(a, c, previous, "delete")?;
        if del.st != "st=0" {
            return Err(format!("delete {previous}: {}", del.describe()));
        }
        let text = key_text(a, c, name)?;
        let imp = admin_import(a, c, name, &text)?;
        if imp.st != "st=0" {
            return Err(format!("import {name}: {}", imp.describe()));
        }
        previous = name;
        let id = format!("cal-R{rounds}");
        let cmd = pk_cmd(c.conf.main, name, HOST, "true");
        let p = attempt_pk(a, c, &spec(&id, cmd, name, Typed::Pass(&wrong)))?;
        println(&format!("P04 INFO kssh-obs kdf rounds={rounds} ms={}", p.enter_ms.unwrap_or(0)));
        if rec_code(&p.rec) != "unlock_failed" {
            println(&format!("P04 INFO kssh-obs kdf-error {id} {}", clip(&p.describe())));
        }
    }
    Ok(())
}

/// After a failed calibration: leave the store as S01 did (only `ka16`), best effort.
fn restore_ka16(a: &mut Term, c: &mut Ctx) {
    for name in ["kr1", "kr64"] {
        let _ = admin_delete(a, c, name, "delete");
    }
    let leaf = admin(a, c, "kms-ssh-admin status");
    if leaf.is_ok_and(|r| has_line(&r.out, "  keys: 0")) {
        if let Ok(text) = key_text(a, c, "ka16") {
            let _ = admin_import(a, c, "ka16", &text);
        }
    }
}

// ---------------------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------------------

pub(crate) fn kssh_checks(a: &mut Term) {
    if !wait_network(a) {
        println("P04 INFO ssh-network-ready never");
    }
    let lines = read_lines(a, "/kssh/kssh.conf");
    let hint_f = lines.as_ref().is_ok_and(|l| mode_hint_f(l));
    let markers: &[&str] = if hint_f { &F_MARKERS } else { &T0_MARKERS };
    let parsed = match &lines {
        Ok(lines) => parse_conf(lines),
        Err(why) => Err(*why),
    };
    check("kssh-conf", parsed.is_ok(), parsed.as_ref().err().copied().unwrap_or(""));
    let conf = match parsed {
        Ok(conf) => conf,
        Err(_) => return skip(&markers[1..], "no usable kssh.conf"),
    };
    println(&format!(
        "P04 INFO kssh-obs conf mode={} calibrate={} keys={}",
        if conf.f_mode { "f" } else { "t0" },
        conf.calibrate,
        conf.keys.len()
    ));
    let (f_mode, do_calibrate) = (conf.f_mode, conf.calibrate);
    let mut c = Ctx::new(conf);
    let c = &mut c;

    fin("kssh-s00", s00(a, c));
    if f_mode {
        fin("kssh-s01", s01(a, c));
        if do_calibrate {
            calibrate(a, c);
        }
        fin("kssh-s05a", s05(a, c, "s05a", "session", "key service signed a different input", "kms_input_mismatch"));
        fin("kssh-s05b", s05(a, c, "s05b", "user", "key service signed a different input", "kms_input_mismatch"));
        fin("kssh-s05c", s05(a, c, "s05c", "signature", "key service returned an invalid signature", "kms_bad_signature"));
        fin("kssh-s02", s02(a, c, "s02", "ka16"));
        fin("kssh-s11", s11(a, c));
    } else {
        fin("kssh-s01a", s01a(a, c));
        fin("kssh-s01b", s01b(a, c));
        fin("kssh-s01", s01(a, c));
        if do_calibrate {
            calibrate(a, c);
        }
        fin("kssh-s09", s09(a, c));
        fin("kssh-s10", s10(a, c));
        fin("kssh-s04a", s04a(a, c));
        fin("kssh-s04b", s04b(a, c));
        fin("kssh-s06a", s06a());
        fin("kssh-s06b", s06b());
        fin("kssh-s06c", s06c());
        fin("kssh-s06d", s06d(a, c));
        fin("kssh-s02", s02(a, c, "s02", "ka16"));
        fin("kssh-s02p", s02p(a, c));
        fin("kssh-s03", s03(a, c));
        fin("kssh-s03b", s03b(a, c));
        fin("kssh-s02x", s02(a, c, "s02x", "ka16"));
        fin("kssh-s08", s08(a, c));
        fin("kssh-s15", s15(a, c));
    }

    // The last transcripts, then the counts.
    scan(a, c, "end");
    println(&format!(
        "P04 INFO kssh-obs transcripts scans={} hits={} canaries={}",
        c.scans,
        c.hits,
        c.needles.len()
    ));
    check("kssh-transcripts", c.hits == 0 && c.scans > 0, &format!("hits={} scans={}", c.hits, c.scans));
}
