# Phase 04 — Integration Test: RedoxFS /srv

**Status**: Planned
**Priority**: High
**Blocked by**: Phase 03

---

## Context Links

- [cells/apps/vfs-test/](cells/apps/vfs-test/) — existing test-cell pattern to mirror
- [scripts/build-test-hooks-ci.sh](scripts/build-test-hooks-ci.sh) — embedding mechanism
- [tests/integration/src/lib.rs](tests/integration/src/lib.rs) — `boot_rv64` pattern (lines 239–277)
- [tests/integration/tests/vfs-quota.rs](tests/integration/tests/vfs-quota.rs) — integration harness pattern
- [libs/api/src/disk.rs](libs/api/src/disk.rs) — `PART_SRV_BASE_LBA = 931_072`, `PART_SRV_SECTORS = 131_072`

---

## Overview

Three deliverables:

1. **`cells/apps/srv-test/`** — embedded test cell that exercises VFS /srv via IPC in 5 basic scenarios
2. **`scripts/mksrv-img.sh`** — builds a 519 MB sparse disk with P5 formatted as RedoxFS (P1–P4 blank)
3. **`tests/integration/tests/redoxfs-srv.rs`** — 3 integration test functions (basic, degrade, persist)

Support files: `scripts/build-srv-test-ci.sh`, `tests/integration/Cargo.toml` entry, `.github/workflows/ci.yml` job.

---

## Architecture: Why a full disk image?

`VicellDisk` in `cells/services/vfs/src/disk_redoxfs.rs` reads from absolute sector
`PART_SRV_BASE_LBA = 931_072` on the **first** VirtIO-BLK device.  The test disk must therefore
be a full-size raw image where bytes `[931_072 × 512 …)` contain a valid RedoxFS partition.
Total disk size: `(931_072 + 131_072) × 512 = 543,817,728 bytes ≈ 519 MB` (created as a sparse
file — only the RedoxFS 64 MB region contains non-zero data).

A standalone 64 MB `srv.img` as a *second* device would require `VicellDisk` to use LBA 0 — a
kernel change outside this phase's scope.

---

## Test Scenarios

| # | Scenario | Where tested |
|---|----------|-------------|
| 1 | P5 RedoxFS opens → log line confirmed | `srv-test` cell + harness |
| 2 | Write `/srv/test.txt`, read back exact content | `srv-test` cell |
| 3 | List `/srv/` returns written file | `srv-test` cell |
| 4 | `mkdir /srv/subdir` → stat reports directory | `srv-test` cell |
| 5 | Write `/srv/tmp.txt`, unlink → stat returns None | `srv-test` cell |
| 6 | Boot with no disk → `[vfs] WARNING: RedoxFS P5 open failed` in log | harness: `boot_rv64` (no disk) |
| 7 | Write in boot 1, kill QEMU, boot 2 same disk → data still present | harness: two-boot persistence |

---

## Related Code Files

| File | Action |
|------|--------|
| `cells/apps/srv-test/Cargo.toml` | Create |
| `cells/apps/srv-test/src/main.rs` | Create — 5-scenario IPC test cell |
| `cells/apps/srv-test/srv-test.ld` | Create — linker script (unique base address) |
| `Cargo.toml` (workspace) | Add `cells/apps/srv-test` to `[workspace.members]` |
| `scripts/mksrv-img.sh` | Create — generates 519 MB sparse disk with P5 RedoxFS |
| `scripts/build-srv-test-ci.sh` | Create — builds `srv-test` kernel (embedded cells) |
| `tests/integration/src/lib.rs` | Add `boot_rv64_with_disk(kernel, disk)` constructor |
| `tests/integration/tests/redoxfs-srv.rs` | Create — 3 integration test functions |
| `tests/integration/Cargo.toml` | Add `[[test]] name = "redoxfs-srv"` |
| `.github/workflows/ci.yml` | Add `redoxfs-srv` job |

---

## Implementation Steps

### Step 1 — `cells/apps/srv-test/`

**`Cargo.toml`**: clone `cells/apps/vfs-test/Cargo.toml`, rename package to `app-srv-test`,
keep same deps (`api`, `ostd`).

**`srv-test.ld`**: clone from any other cell; pick a base address not colliding with existing
cells (run `python3 scripts/check-cell-va-layout.py` after to verify).

**`src/main.rs`** — skeleton:

```rust
#![no_std]
#![no_main]
extern crate alloc;

use ostd::prelude::*;
use api::vfs::{VfsRequest, VfsResponse};
use api::service;

api::declare_manifest!(block_io = false, network = false, spawn = false);

const PASS: &str = "PASS";

fn vfs_tid() -> u64 {
    loop {
        if let Some(tid) = ostd::syscall::sys_lookup_service(service::VFS) {
            return tid;
        }
        ostd::task::yield_now();
    }
}

fn vfs_call(tid: u64, req: VfsRequest<'_>) -> VfsResponse {
    // encode req → send to tid → recv reply → decode
    // (mirror the pattern in cells/apps/vfs-test)
}

#[no_mangle]
pub fn main() {
    let tid = vfs_tid();

    // Scenario 1: mount line already printed by VFS service at startup.
    // Wait for a successful stat of /srv root (empty dir returns is_dir=true).
    assert!(matches!(vfs_call(tid, VfsRequest::Stat { path: "/srv" }),
                     VfsResponse::Stat { is_dir: true, .. }),
            "S1 FAIL: /srv not a dir");
    println!("[srv-test] S1 mount: {PASS}");

    // Scenario 2: write + read back
    vfs_call(tid, VfsRequest::Write { path: "/srv/test.txt", content: b"ViCell RedoxFS" });
    let data = match vfs_call(tid, VfsRequest::Read { path: "/srv/test.txt" }) {
        VfsResponse::Data(d) => d,
        r => panic!("S2 FAIL: unexpected {r:?}"),
    };
    assert_eq!(data.as_slice(), b"ViCell RedoxFS", "S2 FAIL: content mismatch");
    println!("[srv-test] S2 write+read: {PASS}");

    // Scenario 3: directory listing contains test.txt
    let listing = match vfs_call(tid, VfsRequest::ReadDir { path: "/srv" }) {
        VfsResponse::DirListing(l) => l,
        r => panic!("S3 FAIL: {r:?}"),
    };
    assert!(listing.iter().any(|e| e.name.as_deref() == Some("test.txt")), "S3 FAIL");
    println!("[srv-test] S3 list: {PASS}");

    // Scenario 4: mkdir
    vfs_call(tid, VfsRequest::Mkdir { path: "/srv/subdir" });
    assert!(matches!(vfs_call(tid, VfsRequest::Stat { path: "/srv/subdir" }),
                     VfsResponse::Stat { is_dir: true, .. }),
            "S4 FAIL");
    println!("[srv-test] S4 mkdir: {PASS}");

    // Scenario 5: write + unlink → stat returns NotFound
    vfs_call(tid, VfsRequest::Write { path: "/srv/tmp.txt", content: b"x" });
    vfs_call(tid, VfsRequest::Unlink { path: "/srv/tmp.txt" });
    assert!(matches!(vfs_call(tid, VfsRequest::Stat { path: "/srv/tmp.txt" }),
                     VfsResponse::NotFound | VfsResponse::Err(_)),
            "S5 FAIL: expected NotFound");
    println!("[srv-test] S5 unlink: {PASS}");

    // Persistence marker: write /srv/persist.txt so the two-boot test can verify it.
    vfs_call(tid, VfsRequest::Write { path: "/srv/persist.txt", content: b"ViCell-persist-ok" });
    println!("[srv-test] PERSIST_WRITE_DONE");

    println!("[srv-test] ALL TESTS PASSED");
}
```

Adjust the VfsRequest/VfsResponse variant names to match the actual `libs/api/src/ipc.rs`
(use the same encoding path as `cells/apps/vfs-test/src/main.rs`).

### Step 2 — `Cargo.toml` workspace

Add `"cells/apps/srv-test"` to the `[workspace.members]` array in the root `Cargo.toml`.

### Step 3 — `scripts/mksrv-img.sh`

```bash
#!/usr/bin/env bash
# mksrv-img.sh — build a 519 MB sparse disk with RedoxFS on P5 (LBA 931_072).
# Sectors P1–P4 are zero (FAT/LFS fail gracefully; cell table P2 not needed
# because the test kernel uses embedded cells).
#
# Usage: bash scripts/mksrv-img.sh [OUT_IMG]
# Output: build/disk_srv.img  (519 MB sparse file)

set -euo pipefail
OUT="${1:-build/disk_srv.img}"
PART_SRV_BASE_LBA=931072
PART_SRV_SECTORS=131072
FULL_SECTORS=$((PART_SRV_BASE_LBA + PART_SRV_SECTORS))   # 1_062_144

mkdir -p "$(dirname "$OUT")"

# ---------- Build redoxfs-ar from source (host target, std features) ----------
REDOXFS_AR="third_party/redoxfs/target/release/redoxfs-ar"
if [[ ! -x "$REDOXFS_AR" ]]; then
    echo "[mksrv-img] Building redoxfs-ar (host, --features std)..."
    cargo build \
        --manifest-path third_party/redoxfs/Cargo.toml \
        --features std --release --bin redoxfs-ar
fi

# ---------- Create staging folder with seed files --------------------------------
SEED=$(mktemp -d)
trap 'rm -rf "$SEED"' EXIT
printf 'ViCell RedoxFS' > "$SEED/hello.txt"

# ---------- Create 64 MB RedoxFS partition image ---------------------------------
PART_IMG=$(mktemp --suffix=.img)
trap 'rm -f "$PART_IMG"' EXIT
# Pre-allocate exactly PART_SRV_SECTORS sectors so create_reserved has room
dd if=/dev/zero of="$PART_IMG" bs=512 count="$PART_SRV_SECTORS" status=none
# redoxfs-ar: creates new filesystem + archives seed folder; truncates to used size.
"$REDOXFS_AR" "$PART_IMG" "$SEED"
# Restore to full partition size (redoxfs-ar truncates to fs size)
truncate -s "$((PART_SRV_SECTORS * 512))" "$PART_IMG"

# ---------- Assemble full disk image (sparse) ------------------------------------
# truncate creates a sparse file — P1–P4 sectors are all zeros
truncate -s "$((FULL_SECTORS * 512))" "$OUT"
# Splice the RedoxFS partition at P5 byte offset
dd if="$PART_IMG" of="$OUT" bs=512 seek="$PART_SRV_BASE_LBA" conv=notrunc status=none

echo "[mksrv-img] Done: $OUT ($(du -sh "$OUT" | cut -f1) on disk, $(wc -c < "$OUT") bytes total)"
```

### Step 4 — `scripts/build-srv-test-ci.sh`

Mirror `scripts/build-test-hooks-ci.sh` exactly, but:

- Build `service-vfs` WITHOUT `--features test-hooks` (full quota, full RedoxFS backend)
- Build `app-srv-test` instead of `app-vfs-test`
- Include `app-srv-test` in `mkfat32.py` call and in the `kernel_fs.img`
- Output kernel to `target/.../release/vicell-kernel-srv-test`

```bash
#!/usr/bin/env bash
set -euo pipefail

REL="target/riscv64gc-unknown-none-elf/release"
SRV_DIR="kernel/src/embedded-srv-test"

export CC_riscv64gc_unknown_none_elf="riscv64-unknown-elf-gcc"
export CFLAGS_riscv64gc_unknown_none_elf="-march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS"

echo "==> Building base cells..."
cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc \
    -p app-init -p app-shell -p service-config

echo "==> Building service-vfs (full, no test-hooks)..."
cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc \
    -p service-vfs

echo "==> Building app-srv-test..."
cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc \
    -p app-srv-test

echo "==> Assembling kernel_fs.img (srv-test)..."
mkdir -p "$SRV_DIR"
TMPDIR_KFS=$(mktemp -d)
printf 'ViCell-srv-test' > "$TMPDIR_KFS/hostname"

python3 tools/mkfat32.py \
    "$SRV_DIR/kernel_fs.img" \
    "$REL/app-init"      /bin/init \
    "$REL/app-shell"     /bin/shell \
    "$REL/service-vfs"   /bin/vfs \
    "$REL/service-config" /bin/config \
    "$REL/app-srv-test"  /bin/srv-test \
    "$TMPDIR_KFS/hostname" /etc/hostname

cp "$REL/app-init" "$SRV_DIR/init"

echo "==> Building srv-test kernel (PIC)..."
EMBEDDED_OVERRIDE="$SRV_DIR" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc \
    -p vicell-kernel

cp "$REL/vicell-kernel" "$REL/vicell-kernel-srv-test"
echo "==> Done: $REL/vicell-kernel-srv-test"
```

Note: `kernel/src/embedded-srv-test/` must be listed in `.gitignore` (same as
`embedded-test-hooks` is presumably gitignored).

### Step 5 — `tests/integration/src/lib.rs`: add `boot_rv64_with_disk`

Add after line 277 (after `boot_rv64`):

```rust
/// Boot QEMU RISC-V with a single VirtIO-BLK disk attached.
///
/// The disk is used directly (no temp copy) — callers that need isolation must
/// copy the image themselves.  This enables the persistence test to share one
/// image file across two sequential `QemuRunner` instances.
pub fn boot_rv64_with_disk(kernel: &str, disk: &str) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind serial socket");
    let port = listener.local_addr().unwrap().port();

    let child = Command::new(qemu_binary())
        .args([
            "-machine", "virt",
            "-m", "256M",
            "-nographic",
            "-bios", "default",
            "-kernel", kernel,
            "-drive", &format!("file={disk},format=raw,if=none,id=hd0"),
            "-device", "virtio-blk-device,drive=hd0",
            "-monitor", "none",
            "-serial", &format!("tcp:127.0.0.1:{port}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("qemu-system-riscv64 must be on PATH");

    listener.set_nonblocking(false).expect("blocking listener");
    let stream = listener.accept().expect("QEMU did not connect").0;
    let writer = stream.try_clone().expect("clone serial stream");

    let output = Arc::new(Mutex::new(String::new()));
    let buf = Arc::clone(&output);
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut byte = [0u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => buf.lock().unwrap().push(byte[0] as char),
            }
        }
    });

    Self { child, writer: Some(writer), output, temp_disk: None, monitor: None }
}
```

### Step 6 — `tests/integration/tests/redoxfs-srv.rs`

```rust
//! RedoxFS /srv integration tests.
//!
//! Boot 1: srv-test cell runs 5 scenarios; harness waits for "[srv-test] ALL TESTS PASSED".
//! Boot 2 (persistence): same disk, verifies /srv/persist.txt written in boot 1 is still readable.
//! No-disk boot: standard boot_rv64 (no disk), verifies graceful VFS degradation.
//!
//! Prerequisites (run scripts/build-srv-test-ci.sh first):
//!   target/riscv64gc-unknown-none-elf/release/vicell-kernel-srv-test
//!   build/disk_srv.img  (run scripts/mksrv-img.sh first)

use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..").join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn srv_test_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel-srv-test")
        .to_string_lossy().into_owned()
}

/// Standard kernel (no disk) — used by the degrade test.
fn base_kernel() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks")
        .to_string_lossy().into_owned()
}

fn srv_disk() -> String {
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy().into_owned()
}

fn prerequisites_ok(need_disk: bool) -> bool {
    let kernel = PathBuf::from(srv_test_kernel());
    let disk   = PathBuf::from(srv_disk());
    let qemu   = std::process::Command::new(qemu_binary())
        .arg("--version").output().is_ok();
    if !kernel.exists() {
        eprintln!("SKIP: srv-test kernel not found. Run scripts/build-srv-test-ci.sh.");
    }
    if need_disk && !disk.exists() {
        eprintln!("SKIP: disk_srv.img not found. Run scripts/mksrv-img.sh.");
    }
    if !qemu { eprintln!("SKIP: qemu-system-riscv64 not on PATH"); }
    kernel.exists() && (!need_disk || disk.exists()) && qemu
}

/// S1–S5: mount, write+read, list, mkdir, unlink.
#[test]
fn riscv64_redoxfs_srv_basic() {
    if !prerequisites_ok(true) { return; }

    let runner = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &srv_disk());
    runner.wait_for("[srv-test] ALL TESTS PASSED", 90)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
}

/// S6: no-disk boot → VFS degrades; /srv unavailable but no panic.
#[test]
fn riscv64_redoxfs_srv_degrade_no_disk() {
    let th_kernel = PathBuf::from(base_kernel());
    if !th_kernel.exists() {
        eprintln!("SKIP: test-hooks kernel not found. Run scripts/build-test-hooks-ci.sh.");
        return;
    }
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version").output().is_ok();
    if !qemu_ok { eprintln!("SKIP: qemu-system-riscv64 not on PATH"); return; }

    // boot_rv64 attaches NO block device — VFS must warn but not panic.
    let runner = QemuRunner::boot_rv64(&th_kernel.to_str().unwrap().to_owned());
    runner.wait_for("[vfs] WARNING: RedoxFS P5 open failed", 60)
        .unwrap_or_else(|e| {
            eprintln!("--- serial output ---\n{}\n---", runner.dump());
            panic!("{e}");
        });
}

/// S7: write in boot 1, QEMU exits, boot 2 same disk → data persists.
#[test]
fn riscv64_redoxfs_srv_persistence() {
    if !prerequisites_ok(true) { return; }

    // Use a temporary copy of the disk so the base image stays clean.
    let tmp = tempfile::Builder::new().suffix(".img").tempfile()
        .expect("create temp disk");
    std::fs::copy(srv_disk(), tmp.path()).expect("copy srv disk");
    let tmp_path = tmp.path().to_str().unwrap().to_owned();

    // Boot 1: srv-test writes /srv/persist.txt and prints PERSIST_WRITE_DONE.
    {
        let r = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &tmp_path);
        r.wait_for("[srv-test] PERSIST_WRITE_DONE", 90)
            .unwrap_or_else(|e| {
                eprintln!("--- boot-1 serial ---\n{}\n---", r.dump());
                panic!("boot 1: {e}");
            });
    }  // Drop kills QEMU; temp_disk file stays intact.

    // Boot 2: /srv/persist.txt must still exist.
    {
        let r = QemuRunner::boot_rv64_with_disk(&srv_test_kernel(), &tmp_path);
        r.wait_for("[srv-test] S2 write+read: PASS", 90)
            .unwrap_or_else(|e| {
                eprintln!("--- boot-2 serial ---\n{}\n---", r.dump());
                panic!("boot 2 (persistence): {e}");
            });
    }
}
```

Add `tempfile = "3"` to `tests/integration/Cargo.toml` under `[dev-dependencies]`.

### Step 7 — `tests/integration/Cargo.toml`

Append:

```toml
[[test]]
name = "redoxfs-srv"
path = "tests/redoxfs-srv.rs"

[dev-dependencies]
tempfile = "3"
```

### Step 8 — `.github/workflows/ci.yml`

Add job after `vfs-quota:` (around line 272):

```yaml
  redoxfs-srv:
    name: RedoxFS /srv Integration Test
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: nightly-2026-05-01
          components: rust-src

      - uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true
          key: redoxfs-srv

      - name: Install dependencies
        run: |
          sudo apt-get update -q
          sudo apt-get install -y -q \
            qemu-system-misc gcc-riscv64-unknown-elf libclang-dev

      - name: Build srv-test kernel
        run: |
          chmod +x scripts/build-srv-test-ci.sh
          bash scripts/build-srv-test-ci.sh

      - name: Build disk_srv.img
        run: |
          chmod +x scripts/mksrv-img.sh
          bash scripts/mksrv-img.sh build/disk_srv.img

      - name: Run RedoxFS /srv integration tests
        run: |
          cargo test --manifest-path tests/integration/Cargo.toml \
            --test redoxfs-srv
```

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Full 519 MB sparse disk (not standalone 64 MB) | `VicellDisk` reads from absolute LBA 931_072; changing that is a separate kernel concern |
| `redoxfs-ar` for seeding, not FUSE | `redoxfs-ar` works on all Linux CI with no kernel module; FUSE adds flakiness |
| `app-srv-test` embedded cell (not shell commands) | Shell requires P2 cell table; embedded cell works with blank P1–P4 |
| `service-vfs` built WITHOUT `test-hooks` | Full quota needed; test-hooks quota (1.1 KB) blocks multi-file /srv writes |
| Two-boot persistence via temp-copy | Base `disk_srv.img` stays pristine; temp copy shared across the two `boot_rv64_with_disk` calls |
| `boot_rv64_with_disk` does NOT auto-copy | Explicit choice: callers control isolation; persistence test intentionally shares the path |

---

## Todo

- [ ] Step 1: Create `cells/apps/srv-test/` (Cargo.toml, main.rs, srv-test.ld)
- [ ] Step 2: Add to workspace `Cargo.toml`
- [ ] Step 3: Write `scripts/mksrv-img.sh`
- [ ] Step 4: Write `scripts/build-srv-test-ci.sh`
- [ ] Step 5: Add `boot_rv64_with_disk` to `tests/integration/src/lib.rs`
- [ ] Step 6: Write `tests/integration/tests/redoxfs-srv.rs`
- [ ] Step 7: Update `tests/integration/Cargo.toml` (add entry + tempfile dep)
- [ ] Step 8: Add `redoxfs-srv` job to `.github/workflows/ci.yml`
- [ ] Verify: `bash scripts/mksrv-img.sh` produces valid sparse disk
- [ ] Verify: all 3 test functions pass locally (QEMU rv64)

---

## Success Criteria

- `riscv64_redoxfs_srv_basic` passes: S1–S5 all print PASS, kernel prints ALL TESTS PASSED
- `riscv64_redoxfs_srv_degrade_no_disk` passes: VFS warns but kernel does NOT panic
- `riscv64_redoxfs_srv_persistence` passes: data written in boot 1 is readable in boot 2
- `redoxfs-srv` CI job passes on `ubuntu-24.04` runner
- Existing `vfs-quota` CI job still passes (service-vfs with test-hooks unchanged)

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `redoxfs-ar` build fails on `--features std` (libredox/redox-scheme deps) | Those deps are `cfg(target_os = "redox")` only; Linux CI skips them |
| Blank P1 FAT32 causes VFS panic instead of graceful degrade | `FatBackend::mount` returns fallback stub on init failure — verified in Phase 02 |
| `app-srv-test` linker base collision | Run `check-cell-va-layout.py` after adding linker script |
| 519 MB disk blows CI disk quota | Sparse file — actual blocks only for 64 MB RedoxFS region; CI typically has 14 GB free |
| Persistence test is flaky if RedoxFS `tx()` doesn't flush to VirtIO before QEMU exits | RedoxFS writes commit each transaction immediately; drop(runner) waits for QEMU teardown |
