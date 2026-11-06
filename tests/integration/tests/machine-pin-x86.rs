//! x86_64 machine fingerprint pinning (ADR-0029 §3, plan 261031-1241 phase 03).
//!
//! Init compares the kernel's machine facts (syscall 433) with
//! `/srv/.cellos-machine/pin` before any configured service starts. Lanes:
//!
//! * `pc_pin_match_change_confirm_sequence` — one disk, four boots of the
//!   `x86_64-pc` image with VT-d: first boot pins; an unchanged reboot matches
//!   (and an in-system tamper attempt is refused); adding `-device e1000` gives
//!   a diff naming the PCI set and safe mode without the network; the console
//!   `machine pin --confirm` re-pins; the next boot matches and starts `net`.
//! * `pc_corrupt_record_enters_safe_mode` — an unreadable record is safe mode,
//!   never an automatic re-pin.
//! * `pc_without_iommu_enters_safe_mode` — the strict profile refuses DMA, so
//!   the store is unreachable: safe mode.
//! * `q35_without_a_store_warns_and_continues` — the optional q35 profile on a
//!   disk without `/srv` warns and boots normally.
//! * `q35_vtd_removal_names_the_dmar_and_vtd_fields` — pin under VT-d, boot
//!   without it: the diff names `dmar.present` and `vtd.active`.
//! * `q35_unbound_root_port_nic_is_in_the_pci_set` — a `pcnet` (no Driver
//!   Cell) added behind a PCIe root port is named in the diff exactly like a
//!   bus-0 function, while the Platform Cell still names it refused.
//!
//! Disks come from `scripts/x86/machine_pin_disk.py` (P1 FAT32 + blank P5
//! CellosFS). Serial capture maps bytes to chars one to one, so markers are
//! ASCII-only. Evidence ceiling: QEMU/TCG q35 + SeaBIOS.
//!
//! Skips gracefully when an ISO is not built or `qemu-system-x86_64` is not on
//! PATH; in CI a missing prerequisite is a hard failure (`ci_guard`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 240;
const PROMPT: &str = "Cellos >";
const FIRST_BOOT: &str = "Init: machine pin: first boot";
const MATCH: &str = "Init: machine pin: match ";
const MISMATCH: &str = "Init: machine pin: mismatch";
const SAFE_MODE: &str = "Init: SAFE MODE";
const NET_LAUNCHED: &str = "Init: net launched.";
const E1000_LAUNCHED: &str = "Init: e1000 launched.";
const E1000_ADDED: &str = "Init: machine pin: pci +8086:100e 02:00:00";
const PC_REFUSAL: &str = "[iommu] DMA isolation REQUIRED by board x86_64-pc";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

/// The `x86_64-pc` image (`VICELL_PC_ISO` overrides).
fn pc_iso() -> String {
    std::env::var("VICELL_PC_ISO").unwrap_or_else(|_| {
        repo_root().join("build/x86-pc-lane/vicell-x86-pc.iso").to_string_lossy().into_owned()
    })
}

/// The q35 image (`VICELL_X86_ISO` overrides).
fn q35_iso() -> String {
    vicell_integration_tests::x86_iso_path(&repo_root())
}

fn prerequisites_ok(iso: &str) -> bool {
    let iso_ok = PathBuf::from(iso).exists();
    let qemu_ok = std::process::Command::new(qemu_x86_binary()).arg("--version").output().is_ok();
    if !iso_ok {
        eprintln!("SKIP machine-pin-x86: x86_64 ISO not built ({iso})");
    }
    if !qemu_ok {
        eprintln!("SKIP machine-pin-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

/// A fixture disk from `scripts/x86/machine_pin_disk.py`, removed on drop.
struct Disk(PathBuf);

impl Disk {
    fn new(corrupt_pin: bool) -> Self {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "vicell_machine_pin_{}_{}.img",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));
        let mut command = std::process::Command::new("python3");
        command.arg(repo_root().join("scripts/x86/machine_pin_disk.py")).arg(&path);
        if corrupt_pin {
            command.arg("--corrupt-pin");
        }
        let status = command.status().expect("run machine_pin_disk.py");
        assert!(status.success(), "machine_pin_disk.py failed: {status}");
        Self(path)
    }

    /// A plain 64 MiB disk with no partitions, so no `/srv`.
    fn blank() -> Self {
        let path = std::env::temp_dir().join(format!("vicell_machine_pin_blank_{}.img", std::process::id()));
        let file = std::fs::File::create(&path).expect("create blank disk");
        file.set_len(64 * 1024 * 1024).expect("size blank disk");
        Self(path)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Disk {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let _ = std::fs::remove_file(PathBuf::from(format!("{}.srv-receipt.json", self.0.display())));
    }
}

fn require(qemu: &QemuRunner, marker: &str, context: &str) {
    qemu.wait_for(marker, BOOT_TIMEOUT).unwrap_or_else(|error| {
        panic!("{context}: marker {marker:?} absent: {error}\n--- serial output ---\n{}", qemu.dump())
    });
}

fn require_after(qemu: &QemuRunner, marker: &str, checkpoint: usize, context: &str) {
    qemu.wait_for_after(marker, checkpoint, BOOT_TIMEOUT).unwrap_or_else(|error| {
        panic!("{context}: marker {marker:?} absent: {error}\n--- serial output ---\n{}", qemu.dump())
    });
}

/// Run one shell command after the prompt and wait for `marker` in its output.
fn shell(qemu: &mut QemuRunner, command: &str, marker: &str, context: &str) {
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line(command);
    require_after(qemu, marker, checkpoint, context);
}

/// The 12 hex digits after `key` on the first line containing `marker`.
fn digest_after(serial: &str, marker: &str, key: &str) -> String {
    let line = serial.lines().find(|line| line.contains(marker)).expect("marker line present");
    let rest = line.split(key).nth(1).expect("key on marker line");
    let digest: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    assert_eq!(digest.len(), 12, "short digest on {line:?}");
    digest
}

/// Let the boot finish its current step, then print the boot's machine pin
/// evidence lines (the CI log is the record) and refuse a kernel panic.
fn settle(qemu: &QemuRunner) -> String {
    std::thread::sleep(std::time::Duration::from_secs(3));
    let serial = qemu.dump();
    const EVIDENCE: [&str; 8] = [
        "[x86-gate]", "Intel VT-d:", "[iommu] DMA isolation", "machine pin", "SAFE MODE", "launched.", "OS ready",
        ".cellos-machine",
    ];
    for line in serial.lines().filter(|line| EVIDENCE.iter().any(|key| line.contains(key))) {
        eprintln!("[machine-pin] | {}", line.trim_end());
    }
    assert!(!serial.contains("[KERNEL PANIC]"), "kernel panic\n{serial}");
    serial
}

fn boot(iso: &str, disk: &Disk, vtd: bool, nic: Option<&str>) -> QemuRunner {
    eprintln!("[machine-pin] boot iso={iso} vtd={vtd} nic={nic:?}");
    QemuRunner::boot_x86_bios_for_machine_pin(iso, &disk.path(), vtd, nic)
}

#[test]
fn pc_pin_match_change_confirm_sequence() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::new(false);

    // 1. First boot records the pin and boots normally.
    let pinned = {
        let mut qemu = boot(&iso, &disk, true, None);
        require(&qemu, FIRST_BOOT, "first boot did not record the pin");
        require(&qemu, "Init: OS ready.", "first boot withheld services");
        require(&qemu, PROMPT, "no shell after first boot");
        shell(&mut qemu, "machine pin", "machine pin: state=first-boot-recorded", "status after first boot");
        shell(&mut qemu, "cat /srv/.cellos-machine/pin", "cat: /srv/.cellos-machine/pin: Permission denied", "read attempt");
        shell(&mut qemu, "ls /srv/.cellos-machine", "ls: cannot access '/srv/.cellos-machine'", "list attempt");
        let serial = settle(&qemu);
        assert!(!serial.contains(SAFE_MODE), "first boot entered safe mode\n{serial}");
        digest_after(&serial, FIRST_BOOT, "recorded ")
    };

    // 2. Unchanged reboot matches; in-system tamper and confirm are refused.
    {
        let mut qemu = boot(&iso, &disk, true, None);
        require(&qemu, MATCH, "unchanged reboot did not match");
        require(&qemu, PROMPT, "no shell after the matching boot");
        shell(&mut qemu, "machine pin --confirm", "machine pin: refused NotSafeMode", "confirm outside safe mode");
        shell(&mut qemu, "rm /srv/.cellos-machine/pin", "rm: cannot remove '/srv/.cellos-machine/pin'", "tamper attempt");
        shell(&mut qemu, "machine pin", "machine pin: state=match", "status after the matching boot");
        let serial = settle(&qemu);
        assert_eq!(digest_after(&serial, MATCH, "match "), pinned, "match names another digest");
        assert!(serial.contains(NET_LAUNCHED), "the network did not start on a match\n{serial}");
        assert!(!serial.contains(SAFE_MODE), "a match entered safe mode\n{serial}");
    }

    // 3. An added e1000 is a mismatch naming the PCI set: safe mode, no network.
    {
        let mut qemu = boot(&iso, &disk, true, Some("e1000"));
        require(&qemu, MISMATCH, "the tamper attempt removed the pin, or e1000 was not detected");
        require(&qemu, E1000_ADDED, "the diff does not name the added PCI function");
        require(&qemu, SAFE_MODE, "a mismatch did not enter safe mode");
        require(&qemu, "machine pin --confirm' then reboot", "safe mode banner names the confirm procedure");
        require(&qemu, PROMPT, "no console shell in safe mode");
        shell(&mut qemu, "machine pin --confirm", "machine pin: recorded ", "console confirm");
        require(&qemu, "Init: machine pin: re-confirmed", "init did not re-pin");
        let serial = settle(&qemu);
        assert!(serial.contains("required services withheld"), "withheld list missing\n{serial}");
        assert!(!serial.contains(NET_LAUNCHED), "net started in safe mode\n{serial}");
        assert!(!serial.contains(E1000_LAUNCHED), "the NIC driver started in safe mode\n{serial}");
        assert!(!serial.contains("Init: OS ready."), "OS ready in safe mode\n{serial}");
        assert!(!serial.contains(FIRST_BOOT), "the pin was lost\n{serial}");
        assert!(!serial.contains("Init: machine pin: pci -"), "only a function was added\n{serial}");
        assert!(!serial.contains("Init: machine pin: changed "), "only the PCI set changed\n{serial}");
    }

    // 4. After confirm, the next boot with e1000 matches and starts the network.
    let qemu = boot(&iso, &disk, true, Some("e1000"));
    require(&qemu, MATCH, "the re-confirmed pin did not match");
    require(&qemu, NET_LAUNCHED, "the network did not start after re-confirmation");
    let serial = settle(&qemu);
    assert!(!serial.contains(SAFE_MODE), "safe mode after re-confirmation\n{serial}");
    assert_ne!(digest_after(&serial, MATCH, "match "), pinned, "the pin did not change");
    assert!(!serial.contains(MISMATCH), "mismatch after re-confirmation\n{serial}");
}

#[test]
fn pc_corrupt_record_enters_safe_mode() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::new(true);
    let qemu = boot(&iso, &disk, true, None);
    require(&qemu, "Init: machine pin: record unreadable", "a corrupt record was not refused");
    require(&qemu, SAFE_MODE, "a corrupt record did not enter safe mode");
    require(&qemu, PROMPT, "no console shell in safe mode");
    let serial = settle(&qemu);
    assert!(!serial.contains(FIRST_BOOT), "a corrupt record was re-pinned automatically\n{serial}");
    assert!(!serial.contains(NET_LAUNCHED), "net started in safe mode\n{serial}");
}

#[test]
fn pc_without_iommu_enters_safe_mode() {
    let iso = pc_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::new(false);
    let qemu = boot(&iso, &disk, false, None);
    require(&qemu, PC_REFUSAL, "the strict profile did not refuse untranslated DMA");
    require(&qemu, "Init: machine pin: store unavailable", "the unreachable store was not named");
    require(&qemu, SAFE_MODE, "no store under the strict profile did not enter safe mode");
    require(&qemu, PROMPT, "no console shell in safe mode");
    let serial = settle(&qemu);
    assert!(!serial.contains(NET_LAUNCHED), "net started in safe mode\n{serial}");
}

#[test]
fn q35_without_a_store_warns_and_continues() {
    let iso = q35_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::blank();
    let qemu = boot(&iso, &disk, false, None);
    require(&qemu, "Init: machine pin disabled", "q35 without a store did not warn");
    require(&qemu, "Init: OS ready.", "q35 without a store did not continue");
    require(&qemu, PROMPT, "no shell");
    let serial = settle(&qemu);
    assert!(!serial.contains(SAFE_MODE), "q35 entered safe mode without a store\n{serial}");
}

#[test]
fn q35_vtd_removal_names_the_dmar_and_vtd_fields() {
    let iso = q35_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::new(false);
    {
        let qemu = boot(&iso, &disk, true, None);
        require(&qemu, FIRST_BOOT, "q35 first boot did not pin");
        require(&qemu, PROMPT, "no shell");
        settle(&qemu);
    }
    let qemu = boot(&iso, &disk, false, None);
    require(&qemu, "Init: machine pin: changed dmar.present: 1 -> 0", "dmar.present not named");
    require(&qemu, "Init: machine pin: changed vtd.active: 1 -> 0", "vtd.active not named");
    require(&qemu, SAFE_MODE, "the VT-d change did not enter safe mode");
    let serial = settle(&qemu);
    assert!(!serial.contains(NET_LAUNCHED), "net started in safe mode\n{serial}");
}

#[test]
fn q35_unbound_root_port_nic_is_in_the_pci_set() {
    let iso = q35_iso();
    if !prerequisites_ok(&iso) {
        return;
    }
    let disk = Disk::new(false);
    let boot = |nic| {
        eprintln!("[machine-pin] boot iso={iso} vtd=true root-port nic={nic:?}");
        QemuRunner::boot_x86_bios_for_machine_pin_behind_root_port(&iso, &disk.path(), nic)
    };
    {
        let qemu = boot(None);
        require(&qemu, FIRST_BOOT, "q35 first boot did not pin");
        require(&qemu, PROMPT, "no shell");
        settle(&qemu);
    }
    let qemu = boot(Some("pcnet"));
    require(
        &qemu,
        "[platform] registration refused bus 1 device 0 function 0 id 1022:2000 class 2:0:0",
        "the Platform Cell did not name the unbound NIC",
    );
    require(&qemu, MISMATCH, "the root-port NIC did not change the pin");
    require(&qemu, "Init: machine pin: pci +1022:2000 02:00:00", "the diff does not name the unbound NIC");
    require(&qemu, SAFE_MODE, "a mismatch did not enter safe mode");
    let serial = settle(&qemu);
    assert!(!serial.contains("Init: machine pin: pci -"), "only a function was added\n{serial}");
    assert!(!serial.contains("Init: machine pin: changed "), "only the PCI set changed\n{serial}");
}
