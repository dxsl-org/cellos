//! Residual N-3: a stalled virtio-blk request is bounded end to end (RV64,
//! virtio-mmio).
//!
//! The boot disk sits behind QEMU's `blkdebug` filter. Once the guest is up,
//! `qemu-io hd0 "break pwritev stall"` suspends the next write inside QEMU and
//! never resumes it, so the Driver Cell sees a published request whose used
//! entry never arrives. The lane proves the contract of
//! `cells/drivers/virtio-blk/src/io.rs`:
//!
//! * the request is answered `Indeterminate` only after the 30 s deadline
//!   (the VFS logs the indeterminate write and the shell's write fails);
//! * the device is poisoned once, the line naming its MMIO base;
//! * every later request is refused `NotIssued` at once by the driver and never
//!   reaches the virtqueue: QEMU's avail index stays where the stall left it.
//!
//! The VFS bounds a driver reply at 35 s (`BLOCK_REPLY_TICKS`) and then refuses
//! the device itself for a while, so a failed write alone proves nothing: the
//! lane also requires the poison line and that the VFS never cut the driver
//! off (`device stalled`).
//!
//! Needs the kernel and `disk_v3.img` from `scripts/gen-disk-ci.sh`.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use vicell_integration_tests::{qemu_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 60;
const CMD_TIMEOUT: u64 = 20;
const PROMPT: &str = "Cellos >";
/// `IO_BUDGET_MS` in `cells/drivers/virtio-blk/src/wait.rs`.
const DRIVER_DEADLINE: Duration = Duration::from_secs(30);
/// How long the stalled write may take to be cut off: the driver deadline,
/// the VFS's own 35 s bound behind it, and slack for a loaded host.
const STALL_TIMEOUT: u64 = 90;
/// A refused request must come back well inside one deadline.
const REFUSAL_BOUND: Duration = Duration::from_secs(10);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("disk_v3.img")
        .to_string_lossy()
        .into_owned()
}

fn prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(kernel_path()).exists();
    let disk_ok = PathBuf::from(disk_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP virtio-blk-stall: kernel not built ({})",
            kernel_path()
        );
    }
    if !disk_ok {
        eprintln!("SKIP virtio-blk-stall: disk_v3.img missing — run scripts/gen-disk-ci.sh");
    }
    if !qemu_ok {
        eprintln!("SKIP virtio-blk-stall: qemu-system-riscv64 not on PATH");
    }
    vicell_integration_tests::ci_guard(kernel_ok && disk_ok && qemu_ok)
}

fn expect(qemu: &QemuRunner, result: Result<String, String>, what: &str) {
    if let Err(error) = result {
        panic!("{what}: {error}\n--- output ---\n{}", qemu.dump());
    }
}

/// MMIO base of the virtio-mmio transport carrying the virtio-blk device, from
/// `info qtree` (a `mmio <base>/<size>` line precedes the transport's bus).
fn blk_mmio_base(qemu: &mut QemuRunner) -> u64 {
    let tree = qemu
        .human_monitor("info qtree")
        .expect("QMP monitor answers `info qtree`");
    let mut base = None;
    for line in tree.lines() {
        let line = line.trim();
        if let Some(range) = line.strip_prefix("mmio ") {
            base = range
                .split('/')
                .next()
                .and_then(|hex| u64::from_str_radix(hex, 16).ok());
        } else if line.starts_with("dev: virtio-blk-device") {
            return base.expect("virtio-blk transport has an mmio range");
        }
    }
    panic!("no virtio-blk-device in `info qtree`:\n{tree}");
}

/// `(last-avail-idx, inuse)` of the virtio-blk request queue: how many requests
/// the device has taken from the avail ring, and how many it holds unanswered.
fn queue_state(qemu: &mut QemuRunner) -> (u64, u64) {
    let reply = qemu
        .qmp(concat!(
            r#"{"execute":"x-query-virtio-queue-status","arguments":"#,
            r#"{"path":"/machine/peripheral-anon/device[0]","queue":0}}"#
        ))
        .expect("QMP x-query-virtio-queue-status");
    assert!(
        reply.contains(r#""name": "virtio-blk""#),
        "device[0] is not the virtio-blk device: {reply}"
    );
    (field(&reply, "last-avail-idx"), field(&reply, "inuse"))
}

fn field(reply: &str, key: &str) -> u64 {
    let pattern = format!("\"{key}\": ");
    let start = reply
        .find(&pattern)
        .unwrap_or_else(|| panic!("{key} missing from {reply}"))
        + pattern.len();
    reply[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a number in {reply}"))
}

#[test]
fn stalled_request_poisons_the_device_and_later_requests_are_not_issued() {
    if !prerequisites_ok() {
        return;
    }
    let mut qemu = QemuRunner::boot_with_fresh_disk_drive(&kernel_path(), &disk_path(), |path| {
        format!("driver=blkdebug,image.driver=raw,image.file.filename={path},if=none,id=hd0")
    });
    expect(&qemu, qemu.wait_for(PROMPT, BOOT_TIMEOUT), "shell prompt");
    expect(
        &qemu,
        qemu.wait_for("[virtio-blk] ready", 1),
        "virtio-blk cell ready",
    );
    std::thread::sleep(Duration::from_secs(1));
    let base = blk_mmio_base(&mut qemu);

    // Control: a write through the unbroken blkdebug filter completes.
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("echo STALL_LANE_BEFORE > /data/stall-before.txt");
    qemu.send_line("vcat /data/stall-before.txt");
    expect(
        &qemu,
        qemu.wait_for_after("STALL_LANE_BEFORE\r", checkpoint, CMD_TIMEOUT),
        "write before the stall reads back",
    );
    assert_eq!(qemu.output_count("[virtio-blk] poisoned"), 0);

    qemu.human_monitor(r#"qemu-io hd0 "break pwritev stall""#)
        .expect("QMP monitor answers qemu-io");

    let checkpoint = qemu.output_checkpoint();
    let issued = Instant::now();
    qemu.send_line("echo STALL_LANE > /data/stall.txt");
    let poison =
        format!("[virtio-blk] poisoned mmio base={base:#x}: completion deadline expired; refusing further requests");
    expect(
        &qemu,
        qemu.wait_for_after(&poison, checkpoint, STALL_TIMEOUT),
        "stalled write poisons the device",
    );
    let waited = issued.elapsed();
    assert!(
        waited >= DRIVER_DEADLINE - Duration::from_secs(1),
        "device poisoned after {waited:?}, before the {DRIVER_DEADLINE:?} deadline\n{}",
        qemu.dump()
    );
    expect(
        &qemu,
        qemu.wait_for_after("indeterminate; not replayed", checkpoint, CMD_TIMEOUT),
        "VFS receives Indeterminate for the stalled write",
    );
    expect(
        &qemu,
        qemu.wait_for_after(
            "echo: cannot write '/data/stall.txt'",
            checkpoint,
            CMD_TIMEOUT,
        ),
        "the shell reports the stalled write",
    );
    expect(
        &qemu,
        qemu.wait_for_after(PROMPT, checkpoint, CMD_TIMEOUT),
        "prompt after the stalled write",
    );

    // The stalled request stays with the device; nothing after it is published.
    let (avail_after_stall, inuse) = queue_state(&mut qemu);
    assert_eq!(inuse, 1, "the abandoned request must stay in flight");

    let checkpoint = qemu.output_checkpoint();
    let issued = Instant::now();
    qemu.send_line("echo STALL_LANE_AFTER > /data/stall-after.txt");
    expect(
        &qemu,
        qemu.wait_for_after(
            "echo: cannot write '/data/stall-after.txt'",
            checkpoint,
            CMD_TIMEOUT,
        ),
        "a write after the poison fails",
    );
    let refused = issued.elapsed();
    assert!(
        refused < REFUSAL_BOUND,
        "write after the poison took {refused:?}; it must be refused, not waited out\n{}",
        qemu.dump()
    );
    expect(
        &qemu,
        qemu.wait_for_after(PROMPT, checkpoint, CMD_TIMEOUT),
        "prompt after the refused write",
    );
    assert_eq!(
        queue_state(&mut qemu),
        (avail_after_stall, 1),
        "a request after the poison reached the virtqueue"
    );
    assert_eq!(
        qemu.output_count("[virtio-blk] poisoned"),
        1,
        "the poison line must be printed once\n{}",
        qemu.dump()
    );
    // The VFS logs every Indeterminate write and every reply it had to cut off
    // itself; neither may follow the stall, so the refused write was answered
    // by the driver, and with `NotIssued`.
    assert_eq!(
        qemu.output_count("indeterminate; not replayed"),
        1,
        "only the stalled write may be Indeterminate\n{}",
        qemu.dump()
    );
    assert!(
        !qemu.output_contains("; device stalled"),
        "the VFS cut the driver off instead of the driver answering\n{}",
        qemu.dump()
    );
}
