//! Phase 18 two-NIC witness on x86_64 q35 (plan 261110-1703, phase 18, census
//! `p18-census-drivers.md` §5.3).
//!
//! Two runs, each with two QEMU NICs on their own restricted user netdevs at
//! fixed slots 00:03.0 and 00:04.0: different models (`e1000e` + `igb`) and
//! the same model (two `e1000e`, one `/bin/e1000e` instance per function,
//! which the boot config's duplicate `path` allows). Both netdevs keep SLIRP's
//! default 10.0.2.0/24, so after a failover the promoted NIC still reaches the
//! DNS server (10.0.2.3) the net service resolves through. Each run proves:
//!
//! 1. **Both bound on their own routes.** Two `[driver_cell] NIC driver
//!    registered` lines, two tids, two devices, two routes; exactly one route
//!    reports itself secondary, so the other is the primary.
//! 2. **The primary carries traffic.** SLIRP DHCP completes (`[net] IP
//!    address:`) through the primary.
//! 3. **Killing the primary leaves the other bound.** `nc F18-DIE-<device> 53`
//!    makes the net service transmit a DNS query naming the primary's function;
//!    its fault-injecting driver exits 1 without quiescing (the image is
//!    `scripts/x86/make-nic2-iso.sh`). The kernel quiesces the function,
//!    re-points `service::NIC_DRIVER` to the other route's live provider
//!    (`NIC route N promoted to primary`, kernel `0e5a6deb1b`), and init
//!    restarts the dead instance (`restart = "on-failure"`), which registers on
//!    its old route as a secondary. The other route is never re-registered and
//!    its provider never dies.
//! 4. **The promoted route carries traffic, and the roles reverse.** A second
//!    `nc` names the promoted device: only a frame the net service sent through
//!    the promoted driver can carry it, so its death proves the net service
//!    followed `NIC_DRIVER`. The restarted first driver is promoted in turn,
//!    and the second driver's replacement registers as a secondary.
//!
//! Hygiene: no kernel panic, Cell fault or crash, restart storm, or
//! non-quiescent function.
//!
//! The marker names one device, so a DNS retransmit leaving through the other
//! NIC after a failover does not kill that one too.
//!
//! Skips when `VICELL_NIC2_ISO` is unset (build: `bash
//! scripts/x86/make-nic2-iso.sh`) or `qemu-system-x86_64` is not on PATH.

use std::time::{Duration, Instant};
use vicell_integration_tests::{qemu_x86_binary, QemuRunner};

const BOOT_TIMEOUT: u64 = 120;
const KILL_TIMEOUT: u64 = 60;
const REGISTERED: &str = "[driver_cell] NIC driver registered: ";

/// One NIC: QEMU model, slot and what its driver prints when it binds.
struct Nic {
    model: &'static str,
    slot: u8,
    bind: &'static str,
}

const E1000E_BIND: &str = "[e1000e] controller bound 8086:10d3 82574L";
const IGB_BIND: &str = "[igb] controller bound 8086:10c9";

impl Nic {
    fn device(&self) -> String {
        format!("pci-00-{:02x}-0", self.slot)
    }
}

fn iso_path() -> Option<String> {
    std::env::var("VICELL_NIC2_ISO").ok()
}

fn prerequisites_ok() -> bool {
    let iso_ok = iso_path().is_some_and(|path| std::path::Path::new(&path).exists());
    if !iso_ok {
        eprintln!(
            "SKIP nic2-x86: VICELL_NIC2_ISO unset or missing \
             (build: bash scripts/x86/make-nic2-iso.sh)"
        );
    }
    let qemu_ok = std::process::Command::new(qemu_x86_binary())
        .arg("--version")
        .output()
        .is_ok();
    if !qemu_ok {
        eprintln!("SKIP nic2-x86: qemu-system-x86_64 not on PATH");
    }
    vicell_integration_tests::ci_guard(iso_ok && qemu_ok)
}

/// One `NIC driver registered` line.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Registration {
    tid: usize,
    device: String,
    route: usize,
}

fn registrations(serial: &str) -> Vec<Registration> {
    serial
        .lines()
        .filter_map(|line| line.split(REGISTERED).nth(1))
        .map(|rest| {
            let field = |key: &str| {
                rest.split_whitespace()
                    .find_map(|word| word.strip_prefix(key))
                    .unwrap_or_else(|| panic!("{key} missing in {rest:?}"))
                    .to_string()
            };
            Registration {
                tid: field("tid=").parse().expect("tid"),
                device: field("device="),
                route: field("route=").parse().expect("route"),
            }
        })
        .collect()
}

fn secondary_line(route: usize) -> String {
    format!("[driver_cell] NIC route {route} is secondary")
}

fn promoted_line(to: &Registration) -> String {
    format!(
        "[driver_cell] NIC route {} promoted to primary: tid={} device={}",
        to.route, to.tid, to.device
    )
}

fn fault_line(device: &str) -> String {
    format!("fault-inject: F18-DIE-{device} transmitted, driver exiting")
}

/// `[pcie] quiesced 00:0X.0 for tid T:` for the function at `slot`.
fn quiesced_line(slot: u8, tid: usize) -> String {
    format!("[pcie] quiesced 00:{slot:02x}.0 for tid {tid}: ")
}

fn fail(qemu: &QemuRunner, what: &str) -> ! {
    panic!("{what}\n--- serial output ---\n{}", qemu.dump())
}

fn expect(qemu: &QemuRunner, marker: &str, timeout: u64, what: &str) {
    qemu.wait_for(marker, timeout)
        .unwrap_or_else(|e| fail(qemu, &format!("{what}: {e}")));
}

fn expect_after(qemu: &QemuRunner, marker: &str, checkpoint: usize, timeout: u64, what: &str) {
    qemu.wait_for_after(marker, checkpoint, timeout)
        .unwrap_or_else(|e| fail(qemu, &format!("{what}: {e}")));
}

/// Registrations after `checkpoint` until one names `device`.
fn wait_registration(
    qemu: &QemuRunner,
    checkpoint: usize,
    device: &str,
    timeout: u64,
) -> Registration {
    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let serial = qemu.dump();
        if let Some(found) = registrations(serial.get(checkpoint..).unwrap_or_default())
            .into_iter()
            .find(|registration| registration.device == device)
        {
            return found;
        }
        if Instant::now() > deadline {
            fail(
                qemu,
                &format!("no new registration of {device} in {timeout}s"),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Kill the primary `victim` through a DNS frame naming it, and check the
/// failover to `survivor`. Returns the victim's replacement.
fn kill_primary(
    qemu: &mut QemuRunner,
    label: &str,
    victim: &Registration,
    victim_slot: u8,
    survivor: &Registration,
) -> Registration {
    // The shell is idle at its prompt before the command is typed.
    std::thread::sleep(Duration::from_millis(500));
    let checkpoint = qemu.output_checkpoint();
    qemu.send_line(&format!("nc F18-DIE-{} 53", victim.device));
    expect_after(
        qemu,
        &fault_line(&victim.device),
        checkpoint,
        KILL_TIMEOUT,
        &format!(
            "{label}: the primary {} did not transmit the fault frame",
            victim.device
        ),
    );
    expect_after(
        qemu,
        &quiesced_line(victim_slot, victim.tid),
        checkpoint,
        KILL_TIMEOUT,
        &format!("{label}: the dead driver's function was not quiesced"),
    );
    expect_after(
        qemu,
        &promoted_line(survivor),
        checkpoint,
        KILL_TIMEOUT,
        &format!("{label}: service::NIC_DRIVER did not move to the surviving route"),
    );
    expect_after(
        qemu,
        "Init: service restarted.",
        checkpoint,
        KILL_TIMEOUT,
        &format!("{label}: init did not restart the dead driver"),
    );
    let replacement = wait_registration(qemu, checkpoint, &victim.device, KILL_TIMEOUT);
    expect_after(
        qemu,
        &secondary_line(victim.route),
        checkpoint,
        KILL_TIMEOUT,
        &format!("{label}: the replacement did not register as a secondary"),
    );
    // The shell answers again: nc gave up on the name, and the next command can
    // be typed into an idle prompt.
    expect_after(
        qemu,
        "Cellos >",
        checkpoint,
        KILL_TIMEOUT,
        &format!("{label}: shell"),
    );

    let serial = qemu.dump();
    let fresh = &serial[checkpoint..];
    assert_eq!(
        replacement.route, victim.route,
        "{label}: the replacement took another route\n{serial}"
    );
    assert_ne!(
        replacement.tid, victim.tid,
        "{label}: no new incarnation\n{serial}"
    );
    // The other route stays bound: never re-registered, its provider never
    // quiesced, and nothing refused or replaced it.
    let rebound: Vec<_> = registrations(fresh)
        .into_iter()
        .filter(|registration| registration.device == survivor.device)
        .collect();
    assert!(
        rebound.is_empty(),
        "{label}: the surviving route was re-registered: {rebound:?}\n{serial}"
    );
    assert!(
        !fresh.contains(&format!("for tid {}: ", survivor.tid)),
        "{label}: the surviving driver was torn down\n{serial}"
    );
    assert!(
        !fresh.contains(&fault_line(&survivor.device)),
        "{label}: the surviving driver was killed too\n{serial}"
    );
    assert_eq!(
        fresh.matches(REGISTERED).count(),
        1,
        "{label}: only the replacement registers\n{serial}"
    );
    for line in [
        fault_line(&victim.device),
        quiesced_line(victim_slot, victim.tid),
        promoted_line(survivor),
        format!("{REGISTERED}tid={}", replacement.tid),
        secondary_line(victim.route),
    ] {
        let at = fresh.find(&line).expect("line seen above");
        eprintln!(
            "[nic2-x86 {label}] {}",
            fresh[at..].lines().next().unwrap_or_default().trim_end()
        );
    }
    replacement
}

fn run(label: &str, nics: [Nic; 2]) {
    if !prerequisites_ok() {
        return;
    }
    let iso = iso_path().expect("checked above");
    let mut devices = Vec::new();
    for (index, nic) in nics.iter().enumerate() {
        devices.extend([
            "-netdev".to_string(),
            format!("user,id=n{index},restrict=on"),
            "-device".to_string(),
            format!(
                "{},netdev=n{index},addr={:#x},mac=52:54:00:18:00:{:02x}",
                nic.model,
                nic.slot,
                index + 1
            ),
        ]);
    }
    let mut qemu = QemuRunner::boot_x86_q35(&iso, 1, &devices);

    // 1. Both NICs bound and registered on their own routes.
    expect(&qemu, "Cellos >", BOOT_TIMEOUT, "shell");
    let [first, second] = &nics;
    let bound = [
        wait_registration(&qemu, 0, &first.device(), BOOT_TIMEOUT),
        wait_registration(&qemu, 0, &second.device(), BOOT_TIMEOUT),
    ];
    // 2. The primary carries traffic.
    expect(
        &qemu,
        "[net] IP address:",
        BOOT_TIMEOUT,
        "SLIRP DHCP through the primary",
    );
    std::thread::sleep(Duration::from_millis(500));

    let serial = qemu.dump();
    let all = registrations(&serial);
    assert_eq!(all.len(), 2, "{label}: exactly two registrations\n{serial}");
    assert_ne!(
        bound[0].tid, bound[1].tid,
        "{label}: one driver per NIC\n{serial}"
    );
    assert_ne!(
        bound[0].route, bound[1].route,
        "{label}: one route per NIC\n{serial}"
    );
    for nic in &nics {
        let expected = nics.iter().filter(|other| other.bind == nic.bind).count();
        assert_eq!(
            serial.matches(nic.bind).count(),
            expected,
            "{label}: {:?} seen a wrong number of times\n{serial}",
            nic.bind
        );
    }
    let secondary: Vec<usize> = (0..2)
        .filter(|&index| serial.contains(&secondary_line(bound[index].route)))
        .collect();
    assert_eq!(
        secondary.len(),
        1,
        "{label}: exactly one route must be secondary\n{serial}"
    );
    let (p, s) = if secondary[0] == 0 { (1, 0) } else { (0, 1) };
    let (primary, other) = (bound[p].clone(), bound[s].clone());
    eprintln!("[nic2-x86 {label}] primary {primary:?}, secondary {other:?}");
    for line in [
        format!("{REGISTERED}tid={}", bound[0].tid),
        format!("{REGISTERED}tid={}", bound[1].tid),
        secondary_line(other.route),
        "[net] IP address:".to_string(),
    ] {
        let at = serial.find(&line).expect("line seen above");
        eprintln!(
            "[nic2-x86 {label}] {}",
            serial[at..].lines().next().unwrap_or_default().trim_end()
        );
    }

    // 3. Kill the primary: the secondary takes NIC_DRIVER and stays bound.
    let replacement = kill_primary(&mut qemu, label, &primary, nics[p].slot, &other);
    // 4. Kill the promoted driver: only the net service's traffic through it
    // can carry its marker. The first driver's replacement takes over.
    kill_primary(&mut qemu, label, &other, nics[s].slot, &replacement);

    let serial = qemu.dump();
    for forbidden in [
        "[KERNEL PANIC]",
        "PANIC: Application crashed!",
        "[fault] Cell",
        "restart storm",
        "not quiescent",
        "driver gate closed",
        "held by live tid",
        "not published",
    ] {
        assert!(
            !serial.contains(forbidden),
            "{label}: {forbidden:?} in the serial log\n{serial}"
        );
    }
}

#[test]
fn nic2_x86_e1000e_and_igb() {
    run(
        "e1000e+igb",
        [
            Nic {
                model: "e1000e",
                slot: 3,
                bind: E1000E_BIND,
            },
            Nic {
                model: "igb",
                slot: 4,
                bind: IGB_BIND,
            },
        ],
    );
}

#[test]
fn nic2_x86_two_e1000e() {
    run(
        "e1000e+e1000e",
        [
            Nic {
                model: "e1000e",
                slot: 3,
                bind: E1000E_BIND,
            },
            Nic {
                model: "e1000e",
                slot: 4,
                bind: E1000E_BIND,
            },
        ],
    );
}
