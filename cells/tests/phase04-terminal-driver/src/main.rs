//! Private Phase04 slice S3 driver. It links the same `FrontendHost` the GUI Terminal
//! uses and drives a real `cellbox sh` through it: the shell controls its own GUI-kind
//! session and this Cell is only its frontend (Phase 05 S5-3 annex option C; keyboard in
//! via `submit_*` on the frontend attachment, bytes out via `drain_*` of the rings the
//! shell relays to), printing `P04 PASS|FAIL <name>` observations. The host runner supplies
//! the independent verdict from serial output. Not part of any ordinary image: init
//! starts it only from the runner's private autoload plan, and the kernel carries its
//! `-> /bin/cellbox` launch edge only under `phase04-driver-profile`.
//!
//! Secrets: the driver never reads, stores or prints any. Every typed command is a fixed
//! script, the shells it launches get an empty environment, and a FAIL line carries only a
//! status name or a short printable slice of the scripted (non-secret) transcript. The one
//! token it types into `read -s` is a fixed fixture constant; that scenario's FAIL lines
//! carry statuses and counts only, so the runner can require the token absent from the UART.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
// The Phase 09 lane runs only the ssh checks; the Phase04 suite is compiled but unused.
#![cfg_attr(feature = "phase09-ssh", allow(dead_code, unused_imports))]

extern crate alloc;
extern crate ostd;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use api::cell_session::{ChildRefV1, InputOp, KernelOp, ObjectRefV1, Reader, Status, Writer, HEADER_LEN};
use core::sync::atomic::{AtomicUsize, Ordering};
use ostd::child_context::{ChildContext, Grant, GrantSlot};
use ostd::io::println;
use ostd::syscall::{sys_exit, sys_mem_info, sys_set_timer};
use terminal_sdk::{ChildPoll, FrontendHost, FrontendState};
use terminal_sdk::stream::{Endpoint, EndpointRights, Operation};

mod edit;
mod records;
mod oneshot;
mod rg;
mod screen;
#[cfg(feature = "phase09-ssh")]
mod ssh;
mod top_load;
use screen::{is_cell_identity, Color, Screen};

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    gpio = false,
    uart = false,
    hypervisor = false
);

// Exactly what FrontendHost needs (frontend register/bind/query/submit/drain, governed child
// launch through SpawnFromPath/PrepareChild/CommitChild, timing) plus MemInfo for the
// repeat-open/close leak check. No Wait, no SpawnFromElf, no capability.
api::declare_syscalls![
    Send,
    Recv,
    TryRecv,
    RecvTimeout,
    Reply,
    Log,
    Exit,
    Yield,
    Heartbeat,
    LookupService,
    GetTime,
    SetTimer,
    MemInfo,
    SpawnFromPath,
];

ostd::cell_main!(cell_main);

const SHELL: &str = "/bin/cellbox";
const PROMPT: &str = "Cellos > ";
const ENTER: u32 = 0x110001;
const CTRL: u32 = 2;
/// Transcript cap per stream; older bytes are dropped so an endless producer is bounded.
const CAP: usize = 128 * 1024;
const BINARY_PATH: &str = "/p04-output/raw.bin";
const LARGE_PATH: &str = "/p04-output/large.bin";
const LARGE_LEN: usize = 96 * 1024 + 64;

/// Must match the private runner's generated file, including a UTF-8 sequence split
/// over cat's 512-byte read boundary. Never decode command payloads for assertions.
fn fixture_bytes(len: usize) -> Vec<u8> {
    let mut bytes: Vec<u8> = (0..len).map(|i| ((i * 73 + 19) % 256) as u8).collect();
    for (index, chunk) in bytes.chunks_mut(512).enumerate() {
        let sentinel = format!("P04-CHUNK-{index:04}-NO-UART!");
        chunk[8..8 + sentinel.len()].copy_from_slice(sentinel.as_bytes());
    }
    bytes[31] = 0;
    bytes[32] = 0xff;
    bytes[511..514].copy_from_slice(&[0xe2, 0x82, 0xac]);
    bytes
}

static FAILURES: AtomicUsize = AtomicUsize::new(0);
static CHECKS: AtomicUsize = AtomicUsize::new(0);

fn now_ms() -> u64 {
    ostd::session::monotonic_ns().unwrap_or(0) / 1_000_000
}

fn sleep_ms(ms: u64) {
    let end = now_ms() + ms;
    while now_ms() < end {
        let _ = sys_set_timer(1);
    }
}

fn check(name: &str, ok: bool, why: &str) {
    CHECKS.fetch_add(1, Ordering::Relaxed);
    if ok {
        println(&format!("P04 PASS {name}"));
    } else {
        FAILURES.fetch_add(1, Ordering::Relaxed);
        println(&format!("P04 FAIL {name} {}", clip(why)));
    }
}

/// Hex encodes observed bytes solely as fixture evidence, never as a data fallback.
fn evidence(name: &str, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    println(&format!("P04 BYTES {name} len={}", bytes.len()));
    for (index, chunk) in bytes.chunks(128).enumerate() {
        let mut hex = String::with_capacity(chunk.len() * 2);
        for byte in chunk {
            hex.push(HEX[(byte >> 4) as usize] as char);
            hex.push(HEX[(byte & 15) as usize] as char);
        }
        println(&format!("P04 HEX {name} offset={} {hex}", index * 128));
    }
}

fn stream_evidence(name: &str, term: &Term, cmd: &str) {
    evidence(&format!("{name}.stdout"), term.raw_result(cmd).unwrap_or(&term.out));
    evidence(&format!("{name}.stderr"), &term.err);
}

fn clip(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars().take(200) {
        out.push(if ch.is_ascii_graphic() || ch == ' ' { ch } else { '.' });
    }
    out
}

fn status<T>(result: &Result<T, Status>) -> String {
    match result {
        Ok(_) => String::from("Ok"),
        Err(status) => format!("{status:?}"),
    }
}

/// Remove CSI/ESC sequences; everything else is kept byte-for-byte as text.
fn strip_ansi(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                    i += 1;
                }
            }
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn lines_of(text: &str) -> Vec<String> {
    text.split('\n').map(|l| String::from(l.trim_end_matches('\r'))).collect()
}

/// One frontend host (and the shell controlling its own session behind it), the
/// transcripts the driver collected from it and the grid model of its stdout.
struct Term {
    host: FrontendHost,
    out: Vec<u8>,
    err: Vec<u8>,
    screen: Screen,
}

impl Term {
    fn open(rows: u16, cols: u16) -> Result<Term, Status> {
        let host = FrontendHost::open(rows, cols)?;
        Ok(Term { host, out: Vec::new(), err: Vec::new(), screen: Screen::new(usize::from(cols), usize::from(rows)) })
    }

    fn launch_shell(&mut self) -> Result<(), Status> {
        // No directory grant: a granted cwd would make the shell path-sealed and the VFS
        // refuses every path-addressed request of a sealed cell. The shell then creates
        // its session for this frontend; input is accepted once it is attached.
        self.host.launch(SHELL, &["sh"], &[])?;
        self.host.wait_attached(60_000)
    }

    fn pump(&mut self) {
        // The model follows the session size the host last set (resize/re-attach).
        self.screen.fit(usize::from(self.host.cols()), usize::from(self.host.rows()));
        let mut buf = [0u8; 512];
        for stderr in [false, true] {
            for _ in 0..16 {
                let read = if stderr {
                    self.host.drain_stderr(&mut buf)
                } else {
                    self.host.drain_stdout(&mut buf)
                };
                match read {
                    Ok(n) if n > 0 => {
                        if !stderr {
                            self.screen.feed(&buf[..n]);
                        }
                        let sink = if stderr { &mut self.err } else { &mut self.out };
                        sink.extend_from_slice(&buf[..n]);
                        if sink.len() > CAP {
                            let cut = sink.len() - CAP / 2;
                            sink.drain(..cut);
                        }
                    }
                    _ => break,
                }
            }
        }
    }

    fn at_prompt(&self) -> bool {
        strip_ansi(&self.out).ends_with(PROMPT)
    }

    /// Wait until the transcript ends with the prompt. `Err` names why not.
    fn wait_prompt(&mut self, ms: u64) -> Result<(), String> {
        let end = now_ms() + ms;
        let mut spins = 0u32;
        loop {
            self.pump();
            if self.at_prompt() {
                return Ok(());
            }
            spins += 1;
            if spins % 32 == 0 {
                match self.host.poll_child() {
                    Ok(ChildPoll::Exited) => {
                        self.pump();
                        if self.at_prompt() {
                            return Ok(());
                        }
                        return Err(String::from("shell-exited"));
                    }
                    Ok(ChildPoll::Running) => {}
                    Err(status) => return Err(format!("poll {status:?}")),
                }
            }
            if now_ms() >= end {
                let tail = strip_ansi(&self.out);
                let tail: String = tail.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
                return Err(format!("prompt-timeout tail={tail:?}"));
            }
            let _ = sys_set_timer(1);
        }
    }

    /// Type `cmd` and press Enter without waiting (transcripts are reset first).
    fn send(&mut self, cmd: &str) -> Result<(), String> {
        self.pump();
        self.out.clear();
        self.err.clear();
        self.host.submit_text(cmd).map_err(|s| format!("submit_text {s:?}"))?;
        self.host.submit_key(ENTER, 0, 1).map_err(|s| format!("submit_key {s:?}"))
    }

    /// Output lines of the finished command: the echoed command line and the trailing
    /// prompt are removed, ANSI sequences stripped.
    fn result(&self, cmd: &str) -> Vec<String> {
        let mut lines = lines_of(&strip_ansi(&self.out));
        if lines.last().map(|l| l.starts_with(PROMPT.trim_end())).unwrap_or(false) {
            lines.pop();
        }
        if lines.first().map(String::as_str) == Some(cmd) {
            lines.remove(0);
        }
        lines
    }

    /// Remove only the exact editor echo and prompt, leaving arbitrary data intact.
    fn raw_result(&self, cmd: &str) -> Option<&[u8]> {
        self.out.strip_prefix(cmd.as_bytes())?.strip_prefix(b"\n")?.strip_suffix(PROMPT.as_bytes())
    }

    fn run(&mut self, cmd: &str, ms: u64) -> Result<Vec<String>, String> {
        self.send(cmd)?;
        self.wait_prompt(ms)?;
        // Diagnostics of the scripted command (never secrets: fixed scripts only).
        let diagnostic = self.stderr_text();
        if !diagnostic.trim().is_empty() {
            println(&format!("P04 INFO stderr cmd={} text={}", clip(cmd), clip(diagnostic.trim())));
        }
        Ok(self.result(cmd))
    }

    fn stderr_text(&self) -> String {
        strip_ansi(&self.err)
    }
}

fn rows_of(lines: &[String]) -> Vec<&String> {
    lines.iter().filter(|l| l.starts_with('{') && l.ends_with('}')).collect()
}

fn count_cellbox(term: &mut Term) -> Result<usize, String> {
    let lines = term.run("ps --records | where name '==' '\"cellbox\"' | select tid | to jsonl", 30_000)?;
    Ok(rows_of(&lines).len())
}

fn mem_used() -> Option<u64> {
    sys_mem_info().ok().map(|info| info.used_frames)
}

fn cell_main() {
    println("P04 START");
    let mut a = match Term::open(24, 80) {
        Ok(term) => term,
        Err(status) => {
            check("shell-prompt", false, &format!("open {status:?}"));
            finish();
        }
    };
    let launched = a.launch_shell();
    let prompt = match launched {
        Ok(()) => a.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    check("shell-prompt", prompt.is_ok(), prompt.as_ref().err().map(String::as_str).unwrap_or(""));
    // Phase 09 lane (`scripts/qemu-phase09-ssh-test.sh`): only the ssh checks run.
    #[cfg(feature = "phase09-ssh")]
    if prompt.is_ok() {
        ssh::ssh_checks(&mut a);
    }
    #[cfg(not(feature = "phase09-ssh"))]
    if prompt.is_ok() {
        builtins(&mut a);
        typed_pipeline(&mut a);
        file_create_ls(&mut a);
        externals(&mut a);
        soft_reset(&mut a);
        stock_backpressure(&mut a);
        stock_negative_grants(&mut a);
        exit_status(&mut a);
        rg::rg_checks(&mut a);
        rg::rg_fixture_checks(&mut a);
        edit::edit_checks(&mut a);
        ctrl_c(&mut a);
        endless_local_sink(&mut a);
        secret_namespace(&mut a);
        secret_read(&mut a);
        hostile_output(&mut a);
        paste_injection(&mut a);
        net_rights(&mut a);
        config_rights(&mut a);
        sandbox_rights(&mut a);
        // Phase 10 S10-0: `/bin/cellbox <command>` launched directly, and the in-shell
        // explicit form.
        oneshot::oneshot_checks(&mut a);
        resize(&mut a);
        frontend(&mut a);
        selective_tui(&mut a);
        top_tui(&mut a);
        top_load::top_load(&mut a);
        two_hosts(&mut a);
        child_exit_then_close();
        relaunch_after_exit();
        repeat_open_close(&mut a);
        cancel_child();
    }
    let closed = a.host.close();
    check("session-a-close", closed.is_ok(), &status(&closed));
    finish();
}

fn finish() -> ! {
    let failures = FAILURES.load(Ordering::Relaxed);
    let checks = CHECKS.load(Ordering::Relaxed);
    if failures == 0 {
        println(&format!("P04-TERMINAL: PASS checks={checks}"));
    } else {
        println(&format!("P04-TERMINAL: FAIL failures={failures} checks={checks}"));
    }
    sys_exit(if failures == 0 { 0 } else { 1 });
}

fn builtins(a: &mut Term) {
    let echo = a.run("echo p04-a'b'c", 30_000);
    check(
        "builtin-echo",
        matches!(&echo, Ok(l) if l.len() == 1 && l[0] == "p04-abc"),
        &format!("{echo:?}"),
    );

    let pwd = a.run("pwd", 30_000);
    check(
        "builtin-pwd",
        matches!(&pwd, Ok(l) if l.len() == 1 && l[0].starts_with('/')),
        &format!("{pwd:?}"),
    );

    let free = a.run("free", 30_000);
    let free_ok = free.as_ref().ok().is_some_and(|l| {
        l.iter().find(|x| x.starts_with("Mem (KiB):")).is_some_and(|row| {
            let n: Vec<u64> = row["Mem (KiB):".len()..].split_whitespace().filter_map(|t| t.parse().ok()).collect();
            n.len() == 3 && n[0] > 0 && n[0] == n[1] + n[2]
        })
    });
    check("builtin-free", free_ok, &format!("{free:?}"));

    // `date` is the wall clock (or uptime without an RTC): two runs seconds apart differ.
    let first = a.run("date", 30_000);
    sleep_ms(2500);
    let second = a.run("date", 30_000);
    let shaped = |r: &Result<Vec<String>, String>| {
        matches!(r, Ok(l) if l.len() == 1 && (l[0].contains(" UTC ") || l[0].contains("uptime")))
    };
    check(
        "builtin-date-changes",
        shaped(&first) && shaped(&second) && first != second,
        &format!("{first:?} then {second:?}"),
    );

    let ps = a.run("ps --records | select tid name | to jsonl", 30_000);
    let has = |l: &Vec<String>, name: &str| {
        rows_of(l).iter().any(|r| r.contains(&format!("\"name\":\"{name}\"")))
    };
    check(
        "builtin-ps-records",
        matches!(&ps, Ok(l) if has(l, "cellbox") && has(l, "phase04-terminal-driver") && has(l, "input")),
        &format!("{:?}", ps.as_ref().map(|l| l.len())),
    );
    // C5a: this image boots without `ui`; the data commands above ran with no UI task
    // anywhere in the system.
    let ui = ["compositor", "desktop", "fb-console"];
    check(
        "headless-no-ui-tasks",
        matches!(&ps, Ok(l) if !rows_of(l).is_empty() && !ui.iter().any(|name| has(l, name))),
        &format!("{:?}", ps.as_ref().map(|l| rows_of(l).len())),
    );
}

fn typed_pipeline(a: &mut Term) {
    let pipe = a.run("ps --records | where tid '>=' int:0 | sort-by tid | select tid name | take 2 | to jsonl", 30_000);
    let ok = matches!(&pipe, Ok(l) if {
        let rows = rows_of(l);
        rows.len() == 2 && rows.iter().all(|r| r.starts_with("{\"tid\":") && r.contains("\"name\":"))
    });
    check("typed-pipeline", ok, &format!("{pipe:?}"));

    // A typed stage's own refusal is a diagnostic, not silent output.
    let stage = a.run("ps --records | take 0 | to jsonl", 30_000);
    check("typed-pipeline-empty", matches!(&stage, Ok(l) if rows_of(l).is_empty()), &format!("{stage:?}"));
}

fn file_create_ls(a: &mut Term) {
    let stamp = now_ms() % 1_000_000;
    let file = format!("p04-{stamp}.txt");
    let body = format!("p04-body-{stamp}");
    let create = a.run(&format!("echo {body} > /tmp/{file}"), 30_000);
    let listed = a.run("ls --records /tmp | select name | to jsonl", 30_000);
    check(
        "file-create-ls",
        create.is_ok() && matches!(&listed, Ok(l) if rows_of(l).iter().any(|r| r.contains(&format!("\"{file}\"")))),
        &format!("create={create:?} ls={listed:?}"),
    );
    let cat = a.run(&format!("cat /tmp/{file}"), 30_000);
    check("file-cat-roundtrip", matches!(&cat, Ok(l) if l.len() == 1 && l[0] == body), &format!("{cat:?}"));
}

/// Contents of the private fixture `/srv/cellos/p04-secret` in the boot ramdisk (VIFS1).
/// The VFS reserves `/srv/cellos/**` to the live KMS service, so no shell read path,
/// including the kernel BootFS fallback, may ever deliver these bytes.
const SECRET_TOKEN: &str = "p04-kms-namespace-fixture-token";

/// Reads, copies, listings and writes of the KMS namespace from the shell are refused
/// (nonzero status) and never deliver the fixture bytes; a readable ramdisk file still
/// reads through the same commands.
fn secret_namespace(a: &mut Term) {
    const DENIED: [&str; 9] = [
        "cat /srv/cellos/p04-secret",
        "grep p04 /srv/cellos/p04-secret",
        "head -n 1 /srv/cellos/p04-secret",
        "wc /srv/cellos/p04-secret",
        "sort /srv/cellos/p04-secret",
        "from jsonl < /srv/cellos/p04-secret | to jsonl",
        "cp /srv/cellos/p04-secret /tmp/p04-leak",
        "ls /srv/cellos",
        "echo p04-w > /srv/cellos/p04-w",
    ];
    let mut leaked = false;
    for (index, cmd) in DENIED.iter().enumerate() {
        let run = a.run(cmd, 30_000);
        let shown = strip_ansi(&a.out).contains(SECRET_TOKEN) || a.stderr_text().contains(SECRET_TOKEN);
        leaked |= shown;
        let st = a.run("echo sk=$?", 30_000);
        let refused = matches!(&st, Ok(l) if l.len() == 1 && l[0].starts_with("sk=") && l[0] != "sk=0");
        check(
            &format!("secret-denied-{index}"),
            run.is_ok() && refused && !shown,
            &format!("cmd={cmd} run={:?} st={st:?} shown={shown}", run.map(|l| l.len())),
        );
    }
    // A copy that was refused left nothing behind to read.
    let leak = a.run("cat /tmp/p04-leak", 30_000);
    let copied = strip_ansi(&a.out).contains(SECRET_TOKEN);
    leaked |= copied;
    check("secret-no-copy", !copied, &format!("{leak:?}"));
    check("secret-never-shown", !leaked, "fixture bytes reached a shell stream");

    // Positive control: the ramdisk fallback still serves a policy-readable file.
    let wc = a.run("wc /p04-list/alpha-entry", 30_000);
    let st = a.run("echo sw=$?", 30_000);
    check(
        "ramdisk-read-still-works",
        matches!(&wc, Ok(l) if !l.is_empty()) && matches!(&st, Ok(l) if l == &[String::from("sw=0")]),
        &format!("{wc:?} {st:?}"),
    );
}

/// S5-4a item 1: a fixed fixture token (not a real secret) typed into `read -s` through
/// the frontend. The driver never prints it: FAIL lines of this scenario carry statuses and
/// counts only, never transcript text. The runner checks the UART for it independently.
const READ_SECRET_TOKEN: &str = "p04-read-s-fixture-token-Q7v2Lx";

/// Entries of `history` output (`{:5}  {entry}` per line), or `None` if a line is not one.
fn history_entries(lines: &[String]) -> Option<Vec<String>> {
    lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (number, entry) = line.trim_start().split_once("  ")?;
            number.parse::<usize>().ok()?;
            Some(String::from(entry))
        })
        .collect()
}

/// `read -s` on the shell's own GUI-kind session: the shell takes its echo-off lease, the
/// typed token is never echoed (the exact stdout is the command echo, the newline the reader
/// prints on Enter and the prompt), `${#SECRET}` shows only its length, `history` shows only
/// the typed command lines, and no drained byte of the scenario contains the token.
fn secret_read(a: &mut Term) {
    let token = READ_SECRET_TOKEN.as_bytes();
    let mut seen: Vec<u8> = Vec::new();
    let cleared = a.run("history -c", 30_000);
    seen.extend_from_slice(&a.out);
    seen.extend_from_slice(&a.err);
    let cmd = "read -s SECRET";
    let sent = a.send(cmd);
    // Typed text is read only after the command line was submitted; give the shell time to
    // take its echo-off lease first (keys queued before it would still reach the silent
    // reader: the line editor is not reading while a command runs).
    sleep_ms(1500);
    let typed = a.host.submit_text(READ_SECRET_TOKEN).and_then(|_| a.host.submit_key(ENTER, 0, 1));
    let back = a.wait_prompt(30_000).is_ok();
    seen.extend_from_slice(&a.out);
    seen.extend_from_slice(&a.err);
    let silent = a.out == format!("{cmd}\n\n{PROMPT}").as_bytes() && a.err.is_empty();
    check(
        "secret-read-silent",
        cleared.is_ok() && sent.is_ok() && typed.is_ok() && back && silent && occurrences(&seen, token) == 0,
        &format!(
            "cleared={} sent={} typed={} back={back} silent={silent} out_len={} err_len={}",
            cleared.is_ok(), sent.is_ok(), status(&typed), a.out.len(), a.err.len()
        ),
    );

    let length = a.run("echo ${#SECRET}", 30_000);
    seen.extend_from_slice(&a.out);
    seen.extend_from_slice(&a.err);
    let want = format!("{}", token.len());
    // Only digits are ever echoed into a FAIL line.
    let shown: Vec<String> = length.as_ref().map(|lines| {
        lines.iter().map(|l| if l.bytes().all(|b| b.is_ascii_digit()) { l.clone() } else { String::from("<non-numeric>") }).collect()
    }).unwrap_or_default();
    check(
        "secret-read-length",
        matches!(&length, Ok(lines) if lines.len() == 1 && lines[0] == want),
        &format!("ok={} want={want} lines={shown:?}", length.is_ok()),
    );

    let history = a.run("history", 30_000);
    seen.extend_from_slice(&a.out);
    seen.extend_from_slice(&a.err);
    let entries = history.as_ref().ok().and_then(|lines| history_entries(lines));
    let typed_only = ["read -s SECRET", "echo ${#SECRET}", "history"].map(String::from).to_vec();
    check(
        "secret-read-history",
        entries.as_ref() == Some(&typed_only),
        &format!("ok={} entries={:?}", history.is_ok(), entries.as_ref().map(Vec::len)),
    );
    let unset = a.run("unset SECRET", 30_000);
    seen.extend_from_slice(&a.out);
    seen.extend_from_slice(&a.err);
    check(
        "secret-read-never-drained",
        unset.is_ok() && occurrences(&seen, token) == 0,
        &format!("unset={} drained_bytes={}", unset.is_ok(), seen.len()),
    );
}

/// Host-built hostile output (scripts/phase05_vt_hostile.py) in the private image.
const HOSTILE_PATH: &str = "/p04-output/vt-hostile.bin";

/// S5-4a item 2: `cat` of OSC 52/8/0/2, >4 KiB DCS/APC/PM, DA/DSR/DECRQM queries, 8-bit
/// C1, a 17-parameter CSI and an embedded `ESC[201~`. The relayed bytes are evidence for
/// the runner's exact comparison; nothing answers the queries: the next typed command runs
/// exactly as typed (exact stdout) and `history` lists only the typed command lines.
fn hostile_output(a: &mut Term) {
    let cleared = a.run("history -c", 30_000);
    let cmd = format!("cat {HOSTILE_PATH}");
    let shown = a.run(&cmd, 60_000);
    stream_evidence("hostile-cat", a, &cmd);
    let relayed = a.raw_result(&cmd).map(<[u8]>::len).unwrap_or(0);
    let st = a.run("echo sthostile=$?", 30_000);
    check(
        "hostile-cat-relayed",
        cleared.is_ok() && shown.is_ok() && relayed > 3 * 4096 && a.err.is_empty()
            && matches!(&st, Ok(l) if l == &[String::from("sthostile=0")]),
        &format!("cleared={} shown={shown:?} relayed={relayed} st={st:?}", cleared.is_ok()),
    );

    let next = "echo p04-after-hostile";
    let ran = a.run(next, 30_000);
    let exact = a.out == format!("{next}\np04-after-hostile\n{PROMPT}").as_bytes() && a.err.is_empty();
    check(
        "hostile-next-command-exact",
        ran.is_ok() && exact,
        &format!("ran={ran:?} out={}", clip(&strip_ansi(&a.out))),
    );

    let history = a.run("history", 30_000);
    let entries = history.as_ref().ok().and_then(|lines| history_entries(lines));
    let typed_only = [cmd.as_str(), "echo sthostile=$?", next, "history"].map(String::from).to_vec();
    check(
        "hostile-history-typed-only",
        entries.as_ref() == Some(&typed_only) && a.at_prompt(),
        &format!("history={history:?}"),
    );
}

/// Paste batch with an embedded bracketed-paste end and further lines: a frontend that
/// forwarded it would end the paste early and run the rest as typed commands.
const PASTE_INJECTION: &str =
    "echo p04-paste-ran > /tmp/p04-paste-ran\x1b[201~\necho p04-paste-ran > /tmp/p04-paste-ran\n";

/// One input-owner request on `op` built here (bypassing the SDK's own validation).
fn raw_input(op: ObjectRefV1, selector: InputOp, body: &[u8], reply: &mut [u8]) -> Result<(Status, usize), Status> {
    let owner = ostd::session::service_binding(api::syscall::service::INPUT)?;
    let header = ostd::session::service_call(owner, api::syscall::service::INPUT, op, selector as u16, true, body, reply)?;
    Ok((header.status, header.total_len as usize))
}

/// PreparePaste / WritePaste / CommitPaste of `text` sent straight to the input owner, then
/// AbortPaste. Returns the (write, commit, abort) statuses.
fn raw_paste(op: ObjectRefV1, session: ObjectRefV1, attachment: ObjectRefV1, text: &[u8]) -> Result<(Status, Status, Status), Status> {
    let mut body = [0u8; 48];
    let mut w = Writer::new(&mut body);
    w.reference(session)?; w.reference(attachment)?; w.u64(0)?; w.u32(text.len() as u32)?; w.u32(0)?;
    let mut reply = [0u8; HEADER_LEN + 16];
    let (prepared, n) = raw_input(op, InputOp::PreparePaste, &body, &mut reply)?;
    if prepared != Status::Ok {
        return Err(prepared);
    }
    let paste = ObjectRefV1::from_bytes(&reply[HEADER_LEN..n])?;
    let mut body = [0u8; 600];
    let mut w = Writer::new(&mut body);
    w.reference(paste)?; w.u32(0)?; w.u32(text.len() as u32)?; w.bytes(text)?;
    let len = w.position();
    let (written, _) = raw_input(op, InputOp::WritePaste, &body[..len], &mut [0u8; HEADER_LEN + 8])?;
    let (committed, _) = raw_input(op, InputOp::CommitPaste, &paste.to_bytes(), &mut [0u8; HEADER_LEN + 8])?;
    let (aborted, _) = raw_input(op, InputOp::AbortPaste, &paste.to_bytes(), &mut [0u8; HEADER_LEN])?;
    Ok((written, committed, aborted))
}

/// S5-4a item 4: a frontend paste containing ESC / `ESC[201~` is rejected as a whole, by
/// the SDK before any owner request and by the input owner when the SDK is bypassed, and
/// nothing of it executes. A clean paste through the same API is the positive control.
fn paste_injection(a: &mut Term) {
    a.pump();
    a.out.clear();
    a.err.clear();
    let clean = a.host.submit_paste("echo p04-clean-paste");
    let enter = a.host.submit_key(ENTER, 0, 1);
    let ran = wait_text(a, "\np04-clean-paste\n", 30_000).and_then(|_| a.wait_prompt(30_000));
    check(
        "paste-clean-control",
        clean.is_ok() && enter.is_ok() && ran.is_ok(),
        &format!("paste={} enter={} ran={ran:?}", status(&clean), status(&enter)),
    );

    let sdk = a.host.submit_paste(PASTE_INJECTION);
    check("paste-escape-rejected-sdk", sdk == Err(Status::UnsupportedInput), &status(&sdk));
    let owner = match (a.host.shell_operation(), a.host.session(), a.host.attachment()) {
        (Some(op), Some(session), Some(attachment)) => raw_paste(op.raw(), session, attachment, PASTE_INJECTION.as_bytes()),
        _ => Err(Status::NotFound),
    };
    check(
        "paste-escape-rejected-owner",
        owner == Ok((Status::Ok, Status::UnsupportedInput, Status::Ok)),
        &format!("{owner:?}"),
    );

    sleep_ms(1000);
    let next = "echo p04-after-paste";
    let alive = a.run(next, 30_000);
    let exact = a.out == format!("{next}\np04-after-paste\n{PROMPT}").as_bytes() && a.err.is_empty();
    let listed = a.run("ls --records /tmp | select name | to jsonl", 30_000);
    check(
        "paste-escape-nothing-executed",
        alive.is_ok() && exact
            && matches!(&listed, Ok(l) if !rows_of(l).is_empty() && !rows_of(l).iter().any(|r| r.contains("p04-paste-ran"))),
        &format!("alive={alive:?} exact={exact} listed={:?}", listed.map(|l| l.len())),
    );
}

fn externals(a: &mut Term) {
    let echo_cmd = "/bin/echo p04-e'x't two";
    let echo = a.run(echo_cmd, 40_000);
    let echo_ok = echo.is_ok() && a.raw_result(echo_cmd) == Some(b"p04-ext two\n".as_slice()) && a.err.is_empty();
    stream_evidence("echo", a, echo_cmd);
    let st = a.run("echo stecho=$?", 30_000);
    check("external-echo", echo_ok && matches!(&st, Ok(l) if l == &[String::from("stecho=0")]), "exact stdout/empty stderr/status");

    let ls_cmd = "/bin/ls /p04-list";
    let ls = a.run(ls_cmd, 40_000);
    let listed = a.raw_result(ls_cmd);
    let ls_ok = ls.is_ok() && a.err.is_empty()
        && matches!(listed, Some(b".\n..\nalpha-entry\nbeta-entry\n" | b".\n..\nbeta-entry\nalpha-entry\n"));
    stream_evidence("ls", a, ls_cmd);
    let st = a.run("echo stls=$?", 30_000);
    check("external-ls", ls_ok && matches!(&st, Ok(l) if l == &[String::from("stls=0")]), "known directory exact entries/stdout/status");

    let bad_cmd = "/bin/ls /p04-output-missing";
    let bad = a.run(bad_cmd, 40_000);
    let bad_ok = bad.is_ok() && a.raw_result(bad_cmd) == Some(b"".as_slice())
        && a.err == b"ls: /p04-output-missing: no such directory\n";
    stream_evidence("ls-error", a, bad_cmd);
    let st = a.run("echo stbadls=$?", 30_000);
    check("external-ls-stderr", bad_ok && matches!(&st, Ok(l) if l == &[String::from("stbadls=1")]), "stderr only/nonzero");

    let cat_cmd = format!("/bin/cat {BINARY_PATH} /p04-ignored-second-path");
    let cat = a.run(&cat_cmd, 40_000);
    let expected = fixture_bytes(1600);
    let cat_ok = cat.is_ok() && a.raw_result(&cat_cmd) == Some(expected.as_slice()) && a.err.is_empty();
    stream_evidence("cat", a, &cat_cmd);
    let st = a.run("echo stcat=$?", 30_000);
    check("external-cat", cat_ok && matches!(&st, Ok(l) if l == &[String::from("stcat=0")]), "raw bytes/NUL/invalid UTF8/512 boundary/first path");
    check("external-stdout-not-stderr", echo_ok && ls_ok && cat_ok, "successful stock command leaked to stderr");

    let free = a.run("/bin/free", 40_000);
    let free_ok = free.is_ok() && a.err.is_empty()
        && a.raw_result("/bin/free").is_some_and(|bytes| bytes.starts_with(b"              total        used        free\nMem (KiB):"));
    stream_evidence("free", a, "/bin/free");
    let st = a.run("echo stfree=$?", 30_000);
    check("external-free", free_ok && matches!(&st, Ok(l) if l == &[String::from("stfree=0")]), "free finite deadline syscall/stream/status");

    // A single external is the controlling shell's foreground child: its stdin is the
    // session, so the empty input is the user's end of input (EndInput through the
    // frontend) once the child reads. /bin/p03-filter is the Phase03 private fixture that
    // reads/writes its Stdin/Stdout grants; exact bytes reach the host.
    let single_cmd = "/bin/p03-filter hex";
    let single = a.send(single_cmd).and_then(|_| {
        sleep_ms(2000);
        a.host.end_input().map_err(|s| format!("end_input {s:?}"))?;
        a.wait_prompt(40_000)
    }).map(|_| a.result(single_cmd));
    check(
        "external-filter-single",
        matches!(&single, Ok(l) if l == &[String::from("p03-hex len=0 hex=")]),
        &format!("{single:?}"),
    );
    let upper = a.run("echo abcXYZ-09 | /bin/p03-filter upper", 40_000);
    check(
        "external-filter-pipeline",
        matches!(&upper, Ok(l) if l == &[String::from("ABCXYZ-09")]),
        &format!("{upper:?}"),
    );
    let bytes = a.run("/bin/p03-filter gen | /bin/p03-filter hex", 40_000);
    check(
        "external-filter-exact-bytes",
        matches!(&bytes, Ok(l) if l == &[String::from("p03-hex len=6 hex=00ff41fe807a")]),
        &format!("{bytes:?}"),
    );
}

/// `/bin/p03-filter tui-abandon` output: the `vitui` enter bytes and one line, never
/// restored (a killed TUI).
const TUI_ABANDON: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?2004hp03-tui-abandon\n";
/// The `cellos-vt1` soft reset cellbox owes after such a child.
const SOFT_RESET: &[u8] = b"\x1b[?1049l\x1b[?25h\x1b[?2004l\x1b[0m";
/// `/bin/p03-filter rows` output.
const ROWS_JSONL: &[u8] = b"{\"a\":1}\n{\"a\":2}\n";

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|window| *window == needle).count()
}

/// No soft reset on either stream of the last command.
fn no_reset(a: &Term) -> bool {
    occurrences(&a.out, SOFT_RESET) == 0 && occurrences(&a.err, SOFT_RESET) == 0
}

/// S5-2 foreground-loss reset: the shell follows the screen modes of the child bytes it
/// relays. A child that exits with the alternate screen, a hidden cursor and bracketed
/// paste on is followed by exactly one soft reset on the shell's stdout, after the child's
/// bytes and before the next prompt, as a single command and as a pipeline stage; builtin
/// output, a stock external and a plain fixture mode are followed by none.
fn soft_reset(a: &mut Term) {
    let expected = [TUI_ABANDON, SOFT_RESET].concat();
    for (name, cmd, status_var) in [
        ("tui-abandon", "/bin/p03-filter tui-abandon", "sttui"),
        ("tui-pipeline", "/bin/p03-filter tui-abandon | /bin/p03-filter cat", "sttuipipe"),
    ] {
        let run = a.run(cmd, 40_000);
        let once = occurrences(&a.out, SOFT_RESET) == 1 && occurrences(&a.err, SOFT_RESET) == 0;
        let exact = run.is_ok() && a.raw_result(cmd) == Some(expected.as_slice()) && a.err.is_empty();
        stream_evidence(name, a, cmd);
        let st = a.run(&format!("echo {status_var}=$?"), 30_000);
        let after = no_reset(a);
        check(
            &format!("soft-reset-after-{name}"),
            exact && once && after && matches!(&st, Ok(l) if l == &[format!("{status_var}=0")]),
            &format!("run={run:?} exact={exact} once={once} next-clean={after} st={st:?}"),
        );
    }

    let plain: [(&str, &str, &[u8]); 3] = [
        ("builtin-echo", "echo p04-plain-reset", b"p04-plain-reset\n"),
        ("external-echo", "/bin/echo p04-plain-reset", b"p04-plain-reset\n"),
        ("filter", "/bin/p03-filter rows", ROWS_JSONL),
    ];
    for (name, cmd, output) in plain {
        let run = a.run(cmd, 40_000);
        let exact = run.is_ok() && a.raw_result(cmd) == Some(output) && a.err.is_empty();
        let clean = no_reset(a);
        if name == "filter" {
            stream_evidence("plain-filter", a, cmd);
        }
        check(
            &format!("soft-reset-none-{name}"),
            exact && clean,
            &format!("run={run:?} exact={exact} clean={clean}"),
        );
    }
}

/// Withhold host reads so the real stock producer fills the pipeline and host
/// rings; resume with an odd-sized read, then compare every raw byte.
fn stock_backpressure(a: &mut Term) {
    let cmd = format!("/bin/cat {LARGE_PATH}");
    let sent = a.send(&cmd);
    sleep_ms(1500);
    let mut first = [0u8; 137];
    let read = a.host.drain_stdout(&mut first);
    if let Ok(n) = read {
        a.out.extend_from_slice(&first[..n]);
    }
    let resumed = a.wait_prompt(90_000);
    let expected = fixture_bytes(LARGE_LEN);
    let exact = resumed.is_ok() && a.raw_result(&cmd) == Some(expected.as_slice()) && a.err.is_empty();
    stream_evidence("large-cat", a, &cmd);
    let st = a.run("echo stlarge=$?", 30_000);
    check(
        "stock-full-ring-drain-exact",
        sent.is_ok() && matches!(read, Ok(137)) && exact
            && matches!(&st, Ok(l) if l == &[String::from("stlarge=0")]),
        "withheld drain/resume: every binary byte once and successful stock exit",
    );

    // ls emits 60-byte names plus separate newline writes, deliberately not a
    // divisor of the 4096-byte ring. Withheld drains force its helper to retain
    // the remainder of a partially accepted name rather than replay that name.
    let names: Vec<String> = (0..256).map(|i| format!("p04-pressure-{i:03}-{}", "x".repeat(43))).collect();
    let mut listing = Vec::from(b".\n..\n".as_slice());
    listing.extend(names.iter().flat_map(|name| name.bytes().chain(core::iter::once(b'\n'))));
    let ls_cmd = "/bin/ls /p04-pressure";
    let sent = a.send(ls_cmd);
    sleep_ms(1500);
    let back = a.wait_prompt(60_000);
    let exact = a.raw_result(ls_cmd) == Some(listing.as_slice()) && a.err.is_empty();
    stream_evidence("pressure-ls", a, ls_cmd);
    let st = a.run("echo stpressure=$?", 30_000);
    check(
        "stock-fragmented-ring-drain-exact",
        sent.is_ok() && back.is_ok() && exact
            && matches!(&st, Ok(l) if l == &[String::from("stpressure=0")]),
        "stock helper non-aligned fragments across full ring without replay",
    );

    // A real external downstream closes without reading the stock cat's output.
    let close_start = now_ms();
    let broken = a.run(&format!("/bin/cat {LARGE_PATH} | /bin/p03-filter exit 0"), 15_000);
    let close_elapsed = now_ms() - close_start;
    evidence("stock-close-transcript.stdout", &a.out);
    evidence("stock-close-transcript.stderr", &a.err);
    let close_err_empty = a.err.is_empty();
    let after = a.run("/bin/echo p04-after-stock-close", 15_000);
    println(&format!("P04 INFO stock-close elapsed_ms={close_elapsed} result={broken:?} stderr_empty={close_err_empty} after={after:?}"));
    check(
        "stock-downstream-close-recovers",
        broken.is_ok() && close_elapsed < 15_000 && close_err_empty
            && matches!(&after, Ok(l) if l == &[String::from("p04-after-stock-close")])
            && a.err.is_empty(),
        &format!("elapsed_ms={close_elapsed} result={broken:?} stderr_empty={close_err_empty} after={after:?}"),
    );

    // Cancel a stock producer while full, separately from the existing filter test.
    let sent = a.send(&cmd);
    sleep_ms(1500);
    let cancel_start = now_ms();
    let key = a.host.submit_key(0x63, CTRL, 1);
    // Recovery must precede the stock helper's 30s deadline: expiration is not
    // evidence that Ctrl-C cancelled its blocked write.
    let recovered = a.wait_prompt(15_000);
    let cancel_elapsed = now_ms() - cancel_start;
    evidence("stock-cancel-transcript.stdout", &a.out);
    evidence("stock-cancel-transcript.stderr", &a.err);
    let st = if recovered.is_ok() {
        a.run("echo stockcc=$?", 15_000)
    } else {
        Err(String::from("recovery failed before status probe"))
    };
    println(&format!("P04 INFO stock-cancel elapsed_ms={cancel_elapsed} sent={sent:?} key={} recovered={recovered:?} probe={st:?}", status(&key)));
    check(
        "stock-full-ring-cancel",
        sent.is_ok() && key.is_ok() && recovered.is_ok() && cancel_elapsed < 15_000
            && matches!(&st, Ok(l) if l == &[String::from("stockcc=130")]),
        &format!("elapsed_ms={cancel_elapsed} sent={sent:?} key={} recovered={recovered:?} probe={st:?}", status(&key)),
    );

    let result = (|| -> Result<(), String> {
        let mut t = Term::open(24, 80).map_err(|s| format!("open {s:?}"))?;
        t.launch_shell().map_err(|s| format!("launch {s:?}"))?;
        t.wait_prompt(60_000)?;
        t.send(&cmd)?;
        sleep_ms(1500);
        t.host.close().map_err(|s| format!("close {s:?}"))?;
        if t.host.submit_text("after-close").is_ok() {
            return Err(String::from("closed host accepted input"));
        }
        Ok(())
    })();
    check("stock-full-ring-host-close", result.is_ok(), result.as_ref().err().map(String::as_str).unwrap_or(""));
}

/// Query the canonical terminal record of a directly prepared child; do not
/// confuse a publication refusal or owner cancellation with a helper exit.
fn direct_child_exit(op: Operation, child: ChildRefV1) -> Result<i32, Status> {
    let end = now_ms() + 60_000;
    loop {
        let mut reply = [0u8; HEADER_LEN + 24];
        let h = ostd::session::kernel_call(
            KernelOp::QueryChild, op.reference().raw(), false, &child.to_bytes(), &mut reply,
        )?;
        if h.status != Status::Ok { return Err(h.status); }
        let mut body = Reader::new(&reply[HEADER_LEN..h.total_len as usize]);
        let state = body.u32()?;
        let reason = body.u32()?;
        let code = body.i32()?;
        if body.u32()? != 0 { return Err(Status::Malformed); }
        let _sequence = body.u64()?;
        body.finish()?;
        if state == 3 {
            return if reason == 0 { Ok(code) } else { Err(Status::Cancelled) };
        }
        if now_ms() >= end { return Err(Status::TimedOut); }
        let _ = sys_set_timer(1);
    }
}

fn drain_endpoint(endpoint: Endpoint) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 512];
    for _ in 0..16 {
        match endpoint.read(&mut chunk, 0) {
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            _ => break,
        }
    }
    bytes
}

/// The amendment admits only this stock /bin/echo Path edge in private images.
/// Missing slots reach a real committed child; forged refs must be refused by
/// admission, a different observation which never claims helper execution.
fn stock_negative_grants(a: &mut Term) {
    a.pump();
    let baseline = (a.out.len(), a.err.len());
    for (name, stdout, stderr, invalid) in [
        ("negative-missing-stdout", false, true, false),
        ("negative-missing-stderr", true, false, false),
        ("negative-missing-both", false, false, false),
        ("negative-invalid-stdout", true, true, true),
    ] {
        let attempt = (|| -> Result<(), Status> {
            let op = Operation::create(None)?;
            let (out_read, out_write) = op.create_stream(64)?;
            let (err_read, err_write) = op.create_stream(64)?;
            let payload = format!("p04-forbidden-{name}");
            let grants_count = usize::from(stdout) + usize::from(stderr);
            let context_len = 48 + 7 + payload.len() + 1 + grants_count * 40;
            let outcome = (|| -> Result<(), Status> {
            let mut prepared = op.prepare_child("/bin/echo", context_len)?;
            let mut grants = Vec::new();
            for (slot, enabled, source) in [
                (GrantSlot::Stdout, stdout, out_write),
                (GrantSlot::Stderr, stderr, err_write),
            ] {
                if enabled {
                    let reference = if invalid && slot == GrantSlot::Stdout {
                        ObjectRefV1 { id: u64::MAX - 17, generation: 1 }
                    } else {
                        source.delegate(&prepared, EndpointRights::WRITE_ONLY)?.reference().raw()
                    };
                    grants.push(Grant::endpoint(slot, reference, EndpointRights::WRITE_ONLY.bits() as u16)?);
                }
            }
            let context = ChildContext::new(op.reference().raw(), &[&payload], b"", &[], &grants)?;
            let published = prepared.write_context(&context).and_then(|_| prepared.commit());
            match published {
                Ok(child) => {
                    let result = direct_child_exit(op, child);
                    println(&format!("P04 INFO negative-child name={name} exit={result:?} published=true"));
                    if invalid { Err(Status::Malformed) } else {
                        match result { Ok(code) if code != 0 => Ok(()), Ok(_) => Err(Status::Malformed), Err(s) => Err(s) }
                    }
                }
                Err(s) if invalid && matches!(s, Status::Denied | Status::Stale | Status::Revoked | Status::NotFound | Status::WrongOwner | Status::WrongSession | Status::Malformed) => {
                    println(&format!("P04 INFO negative-invalid-ref admission={s:?} helper-executed=false"));
                    Ok(())
                }
                Err(s) => Err(s),
            }
            })();
            let _ = (out_write.close(), err_write.close());
            let observed_out = drain_endpoint(out_read);
            let observed_err = drain_endpoint(err_read);
            evidence(&format!("{name}.stdout"), &observed_out);
            evidence(&format!("{name}.stderr"), &observed_err);
            let quiet = observed_out.is_empty() && observed_err.is_empty();
            let _ = op.cancel(2);
            let _ = (out_read.close(), err_read.close(), op.close());
            if !quiet { return Err(Status::Malformed); }
            outcome
        })();
        check(name, attempt.is_ok(), &status(&attempt));
    }
    a.pump();
    check("negative-grants-host-isolation", baseline == (a.out.len(), a.err.len()), "direct negative child contaminated unrelated host streams");
}

fn exit_status(a: &mut Term) {
    let ok = a.run("/bin/echo fine", 40_000);
    let st0 = a.run("echo st0=$?", 30_000);
    let bad = a.run("/bin/ls /p04-no-such-dir", 40_000);
    let st1 = a.run("echo st1=$?", 30_000);
    check(
        "exit-status",
        ok.is_ok()
            && bad.is_ok()
            && matches!(&st0, Ok(l) if l.len() == 1 && l[0] == "st0=0")
            && matches!(&st1, Ok(l) if l.len() == 1 && l[0] == "st1=1"),
        &format!("st0={st0:?} st1={st1:?}"),
    );
    let seven = a.run("/bin/p03-filter exit 7", 40_000);
    let st7 = a.run("echo st7=$?", 30_000);
    check(
        "exit-status-external-7",
        seven.is_ok() && matches!(&st7, Ok(l) if l == &[String::from("st7=7")]),
        &format!("{seven:?} {st7:?}"),
    );
    let unknown = a.run("/bin/p04-no-such-program", 40_000);
    let stu = a.run("echo stu=$?", 30_000);
    check(
        "exit-status-unknown-program",
        unknown.is_ok() && matches!(&stu, Ok(l) if l.len() == 1 && l[0].starts_with("stu=") && l[0] != "stu=0"),
        &format!("{stu:?}"),
    );
}

/// Every rights-gated `NetRequest`, in the order `/bin/p03-filter net-probe` sends them.
const NET_PROBE_OPS: [&str; 7] =
    ["resolve", "tcp-connect", "udp-create", "udp-bind", "tcp-listen", "multicast-join", "l2-send"];

/// S2-N: a Cell launched on a capability-free edge (`cellbox -> /bin/p03-filter`, the
/// shell's own rights) is refused every gated net operation with `Err(0xFC)`, while
/// `/bin/curl` on its reviewed edge is admitted and fails only on the network: the
/// runner's SLIRP is `restrict=on` with no forward, so nothing leaves the VM.
///
/// curl still reports through the legacy `Log` path (UART `USER:` lines), not through
/// the host streams, so the driver only frames the run between two INFO lines and checks
/// that nothing on the host streams shows a refusal or a response; the runner reads
/// curl's own text inside that frame as the independent admission verdict.
fn net_rights(a: &mut Term) {
    let probe = a.run("/bin/p03-filter net-probe", 60_000);
    let lines = probe.as_ref().map(Vec::as_slice).unwrap_or(&[]);
    for op in NET_PROBE_OPS {
        let prefix = format!("NETPROBE {op} ");
        let seen: Vec<&String> = lines.iter().filter(|l| l.starts_with(&prefix)).collect();
        check(
            &format!("net-denied-{op}"),
            seen.len() == 1 && &seen[0][prefix.len()..] == "denied",
            &format!("{seen:?} run={:?}", probe.as_ref().map(Vec::len)),
        );
    }
    let st = a.run("echo stnp=$?", 30_000);
    let probe_lines: Vec<&String> = lines.iter().filter(|l| l.starts_with("NETPROBE ")).collect();
    check(
        "net-probe-done",
        probe_lines.len() == NET_PROBE_OPS.len() + 1
            && probe_lines.last().is_some_and(|l| l.as_str() == "NETPROBE DONE")
            && matches!(&st, Ok(l) if l == &[String::from("stnp=0")]),
        &format!("lines={} st={st:?}", probe_lines.len()),
    );

    // Admitted Resolve (a literal is answered service-side) and TcpConnect lead curl to
    // its exchange, which the unreachable SLIRP host alias ends without a response
    // (`curl: empty response`). A denied Resolve or TcpConnect prints `cannot resolve
    // host` / `connect failed` instead.
    let curl_cmd = "/bin/curl http://10.0.2.2:9/";
    println("P04 INFO net-curl-begin");
    let started = now_ms();
    let curl = a.run(curl_cmd, 180_000);
    let elapsed = now_ms().saturating_sub(started);
    let streams = format!("{}\n{}", strip_ansi(a.raw_result(curl_cmd).unwrap_or(&a.out)), a.stderr_text());
    let st = a.run("echo stcurl=$?", 30_000);
    println(&format!("P04 INFO net-curl-end elapsed_ms={elapsed} status={st:?}"));
    let refused = ["cannot resolve host", "connect failed", "denied", "Denied", "permission", "Permission"]
        .iter()
        .any(|needle| streams.contains(needle));
    check(
        "net-curl-admitted",
        curl.is_ok()
            && matches!(&st, Ok(l) if l.len() == 1 && l[0].starts_with("stcurl="))
            && !streams.contains("HTTP/")
            && !refused,
        &format!("{curl:?} st={st:?} streams={}", streams.trim()),
    );
}

/// One `/bin/p03-filter config-probe ...` run: the probe's single `CFGPROBE` result
/// word for `op key`, or a diagnostic. The probe Cell sits on the capability-free
/// `phase03-filter-fixture-edge`, so it holds no Q right.
fn config_probe(a: &mut Term, args: &str) -> String {
    let cmd = format!("/bin/p03-filter config-probe {args}");
    let lines = match a.run(&cmd, 60_000) {
        Ok(lines) => lines,
        Err(error) => return format!("run-error:{error}"),
    };
    let mut words = args.split(' ');
    let prefix = format!("CFGPROBE {} {} ", words.next().unwrap_or(""), words.next().unwrap_or(""));
    let seen: Vec<&String> = lines.iter().filter(|l| l.starts_with(&prefix)).collect();
    match seen.as_slice() {
        [line] => String::from(&line[prefix.len()..]),
        _ => format!("lines:{lines:?}"),
    }
}

/// `export KEY=VALUE` from the shell, then `$?`: returns (status line, stdout lines,
/// stderr text). The shell intercepts `export` before parsing, so `$?` is read on
/// its own line.
fn export(a: &mut Term, assignment: &str) -> (Result<Vec<String>, String>, Vec<String>, String) {
    let cmd = format!("export {assignment}");
    let out = a.run(&cmd, 30_000).unwrap_or_default();
    // Diagnostics travel on the separate stderr stream; give a late chunk one pump.
    sleep_ms(50);
    a.pump();
    let err = a.stderr_text();
    let st = a.run("echo stq=$?", 30_000);
    (st, out, err)
}

/// Phase 01 CP1 (Q right): the shell's edge holds `config_write`, so `export` of a
/// shell-namespace key lands in Config with `$?` 0; a system key or a key outside the
/// namespace is refused by service-config (`$?` 1, message on stderr only, store
/// unchanged). A Cell without Q (the p03-filter probe) is refused every write, and
/// reads stay open to it.
fn config_rights(a: &mut Term) {
    let (st, out, err) = export(a, "P01Q_OK=1");
    let got = config_probe(a, "get P01Q_OK");
    check(
        "config-export-granted",
        matches!(&st, Ok(l) if l == &[String::from("stq=0")])
            && out.is_empty()
            && err.trim().is_empty()
            && got == "value:1",
        &format!("st={st:?} out={out:?} err={err:?} probe={got}"),
    );

    let (st, out, err) = export(a, "PATH=/tmp");
    let got = config_probe(a, "get PATH");
    check(
        "config-export-system-denied",
        matches!(&st, Ok(l) if l == &[String::from("stq=1")])
            && err.contains("export: PATH: permission denied")
            && got == "value:/bin",
        &format!("st={st:?} err={err:?} probe={got}"),
    );
    let system_out = out;

    let (st, out, err) = export(a, "a.b=1");
    let got = config_probe(a, "get a.b");
    check(
        "config-export-namespace-denied",
        matches!(&st, Ok(l) if l == &[String::from("stq=1")])
            && err.contains("export: a.b: permission denied")
            && got == "notfound",
        &format!("st={st:?} err={err:?} probe={got}"),
    );
    check(
        "config-denial-on-stderr-only",
        system_out.is_empty()
            && out.is_empty()
            && !system_out.iter().chain(out.iter()).any(|l| l.contains("permission denied")),
        &format!("system={system_out:?} namespace={out:?}"),
    );

    let set = config_probe(a, "set P01Q_PROBE x");
    let after = config_probe(a, "get P01Q_PROBE");
    check(
        "config-probe-set-denied",
        set == "denied" && after == "notfound",
        &format!("set={set} get={after}"),
    );
    let delete = config_probe(a, "delete OS");
    let after = config_probe(a, "get OS");
    check(
        "config-probe-delete-denied",
        delete == "denied" && after == "value:Cellos",
        &format!("delete={delete} get={after}"),
    );
}

/// Every right `s2tw-probe` exercises, in the order it reports them. `telemetry` is
/// the Phase 06 S6-1 `TelemetrySnapshot` provider (X right, shared bit 55).
const S2TW_RIGHTS: [&str; 5] = ["getprocs", "getprocs2", "telemetry", "meminfo", "vfs-write"];

/// One probe run: its `S2TW` lines and whether it ran to `S2TW DONE` with status 0.
struct ProbeRun {
    lines: Vec<String>,
    complete: bool,
    why: String,
}

fn run_s2tw_probe(a: &mut Term, cmd: &str, status_var: &str) -> ProbeRun {
    let run = a.run(cmd, 60_000);
    let st = a.run(&format!("echo {status_var}=$?"), 30_000);
    let lines: Vec<String> = run
        .as_ref()
        .map(|l| l.iter().filter(|l| l.starts_with("S2TW ")).cloned().collect())
        .unwrap_or_default();
    let complete = lines.len() == S2TW_RIGHTS.len() + 1
        && lines.last().is_some_and(|l| l.as_str() == "S2TW DONE")
        && matches!(&st, Ok(l) if l == &[format!("{status_var}=0")]);
    let why = format!("cmd={cmd} lines={lines:?} run={:?} st={st:?}", run.as_ref().map(Vec::len));
    ProbeRun { lines, complete, why }
}

/// The value the probe reported for `right`, when it reported it exactly once.
fn s2tw_result<'a>(probe: &'a ProbeRun, right: &str) -> Option<&'a str> {
    let prefix = format!("S2TW {right} ");
    let mut seen = probe.lines.iter().filter_map(|l| l.strip_prefix(prefix.as_str()));
    let first = seen.next()?;
    seen.next().is_none().then_some(first)
}

/// S2-T/S2-W (`phase-04-s2tw-annex.md` §6): the same signed probe binary declares
/// GetProcs, GetProcs2, TelemetrySnapshot, MemInfo and VfsMutate. Launched by the shell from the sandbox
/// prefix `/data/bin/` (EMPTY ceiling) every right is refused and nothing is written;
/// launched from its private `/bin` fixture row (`LEGACY_OBSERVE_MUTATE`) every right is
/// admitted and the file exists. The gate is the launch path, not the binary.
fn sandbox_rights(a: &mut Term) {
    let sandbox = run_s2tw_probe(a, "/data/bin/s2tw-probe sb", "sts2sb");
    for right in S2TW_RIGHTS {
        check(
            &format!("sandbox-denied-{right}"),
            sandbox.complete && s2tw_result(&sandbox, right) == Some("denied"),
            &sandbox.why,
        );
    }
    // The refused write left nothing behind: the shell cannot read or list it, and
    // `cat` of the missing file diagnoses it and exits nonzero.
    let cat = a.run("cat /tmp/s2tw-probe-sb", 30_000);
    let shown = strip_ansi(&a.out).contains("s2tw-sb");
    let missing = a.stderr_text().contains("No such file or directory");
    let cst = a.run("echo stsbcat=$?", 30_000);
    let listed = a.run("ls --records /tmp | select name | to jsonl", 30_000);
    let lst = a.run("echo stsbls=$?", 30_000);
    check(
        "sandbox-no-file",
        cat.is_ok()
            && !shown
            && missing
            && matches!(&cst, Ok(l) if l.len() == 1 && l[0].starts_with("stsbcat=") && l[0] != "stsbcat=0")
            && matches!(&listed, Ok(l) if !rows_of(l).iter().any(|r| r.contains("s2tw-probe-sb")))
            && matches!(&lst, Ok(l) if l == &[String::from("stsbls=0")]),
        &format!("cat={cat:?} shown={shown} missing={missing} cst={cst:?} ls={listed:?} lst={lst:?}"),
    );

    let control = run_s2tw_probe(a, "/bin/s2tw-probe ok", "sts2ok");
    for right in S2TW_RIGHTS {
        // An admitted snapshot must also decode as a V1 snapshot, not merely succeed.
        let admitted = if right == "telemetry" { "other:cells-" } else { "other:" };
        check(
            &format!("control-allowed-{right}"),
            control.complete && s2tw_result(&control, right).is_some_and(|r| r.starts_with(admitted)),
            &control.why,
        );
    }
    let cat = a.run("cat /tmp/s2tw-probe-ok", 30_000);
    let st = a.run("echo stokcat=$?", 30_000);
    check(
        "control-file-created",
        matches!(&cat, Ok(l) if l == &[String::from("s2tw-ok")])
            && matches!(&st, Ok(l) if l == &[String::from("stokcat=0")]),
        &format!("cat={cat:?} st={st:?}"),
    );
}

/// Type `cmd`, wait until `needle` shows (the producer is running), let it run `run_ms`
/// longer, press Ctrl-C (`submit_key`) and wait for the prompt. `Ok(true)` = needle seen.
fn interrupt_run(a: &mut Term, cmd: &str, needle: Option<&str>, run_ms: u64) -> Result<bool, String> {
    a.send(cmd)?;
    let mut seen = needle.is_none();
    if let Some(needle) = needle {
        let end = now_ms() + 90_000;
        while now_ms() < end {
            a.pump();
            if strip_ansi(&a.out).contains(needle) {
                seen = true;
                break;
            }
            let _ = sys_set_timer(1);
        }
    }
    sleep_ms(run_ms);
    a.pump();
    a.host.submit_key(0x63, CTRL, 1).map_err(|s| format!("submit_key {s:?}"))?;
    a.wait_prompt(60_000)?;
    Ok(seen)
}

/// Ctrl-C reaches the controlling shell as an Interrupt event; the shell cancels its own
/// pipeline and prompts again with status 130 (the shell itself is never cancelled).
fn ctrl_c(a: &mut Term) {
    // An endless typed producer feeding an external consumer.
    let top = interrupt_run(a, "top --records --frames 100000 | to jsonl | /bin/p03-filter cat", Some("{\""), 500);
    check("ctrl-c-recovers", matches!(top, Ok(true)), &format!("{top:?}"));
    // Nothing keeps flowing after the prompt is back.
    a.pump();
    let settled = a.out.len();
    sleep_ms(1500);
    a.pump();
    check("ctrl-c-output-stops", a.out.len() == settled && a.at_prompt(), "output continued after the prompt");
    let st = a.run("echo cc=$?", 30_000);
    check("ctrl-c-status-130", matches!(&st, Ok(l) if l == &[String::from("cc=130")]), &format!("{st:?}"));
    let alive = a.run("echo p04-after-ctrl-c", 30_000);
    check("ctrl-c-shell-alive", matches!(&alive, Ok(l) if l == &[String::from("p04-after-ctrl-c")]), &format!("{alive:?}"));

    // An endless external producer feeding an external consumer.
    let ext = interrupt_run(a, "/bin/p03-filter endless | /bin/p03-filter hex", None, 3000);
    let st = a.run("echo cc2=$?", 30_000);
    check(
        "ctrl-c-external-pipeline",
        ext.is_ok() && matches!(&st, Ok(l) if l == &[String::from("cc2=130")]),
        &format!("{ext:?} {st:?}"),
    );

    // Ctrl-C at an idle prompt: the shell prints ^C and prompts again.
    a.pump();
    a.out.clear();
    let key = a.host.submit_key(0x63, CTRL, 1);
    let back = a.wait_prompt(30_000);
    let idle = a.run("echo p04-idle-ok", 30_000);
    check(
        "ctrl-c-idle-prompt",
        key.is_ok() && back.is_ok() && matches!(&idle, Ok(l) if l == &[String::from("p04-idle-ok")]),
        &format!("key={} back={back:?} idle={idle:?}", status(&key)),
    );
}

/// Allocator-failure text of any stream: the cell heap handler logs `OOM: cell heap
/// exhausted`, which a streamed (never fully materialized) sink must never reach.
fn oom_text(a: &Term) -> bool {
    [strip_ansi(&a.out), strip_ansi(&a.err)]
        .iter()
        .any(|text| text.contains("OOM") || text.to_ascii_lowercase().contains("heap exhausted"))
}

/// An endless typed producer with a LOCAL terminal sink (no external tail): rows must
/// stream to the terminal while the pipeline runs, Ctrl-C must return the prompt promptly
/// with status 130, the shell must survive, and the cell heap must never be exhausted.
fn endless_local_sink(a: &mut Term) {
    const CMD: &str = "top --records --frames 100000 | to jsonl";
    let mut oom = false;
    let sent = a.send(CMD);
    let mut seen = false;
    let mut early_prompt = false;
    let end = now_ms() + 90_000;
    while sent.is_ok() && now_ms() < end {
        a.pump();
        oom |= oom_text(a);
        if strip_ansi(&a.out).contains("{\"") {
            seen = true;
            break;
        }
        if a.at_prompt() {
            early_prompt = true;
            break;
        }
        let _ = sys_set_timer(1);
    }
    // Rows keep arriving while the pipeline is still running: discard what was seen and
    // require fresh rows (and no prompt) after a pause.
    let mut more = false;
    if seen {
        a.pump();
        a.out.clear();
        let until = now_ms() + 1_500;
        while now_ms() < until {
            a.pump();
            oom |= oom_text(a);
            let _ = sys_set_timer(1);
        }
        a.pump();
        oom |= oom_text(a);
        more = strip_ansi(&a.out).contains("{\"") && !a.at_prompt();
    }
    check(
        "endless-local-sink-streams-live",
        sent.is_ok() && seen && more,
        &format!("sent={sent:?} seen={seen} more={more} early_prompt={early_prompt}"),
    );

    // Same key path as the other Ctrl-C scenarios.
    let started = now_ms();
    let key = a.host.submit_key(0x63, CTRL, 1);
    let back = a.wait_prompt(15_000);
    let elapsed = now_ms().saturating_sub(started);
    println(&format!("P04 INFO endless-local-sink elapsed_ms={elapsed}"));
    oom |= oom_text(a);
    let st = a.run("echo ccsink=$?", 30_000);
    oom |= oom_text(a);
    check(
        "endless-local-sink-ctrl-c-130",
        key.is_ok()
            && back.is_ok()
            && elapsed < 15_000
            && matches!(&st, Ok(l) if l == &[String::from("ccsink=130")]),
        &format!("key={} back={back:?} elapsed={elapsed} st={st:?}", status(&key)),
    );
    let alive = a.run("echo alive", 30_000);
    oom |= oom_text(a);
    check(
        "endless-local-sink-shell-alive",
        matches!(&alive, Ok(l) if l == &[String::from("alive")]),
        &format!("{alive:?}"),
    );
    check("endless-local-sink-no-oom", !oom, "OOM/heap exhausted text on the terminal streams");
}

fn resize(a: &mut Term) {
    let grow = a.host.resize(40, 120);
    let shrink = a.host.resize(24, 80);
    let alive = a.run("echo p04-after-resize", 30_000);
    check(
        "resize-accepted",
        grow.is_ok() && shrink.is_ok() && matches!(&alive, Ok(l) if l.len() == 1 && l[0] == "p04-after-resize"),
        &format!("grow={} shrink={} alive={alive:?}", status(&grow), status(&shrink)),
    );
}

/// Pump until the transcript contains `needle`.
fn wait_text(a: &mut Term, needle: &str, ms: u64) -> Result<(), String> {
    let end = now_ms() + ms;
    loop {
        a.pump();
        if strip_ansi(&a.out).contains(needle) {
            return Ok(());
        }
        if now_ms() >= end {
            return Err(format!("no {needle:?}"));
        }
        let _ = sys_set_timer(1);
    }
}

/// Keep pumping until `term`'s stdout has been quiet for a moment, so a frame drawn in
/// several ring chunks is complete in the grid model (at most 3 s).
fn settle(term: &mut Term) {
    const QUIET_MS: u64 = 250;
    let end = now_ms() + 3_000;
    let mut seen = term.screen.bytes;
    let mut quiet_since = now_ms();
    while now_ms() < end {
        term.pump();
        if term.screen.bytes != seen {
            seen = term.screen.bytes;
            quiet_since = now_ms();
        } else if now_ms().saturating_sub(quiet_since) >= QUIET_MS {
            return;
        }
        let _ = sys_set_timer(1);
    }
}

/// Pump `term` until `pred` yields a value on its grid model, re-checked on the settled
/// frame. `Err` carries a structure-only description of the screen.
fn wait_screen<T>(term: &mut Term, ms: u64, what: &str, mut pred: impl FnMut(&Screen) -> Option<T>) -> Result<T, String> {
    let end = now_ms() + ms;
    loop {
        term.pump();
        if pred(&term.screen).is_some() {
            settle(term);
            if let Some(value) = pred(&term.screen) {
                return Ok(value);
            }
        }
        if now_ms() >= end {
            return Err(format!("no {what}: {}", term.screen.summary()));
        }
        let _ = sys_set_timer(1);
    }
}

/// Pump until the grid model shows, inside the TUI's bordered block, a data row whose
/// `id:gen` token is followed by `name`; returns that identity.
fn wait_cell_row(a: &mut Term, name: &str, ms: u64) -> Result<String, String> {
    wait_screen(a, ms, "drawn Cell row", |s| s.frame().and_then(|f| f.cell_row(name)))
}

/// One `SubmitEvents` of text through `attachment`, built here (not through the host) so a
/// stale attachment can be presented to the input owner. Returns the owner's status.
fn raw_submit_text(op: ObjectRefV1, session: ObjectRefV1, attachment: ObjectRefV1, text: &[u8]) -> Result<Status, Status> {
    let owner = ostd::session::service_binding(api::syscall::service::INPUT)?;
    let mut body = [0u8; 128];
    let mut w = Writer::new(&mut body);
    w.reference(session)?; w.reference(attachment)?; w.u64(0)?; w.u16(1)?; w.u16(0)?; w.u32((24 + text.len()) as u32)?;
    w.u64(0)?; w.u64(0)?; w.u16(2)?; w.u16(text.len() as u16)?; w.u32(0)?; w.bytes(text)?;
    let len = w.position();
    let mut reply = [0u8; HEADER_LEN + 8];
    let header = ostd::session::service_call(owner, api::syscall::service::INPUT, op, InputOp::SubmitEvents as u16, true, &body[..len], &mut reply)?;
    Ok(header.status)
}

/// Phase 05 S5-3 annex option C: the shell controls its own GUI-kind session and this
/// driver is only its frontend. Interactive controller features (`rm -i`, `top` with `q`)
/// work through the frontend; releasing the frontend leaves the shell running Detached
/// (the shell logs `Lost(4)` on the UART); re-attaching a fresh frontend with the
/// holder-bound resume capability reaches the same shell (cwd and history preserved, the
/// prompt line redrawn) and the old attachment is stale. Holder death is not exercised
/// here: the holder is this driver.
fn frontend(a: &mut Term) {
    let attached = a.host.state() == FrontendState::Attached;
    check(
        "frontend-controller-session",
        attached && a.host.session().is_some() && a.host.attachment().is_some() && a.host.child().is_some(),
        &format!("state={:?}", a.host.state()),
    );

    // `rm -i` asks through the shell's own canonical session and reads the answer typed
    // on the frontend.
    let stamp = now_ms() % 1_000_000;
    let name = format!("p04-rmi-{stamp}");
    let made = a.run(&format!("echo rmi > /tmp/{name}"), 30_000);
    let cmd = format!("rm -i /tmp/{name}");
    let asked = a.send(&cmd).and_then(|_| wait_text(a, "'? (y/n) ", 30_000));
    let answered = a.host.submit_text("y").and_then(|_| a.host.submit_key(ENTER, 0, 1));
    let back = a.wait_prompt(30_000);
    let st = a.run("echo strmi=$?", 30_000);
    let listed = a.run("ls --records /tmp | select name | to jsonl", 30_000);
    check(
        "frontend-rm-interactive",
        made.is_ok() && asked.is_ok() && answered.is_ok() && back.is_ok()
            && matches!(&st, Ok(l) if l == &[String::from("strmi=0")])
            && matches!(&listed, Ok(l) if !rows_of(l).is_empty() && !rows_of(l).iter().any(|r| r.contains(&name))),
        &format!("asked={asked:?} answered={} back={back:?} st={st:?}", status(&answered)),
    );

    // Interactive `top` draws frames until `q` typed on the frontend quits it. A frame is
    // shown once the grid model has a data row, inside the bordered block, with an
    // `id:gen` token followed by `cellbox` (a shell's Cell).
    let shown = a.send("top").and_then(|_| wait_cell_row(a, "cellbox", 30_000));
    let quit = a.host.submit_text("q");
    let back = a.wait_prompt(30_000);
    let st = a.run("echo sttop=$?", 30_000);
    check(
        "frontend-top-quit",
        shown.is_ok() && quit.is_ok() && back.is_ok() && matches!(&st, Ok(l) if l == &[String::from("sttop=0")]),
        &format!("shown={shown:?} quit={} back={back:?} st={st:?}", status(&quit)),
    );

    // State the re-attached frontend must find again.
    let cd = a.run("cd /tmp", 30_000);
    let marked = a.run("echo p04-frontend-history", 30_000);
    let (session, old, op) = (a.host.session(), a.host.attachment(), a.host.shell_operation());

    a.pump();
    let released = a.host.release();
    sleep_ms(1500);
    a.pump();
    let running = a.host.poll_child();
    let local = a.host.submit_text("x");
    check(
        "frontend-release-detached",
        cd.is_ok() && marked.is_ok() && released.is_ok() && a.host.state() == FrontendState::Detached
            && running == Ok(ChildPoll::Running) && local == Err(Status::WouldBlock),
        &format!("release={} state={:?} child={running:?} local={}", status(&released), a.host.state(), status(&local)),
    );

    a.out.clear();
    a.err.clear();
    let reattached = a.host.reattach(30, 100);
    let redrawn = a.wait_prompt(30_000);
    check(
        "frontend-reattach-redraw",
        reattached.is_ok() && redrawn.is_ok() && a.host.state() == FrontendState::Attached
            && a.host.session() == session && a.host.attachment().is_some() && a.host.attachment() != old,
        &format!("reattach={} redraw={redrawn:?} state={:?}", status(&reattached), a.host.state()),
    );
    let pwd = a.run("pwd", 30_000);
    check("frontend-reattach-cwd-preserved", matches!(&pwd, Ok(l) if l == &[String::from("/tmp")]), &format!("{pwd:?}"));

    // History of the same shell: the line before the re-attach `pwd` is two Ups back;
    // Enter runs it again.
    a.pump();
    a.out.clear();
    a.err.clear();
    let keys = a.host.submit_key(0x110010, 0, 1)
        .and_then(|_| a.host.submit_key(0x110010, 0, 1))
        .and_then(|_| a.host.submit_key(ENTER, 0, 1));
    // The recall redraws the prompt before the line: wait for the command's own output.
    let rerun = wait_text(a, "\np04-frontend-history\n", 30_000).and_then(|_| a.wait_prompt(30_000));
    let lines = lines_of(&strip_ansi(&a.out));
    check(
        "frontend-reattach-history-preserved",
        keys.is_ok() && rerun.is_ok() && lines.iter().any(|l| l == "p04-frontend-history"),
        &format!("keys={} rerun={rerun:?} lines={}", status(&keys), lines.len()),
    );

    let stale = match (op, session, old) {
        (Some(op), Some(session), Some(old)) => raw_submit_text(op.raw(), session, old, b"x"),
        _ => Err(Status::NotFound),
    };
    let alive = a.run("echo p04-after-stale", 30_000);
    check(
        "frontend-old-attachment-stale",
        stale == Ok(Status::Stale) && matches!(&alive, Ok(l) if l == &[String::from("p04-after-stale")]),
        &format!("stale={stale:?} alive={alive:?}"),
    );
    let restored = a.host.resize(24, 80);
    check("frontend-resize-after-reattach", restored.is_ok(), &status(&restored));
}

/// Screen-mode bytes no one-shot or batch command may emit: alternate screen enter/leave,
/// hidden cursor, bracketed paste on, and the shell's soft reset.
const TUI_MODES: [&[u8]; 5] = [b"\x1b[?1049h", b"\x1b[?1049l", b"\x1b[?25l", b"\x1b[?2004h", SOFT_RESET];

fn tui_bytes(bytes: &[u8]) -> bool {
    TUI_MODES.iter().any(|seq| occurrences(bytes, seq) > 0)
}

/// Phase 05 S5-3 selective TUI: one-shot commands never enter a TUI screen, and `top`
/// stays batch when its output is machine data and is refused (status 2, a diagnostic)
/// in its interactive form inside a pipeline.
fn selective_tui(a: &mut Term) {
    if let Err(why) = a.run("echo sel > /tmp/sel-alpha", 30_000) {
        check("oneshot-no-tui-bytes", false, &format!("setup {why}"));
        return;
    }
    let mut seen = Vec::new();
    let mut ok = true;
    for (cmd, needle) in [
        ("ls /tmp", "sel-alpha"),
        ("ps", "cellbox"),
        ("cat /tmp/sel-alpha", "sel"),
        ("/bin/ls /p04-list", "alpha-entry"),
    ] {
        let run = a.run(cmd, 30_000);
        let shown = matches!(&run, Ok(l) if l.iter().any(|line| line.contains(needle)));
        let quiet = !tui_bytes(&a.out) && !tui_bytes(&a.err);
        ok &= shown && quiet;
        seen.push(format!("{cmd}: shown={shown} quiet={quiet}"));
    }
    check("oneshot-no-tui-bytes", ok, &seen.join("; "));

    let batch = a.run("top --records | take --frame 1 | to jsonl", 30_000);
    let batch_quiet = !tui_bytes(&a.out) && !tui_bytes(&a.err);
    let one_frame = matches!(&batch, Ok(l) if {
        let envelopes = rows_of(l);
        let kind = |r: &str, k: &str| r.starts_with(&format!("{{\"frame\":\"{k}\""));
        let count = |k: &str| envelopes.iter().filter(|r| kind(r.as_str(), k)).count();
        count("begin") == 1 && count("end") == 1 && count("row") >= 1
            && envelopes.first().is_some_and(|r| kind(r.as_str(), "begin"))
            && envelopes.last().is_some_and(|r| kind(r.as_str(), "end"))
    });
    let piped_top = "top | /bin/cat";
    let refused = a.run(piped_top, 30_000);
    let refused_clean = a.raw_result(piped_top) == Some(b"".as_slice()) && !tui_bytes(&a.err);
    let refused_err = a.stderr_text();
    let refused_st = a.run("echo stpt=$?", 30_000);
    check(
        "top-pipe-batch",
        one_frame && batch_quiet && refused.is_ok() && refused_clean
            && refused_err.contains("interactive session")
            && matches!(&refused_st, Ok(l) if l == &[String::from("stpt=2")]),
        &format!(
            "batch={batch:?} quiet={batch_quiet} refused={:?} clean={refused_clean} err={refused_err:?} st={refused_st:?}",
            refused.as_ref().err()
        ),
    );
}

/// Phase 06 S6-4 §7.2 markers of the interactive Ratatui `top` on the driver's hosts.
const TOP_TUI_MARKERS: [&str; 10] = [
    "top-tui-frame", "top-tui-refresh", "top-tui-drill", "top-tui-keys", "top-tui-no-lifecycle",
    "top-tui-resize", "top-tui-quit-restores", "top-tui-two-sessions", "top-tui-idle-cost",
    "top-tui-ctrl-c-130",
];
const ESCAPE: u32 = 0x110002;
const DELETE: u32 = 0x110005;
const UP: u32 = 0x110010;
const DOWN: u32 = 0x110011;
const PAGE_UP: u32 = 0x110016;
const PAGE_DOWN: u32 = 0x110017;
/// vitui theme colours as a GUI-kind (truecolor) session receives them: title-bar accent,
/// status/key-hint surface, border foreground.
const ACCENT: Color = Color::Rgb(0xcb, 0xa6, 0xf7);
const SURFACE: Color = Color::Rgb(0x31, 0x32, 0x44);
const BORDER: Color = Color::Rgb(0x89, 0xb4, 0xfa);
/// TUI entry (alternate screen, hidden cursor, bracketed paste) and its first clear. `top`
/// draws on a bounded (fixed) Ratatui viewport, whose clear erases row by row from column
/// 1 (`CUP y;1` + `ED 0`); its first step homes the cursor with the default pen and erases
/// to the end of the screen, i.e. clears the whole screen.
const TUI_ENTRY: [&[u8]; 4] = [b"\x1b[?1049h", b"\x1b[?25l", b"\x1b[?2004h", b"\x1b[1;1H\x1b[0m\x1b[J"];
/// The CANONICAL transition every `top` exit sends.
const TUI_RESTORE: [&[u8]; 3] = [b"\x1b[?1049l", b"\x1b[?25h", b"\x1b[?2004l"];
/// Ceiling for a Cell idling in interactive `top` (CPU% tenths of one core). An
/// implementation choice recorded with D64-6 W1 (tick-polled raw input), not a proposal
/// number.
const IDLE_COST_MAX_TENTHS: u64 = 500;

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

/// Every sequence of `seqs` occurs in `bytes`, first occurrences in this order.
fn in_order(bytes: &[u8], seqs: &[&[u8]]) -> (bool, Vec<Option<usize>>) {
    let at: Vec<Option<usize>> = seqs.iter().map(|seq| find(bytes, seq)).collect();
    (at.iter().all(Option::is_some) && at.windows(2).all(|pair| pair[0] < pair[1]), at)
}

fn skip(names: &[&str], why: &str) {
    for name in names {
        check(name, false, why);
    }
}

fn press(term: &mut Term, code: u32) -> Result<(), String> {
    term.host.submit_key(code, 0, 1).map_err(|s| format!("submit_key {s:?}"))
}

fn type_keys(term: &mut Term, text: &str) -> Result<(), String> {
    term.host.submit_text(text).map_err(|s| format!("submit_text {s:?}"))
}

/// The `id:gen` of the host's shell Cell: its committed child principal.
fn shell_identity(term: &Term) -> Option<String> {
    term.host.child().map(|child| format!("{}:{}", child.principal.cell_id, child.principal.generation))
}

/// A fresh host of `rows` x `cols` with its shell at the prompt.
fn open_shell(rows: u16, cols: u16) -> Result<Term, String> {
    let mut term = Term::open(rows, cols).map_err(|s| format!("open {s:?}"))?;
    let up = match term.launch_shell() {
        Ok(()) => term.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    match up {
        Ok(()) => Ok(term),
        Err(why) => {
            let _ = term.host.close();
            Err(why)
        }
    }
}

/// Type `top` and wait for a drawn Cells frame with a `cellbox` row.
fn start_top(term: &mut Term) -> Result<String, String> {
    term.send("top")?;
    wait_cell_row(term, "cellbox", 60_000)
}

/// Keep draining `term` (a `top` stalls once its 4 KiB ring is full).
fn drain_for(term: &mut Term, ms: u64) {
    let end = now_ms() + ms;
    while now_ms() < end {
        term.pump();
        let _ = sys_set_timer(1);
    }
}

/// `Term::run` on `term` while `other` (running `top`) keeps being drained.
fn run_beside(term: &mut Term, other: &mut Term, cmd: &str, ms: u64) -> Result<Vec<String>, String> {
    term.send(cmd)?;
    let end = now_ms() + ms;
    loop {
        other.pump();
        term.pump();
        if term.at_prompt() {
            return Ok(term.result(cmd));
        }
        if now_ms() >= end {
            return Err(format!("prompt-timeout running {}", clip(cmd)));
        }
        let _ = sys_set_timer(1);
    }
}

/// The system's `ps --records` TID set, read on `a` while `other` keeps drawing.
fn tid_set(a: &mut Term, other: &mut Term) -> Result<Vec<String>, String> {
    let lines = run_beside(a, other, "ps --records | select tid | to jsonl", 30_000)?;
    let mut tids: Vec<String> = rows_of(&lines).into_iter().cloned().collect();
    tids.sort();
    Ok(tids)
}

/// Integer first tokens of `lines` (the TID column of a task table), sorted.
fn first_ints(lines: &[String]) -> Vec<u64> {
    let mut ints: Vec<u64> = lines.iter().filter_map(|l| l.split_whitespace().next()?.parse().ok()).collect();
    ints.sort_unstable();
    ints
}

/// `format_bytes` text (`512B`, `1.5K`, `12.0M`) as comparable tenths of bytes.
fn bytes_value(text: &str) -> Option<u128> {
    let shift = match text.chars().last()? {
        'B' => 0,
        'K' => 10,
        'M' => 20,
        'G' => 30,
        'T' => 40,
        'P' => 50,
        'E' => 60,
        _ => return None,
    };
    let number = &text[..text.len() - 1];
    let tenths = match number.split_once('.') {
        Some((whole, tenth)) if tenth.len() == 1 => whole.parse::<u128>().ok()? * 10 + tenth.parse::<u128>().ok()?,
        None if shift == 0 => number.parse::<u128>().ok()? * 10,
        _ => return None,
    };
    Some(tenths << shift)
}

/// An Ok CPU% cell (`12.3`) as tenths; markers (`new`, `reset`, `n/a`) are `None`.
fn tenths_value(text: &str) -> Option<u64> {
    let (whole, tenth) = text.split_once('.')?;
    if whole.is_empty() || tenth.len() != 1 || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(whole.parse::<u64>().ok()? * 10 + tenth.parse::<u64>().ok()?)
}

/// The `cell_id` of every row's `id:gen` token, in row order.
fn cell_ids(rows: &[Vec<String>]) -> Option<Vec<u64>> {
    rows.iter()
        .map(|row| {
            let token = row.first().filter(|t| is_cell_identity(t))?;
            token.split_once(':')?.0.parse().ok()
        })
        .collect()
}

/// Phase 06 S6-4 §7.2: interactive `top` read through the grid model by structure only
/// (border glyphs, the highlight symbol, `id:gen` tokens, theme colours, sample ids,
/// statuses, mode bytes). Runs on two fresh 100x30 hosts beside `a`, which supplies the
/// system-wide `ps` view; both are closed again before the later host-count checks.
fn top_tui(a: &mut Term) {
    top_subst_refused(a);
    let baseline = count_cellbox(a);
    let mut t = match open_shell(30, 100) {
        Ok(term) => term,
        Err(why) => return skip(&TOP_TUI_MARKERS, &format!("host t {why}")),
    };
    if top_frame(&mut t) {
        top_refresh(&mut t);
        let drill = top_drill(&mut t);
        top_keys(&mut t);
        top_no_lifecycle(a, &mut t);
        top_resize(&mut t);
        top_sessions(&mut t, drill);
    } else {
        skip(&TOP_TUI_MARKERS[1..], "no first top frame on host t");
    }
    let _ = t.host.close();
    // The later host-count checks start from the shells that existed before.
    let end = now_ms() + 30_000;
    while now_ms() < end && count_cellbox(a).ok() != baseline.as_ref().ok().copied() {
        sleep_ms(1000);
    }
}

/// `x=$(top)`: a capture is no interactive terminal, so `top` refuses with one diagnostic
/// naming `-b` and status 2 before drawing anything (§2 runtime gate).
fn top_subst_refused(a: &mut Term) {
    let enters = a.screen.enters;
    let cmd = "x=$(top)";
    let run = a.run(cmd, 30_000);
    let clean = a.raw_result(cmd) == Some(b"".as_slice()) && !tui_bytes(&a.out) && !tui_bytes(&a.err);
    let named = a.stderr_text().contains("-b");
    let st = a.run("echo stsub=$?", 30_000);
    check(
        "top-subst-refused",
        run.is_ok() && clean && named && a.screen.enters == enters
            && matches!(&st, Ok(l) if l == &[String::from("stsub=2")]),
        &format!("run={:?} clean={clean} names-b={named} st={st:?}", run.as_ref().err()),
    );
}

/// `top` enters with 1049h/?25l/?2004h then a clear; the first frame's bordered block
/// spans exactly 100x30 with a `id:gen cellbox` row, the first data row highlighted, the
/// title bar on the accent and the status/key-hint rows on the surface colour across all
/// cells, the border in its colour, and every row's last column drawn.
fn top_frame(t: &mut Term) -> bool {
    let shown = start_top(t);
    let (ordered, at) = in_order(&t.out, &TUI_ENTRY);
    let s = &t.screen;
    let modes = s.alternate() && s.cursor_hidden() && s.bracketed_paste() && s.enters == 1;
    let layout = s.frame().is_some_and(|f| {
        let corners = [(0, 2), (f.w - 1, 2), (0, f.h - 2), (f.w - 1, f.h - 2)];
        (f.w, f.h) == (100, 30)
            && f.closed()
            && f.cell_row("cellbox").is_some()
            && f.selected() == Some(0)
            && f.last_column_inked()
            && f.row_background(0) == Some(ACCENT)
            && f.row_background(1) == Some(SURFACE)
            && f.row_background(f.h - 1) == Some(SURFACE)
            && corners.iter().all(|&(x, y)| f.cell(x, y).pen.fg == BORDER)
    });
    let bars = s.frame().map(|f| (f.row_background(0), f.row_background(1), f.row_background(f.h - 1), f.cell(0, 2).pen.fg));
    check(
        "top-tui-frame",
        shown.is_ok() && ordered && modes && layout,
        &format!(
            "shown={:?} entry={at:?} modes={modes} layout={layout} {} bars={bars:?}",
            shown.as_ref().err(),
            s.summary()
        ),
    );
    shown.is_ok()
}

/// Without input the view redraws: three successive status-row sample ids, strictly
/// increasing.
fn top_refresh(t: &mut Term) {
    let mut seen: Vec<u64> = Vec::new();
    let mut why = String::new();
    for _ in 0..3 {
        let last = seen.last().copied();
        let next = wait_screen(t, 15_000, "next sample", |s| {
            s.frame().and_then(|f| f.sample()).filter(|v| last != Some(*v))
        });
        match next {
            Ok(sample) => seen.push(sample),
            Err(e) => {
                why = e;
                break;
            }
        }
    }
    check(
        "top-tui-refresh",
        seen.len() == 3 && seen.windows(2).all(|pair| pair[0] < pair[1]),
        &format!("samples={seen:?} {why}"),
    );
}

/// Move the highlight with Up/Down until the selected row's identity is `id`.
fn select_row(t: &mut Term, id: &str) -> Result<(), String> {
    for _ in 0..64 {
        let (target, selected, current) = {
            let f = t.screen.frame().ok_or_else(|| String::from("no frame"))?;
            let rows = f.rows();
            let target = rows
                .iter()
                .position(|r| r.first().map(String::as_str) == Some(id))
                .ok_or_else(|| format!("{id} not among {} rows", rows.len()))?;
            (target, f.selected().ok_or_else(|| String::from("no highlighted row"))?, f.selected_id())
        };
        if target == selected {
            return Ok(());
        }
        press(t, if target > selected { DOWN } else { UP })?;
        wait_screen(t, 5_000, "highlight move", |s| {
            s.frame().and_then(|f| f.selected_id()).filter(|now| Some(now) != current.as_ref()).map(|_| ())
        })?;
    }
    Err(format!("{id} never highlighted"))
}

/// Navigate to this host's own shell Cell (a `cellbox` row), Enter: the title carries its
/// `id:gen` and every row is a task (integer TID). Esc returns to the Cells view with the
/// same Cell still highlighted. Returns the identity and the drilled TIDs; the batch
/// comparison runs after quitting ([`top_drill_compare`]).
fn top_drill(t: &mut Term) -> Result<(String, Vec<u64>), String> {
    let own = shell_identity(t).ok_or_else(|| String::from("no shell identity"))?;
    select_row(t, &own)?;
    press(t, ENTER)?;
    let tids = wait_screen(t, 30_000, "drilled task view", |s| {
        let f = s.frame()?;
        if !f.title().split_whitespace().any(|token| token == own) {
            return None;
        }
        let tids: Option<Vec<u64>> = f.rows().iter().map(|r| r.first()?.parse().ok()).collect();
        let mut tids = tids.filter(|v| !v.is_empty())?;
        tids.sort_unstable();
        Some(tids)
    })?;
    press(t, ESCAPE)?;
    wait_screen(t, 30_000, "Cells view after Esc", |s| {
        let f = s.frame()?;
        let listed = f.rows().iter().any(|r| r.len() >= 2 && r[0] == own && r[1] == "cellbox");
        (listed && f.selected_id().as_deref() == Some(own.as_str())).then_some(())
    })?;
    Ok((own, tids))
}

/// After quitting: the drilled TIDs equal `top -b -n 1 --cell ID:GEN` and include the
/// shell's root task.
fn top_drill_compare(t: &mut Term, drill: Result<(String, Vec<u64>), String>) {
    let root = t.host.child().map(|child| child.root_tid);
    let (own, shown) = match drill {
        Ok(found) => found,
        Err(why) => return check("top-tui-drill", false, &why),
    };
    let batch = t.run(&format!("top -b -n 1 --cell {own}"), 30_000);
    let listed = batch.as_ref().map(|lines| first_ints(lines));
    check(
        "top-tui-drill",
        matches!(&listed, Ok(tids) if *tids == shown) && root.is_some_and(|r| shown.contains(&r)),
        &format!("cell={own} tui={shown:?} batch={listed:?} root={root:?}"),
    );
}

/// Type a sort key and wait for the redraw it causes (the title names the sort); returns
/// the rows of the settled frame.
fn sort_key(t: &mut Term, key: &str) -> Result<Vec<Vec<String>>, String> {
    let title = t.screen.frame().map(|f| f.title()).ok_or_else(|| String::from("no frame"))?;
    type_keys(t, key)?;
    wait_screen(t, 10_000, &format!("redraw after {key}"), |s| s.frame().filter(|f| f.title() != title).map(|f| f.rows()))
}

/// Press `code` and wait until the highlight is on visible data row `want`.
fn nav(t: &mut Term, code: u32, want: usize) -> Result<(), String> {
    press(t, code)?;
    wait_screen(t, 10_000, &format!("highlight on row {want} after key {code:#x}"), |s| {
        s.frame().and_then(|f| f.selected()).filter(|&row| row == want).map(|_| ())
    })
}

/// `f` opens the field picker, Space toggles its first field (`cell_id`, which the CELL
/// column counts as), Enter applies: the shown column count afterwards.
fn toggle_first_field(t: &mut Term, before: usize) -> Result<usize, String> {
    type_keys(t, "f")?;
    wait_screen(t, 10_000, "field picker", |s| s.frame().filter(|f| f.popup()).map(|_| ()))?;
    type_keys(t, " ")?;
    press(t, ENTER)?;
    wait_screen(t, 10_000, "applied fields", |s| {
        s.frame().filter(|f| !f.popup() && f.header().len() != before).map(|f| f.header().len())
    })
}

/// §3.5 keys on the Cells view, judged on settled frames: `o` sorts by the next shown
/// column (QUOTA, descending); three more reach CELL (cell_id ascending); `r` reverses it
/// and `r` again restores it. On that stable order PageUp/Down/PageDown/Up put the
/// highlight on rows 0, 1, the last visible row and the one above it. `f`/Space/Enter on
/// the first field drops exactly one column, the same again restores it.
fn keys_walk(t: &mut Term) -> Result<String, String> {
    let rows = sort_key(t, "o")?;
    let quotas: Vec<u128> = rows.iter().filter_map(|r| r.get(4).and_then(|v| bytes_value(v))).collect();
    if quotas.len() < 2 || !quotas.windows(2).all(|pair| pair[0] >= pair[1]) {
        return Err(format!("o: QUOTA not descending {quotas:?}"));
    }
    sort_key(t, "o")?;
    sort_key(t, "o")?;
    let ascending = cell_ids(&sort_key(t, "o")?);
    let reversed = cell_ids(&sort_key(t, "r")?);
    let rows = sort_key(t, "r")?;
    let again = cell_ids(&rows);
    let strict = |ids: &Option<Vec<u64>>, up: bool| {
        ids.as_ref().is_some_and(|v| v.len() >= 2 && v.windows(2).all(|p| if up { p[0] < p[1] } else { p[0] > p[1] }))
    };
    if !strict(&ascending, true) || !strict(&reversed, false) || !strict(&again, true) {
        return Err(format!("CELL order o={ascending:?} r={reversed:?} r={again:?}"));
    }
    let n = rows.len();
    if n < 3 {
        return Err(format!("only {n} rows"));
    }
    nav(t, PAGE_UP, 0)?;
    nav(t, DOWN, 1)?;
    nav(t, PAGE_DOWN, n - 1)?;
    nav(t, UP, n - 2)?;
    let columns = t.screen.frame().map(|f| f.header().len()).ok_or_else(|| String::from("no frame"))?;
    let fewer = toggle_first_field(t, columns)?;
    let restored = toggle_first_field(t, fewer)?;
    if fewer + 1 != columns || restored != columns {
        return Err(format!("fields: {columns} -> {fewer} -> {restored} columns"));
    }
    Ok(format!("rows={n} columns={columns}->{fewer}->{restored}"))
}

fn top_keys(t: &mut Term) {
    let walk = keys_walk(t);
    check("top-tui-keys", walk.is_ok(), &format!("{walk:?}"));
}

/// `k`, `K` and Delete are inert: the system TID set read on `a` is the same before and
/// after, and `t` is still drawing its view.
fn top_no_lifecycle(a: &mut Term, t: &mut Term) {
    let before = tid_set(a, t);
    let keys = type_keys(t, "k").and_then(|_| type_keys(t, "K")).and_then(|_| press(t, DELETE));
    drain_for(t, 2_000);
    let after = tid_set(a, t);
    let still = t.screen.frame().is_some() && t.host.poll_child() == Ok(ChildPoll::Running);
    check(
        "top-tui-no-lifecycle",
        keys.is_ok() && still && matches!((&before, &after), (Ok(x), Ok(y)) if !x.is_empty() && x == y),
        &format!(
            "keys={keys:?} still={still} before={:?} after={:?}",
            before.as_ref().map(Vec::len),
            after.as_ref().map(Vec::len)
        ),
    );
}

/// Each resize repaints at once (a clear, then a closed border of the drawn size): 120x40,
/// 80x24, then 132x50 clamped to 120x40 with nothing drawn outside it, then back to 100x30.
/// The shell stays alive and no allocation-failure text appears.
fn top_resize(t: &mut Term) {
    let mut ok = true;
    let mut steps = Vec::new();
    for (rows, cols, want) in [(40u16, 120u16, (120, 40)), (24, 80, (80, 24)), (50, 132, (120, 40)), (30, 100, (100, 30))] {
        let clears = t.screen.clears;
        let resized = t.host.resize(rows, cols);
        let drawn = wait_screen(t, 30_000, "repaint", |s| {
            s.frame().filter(|f| s.clears > clears && (f.w, f.h) == want && f.closed() && f.outside_blank()).map(|_| ())
        });
        ok &= resized.is_ok() && drawn.is_ok();
        steps.push(format!("{cols}x{rows}:{}:{:?}", status(&resized), drawn.err()));
    }
    let alive = t.host.poll_child() == Ok(ChildPoll::Running) && t.screen.alternate();
    let clean = !oom_text(t);
    check("top-tui-resize", ok && alive && clean, &format!("{} alive={alive} clean={clean}", steps.join(" ")));
}

/// Host u starts `top`; t quits it (restore + prompt + status 0), compares its drill, then
/// u idles in `top` for the two-session and idle-cost checks before quitting; finally t
/// interrupts a fresh `top` with Ctrl-C.
fn top_sessions(t: &mut Term, drill: Result<(String, Vec<u64>), String>) {
    let mut u = open_shell(30, 100);
    let since = match u.as_mut() {
        Ok(u) => start_top(u).map(|_| now_ms()),
        Err(why) => Err(why.clone()),
    };
    top_quit_restores(t);
    top_drill_compare(t, drill);
    match (u.as_mut(), &since) {
        (Ok(u), Ok(since)) => {
            top_two_sessions(t, u);
            top_idle_cost(t, u, *since);
            let _ = type_keys(u, "q");
            let _ = u.wait_prompt(30_000);
        }
        _ => skip(&["top-tui-two-sessions", "top-tui-idle-cost"], &format!("host u {:?}", since.as_ref().err())),
    }
    top_ctrl_c(t);
    if let Ok(u) = u.as_mut() {
        let _ = u.host.close();
    }
}

/// `q`: the CANONICAL transition in order, the model back on the primary screen with the
/// cursor shown and bracketed paste off (no soft reset needed), the prompt, and the next
/// typed command runs and prints status 0 (lease released).
fn top_quit_restores(t: &mut Term) {
    t.pump();
    t.out.clear();
    t.err.clear();
    let quit = type_keys(t, "q");
    let back = t.wait_prompt(30_000);
    let (restored, at) = in_order(&t.out, &TUI_RESTORE);
    let s = &t.screen;
    let model = !s.alternate() && !s.cursor_hidden() && !s.bracketed_paste() && s.leaves == 1;
    let summary = s.summary();
    let st = t.run("echo stq=$?", 30_000);
    check(
        "top-tui-quit-restores",
        quit.is_ok() && back.is_ok() && restored && model && matches!(&st, Ok(l) if l == &[String::from("stq=0")]),
        &format!("quit={quit:?} back={back:?} restore={at:?} {summary} st={st:?}"),
    );
}

/// Quitting t's `top` leaves u's drawing (a newer sample after t is back at its prompt);
/// each host saw only its own mode bytes: one enter on u and none of t's leave, one
/// enter/leave on t, and nothing of u's frames on t.
fn top_two_sessions(t: &mut Term, u: &mut Term) {
    settle(u);
    let before = u.screen.frame().and_then(|f| f.sample());
    let drawing = wait_screen(u, 15_000, "u redraw after t quit", |s| {
        s.frame().and_then(|f| f.sample()).filter(|v| before.is_some_and(|b| *v > b))
    });
    t.pump();
    let t_quiet = !tui_bytes(&t.out) && !tui_bytes(&t.err) && t.at_prompt();
    let own = u.screen.alternate() && u.screen.enters == 1 && u.screen.leaves == 0
        && t.screen.enters == 1 && t.screen.leaves == 1;
    check(
        "top-tui-two-sessions",
        drawing.is_ok() && t_quiet && own,
        &format!(
            "u={before:?}->{drawing:?} t-quiet={t_quiet} u-modes={}/{} t-modes={}/{}",
            u.screen.enters, u.screen.leaves, t.screen.enters, t.screen.leaves
        ),
    );
}

/// While u idles in `top` for at least 5 s, t's `top -b -n 2 -d 2` shows u's Cell twice
/// and its second sample with an Ok CPU% below [`IDLE_COST_MAX_TENTHS`].
fn top_idle_cost(t: &mut Term, u: &mut Term, since: u64) {
    let id = shell_identity(u).unwrap_or_default();
    while now_ms() < since + 5_000 {
        u.pump();
        let _ = sys_set_timer(1);
    }
    let run = run_beside(t, u, "top -b -n 2 -d 2", 60_000);
    let mine: Vec<Vec<String>> = run
        .as_ref()
        .map(|lines| {
            lines
                .iter()
                .map(|l| l.split_whitespace().map(String::from).collect::<Vec<String>>())
                .filter(|r| r.first() == Some(&id))
                .collect()
        })
        .unwrap_or_default();
    let cpu = mine.last().and_then(|r| r.get(3)).and_then(|v| tenths_value(v));
    println(&format!(
        "P04 INFO top-tui-idle-cost cell={id} cpu_tenths={cpu:?} threshold_tenths={IDLE_COST_MAX_TENTHS} (implementation choice, D64-6 W1)"
    ));
    check(
        "top-tui-idle-cost",
        is_cell_identity(&id)
            && mine.len() == 2
            && mine.iter().all(|r| r.get(1).map(String::as_str) == Some("cellbox"))
            && u.screen.alternate()
            && cpu.is_some_and(|v| v < IDLE_COST_MAX_TENTHS),
        &format!(
            "cell={id} rows={} cpu_tenths={cpu:?} threshold_tenths={IDLE_COST_MAX_TENTHS} (threshold is an implementation choice) run={:?}",
            mine.len(),
            run.as_ref().err()
        ),
    );
}

/// Ctrl-C in `top`: the CANONICAL transition, then `^C`, the prompt and status 130.
fn top_ctrl_c(t: &mut Term) {
    let shown = start_top(t);
    t.pump();
    t.out.clear();
    t.err.clear();
    let key = t.host.submit_key(0x63, CTRL, 1);
    let back = t.wait_prompt(30_000);
    let (restored, at) = in_order(&t.out, &TUI_RESTORE);
    let caret = find(&t.out, b"^C\n");
    let after = matches!((at.last().copied().flatten(), caret), (Some(restore), Some(c)) if c > restore);
    let model = !t.screen.alternate() && !t.screen.cursor_hidden() && !t.screen.bracketed_paste();
    let st = t.run("echo stc=$?", 30_000);
    check(
        "top-tui-ctrl-c-130",
        shown.is_ok() && key.is_ok() && back.is_ok() && restored && after && model
            && matches!(&st, Ok(l) if l == &[String::from("stc=130")]),
        &format!(
            "shown={:?} key={} back={back:?} restore={at:?} caret={caret:?} model={model} st={st:?}",
            shown.as_ref().err(),
            status(&key)
        ),
    );
}

fn two_hosts(a: &mut Term) {
    let before = count_cellbox(a);
    let mut b = match Term::open(30, 100) {
        Ok(term) => term,
        Err(status) => {
            check("two-hosts-independent-output", false, &format!("open {status:?}"));
            return;
        }
    };
    let up = match b.launch_shell() {
        Ok(()) => b.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    if up.is_err() {
        check("two-hosts-independent-output", false, &format!("b {up:?}"));
        let _ = b.host.close();
        return;
    }
    let after = count_cellbox(a);
    check(
        "ps-reflects-new-shell",
        matches!((&before, &after), (Ok(x), Ok(y)) if *x >= 1 && *y > *x),
        &format!("before={before:?} after={after:?}"),
    );

    // Concurrent: both commands are typed before either result is read.
    let cmd_a = "/bin/echo p04-stock-from-a";
    let cmd_b = "/bin/echo p04-stock-from-b";
    let sent_a = a.send(cmd_a);
    let sent_b = b.send(cmd_b);
    let wa = a.wait_prompt(40_000);
    let wb = b.wait_prompt(40_000);
    stream_evidence("host-a", a, cmd_a);
    stream_evidence("host-b", &b, cmd_b);
    check(
        "two-hosts-independent-output",
        sent_a.is_ok() && sent_b.is_ok() && wa.is_ok() && wb.is_ok()
            && a.raw_result(cmd_a) == Some(b"p04-stock-from-a\n".as_slice())
            && b.raw_result(cmd_b) == Some(b"p04-stock-from-b\n".as_slice())
            && a.err.is_empty() && b.err.is_empty(),
        "concurrent stock echo exact stdout and empty stderr",
    );
    let err_a = "/bin/ls /p04-missing-host-a";
    let err_b = "/bin/ls /p04-missing-host-b";
    let sa = a.send(err_a);
    let sb = b.send(err_b);
    let wa = a.wait_prompt(40_000);
    let wb = b.wait_prompt(40_000);
    stream_evidence("host-a-error", a, err_a);
    stream_evidence("host-b-error", &b, err_b);
    check(
        "two-hosts-independent-stderr",
        sa.is_ok() && sb.is_ok() && wa.is_ok() && wb.is_ok()
            && a.raw_result(err_a) == Some(b"".as_slice())
            && b.raw_result(err_b) == Some(b"".as_slice())
            && a.err == b"ls: /p04-missing-host-a: no such directory\n"
            && b.err == b"ls: /p04-missing-host-b: no such directory\n",
        "concurrent stock diagnostics exact stderr, empty stdout, no cross-host bytes",
    );

    // Per-session state: a cwd change in one shell never reaches the other.
    let cd = a.run("cd /bin", 30_000);
    let pa = a.run("pwd", 30_000);
    let pb = b.run("pwd", 30_000);
    check(
        "two-hosts-independent-state",
        cd.is_ok() && matches!(&pa, Ok(l) if l == &[String::from("/bin")]) && matches!(&pb, Ok(l) if l.len() == 1 && l[0] != "/bin"),
        &format!("a={pa:?} b={pb:?}"),
    );

    // Independent resize and independent exit.
    let resized = b.host.resize(50, 132);
    let a_alive = a.run("echo p04-a-alive", 30_000);
    check(
        "two-hosts-independent-resize",
        resized.is_ok() && matches!(&a_alive, Ok(l) if l == &[String::from("p04-a-alive")]),
        &format!("{} {a_alive:?}", status(&resized)),
    );
    let _ = b.send("exit");
    let exited = wait_exit(&mut b, 30_000);
    let a_after = a.run("echo p04-a-after-b-exit", 30_000);
    check(
        "two-hosts-independent-exit",
        exited.is_ok() && matches!(&a_after, Ok(l) if l == &[String::from("p04-a-after-b-exit")]),
        &format!("exit={exited:?} a={a_after:?}"),
    );
    let closed = b.host.close();
    check("two-hosts-close", closed.is_ok(), &status(&closed));
}

/// Poll until the shell child has exited (output stays readable through the pump).
fn wait_exit(term: &mut Term, ms: u64) -> Result<(), String> {
    let end = now_ms() + ms;
    loop {
        term.pump();
        match term.host.poll_child() {
            Ok(ChildPoll::Exited) => {
                term.pump();
                return Ok(());
            }
            Ok(ChildPoll::Running) => {}
            Err(status) => return Err(format!("poll {status:?}")),
        }
        if now_ms() >= end {
            return Err(String::from("exit-timeout"));
        }
        let _ = sys_set_timer(1);
    }
}

fn child_exit_then_close() {
    let mut t = match Term::open(24, 80) {
        Ok(term) => term,
        Err(status) => {
            check("child-exit-then-close", false, &format!("open {status:?}"));
            return;
        }
    };
    let up = match t.launch_shell() {
        Ok(()) => t.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    if up.is_err() {
        check("child-exit-then-close", false, &format!("{up:?}"));
        let _ = t.host.close();
        return;
    }
    let sent = t.send("exit");
    let exited = wait_exit(&mut t, 30_000);
    let closed = t.host.close();
    let again = t.host.close();
    let after = t.host.submit_text("x");
    check(
        "child-exit-then-close",
        sent.is_ok() && exited.is_ok() && closed.is_ok() && again.is_ok() && after.is_err(),
        &format!("exit={exited:?} close={} again={} after={}", status(&closed), status(&again), status(&after)),
    );
}

/// A host that saw its shell exit can launch a new shell in the same session.
fn relaunch_after_exit() {
    let mut t = match Term::open(24, 80) {
        Ok(term) => term,
        Err(status) => {
            check("relaunch-after-exit", false, &format!("open {status:?}"));
            return;
        }
    };
    let up = match t.launch_shell() {
        Ok(()) => t.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    let result = if up.is_err() {
        Err(format!("first {up:?}"))
    } else if let Err(why) = t.send("exit") {
        Err(why)
    } else if let Err(why) = wait_exit(&mut t, 30_000) {
        Err(why)
    } else {
        t.out.clear();
        t.err.clear();
        match t.launch_shell() {
            Err(status) => Err(format!("relaunch {status:?}")),
            Ok(()) => t.wait_prompt(60_000).and_then(|_| t.run("echo p04-relaunched", 30_000)),
        }
    };
    let closed = t.host.close();
    check(
        "relaunch-after-exit",
        matches!(&result, Ok(l) if l == &[String::from("p04-relaunched")]) && closed.is_ok(),
        &format!("{result:?} close={}", status(&closed)),
    );
}

fn repeat_open_close(a: &mut Term) {
    const ROUNDS: usize = 6;
    let baseline = count_cellbox(a);
    let mut ok = true;
    let mut why = String::new();
    let mut mem_after_first = None;
    for round in 0..ROUNDS {
        let mut t = match Term::open(24, 80) {
            Ok(term) => term,
            Err(status) => {
                ok = false;
                why = format!("round {round} open {status:?}");
                break;
            }
        };
        let up = match t.launch_shell() {
            Ok(()) => t.wait_prompt(60_000),
            Err(status) => Err(format!("launch {status:?}")),
        };
        let cmd = format!("echo p04-round-{round}");
        let echoed = if up.is_ok() { t.run(&cmd, 30_000) } else { Err(String::from("no prompt")) };
        // Closing with the shell still running cancels and reaps it.
        let closed = t.host.close();
        let good = up.is_ok() && matches!(&echoed, Ok(l) if l.len() == 1 && l[0] == format!("p04-round-{round}")) && closed.is_ok();
        if !good {
            ok = false;
            why = format!("round {round} up={up:?} echo={echoed:?} close={}", status(&closed));
            break;
        }
        drop(t);
        if round == 0 {
            sleep_ms(2000);
            mem_after_first = mem_used();
        }
    }
    check("repeat-open-close", ok, &why);

    sleep_ms(3000);
    let finish = count_cellbox(a);
    let mem_last = mem_used();
    println(&format!("P04 INFO repeat cellbox={baseline:?}->{finish:?} used_frames={mem_after_first:?}->{mem_last:?}"));
    // Every closed shell is gone, and kernel frame use has not grown beyond a small slack.
    let procs_ok = matches!((&baseline, &finish), (Ok(x), Ok(y)) if x == y);
    let mem_ok = match (mem_after_first, mem_last) {
        (Some(x), Some(y)) => y <= x + 256,
        _ => false,
    };
    check(
        "repeat-no-leak",
        ok && procs_ok && mem_ok,
        &format!("procs {baseline:?}->{finish:?} frames {mem_after_first:?}->{mem_last:?}"),
    );
}

fn cancel_child() {
    let mut t = match Term::open(24, 80) {
        Ok(term) => term,
        Err(status) => {
            check("cancel-child", false, &format!("open {status:?}"));
            return;
        }
    };
    let up = match t.launch_shell() {
        Ok(()) => t.wait_prompt(60_000),
        Err(status) => Err(format!("launch {status:?}")),
    };
    if up.is_err() {
        check("cancel-child", false, &format!("{up:?}"));
        let _ = t.host.close();
        return;
    }
    let cancelled = t.host.cancel_child();
    let exited = wait_exit(&mut t, 30_000);
    let closed = t.host.close();
    check(
        "cancel-child",
        cancelled.is_ok() && exited.is_ok() && closed.is_ok(),
        &format!("cancel={} exit={exited:?} close={}", status(&cancelled), status(&closed)),
    );
}
