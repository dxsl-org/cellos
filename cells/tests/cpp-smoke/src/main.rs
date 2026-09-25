//! `cpp-freestanding` runtime profile smoke cell.
//!
//! Proves the profile end to end in a Tier 2 paged domain:
//!   1. the cell is admitted as an `FFI`-class Tier 2 cell (kernel marker),
//!   2. C++ static constructors ran (crt0 walks `__init_array`),
//!   3. virtual dispatch, templates, and `new`/`delete` work,
//!   4. file I/O works both ways: the VFS service over typed IPC and the shim's
//!      C `open`/`read`/`close` ABI through the kernel file table.
//!
//! Markers (integration-test contract):
//!   `[cpp-smoke] static-ctor marker=0xC0FFEE11`
//!   `[cpp-smoke] virtual-dispatch area=37`
//!   `[cpp-smoke] templates total=64`
//!   `[cpp-smoke] heap churn checksum=…`
//!   `[cpp-smoke] vfs client roundtrip bytes=…`
//!   `[cpp-smoke] c-abi read magic=ELF`
//!   `CPP-SMOKE: PASS`

#![no_std]
#![no_main]

extern crate alloc;
extern crate ostd;

mod cpp;

use alloc::format;
use ostd::clients::VfsClient;
use ostd::io::println;
use ostd::syscall::sys_exit;

api::declare_manifest!(
    block_io = false,
    network = false,
    spawn = false,
    tier = api::manifest::PROTECTION_CLASS_FFI
);

api::declare_syscalls![
    Send,
    Recv,
    LookupService,
    Open,
    Read,
    Close,
    VfsMutate,
    Log,
    Exit
];

ostd::cell_main!(cell_main);

const STATIC_CTOR_SENTINEL: u32 = 0xC0FFEE11;
const VFS_PATH: &str = "/srv/cpp-smoke.txt";
/// `/BIN/INIT` is the embedded init ELF: the kernel file table resolves it, and
/// the shim's C `open`/`read` ABI goes through that table (not the VFS service).
const BIN_INIT_NUL: &[u8] = b"/BIN/INIT\0";
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const PAYLOAD: &[u8] = b"cpp-freestanding payload read through the VFS service";
const HEAP_ITERATIONS: i32 = 64;

fn fail(stage: &str) -> ! {
    println(&format!("[cpp-smoke] FAIL stage={}", stage));
    sys_exit(1)
}

fn expect_eq(stage: &str, actual: i64, expected: i64) {
    if actual != expected {
        println(&format!(
            "[cpp-smoke] mismatch stage={} actual={} expected={}",
            stage, actual, expected
        ));
        fail(stage);
    }
}

/// The same checksum the C++ side computes, implemented independently here: a
/// wrong `operator new`/`delete` (aliasing, truncation, double free) shows up as
/// a mismatch rather than as a lucky pass.
fn expected_heap_checksum(iterations: i32) -> i32 {
    let mut checksum: i32 = 0;
    for i in 0..iterations {
        for j in 0..16u32 {
            checksum += ((i as u32 + j) % 7) as i32;
        }
    }
    checksum
}

fn cell_main() {
    println("[cpp-smoke] start (Tier 2 FFI cell, cpp-freestanding profile)");

    // 1. Static construction: the sentinel is written by a global constructor in
    //    engine.cpp, so a non-zero value proves crt0 ran `__init_array`.
    let ctor = cpp::static_ctor_marker();
    expect_eq("static-ctor", ctor as i64, STATIC_CTOR_SENTINEL as i64);
    println(&format!("[cpp-smoke] static-ctor marker=0x{:08X}", ctor));

    // Static destructors are a documented non-feature (atexit is a stub): the
    // cell is reclaimed by the kernel, so nothing runs them.
    expect_eq("static-dtor", cpp::static_dtor_marker() as i64, 0);

    // 2. Virtual dispatch through a base pointer, then through a virtual
    //    destructor on a heap object.
    let area = cpp::virtual_dispatch_total();
    expect_eq("virtual-dispatch", area as i64, 37);
    println(&format!("[cpp-smoke] virtual-dispatch area={}", area));

    let deleted_area = cpp::virtual_delete_area();
    expect_eq("virtual-delete", deleted_area as i64, 36);
    println(&format!("[cpp-smoke] virtual-delete area={}", deleted_area));

    // 3. Templates: tmax/tmin plus two Stack instantiations and twice<i32>.
    let total = cpp::template_total();
    expect_eq("templates", total as i64, 64);
    println(&format!("[cpp-smoke] templates total={}", total));

    // 4. Allocator: operator new/delete over the shim allocator.
    let checksum = cpp::heap_churn(HEAP_ITERATIONS);
    expect_eq(
        "heap-churn",
        checksum as i64,
        expected_heap_checksum(HEAP_ITERATIONS) as i64,
    );
    println(&format!(
        "[cpp-smoke] heap churn checksum={} iterations={}",
        checksum, HEAP_ITERATIONS
    ));

    // 5. File I/O, two paths that are genuinely different in Cellos:
    //    (a) the VFS service over typed IPC (the `/srv` CellosFS volume), and
    //    (b) the shim's C `open`/`read`/`close` ABI, which resolves through the
    //        kernel file table (`/BIN/INIT` is the embedded init ELF).
    let mut vfs = VfsClient::new();
    if vfs.write_file(VFS_PATH, PAYLOAD).is_err() {
        fail("vfs-write");
    }
    let roundtrip = match vfs.read_file(VFS_PATH) {
        Ok(bytes) => bytes,
        Err(_) => fail("vfs-read"),
    };
    if roundtrip != PAYLOAD {
        fail("vfs-content");
    }
    println(&format!(
        "[cpp-smoke] vfs client roundtrip bytes={}",
        roundtrip.len()
    ));

    let mut magic = [0u8; 4];
    let read = cpp::read_file(BIN_INIT_NUL, &mut magic);
    expect_eq("c-abi-read", read as i64, 4);
    if magic != ELF_MAGIC {
        fail("c-abi-content");
    }
    println("[cpp-smoke] c-abi read magic=ELF");

    println("CPP-SMOKE: PASS");
    sys_exit(0);
}
