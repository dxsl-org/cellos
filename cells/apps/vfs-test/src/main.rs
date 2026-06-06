//! VFS integration test cell.
//!
//! Runs automated test scenarios against the VFS service and prints PASS/FAIL
//! for each.  Exit code 0 = all pass, 1 = at least one failure.
//!
//! Spawn with: `spawn /bin/vfs-test` from the shell.

#![no_std]
#![no_main]
extern crate alloc;

use core::sync::atomic::{AtomicU32, Ordering};

/// VFS service task endpoint (task 3: init=1, user_hello=2, vfs=3).
const VFS: usize = 3;

static PASSED: AtomicU32 = AtomicU32::new(0);
static FAILED: AtomicU32 = AtomicU32::new(0);

// ── Test harness ─────────────────────────────────────────────────────────────

fn vfs_req(req: &api::ipc::VfsRequest<'_>) -> api::ipc::VfsResponse<'static> {
    let mut send_buf = [0u8; 512];
    let n = api::ipc::encode(req, &mut send_buf).map(|s| s.len()).unwrap_or(0);
    ostd::syscall::sys_send(VFS, &send_buf[..n]);
    // Leak the recv buffer so VfsResponse::Data borrows from it safely.
    // This is fine in a test cell that runs and exits.
    let buf: &'static mut [u8; 512] = alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; 512]));
    match ostd::syscall::sys_recv(0, buf) {
        ostd::syscall::SyscallResult::Ok(_) => {
            api::ipc::decode::<api::ipc::VfsResponse>(buf)
                .unwrap_or(api::ipc::VfsResponse::Err(0xFE))
        }
        _ => api::ipc::VfsResponse::Err(0xFD),
    }
}

fn pass(msg: &str) {
    PASSED.fetch_add(1, Ordering::Relaxed);
    ostd::io::print("[PASS] ");
    ostd::io::println(msg);
}

fn fail(msg: &str) {
    FAILED.fetch_add(1, Ordering::Relaxed);
    ostd::io::print("[FAIL] ");
    ostd::io::println(msg);
}

macro_rules! assert_ok {
    ($req:expr, $msg:literal) => {
        match vfs_req(&$req) {
            api::ipc::VfsResponse::Ok => pass($msg),
            _ => fail($msg),
        }
    };
}

macro_rules! assert_err {
    ($req:expr, $code:expr, $msg:literal) => {
        match vfs_req(&$req) {
            api::ipc::VfsResponse::Err(c) if c == $code => pass($msg),
            api::ipc::VfsResponse::Err(c) => {
                ostd::io::print("[FAIL] "); ostd::io::print($msg);
                ostd::io::print(" — wrong code: "); ostd::io::print_usize(c as usize); ostd::io::println("");
                FAILED.fetch_add(1, Ordering::Relaxed);
            }
            _ => fail($msg),
        }
    };
}

// ── Test scenarios ───────────────────────────────────────────────────────────

/// 1. File lifecycle: write → read → verify → unlink → verify gone.
fn test_file_lifecycle() {
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/test_lifecycle.txt", content: b"hello world" },
        "write /data/test_lifecycle.txt");

    // Verify content via Stat (size check)
    match vfs_req(&api::ipc::VfsRequest::Stat("/data/test_lifecycle.txt")) {
        api::ipc::VfsResponse::Stat { size: 11, is_dir: false } => pass("stat size=11 is_file"),
        _ => fail("stat after write"),
    }

    assert_ok!(api::ipc::VfsRequest::Unlink("/data/test_lifecycle.txt"),
        "unlink /data/test_lifecycle.txt");

    // Verify gone
    match vfs_req(&api::ipc::VfsRequest::Stat("/data/test_lifecycle.txt")) {
        api::ipc::VfsResponse::Err(_) => pass("stat after unlink returns Err"),
        _ => fail("stat after unlink should return Err"),
    }
}

/// 2. Directory operations: mkdir, write inside, listdir, rmdir.
fn test_directory_ops() {
    assert_ok!(api::ipc::VfsRequest::Mkdir("/data/testdir"),
        "mkdir /data/testdir");
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/testdir/file.txt", content: b"x" },
        "write inside testdir");

    // ListDir should contain "f:file.txt"
    match vfs_req(&api::ipc::VfsRequest::ListDir("/data/testdir")) {
        api::ipc::VfsResponse::Data(bytes) => {
            if bytes.windows(10).any(|w| w == b"f:file.txt") {
                pass("listdir /data/testdir contains f:file.txt");
            } else {
                fail("listdir /data/testdir missing f:file.txt");
            }
        }
        _ => fail("listdir /data/testdir failed"),
    }

    // Cleanup
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/data/testdir/file.txt"));
    assert_ok!(api::ipc::VfsRequest::Rmdir("/data/testdir"),
        "rmdir /data/testdir after cleanup");
}

/// 3. Access control: write to /bin/ should return PermissionDenied (Err 3).
fn test_access_control() {
    assert_err!(api::ipc::VfsRequest::Write { path: "/bin/evil", content: b"hack" },
        3, "write /bin/ returns PermissionDenied");

    // Writing to /data/ should still work
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/access_ok.txt", content: b"ok" },
        "write /data/ still works");
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/data/access_ok.txt"));
}

/// 4. Async read protocol: ReadAsync → PendingHandle → Poll → Data.
fn test_async_read() {
    // Create a file to read asynchronously.
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/async_test.txt", content: b"async content" },
        "write file for async read");

    let handle = match vfs_req(&api::ipc::VfsRequest::ReadAsync { path: "/data/async_test.txt" }) {
        api::ipc::VfsResponse::PendingHandle(h) => { pass("ReadAsync returns PendingHandle"); h }
        _ => { fail("ReadAsync did not return PendingHandle"); 0 }
    };

    if handle != 0 {
        // Poll should return data immediately (synchronous backend).
        match vfs_req(&api::ipc::VfsRequest::Poll { handle }) {
            api::ipc::VfsResponse::Data(bytes) => {
                if bytes.starts_with(b"async content") {
                    pass("Poll returns correct data");
                } else {
                    fail("Poll returned wrong data");
                }
            }
            _ => fail("Poll did not return Data"),
        }

        // Second poll on same handle → Err (consumed).
        assert_err!(api::ipc::VfsRequest::Poll { handle },
            4, "Poll stale handle returns Err");
    }

    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/data/async_test.txt"));
}

/// 5. RamFS (/tmp) operations.
fn test_ramfs() {
    assert_ok!(api::ipc::VfsRequest::Write { path: "/tmp/volatile.txt", content: b"volatile" },
        "write /tmp/volatile.txt");

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp/volatile.txt")) {
        api::ipc::VfsResponse::Stat { size: 8, is_dir: false } => pass("stat /tmp/volatile.txt size=8"),
        _ => fail("stat /tmp/volatile.txt"),
    }

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp")) {
        api::ipc::VfsResponse::Stat { is_dir: true, .. } => pass("stat /tmp is_dir=true"),
        _ => fail("stat /tmp"),
    }
}

/// 6. Stat on /data/ root directory.
fn test_stat_dir() {
    match vfs_req(&api::ipc::VfsRequest::Stat("/data")) {
        api::ipc::VfsResponse::Stat { is_dir: true, .. } => pass("stat /data is_dir=true"),
        _ => fail("stat /data should return is_dir=true"),
    }
}

/// 7. Edge cases: invalid paths, empty paths.
fn test_edge_cases() {
    // Stat nonexistent file → Err
    match vfs_req(&api::ipc::VfsRequest::Stat("/data/does_not_exist_xyz.txt")) {
        api::ipc::VfsResponse::Err(_) => pass("stat nonexistent returns Err"),
        _ => fail("stat nonexistent should Err"),
    }

    // ListDir nonexistent → empty or Err (both acceptable)
    match vfs_req(&api::ipc::VfsRequest::ListDir("/data/nonexistent_dir")) {
        api::ipc::VfsResponse::Data(b) if b.is_empty() => pass("listdir nonexistent = empty"),
        api::ipc::VfsResponse::Err(_) => pass("listdir nonexistent = Err"),
        _ => fail("listdir nonexistent unexpected response"),
    }
}

/// 8. Quota enforcement (Err 2). Only built with `test-hooks`, where the VFS
/// uses a 2 KiB quota — writing past it returns `Err(2)`, and releasing a file
/// frees the charge so a subsequent write fits again.
#[cfg(feature = "test-hooks")]
fn test_quota_limit() {
    // VFS test-hooks quota = 2048 B. Charge 800 + 800 = 1600 (both fit).
    let chunk = [b'q'; 800];
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/q1.bin", content: &chunk },
        "quota write 1 (800B) fits");
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/q2.bin", content: &chunk },
        "quota write 2 (1600B) fits");
    // Third write → 2400 > 2048 → quota exceeded.
    assert_err!(api::ipc::VfsRequest::Write { path: "/data/q3.bin", content: &chunk },
        2, "quota write 3 exceeds 2KiB limit → Err(2)");
    // Releasing q1 frees 800 B; q3 (800 B) now fits within 2048.
    assert_ok!(api::ipc::VfsRequest::Unlink("/data/q1.bin"),
        "unlink q1 releases quota");
    assert_ok!(api::ipc::VfsRequest::Write { path: "/data/q3.bin", content: &chunk },
        "quota write after release succeeds");
    // Cleanup.
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/data/q2.bin"));
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/data/q3.bin"));
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[no_mangle]
pub fn main() {
    ostd::io::println("[vfs-test] Starting VFS integration test suite...");

    test_file_lifecycle();
    test_directory_ops();
    test_access_control();
    test_async_read();
    test_ramfs();
    test_stat_dir();
    test_edge_cases();
    #[cfg(feature = "test-hooks")]
    test_quota_limit();

    let passed = PASSED.load(Ordering::Relaxed);
    let failed = FAILED.load(Ordering::Relaxed);

    ostd::io::println("");
    ostd::io::print("[vfs-test] Results: ");
    ostd::io::print_usize(passed as usize);
    ostd::io::print(" PASS, ");
    ostd::io::print_usize(failed as usize);
    ostd::io::println(" FAIL");

    if failed == 0 {
        ostd::io::println("[vfs-test] ALL TESTS PASSED");
        ostd::syscall::sys_exit(0);
    } else {
        ostd::io::println("[vfs-test] FAILURES DETECTED");
        ostd::syscall::sys_exit(1);
    }
}
