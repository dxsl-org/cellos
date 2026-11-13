use ostd::syscall::{sys_spawn_from_path, SyscallResult};

/// Has `tid` terminated? A task the listing no longer carries has.
fn task_terminated(tid: usize) -> bool {
    let mut rows = [abi::syscall::ProcessInfo::default(); 64];
    match ostd::syscall::sys_get_procs(&mut rows) {
        Ok(count) => rows
            .iter()
            .take(count)
            .all(|row| row.id != tid || row.state == 3),
        // A listing that cannot be taken is not evidence of death.
        Err(_) => false,
    }
}

const PLATFORM_WAIT_MS: u64 = 60_000;
const PLATFORM_PROCESS_ROWS: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlatformWaitError {
    Listing,
    FullTable,
    Ambiguous,
    NoClock,
    Deadline,
}

impl PlatformWaitError {
    pub(crate) fn diagnostic(&self) -> &'static str {
        match self {
            Self::Listing => {
                "Init: platform process listing failed — boot stopped before driver spawning."
            }
            Self::FullTable => {
                "Init: platform process table full — boot stopped before driver spawning."
            }
            Self::Ambiguous => {
                "Init: platform process lifetime ambiguous — boot stopped before driver spawning."
            }
            Self::NoClock => {
                "Init: platform live without a clock — boot stopped before driver spawning."
            }
            Self::Deadline => {
                "Init: platform still live at 60 s — boot stopped before driver spawning."
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum PlatformWait {
    Ready,
    Yield,
}

/// Order bootstrap lifetimes only: absence/termination is not scan-success attestation.
fn platform_wait_step(
    rows: &[abi::syscall::ProcessInfo],
    count: Result<usize, ()>,
    elapsed_ms: Option<u64>,
) -> Result<PlatformWait, PlatformWaitError> {
    let count = count.map_err(|()| PlatformWaitError::Listing)?;
    // GetProcs truncates at capacity; a full snapshot cannot prove absence or uniqueness.
    if count >= rows.len() {
        return Err(PlatformWaitError::FullTable);
    }
    let mut platform = None;
    for row in &rows[..count] {
        // The kernel exports zero-padded task names. Do not match prefixes or paths.
        if &row.name[..8] == b"platform" && row.name[8..].iter().all(|&byte| byte == 0) {
            if platform.is_some() || row.state > 3 {
                return Err(PlatformWaitError::Ambiguous);
            }
            platform = Some(row.state);
        }
    }
    if platform.is_none() || platform == Some(3) {
        return Ok(PlatformWait::Ready);
    }
    match elapsed_ms {
        None => Err(PlatformWaitError::NoClock),
        Some(ms) if ms >= PLATFORM_WAIT_MS => Err(PlatformWaitError::Deadline),
        Some(_) => Ok(PlatformWait::Yield),
    }
}

/// The continuation owns every driver/VFS/configured-service spawn, so failures cannot
/// fall through into a partial boot. Host tests inject the same flow's syscall edges.
fn platform_boot_flow<R>(
    x86: bool,
    mut snapshot: impl FnMut(&mut [abi::syscall::ProcessInfo]) -> Result<usize, ()>,
    mut now_ms: impl FnMut() -> Option<u64>,
    mut yield_now: impl FnMut(),
    start: impl FnOnce() -> R,
) -> Result<R, PlatformWaitError> {
    if x86 {
        let mut rows = [abi::syscall::ProcessInfo::default(); PLATFORM_PROCESS_ROWS];
        let started = now_ms();
        loop {
            let count = snapshot(&mut rows);
            let elapsed = started.and_then(|start| now_ms().map(|now| now.saturating_sub(start)));
            match platform_wait_step(&rows, count, elapsed)? {
                PlatformWait::Ready => break,
                PlatformWait::Yield => yield_now(),
            }
        }
    }
    Ok(start())
}

pub(crate) fn start_after_platform(start: impl FnOnce()) -> Result<(), PlatformWaitError> {
    platform_boot_flow(
        cfg!(target_arch = "x86_64"),
        |rows| ostd::syscall::sys_get_procs(rows).map_err(|_| ()),
        ostd::syscall::sys_get_time_ms,
        ostd::task::yield_now,
        start,
    )
}

#[cfg(test)]
mod platform_wait_tests {
    use super::{platform_boot_flow, PlatformWaitError, PLATFORM_PROCESS_ROWS, PLATFORM_WAIT_MS};
    use abi::syscall::ProcessInfo;
    use alloc::{vec, vec::Vec};
    use core::cell::RefCell;

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Clock,
        Snapshot,
        Yield,
        Spawn(&'static str),
    }

    const SPAWNS: [Event; 4] = [
        Event::Spawn("/bin/block"),
        Event::Spawn("/bin/nvme"),
        Event::Spawn("/bin/ahci"),
        Event::Spawn("/bin/vfs"),
    ];

    fn process(name: &str, state: usize) -> ProcessInfo {
        let mut row = ProcessInfo {
            id: 1,
            state,
            ..ProcessInfo::default()
        };
        row.name[..name.len()].copy_from_slice(name.as_bytes());
        row
    }

    fn run_flow(
        x86: bool,
        snapshots: &[Result<&[ProcessInfo], ()>],
        clocks: &[Option<u64>],
    ) -> (Result<(), PlatformWaitError>, Vec<Event>) {
        let trace = RefCell::new(Vec::new());
        let mut snapshots = snapshots.iter();
        let mut clocks = clocks.iter();
        let result = platform_boot_flow(
            x86,
            |rows| {
                trace.borrow_mut().push(Event::Snapshot);
                let next = (*snapshots.next().expect("unexpected process sample"))?;
                rows[..next.len()].copy_from_slice(next);
                Ok(next.len())
            },
            || {
                trace.borrow_mut().push(Event::Clock);
                *clocks.next().expect("unexpected clock sample")
            },
            || trace.borrow_mut().push(Event::Yield),
            || trace.borrow_mut().extend(SPAWNS),
        );
        (result, trace.into_inner())
    }

    fn assert_stopped(
        snapshots: &[Result<&[ProcessInfo], ()>],
        clocks: &[Option<u64>],
        expected: PlatformWaitError,
    ) -> Vec<Event> {
        let (result, trace) = run_flow(true, snapshots, clocks);
        assert_eq!(result, Err(expected));
        assert!(trace.iter().all(|event| !matches!(event, Event::Spawn(_))));
        trace
    }

    #[test]
    fn live_to_terminated_or_absent_yields_before_every_driver_and_vfs_spawn() {
        let terminated = [process("platform", 3)];
        for state in 0..=2 {
            let live = [process("platform", state)];
            for final_rows in [terminated.as_slice(), &[]] {
                let (result, trace) = run_flow(
                    true,
                    &[Ok(&live), Ok(&live), Ok(final_rows)],
                    &[Some(100), Some(100), Some(101), Some(102)],
                );
                assert_eq!(result, Ok(()));
                let mut expected = vec![
                    Event::Clock,
                    Event::Snapshot,
                    Event::Clock,
                    Event::Yield,
                    Event::Snapshot,
                    Event::Clock,
                    Event::Yield,
                    Event::Snapshot,
                    Event::Clock,
                ];
                expected.extend(SPAWNS);
                assert_eq!(trace, expected);
            }
        }
    }

    #[test]
    fn absent_exited_and_other_names_pass_without_a_clock() {
        let exited = [process("platform", 3)];
        let other_names = [
            process("platform-helper", 1),
            process("platform ", 1),
            process("/bin/platform", 1),
            process("Platform", 1),
            process("ahci", 1),
        ];
        for rows in [&[][..], exited.as_slice(), other_names.as_slice()] {
            let (result, trace) = run_flow(true, &[Ok(rows)], &[None]);
            assert_eq!(result, Ok(()));
            let mut expected = vec![Event::Clock, Event::Snapshot];
            expected.extend(SPAWNS);
            assert_eq!(trace, expected);
        }
    }

    #[test]
    fn listing_full_and_ambiguous_snapshots_stop_before_every_spawn() {
        let full = [ProcessInfo::default(); PLATFORM_PROCESS_ROWS];
        let duplicates = [process("platform", 1), process("platform", 3)];
        let invalid_state = [process("platform", 4)];
        for (snapshot, error) in [
            (Err(()), PlatformWaitError::Listing),
            (Ok(full.as_slice()), PlatformWaitError::FullTable),
            (Ok(duplicates.as_slice()), PlatformWaitError::Ambiguous),
            (Ok(invalid_state.as_slice()), PlatformWaitError::Ambiguous),
        ] {
            assert_stopped(&[snapshot], &[Some(0), Some(0)], error);
        }
    }

    #[test]
    fn live_clock_loss_stops_before_every_spawn() {
        let live = [process("platform", 1)];
        assert_stopped(&[Ok(&live)], &[None], PlatformWaitError::NoClock);
        let trace = assert_stopped(
            &[Ok(&live), Ok(&live)],
            &[Some(100), Some(100), None],
            PlatformWaitError::NoClock,
        );
        assert_eq!(
            trace.iter().filter(|event| **event == Event::Yield).count(),
            1
        );
    }

    #[test]
    fn exact_deadline_stops_live_platform_without_resetting_the_budget() {
        let live = [process("platform", 1)];
        let trace = assert_stopped(
            &[Ok(&live), Ok(&live)],
            &[
                Some(100),
                Some(100 + PLATFORM_WAIT_MS - 1),
                Some(100 + PLATFORM_WAIT_MS),
            ],
            PlatformWaitError::Deadline,
        );
        assert_eq!(
            trace.iter().filter(|event| **event == Event::Yield).count(),
            1
        );
    }

    #[test]
    fn absence_at_the_deadline_still_orders_lifetimes_without_claiming_scan_success() {
        let live = [process("platform", 1)];
        let (result, trace) = run_flow(
            true,
            &[Ok(&live), Ok(&[])],
            &[
                Some(100),
                Some(100 + PLATFORM_WAIT_MS - 1),
                Some(100 + PLATFORM_WAIT_MS),
            ],
        );
        assert_eq!(result, Ok(()));
        assert_eq!(&trace[trace.len() - SPAWNS.len()..], &SPAWNS);
        assert_eq!(
            trace.iter().filter(|event| **event == Event::Yield).count(),
            1
        );
    }

    #[test]
    fn non_x86_runs_original_spawn_flow_without_sampling_or_yielding() {
        let (result, trace) = run_flow(false, &[], &[]);
        assert_eq!(result, Ok(()));
        assert_eq!(trace, SPAWNS);
    }
}

// Storage bootstrap is fixed; optional network/display drivers are selected by
// the generated service configuration rather than compiled runtime choices.
#[cfg(not(feature = "board-rpi3"))]
const VIRTIO_BLOCK_DRIVER: &str = "/bin/block";

/// How long init waits for the block Driver Cells to settle (Phase 10 S10-3c, D3c-2 B):
/// one NVMe admin budget. The VFS enumerates block routes once, when it starts, so every
/// device must have its route bound by then.
const BLOCK_DRIVER_WAIT_MS: u64 = 60_000;
/// The bound when there is no monotonic clock.
const BLOCK_DRIVER_WAIT_YIELDS: usize = 400;

/// Where one spawned block Driver Cell stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DriverProgress {
    /// A block route names it as the live provider.
    Routed,
    /// It exited (no device on this machine, or a failure).
    Terminated,
    /// Neither yet.
    Starting,
}

/// What the block-driver wait does next.
#[derive(Debug, PartialEq, Eq)]
enum BlockWait {
    /// Every driver is routed or terminated.
    Settled,
    /// The bound ran out with a driver still starting.
    TimedOut,
    KeepWaiting,
}

/// One step of the block-driver wait: settled once no driver is still starting;
/// otherwise wait on until `BLOCK_DRIVER_WAIT_MS` (or, without a clock,
/// `BLOCK_DRIVER_WAIT_YIELDS` yields) is spent.
fn block_wait_step(
    drivers: &[DriverProgress],
    elapsed_ms: Option<u64>,
    yields: usize,
) -> BlockWait {
    if !drivers.contains(&DriverProgress::Starting) {
        return BlockWait::Settled;
    }
    let spent = match elapsed_ms {
        Some(ms) => ms >= BLOCK_DRIVER_WAIT_MS,
        None => yields >= BLOCK_DRIVER_WAIT_YIELDS,
    };
    if spent {
        BlockWait::TimedOut
    } else {
        BlockWait::KeepWaiting
    }
}

/// A block Driver Cell that init respawns when it dies with a failure status.
/// Like every supervised service, a replacement starts only after the dead
/// incarnation's retirement verdict allows it (`Contained`, or `Quarantined`
/// without `DMA_DEFERRED`). A verdict with `DMA_DEFERRED` means the kernel
/// retains the PCI function: the driver is never restarted this boot and the
/// device stays unavailable. A clean exit (nothing to claim) is final. An
/// incarnation is ready once a block route names it as the live provider.
fn supervised_block_driver(name: &str, path: &str, tid: usize) -> crate::service_table::Service {
    use alloc::string::String;
    use cellos_boot_config::{CellSpec, Registration, RestartPolicy};
    let mut driver = crate::service_table::Service::new(CellSpec {
        name: String::from(name),
        path: String::from(path),
        args: alloc::vec::Vec::new(),
        enabled: true,
        required: false,
        restart: RestartPolicy::OnFailure,
        service_id: None,
        registration: Registration::SelfReady,
        after: alloc::vec::Vec::new(),
        // A replacement resets its controller and identifies the disk before it
        // binds its route; 10 s covers both on the QEMU models.
        ready_timeout_ticks: 1000,
    });
    driver.readiness = crate::service_table::Readiness::BlockRoute;
    if matches!(ostd::syscall::sys_notify_on_exit(tid), SyscallResult::Ok(0)) {
        driver.tid = Some(tid);
    }
    driver
}

/// Spawn the block Driver Cells and wait until each has bound its route or
/// exited. Returns the drivers init supervises from here on (NVMe and, on x86,
/// AHCI, virtio-scsi and PVSCSI); the VirtIO block driver is waited for but not
/// supervised.
pub(crate) fn start_block_drivers() -> alloc::vec::Vec<crate::service_table::Service> {
    let mut supervised = alloc::vec::Vec::new();
    let mut spawned = alloc::vec::Vec::new();
    #[cfg(not(feature = "board-rpi3"))]
    if let SyscallResult::Ok(tid) = sys_spawn_from_path(VIRTIO_BLOCK_DRIVER) {
        spawned.push(tid);
    }
    if let SyscallResult::Ok(tid) = sys_spawn_from_path("/bin/nvme") {
        spawned.push(tid);
        supervised.push(supervised_block_driver("nvme", "/bin/nvme", tid));
    }
    // x86_64 PC lane (phase 02a): the SATA/AHCI cell. Absent from non-x86
    // images and exits cleanly when the machine has no SATA disk, so this
    // spawn is inert everywhere else.
    #[cfg(target_arch = "x86_64")]
    if let SyscallResult::Ok(tid) = sys_spawn_from_path("/bin/ahci") {
        spawned.push(tid);
        supervised.push(supervised_block_driver("ahci", "/bin/ahci", tid));
    }
    // ADR-0031 phase 05: the virtio-scsi cell is a block driver too, so it must
    // bind its route before VFS mounts; it exits cleanly without a controller.
    #[cfg(target_arch = "x86_64")]
    if let SyscallResult::Ok(tid) = sys_spawn_from_path("/bin/virtio-scsi") {
        spawned.push(tid);
        supervised.push(supervised_block_driver(
            "virtio-scsi",
            "/bin/virtio-scsi",
            tid,
        ));
    }
    // S3 phase 09: VMware's PVSCSI host is a block driver too; it exits
    // cleanly without a controller.
    #[cfg(target_arch = "x86_64")]
    if let SyscallResult::Ok(tid) = sys_spawn_from_path("/bin/pvscsi") {
        spawned.push(tid);
        supervised.push(supervised_block_driver("pvscsi", "/bin/pvscsi", tid));
    }
    let started = ostd::syscall::sys_get_time_ms();
    let mut yields = 0;
    loop {
        let routes = crate::service_table::block_routes();
        let progress: alloc::vec::Vec<DriverProgress> = spawned
            .iter()
            .map(|&tid| {
                if crate::service_table::routes_name(&routes, tid) {
                    DriverProgress::Routed
                } else if task_terminated(tid) {
                    DriverProgress::Terminated
                } else {
                    DriverProgress::Starting
                }
            })
            .collect();
        let elapsed = started.and_then(|start| {
            ostd::syscall::sys_get_time_ms().map(|now| now.saturating_sub(start))
        });
        match block_wait_step(&progress, elapsed, yields) {
            BlockWait::Settled => break,
            BlockWait::TimedOut => {
                ostd::io::println("Init: a block driver bound no route within the wait; starting the VFS with the routes bound so far.");
                break;
            }
            BlockWait::KeepWaiting => {
                yields += 1;
                ostd::task::yield_now();
            }
        }
    }
    // A driver that exited (no device) is still retiring: its quiescence fences
    // every CPU. The VFS starts only after those retirements are final, so it
    // discovers a settled route set and its spawn does not overlap them.
    for &tid in &spawned {
        if task_terminated(tid) {
            let _ = crate::supervisor::await_verdict(tid);
        }
    }
    supervised
}

#[cfg(test)]
mod block_wait_tests {
    use super::{
        block_wait_step, BlockWait, DriverProgress, BLOCK_DRIVER_WAIT_MS, BLOCK_DRIVER_WAIT_YIELDS,
    };
    use DriverProgress::{Routed, Starting, Terminated};

    #[test]
    fn every_driver_routed_or_terminated_ends_the_wait_at_once() {
        for drivers in [
            &[][..],
            &[Routed],
            &[Terminated],
            &[Routed, Terminated, Routed],
            &[Terminated, Terminated],
        ] {
            for elapsed in [Some(0), None, Some(BLOCK_DRIVER_WAIT_MS + 1)] {
                assert_eq!(block_wait_step(drivers, elapsed, 0), BlockWait::Settled);
            }
        }
    }

    #[test]
    fn one_starting_driver_among_several_is_waited_for_up_to_the_budget() {
        for drivers in [
            &[Routed, Starting][..],
            &[Starting, Terminated, Routed],
            &[Starting, Starting],
        ] {
            assert_eq!(block_wait_step(drivers, Some(0), 0), BlockWait::KeepWaiting);
            assert_eq!(
                block_wait_step(drivers, Some(BLOCK_DRIVER_WAIT_MS - 1), 1_000_000),
                BlockWait::KeepWaiting
            );
            assert_eq!(
                block_wait_step(drivers, Some(BLOCK_DRIVER_WAIT_MS), 0),
                BlockWait::TimedOut
            );
        }
    }

    #[test]
    fn without_a_clock_the_yield_bound_applies() {
        let drivers = [Routed, Starting];
        assert_eq!(block_wait_step(&drivers, None, 0), BlockWait::KeepWaiting);
        assert_eq!(
            block_wait_step(&drivers, None, BLOCK_DRIVER_WAIT_YIELDS - 1),
            BlockWait::KeepWaiting
        );
        assert_eq!(
            block_wait_step(&drivers, None, BLOCK_DRIVER_WAIT_YIELDS),
            BlockWait::TimedOut
        );
    }
}
