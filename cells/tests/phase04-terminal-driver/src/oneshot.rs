//! Phase 10 S10-0 (`phase-10-s0-proposal.md` §2 S10-0.8): `/bin/cellbox <command>` launched
//! directly with a prepared context, as `stock_negative_grants` launches `/bin/echo`, on
//! this fixture's reviewed `-> /bin/cellbox` edge. Every one-shot payload carries the
//! `p04os-` token, which the runner requires absent from the UART; FAIL lines print
//! observed bytes as hex only.

use super::*;

/// Ring of each stream this driver creates for a one-shot (bytes).
const RING: u32 = 4096;
/// Bound for one ordinary one-shot, launch to exit record (TCG).
const RUN_MS: u64 = 120_000;
/// A refused one-shot of [`ONE_SHOT_REFUSED`] or a consumer close must end well before the
/// 30 s write deadline of the child.
const PROMPT_EXIT_MS: u64 = 20_000;
const FRONTEND_ONLY_SH: &[u8] = b"cellbox: a terminal frontend runs only 'sh'\n";
const RG_FX_HD: &str = "/mnt/sd/rg/hd";

/// How one direct one-shot launch is wired and driven.
struct Spec<'a> {
    argv: &'a [&'a str],
    stdout: bool,
    stderr: bool,
    /// `Some`: a Stdin grant fed with these bytes, then closed (end of stream).
    stdin: Option<&'a [u8]>,
    /// Close the stdout reader once this many bytes arrived (a consumer that stops).
    close_stdout_after: Option<usize>,
    /// Cancel the one-shot's Operation this long after the commit.
    cancel_after_ms: Option<u64>,
    timeout_ms: u64,
}

impl<'a> Spec<'a> {
    fn new(argv: &'a [&'a str]) -> Self {
        Spec { argv, stdout: true, stderr: true, stdin: None, close_stdout_after: None, cancel_after_ms: None, timeout_ms: RUN_MS }
    }
}

/// What a one-shot did: its terminal record, both streams and the elapsed time.
struct Ran {
    /// `Ok(code)` for a normal exit; `Err(reason)` for a cancelled/killed child.
    exit: Result<i32, u32>,
    out: Vec<u8>,
    err: Vec<u8>,
    elapsed_ms: u64,
}

impl Ran {
    fn code(&self) -> Option<i32> {
        self.exit.ok()
    }

    fn quiet_err(&self) -> bool {
        self.err.is_empty()
    }

    fn summary(&self) -> String {
        format!("exit={:?} out={} err={} ms={}", self.exit, hexs(&self.out), hexs(&self.err), self.elapsed_ms)
    }
}

/// Short hex of observed bytes for FAIL lines (payload text never reaches the UART).
fn hexs(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = format!("{}:", bytes.len());
    for byte in bytes.iter().take(96) {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}

/// One non-blocking QueryChild: `Some((reason, code))` once the child has ended.
fn query_child(op: Operation, child: ChildRefV1) -> Result<Option<(u32, i32)>, Status> {
    let mut reply = [0u8; HEADER_LEN + 24];
    let h = ostd::session::kernel_call(KernelOp::QueryChild, op.reference().raw(), false, &child.to_bytes(), &mut reply)?;
    if h.status != Status::Ok {
        return Err(h.status);
    }
    let mut body = Reader::new(&reply[HEADER_LEN..h.total_len as usize]);
    let state = body.u32()?;
    let reason = body.u32()?;
    let code = body.i32()?;
    if body.u32()? != 0 {
        return Err(Status::Malformed);
    }
    let _sequence = body.u64()?;
    body.finish()?;
    Ok((state == 3).then_some((reason, code)))
}

/// Move everything readable now from `endpoint` into `sink`; `false` at end of stream.
fn drain_now(endpoint: Endpoint, sink: &mut Vec<u8>) -> bool {
    let mut chunk = [0u8; 512];
    for _ in 0..64 {
        match endpoint.read(&mut chunk, 0) {
            Ok(n) => sink.extend_from_slice(&chunk[..n]),
            Err(Status::WouldBlock | Status::TimedOut) => return true,
            Err(_) => return false,
        }
    }
    true
}

/// Launch `/bin/cellbox` with `spec.argv` as its context argv (no program path, no cwd,
/// no environment), drive its streams and wait for its terminal record.
fn launch(spec: &Spec) -> Result<Ran, Status> {
    let op = Operation::create(None)?;
    let result = (|| -> Result<Ran, Status> {
        let (out_read, out_write) = op.create_stream(RING)?;
        let (err_read, err_write) = op.create_stream(RING)?;
        let stdin = match spec.stdin {
            Some(_) => Some(op.create_stream(RING)?),
            None => None,
        };
        let placeholder = ObjectRefV1 { id: 1, generation: 1 };
        let mut wanted: Vec<(GrantSlot, Endpoint, EndpointRights)> = Vec::new();
        if spec.stdout {
            wanted.push((GrantSlot::Stdout, out_write, EndpointRights::WRITE_ONLY));
        }
        if spec.stderr {
            wanted.push((GrantSlot::Stderr, err_write, EndpointRights::WRITE_ONLY));
        }
        if let Some((reader, _)) = stdin {
            wanted.push((GrantSlot::Stdin, reader, EndpointRights::READ_ONLY));
        }
        let sizing: Vec<Grant> = wanted
            .iter()
            .map(|(slot, _, rights)| Grant::endpoint(*slot, placeholder, rights.bits() as u16))
            .collect::<Result<_, _>>()?;
        let context_len = ChildContext::new(op.reference().raw(), spec.argv, b"", &[], &sizing)?.as_bytes().len();
        let mut prepared = op.prepare_child(SHELL, context_len)?;
        let mut grants = Vec::new();
        for (slot, source, rights) in &wanted {
            let delegated = source.delegate(&prepared, *rights)?;
            grants.push(Grant::endpoint(*slot, delegated.reference().raw(), rights.bits() as u16)?);
        }
        let context = ChildContext::new(op.reference().raw(), spec.argv, b"", &[], &grants)?;
        prepared.write_context(&context)?;
        // Stdin that fits the ring goes in before the commit, and its writer is closed
        // then: the child may run (and, not reading stdin, exit) before this driver runs
        // again, and a writer still open when the child's reader goes away is a producer
        // whose consumer closed — the kernel cancels its operation (stop reason 3), with
        // this driver's output rings. A larger stdin is fed below, to a consumer.
        let mut feed = None;
        if let (Some((_, writer)), Some(bytes)) = (stdin, spec.stdin) {
            let mut rest = bytes;
            while !rest.is_empty() {
                match writer.write(rest, 0) {
                    Ok(n) => rest = &rest[n..],
                    Err(_) => break,
                }
            }
            if rest.is_empty() {
                let _ = writer.close();
            } else {
                feed = Some((writer, rest));
            }
        }
        let child = prepared.commit()?;
        let start = now_ms();
        // Only the child holds the writers (and the stdin reader) now: its exit ends both
        // output streams.
        let _ = (out_write.close(), err_write.close());
        if let Some((reader, _)) = stdin {
            let _ = reader.close();
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut out_open = Some(out_read);
        let mut cancelled = false;
        let exit = loop {
            // Close the feed as soon as its last byte is in.
            if let Some((writer, rest)) = feed.as_mut() {
                let done = match writer.write(rest, 0) {
                    Ok(n) => {
                        *rest = &rest[n..];
                        rest.is_empty()
                    }
                    Err(Status::WouldBlock | Status::TimedOut) => false,
                    Err(_) => true,
                };
                if done {
                    let _ = writer.close();
                    feed = None;
                }
            }
            if let Some(reader) = out_open {
                drain_now(reader, &mut out);
                if spec.close_stdout_after.is_some_and(|after| out.len() >= after) {
                    let _ = reader.close();
                    out_open = None;
                }
            }
            drain_now(err_read, &mut err);
            if let Some(ms) = spec.cancel_after_ms {
                if !cancelled && now_ms() >= start + ms {
                    cancelled = true;
                    op.cancel(2)?;
                }
            }
            if let Some((reason, code)) = query_child(op, child)? {
                break if reason == 0 { Ok(code) } else { Err(reason) };
            }
            if now_ms() >= start + spec.timeout_ms {
                return Err(Status::TimedOut);
            }
            let _ = sys_set_timer(1);
        };
        let elapsed_ms = now_ms().saturating_sub(start);
        if let Some(reader) = out_open {
            // The child has ended: what it wrote is in the ring, then end of stream.
            for _ in 0..256 {
                let before = out.len();
                if !drain_now(reader, &mut out) || out.len() == before {
                    break;
                }
            }
            let _ = reader.close();
        }
        drain_now(err_read, &mut err);
        let _ = err_read.close();
        if let Some((writer, _)) = feed {
            let _ = writer.close();
        }
        Ok(Ran { exit, out, err, elapsed_ms })
    })();
    let _ = op.cancel(2);
    let _ = op.close();
    // Every run, for the record (observed bytes as hex only).
    println(&format!("P04 INFO oneshot-run argv0={} argc={} {}", spec.argv[0], spec.argv.len(), ran(&result)));
    result
}

/// Launch with stdout and stderr only.
fn run(argv: &[&str]) -> Result<Ran, Status> {
    launch(&Spec::new(argv))
}

/// Launch with stdin bytes as well.
fn run_stdin(argv: &[&str], stdin: &[u8]) -> Result<Ran, Status> {
    launch(&Spec { stdin: Some(stdin), ..Spec::new(argv) })
}

fn ran(result: &Result<Ran, Status>) -> String {
    match result {
        Ok(ran) => ran.summary(),
        Err(status) => format!("launch {status:?}"),
    }
}

fn is(result: &Result<Ran, Status>, code: i32, out: &[u8], err: &[u8]) -> bool {
    matches!(result, Ok(r) if r.code() == Some(code) && r.out == out && r.err == err)
}

fn lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()).collect()
}

fn typed_jsonl(bytes: &[u8]) -> bool {
    let rows = lines(bytes);
    !rows.is_empty() && bytes.ends_with(b"\n") && rows.iter().all(|row| row.starts_with(b"{\"cellbox\":1,") && row.ends_with(b"}"))
}

fn no_esc(bytes: &[u8]) -> bool {
    !bytes.contains(&0x1b)
}

/// The shell's view of tasks named `name`: their tids, sorted (the shell runs `ps`
/// in process, so the listing itself adds no task).
fn tasks_named(a: &mut Term, name: &str) -> Result<Vec<String>, String> {
    let lines = a.run(&format!("ps --records | where name '==' '\"{name}\"' | select tid | to jsonl"), 30_000)?;
    let mut tids: Vec<String> = rows_of(&lines).into_iter().cloned().collect();
    tids.sort();
    Ok(tids)
}

fn task_set(a: &mut Term) -> Result<Vec<Vec<String>>, String> {
    ["cellbox", "echo", "ls", "hypervisor"].iter().map(|name| tasks_named(a, name)).collect()
}

pub(crate) fn oneshot_checks(a: &mut Term) {
    echo_exact();
    argv_literal();
    ls_fixture(a);
    cat_stdin_bytes();
    grep_status();
    records();
    rg_records();
    top_batch();
    tui_refused();
    state_refused();
    launch_refused(a);
    stage_refused();
    unknown_127(a);
    mutate_effect(a);
    denials_same_as_shell(a);
    stderr_separate();
    missing_stdout();
    consumer_close();
    cancel(a);
    frontend_argv_refused();
    shell_explicit_prefix(a);
}

fn echo_exact() {
    let r = run(&["echo", "a b", ""]);
    if let Ok(ran) = &r {
        evidence("oneshot-echo.stdout", &ran.out);
        evidence("oneshot-echo.stderr", &ran.err);
    }
    check("oneshot-echo-exact", is(&r, 0, b"a b \n", b""), &ran(&r));
}

fn argv_literal() {
    let r = run(&["echo", "p04os-argv", "$HOME", "*", "|", ";", "$(echo x)", ">", "&&"]);
    check("oneshot-argv-literal", is(&r, 0, b"p04os-argv $HOME * | ; $(echo x) > &&\n", b""), &ran(&r));
}

/// The one-shot lists the fixture directory exactly as the shell's `ls` does.
fn ls_fixture(a: &mut Term) {
    let shell = a.run("ls /p04-list", 30_000);
    let r = run(&["ls", "/p04-list"]);
    let expected = shell.as_ref().ok().map(|l| {
        let mut bytes = l.iter().filter(|line| !line.is_empty()).flat_map(|line| line.bytes().chain(core::iter::once(b'\n'))).collect::<Vec<u8>>();
        bytes.shrink_to_fit();
        bytes
    });
    let names = matches!(&r, Ok(ran) if lines(&ran.out).iter().any(|l| *l == b"alpha-entry") && lines(&ran.out).iter().any(|l| *l == b"beta-entry"));
    check(
        "oneshot-ls-fixture",
        names && matches!((&r, &expected), (Ok(ran), Some(bytes)) if ran.code() == Some(0) && &ran.out == bytes && ran.quiet_err()),
        &format!("{} shell={:?}", ran(&r), shell.map(|l| l.len())),
    );
}

/// `cat` with no operand copies its stdin: UTF-8 (NUL bytes included) byte for byte; a
/// non-UTF-8 stdin prints nothing, exactly as the shell's builtin `cat` (S10-0 CP2 note).
fn cat_stdin_bytes() {
    let text: &[u8] = b"p04os-cat\x00nul \xe2\x82\xac euro\n\x00second line\n";
    let utf8 = run_stdin(&["cat"], text);
    let dash = run_stdin(&["cat", "-"], text);
    let raw = run_stdin(&["cat"], b"p04os-raw\xff\xfe\x80\n");
    check(
        "oneshot-cat-stdin-bytes",
        is(&utf8, 0, text, b"") && is(&dash, 0, text, b"") && is(&raw, 0, b"", b""),
        &format!("utf8 {} dash {} raw {}", ran(&utf8), ran(&dash), ran(&raw)),
    );
}

fn grep_status() {
    let input: &[u8] = b"p04os-alpha\np04os-beta\n";
    let hit = run_stdin(&["grep", "beta"], input);
    let miss = run_stdin(&["grep", "p04os-none"], input);
    let bad = run(&["grep", "-Z", "x"]);
    check(
        "oneshot-grep-status",
        is(&hit, 0, b"p04os-beta\n", b"") && is(&miss, 1, b"", b"") && is(&bad, 2, b"grep: unknown flag '-Z'\n", b""),
        &format!("hit {} miss {} bad {}", ran(&hit), ran(&miss), ran(&bad)),
    );
}

fn records() {
    let jsonl = run(&["ps", "--records"]);
    check(
        "oneshot-records-jsonl",
        matches!(&jsonl, Ok(r) if r.code() == Some(0) && typed_jsonl(&r.out) && lines(&r.out).len() >= 2 && no_esc(&r.out) && r.quiet_err()),
        &ran(&jsonl),
    );
    let text = run(&["ps", "--records", "--text"]);
    check(
        "oneshot-records-text",
        matches!(&text, Ok(r) if r.code() == Some(0) && lines(&r.out).len() >= 2
            && lines(&r.out).iter().all(|l| !l.starts_with(b"{")) && no_esc(&r.out) && r.quiet_err()),
        &ran(&text),
    );
}

/// `rg` over the S7-4 fixture: typed records by default, the search text with `--text`,
/// status 0/1/2, and `rg -a … -` over the one-shot's stdin.
fn rg_records() {
    let rows = run(&["rg", "HIDTOK", RG_FX_HD]);
    let text = run(&["rg", "--text", "HIDTOK", RG_FX_HD]);
    let miss = run(&["rg", "p04os-none", RG_FX_HD]);
    let bad = run(&["rg", "-P", "x", RG_FX_HD]);
    let stdin = run_stdin(&["rg", "--text", "-a", "p04os-beta", "-"], b"p04os-alpha\np04os-beta\n");
    // A typed `path` value is its bytes in lowercase hex: `["path","2f6d6e74…"]`.
    let path = b"[\"path\",\"2f6d6e742f73642f72672f68642f7669732e747874\"]";
    let rows_ok = matches!(&rows, Ok(r) if r.code() == Some(0) && typed_jsonl(&r.out) && lines(&r.out).len() == 1
        && r.out.windows(path.len()).any(|w| w == path) && r.quiet_err());
    let text_ok = is(&text, 0, b"/mnt/sd/rg/hd/vis.txt:1:HIDTOK vis\n", b"");
    let miss_ok = is(&miss, 1, b"", b"");
    let bad_ok = matches!(&bad, Ok(r) if r.code() == Some(2) && r.out.is_empty() && !r.err.is_empty());
    let stdin_ok = is(&stdin, 0, b"<stdin>:2:p04os-beta\n", b"");
    check(
        "oneshot-rg-records",
        rows_ok && text_ok && miss_ok && bad_ok && stdin_ok,
        &format!("rows {} text {} miss {} bad {} stdin {}", ran(&rows), ran(&text), ran(&miss), ran(&bad), ran(&stdin)),
    );
}

fn top_batch() {
    let r = run(&["top", "-b"]);
    check(
        "oneshot-top-batch",
        matches!(&r, Ok(r) if r.code() == Some(0) && !r.out.is_empty() && no_esc(&r.out)
            && r.out.windows(7).any(|w| w == b"cellbox") && r.quiet_err()),
        &ran(&r),
    );
}

fn tui_refused() {
    let top = run(&["top"]);
    let edit = run(&["edit", "/tmp/p04os-edit"]);
    check(
        "oneshot-tui-refused",
        is(&top, 2, b"", b"top: no interactive terminal here; use top -b for text frames\n")
            && is(&edit, 1, b"", b"edit: no interactive terminal here\n"),
        &format!("top {} edit {}", ran(&top), ran(&edit)),
    );
}

fn state_refused() {
    let mut bad = Vec::new();
    for argv in [&["cd", "/"][..], &["export", "P04OS=1"], &["exit", "3"], &["alias", "p04os=x"]] {
        let expected = format!("cellbox: {} changes or reads shell state; run it inside 'cellbox sh'\n", argv[0]);
        let r = run(argv);
        if !is(&r, 2, b"", expected.as_bytes()) {
            bad.push(format!("{}: {}", argv[0], ran(&r)));
        }
    }
    check("oneshot-state-refused", bad.is_empty(), &format!("{bad:?}"));
}

fn launch_refused(a: &mut Term) {
    let before = task_set(a);
    let exec = run(&["exec", "/bin/echo", "p04os-exec"]);
    let hv = run(&["hv"]);
    let after = task_set(a);
    check(
        "oneshot-launch-refused",
        is(&exec, 2, b"", b"cellbox: exec launches a Cell; run it inside 'cellbox sh'\n")
            && is(&hv, 2, b"", b"cellbox: hv launches a Cell; run it inside 'cellbox sh'\n")
            && before.is_ok() && before == after,
        &format!("exec {} hv {} tasks {before:?} -> {after:?}", ran(&exec), ran(&hv)),
    );
}

fn stage_refused() {
    let r = run(&["select", "tid"]);
    check("oneshot-stage-refused", is(&r, 2, b"", b"cellbox: select is a pipeline stage; use 'cellbox sh'\n"), &ran(&r));
}

fn unknown_127(a: &mut Term) {
    let before = task_set(a);
    let nope = run(&["nope", "p04os-nope"]);
    let ls = run(&["/bin/ls", "/p04-list"]);
    let after = task_set(a);
    check(
        "oneshot-unknown-127",
        is(&nope, 127, b"", b"cellbox: nope: unknown command\n")
            && is(&ls, 127, b"", b"cellbox: /bin/ls: unknown command\n")
            && before.is_ok() && before == after,
        &format!("nope {} ls {} tasks {before:?} -> {after:?}", ran(&nope), ran(&ls)),
    );
}

fn mutate_effect(a: &mut Term) {
    let dir = "/tmp/p04os-dir";
    let file = "/tmp/p04os-dir/p04os-file";
    let mkdir = run(&["mkdir", dir]);
    let touch = run(&["touch", file]);
    let listed = run(&["ls", dir]);
    let shell = a.run(&format!("ls {dir}"), 30_000);
    let rm = run(&["rm", file]);
    let gone = run(&["ls", dir]);
    let has = |r: &Result<Ran, Status>| matches!(r, Ok(r) if lines(&r.out).iter().any(|l| *l == b"p04os-file"));
    check(
        "oneshot-mutate-effect",
        matches!(&mkdir, Ok(r) if r.code() == Some(0)) && matches!(&touch, Ok(r) if r.code() == Some(0))
            && has(&listed) && matches!(&shell, Ok(l) if l.iter().any(|l| l == "p04os-file"))
            && matches!(&rm, Ok(r) if r.code() == Some(0)) && matches!(&gone, Ok(r) if r.code() == Some(0)) && !has(&gone),
        &format!("mkdir {} touch {} ls {} shell={} rm {} after {}", ran(&mkdir), ran(&touch), ran(&listed),
            shell.is_ok(), ran(&rm), ran(&gone)),
    );
}

/// The S2-K secret namespace and a write into it are refused exactly as in the shell:
/// same nonzero status, the fixture bytes on no stream, nothing left behind; a one-shot
/// cannot even reach the S2-T/W sandbox probe (no launch, 127).
fn denials_same_as_shell(a: &mut Term) {
    let mut bad = Vec::new();
    let token = SECRET_TOKEN.as_bytes();
    for cmd in [
        "cat /srv/cellos/p04-secret",
        "grep p04 /srv/cellos/p04-secret",
        "head -n 1 /srv/cellos/p04-secret",
        "wc /srv/cellos/p04-secret",
        "sort /srv/cellos/p04-secret",
        // The shell echoes these command lines in INFO lines: no `p04os-` token here.
        "cp /srv/cellos/p04-secret /tmp/p04-osleak",
        "ls /srv/cellos",
        "vwrite /srv/cellos/p04-osw p04-osw",
    ] {
        let shell_out = a.run(cmd, 30_000);
        let shell_err = a.stderr_text().replace('\r', "");
        let shell_status = a.run("echo sos=$?", 30_000);
        let argv: Vec<&str> = cmd.split(' ').collect();
        let r = run(&argv);
        // Same status and the same bytes as the shell (its transcript has the echoed line
        // and prompt removed); a refused read or copy is nonzero, the legacy `vwrite`
        // helper reports its refused write with status 0.
        let same = match (&shell_status, &shell_out, &r) {
            (Ok(l), Ok(shell_lines), Ok(r)) => {
                let text = String::from_utf8_lossy(&r.out);
                let ours: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
                let theirs: Vec<&str> = shell_lines.iter().map(String::as_str).filter(|line| !line.is_empty()).collect();
                l.len() == 1
                    && r.code().is_some_and(|code| l[0] == format!("sos={code}") && (code != 0 || argv[0] == "vwrite"))
                    && ours == theirs
                    && r.err == shell_err.as_bytes()
                    && !(r.out.is_empty() && r.err.is_empty())
            }
            _ => false,
        };
        let leaked = matches!(&r, Ok(r) if r.out.windows(token.len()).any(|w| w == token) || r.err.windows(token.len()).any(|w| w == token));
        if !same || leaked {
            bad.push(format!("{cmd}: shell={shell_status:?} shell_err={} {} leaked={leaked}", hexs(shell_err.as_bytes()), ran(&r)));
        }
    }
    let leak = run(&["cat", "/tmp/p04-osleak"]);
    let probe = run(&["/data/bin/s2tw-probe", "sb"]);
    let clean = matches!(&leak, Ok(r) if r.code() != Some(0) && !r.out.windows(token.len()).any(|w| w == token))
        && is(&probe, 127, b"", b"cellbox: /data/bin/s2tw-probe: unknown command\n");
    check(
        "oneshot-denials-same-as-shell",
        bad.is_empty() && clean,
        &format!("{bad:?} leak {} probe {}", ran(&leak), ran(&probe)),
    );
}

fn stderr_separate() {
    let r = run(&["cat", "/p04os-missing"]);
    if let Ok(ran) = &r {
        evidence("oneshot-stderr.stdout", &ran.out);
        evidence("oneshot-stderr.stderr", &ran.err);
    }
    check("oneshot-stderr-separate", is(&r, 1, b"", b"cat: /p04os-missing: No such file or directory\n"), &ran(&r));
}

/// Without a Stdout grant nothing is written anywhere: not stderr, not the UART (the
/// runner checks the `p04os-` token).
fn missing_stdout() {
    let r = launch(&Spec { stdout: false, ..Spec::new(&["echo", "p04os-missing-stdout"]) });
    let both = launch(&Spec { stdout: false, stderr: false, ..Spec::new(&["nope", "p04os-missing-both"]) });
    check(
        "oneshot-missing-stdout",
        matches!(&r, Ok(r) if r.code().is_some_and(|c| c != 0) && r.out.is_empty() && r.err.is_empty())
            && matches!(&both, Ok(r) if r.code().is_some_and(|c| c != 0) && r.out.is_empty() && r.err.is_empty()),
        &format!("stderr-only {} none {}", ran(&r), ran(&both)),
    );
}

/// A reader that closes after the first chunk stops a streamed `rg` at once, long before
/// the 30 s write deadline: the kernel stops a producer whose final consumer closed
/// (stop reason 3, which a parent shell reports as 141) instead of letting it write on.
fn consumer_close() {
    // > 64 KiB of matching lines, written to RamFS by a one-shot `tee` over its stdin.
    let mut big = Vec::new();
    let mut n = 0u32;
    while big.len() <= 72 * 1024 {
        big.extend_from_slice(format!("p04os-line {n:06} needle\n").as_bytes());
        n += 1;
    }
    let path = "/tmp/p04os-big.txt";
    let tee = launch(&Spec { stdin: Some(&big), ..Spec::new(&["tee", path]) });
    let tee_ok = matches!(&tee, Ok(r) if r.code() == Some(0) && r.out == big && r.quiet_err());
    let r = launch(&Spec { close_stdout_after: Some(1), timeout_ms: 60_000, ..Spec::new(&["rg", "needle", path]) });
    let reported = match &r {
        Ok(Ran { exit: Err(3), .. }) => Some(141),
        Ok(Ran { exit: Err(_), .. }) => Some(130),
        Ok(Ran { exit: Ok(code), .. }) => Some(*code),
        Err(_) => None,
    };
    println(&format!("P04 INFO oneshot-consumer-close lines={n} reported={reported:?} {}", ran(&r)));
    check(
        "oneshot-consumer-close",
        tee_ok
            && matches!(&r, Ok(r) if r.exit == Err(3) && !r.out.is_empty() && r.out.len() < 64 * 1024 && r.elapsed_ms < PROMPT_EXIT_MS)
            && reported == Some(141),
        &format!("tee_ok={tee_ok} rg {}", ran(&r)),
    );
}

/// Cancelling the one-shot's Operation during `sleep 30` ends it promptly with a
/// cancellation record (reported as 130), and no `cellbox` task is left behind.
fn cancel(a: &mut Term) {
    let before = tasks_named(a, "cellbox");
    let r = launch(&Spec { cancel_after_ms: Some(1500), timeout_ms: 25_000, ..Spec::new(&["sleep", "30"]) });
    sleep_ms(2000);
    let after = tasks_named(a, "cellbox");
    let reported = match &r {
        Ok(Ran { exit: Err(_), .. }) => Some(130),
        Ok(Ran { exit: Ok(code), .. }) => Some(*code),
        Err(_) => None,
    };
    println(&format!("P04 INFO oneshot-cancel {} reported={reported:?}", ran(&r)));
    check(
        "oneshot-cancel",
        matches!(&r, Ok(r) if r.exit.is_err() && r.elapsed_ms < PROMPT_EXIT_MS) && reported == Some(130)
            && before.is_ok() && before == after,
        &format!("{} cellbox {before:?} -> {after:?}", ran(&r)),
    );
}

/// A terminal frontend runs only `sh`: any other argv exits 2 with one line on the
/// holder's stderr writer and never attaches.
fn frontend_argv_refused() {
    let result = (|| -> Result<(Option<i32>, Vec<u8>, Vec<u8>), String> {
        let mut t = Term::open(24, 80).map_err(|s| format!("open {s:?}"))?;
        t.host.launch(SHELL, &["ls", "/p04-list"], &[]).map_err(|s| format!("launch {s:?}"))?;
        let exited = wait_exit(&mut t, 60_000);
        let code = t.host.exit_code();
        let (out, err) = (core::mem::take(&mut t.out), core::mem::take(&mut t.err));
        let _ = t.host.close();
        exited?;
        Ok((code, out, err))
    })();
    check(
        "oneshot-frontend-argv-refused",
        matches!(&result, Ok((Some(2), out, err)) if out.is_empty() && err.as_slice() == FRONTEND_ONLY_SH),
        &match &result {
            Ok((code, out, err)) => format!("code={code:?} out={} err={}", hexs(out), hexs(err)),
            Err(why) => why.clone(),
        },
    );
}

/// In `cellbox sh`: `cellbox CMD` and `/bin/cellbox CMD` are the short forms in process;
/// a refused name answers like the one-shot, with no effect.
fn shell_explicit_prefix(a: &mut Term) {
    let short = a.run("echo p04os-hi", 30_000);
    let long = a.run("cellbox echo p04os-hi", 30_000);
    let ls_short = a.run("ls /p04-list", 30_000);
    let ls_long = a.run("/bin/cellbox ls /p04-list", 30_000);
    let st_ls = a.run("echo stexl=$?", 30_000);
    let pwd_before = a.run("pwd", 30_000);
    let cd = a.run("cellbox cd /p04-list", 30_000);
    let cd_err = a.stderr_text();
    let st_cd = a.run("echo stexcd=$?", 30_000);
    let pwd_after = a.run("pwd", 30_000);
    let typed = a.run("cellbox ps --records | select tid | take 1 | to jsonl", 30_000);
    let st_typed = a.run("echo stexty=$?", 30_000);
    let ok = matches!((&short, &long), (Ok(s), Ok(l)) if s == l && s == &[String::from("p04os-hi")])
        && matches!((&ls_short, &ls_long), (Ok(s), Ok(l)) if s == l && s.iter().any(|x| x == "alpha-entry"))
        && matches!(&st_ls, Ok(l) if l == &[String::from("stexl=0")])
        && cd.is_ok() && cd_err.contains("cellbox: cd changes or reads shell state")
        && matches!(&st_cd, Ok(l) if l == &[String::from("stexcd=2")])
        && pwd_before.is_ok() && pwd_before == pwd_after
        && matches!(&typed, Ok(l) if rows_of(l).len() == 1)
        && matches!(&st_typed, Ok(l) if l == &[String::from("stexty=0")]);
    check(
        "shell-explicit-prefix",
        ok,
        &format!("echo={} ls={} st_ls={st_ls:?} cd_st={st_cd:?} pwd={} typed={:?} st_typed={st_typed:?}",
            short == long, ls_short == ls_long, pwd_before == pwd_after, typed.map(|l| rows_of(&l).len())),
    );
}
