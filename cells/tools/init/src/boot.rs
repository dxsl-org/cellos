use api::syscall::service;
use ostd::syscall::{
    sys_lookup_service, sys_notify_on_exit, sys_register_service, sys_spawn_from_path,
    SyscallResult,
};

// ── Phase-03 step-5 Tier-2 grant pair (opt-in `tier2-grant-pair`) ───────────
//
// The AArch64 test image has no interactive window: the shell's first prompt
// arrives long after the image's own test root exits the VM, so the pair cannot be
// typed at a shell the way the RV64 lane drives it (`scripts/qemu-native-domain-test.sh`
// case `grant-pair`). It is launched from the boot order instead — the same place
// the phase-02 fixtures come from — and driven to completion by this cell.
//
// Both cells run on reviewed init-launch edges. A brand-new `/bin/<name>` would
// need a kernel launch-profile row (`kernel/src/loader/launch_profile/targets.rs`)
// and a boot-ceiling row, and this lane owns neither file. `/bin/silo` and
// `/bin/net-broker` are reviewed init edges whose services are *not built into
// this image* (`development-silo-provider` and `c2c-broker` off), so nothing else
// is launched there — and the two features are refused together with ours below,
// because then init would launch the real service at the pair's path. Every marker
// the pair emits is namespaced `S22-<arch>-GRANT-PAIR-*` (`S22-AARCH64-…` on this
// image), so a reader can never mistake it for the real service's output or for
// the kernel-side `S22-AARCH64-GRANT-*` fixtures.
#[cfg(all(feature = "tier2-grant-pair", feature = "development-silo-provider"))]
compile_error!("tier2-grant-pair launches the pair at /bin/silo, which this profile also launches");
#[cfg(all(feature = "tier2-grant-pair", feature = "c2c-broker"))]
compile_error!(
    "tier2-grant-pair launches the pair at /bin/net-broker, which c2c-broker also launches"
);

/// The pair's owner runs on the reviewed `/bin/silo` init edge.
#[cfg(feature = "tier2-grant-pair")]
const GRANT_PAIR_OWNER_PATH: &str = "/bin/silo";
/// The pair's receiver runs on the reviewed `/bin/net-broker` init edge.
#[cfg(feature = "tier2-grant-pair")]
const GRANT_PAIR_RECEIVER_PATH: &str = "/bin/net-broker";
/// One receiver generation per property the phase must witness, in the order the
/// owner serves them: ReadWrite, ReadOnly, same-recipient downgrade,
/// GrantUnregister, owner exit.
#[cfg(feature = "tier2-grant-pair")]
const GRANT_PAIR_MODES: [&str; 5] = ["rw", "ro", "downgrade", "unregister", "exit"];
/// Bounded wait for a generation's terminal fault: 300 scheduler ticks (10 ms
/// slices, the clock `service_table::now_ticks` documents) is ~3 s, two orders of
/// magnitude above the ~200 ms one generation costs, and the bound keeps a
/// generation that neither faults nor exits from hanging the boot with no
/// diagnosis. The yield count is a backstop for a tick clock that never advances.
#[cfg(feature = "tier2-grant-pair")]
const GRANT_PAIR_DEATH_TIMEOUT_TICKS: u64 = 300;
#[cfg(feature = "tier2-grant-pair")]
const GRANT_PAIR_DEATH_POLL_LIMIT: usize = 5_000_000;

#[cfg(feature = "tier2-grant-pair")]
fn path_task_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Has `tid` terminated? A task the listing no longer carries has.
#[cfg(feature = "tier2-grant-pair")]
fn task_terminated(tid: usize) -> bool {
    let mut rows = [api::syscall::ProcessInfo::default(); 64];
    match ostd::syscall::sys_get_procs(&mut rows) {
        Ok(count) => rows
            .iter()
            .take(count)
            .all(|row| row.id != tid || row.state == 3),
        // A listing that cannot be taken is not evidence of death.
        Err(_) => false,
    }
}

/// Launch and drive the phase-03 grant pair from the boot order.
///
/// The owner is started once, with the receiver's task name as its command line so
/// it can recognize the generations that will ask it for grants. Each generation is
/// then started with its own mode and the owner's task name. The grant ids never
/// appear on a command line: they are kernel-assigned after the owner starts, so the
/// receiver asks the owner for them over the pair's own IPC protocol — the in-band
/// handoff the interactive lane replaces by reading the owner's `HANDOFF` line.
///
/// Each generation ends in the deliberate store fault it exists to witness, so the
/// next one may only start once this one is gone: the phases share grant ids, and
/// the fault address is what attributes each fault to its phase.
#[cfg(feature = "tier2-grant-pair")]
pub(crate) fn run_grant_pair() {
    let owner_name = path_task_name(GRANT_PAIR_OWNER_PATH);
    let receiver_name = path_task_name(GRANT_PAIR_RECEIVER_PATH);

    if !ostd::syscall::sys_set_spawn_args(&alloc::format!("peer={receiver_name}")) {
        ostd::io::println("Init: tier2-grant-pair owner command line refused.");
        return;
    }
    match sys_spawn_from_path(GRANT_PAIR_OWNER_PATH) {
        SyscallResult::Ok(_) => ostd::io::println("Init: tier2-grant-pair owner admitted."),
        SyscallResult::Err(_) => {
            ostd::io::println("Init: tier2-grant-pair owner spawn failed.");
            return;
        }
    }

    for mode in GRANT_PAIR_MODES {
        // Every launch republishes the command line: a spawn attempt consumes it.
        if !ostd::syscall::sys_set_spawn_args(&alloc::format!("{mode} peer={owner_name}")) {
            ostd::io::println("Init: tier2-grant-pair receiver command line refused.");
            return;
        }
        let tid = match sys_spawn_from_path(GRANT_PAIR_RECEIVER_PATH) {
            SyscallResult::Ok(tid) => tid,
            SyscallResult::Err(_) => {
                ostd::io::println(&alloc::format!(
                    "Init: tier2-grant-pair generation {mode} spawn failed."
                ));
                return;
            }
        };
        let started = crate::service_table::now_ticks();
        let mut polls = 0usize;
        while !task_terminated(tid) {
            if crate::service_table::now_ticks().wrapping_sub(started)
                >= GRANT_PAIR_DEATH_TIMEOUT_TICKS
                || polls >= GRANT_PAIR_DEATH_POLL_LIMIT
            {
                ostd::io::println(&alloc::format!(
                    "Init: tier2-grant-pair generation {mode} did not terminate."
                ));
                return;
            }
            ostd::task::yield_now();
            polls += 1;
        }
    }
    ostd::io::println("Init: tier2-grant-pair complete.");
}

#[cfg(feature = "board-rpi3")]
const DISPLAY_DRIVER_PATH: &str = "/bin/bcm-display";
#[cfg(not(feature = "board-rpi3"))]
const DISPLAY_DRIVER_PATH: &str = "/bin/virtio-gpu";

// QEMU-virt VirtIO drivers. BCM2837 exposes no VirtIO MMIO window, so probing
// them on board-rpi3 is a guaranteed data abort at 0x0A000000; storage is the
// in-kernel Arasan SDHCI path and networking is the LAN9514 via /bin/dwc2-usb.
#[cfg(not(feature = "board-rpi3"))]
const VIRTIO_BLOCK_DRIVER: &str = "/bin/block";
#[cfg(not(feature = "board-rpi3"))]
const VIRTIO_NET_DRIVER: &str = "/bin/virtio-net";

pub(crate) fn start_block_drivers() {
    #[cfg(not(feature = "board-rpi3"))]
    let _ = sys_spawn_from_path(VIRTIO_BLOCK_DRIVER);
    let _ = sys_spawn_from_path("/bin/nvme");
    // x86_64 PC lane (phase 02a): the SATA/AHCI cell. Absent from non-x86
    // images and idles when the machine has no AHCI controller, so this spawn
    // is inert everywhere else.
    #[cfg(target_arch = "x86_64")]
    let _ = sys_spawn_from_path("/bin/ahci");
    for _ in 0..400 {
        if sys_lookup_service(service::BLOCK_DRIVER).is_some() {
            break;
        }
        ostd::task::yield_now();
    }
}

pub(crate) fn prepare_service(path: &str) {
    if path == "/bin/net" {
        #[cfg(not(feature = "board-rpi3"))]
        let _ = sys_spawn_from_path(VIRTIO_NET_DRIVER);
        let _ = sys_spawn_from_path("/bin/e1000");
        // x86_64 PC lane (phase 04a): the igb (i210/i211) NIC cell, on the same
        // NIC edge as e1000. It confirms the PCI identity is its own family and
        // idles when the machine has no igb, so this spawn is inert on every
        // other profile and on an e1000-only machine.
        let _ = sys_spawn_from_path("/bin/igb");
        // x86_64 PC lane (phase 06): the 16550 serial cell. The kernel owns the
        // ports and probes them at boot; this cell drives only the ones that
        // answered, so the spawn is inert on a machine with just a console.
        let _ = sys_spawn_from_path("/bin/serial");
        #[cfg(feature = "usb-host")]
        let _ = sys_spawn_from_path("/bin/dwc2-usb");
        // x86_64 PC lane (phase 03): the xHCI USB host cell, on the same USB
        // edge. It idles when the machine has no xHCI controller, so this spawn
        // is inert on every other profile.
        #[cfg(target_arch = "x86_64")]
        let _ = sys_spawn_from_path("/bin/xhci");
        if sys_lookup_service(service::BLOCK_DRIVER).is_none() {
            let _ = sys_spawn_from_path("/bin/nvme");
        }
        for _ in 0..4 {
            ostd::task::yield_now();
        }
    }
    if path == "/bin/compositor" {
        let _ = sys_spawn_from_path(DISPLAY_DRIVER_PATH);
        for _ in 0..4 {
            ostd::task::yield_now();
        }
    }
}

pub(crate) fn spawn_hypervisor() -> Option<usize> {
    match sys_spawn_from_path("/bin/hypervisor") {
        SyscallResult::Ok(tid) => {
            let _ = sys_notify_on_exit(tid);
            if let SyscallResult::Err(_) =
                sys_register_service(api::hypervisor::HYPERVISOR_SERVICE_ID, tid)
            {
                ostd::io::println("Init: hypervisor service registration failed.");
                None
            } else {
                Some(tid)
            }
        }
        _ => None,
    }
}

pub(crate) fn spawn_optional_services() -> Option<usize> {
    // Tier-2 entry fixtures (phase 02). Only the AArch64 test-hooks image can
    // admit a domain-class cell, and it is also the one image whose boot ends
    // before its shell becomes interactive: the shell sleeps ~2 s before its
    // first prompt, and the image's own test root exits the VM at ~4.5 s. So the
    // fixtures are launched here, first, where the boot order schedules them —
    // and tears them down — before that terminal exit. `tier2-entry` is opted
    // into by `scripts/build-aarch64-test-hooks-ci.sh` alone; no other image
    // builds init with it, so no other lane changes.
    #[cfg(feature = "tier2-entry")]
    {
        match sys_spawn_from_path("/bin/tier2-smoke") {
            SyscallResult::Ok(_) => ostd::io::println("Init: tier2-smoke admitted."),
            SyscallResult::Err(_) => ostd::io::println("Init: tier2-smoke spawn failed."),
        }
        match sys_spawn_from_path("/bin/tier2-exploit") {
            SyscallResult::Ok(_) => ostd::io::println("Init: tier2-exploit admitted."),
            SyscallResult::Err(_) => ostd::io::println("Init: tier2-exploit spawn failed."),
        }
    }

    // Phase-03 step-5 pair, from the boot order (see `run_grant_pair`). It runs
    // before the services below so a generation's terminal fault can never race
    // the image's own test root, whose exit is what ends the boot.
    #[cfg(feature = "tier2-grant-pair")]
    run_grant_pair();

    // fb-console mirrors the kernel user log to the display, so it stays on for
    // RPi3: a board whose only console is the serial header shows nothing on
    // HDMI otherwise. Bitmap text on a compositor surface, not a TTY.
    #[cfg(feature = "ui")]
    match sys_spawn_from_path("/bin/fb-console") {
        SyscallResult::Ok(_) => ostd::io::println("Init: fb-console spawned."),
        SyscallResult::Err(_) => ostd::io::println("Init: fb-console spawn failed."),
    }
    // The desktop shell is not auto-started on RPi3: it is packaged in the
    // image and launched on demand (`desktop &`), which keeps the board's
    // console and compositor free during device bring-up.
    #[cfg(all(feature = "ui", not(feature = "board-rpi3")))]
    match sys_spawn_from_path("/bin/desktop") {
        SyscallResult::Ok(_) => ostd::io::println("Init: desktop spawned."),
        SyscallResult::Err(_) => ostd::io::println("Init: desktop spawn failed."),
    }

    // Tier 3 is an option, not a profile: `tier3` puts the guest-hosting cell in
    // this image, `tier3-autostart` preloads the VM so the first app starts
    // fast, and without `tier3` there is nothing to start at all.
    #[cfg(all(feature = "tier3", feature = "tier3-autostart"))]
    let hypervisor_tid = spawn_hypervisor();
    #[cfg(all(feature = "tier3", not(feature = "tier3-autostart")))]
    let hypervisor_tid = {
        ostd::io::println("Init: tier-3 VM idle — run 'hv' in the shell to start a guest");
        None
    };
    #[cfg(not(feature = "tier3"))]
    let hypervisor_tid = {
        ostd::io::println("Init: Tier 3 is not part of this image — no guest can be started");
        None
    };

    #[cfg(not(feature = "board-rpi3"))]
    {
        let _ = sys_spawn_from_path("/bin/silo-test");
        // Spec 24 G2 Level A oracle: exercises the inference service over typed IPC.
        let _ = sys_spawn_from_path("/bin/ai-test");
        let _ = sys_spawn_from_path("/bin/vfs-test");
        let _ = sys_spawn_from_path("/bin/srv-test");
        let _ = sys_spawn_from_path("/bin/std-smoke");
    }
    hypervisor_tid
}
