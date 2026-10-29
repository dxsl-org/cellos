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
    expect_status(jobs, "top batch Cell frame succeeds", "top -b -n 1 -d 0 -o cpu", 0);
    expect_status(jobs, "top batch task frame succeeds", "top -b -n 1 -d 0 --tasks -o stack_bytes", 0);
    expect_status(jobs, "top refuses a Cell-only sort key in the task view", "top -b -n 1 --tasks -o mem", 2);
    expect_status(jobs, "top refuses an unknown flag", "top -b --bogus", 2);
    // The harness has no terminal session: interactive top refuses before any capture.
    expect_status(jobs, "interactive top refuses without a terminal session", "top -n 1", 2);
    expect_status(jobs, "interactive top refuses a delay below 0.1 s", "top -d 0.05", 2);
    // A command substitution is a capture: interactive top refuses there with its own
    // status and the containing assignment never runs.
    expect_status(jobs, "interactive top refuses inside a command substitution", "st_top_subst=$(top)", 2);
    expect_output(jobs, "a refused substitution assigns nothing", "echo [$st_top_subst]", "[]\n");
    expect_status(jobs, "ps refuses an unknown flag", "ps --bogus", 2);
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
    assert_status(
        jobs,
        "shell touch creates file",
        "test -f /tmp/st_touch.txt",
        0,
    );

    // cp copies file
    exec(jobs, "cp /tmp/st_touch.txt /tmp/st_copied.txt");
    assert_status(
        jobs,
        "shell cp copies file",
        "test -f /tmp/st_copied.txt",
        0,
    );

    // rm deletes file
    exec(jobs, "rm /tmp/st_touch.txt");
    assert_status(
        jobs,
        "shell rm deletes file",
        "test -f /tmp/st_touch.txt",
        1,
    );

    // mkdir creates directory
    exec(jobs, "mkdir -p /tmp/st_mkdir_dir/sub");
    assert_status(
        jobs,
        "shell mkdir -p creates directory",
        "test -d /tmp/st_mkdir_dir/sub",
        0,
    );

    // rmdir removes directory
    exec(jobs, "rmdir /tmp/st_mkdir_dir/sub");
    assert_status(
        jobs,
        "shell rmdir removes directory",
        "test -d /tmp/st_mkdir_dir/sub",
        1,
    );

    // rm -i declines deletion on 'n'
    exec(jobs, "touch /tmp/st_rm_i.txt");
    exec(jobs, "echo n | rm -i /tmp/st_rm_i.txt");
    assert_status(
        jobs,
        "shell rm -i keeps file on decline",
        "test -f /tmp/st_rm_i.txt",
        0,
    );

    // rm -i accepts deletion on 'y'
    exec(jobs, "echo y | rm -i /tmp/st_rm_i.txt");
    assert_status(
        jobs,
        "shell rm -i deletes file on confirm",
        "test -f /tmp/st_rm_i.txt",
        1,
    );
    // ls lists root mount points
    let ls_out = crate::executor::capture_line("ls /", jobs);
    let ls_str = core::str::from_utf8(&ls_out).unwrap_or("");
    if ls_str.contains("tmp") && ls_str.contains("data") && ls_str.contains("srv") {
        pass("shell ls lists root mount points");
    } else {
        fail(
            "shell ls lists root mount points",
            ls_str,
            "contains tmp, data, srv",
        );
    }

    // rmdir cannot remove protected mount point
    exec(jobs, "rmdir /tmp");
    assert_status(
        jobs,
        "shell rmdir protected mount point fails",
        "test -d /tmp",
        0,
    );

    // mkdir -v outputs diagnostic
    let mk_v = crate::executor::capture_line("mkdir -v /tmp/st_mkdir_v", jobs);
    let mk_str = core::str::from_utf8(&mk_v).unwrap_or("");
    if mk_str.contains("created directory") {
        pass("shell mkdir -v outputs diagnostic");
    } else {
        fail(
            "shell mkdir -v outputs diagnostic",
            mk_str,
            "contains 'created directory'",
        );
    }

    // rmdir -p removes parent directories
    exec(jobs, "mkdir -p /tmp/st_p1/p2/p3");
    exec(jobs, "rmdir -p /tmp/st_p1/p2/p3");
    assert_status(
        jobs,
        "shell rmdir -p removes parent directories",
        "test -d /tmp/st_p1",
        1,
    );

    // ls -F appends slash to dirs
    let lsf = crate::executor::capture_line("ls -F /", jobs);
    let lsf_str = core::str::from_utf8(&lsf).unwrap_or("");
    if lsf_str.contains("tmp/") && lsf_str.contains("data/") {
        pass("shell ls -F appends slash to dirs");
    } else {
        fail(
            "shell ls -F appends slash to dirs",
            lsf_str,
            "contains 'tmp/' and 'data/'",
        );
    }

    // ls -l prints type and size
    let lsl = crate::executor::capture_line("ls -l /", jobs);
    let lsl_str = core::str::from_utf8(&lsl).unwrap_or("");
    if lsl_str.contains("d ") && lsl_str.contains("tmp") {
        pass("shell ls -l prints type and size");
    } else {
        fail(
            "shell ls -l prints type and size",
            lsl_str,
            "contains 'd ' and 'tmp'",
        );
    }

    // cp file into existing directory
    exec(jobs, "mkdir -p /tmp/st_cp_dir");
    exec(jobs, "touch /tmp/st_file.txt");
    exec(jobs, "cp /tmp/st_file.txt /tmp/st_cp_dir");
    assert_status(
        jobs,
        "shell cp copies file into target directory",
        "test -f /tmp/st_cp_dir/st_file.txt",
        0,
    );

    // cp -r copies directory recursively
    exec(jobs, "mkdir -p /tmp/st_src_tree/sub");
    exec(jobs, "touch /tmp/st_src_tree/sub/f.txt");
    exec(jobs, "cp -r /tmp/st_src_tree /tmp/st_dst_tree");
    assert_status(
        jobs,
        "shell cp -r copies directory recursively",
        "test -f /tmp/st_dst_tree/sub/f.txt",
        0,
    );

    // mv moves file into target directory
    exec(jobs, "touch /tmp/st_mv_src.txt");
    exec(jobs, "mv /tmp/st_mv_src.txt /tmp/st_cp_dir");
    assert_status(
        jobs,
        "shell mv moves file into target directory",
        "test -f /tmp/st_cp_dir/st_mv_src.txt",
        0,
    );
    assert_status(
        jobs,
        "shell mv removes original file",
        "test -f /tmp/st_mv_src.txt",
        1,
    );

    // history records commands and clears with -c
    crate::history::record_history("echo hist_test1");
    crate::history::record_history("echo hist_test2");
    let hist_out = crate::executor::capture_line("history", jobs);
    let hist_str = core::str::from_utf8(&hist_out).unwrap_or("");
    if hist_str.contains("hist_test1") && hist_str.contains("hist_test2") {
        pass("shell history lists recorded commands");
    } else {
        fail(
            "shell history lists recorded commands",
            hist_str,
            "contains 'hist_test1' and 'hist_test2'",
        );
    }
    exec(jobs, "history -c");
    let hist_c_out = crate::executor::capture_line("history", jobs);
    let hist_c_str = core::str::from_utf8(&hist_c_out).unwrap_or("");
    if !hist_c_str.contains("hist_test1") {
        pass("shell history -c clears history");
    } else {
        fail(
            "shell history -c clears history",
            hist_c_str,
            "does not contain 'hist_test1'",
        );
    }

    // cat reads from pipeline stdin
    let cat_pipe = crate::executor::capture_line("echo hello_cat | cat", jobs);
    let cat_str = core::str::from_utf8(&cat_pipe).unwrap_or("");
    if cat_str.contains("hello_cat") {
        pass("shell cat reads from pipeline stdin");
    } else {
        fail(
            "shell cat reads from pipeline stdin",
            cat_str,
            "contains 'hello_cat'",
        );
    }

    // clean up
    exec(jobs, "rm /tmp/st_copied.txt");
    exec(jobs, "rmdir /tmp/st_mkdir_v");
    exec(jobs, "rmdir /tmp/st_mkdir_dir");
    exec(jobs, "rm /tmp/st_cp_dir/st_file.txt");
    exec(jobs, "rm /tmp/st_cp_dir/st_mv_src.txt");
    exec(jobs, "rmdir /tmp/st_cp_dir");
    exec(jobs, "rm /tmp/st_file.txt");
    exec(jobs, "rm /tmp/st_dst_tree/sub/f.txt");
    exec(jobs, "rmdir /tmp/st_dst_tree/sub");
    exec(jobs, "rmdir /tmp/st_dst_tree");
    exec(jobs, "rm /tmp/st_src_tree/sub/f.txt");
    exec(jobs, "rmdir /tmp/st_src_tree/sub");
    exec(jobs, "rmdir /tmp/st_src_tree");
}

// ── Typed pipelines ───────────────────────────────────────────────────────────

/// Run `line` with its output captured; returns `(status, output)`.
fn run_captured(jobs: &mut Jobs, line: &str) -> (i32, alloc::vec::Vec<u8>) {
    let guard = crate::shell_state::CaptureGuard::new();
    let status = crate::executor::execute(&crate::parser::parse(line), jobs);
    (status, guard.finish())
}

fn check(name: &str, ok: bool, got: &str, want: &str) {
    if ok {
        pass(name);
    } else {
        fail(name, got, want);
    }
}

fn expect_status(jobs: &mut Jobs, name: &str, line: &str, want: i32) {
    let (status, _) = run_captured(jobs, line);
    check(
        name,
        status == want,
        &alloc::format!("{status}"),
        &alloc::format!("{want}"),
    );
}

/// Status 0 and exactly `want` on captured stdout.
fn expect_output(jobs: &mut Jobs, name: &str, line: &str, want: &str) {
    let (status, out) = run_captured(jobs, line);
    let got = core::str::from_utf8(&out).unwrap_or("<invalid utf8>");
    check(name, status == 0 && got == want, got, want);
}

/// Admission refusal: status 2 and the sentinel file was never created.
fn expect_refused(jobs: &mut Jobs, name: &str, line: &str, sentinel: &str) {
    let (status, out) = run_captured(jobs, line);
    let (absent, _) = run_captured(jobs, &alloc::format!("test -f {sentinel}"));
    check(
        name,
        status == 2 && out.is_empty() && absent == 1,
        &alloc::format!("status={status} output_bytes={} sentinel_status={absent}", out.len()),
        "status=2 output_bytes=0 sentinel_status=1",
    );
}

fn file_text(path: &str) -> alloc::string::String {
    match crate::native_rows::read_file_bytes_exact(path) {
        Ok(bytes) => alloc::string::String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => alloc::string::String::from("<unreadable>"),
    }
}

fn test_typed_admission_refusals(jobs: &mut Jobs) {
    const SENTINEL: &str = "/tmp/st_typed_sentinel.txt";
    exec(jobs, &alloc::format!("rm {SENTINEL}"));

    expect_refused(
        jobs,
        "typed refuses rows into a byte command without to",
        &alloc::format!("ps --records | grep sh > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses bytes into a row stage without from",
        &alloc::format!("echo x | take 1 > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses an external stage",
        &alloc::format!("ps --records | to jsonl | st_no_such_cell > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses an external head without a terminal session",
        &alloc::format!("st_no_such_cell | from json > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses an external stage between typed stages without a terminal session",
        &alloc::format!("ps --records | to jsonl | st_no_such_cell | st_no_such_cell > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses background on a typed pipeline",
        &alloc::format!("ps --records | take 1 > {SENTINEL} &"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses background on a lone producer",
        &alloc::format!("ps --records > {SENTINEL} &"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses stderr redirect",
        &alloc::format!("ps --records | take 1 2> {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses a redirect on a non-final stage",
        &alloc::format!("ps --records > {SENTINEL} | take 1"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses a row stage with no producer",
        &alloc::format!("take 1 > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses from without byte input",
        &alloc::format!("from json > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "typed refuses invalid stage arguments",
        &alloc::format!("ps --records | where cpu_tenths ~~ 1 > {SENTINEL}"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "syntax: redirect without target is refused",
        &alloc::format!("echo hi > {SENTINEL} >"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "syntax: empty pipeline stage is refused",
        &alloc::format!("echo hi > {SENTINEL} |"),
        SENTINEL,
    );
    expect_refused(
        jobs,
        "syntax: background marker must end the command",
        &alloc::format!("echo hi > {SENTINEL} & echo more"),
        SENTINEL,
    );

    // More than 16 stages is refused before any stage runs.
    let mut long = alloc::string::String::from("ps --records");
    for _ in 0..16 {
        long.push_str(" | take 1");
    }
    long.push_str(&alloc::format!(" > {SENTINEL}"));
    expect_refused(jobs, "typed refuses more than 16 stages", &long, SENTINEL);

    // Stateful built-ins and sessions never run inside a pipeline; the parent
    // context is untouched.
    let (_, cwd_before) = run_captured(jobs, "pwd");
    expect_status(jobs, "pipeline refuses cd stage", "cd /tmp | ps --records", 2);
    expect_status(jobs, "legacy pipeline refuses assignment stage", "ST_TYPED_V=1 | cat", 2);
    expect_status(jobs, "legacy pipeline refuses read stage", "echo x | read ST_TYPED_V", 2);
    expect_status(jobs, "pipeline refuses an interactive top stage", "top | grep x", 2);
    expect_status(jobs, "background refuses a stateful builtin", "cd /tmp &", 2);
    let (_, cwd_after) = run_captured(jobs, "pwd");
    check(
        "refused stateful stages leave cwd and variables unchanged",
        cwd_before == cwd_after && crate::shell_state::get_var("ST_TYPED_V").is_none(),
        core::str::from_utf8(&cwd_after).unwrap_or("<invalid utf8>"),
        core::str::from_utf8(&cwd_before).unwrap_or("<invalid utf8>"),
    );
    expect_status(jobs, "fg reports unsupported with status 2", "fg", 2);
    expect_status(jobs, "bg reports unsupported with status 2", "bg", 2);
}

fn test_typed_data_paths(jobs: &mut Jobs) {
    const ROWS: &str = "/tmp/st_typed_rows.jsonl";
    const NUMS: &str = "/tmp/st_typed_nums.jsonl";
    const BAD: &str = "/tmp/st_typed_bad.json";
    let wrote = crate::cmd_fs::vfs_write_chunked(
        ROWS,
        b"{\"name\":\"a\",\"n\":1}\n{\"name\":\"b\",\"n\":2}\n{\"name\":\"c\",\"n\":3}\n",
        false,
    ) && crate::cmd_fs::vfs_write_chunked(
        NUMS,
        b"{\"v\":18446744073709551615}\n{\"v\":null}\n{\"v\":0}\n",
        false,
    ) && crate::cmd_fs::vfs_write_chunked(BAD, b"{\"a\":", false);
    if !wrote {
        fail("typed fixture setup", "write failed", "write succeeds");
        return;
    }

    // Byte source -> from -> row stages -> explicit serializer.
    expect_output(
        jobs,
        "cat | from jsonl | where | to text filters numerically",
        &alloc::format!("cat {ROWS} | from jsonl | where n '>' int:1 | to text --field name"),
        "b\nc\n",
    );
    expect_output(
        jobs,
        "from jsonl < FILE takes the first rows",
        &alloc::format!("from jsonl < {ROWS} | take 2 | to text --field name"),
        "a\nb\n",
    );
    expect_output(
        jobs,
        "sort-by --reverse then take",
        &alloc::format!("cat {ROWS} | from jsonl | sort-by --reverse n | take 1 | to text --field name"),
        "c\n",
    );
    expect_output(
        jobs,
        "select projects fields in order",
        &alloc::format!("cat {ROWS} | from jsonl | select name | take 1 | to text --field name"),
        "a\n",
    );
    expect_output(
        jobs,
        "uint above i64::MAX compares exactly",
        &alloc::format!("cat {NUMS} | from jsonl | where v == uint:18446744073709551615 | to text --field v"),
        "18446744073709551615\n",
    );
    expect_output(
        jobs,
        "null is distinct from zero",
        &alloc::format!("cat {NUMS} | from jsonl | where v == int:0 | to text --field v"),
        "0\n",
    );
    let (status, out) = run_captured(jobs, &alloc::format!("cat {NUMS} | from jsonl | where v == null | to jsonl"));
    let text = core::str::from_utf8(&out).unwrap_or("<invalid utf8>");
    check(
        "where == null selects only the null row",
        status == 0 && text.contains("null") && !text.contains("18446744073709551615"),
        text,
        "one row with null",
    );
    // A byte consumer after to still sees exact serializer bytes.
    expect_output(
        jobs,
        "to text feeds a byte consumer",
        &alloc::format!("cat {ROWS} | from jsonl | to text --field name | grep b"),
        "b\n",
    );

    // Typed rows reach a terminal as a table and a redirect as typed JSONL.
    let (status, out) = run_captured(jobs, "ps --records | take 1");
    let table = core::str::from_utf8(&out).unwrap_or("<invalid utf8>");
    check(
        "terminal rows render as a table, not JSON",
        status == 0 && table.contains("tid") && !table.contains('{') && !table.contains("cellbox"),
        table,
        "header row without JSON",
    );
    exec(jobs, "rm /tmp/st_typed_out.jsonl");
    let (status, out) = run_captured(jobs, "ps --records | take 2 > /tmp/st_typed_out.jsonl");
    let encoded = file_text("/tmp/st_typed_out.jsonl");
    check(
        "redirected rows are typed JSONL",
        status == 0
            && out.is_empty()
            && encoded.lines().count() >= 1
            && encoded.lines().all(|line| line.starts_with("{\"cellbox\":1,\"value\":[\"record\"")),
        &encoded,
        "typed JSONL lines",
    );
    exec(jobs, "rm /tmp/st_typed_out.jsonl");
    exec(jobs, "ps --records | take 1 > /tmp/st_typed_out.jsonl");
    exec(jobs, "ps --records | take 1 >> /tmp/st_typed_out.jsonl");
    let appended = file_text("/tmp/st_typed_out.jsonl");
    check(
        "append redirect adds a second typed row",
        appended.lines().count() == 2,
        &appended,
        "two lines",
    );
    exec(jobs, "rm /tmp/st_typed_text.txt");
    exec(jobs, "ps --records --text | take 1 > /tmp/st_typed_text.txt");
    let plain = file_text("/tmp/st_typed_text.txt");
    check(
        "--text redirect is a plain table, not JSON",
        plain.contains("tid") && !plain.contains("cellbox") && !plain.contains('{'),
        &plain,
        "plain table",
    );
    exec(jobs, "rm /tmp/st_typed_json.txt");
    exec(jobs, "ps --records | take 1 | to json > /tmp/st_typed_json.txt");
    let json = file_text("/tmp/st_typed_json.txt");
    check(
        "explicit to json serializes through a redirect",
        json.starts_with('[') && json.contains("\"tid\""),
        &json,
        "JSON array of records",
    );

    // Failures are truthful and leave existing files untouched.
    exec(jobs, "echo KEEP > /tmp/st_typed_keep.txt");
    expect_status(
        jobs,
        "malformed JSON input fails with status 1",
        &alloc::format!("cat {BAD} | from json | take 1 > /tmp/st_typed_keep.txt"),
        1,
    );
    check(
        "failed typed pipeline does not touch its redirect target",
        file_text("/tmp/st_typed_keep.txt").contains("KEEP"),
        &file_text("/tmp/st_typed_keep.txt"),
        "KEEP",
    );
    expect_status(jobs, "missing cat input fails with status 1", "cat /tmp/st_no_such.json | from json", 1);
    expect_status(jobs, "missing from redirect fails with status 1", "from json < /tmp/st_no_such.json", 1);
    expect_status(jobs, "producer error fails with status 1", "ls --records /tmp/st_no_such_dir | take 1", 1);
    expect_status(jobs, "missing field fails with status 1", &alloc::format!("cat {ROWS} | from jsonl | select nosuch"), 1);
    expect_status(jobs, "bad producer arguments are status 2", "ps --records --bogus", 2);
    expect_status(jobs, "successful typed pipeline is status 0", "ps --records | take 1", 0);

    // One job per pipeline with a whole-job status and member statuses.
    exec(jobs, "ps --records | take 1 | to jsonl");
    assert_contains(jobs, "jobs lists a typed pipeline as one job", "jobs", "ps | take | to");
    assert_contains(jobs, "jobs records the whole-pipeline status", "jobs", "status=0");

    exec(jobs, "rm /tmp/st_typed_rows.jsonl");
    exec(jobs, "rm /tmp/st_typed_nums.jsonl");
    exec(jobs, "rm /tmp/st_typed_bad.json");
    exec(jobs, "rm /tmp/st_typed_out.jsonl");
    exec(jobs, "rm /tmp/st_typed_text.txt");
    exec(jobs, "rm /tmp/st_typed_json.txt");
    exec(jobs, "rm /tmp/st_typed_keep.txt");
}

/// A producer that never ends and counts how it is used.
struct EndlessRows {
    next: u64,
    fail_at: Option<u64>,
    pulled: alloc::rc::Rc<core::cell::Cell<u64>>,
    closed: alloc::rc::Rc<core::cell::Cell<u32>>,
}

impl text_engine::pipeline::RowSource for EndlessRows {
    fn next_item(
        &mut self,
    ) -> Result<Option<text_engine::pipeline::Item>, text_engine::pipeline::PipelineError> {
        use text_engine::pipeline::{Item, PipelineError, Value};
        if self.fail_at == Some(self.next) {
            return Err(PipelineError::Source(alloc::string::String::from("producer failed")));
        }
        self.next += 1;
        self.pulled.set(self.pulled.get() + 1);
        Ok(Some(Item::Row(Value::Record(alloc::vec![(
            alloc::string::String::from("n"),
            Value::UInt(self.next),
        )]))))
    }

    fn close(&mut self) {
        self.closed.set(self.closed.get() + 1);
    }
}

fn endless(
    fail_at: Option<u64>,
) -> (
    alloc::boxed::Box<dyn text_engine::pipeline::RowSource>,
    alloc::rc::Rc<core::cell::Cell<u64>>,
    alloc::rc::Rc<core::cell::Cell<u32>>,
) {
    let pulled = alloc::rc::Rc::new(core::cell::Cell::new(0));
    let closed = alloc::rc::Rc::new(core::cell::Cell::new(0));
    let source = EndlessRows { next: 0, fail_at, pulled: pulled.clone(), closed: closed.clone() };
    (alloc::boxed::Box::new(source), pulled, closed)
}

fn test_typed_early_stop() {
    use crate::typed::{run_rows, DefaultSink};
    use text_engine::pipeline::{Category, Stage};

    // take N closes an endless producer promptly: exactly N pulls, then close.
    let (source, pulled, closed) = endless(None);
    let result = run_rows(source, Category::Rows, 0, &[Stage::Take { n: 3, frame: false }], DefaultSink::TypedJsonl);
    let lines = result.as_ref().map(|bytes| bytes.iter().filter(|b| **b == b'\n').count());
    check(
        "take closes the producer after N rows",
        lines == Ok(3) && pulled.get() == 3 && closed.get() >= 1,
        &alloc::format!("lines={lines:?} pulled={} closed={}", pulled.get(), closed.get()),
        "lines=3 pulled=3 closed>=1",
    );

    // take 0 never pulls and still releases the producer.
    let (source, pulled, closed) = endless(None);
    let result = run_rows(source, Category::Rows, 0, &[Stage::Take { n: 0, frame: false }], DefaultSink::TypedJsonl);
    check(
        "take 0 closes without pulling",
        matches!(&result, Ok(bytes) if bytes.is_empty()) && pulled.get() == 0 && closed.get() >= 1,
        &alloc::format!("pulled={} closed={}", pulled.get(), closed.get()),
        "pulled=0 closed>=1",
    );

    // An upstream failure aborts the pull, is attributed to the producer stage
    // and releases it; no output bytes are produced.
    let (source, _pulled, closed) = endless(Some(2));
    let result = run_rows(source, Category::Rows, 0, &[Stage::Take { n: 10, frame: false }], DefaultSink::TypedJsonl);
    check(
        "producer failure is attributed to stage 0 and closes the chain",
        matches!(&result, Err(failure) if failure.stage == 0) && closed.get() >= 1,
        &alloc::format!("result_err={} closed={}", result.is_err(), closed.get()),
        "Err(stage 0) closed>=1",
    );

    // Global sort never waits on an infinite history: it errors at its bound.
    let (source, pulled, closed) = endless(None);
    let sort = Stage::SortBy { field: alloc::string::String::from("n"), reverse: false, frame: false };
    let result = run_rows(source, Category::Rows, 0, &[sort], DefaultSink::TypedJsonl);
    check(
        "sort-by over an endless source fails at its bound",
        result.is_err()
            && pulled.get() <= text_engine::pipeline::MAX_SORT_RECORDS as u64 + 1
            && closed.get() >= 1,
        &alloc::format!("err={} pulled={} closed={}", result.is_err(), pulled.get(), closed.get()),
        "Err within the sort bound, closed>=1",
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
    test_shell_vfs_ops(&mut jobs);
    test_vfs_bounded_grant_read(&mut jobs);
    test_typed_admission_refusals(&mut jobs);
    test_typed_data_paths(&mut jobs);
    test_typed_early_stop();
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
