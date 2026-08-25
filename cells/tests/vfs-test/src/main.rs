//! VFS integration test cell.
//!
//! Runs automated test scenarios against the VFS service and prints PASS/FAIL
//! for each.  Exit code 0 = all pass, 1 = at least one failure.
//!
//! Spawn with: `spawn /bin/vfs-test` from the shell.
//!
//! All paths use /tmp (RamFS) so the cell runs with or without a block device.
//! The quota tracker in dispatch.rs charges every write regardless of backend,
//! so /tmp quota tests are equivalent to /data quota tests.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]
extern crate alloc;

use core::sync::atomic::{AtomicU32, Ordering};

mod dircap;
mod grant_io;

// Declare the syscall allowlist and manifest so the kernel enforces a minimal
// capability set for this test cell.
api::declare_manifest!(block_io = false, network = false, spawn = false);
api::declare_syscalls![
    Send,
    Recv,
    Log,
    LookupService,
    OpenCap,
    CloseCap,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    GrantFree
];

/// Resolve the live VFS service tid via the service registry.
/// Spins (yield-looping) until init has registered vfs — safe because init
/// spawns vfs before vfs-test and vfs registers itself before yielding.
fn vfs_tid() -> usize {
    use api::syscall::service;
    loop {
        if let Some(tid) = ostd::syscall::sys_lookup_service(service::VFS) {
            return tid;
        }
        ostd::task::yield_now();
    }
}

static PASSED: AtomicU32 = AtomicU32::new(0);
static FAILED: AtomicU32 = AtomicU32::new(0);

// ── Test harness ─────────────────────────────────────────────────────────────

fn vfs_req(req: &api::ipc::VfsRequest<'_>) -> api::ipc::VfsResponse<'static> {
    let mut send_buf = [0u8; api::ipc::IPC_BUF_SIZE];
    // Leak the receive buffer so VfsResponse::Data borrows from it safely.
    // This is fine in a test cell that runs and exits.
    let recv_buf: &'static mut [u8; api::ipc::IPC_BUF_SIZE] =
        alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; api::ipc::IPC_BUF_SIZE]));
    ostd::ipc::service_call_typed(vfs_tid(), req, &mut send_buf, recv_buf)
        .unwrap_or(api::ipc::VfsResponse::Err(0xFD))
}

/// Send bytes to the VFS exactly as given, without encoding them first.
///
/// The only way to put a message on the wire that the request type cannot
/// express — an invalid-UTF-8 name, for instance, which Rust will not let a
/// `&str` hold but a hostile sender has no trouble producing.
fn vfs_raw(msg: &[u8]) -> api::ipc::VfsResponse<'static> {
    let service_tid = vfs_tid();
    if let ostd::syscall::SyscallResult::Err(_) = ostd::syscall::sys_send(service_tid, msg) {
        return api::ipc::VfsResponse::Err(0xFD);
    }

    // Raw requests cannot use service_call_typed, but replies still must come
    // from VFS and an unexpected sender must never be decoded as a reply.
    let buf: &'static mut [u8; api::ipc::IPC_BUF_SIZE] =
        alloc::boxed::Box::leak(alloc::boxed::Box::new([0u8; api::ipc::IPC_BUF_SIZE]));
    match ostd::syscall::sys_recv(service_tid, buf) {
        ostd::syscall::SyscallResult::Ok(sender) if sender == service_tid => {
            api::ipc::decode::<api::ipc::VfsResponse>(buf)
                .unwrap_or(api::ipc::VfsResponse::Err(0xFE))
        }
        ostd::syscall::SyscallResult::Ok(_) => api::ipc::VfsResponse::Err(0xFC),
        ostd::syscall::SyscallResult::Err(_) => api::ipc::VfsResponse::Err(0xFD),
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
                ostd::io::print("[FAIL] ");
                ostd::io::print($msg);
                ostd::io::print(" — wrong code: ");
                ostd::io::print_usize(c as usize);
                ostd::io::println("");
                FAILED.fetch_add(1, Ordering::Relaxed);
            }
            _ => fail($msg),
        }
    };
}

// ── Test scenarios ───────────────────────────────────────────────────────────

/// 1. File lifecycle: write → stat → unlink → stat-gone.
fn test_file_lifecycle() {
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/test_lifecycle.txt",
            content: b"hello world"
        },
        "write /tmp/test_lifecycle.txt"
    );

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp/test_lifecycle.txt")) {
        api::ipc::VfsResponse::Stat {
            size: 11,
            is_dir: false,
        } => pass("stat size=11 is_file"),
        _ => fail("stat after write"),
    }

    assert_ok!(
        api::ipc::VfsRequest::Unlink("/tmp/test_lifecycle.txt"),
        "unlink /tmp/test_lifecycle.txt"
    );

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp/test_lifecycle.txt")) {
        api::ipc::VfsResponse::Err(_) => pass("stat after unlink returns Err"),
        _ => fail("stat after unlink should return Err"),
    }
}

/// 2. Directory operations: mkdir, write inside, listdir, rmdir.
fn test_directory_ops() {
    assert_ok!(
        api::ipc::VfsRequest::Mkdir("/tmp/testdir"),
        "mkdir /tmp/testdir"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/testdir/file.txt",
            content: b"x"
        },
        "write inside testdir"
    );

    match vfs_req(&api::ipc::VfsRequest::ListDir("/tmp/testdir")) {
        api::ipc::VfsResponse::Data(bytes) => {
            if bytes.windows(10).any(|w| w == b"f:file.txt") {
                pass("listdir /tmp/testdir contains f:file.txt");
            } else {
                fail("listdir /tmp/testdir missing f:file.txt");
            }
        }
        _ => fail("listdir /tmp/testdir failed"),
    }

    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/testdir/file.txt"));
    assert_ok!(
        api::ipc::VfsRequest::Rmdir("/tmp/testdir"),
        "rmdir /tmp/testdir after cleanup"
    );
}

/// 3. Access control: write to /bin/ → PermissionDenied (Err 3).
fn test_access_control() {
    assert_err!(
        api::ipc::VfsRequest::Write {
            path: "/bin/evil",
            content: b"hack"
        },
        3,
        "write /bin/ returns PermissionDenied"
    );

    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/access_ok.txt",
            content: b"ok"
        },
        "write /tmp/ still works"
    );
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/access_ok.txt"));

    // ── Destructive-op authorization ─────────────────────────────────────────
    //
    // `Unlink`/`Rmdir`/`RmdirRecursive` used to skip `AccessTable` entirely: they
    // called straight into the backend, so the `/` rule (allow_write_all: false)
    // was never consulted on any delete path.
    //
    // `/readme.txt` is the decisive fixture — a real pre-seeded RamFS file under
    // the read-only `/` prefix, on a backend that DOES implement `unlink`. Before
    // the gate it was genuinely deleted (Ok); now it must be refused and survive.
    // (`/bin/` is a weaker case: BinOverlay refuses every mutating call, so a
    // delete there already failed at the backend with Err(1) — that arm proves
    // ordering, not that a deletion was ever possible.)
    assert_err!(
        api::ipc::VfsRequest::Unlink("/readme.txt"),
        3,
        "unlink /readme.txt (read-only / prefix) → PermissionDenied"
    );
    match vfs_req(&api::ipc::VfsRequest::Stat("/readme.txt")) {
        api::ipc::VfsResponse::Stat { is_dir: false, .. } => {
            pass("/readme.txt survived the refused unlink")
        }
        _ => fail("/readme.txt survived the refused unlink"),
    }

    // Non-existent paths under `/`: the point is WHICH error comes back. Err(3)
    // proves authorization ran BEFORE the backend; the pre-gate code reached the
    // backend and reported Err(1) (not found). A non-existent target keeps this
    // assertion non-destructive if it is ever run against an ungated build.
    assert_err!(
        api::ipc::VfsRequest::Rmdir("/no_such_dir"),
        3,
        "rmdir under / → PermissionDenied (authorized before backend)"
    );
    // Also proves the check precedes `collect_dir_bytes`, so an unauthorized
    // caller cannot use this op to probe subtree sizes.
    assert_err!(
        api::ipc::VfsRequest::RmdirRecursive("/no_such_dir"),
        3,
        "rmdir_recursive under / → PermissionDenied (before subtree walk)"
    );

    // Unknown and wrong-owner service file handles are intentionally the same
    // denial. The invalid grant must stay untouched because cap resolution
    // fails before grant access.
    assert_err!(
        api::ipc::VfsRequest::WriteGrant {
            cap: 0,
            offset: 0,
            grant: 0,
            bytes: 1
        },
        3,
        "grant-write: unknown handle is denied before grant access"
    );

    match grant_io::unknown_cap_read_grant_returns_zero(5, 4) {
        Ok(()) => pass("ReadGrant unknown cap returns zero bytes without faulting"),
        Err(_) => fail("ReadGrant unknown cap returns zero bytes without faulting"),
    }
    match grant_io::unknown_cap_read_grant_returns_zero(0, 0) {
        Ok(()) => pass("ReadGrant unknown cap returns zero bytes for size=0"),
        Err(_) => fail("ReadGrant unknown cap returns zero bytes for size=0"),
    }
    match grant_io::unknown_cap_read_grant_returns_zero(u64::MAX, 4) {
        Ok(()) => pass("ReadGrant unknown cap returns zero bytes for oversized offset"),
        Err(_) => fail("ReadGrant unknown cap returns zero bytes for oversized offset"),
    }
}

/// 4. Grant-backed writes: capability routing, range checks, and commit reply.
fn test_grant_write() {
    const PATH: &str = "/tmp/grant-write.txt";
    const INITIAL: &[u8] = b"prefix";
    const APPEND: &[u8] = b"grant!";
    const EXPECTED: &[u8] = b"prefixgrant!";

    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: PATH,
            content: INITIAL
        },
        "grant-write: create writable target"
    );
    let dir = match vfs_req(&api::ipc::VfsRequest::OpenRootDir { path: "/tmp" }) {
        api::ipc::VfsResponse::DirHandle(dir) => dir,
        _ => {
            fail("grant-write: open /tmp directory capability");
            return;
        }
    };
    let file = match vfs_req(&api::ipc::VfsRequest::OpenFileAt {
        dir,
        name: "grant-write.txt",
    }) {
        api::ipc::VfsResponse::FileHandle(file) => file,
        _ => {
            fail("grant-write: open service file handle");
            return;
        }
    };
    match ostd::fs::write_all(file.0, INITIAL, vfs_tid()) {
        Ok(bytes) if bytes == INITIAL.len() => {
            pass("grant-write: helper returns committed byte count")
        }
        _ => fail("grant-write: helper returns committed byte count"),
    }
    let grant = match grant_io::GrantRegion::alloc_copy_from(APPEND)
        .and_then(|grant| {
            grant.share_write_with_vfs()?;
            Ok(grant)
        }) {
        Ok(grant) => grant,
        Err(_) => {
            fail("grant-write: allocate and share source grant");
            return;
        }
    };
    match vfs_req(&api::ipc::VfsRequest::WriteGrant {
        cap: file.0,
        offset: INITIAL.len() as u64,
        grant: grant.id(),
        bytes: APPEND.len(),
    }) {
        api::ipc::VfsResponse::GrantDone { bytes } if bytes == APPEND.len() => {
            pass("grant-write: commits exact bytes before acknowledgement")
        }
        _ => fail("grant-write: commits exact bytes before acknowledgement"),
    }

    assert_err!(
        api::ipc::VfsRequest::WriteGrant {
            cap: file.0,
            offset: EXPECTED.len() as u64 + 1,
            grant: 0,
            bytes: 1,
        },
        1,
        "grant-write: invalid offset is refused before grant access"
    );
    let short_grant = match grant_io::GrantRegion::alloc_copy_from(b"bad")
        .and_then(|grant| {
            grant.share_write_with_vfs()?;
            Ok(grant)
        }) {
        Ok(grant) => grant,
        Err(_) => {
            fail("grant-write: allocate short grant");
            return;
        }
    };
    assert_err!(
        api::ipc::VfsRequest::WriteGrant {
            cap: file.0,
            offset: 0,
            grant: short_grant.id(),
            bytes: 4,
        },
        1,
        "grant-write: short grant is refused without mutation"
    );
    match grant_io::read_file_into_grant(PATH) {
        Ok((grant, bytes))
            if bytes == EXPECTED.len() && grant_io::grant_prefix_equals(&grant, EXPECTED) =>
        {
            pass("grant-write: failed writes leave committed bytes unchanged")
        }
        _ => fail("grant-write: failed writes leave committed bytes unchanged"),
    }

    let root = match vfs_req(&api::ipc::VfsRequest::OpenRootDir { path: "/" }) {
        api::ipc::VfsResponse::DirHandle(root) => root,
        _ => {
            fail("grant-write: open read-only root capability");
            return;
        }
    };
    let read_only = match vfs_req(&api::ipc::VfsRequest::OpenFileAt {
        dir: root,
        name: "readme.txt",
    }) {
        api::ipc::VfsResponse::FileHandle(file) => file,
        _ => {
            fail("grant-write: open read-only file handle");
            return;
        }
    };
    assert_err!(
        api::ipc::VfsRequest::WriteGrant {
            cap: read_only.0,
            offset: 0,
            grant: 0,
            bytes: 1,
        },
        3,
        "grant-write: write authorization precedes grant access"
    );
    let _ = vfs_req(&api::ipc::VfsRequest::CloseFile { file: read_only });
    let _ = vfs_req(&api::ipc::VfsRequest::CloseDir { dir: root });

    #[cfg(feature = "test-hooks")]
    {
        assert_err!(
            api::ipc::VfsRequest::WriteGrant {
                cap: file.0,
                offset: 0,
                grant: 0,
                bytes: 2200,
            },
            2,
            "grant-write: quota overflow is refused before grant access"
        );
        match vfs_req(&api::ipc::VfsRequest::Stat(PATH)) {
            api::ipc::VfsResponse::Stat {
                size,
                is_dir: false,
            } if size == EXPECTED.len() as u64 => {
                pass("grant-write: quota refusal leaves committed bytes unchanged")
            }
            _ => fail("grant-write: quota refusal leaves committed bytes unchanged"),
        }
    }

    let _ = vfs_req(&api::ipc::VfsRequest::CloseFile { file });
    let _ = vfs_req(&api::ipc::VfsRequest::CloseDir { dir });
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink(PATH));
}

/// 5. Async read protocol: ReadAsync → PendingHandle → Poll → Data.
fn test_async_read() {
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/async_test.txt",
            content: b"async content"
        },
        "write file for async read"
    );

    let handle = match vfs_req(&api::ipc::VfsRequest::ReadAsync {
        path: "/tmp/async_test.txt",
    }) {
        api::ipc::VfsResponse::PendingHandle(h) => {
            pass("ReadAsync returns PendingHandle");
            h
        }
        _ => {
            fail("ReadAsync did not return PendingHandle");
            0
        }
    };

    if handle != 0 {
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

        assert_err!(
            api::ipc::VfsRequest::Poll { handle },
            4,
            "Poll stale handle returns Err"
        );
    }

    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/async_test.txt"));
}

/// 6. RamFS (/tmp) volatile write and stat.
fn test_ramfs() {
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/volatile.txt",
            content: b"volatile"
        },
        "write /tmp/volatile.txt"
    );

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp/volatile.txt")) {
        api::ipc::VfsResponse::Stat {
            size: 8,
            is_dir: false,
        } => pass("stat /tmp/volatile.txt size=8"),
        _ => fail("stat /tmp/volatile.txt"),
    }

    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp")) {
        api::ipc::VfsResponse::Stat { is_dir: true, .. } => pass("stat /tmp is_dir=true"),
        _ => fail("stat /tmp"),
    }
}

/// 7. Stat on /tmp root directory.
fn test_stat_dir() {
    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp")) {
        api::ipc::VfsResponse::Stat { is_dir: true, .. } => pass("stat /tmp is_dir=true"),
        _ => fail("stat /tmp should return is_dir=true"),
    }
}

/// 8. Edge cases: nonexistent path stat and listdir.
fn test_edge_cases() {
    match vfs_req(&api::ipc::VfsRequest::Stat("/tmp/does_not_exist_xyz.txt")) {
        api::ipc::VfsResponse::Err(_) => pass("stat nonexistent returns Err"),
        _ => fail("stat nonexistent should Err"),
    }

    match vfs_req(&api::ipc::VfsRequest::ListDir("/tmp/nonexistent_dir")) {
        api::ipc::VfsResponse::Data([]) => pass("listdir nonexistent = empty"),
        api::ipc::VfsResponse::Err(_) => pass("listdir nonexistent = Err"),
        _ => fail("listdir nonexistent unexpected response"),
    }
}

/// 9. Quota enforcement (Err 2). Only built with `test-hooks`, where the VFS
/// uses a 1.1 KiB quota.  All paths use /tmp (RamFS) so no block device is needed.
/// The QuotaTracker in dispatch.rs charges every successful write regardless of
/// which backend path is used.
///
/// Chunk size is 400 B so the encoded VfsRequest::Write fits inside the 512 B
/// IPC buffer (≈415 B on the wire: 1+1+11+2+400).
///
/// NOTE: test_ramfs writes 8 bytes (/tmp/volatile.txt) without cleanup.
/// Total before this test: 8 B (from test_ramfs).
/// 8 + 400 = 408 ≤ 1100  ✓
/// 408 + 400 = 808 ≤ 1100  ✓
/// 808 + 400 = 1208 > 1100  → Err(2) ✓
#[cfg(feature = "test-hooks")]
fn test_quota_limit() {
    let chunk = [b'q'; 400];
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/q1.bin",
            content: &chunk
        },
        "quota write 1 (400B) fits"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/q2.bin",
            content: &chunk
        },
        "quota write 2 (800B total) fits"
    );
    // Third write → 1208 total > 1100 → quota exceeded.
    assert_err!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/q3.bin",
            content: &chunk
        },
        2,
        "quota write 3 exceeds 1.1KiB limit → Err(2)"
    );
    // Releasing q1 frees 800B; q3 (800B) now fits.
    assert_ok!(
        api::ipc::VfsRequest::Unlink("/tmp/q1.bin"),
        "unlink q1 releases quota"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/q3.bin",
            content: &chunk
        },
        "quota write after release succeeds"
    );
    // Cleanup: net 0 delta (q2+q3=1600B remain, but test_rmdir starts fresh).
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/q2.bin"));
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/q3.bin"));
}

/// 10. RmdirRecursive releases quota (test-hooks only, 1.1 KiB quota).
/// After test_quota_limit cleanup: 8 B used (volatile.txt).
/// 8 + 400 = 408 ≤ 1100  ✓
/// 408 + 400 = 808 ≤ 1100  ✓
/// 808 + 300 = 1108 > 1100  → Err(2) ✓   (overflow check before delete)
/// RmdirRecursive → releases 800B → back to 8B
/// 8 + 300 = 308 ≤ 1100  ✓   (write succeeds after delete)
#[cfg(feature = "test-hooks")]
fn test_rmdir_recursive_quota() {
    let chunk = [b'r'; 400];
    assert_ok!(
        api::ipc::VfsRequest::Mkdir("/tmp/rdir_q"),
        "rdir-quota: mkdir"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/rdir_q/a.bin",
            content: &chunk
        },
        "rdir-quota: write a.bin (400B)"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/rdir_q/b.bin",
            content: &chunk
        },
        "rdir-quota: write b.bin (800B total)"
    );
    let small = [b'x'; 300];
    assert_err!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/rdir_quota_overflow.bin",
            content: &small
        },
        2,
        "rdir-quota: overflow write correctly blocked before delete"
    );
    assert_ok!(
        api::ipc::VfsRequest::RmdirRecursive("/tmp/rdir_q"),
        "rdir-quota: RmdirRecursive releases 800B"
    );
    assert_ok!(
        api::ipc::VfsRequest::Write {
            path: "/tmp/rdir_quota_ok.bin",
            content: &small
        },
        "rdir-quota: write after recursive delete succeeds (quota freed)"
    );
    let _ = vfs_req(&api::ipc::VfsRequest::Unlink("/tmp/rdir_quota_ok.bin"));
}

// ── Entry point ──────────────────────────────────────────────────────────────

ostd::cell_main!(cell_main);

fn cell_main() {
    ostd::io::println("[vfs-test] Starting VFS integration test suite...");

    test_file_lifecycle();
    test_directory_ops();
    test_access_control();
    test_grant_write();
    test_async_read();
    test_ramfs();
    test_stat_dir();
    test_edge_cases();
    #[cfg(feature = "test-hooks")]
    test_quota_limit();
    #[cfg(feature = "test-hooks")]
    test_rmdir_recursive_quota();
    // Last: it ends by giving up path strings for good, so nothing above it
    // could run afterwards.
    dircap::run();

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
