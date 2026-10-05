//! x86_64 q35 multi-port 16550 gate — phase 06.
//!
//! The kernel owns the 16550 mechanism: it registers every port the board
//! profile declares, offers only the ones that answer a register-set probe, and
//! hands a cell a port-I/O surface gated by the `serial_port` capability. This
//! lane proves the claim end to end:
//!
//! * q35 is started with three extra `isa-serial` devices, each behind its own
//!   `-chardev socket`, so the test can read what the cell transmitted and
//!   inject a byte for it to receive;
//! * `/bin/serial` enumerates the four declared ports, transmits a per-port
//!   marker, and echoes whatever arrives;
//! * the test asserts the marker arrives on a non-console port, that an injected
//!   byte comes back echoed, and that **COM1 is unchanged** — the shell prompt
//!   still appears on the console path, which is the only x86 debug channel.
//!
//! RS485 is **not** claimed here: QEMU models no DE/RE line, so direction-control
//! timing can only be measured on hardware (phase 07). The machine records
//! declare the mechanism; nothing exercises it.
//!
//! Skips gracefully when the ISO is not built or `qemu-system-x86_64` is not on
//! PATH; in CI a missing prerequisite is a hard failure (`ci_guard`).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 120;

/// The cell's summary line: four ports declared, four usable.
const CELL_READY: &str = "[serial] serial cell ready";
/// The shell reached its prompt on COM1 — the console path is intact.
const SHELL_PROMPT: &str = "Cellos >";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn iso_path() -> String {
    std::env::var("VICELL_SERIAL_ISO").unwrap_or_else(|_| {
        repo_root()
            .join("build/vicell-x86.iso")
            .to_string_lossy()
            .into_owned()
    })
}

fn prerequisites_ok(iso: &str) -> bool {
    let iso_ok = PathBuf::from(iso).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !iso_ok {
        eprintln!(
            "SKIP serial-x86: x86_64 ISO not built ({iso})\n\
             Build with: pwsh scripts/build-x86_64-cells.ps1 then the kernel + ISO"
        );
    }
    if !qemu_ok {
        eprintln!("SKIP serial-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

fn require_marker(qemu: &QemuRunner, disk: &std::path::Path, marker: &str, context: &str) {
    qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|error| {
        let _ = std::fs::remove_file(disk);
        panic!(
            "{context}: marker {marker:?} absent after {BOOT_TIMEOUT}s: {error}\n\
             --- serial output ---\n{}",
            qemu.dump()
        )
    });
}

fn make_nvme_disk() -> PathBuf {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "vicell_serial_x86_{}_{}.img",
        std::process::id(),
        CTR.fetch_add(1, Ordering::Relaxed)
    ));
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&path)
        .expect("create NVMe disk image");
    f.set_len(64 * 1024 * 1024).expect("set NVMe disk size");
    let _ = f.write_all(b"");
    path
}

/// Read from `stream` until `needle` appears or `timeout` elapses.
fn read_until(stream: &mut UnixStream, needle: &[u8], timeout: Duration) -> Vec<u8> {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    let mut buf = [0u8; 256];
    while Instant::now() < deadline {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => {
                seen.extend_from_slice(&buf[..read]);
                if seen.windows(needle.len()).any(|w| w == needle) {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    seen
}

/// Connect to a chardev socket, retrying while QEMU finishes creating it.
fn connect_socket(path: &std::path::Path, timeout: Duration) -> UnixStream {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(error) => {
                if Instant::now() >= deadline {
                    panic!("could not connect to {}: {error}", path.display());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

/// Four declared ports, a marker out on a non-console port, an injected byte
/// echoed back, and an unchanged COM1 console.
#[test]
fn serial_x86_multiport_tx_rx_and_console_unchanged() {
    let iso = iso_path();
    if !prerequisites_ok(&iso) {
        return;
    }

    let disk = make_nvme_disk();
    let dir = std::env::temp_dir().join(format!("vicell_serial_sockets_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let sockets: Vec<(u32, PathBuf)> = (1..=3)
        .map(|index| (index, dir.join(format!("com{}.sock", index + 1))))
        .collect();
    for (_, path) in sockets.iter() {
        let _ = std::fs::remove_file(path);
    }
    let socket_args: Vec<(u32, String)> = sockets
        .iter()
        .map(|(index, path)| (*index, path.to_string_lossy().into_owned()))
        .collect();
    let socket_refs: Vec<(u32, &str)> = socket_args
        .iter()
        .map(|(index, path)| (*index, path.as_str()))
        .collect();

    let qemu = QemuRunner::boot_x86_bios_with_serial_ports(
        &iso,
        &disk.to_string_lossy(),
        &socket_refs,
    );

    // Attach to the port chardevs *before* the guest transmits: QEMU's
    // `server=on,wait=off` socket discards what the guest writes while no client
    // is connected, so a late reader would see an empty port and call a working
    // transmit path broken.
    let mut com2 = connect_socket(&sockets[0].1, Duration::from_secs(30));
    com2.set_read_timeout(Some(Duration::from_secs(30)))
        .expect("set read timeout");

    // Transmit path: the cell's marker must arrive on COM2's chardev.
    let marker = read_until(&mut com2, b"marker", Duration::from_secs(30));
    assert!(
        marker.windows(13).any(|w| w == b"port 1 marker"),
        "no COM2 transmit marker reached the chardev: {:?}",
        String::from_utf8_lossy(&marker)
    );

    // Receive path: a byte written to the chardev must come back echoed by the
    // cell, which is the proof that the port is usable *from a cell* in both
    // directions rather than only for kernel console output. The cell polls for a
    // bounded window after transmitting, so this has to happen inside it.
    com2.write_all(b"Z").expect("inject a byte into COM2");
    com2.flush().expect("flush the injected byte");
    let echo = read_until(&mut com2, b"Z", Duration::from_secs(30));
    assert!(
        echo.contains(&b'Z'),
        "the injected byte was not echoed back: {:?}",
        String::from_utf8_lossy(&echo)
    );

    // The cell reports the ports it can drive, then the console proves COM1 is
    // untouched: both halves are required, so a serial cell that broke the log
    // channel cannot pass by driving the extras.
    require_marker(&qemu, &disk, CELL_READY, "the serial cell did not report");
    require_marker(
        &qemu,
        &disk,
        SHELL_PROMPT,
        "the console path was disturbed by the serial cell",
    );

    let serial = qemu.dump();
    assert!(
        serial.contains("usable=4 declared=4"),
        "not every declared port was usable:\n{serial}"
    );
    assert!(
        serial.contains("[serial] port 1 base=0x02f8 irq=3 usable"),
        "COM2 was not enumerated as usable:\n{serial}"
    );

    // Keep the guest alive briefly so an immediate panic/fault behind the echo
    // reaches the byte-at-a-time console reader.
    std::thread::sleep(Duration::from_millis(500));
    let serial = qemu.dump();
    assert!(
        serial.contains("[serial] port 1 rx byte=0x5a echoed"),
        "the cell did not report the echoed byte:\n{serial}"
    );
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    assert!(!serial.contains("[fault] Cell"), "Cell fault\n{serial}");

    let _ = std::fs::remove_file(&disk);
    for (_, path) in sockets.iter() {
        let _ = std::fs::remove_file(path);
    }
}
