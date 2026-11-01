//! Phase 09C witness vocabulary: the fault ISO, shell commands, and the kernel,
//! init and VFS lines a verdict-gated block-driver replacement must produce
//! (`phase-09c-cp1.md` §7). Every wait is on a serial marker after a
//! checkpoint; offsets are byte offsets into the captured serial.

use crate::QemuRunner;
use std::time::{Duration, Instant};

pub const BOOT_TIMEOUT: u64 = 90;
pub const CMD_TIMEOUT: u64 = 20;
/// Death → verdict (≤ 200 ticks) → spawn → controller reset → IDENTIFY → 416.
pub const RESTART_TIMEOUT: u64 = 60;
pub const PROMPT: &str = "Cellos >";
pub const REGISTERED: &str = "[driver_cell] block driver registered: tid=";
const FAULT_EXIT: &str = "fault-inject: driver exiting";

/// The fault ISO (`bash scripts/x86/make-ahci-fault-iso.sh`) named by
/// `VICELL_AHCI_FAULT_ISO`; `None` (skip announced) when unset.
pub fn fault_iso() -> Option<String> {
    match std::env::var("VICELL_AHCI_FAULT_ISO") {
        Ok(iso) if !iso.is_empty() => {
            assert!(std::path::Path::new(&iso).exists(), "VICELL_AHCI_FAULT_ISO={iso} does not exist");
            Some(iso)
        }
        _ => {
            crate::skip_notice("VICELL_AHCI_FAULT_ISO unset; build: bash scripts/x86/make-ahci-fault-iso.sh");
            None
        }
    }
}

/// `pci-BB-DD-F` → the kernel's `BB:DD.F` spelling.
pub fn pci_bdf(device: &str) -> String {
    let parts: Vec<&str> = device.strip_prefix("pci-").expect("PCI device name").split('-').collect();
    assert_eq!(parts.len(), 3, "device name {device:?} is not pci-BB-DD-F");
    format!("{}:{}.{}", parts[0], parts[1], parts[2])
}

pub fn find_after(output: &str, from: usize, needle: &str) -> Option<usize> {
    output.get(from..)?.find(needle).map(|i| from + i)
}

pub fn line_at(output: &str, at: usize) -> &str {
    output[at..].lines().next().unwrap_or("")
}

pub fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let start = line.find(key)? + key.len();
    line[start..].split_whitespace().next()
}

/// One `[driver_cell] block driver registered: tid=T device=D route=R` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub at: usize,
    pub tid: usize,
    pub device: String,
    pub route: u32,
}

/// Every block-route registration in `output[from..]`.
pub fn registrations(output: &str, from: usize) -> Vec<Registration> {
    let tail = output.get(from..).unwrap_or_default();
    tail.match_indices(REGISTERED)
        .filter_map(|(i, _)| {
            let line = line_at(output, from + i);
            Some(Registration {
                at: from + i,
                tid: field(line, "tid=")?.parse().ok()?,
                device: field(line, "device=")?.to_string(),
                route: field(line, "route=")?.parse().ok()?,
            })
        })
        .collect()
}

/// Poll the serial until `probe` yields a value.
pub fn wait_until<T>(qemu: &QemuRunner, timeout: u64, what: &str, mut probe: impl FnMut(&str) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let output = qemu.dump();
        if let Some(value) = probe(&output) {
            return value;
        }
        if Instant::now() > deadline {
            panic!("timeout ({timeout}s) waiting for {what}\n--- serial ---\n{output}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub fn wait_after(qemu: &QemuRunner, marker: &str, from: usize, timeout: u64) {
    qemu.wait_for_after(marker, from, timeout)
        .unwrap_or_else(|e| panic!("marker {marker:?}: {e}\n--- serial ---\n{}", qemu.dump()));
}

/// Run one shell command; the serial it produced up to the next prompt.
pub fn run(qemu: &mut QemuRunner, command: &str) -> String {
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line(command);
    wait_after(qemu, PROMPT, checkpoint, CMD_TIMEOUT);
    qemu.dump().get(checkpoint..).unwrap_or_default().to_string()
}

pub fn write_ok(qemu: &mut QemuRunner, path: &str, text: &str) {
    let out = run(qemu, &format!("vwrite {path} {text}"));
    assert!(!out.contains("vwrite: failed"), "vwrite {path} failed:\n{out}\n--- serial ---\n{}", qemu.dump());
}

/// A write the shell must see fail; returns the command's serial.
pub fn write_fails(qemu: &mut QemuRunner, path: &str, text: &str) -> String {
    let out = run(qemu, &format!("vwrite {path} {text}"));
    assert!(out.contains("vwrite: failed to write"), "vwrite {path} did not fail:\n{out}");
    out
}

pub fn read_ok(qemu: &mut QemuRunner, path: &str, text: &str) {
    let out = run(qemu, &format!("vcat {path}"));
    assert!(out.contains(text), "vcat {path} did not return {text:?}:\n{out}\n--- serial ---\n{}", qemu.dump());
}

pub fn read_fails(qemu: &mut QemuRunner, path: &str) {
    let out = run(qemu, &format!("vcat {path}"));
    assert!(out.contains("vcat: cannot read:"), "vcat {path} unexpectedly read:\n{out}");
}

/// What `vcat path` printed: `None` for the shell's read error, otherwise the
/// printed text without the echoed command, log lines and the prompt.
pub fn observe(qemu: &mut QemuRunner, path: &str) -> Option<String> {
    let out = run(qemu, &format!("vcat {path}"));
    if out.contains("vcat: cannot read:") {
        return None;
    }
    // Guest lines may carry the console prefix `USER: `; log lines start with `[`.
    let text: Vec<String> = out
        .lines()
        .map(|line| line.trim_start().trim_start_matches("USER:").trim_start())
        .filter(|line| !line.contains("vcat ") && !line.starts_with('[') && !line.contains("Init:"))
        .map(|line| line.replace(PROMPT, "").trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    Some(text.join("\n"))
}

/// Best-effort guest shutdown, then QEMU is killed. Durability does not rest
/// on it: every FAT write is written through to the device before it returns.
pub fn shutdown(mut qemu: QemuRunner) -> String {
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line("shutdown");
    let _ = qemu.wait_for_after("System shutting down", checkpoint, CMD_TIMEOUT);
    let output = qemu.dump();
    drop(qemu);
    std::thread::sleep(Duration::from_millis(500));
    output
}

/// Wait for a single-disk lane to register its route, mount `/mnt/sd` and
/// reach the shell; with `cpus > 1` the AP must be online (E5).
pub fn wait_single_disk_boot(qemu: &QemuRunner, cpus: usize) -> Registration {
    let mut markers = vec![REGISTERED.to_string(), "[vfs] FAT32 /mnt/sd volume mounted".into(), PROMPT.into()];
    if cpus > 1 {
        markers.push(format!("[x86-smp] topology online={cpus}"));
    }
    for marker in &markers {
        qemu.wait_for(marker, BOOT_TIMEOUT)
            .unwrap_or_else(|e| panic!("boot marker {marker:?} (lane unqualified): {e}\n{}", qemu.dump()));
    }
    let regs = registrations(&qemu.dump(), 0);
    assert_eq!(regs.len(), 1, "one storage device must bind exactly one route: {regs:?}");
    regs[0].clone()
}

/// A driver death caused by a fault-marked write.
pub struct Death {
    pub at: usize,
    /// The dead driver's log tag and init service name (`ahci`, `nvme`).
    pub driver: String,
    /// The fault command's serial up to its prompt.
    pub output: String,
}

/// Send `command`, wait for the driver's fault-inject exit and the prompt.
pub fn begin_death(qemu: &mut QemuRunner, command: &str) -> Death {
    let at = qemu.output_checkpoint();
    qemu.send_line(command);
    wait_after(qemu, FAULT_EXIT, at, CMD_TIMEOUT);
    wait_after(qemu, PROMPT, at, CMD_TIMEOUT);
    let output = qemu.dump();
    let exit = find_after(&output, at, FAULT_EXIT).expect("fault exit seen");
    let line_start = output[..exit].rfind('\n').map_or(0, |i| i + 1);
    // The serial line may carry a console prefix (`USER: `) before the `[tag]`.
    let head = &output[line_start..exit];
    let tag_open = head.rfind('[').map_or(0, |i| i + 1);
    let driver = head[tag_open..].split(']').next().unwrap_or_default();
    assert!(matches!(driver, "ahci" | "nvme"), "unrecognised fault-inject tag: {}", line_at(&output, line_start));
    Death { at, driver: driver.to_string(), output: output[at..].to_string() }
}
