//! Differential test of vigrep against the pinned reference ripgrep 15.2.0
//! (`rg --json`), per phase-07-s0-admission.md §9 acceptance item 1.
//!
//! * `corpus_matches_committed_reference_records` always runs: it walks and
//!   searches the in-memory corpus below with vigrep and compares every
//!   match/context record with `expected.txt`, which was generated once by
//!   the reference `rg --json` (command, binary hash and corpus digest are in
//!   its header). A corpus or case change without regeneration fails.
//! * `reference_rg_reproduces_committed_records` re-runs the reference
//!   binary over a materialized copy of the corpus and checks it still
//!   produces `expected.txt`. Without the binary it prints an explicit
//!   SKIPPED line. With `VIGREP_DIFF_REGENERATE=1` it rewrites
//!   `expected.txt` instead.
//! * `intentional_differences` asserts the documented differences from
//!   upstream defaults on the vigrep side, and on the rg side when the
//!   reference binary is present.
//!
//! The reference binary is `$VIGREP_REFERENCE_RG`, else
//! `/tmp/p07-s0/target-rg/release/rg` (built by S7-0 from the tag).

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};

use vigrep::{
    ignore::overrides::OverrideBuilder,
    io,
    regex::{RegexMatcher, RegexMatcherBuilder},
    searcher::{Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch},
    walk::{
        Begun, BinaryMode, DirEntry, EntryKind, FileState, Handle, PageInfo,
        Step, ViSearchTree, WalkBuilder, WalkEvent, line_submatches,
    },
};

const REFERENCE_VERSION: &str = "ripgrep 15.2.0 (rev e89fff89ac)";
const DEFAULT_RG: &str = "/tmp/p07-s0/target-rg/release/rg";
const EXPECTED: &str = include_str!("expected.txt");
/// Flags that make rg's discovery equal to vigrep's: no ancestor, global or
/// exclude ignore files, `.gitignore` without a repository, sorted serial
/// walk, no BOM sniffing.
const PARITY_FLAGS: &[&str] = &[
    "--json",
    "--no-config",
    "--sort",
    "path",
    "--no-ignore-parent",
    "--no-ignore-global",
    "--no-ignore-exclude",
    "--no-require-git",
    "--encoding",
    "none",
];

// ---------------------------------------------------------------- corpus

fn big_file() -> Vec<u8> {
    let mut out = String::new();
    for i in 0..12_000u32 {
        if i % 1_999 == 7 {
            let _ = writeln!(out, "line {i:06} has a needle in it");
        } else {
            let _ = writeln!(out, "line {i:06} filler text, nothing here");
        }
    }
    out.into_bytes()
}

/// The corpus: paths below the corpus root and their contents.
fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    let f = |p: &'static str, c: &[u8]| (p, c.to_vec());
    vec![
        f("a.txt", b"alpha\nbeta\nAlpha beta gamma\n"),
        f("B.md", b"beta\n"),
        f("crlf.txt", b"one\r\ntwo beta\r\n"),
        f("bom.txt", "\u{feff}beta first\nlast beta".as_bytes()),
        f("utf8.txt", "caf\u{e9} beta\n\u{3a3}\u{3af}\u{3c3}\u{3c5}\u{3c6}\u{3bf}\u{3c2}\nna\u{ef}ve\n".as_bytes()),
        f("invalid.txt", b"ok beta\n\xff\xfe beta\n"),
        f("empty.txt", b""),
        f("nolf.txt", b"beta"),
        f("blank.txt", b"\n\nbeta\n\n"),
        f(".hidden.txt", b"beta hidden\n"),
        f(".gitignore", b"*.log\nbuild/\n!keep.log\n"),
        f("x.log", b"beta log\n"),
        f("keep.log", b"beta keep\n"),
        f("build/out.txt", b"beta built\n"),
        f("sub/.ignore", b"secret*\n"),
        f("sub/.rgignore", b"!secret2.txt\n"),
        f("sub/secret.txt", b"beta secret\n"),
        f("sub/secret2.txt", b"beta s2\n"),
        f("sub/open.txt", b"beta open\nbetabeta\n"),
        f("sub/other.log", b"beta other log\n"),
        f("sub/deep/.gitignore", b"!x.log\n"),
        f("sub/deep/x.log", b"beta deep log\n"),
        f("sub/deep/y.rs", b"fn beta() {}\n"),
        f("ctx.txt", b"l1\nl2\nbeta\nl4\nl5\nl6\nbeta\nl8\n"),
        f("nul.bin", b"beta\0bin\n"),
        f("z/\u{17c}.txt", b"beta zz\n"),
        ("big.txt", big_file()),
    ]
}

/// One differential case: rg flags, patterns and roots (relative to the
/// directory that contains `corpus`).
struct Case {
    name: &'static str,
    flags: &'static [&'static str],
    patterns: &'static [&'static str],
    roots: &'static [&'static str],
}

const ROOT: &[&str] = &["corpus"];

const CASES: &[Case] = &[
    Case { name: "plain", flags: &[], patterns: &["beta"], roots: ROOT },
    Case {
        name: "ignore_case",
        flags: &["-i"],
        patterns: &["alpha"],
        roots: ROOT,
    },
    Case { name: "word", flags: &["-w"], patterns: &["beta"], roots: ROOT },
    Case { name: "line", flags: &["-x"], patterns: &["beta"], roots: ROOT },
    Case { name: "fixed", flags: &["-F"], patterns: &["beta()"], roots: ROOT },
    Case {
        name: "multi",
        flags: &[],
        patterns: &["alpha", "gamma", "zz", "l8"],
        roots: ROOT,
    },
    Case {
        name: "context",
        flags: &["-C", "1"],
        patterns: &["beta"],
        roots: &["corpus/ctx.txt", "corpus/a.txt"],
    },
    Case {
        name: "before_after",
        flags: &["-B", "1", "-A", "2"],
        patterns: &["l4", "l8"],
        roots: &["corpus/ctx.txt"],
    },
    Case {
        name: "hidden",
        flags: &["--hidden"],
        patterns: &["beta"],
        roots: ROOT,
    },
    Case {
        name: "no_ignore",
        flags: &["--no-ignore"],
        patterns: &["beta"],
        roots: ROOT,
    },
    Case {
        name: "globs",
        flags: &["-g", "*.txt", "-g", "!open.txt"],
        patterns: &["beta"],
        roots: ROOT,
    },
    Case {
        name: "glob_overrides_ignore",
        flags: &["-g", "*.log"],
        patterns: &["beta"],
        roots: ROOT,
    },
    Case {
        name: "unicode_class",
        flags: &[],
        patterns: &["\\p{Greek}+"],
        roots: ROOT,
    },
    Case {
        name: "empty_pattern",
        flags: &[],
        patterns: &[""],
        roots: &[
            "corpus/blank.txt",
            "corpus/crlf.txt",
            "corpus/empty.txt",
            "corpus/nolf.txt",
            "corpus/bom.txt",
        ],
    },
    Case {
        name: "zero_width",
        flags: &[],
        patterns: &["\\b"],
        roots: &["corpus/a.txt", "corpus/utf8.txt"],
    },
    Case {
        name: "byte_regex",
        flags: &[],
        patterns: &["(?-u:\\xFF)"],
        roots: ROOT,
    },
    Case {
        name: "text_mode",
        flags: &["-a"],
        patterns: &["bin"],
        roots: ROOT,
    },
    Case { name: "dot", flags: &[], patterns: &["b.ta$"], roots: ROOT },
    Case {
        name: "big",
        flags: &[],
        patterns: &["needle"],
        roots: &["corpus/big.txt"],
    },
    Case {
        name: "roots_in_order",
        flags: &[],
        patterns: &["beta"],
        roots: &["corpus/sub/deep", "corpus/a.txt", "corpus/z"],
    },
];

/// SHA-256 of the corpus and the cases, so `expected.txt` cannot silently
/// describe a different corpus.
fn corpus_digest() -> String {
    let mut data = Vec::new();
    for (path, contents) in corpus() {
        data.extend_from_slice(
            format!("{} {}\n", path, contents.len()).as_bytes(),
        );
        data.extend_from_slice(&contents);
    }
    for case in CASES {
        let line = format!(
            "{}|{:?}|{:?}|{:?}\n",
            case.name, case.flags, case.patterns, case.roots
        );
        data.extend_from_slice(line.as_bytes());
    }
    data.extend_from_slice(format!("{PARITY_FLAGS:?}").as_bytes());
    hex(&sha256(&data))
}

// ---------------------------------------------------------------- records

/// One normalized `match`/`context` record.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Record {
    kind: &'static str,
    path: String,
    line_number: u64,
    absolute_offset: u64,
    lines: Vec<u8>,
    submatches: Vec<(usize, usize)>,
}

impl Record {
    fn to_line(&self) -> String {
        let subs: Vec<String> =
            self.submatches.iter().map(|(s, e)| format!("{s}-{e}")).collect();
        format!(
            "{} {} {} {} {} {}",
            self.kind,
            self.path,
            self.line_number,
            self.absolute_offset,
            hex(&self.lines),
            if subs.is_empty() { "-".to_string() } else { subs.join(",") }
        )
    }

    fn from_line(line: &str) -> Record {
        let f: Vec<&str> = line.split(' ').collect();
        assert_eq!(f.len(), 6, "bad record line {line:?}");
        let submatches = if f[5] == "-" {
            vec![]
        } else {
            f[5].split(',')
                .map(|r| {
                    let (s, e) = r.split_once('-').unwrap();
                    (s.parse().unwrap(), e.parse().unwrap())
                })
                .collect()
        };
        Record {
            kind: if f[0] == "match" { "match" } else { "context" },
            path: f[1].to_string(),
            line_number: f[2].parse().unwrap(),
            absolute_offset: f[3].parse().unwrap(),
            lines: unhex(f[4]),
            submatches,
        }
    }
}

/// Parse `expected.txt` into (digest, case name → records).
fn committed() -> (String, BTreeMap<String, Vec<Record>>) {
    let mut digest = String::new();
    let mut cases = BTreeMap::new();
    let mut current: Option<(String, Vec<Record>)> = None;
    for line in EXPECTED.lines() {
        if let Some(d) = line.strip_prefix("# corpus-digest ") {
            digest = d.to_string();
        } else if line.starts_with('#') || line.is_empty() {
        } else if let Some(name) = line.strip_prefix("case ") {
            current = Some((name.to_string(), vec![]));
        } else if line == "end" {
            let (name, recs) = current.take().expect("end without case");
            cases.insert(name, recs);
        } else {
            current
                .as_mut()
                .expect("record outside case")
                .1
                .push(Record::from_line(line));
        }
    }
    (digest, cases)
}

// ---------------------------------------------------------------- vigrep

/// An in-memory tree holding the corpus below `/corpus`.
struct MemTree {
    files: BTreeMap<Vec<u8>, Vec<u8>>,
    dirs: BTreeMap<Vec<u8>, Vec<DirEntry>>,
    open: usize,
}

impl MemTree {
    fn new(files: &[(&str, Vec<u8>)]) -> MemTree {
        let mut tree =
            MemTree { files: BTreeMap::new(), dirs: BTreeMap::new(), open: 0 };
        tree.dirs.insert(b"/corpus".to_vec(), vec![]);
        for (path, contents) in files {
            let parts: Vec<&str> = path.split('/').collect();
            let mut dir = b"/corpus".to_vec();
            for (k, part) in parts.iter().enumerate() {
                let last = k + 1 == parts.len();
                let kind = if last { EntryKind::File } else { EntryKind::Dir };
                let name = part.as_bytes().to_vec();
                let entries = tree.dirs.entry(dir.clone()).or_default();
                if !entries.iter().any(|e| e.name == name) {
                    entries.push(DirEntry { kind, name });
                }
                dir.push(b'/');
                dir.extend_from_slice(part.as_bytes());
            }
            tree.files.insert(dir, contents.clone());
        }
        tree
    }
}

impl ViSearchTree for MemTree {
    type Dir = Vec<DirEntry>;
    type File = Vec<u8>;

    fn open_dir(&mut self, path: &[u8]) -> Result<Vec<DirEntry>, io::Error> {
        if self.files.contains_key(path) {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let entries = self
            .dirs
            .get(path)
            .cloned()
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        self.open += 1;
        Ok(entries)
    }

    fn next_page(
        &mut self,
        dir: &mut Vec<DirEntry>,
        entries: &mut Vec<DirEntry>,
    ) -> Result<PageInfo, io::Error> {
        let n = dir.len().min(2);
        entries.extend(dir.drain(..n));
        Ok(PageInfo { complete: dir.is_empty(), changed: false })
    }

    fn open_file(&mut self, path: &[u8]) -> Result<Vec<u8>, io::Error> {
        let file = self
            .files
            .get(path)
            .cloned()
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        self.open += 1;
        Ok(file)
    }

    fn read_chunk(
        &mut self,
        file: &mut Vec<u8>,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, io::Error> {
        let start = (offset as usize).min(file.len());
        let n = buf.len().min(4000).min(file.len() - start);
        buf[..n].copy_from_slice(&file[start..start + n]);
        Ok(n)
    }

    fn file_state(
        &mut self,
        file: &mut Vec<u8>,
    ) -> Result<FileState, io::Error> {
        Ok(FileState { size: file.len() as u64, modified: false })
    }

    fn close(&mut self, _: Handle<Vec<DirEntry>, Vec<u8>>) {
        self.open -= 1;
    }
}

/// Collects records the way rg's JSON printer computes them.
struct RecordSink<'a> {
    matcher: &'a RegexMatcher,
    path: String,
    out: &'a mut Vec<Record>,
    /// Pause after this many rows (0: never pause).
    cap: usize,
    queued: usize,
}

impl RecordSink<'_> {
    /// Accept a record; false pauses the search after it.
    fn push(&mut self, record: Record) -> bool {
        self.out.push(record);
        if self.cap == 0 {
            return true;
        }
        self.queued += 1;
        if self.queued < self.cap {
            return true;
        }
        self.queued = 0;
        false
    }
}

impl Sink for RecordSink<'_> {
    type Error = io::Error;

    fn matched(
        &mut self,
        _: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, io::Error> {
        let submatches = line_submatches(self.matcher, mat, 4_096).unwrap();
        Ok(self.push(Record {
            kind: "match",
            path: self.path.clone(),
            line_number: mat.line_number().unwrap(),
            absolute_offset: mat.absolute_byte_offset(),
            lines: mat.bytes().to_vec(),
            submatches,
        }))
    }

    fn context(
        &mut self,
        _: &Searcher,
        ctx: &SinkContext<'_>,
    ) -> Result<bool, io::Error> {
        Ok(self.push(Record {
            kind: "context",
            path: self.path.clone(),
            line_number: ctx.line_number().unwrap(),
            absolute_offset: ctx.absolute_byte_offset(),
            lines: ctx.bytes().to_vec(),
            submatches: vec![],
        }))
    }
}

fn flag_value(flags: &[&str], name: &str) -> Option<usize> {
    flags
        .iter()
        .position(|f| *f == name)
        .map(|i| flags[i + 1].parse().unwrap())
}

/// Run one case with vigrep over `files` mounted at `/corpus`, with the
/// configuration rg uses for the same flags (`crates/core/flags/hiargs.rs`
/// `matcher_rust`/`searcher`).
fn run_vigrep(files: &[(&str, Vec<u8>)], case: &Case) -> Vec<Record> {
    run_segmented(files, case, 0).0
}

/// [`run_vigrep`] whose sink pauses after every `cap` rows (0: never),
/// searching each file with `begin_file` + `step`; also returns the number
/// of pauses. With `cap` 0 it goes through `search_file`.
fn run_segmented(
    files: &[(&str, Vec<u8>)],
    case: &Case,
    cap: usize,
) -> (Vec<Record>, usize) {
    let flags = case.flags;
    let has = |f: &str| flags.contains(&f);
    let text = has("-a");
    let mut mb = RegexMatcherBuilder::new();
    mb.multi_line(true)
        .unicode(true)
        .octal(false)
        .fixed_strings(has("-F"))
        .case_insensitive(has("-i"))
        .line_terminator(Some(b'\n'))
        .dot_matches_new_line(false);
    if has("-w") {
        mb.word(true);
    }
    if has("-x") {
        mb.whole_line(true);
    }
    if !text {
        mb.ban_byte(Some(b'\0'));
    }
    let matcher = mb.build_many(case.patterns).unwrap();
    let ctx = flag_value(flags, "-C");
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .before_context(ctx.or(flag_value(flags, "-B")).unwrap_or(0))
        .after_context(ctx.or(flag_value(flags, "-A")).unwrap_or(0))
        .buffer_capacity(16 * 1024)
        .heap_limit(Some(16 * 1024 + 65_536))
        .build();
    let mut overrides = OverrideBuilder::new("/");
    for (i, f) in flags.iter().enumerate() {
        if *f == "-g" {
            overrides.add(flags[i + 1]).unwrap();
        }
    }
    let roots: Vec<String> =
        case.roots.iter().map(|r| format!("/{r}")).collect();
    let mut walker = WalkBuilder::new(&roots)
        .hidden(has("--hidden"))
        .ignore_files(!has("--no-ignore"))
        .overrides(overrides.build().unwrap())
        .binary(if text { BinaryMode::Search } else { BinaryMode::Skip })
        .build();
    let mut tree = MemTree::new(files);
    let mut out = vec![];
    let mut pauses = 0;
    while let Some(event) = walker.next(&mut tree, &mut || false) {
        let WalkEvent::File { path, .. } = event else {
            panic!("{}: unexpected walk event {event:?}", case.name);
        };
        let rel = String::from_utf8(path[1..].to_vec()).unwrap();
        let mut sink = RecordSink {
            matcher: &matcher,
            path: rel,
            out: &mut out,
            cap,
            queued: 0,
        };
        if cap == 0 {
            walker
                .search_file(
                    &mut tree,
                    &path,
                    &mut searcher,
                    &matcher,
                    sink,
                    &mut || false,
                )
                .unwrap();
            continue;
        }
        let begun = walker.begin_file(&mut tree, &path, &mut || false);
        let Begun::Search(mut file) = begun.unwrap() else { continue };
        loop {
            let step = file
                .step(
                    &mut walker,
                    &mut tree,
                    &mut searcher,
                    &matcher,
                    &mut sink,
                    &mut || false,
                )
                .unwrap();
            if step == Step::Finished {
                break;
            }
            pauses += 1;
        }
    }
    assert_eq!(tree.open, 0, "handles leaked");
    (out, pauses)
}

fn diff(case: &str, expected: &[Record], got: &[Record]) {
    if expected == got {
        return;
    }
    let mut msg = format!("case {case}: vigrep differs from the reference\n");
    for (i, (e, g)) in expected.iter().zip(got).enumerate() {
        if e != g {
            let _ = writeln!(
                msg,
                "first difference at record {i}:\n  rg:     {}\n  vigrep: {}",
                e.to_line(),
                g.to_line()
            );
            break;
        }
    }
    let _ = writeln!(
        msg,
        "rg records: {}, vigrep records: {}",
        expected.len(),
        got.len()
    );
    panic!("{msg}");
}

#[test]
fn corpus_matches_committed_reference_records() {
    let (digest, expected) = committed();
    assert_eq!(
        digest,
        corpus_digest(),
        "expected.txt was generated for a different corpus; regenerate with \
         VIGREP_DIFF_REGENERATE=1 and the reference rg"
    );
    let files = corpus();
    let mut total = 0;
    for case in CASES {
        let want = expected.get(case.name).unwrap_or_else(|| {
            panic!("expected.txt has no case {}", case.name)
        });
        let got = run_vigrep(&files, case);
        diff(case.name, want, &got);
        total += got.len();
    }
    assert_eq!(expected.len(), CASES.len());
    assert!(total > 100, "suspiciously few records: {total}");
}

/// Every case re-run with a sink that pauses after every row and after
/// every third row: the segmented search must reproduce every committed
/// record (line numbers, offsets, bytes and submatches).
#[test]
fn forced_pauses_reproduce_committed_records() {
    let (_, expected) = committed();
    let files = corpus();
    for cap in [1, 3] {
        let mut pauses = 0;
        let mut want_pauses = 0;
        for case in CASES {
            let want = &expected[case.name];
            let (got, n) = run_segmented(&files, case, cap);
            diff(&format!("{} (pause every {cap})", case.name), want, &got);
            pauses += n;
            // The sink's row counter restarts with each file.
            let mut per_file = BTreeMap::new();
            for record in want {
                *per_file.entry(&record.path).or_insert(0) += 1;
            }
            want_pauses += per_file.values().map(|n| n / cap).sum::<usize>();
        }
        assert_eq!(pauses, want_pauses, "cap {cap}");
        assert!(pauses >= 10, "cap {cap}: only {pauses} pauses");
    }
}

// ---------------------------------------------------------------- reference rg

fn reference_rg() -> Option<PathBuf> {
    let path = std::env::var_os("VIGREP_REFERENCE_RG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_RG));
    if !path.is_file() {
        skipped(&format!(
            "reference rg 15.2.0 not found at {} (set VIGREP_REFERENCE_RG); \
             the live reference check did not run",
            path.display()
        ));
        return None;
    }
    let out = Command::new(&path)
        .arg("--version")
        .output()
        .expect("run rg --version");
    let version = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        version.lines().next(),
        Some(REFERENCE_VERSION),
        "{} is not the pinned reference",
        path.display()
    );
    Some(path)
}

/// A temporary directory holding `corpus/`; removed on drop.
struct Materialized(PathBuf);

impl Materialized {
    fn new(tag: &str, files: &[(&str, Vec<u8>)]) -> Materialized {
        let dir = std::env::temp_dir()
            .join(format!("vigrep-diff-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let full = dir.join("corpus").join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, contents).unwrap();
        }
        Materialized(dir)
    }
}

impl Drop for Materialized {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_rg(
    rg: &Path,
    cwd: &Path,
    base_flags: &[&str],
    case: &Case,
) -> Vec<Record> {
    let mut cmd = Command::new(rg);
    cmd.current_dir(cwd)
        .env_remove("RIPGREP_CONFIG_PATH")
        .args(base_flags)
        .args(case.flags);
    for p in case.patterns {
        cmd.arg("-e").arg(p);
    }
    cmd.arg("--").args(case.roots);
    let out = cmd.output().expect("run reference rg");
    let code = out.status.code();
    assert!(
        code == Some(0) || code == Some(1),
        "{}: rg exited {:?}: {}",
        case.name,
        code,
        String::from_utf8_lossy(&out.stderr)
    );
    let mut records = vec![];
    for line in out.stdout.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        let v = Json::parse(line);
        let kind = match v.get("type").str() {
            "match" => "match",
            "context" => "context",
            _ => continue,
        };
        let data = v.get("data");
        records.push(Record {
            kind,
            path: String::from_utf8(data.get("path").text_or_bytes()).unwrap(),
            line_number: data.get("line_number").num(),
            absolute_offset: data.get("absolute_offset").num(),
            lines: data.get("lines").text_or_bytes(),
            submatches: data
                .get("submatches")
                .arr()
                .iter()
                .map(|s| {
                    (
                        s.get("start").num() as usize,
                        s.get("end").num() as usize,
                    )
                })
                .collect(),
        });
    }
    records
}

fn render_expected(rg: &Path, cases: &[(String, Vec<Record>)]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# vigrep differential reference records (generated, do not edit)"
    );
    let _ =
        writeln!(out, "# reference: {REFERENCE_VERSION} at {}", rg.display());
    let _ = writeln!(
        out,
        "# reference binary sha256 {}",
        hex(&sha256(&std::fs::read(rg).unwrap()))
    );
    let _ = writeln!(
        out,
        "# command per case: rg {} [case flags] -e PATTERN... -- ROOTS (cwd = parent of corpus/)",
        PARITY_FLAGS.join(" ")
    );
    let _ = writeln!(
        out,
        "# generator: libs/vigrep/tests/differential/main.rs with VIGREP_DIFF_REGENERATE=1"
    );
    let _ = writeln!(
        out,
        "# record: kind path line_number absolute_offset hex(lines) submatches"
    );
    let _ = writeln!(out, "# corpus-digest {}", corpus_digest());
    for (name, records) in cases {
        let _ = writeln!(out, "case {name}");
        for r in records {
            let _ = writeln!(out, "{}", r.to_line());
        }
        let _ = writeln!(out, "end");
    }
    out
}

#[test]
fn reference_rg_reproduces_committed_records() {
    let Some(rg) = reference_rg() else { return };
    let files = corpus();
    let dir = Materialized::new("parity", &files);
    let results: Vec<(String, Vec<Record>)> = CASES
        .iter()
        .map(|case| {
            (case.name.to_string(), run_rg(&rg, &dir.0, PARITY_FLAGS, case))
        })
        .collect();
    if std::env::var_os("VIGREP_DIFF_REGENERATE").is_some() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/differential/expected.txt");
        std::fs::write(&path, render_expected(&rg, &results)).unwrap();
        eprintln!("regenerated {}", path.display());
        return;
    }
    let (digest, expected) = committed();
    assert_eq!(digest, corpus_digest());
    for (name, records) in &results {
        diff(name, &expected[name], records);
    }
}

// ---------------------------------------------------------------- differences

fn paths(records: &[Record]) -> Vec<String> {
    let mut p: Vec<String> = records.iter().map(|r| r.path.clone()).collect();
    p.dedup();
    p
}

/// Differences from rg's defaults that are intentional (dossier §8,
/// contract §2.4). Each is asserted on the vigrep side; with the reference
/// binary present, rg's default behaviour is asserted to differ.
#[test]
fn intentional_differences() {
    let mut files = corpus();
    // A late NUL, past rg's first 64 KiB buffer.
    let mut late = "beta late\n".repeat(8_000).into_bytes();
    late.extend_from_slice(b"\0\n");
    files.push(("late.bin", late));
    let rg = reference_rg();
    let dir = rg.as_ref().map(|_| Materialized::new("diffs", &files));
    let rg_run = |flags: &[&str], case: &Case| -> Option<Vec<Record>> {
        Some(run_rg(rg.as_deref()?, &dir.as_ref()?.0, flags, case))
    };
    let base: Vec<&str> = PARITY_FLAGS.to_vec();
    let without = |drop: &[&str]| -> Vec<&str> {
        let mut out = vec![];
        let mut skip = false;
        for f in &base {
            if skip {
                skip = false;
                continue;
            }
            if drop.contains(f) {
                skip = *f == "--encoding";
                continue;
            }
            out.push(*f);
        }
        out
    };

    // 1. Stable binary preflight: a file with a NUL anywhere yields no rows.
    //    rg (quit-on-NUL per buffer) reports the matches before the NUL.
    let case = Case {
        name: "late_nul",
        flags: &[],
        patterns: &["late"],
        roots: &["corpus/late.bin"],
    };
    let ours = run_vigrep(&files, &case);
    assert!(ours.is_empty());
    let case_r = Case { roots: &["corpus"], ..case };
    assert!(
        !paths(&run_vigrep(&files, &case_r))
            .contains(&"corpus/late.bin".to_string())
    );
    if let Some(theirs) = rg_run(&base, &case_r) {
        assert!(
            theirs.iter().any(|r| r.path == "corpus/late.bin"),
            "rg reports rows before the NUL"
        );
    }

    // 2. Subtree-only ignore discovery: rules above a root never apply.
    //    rg (default) applies `corpus/.gitignore`'s `*.log` to `sub/other.log`.
    let case = Case {
        name: "parents",
        flags: &[],
        patterns: &["other"],
        roots: &["corpus/sub"],
    };
    assert_eq!(paths(&run_vigrep(&files, &case)), ["corpus/sub/other.log"]);
    if let Some(theirs) = rg_run(&without(&["--no-ignore-parent"]), &case) {
        assert!(theirs.is_empty(), "rg applies the parent .gitignore");
    }

    // 3. `.gitignore` applies without a repository (rg needs a `.git`
    //    unless `--no-require-git`).
    let case = Case {
        name: "require_git",
        flags: &[],
        patterns: &["log"],
        roots: ROOT,
    };
    let ours = paths(&run_vigrep(&files, &case));
    assert!(!ours.contains(&"corpus/x.log".to_string()));
    if let Some(theirs) = rg_run(&without(&["--no-require-git"]), &case) {
        assert!(paths(&theirs).contains(&"corpus/x.log".to_string()));
    }

    // 4. No BOM sniffing (`--encoding none` is the only mode): the BOM is
    //    line content and offsets count it. rg's default strips it.
    let case = Case {
        name: "bom",
        flags: &[],
        patterns: &["first"],
        roots: &["corpus/bom.txt"],
    };
    let ours = run_vigrep(&files, &case);
    assert_eq!(ours[0].lines, "\u{feff}beta first\n".as_bytes());
    assert_eq!(ours[0].submatches, [(8, 13)]);
    if let Some(theirs) = rg_run(&without(&["--encoding"]), &case) {
        assert_eq!(theirs[0].lines, b"beta first\n");
        assert_eq!(theirs[0].submatches, [(5, 10)]);
    }

    // 5. Overlapping roots are deduplicated by path (DV7-3); rg searches
    //    the overlap twice.
    let case = Case {
        name: "overlap",
        flags: &[],
        patterns: &["deep"],
        roots: &["corpus/sub", "corpus/sub/deep"],
    };
    assert_eq!(run_vigrep(&files, &case).len(), 1);
    if let Some(theirs) = rg_run(&base, &case) {
        assert_eq!(theirs.len(), 2);
    }
    if rg.is_none() {
        skipped("rg side of intentional_differences (no reference binary)");
    }
}

/// Report a skipped check on the real stderr: libtest captures `eprintln!`
/// of passing tests, which would make the skip silent.
fn skipped(what: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr(), "SKIPPED: {what}");
}

// ---------------------------------------------------------------- helpers

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// SHA-256 (FIPS 180-4), for the corpus digest.
fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b,
        0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7,
        0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152,
        0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f,
        0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f,
        0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(
                block[i * 4..i * 4 + 4].try_into().unwrap(),
            );
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6)
                ^ v[4].rotate_right(11)
                ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2)
                ^ v[0].rotate_right(13)
                ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

#[test]
fn sha256_known_answers() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

/// A minimal JSON value for reading rg's `--json` lines.
#[derive(Debug)]
enum Json {
    Null,
    Bool,
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn parse(bytes: &[u8]) -> Json {
        let s = std::str::from_utf8(bytes).expect("rg --json emits UTF-8");
        let mut p = s.char_indices().peekable();
        parse_value(s, &mut p)
    }

    fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(fields) => fields
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .unwrap_or(&Json::Null),
            _ => &Json::Null,
        }
    }

    fn str(&self) -> &str {
        match self {
            Json::Str(s) => s,
            _ => "",
        }
    }

    fn num(&self) -> u64 {
        match self {
            Json::Num(n) => *n as u64,
            other => panic!("expected number, got {other:?}"),
        }
    }

    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }

    /// rg's `{"text": ...}` or `{"bytes": base64}` data value.
    fn text_or_bytes(&self) -> Vec<u8> {
        match (self.get("text"), self.get("bytes")) {
            (Json::Str(t), _) => t.as_bytes().to_vec(),
            (_, Json::Str(b)) => base64_decode(b),
            _ => panic!("expected text or bytes in {self:?}"),
        }
    }
}

type Chars<'a> = std::iter::Peekable<std::str::CharIndices<'a>>;

fn skip_ws(p: &mut Chars<'_>) {
    while p.peek().is_some_and(|(_, c)| c.is_ascii_whitespace()) {
        p.next();
    }
}

fn parse_value(s: &str, p: &mut Chars<'_>) -> Json {
    skip_ws(p);
    let (i, c) = *p.peek().expect("unexpected end of JSON");
    match c {
        '{' => {
            p.next();
            let mut fields = vec![];
            loop {
                skip_ws(p);
                if p.peek().map(|x| x.1) == Some('}') {
                    p.next();
                    return Json::Obj(fields);
                }
                let Json::Str(k) = parse_value(s, p) else {
                    panic!("object key")
                };
                skip_ws(p);
                assert_eq!(p.next().map(|x| x.1), Some(':'));
                fields.push((k, parse_value(s, p)));
                skip_ws(p);
                match p.next().map(|x| x.1) {
                    Some(',') => {}
                    Some('}') => return Json::Obj(fields),
                    other => panic!("bad object separator {other:?}"),
                }
            }
        }
        '[' => {
            p.next();
            let mut items = vec![];
            loop {
                skip_ws(p);
                if p.peek().map(|x| x.1) == Some(']') {
                    p.next();
                    return Json::Arr(items);
                }
                items.push(parse_value(s, p));
                skip_ws(p);
                match p.next().map(|x| x.1) {
                    Some(',') => {}
                    Some(']') => return Json::Arr(items),
                    other => panic!("bad array separator {other:?}"),
                }
            }
        }
        '"' => {
            p.next();
            let mut out = String::new();
            loop {
                let (_, c) = p.next().expect("unterminated string");
                match c {
                    '"' => return Json::Str(out),
                    '\\' => {
                        let (_, e) = p.next().unwrap();
                        match e {
                            '"' | '\\' | '/' => out.push(e),
                            'b' => out.push('\u{8}'),
                            'f' => out.push('\u{c}'),
                            'n' => out.push('\n'),
                            'r' => out.push('\r'),
                            't' => out.push('\t'),
                            'u' => {
                                fn unit(p: &mut Chars<'_>) -> u32 {
                                    let h: String = (0..4)
                                        .map(|_| p.next().unwrap().1)
                                        .collect();
                                    u32::from_str_radix(&h, 16).unwrap()
                                }
                                let mut cp = unit(p);
                                if (0xD800..0xDC00).contains(&cp) {
                                    assert_eq!(
                                        p.next().map(|x| x.1),
                                        Some('\\')
                                    );
                                    assert_eq!(
                                        p.next().map(|x| x.1),
                                        Some('u')
                                    );
                                    let lo = unit(p);
                                    cp = 0x10000
                                        + ((cp - 0xD800) << 10)
                                        + (lo - 0xDC00);
                                }
                                out.push(char::from_u32(cp).unwrap());
                            }
                            other => panic!("bad escape {other:?}"),
                        }
                    }
                    c => out.push(c),
                }
            }
        }
        't' | 'f' | 'n' => {
            let word: String = std::iter::from_fn(|| {
                p.next_if(|(_, c)| c.is_ascii_alphabetic()).map(|x| x.1)
            })
            .collect();
            if word == "null" { Json::Null } else { Json::Bool }
        }
        _ => {
            let mut end = i;
            while let Some(&(j, c)) = p.peek() {
                if c.is_ascii_digit() || "+-.eE".contains(c) {
                    end = j + c.len_utf8();
                    p.next();
                } else {
                    break;
                }
            }
            Json::Num(s[i..end].parse().expect("number"))
        }
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("bad base64 {c}"),
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();
    let mut out = vec![];
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c) << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    out
}

#[test]
fn json_and_base64_helpers() {
    let v = Json::parse(br#"{"type":"match","data":{"lines":{"bytes":"/w=="},"n":12,"s":"a\u00e9\ud83d\ude00\n","x":[true,null]}}"#);
    assert_eq!(v.get("type").str(), "match");
    assert_eq!(v.get("data").get("lines").text_or_bytes(), [0xff]);
    assert_eq!(v.get("data").get("n").num(), 12);
    assert_eq!(v.get("data").get("s").str(), "a\u{e9}\u{1f600}\n");
    assert_eq!(base64_decode("YmV0YQ=="), b"beta");
}
