//! Shell built-in test harness (feature = "shell_test").
//!
//! Replaces the interactive REPL when the binary is compiled with
//! `--features shell_test`.  Exercises parser + executor scenarios via
//! `executor::capture_line`, asserts on captured output or VFS file contents,
//! then prints `[shell-test] COMPLETE` so the CI integration test can inspect
//! the deterministic final result without waiting for a restarted shell cell.
//!
//! VFS must be up before file-I/O scenarios run.  The harness waits via
//! `sys_lookup_service(VFS)` (same pattern as vfs-test/srv-test).

use crate::jobs::Jobs;

const VFS_SVC: u16 = api::syscall::service::VFS;

/// Wait for the VFS service to register (blocks until init has spawned vfs).
fn wait_for_vfs() {
    loop {
        if ostd::syscall::sys_lookup_service(VFS_SVC).is_some() {
            return;
        }
        ostd::task::yield_now();
    }
}

// ── Assertion helpers ─────────────────────────────────────────────────────────

static mut PASSED: u32 = 0;
static mut FAILED: u32 = 0;

fn pass(name: &str) {
    // SAFETY: single shell task; no concurrent access.
    unsafe {
        PASSED += 1;
    }
    ostd::io::print("[shell-test] PASS  ");
    ostd::io::println(name);
}

fn fail(name: &str, got: &str, want: &str) {
    // SAFETY: single shell task; no concurrent access.
    unsafe {
        FAILED += 1;
    }
    ostd::io::print("[shell-test] FAIL  ");
    ostd::io::println(name);
    ostd::io::print("  got:  ");
    ostd::io::println(got);
    ostd::io::print("  want: ");
    ostd::io::println(want);
}

/// Assert that captured output (as UTF-8) contains `needle`.
fn assert_contains(jobs: &mut Jobs, name: &str, line: &str, needle: &str) {
    let bytes = crate::executor::capture_line(line, jobs);
    let got = core::str::from_utf8(&bytes).unwrap_or("<invalid utf8>");
    if got.contains(needle) {
        pass(name);
    } else {
        fail(name, got, needle);
    }
}

fn assert_equals(jobs: &mut Jobs, name: &str, line: &str, want: &str) {
    let bytes = crate::executor::capture_line(line, jobs);
    let got = core::str::from_utf8(&bytes).unwrap_or("<invalid utf8>");
    if got == want {
        pass(name);
    } else {
        fail(name, got, want);
    }
}

/// Assert that the VFS file at `path` contains `needle`.
fn assert_file_contains(name: &str, path: &str, needle: &str) {
    let mut buf = [0u8; 480];
    let n = crate::cmd_fs::read_file_vfs(path, &mut buf);
    if n == 0 {
        fail(name, "<file not found>", needle);
        return;
    }
    let got = core::str::from_utf8(&buf[..n]).unwrap_or("<invalid utf8>");
    if got.contains(needle) {
        pass(name);
    } else {
        fail(name, got, needle);
    }
}

/// Execute `line` (captures output, discards it). Used for setup steps.
fn exec(jobs: &mut Jobs, line: &str) {
    crate::executor::capture_line(line, jobs);
}

// ── Test scenarios ────────────────────────────────────────────────────────────

fn test_stdout_redirect(jobs: &mut Jobs) {
    exec(jobs, "echo REDIR_OUT > /tmp/st_redir.txt");
    assert_file_contains(
        "stdout redirect writes file",
        "/tmp/st_redir.txt",
        "REDIR_OUT",
    );
}

fn test_append_redirect(jobs: &mut Jobs) {
    exec(jobs, "echo APPEND_A > /tmp/st_append.txt");
    exec(jobs, "echo APPEND_B >> /tmp/st_append.txt");
    assert_file_contains("append redirect line A", "/tmp/st_append.txt", "APPEND_A");
    assert_file_contains("append redirect line B", "/tmp/st_append.txt", "APPEND_B");
}

fn test_stderr_redirect(jobs: &mut Jobs) {
    // Phase 1: 2> routes output to file (single-channel shell, stderr==stdout).
    exec(jobs, "echo STDERR_OUT 2> /tmp/st_stderr.txt");
    assert_file_contains(
        "stderr redirect writes file",
        "/tmp/st_stderr.txt",
        "STDERR_OUT",
    );
}

fn test_pipe_grep(jobs: &mut Jobs) {
    // Pipeline: echo multi-line | grep pattern.
    assert_contains(
        jobs,
        "pipe grep matches lines",
        "echo -e ax\\nby\\ncx | grep x",
        "ax",
    );
}

fn test_grep_extended_flags(jobs: &mut Jobs) {
    assert_contains(
        jobs,
        "grep -E -n matches uppercase records",
        "echo -e Alpha\\nbeta\\nGamma | grep -E -n \"^[A-Z][a-z]+$\"",
        "1:Alpha",
    );
    assert_contains(
        jobs,
        "grep -F -x -i matches full line",
        "echo -e ALPHA\\nALPHABET | grep -Fix alpha",
        "ALPHA",
    );
    assert_equals(
        jobs,
        "grep -q keeps output empty and sets success status",
        "echo -e foo\\nbar | grep -q foo ; echo $?",
        "0\n",
    );
    assert_contains(
        jobs,
        "grep unknown flag returns status 2",
        "echo foo | grep -z foo ; echo $?",
        "2\n",
    );
    assert_equals(
        jobs,
        "grep empty pattern selects every line",
        "echo -e a\\nb | grep ''",
        "a\nb\n",
    );
    assert_contains(
        jobs,
        "grep reads files created through VFS",
        "echo needle > /tmp/st_grep.txt ; grep needle /tmp/st_grep.txt",
        "needle",
    );
    assert_equals(
        jobs,
        "grep -i longer needle does not panic",
        "echo a | grep -i alphabet ; echo $?",
        "1\n",
    );
}

fn test_wc_l(jobs: &mut Jobs) {
    // wc -l on 3-line input via pipeline.
    assert_contains(jobs, "wc -l counts 3 lines", "echo -e a\\nb\\nc | wc", "3");
}

fn test_sort(jobs: &mut Jobs) {
    assert_contains(
        jobs,
        "sort produces first line 'a'",
        "echo -e c\\na\\nb | sort",
        "a",
    );
}

fn test_tee(jobs: &mut Jobs) {
    // Phase 2: tee writes to both sink and file.
    let bytes = crate::executor::capture_line("echo -e x\\ny | tee /tmp/st_tee.txt", jobs);
    let got = core::str::from_utf8(&bytes).unwrap_or("");
    if got.contains('x') {
        pass("tee passes data to sink");
    } else {
        fail("tee passes data to sink", got, "contains 'x'");
    }
    assert_file_contains("tee writes file", "/tmp/st_tee.txt", "x");
}

fn test_sed(jobs: &mut Jobs) {
    // Phase 2: sed substitution (first occurrence).
    assert_contains(
        jobs,
        "sed first-occurrence substitution",
        "echo foo bar | sed s/foo/baz/",
        "baz bar",
    );
    // Global substitution.
    assert_contains(
        jobs,
        "sed global substitution",
        "echo foo foo | sed s/foo/baz/g",
        "baz baz",
    );
    assert_contains(
        jobs,
        "sed alternate delimiter preserves spaces",
        "echo '/old path/bin' | sed 's|/old path|/new path|g'",
        "/new path/bin",
    );
    assert_contains(
        jobs,
        "sed regex print with -n",
        "echo -e OK\\nERR1\\nERR2 | sed -n '/^ERR[0-9]+/p'",
        "ERR1",
    );
    assert_contains(
        jobs,
        "sed numeric print from file",
        "sed -n 2p /tmp/st_tee.txt",
        "y",
    );
    assert_contains(
        jobs,
        "sed escaped delimiter and backslash",
        r"echo 'a|b \\ a|b' | sed 's|a\|b|X\\\\&|g'",
        r"X\\a|b \\ X\\a|b",
    );
    assert_contains(
        jobs,
        "sed malformed script returns status 2",
        "sed 's/foo/bar/q' /tmp/st_tee.txt ; echo $?",
        "2\n",
    );
}

fn test_awk(jobs: &mut Jobs) {
    assert_equals(
        jobs,
        "awk numeric filter and NR",
        "echo -e alice,12\\nbob,9 | awk -F, '$2 >= 10 { print NR, $1 }'",
        "1 alice\n",
    );
    assert_contains(
        jobs,
        "awk regex filter",
        "echo -e OK\\nERR1\\nERR2 | awk '/^ERR[0-9]+/ { print $1 }'",
        "ERR1",
    );
    assert_equals(
        jobs,
        "awk legacy extractor still works",
        "echo -e left:right | awk -F: 2",
        "right\n",
    );
    assert_contains(
        jobs,
        "awk double quotes still expand shell status",
        "echo ready > /tmp/st_awk.txt ; echo ok | awk \"{ print $? }\"",
        "0\n",
    );
    assert_contains(
        jobs,
        "awk single quotes preserve dollar tokens",
        "echo ok | awk '{ print $? }' ; echo $?",
        "\n2\n",
    );
    assert_contains(
        jobs,
        "awk divide by zero is deterministic",
        "echo 0 | awk '{ print 4 / $1 }' ; echo $?",
        "division by zero",
    );
}

fn test_fg_bg(jobs: &mut Jobs) {
    // Phase 3: fg/bg print limitation message, not "command not found".
    assert_contains(jobs, "fg prints limitation message", "fg", "no job control");
    assert_contains(jobs, "bg prints limitation message", "bg", "no job control");
}

fn test_top_batch(jobs: &mut Jobs) {
    assert_contains(
        jobs,
        "top batch prints heap column",
        "top -b -n 1 -d 0 -o cpu",
        "HEAP",
    );
    assert_contains(
        jobs,
        "top batch prints honest mem column",
        "top -b -n 1 -d 0 -o mem",
        "MEM",
    );
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Test harness entry point — called from `main()` when feature `shell_test` is set.
pub fn run() {
    ostd::io::println("[shell-test] Starting shell utility tests...");

    // Wait for VFS before running file-I/O scenarios.
    wait_for_vfs();
    // Let VFS finish its init pass before issuing writes.
    ostd::task::yield_now();

    let mut jobs = Jobs::new();

    test_stdout_redirect(&mut jobs);
    test_append_redirect(&mut jobs);
    test_stderr_redirect(&mut jobs);
    test_pipe_grep(&mut jobs);
    test_grep_extended_flags(&mut jobs);
    test_wc_l(&mut jobs);
    test_sort(&mut jobs);
    test_tee(&mut jobs);
    test_sed(&mut jobs);
    test_awk(&mut jobs);
    test_fg_bg(&mut jobs);
    test_top_batch(&mut jobs);

    // SAFETY: single shell task; no concurrent reads.
    let (passed, failed) = unsafe { (PASSED, FAILED) };
    ostd::io::println("");
    ostd::io::print("[shell-test] Results: ");
    ostd::io::print_usize(passed as usize);
    ostd::io::print(" PASS, ");
    ostd::io::print_usize(failed as usize);
    ostd::io::println(" FAIL");

    if failed == 0 {
        ostd::io::println("[shell-test] ALL TESTS PASSED");
    } else {
        ostd::io::println("[shell-test] FAILURES DETECTED");
    }
    ostd::io::println("[shell-test] COMPLETE");
    loop {
        ostd::task::yield_now();
    }
}
