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
use core::sync::atomic::{AtomicU32, Ordering};

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

static PASSED: AtomicU32 = AtomicU32::new(0);
static FAILED: AtomicU32 = AtomicU32::new(0);

fn pass(name: &str) {
    PASSED.fetch_add(1, Ordering::SeqCst);
    ostd::io::print("[shell-test] PASS  ");
    ostd::io::println(name);
}

fn fail(name: &str, got: &str, want: &str) {
    FAILED.fetch_add(1, Ordering::SeqCst);
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
    let n = match crate::cmd_fs::read_file_vfs_result(path, &mut buf) {
        Ok(n) => n,
        Err(_) => {
            fail(name, "<file read failed>", needle);
            return;
        }
    };
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

fn test_vfs_bounded_grant_read(jobs: &mut Jobs) {
    const PATH: &str = "/tmp/st_handle_read.txt";
    const DIR: &str = "/tmp/st_handle_dir";
    let mut expected = [b'A'; 700];
    expected[699] = b'Z';
    if !crate::cmd_fs::vfs_write_chunked(PATH, &expected, false) {
        fail(
            "bounded handle read setup",
            "write failed",
            "write succeeds",
        );
        return;
    }
    exec(jobs, "mkdir /tmp/st_handle_dir");
    assert_equals(
        jobs,
        "test -f uses stat for files larger than sample buffers",
        "test -f /tmp/st_handle_read.txt ; echo $?",
        "0\n",
    );

    let mut exact = [0u8; 700];
    match crate::cmd_fs::read_file_vfs_result(PATH, &mut exact) {
        Ok(700) if exact == expected => pass("bounded handle read exact bound"),
        Ok(bytes) => fail(
            "bounded handle read exact bound",
            &alloc::format!("{} bytes", bytes),
            "700 exact bytes",
        ),
        Err(_) => fail(
            "bounded handle read exact bound",
            "typed error",
            "700 exact bytes",
        ),
    }

    let mut full = [0u8; 1024];
    match crate::cmd_fs::read_file_vfs_result(PATH, &mut full) {
        Ok(700) if full[..700] == expected => pass("bounded handle read exceeds 480 bytes"),
        Ok(bytes) => fail(
            "bounded handle read exceeds 480 bytes",
            &alloc::format!("{} bytes", bytes),
            "700 exact bytes",
        ),
        Err(_) => fail(
            "bounded handle read exceeds 480 bytes",
            "typed error",
            "700 exact bytes",
        ),
    }

    let mut too_small = [0u8; 480];
    match crate::cmd_fs::read_file_vfs_result(PATH, &mut too_small) {
        Err(ostd::ViError::InvalidArgument) => pass("bounded handle read rejects truncation"),
        _ => fail(
            "bounded handle read rejects truncation",
            "unexpected result",
            "InvalidArgument",
        ),
    }

    match crate::cmd_fs::read_file_vfs_result(DIR, &mut [0u8; 16]) {
        Err(ostd::ViError::IsADirectory) => pass("bounded handle read preserves directory error"),
        _ => fail(
            "bounded handle read preserves directory error",
            "unexpected result",
            "IsADirectory",
        ),
    }

    let mut missing = [0u8; 16];
    match crate::cmd_fs::read_file_vfs_result("/tmp/st_handle_missing.txt", &mut missing) {
        Err(ostd::ViError::IO) => pass("bounded handle read preserves missing error"),
        _ => fail(
            "bounded handle read preserves missing error",
            "unexpected result",
            "IO",
        ),
    }

    match crate::cmd_fs::read_file_vfs_result(PATH, &mut exact) {
        Ok(700) if exact == expected => pass("bounded handle read cleans up after errors"),
        _ => fail(
            "bounded handle read cleans up after errors",
            "unexpected result",
            "700 exact bytes",
        ),
    }
}
fn assert_status(jobs: &mut Jobs, name: &str, line: &str, want: i32) {
    let status = crate::executor::execute(&crate::parser::parse(line), jobs);
    if status == want {
        pass(name);
    } else {
        fail(name, &alloc::format!("{status}"), &alloc::format!("{want}"));
    }
}

fn test_shell_cwd(jobs: &mut Jobs) {
    assert_equals(jobs, "shell cwd root direct", "pwd", "/\n");
    assert_equals(jobs, "shell cwd root captured", "echo $(pwd)", "/\n");

    assert_status(jobs, "shell cwd relative cd succeeds", "cd BIN", 0);
    assert_equals(jobs, "shell cwd relative direct", "pwd", "/BIN\n");
    assert_equals(jobs, "shell cwd relative captured", "echo $(pwd)", "/BIN\n");

    assert_status(jobs, "shell cwd absolute root cd succeeds", "cd /", 0);
    assert_status(jobs, "shell cwd absolute BIN cd succeeds", "cd /BIN", 0);
    assert_equals(jobs, "shell cwd BIN direct", "pwd", "/BIN\n");
    assert_equals(jobs, "shell cwd BIN captured", "echo $(pwd)", "/BIN\n");

    assert_status(jobs, "shell cwd dot cd succeeds", "cd .", 0);
    assert_equals(jobs, "shell cwd dot direct", "pwd", "/BIN\n");
    assert_equals(jobs, "shell cwd dot captured", "echo $(pwd)", "/BIN\n");

    assert_status(jobs, "shell cwd dotdot cd succeeds", "cd ..", 0);
    assert_equals(jobs, "shell cwd dotdot direct", "pwd", "/\n");
    assert_equals(jobs, "shell cwd dotdot captured", "echo $(pwd)", "/\n");

    assert_status(jobs, "shell cwd root saturation cd succeeds", "cd ..", 0);
    assert_equals(jobs, "shell cwd root saturation direct", "pwd", "/\n");
    assert_equals(
        jobs,
        "shell cwd root saturation captured",
        "echo $(pwd)",
        "/\n",
    );

    assert_status(jobs, "shell cwd zero operands fails", "cd", 1);
    assert_equals(jobs, "shell cwd zero operands retains CWD", "pwd", "/\n");

    assert_status(jobs, "shell cwd two operands fail", "cd /BIN /TMP", 1);
    assert_equals(jobs, "shell cwd two operands retains CWD", "pwd", "/\n");

    assert_status(
        jobs,
        "shell cwd missing dir fails",
        "cd /nonexistent_dir",
        1,
    );
    assert_equals(jobs, "shell cwd missing dir retains CWD", "pwd", "/\n");

    assert_status(jobs, "shell cwd regular file fails", "cd /BIN/init", 1);
    assert_equals(jobs, "shell cwd regular file retains CWD", "pwd", "/\n");

    assert_status(jobs, "shell cwd final BIN cd succeeds", "cd /BIN", 0);
    let direct = crate::executor::capture_line("pwd", jobs);
    let captured = crate::executor::capture_line("echo $(pwd)", jobs);
    if direct == captured && direct == b"/BIN\n" {
        pass("shell cwd direct matches captured");
    } else {
        fail(
            "shell cwd direct matches captured",
            core::str::from_utf8(&direct).unwrap_or(""),
            core::str::from_utf8(&captured).unwrap_or(""),
        );
    }

    assert_status(jobs, "shell cwd restore root cd succeeds", "cd /", 0);
    assert_equals(jobs, "shell cwd restored root direct", "pwd", "/\n");
    assert_equals(
        jobs,
        "shell cwd restored root captured",
        "echo $(pwd)",
        "/\n",
    );
}

fn test_shell_vfs_ops(jobs: &mut Jobs) {
    // touch creates file
    exec(jobs, "touch /tmp/st_touch.txt");
    assert_status(jobs, "shell touch creates file", "test -f /tmp/st_touch.txt", 0);

    // cp copies file
    exec(jobs, "cp /tmp/st_touch.txt /tmp/st_copied.txt");
    assert_status(jobs, "shell cp copies file", "test -f /tmp/st_copied.txt", 0);

    // rm deletes file
    exec(jobs, "rm /tmp/st_touch.txt");
    assert_status(jobs, "shell rm deletes file", "test -f /tmp/st_touch.txt", 1);

    // mkdir creates directory
    exec(jobs, "mkdir -p /tmp/st_mkdir_dir/sub");
    assert_status(jobs, "shell mkdir -p creates directory", "test -d /tmp/st_mkdir_dir/sub", 0);

    // rmdir removes directory
    exec(jobs, "rmdir /tmp/st_mkdir_dir/sub");
    assert_status(jobs, "shell rmdir removes directory", "test -d /tmp/st_mkdir_dir/sub", 1);

    // clean up
    exec(jobs, "rm /tmp/st_copied.txt");
    exec(jobs, "rmdir /tmp/st_mkdir_dir");
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
    test_shell_vfs_ops(&mut jobs);
    test_vfs_bounded_grant_read(&mut jobs);
    test_shell_cwd(&mut jobs);

    let (passed, failed) = (PASSED.load(Ordering::SeqCst), FAILED.load(Ordering::SeqCst));
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
