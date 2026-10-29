//! Phase 07 S7-3 (`phase-07-s3-proposal.md` §7.2): fixture-free `rg` checks. Every check
//! asserts structure (rows `PATH:LINE:TEXT`, `<stdin>` for standard input, escapes) and the
//! `$?` status, never diagnostic wording. The trees live under `/tmp` (RamFS) and are built
//! here with `mkdir`/`echo >`; the ESC bytes come from `/bin/p03-filter tui-abandon`
//! redirected to a file (the shell has no `printf` and `echo -e` has no ESC escape), the
//! CR from `echo -e` piped through `/bin/p03-filter cat`.

use super::*;

const RG_MS: u64 = 60_000;

/// One command's stdout lines, whether it wrote to stderr, and its `$?` as `echo VAR=$?`
/// printed it right after.
struct RgRun {
    out: Result<Vec<String>, String>,
    stderr: bool,
    status: Result<i32, String>,
}

fn rg_run(a: &mut Term, cmd: &str, var: &str) -> RgRun {
    rg_run_ms(a, cmd, var, RG_MS)
}

fn rg_run_ms(a: &mut Term, cmd: &str, var: &str, ms: u64) -> RgRun {
    let out = a.run(cmd, ms);
    let stderr = !a.err.is_empty();
    let status = match a.run(&format!("echo {var}=$?"), 30_000) {
        Ok(lines) => match lines.as_slice() {
            [line] => line
                .strip_prefix(var)
                .and_then(|rest| rest.strip_prefix('='))
                .and_then(|n| n.parse().ok())
                .ok_or_else(|| format!("status line {line:?}")),
            _ => Err(format!("status lines {lines:?}")),
        },
        Err(why) => Err(why),
    };
    RgRun { out, stderr, status }
}

impl RgRun {
    fn rows_are(&self, rows: &[&str]) -> bool {
        matches!(&self.out, Ok(lines) if lines.len() == rows.len() && lines.iter().zip(rows).all(|(l, r)| l == r))
    }

    fn summary(&self) -> String {
        format!("out={:?} stderr={} st={:?}", self.out, self.stderr, self.status)
    }
}

/// Run setup commands; any that fails to return to the prompt or writes to stderr.
fn setup(a: &mut Term, cmds: &[&str]) -> Vec<String> {
    let mut failed = Vec::new();
    for cmd in cmds {
        match a.run(cmd, RG_MS) {
            Ok(_) if a.err.is_empty() => {}
            Ok(_) => failed.push(format!("{cmd}: stderr")),
            Err(why) => failed.push(format!("{cmd}: {why}")),
        }
    }
    failed
}

/// The LINE field of `PATH:LINE:TEXT` rows (escaped paths carry no raw `:`).
fn line_numbers(rows: &[String]) -> Option<Vec<u64>> {
    rows.iter().map(|row| row.split(':').nth(1).and_then(|n| n.parse().ok())).collect()
}

pub(crate) fn rg_checks(a: &mut Term) {
    rg_help(a);
    rg_unsupported(a);
    rg_stdin(a);
    let rows = rg_tmp_tree(a);
    rg_typed_roundtrip(a, rows);
    rg_escape(a);
}

/// `rg -h`: status 0 and a non-empty usage on stdout.
fn rg_help(a: &mut Term) {
    let help = rg_run(a, "rg -h", "strghelp");
    let text = matches!(&help.out, Ok(lines) if lines.iter().any(|l| !l.trim().is_empty()));
    check("rg-help", text && help.status == Ok(0), &help.summary());
}

/// Unsupported flags, combined shorts and an attached `-C` value are status 2 at admission:
/// a diagnostic, no row.
fn rg_unsupported(a: &mut Term) {
    let mut bad = Vec::new();
    for (cmd, var) in [("rg -P x /tmp", "strgp"), ("rg -iw x", "strgiw"), ("rg -q -C1 x", "strgqc")] {
        let run = rg_run(a, cmd, var);
        if !(run.rows_are(&[]) && run.stderr && run.status == Ok(2)) {
            bad.push(format!("{cmd}: {}", run.summary()));
        }
    }
    check("rg-unsupported-before-reads", bad.is_empty(), &format!("{bad:?}"));
}

/// Standard input needs `-a`; with it, a match is `<stdin>:LINE:TEXT` (status 0), none is 1.
fn rg_stdin(a: &mut Term) {
    let refused = rg_run(a, "echo hi | rg hi -", "strgstdin");
    check(
        "rg-stdin-requires-a",
        refused.rows_are(&[]) && refused.stderr && refused.status == Ok(2),
        &refused.summary(),
    );
    let hit = rg_run(a, "echo 'a:b' | rg -a b -", "strgahit");
    let miss = rg_run(a, "echo 'a:b' | rg -a zz -", "strgamiss");
    check(
        "rg-stdin-match",
        hit.rows_are(&["<stdin>:1:a:b"]) && hit.status == Ok(0) && miss.rows_are(&[]) && miss.status == Ok(1),
        &format!("hit {} miss {}", hit.summary(), miss.summary()),
    );
}

/// A RamFS tree with a hidden file and an `.ignore`d file: by default rows name absolute
/// paths of the visible files only (children in byte order, depth first);
/// `--hidden --no-ignore` adds both. Returns the default rows for the round trip.
fn rg_tmp_tree(a: &mut Term) -> Vec<String> {
    let failed = setup(
        a,
        &[
            "mkdir /tmp/rgt",
            "mkdir /tmp/rgt/sub",
            "echo foo > /tmp/rgt/a.txt",
            "echo foo > /tmp/rgt/sub/b.txt",
            "echo foo > /tmp/rgt/.h",
            "echo foo > /tmp/rgt/skip.txt",
            "echo skip.txt > /tmp/rgt/.ignore",
        ],
    );
    let plain = rg_run(a, "rg foo /tmp/rgt", "strgtree");
    let all = rg_run(a, "rg --hidden --no-ignore foo /tmp/rgt", "strgall");
    check(
        "rg-tmp-tree",
        failed.is_empty()
            && plain.rows_are(&["/tmp/rgt/a.txt:1:foo", "/tmp/rgt/sub/b.txt:1:foo"])
            && plain.status == Ok(0)
            && all.rows_are(&[
                "/tmp/rgt/.h:1:foo",
                "/tmp/rgt/a.txt:1:foo",
                "/tmp/rgt/skip.txt:1:foo",
                "/tmp/rgt/sub/b.txt:1:foo",
            ])
            && all.status == Ok(0),
        &format!("setup={failed:?} plain {} all {}", plain.summary(), all.summary()),
    );
    plain.out.unwrap_or_default()
}

/// Redirected `rg` writes typed JSONL (one `cellbox` record per row, nothing on the
/// terminal); `from jsonl --typed | select line_number` reads back the same line numbers.
fn rg_typed_roundtrip(a: &mut Term, text_rows: Vec<String>) {
    let written = rg_run(a, "rg foo /tmp/rgt > /tmp/r.jsonl", "strgjsonl");
    let file = a.run("cat /tmp/r.jsonl", 30_000);
    let typed = matches!(&file, Ok(lines) if {
        let records: Vec<&String> = lines.iter().filter(|l| !l.is_empty()).collect();
        records.len() == text_rows.len() && records.iter().all(|l| l.starts_with("{\"cellbox\":1,"))
    });
    let back = rg_run(a, "from jsonl --typed < /tmp/r.jsonl | select line_number", "strgback");
    let expected = line_numbers(&text_rows);
    // The terminal table: one header line, then one line per row with the bare number.
    let numbers: Option<Vec<u64>> = back.out.as_ref().ok().map(|lines| {
        lines.iter().filter_map(|l| l.trim().parse().ok()).collect()
    });
    check(
        "rg-typed-roundtrip",
        written.rows_are(&[])
            && written.status == Ok(0)
            && typed
            && expected.as_ref().is_some_and(|n| !n.is_empty())
            && numbers == expected
            && back.status == Ok(0),
        &format!("written {} file={file:?} back {} want={expected:?}", written.summary(), back.summary()),
    );
}

/// Content ESC sequences (`p03-filter tui-abandon`'s screen-mode bytes) and a CR are shown
/// escaped (`\x1B`, `\r`); none of the content's raw ESC sequences or its CR reaches the
/// shell's stdout stream.
fn rg_escape(a: &mut Term) {
    let failed = setup(
        a,
        &[
            "mkdir /tmp/rge",
            "/bin/p03-filter tui-abandon > /tmp/rge/e.txt",
            "echo -e 'p04cr\\rtail' | /bin/p03-filter cat >> /tmp/rge/e.txt",
        ],
    );
    let out = a.run("rg p0 /tmp/rge", RG_MS);
    let raw = [&b"\x1b[?1049h"[..], b"\x1b[?25l", b"\x1b[?2004h", b"p04cr\r"]
        .iter()
        .filter(|seq| occurrences(&a.out, seq) + occurrences(&a.err, seq) > 0)
        .count();
    let st = a.run("echo strgesc=$?", 30_000);
    let rows = [
        "/tmp/rge/e.txt:1:\\x1B[?1049h\\x1B[?25l\\x1B[?2004hp03-tui-abandon",
        "/tmp/rge/e.txt:2:p04cr\\rtail",
    ];
    check(
        "rg-escape",
        failed.is_empty()
            && matches!(&out, Ok(l) if l.len() == rows.len() && l.iter().zip(rows).all(|(x, y)| x == y))
            && raw == 0
            && matches!(&st, Ok(l) if l == &[String::from("strgesc=0")]),
        &format!("setup={failed:?} out={out:?} raw-content-seqs={raw} st={st:?}"),
    );
}

// ---------------------------------------------------------------------------
// Phase 07 S7-4 (`phase-07-s4-proposal.md` §2.2): checks over the private FAT32 fixture
// that `scripts/phase07_rg_fixture.py` writes to the NVMe disk mounted at /mnt/sd. The
// constants mirror the generator (and its `expected.json` in the evidence).

const FX: &str = "/mnt/sd/rg";
/// `big.log`: 7 MiB (above the 6 MiB cellbox arena, D77-7) of 64-byte lines with NEEDLE
/// lines at these line numbers.
const BIG_NEEDLES: [u64; 3] = [1, 57_345, 114_688];
const BIG_LINE: u64 = 64;
const WIDE_FILES: usize = 120;
/// A full walk of `wide/` opens every file; on TCG that takes well over [`RG_MS`].
const WIDE_MS: u64 = 600_000;
/// The 7 MiB file is read twice (binary preflight, then search) at roughly 28 KB/s on TCG
/// (S7-4 first run, 17 MiB in 1,230 s); the elapsed time is reported, never judged.
const BIG_MS: u64 = 900_000;

/// The bare numbers of a one-column `select` table (the header is not a number).
fn column_numbers(run: &RgRun) -> Option<Vec<u64>> {
    run.out.as_ref().ok().map(|lines| lines.iter().filter_map(|l| l.trim().parse().ok()).collect())
}

fn wide_rows() -> Vec<String> {
    (0..WIDE_FILES).map(|i| format!("{FX}/wide/wide{i:03}{}.txt:1:wide-hit", "x".repeat(32))).collect()
}

fn rows_equal(run: &RgRun, rows: &[String]) -> bool {
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    run.rows_are(&rows)
}

pub(crate) fn rg_fixture_checks(a: &mut Term) {
    rg_fx_big(a);
    rg_fx_wide(a);
    rg_fx_ignore(a);
    rg_fx_hidden(a);
    rg_fx_binary(a);
    rg_fx_enc(a);
    rg_fx_denied_root(a);
    rg_fx_denied_child(a);
    rg_fx_take_no_leak(a);
}

/// A file larger than the Cell quota and the cellbox arena: exact line numbers and byte
/// offsets of the three matches, through the typed JSONL record.
fn rg_fx_big(a: &mut Term) {
    let start = now_ms();
    let written = rg_run_ms(a, &format!("rg NEEDLE {FX}/big.log > /tmp/big.jsonl"), "strgbig", BIG_MS);
    let elapsed = now_ms().saturating_sub(start);
    println(&format!("P04 INFO rg-fx-big elapsed_ms={elapsed} bytes={}", BIG_NEEDLES[2] * BIG_LINE));
    let lines = rg_run(a, "from jsonl --typed < /tmp/big.jsonl | select line_number", "strgbigl");
    let offsets = rg_run(a, "from jsonl --typed < /tmp/big.jsonl | select absolute_offset", "strgbigo");
    let want_offsets: Vec<u64> = BIG_NEEDLES.iter().map(|n| (n - 1) * BIG_LINE).collect();
    check(
        "rg-fx-big-exact",
        written.rows_are(&[])
            && written.status == Ok(0)
            && column_numbers(&lines).as_deref() == Some(&BIG_NEEDLES[..])
            && column_numbers(&offsets) == Some(want_offsets)
            && lines.status == Ok(0)
            && offsets.status == Ok(0),
        &format!("written {} lines {} offsets {}", written.summary(), lines.summary(), offsets.summary()),
    );
}

/// A directory spanning more than one listing page: every one of the 120 files, in byte
/// order; `-q` prints nothing and is 0.
fn rg_fx_wide(a: &mut Term) {
    let quiet = rg_run_ms(a, &format!("rg -q wide-hit {FX}/wide"), "strgwq", WIDE_MS);
    let all = rg_run_ms(a, &format!("rg wide-hit {FX}/wide"), "strgwall", WIDE_MS);
    check(
        "rg-fx-wide",
        quiet.rows_are(&[]) && quiet.status == Ok(0) && rows_equal(&all, &wide_rows()) && all.status == Ok(0),
        &format!("quiet {} all st={:?} rows={:?}", quiet.summary(), all.status, all.out.as_ref().map(Vec::len)),
    );
}

/// `.ignore` overrides `.gitignore` in one directory; a deeper `.ignore` overrides its
/// parent's; `--no-ignore` shows every file.
fn rg_fx_ignore(a: &mut Term) {
    let plain = rg_run(a, &format!("rg IGNTOK {FX}/ign"), "strgign");
    let all = rg_run(a, &format!("rg --no-ignore IGNTOK {FX}/ign"), "strgignall");
    let want_plain = [format!("{FX}/ign/keep.txt:1:IGNTOK keep"), format!("{FX}/ign/sub/back.txt:1:IGNTOK back")];
    let want_all = [
        format!("{FX}/ign/drop.txt:1:IGNTOK drop"),
        format!("{FX}/ign/keep.txt:1:IGNTOK keep"),
        format!("{FX}/ign/sub/back.txt:1:IGNTOK back"),
    ];
    check(
        "rg-fx-ignore",
        rows_equal(&plain, &want_plain) && plain.status == Ok(0) && rows_equal(&all, &want_all) && all.status == Ok(0),
        &format!("plain {} all {}", plain.summary(), all.summary()),
    );
}

/// Hidden files and directories are skipped unless `--hidden`.
fn rg_fx_hidden(a: &mut Term) {
    let plain = rg_run(a, &format!("rg HIDTOK {FX}/hd"), "strghd");
    let all = rg_run(a, &format!("rg --hidden HIDTOK {FX}/hd"), "strghdall");
    let want_all = [
        format!("{FX}/hd/.dot.txt:1:HIDTOK dot"),
        format!("{FX}/hd/.hid/h.txt:1:HIDTOK h"),
        format!("{FX}/hd/vis.txt:1:HIDTOK vis"),
    ];
    check(
        "rg-fx-hidden",
        rows_equal(&plain, &[format!("{FX}/hd/vis.txt:1:HIDTOK vis")])
            && plain.status == Ok(0)
            && rows_equal(&all, &want_all)
            && all.status == Ok(0),
        &format!("plain {} all {}", plain.summary(), all.summary()),
    );
}

/// A NUL 40 KiB into the file: skipped (no match, status 1) unless `-a`.
fn rg_fx_binary(a: &mut Term) {
    let skipped = rg_run(a, &format!("rg BINTOK {FX}/bin"), "strgbin");
    let text = rg_run(a, &format!("rg -a BINTOK {FX}/bin"), "strgbina");
    check(
        "rg-fx-binary",
        skipped.rows_are(&[])
            && skipped.status == Ok(1)
            && rows_equal(&text, &[format!("{FX}/bin/latenul.bin:1:BINTOK text")])
            && text.status == Ok(0),
        &format!("skipped {} text {}", skipped.summary(), text.summary()),
    );
}

/// Invalid UTF-8 escaped, CR kept, a final line without LF, and offsets that count the
/// BOM's three bytes (line 2 of `bom.txt` starts at byte 9).
fn rg_fx_enc(a: &mut Term) {
    let rows = rg_run(a, &format!("rg ENCTOK {FX}/enc"), "strgenc");
    let want = [
        format!("{FX}/enc/bad.txt:1:ENCTOK \\xFF bad"),
        format!("{FX}/enc/bom.txt:2:ENCTOK bom"),
        format!("{FX}/enc/crlf.txt:1:ENCTOK crlf\\r"),
        format!("{FX}/enc/nolf.txt:2:ENCTOK nolf"),
    ];
    let written = rg_run(a, &format!("rg ENCTOK {FX}/enc/bom.txt > /tmp/bom.jsonl"), "strgbomw");
    let offset = rg_run(a, "from jsonl --typed < /tmp/bom.jsonl | select absolute_offset", "strgbomo");
    check(
        "rg-fx-enc",
        rows_equal(&rows, &want)
            && rows.status == Ok(0)
            && written.status == Ok(0)
            && column_numbers(&offset).as_deref() == Some(&[9][..]),
        &format!("rows {} written {} offset {}", rows.summary(), written.summary(), offset.summary()),
    );
}

/// A denied root is one diagnostic; the walk continues into the next root (all 120 rows)
/// and the status is 2, also with `-q`.
fn rg_fx_denied_root(a: &mut Term) {
    let walk = rg_run_ms(a, &format!("rg wide-hit /srv/cellos {FX}/wide"), "strgden", WIDE_MS);
    let quiet = rg_run_ms(a, &format!("rg -q wide-hit /srv/cellos {FX}/wide"), "strgdenq", WIDE_MS);
    check(
        "rg-fx-denied-root",
        rows_equal(&walk, &wide_rows())
            && walk.stderr
            && walk.status == Ok(2)
            && quiet.rows_are(&[])
            && quiet.stderr
            && quiet.status == Ok(2),
        &format!(
            "walk st={:?} stderr={} rows={:?} quiet {}",
            walk.status,
            walk.stderr,
            walk.out.as_ref().map(Vec::len),
            quiet.summary()
        ),
    );
}

/// A read-denied child inside a backed directory (`/srv/cellos` on the seeded P5 CellosFS
/// volume): one diagnostic for it, the readable entries before and after it in byte order
/// still searched, nothing from inside it, and status 2, also with `-q`.
fn rg_fx_denied_child(a: &mut Term) {
    let walk = rg_run(a, "rg SRVTOK /srv", "strgsrv");
    let quiet = rg_run(a, "rg -q SRVTOK /srv", "strgsrvq");
    let want = [String::from("/srv/pub/a.txt:1:SRVTOK a"), String::from("/srv/zz.txt:1:SRVTOK z")];
    check(
        "rg-fx-denied-child",
        rows_equal(&walk, &want)
            && walk.stderr
            && walk.status == Ok(2)
            && quiet.rows_are(&[])
            && quiet.stderr
            && quiet.status == Ok(2),
        &format!("walk {} quiet {}", walk.summary(), quiet.summary()),
    );
}

/// `take 1` closes rg early 40 times; a directory or file handle leaked per run would hit
/// the 32-handle VFS caps before the last run (D77-4).
fn rg_fx_take_no_leak(a: &mut Term) {
    let first = wide_rows().swap_remove(0);
    let mut bad = Vec::new();
    for i in 0..40 {
        let run = rg_run(a, &format!("rg wide-hit {FX}/wide | take 1"), "strgtake");
        if !(rows_equal(&run, &[first.clone()]) && !run.stderr && run.status == Ok(0)) {
            bad.push(format!("run {i}: {}", run.summary()));
            break;
        }
    }
    check("rg-fx-take-no-leak", bad.is_empty(), &format!("{bad:?}"));
}
