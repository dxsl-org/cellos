// SPDX-License-Identifier: MPL-2.0
//! Cellos Kernel - Entry point

#![cfg_attr(not(all(test, not(target_os = "none"))), no_std)]
#![cfg_attr(not(all(test, not(target_os = "none"))), no_main)]
#![cfg_attr(not(all(test, not(target_os = "none"))), feature(alloc_error_handler))]

#[cfg(all(feature = "board-vf2", feature = "board-pioneer"))]
compile_error!(
    "Conflicting RISC-V board features: `board-vf2` and `board-pioneer` cannot be enabled together."
);

#[cfg(all(feature = "board-rpi3", feature = "board-rpi4"))]
compile_error!(
    "Conflicting AArch64 board features: `board-rpi3` and `board-rpi4` cannot be enabled together."
);

#[cfg(all(
    feature = "production-relay-image",
    any(
        feature = "dev-policy-key",
        feature = "dev-signing-key",
        feature = "dev-weak-rng",
        feature = "test-hooks",
        feature = "qemu-dev-https",
        feature = "maintenance-mode"
    )
))]
compile_error!("production relay kernel excludes development, test, and recovery features");

#[cfg(all(feature = "qemu-dev-https", not(target_arch = "x86_64")))]
compile_error!("QEMU DEV HTTPS requires the x86_64 hardware entropy implementation");

extern crate alloc;

#[cfg(not(all(test, not(target_os = "none"))))]
use core::panic::PanicInfo;

#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
static TEST_SAFE_ROOT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

// Core kernel modules
pub mod acpi;
pub mod admission;
pub mod arch; // Architecture code; arch/mod.rs selects one module per target
pub mod audit;
#[cfg(feature = "bench-counters")]
pub(crate) mod bench_counters; // Measurement-only counters, raw opcode 504
mod board;
pub mod boot;
pub mod catalog;
#[cfg(test)]
mod catalog_export;
pub mod cell;
pub mod dev; // Kernel-resident device mechanisms (IRQ, IOMMU, MSI, early console)
pub mod ed25519; // Ed25519 verify (no_std) for signed operator policy (P5 spike)
pub mod fs; // Filesystem
pub mod fsapi; // Per-task file descriptors and cwd over BootFS (Spec 15 exception)
pub mod hypervisor; // EL2 VMM kernel support (Phase 03+)
pub mod ipc; // Kernel IPC objects: wire copy, mailboxes, pipes, futex, sessions
pub mod loader;
pub(crate) mod machine_facts; // Machine facts record for syscall 433 (ADR-0029 §3)
pub mod measurement_log; // Per-Cell integrity measurement (IMA-style, TPM-free)
pub mod memory;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_measurement;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_noncpu;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_noncpu_topology;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_resources;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_retained;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_topology;
#[cfg(feature = "p01-trace")]
pub(crate) mod p01_vtd_topology;
pub mod policy; // Signed operator policy (P5b) — headless consent
#[cfg(test)]
mod posture;
pub mod resource_registry;
pub mod sched; // Scheduler, SMP bring-up, hart-local state, quiescence
pub mod sha256; // Self-contained SHA-256 for measurement
pub mod signing; // Cell binary signing (Ed25519) — verification gate at spawn time
pub mod smbios; // Bounded SMBIOS type 1 reader (x86 boot identity)

// The snapshot engine is compiled only for the qualified profile (and the host
// tests); every other kernel gets the gate-closed API (Phase 09). Callers use
// `crate::snapshot::*` either way.
#[cfg(any(not(feature = "snapshot-qualified"), test))]
#[cfg_attr(test, allow(dead_code))] // host tests reach it only through its own tests
mod snapshot_disabled;
#[cfg(any(feature = "snapshot-qualified", test))]
pub mod snapshot_engine;
pub mod snapshot_format;
pub mod snapshot {
    #[cfg(not(any(feature = "snapshot-qualified", test)))]
    pub use crate::snapshot_disabled::*;
    #[cfg(any(feature = "snapshot-qualified", test))]
    pub use crate::snapshot_engine::*;
}
pub mod system_config; // Bounded cold-boot settings, loaded after embedded FS mount
pub mod task; // Renamed from 'process'
pub extern crate hal; // HAL (Architecture specific)
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
use boot::BootInfo;
#[cfg(target_arch = "aarch64")]
use hal::backend::ArmPlatform as _;
use hal::Arch;

// Internal utilities
mod cpu_features;
pub mod platform;
mod sync;
#[cfg(test)]
pub(crate) static TEST_STATE_LOCK: sync::Spinlock<()> =
    sync::Spinlock::ranked((), crate::sync::rank::TEST_STATE_LOCK);

/// Signal QEMU to exit with a success (0) or failure (1) code.
///
/// Only available under the `test-hooks` feature — never call this in
/// production builds. The kernel integration-test harness uses this
/// instead of parsing serial output for "PASS"/"FAIL" banners.
///
/// Device addresses: RISC-V = SiFive test 0x100000, ARM64 = semihosting,
/// x86_64 = isa-debug-exit (iobase 0xF4).
#[cfg(feature = "test-hooks")]
pub fn qemu_exit(success: bool) -> ! {
    use qemu_exit::QEMUExit;
    #[cfg(target_arch = "riscv64")]
    {
        // SAFETY: 0x100000 is the SiFive test device address on the QEMU `virt`
        // machine used by this test-only exit path.
        unsafe {
            qemu_exit::RISCV64::new(0x100000).exit(if success { 0 } else { 1 });
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        qemu_exit::AArch64::new().exit(if success { 0 } else { 1 });
    }
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: 0xF4 is the QEMU `isa-debug-exit` I/O port wired in the x86
        // test harness (`-device isa-debug-exit,iobase=0xf4`); writing it exits
        // QEMU. Constructing the port handle is `unsafe` (arbitrary port I/O).
        unsafe {
            qemu_exit::X86::new(0xF4, 0).exit(if success { 0 } else { 1 });
        }
    }
    // Fallback for other arches: spin forever so the test times out clearly.
    #[allow(clippy::empty_loop)]
    // reason: on riscv/aarch64/x86_64 the arch-specific `.exit()` above diverges,
    // so this loop is unreachable there; it only executes on a hypothetical arch
    // with no qemu_exit backend.
    #[allow(unreachable_code)]
    loop {}
}

// Re-export types for convenience
pub use types::*;

// Embed Init Binary (stripped by build.rs, served from EMBEDDED_OUT_DIR).
// RV32 Nano (Phase 31) has no init ELF; x86_64 is now included (Phase 04).
#[cfg(cellos_wide)]
static INIT_ELF: &[u8] = include_bytes!(concat!(env!("EMBEDDED_OUT_DIR"), "/init"));

/// Kernel entry point called from HAL boot code
///
/// The architecture phases (`crate::arch::boot::early`, then the heap, then
/// `crate::arch::boot::platform_init`) bring the platform up; everything after
/// the [`boot::PlatformReady`] they return is arch-neutral. The `arch::boot`
/// hooks below run where that architecture's code ran before the split.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[no_mangle]
pub extern "C" fn kmain(hartid: usize, dtb: usize) -> ! {
    // 0–3. Early console, traps, boot memory, frame allocator, paging.
    let early = arch::boot::early(hartid, dtb);

    // 4. Heap Allocator (Global) - MUST be after paging but before any allocations
    // 4 MiB = 1024 frames (reduced from 32 MiB / 8192 frames under Phase 01 memory footprint reduction).
    // Sized to hold kernel structures, scheduler state, IPC buffers, and cell signing scratch.
    #[cfg(target_pointer_width = "64")]
    const HEAP_FRAMES: usize = 8_192; // 32 MiB
    #[cfg(target_pointer_width = "32")]
    const HEAP_FRAMES: usize = 1_024; // 4 MiB
    let heap_start = {
        let mut allocator_guard = memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = allocator_guard
            .as_mut()
            .expect("Frame allocator not initialized");
        // One contiguous reservation: the heap below is initialized over
        // `HEAP_FRAMES * 4096` bytes starting at this frame, so a scattered
        // allocation would write over frames this kernel does not own. Refusing
        // here is the only safe outcome — never a smaller or fragmented heap.
        // The run is taken in the bitmap's range, right after the bitmap and the
        // early frames there: the shared `[bitmap, heap)` registration below
        // depends on that order.
        memory::frame::reserve_boot_heap(allocator, HEAP_FRAMES).expect(
            "OOM: no contiguous run for the boot heap in the frame bitmap's range — \
             refusing to boot with a fragmented heap",
        )
    };
    let heap_size = HEAP_FRAMES * 4096;
    // On x86_64, phys_to_virt adds HHDM offset (Limine maps RAM at HHDM+phys).
    // On RISC-V, phys_to_virt returns phys unchanged (identity-mapped before paging).
    let heap_virt = memory::frame::phys_to_virt(heap_start);
    unsafe {
        memory::heap::init_heap(heap_virt, heap_size);
    }
    log_info("Heap initialized");

    memory::rt_heap::init();
    log_info("RT heap initialized");

    // 5–6. Interrupt controllers, log backend and console RX, devices, the
    // early PCIe scan and DMA isolation.
    let boot::PlatformReady {
        console,
        irq,
        clock,
        pci,
        iommu,
        storage,
    } = arch::boot::platform_init(early);

    match storage {
        boot::BootStorage::VirtioMmio => {
            // Attempt warm boot from snapshot before any cell initialization.
            if snapshot::try_restore() {
                // try_restore() called yield_cpu() and should not return in a successful
                // warm boot.  If we reach here, fall through to cold boot as a safety net.
                log::warn!("[boot] snapshot restore returned unexpectedly → cold boot");
            }

            // Cross-check the on-disk MBR against the compiled-in partition layout
            // (warn-only — surfaces image/kernel drift at boot instead of as silent
            // corruption later).
            crate::loader::disk_layout::verify_mbr();

            // Probe the cell bootstrap table so SpawnFromPath works during init.
            match crate::loader::early::EarlyLoader::probe() {
                Ok(()) => arch::boot::puts("[loader] cell bootstrap table loaded\n"),
                Err(_) => arch::boot::puts(
                    "[loader] WARN: cell table not found — disk image may lack bootstrap section\n",
                ),
            }
        }
        // x86_64 serves cells from the ramdisk; RV32 Nano has no disk.
        boot::BootStorage::None => {}
    }

    // RV32 Nano: no FAT32 FS in bring-up.
    // x86_64 uses the ramdisk-backed embedded FS to serve cell ELFs via VIFS1.
    #[cfg(cellos_wide)]
    fs::init();

    #[cfg(cellos_wide)]
    system_config::load_from_vifs1();

    // Load + verify the signed operator policy (P5b) NOW: after VIFS1 is mounted,
    // before any cap-bearing cell spawns. Absent → dev-permissive (this G1 build);
    // invalid → fail-closed. Phase 04 folds policy::lookup into the spawn grant.
    #[cfg(cellos_wide)]
    policy::load_from_vifs1();

    arch::boot::fixtures_after_policy();

    log_info("Kernel subsystems initialized successfully.");

    // 7. Initialize Scheduler
    log_info("Initializing scheduler...");
    task::init();
    log_info("Scheduler initialized");
    #[cfg(feature = "test-hooks")]
    if crate::sched::scheduler::death_subscription_quota_self_test() {
        log_info("death-subscriber quota self-test PASS");
    } else {
        log_info("death-subscriber quota self-test FAIL");
    }
    // Must precede every unrelated boot suite (riscv64 getrandom SAS branch).
    arch::boot::fixtures_after_task_init();

    #[cfg(feature = "test-hooks")]
    if crate::resource_registry::pcie_bar_window_self_test() {
        log_info("[selftest] PCIE-BAR-WINDOW: PASS (bounded, aligned, overflow-safe)");
    } else {
        log_info("[selftest] PCIE-BAR-WINDOW: FAIL");
    }

    // The loader corpus exercises real spawn_gated denials and snapshots the
    // scheduler around every malformed image. Run it after task::init(), while
    // the scheduler is available but still empty and before secondary harts or
    // unrelated boot tasks can add noise. Assertions make any failure boot-fatal.
    #[cfg(feature = "test-hooks")]
    crate::loader::elf_tests::run_all();
    arch::boot::fixtures_after_loader_corpus();

    // 7a. Trust-model self-tests (thread identity inheritance + honest revoke).
    // Runs HERE — after the scheduler exists but BEFORE secondaries start — so the
    // synthetic thread it spawns cannot be raced onto another hart before teardown.
    // Fixtures that drive the live scheduler: never compiled into a release
    // kernel (Spec 15 §3.5; architecture remediation Phase 09).
    #[cfg(all(not(cellos_posture = "release"), cellos_wide))]
    {
        if task::thread_cap_selftest::self_test() {
            log_info("thread-cap self-test PASS (thread-inherit + honest-revoke + spawn bound)");
        } else {
            log_info("thread-cap self-test FAIL");
        }
        if task::grant_reclaim_selftest::self_test() {
            log_info("grant-reclaim self-test PASS (owned reclaimed, received kept, pin withheld)");
        } else {
            log_info("grant-reclaim self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::mmio_revoke_selftest::self_test() {
            log_info("mmio-revoke self-test PASS (class selective, user access removed)");
        } else {
            log_info("mmio-revoke self-test FAIL");
        }
        if task::thread_quota_selftest::self_test() {
            log_info("thread-quota self-test PASS (charged, released, enforced)");
        } else {
            log_info("thread-quota self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::task_id_selftest::self_test() {
            log_info("task-id-reuse self-test PASS (monotonic across spawn/exit)");
        } else {
            log_info("task-id-reuse self-test FAIL");
        }
        match task::thread_user_entry_selftest::self_test() {
            Some(true) => log_info("thread-user-entry self-test PASS (U-mode entry+arg+exit)"),
            Some(false) => log_info("thread-user-entry self-test FAIL"),
            None => log_info("thread-user-entry self-test SKIP (RV64 runtime gate only)"),
        }
        if task::completion_selftest::self_test() {
            log_info("completion-queue self-test PASS (reserve, land, bound, defer)");
        } else {
            log_info("completion-queue self-test FAIL");
        }
        if task::net_rx_selftest::self_test() {
            log_info("net-rx-reservation self-test PASS (fill, remember, release)");
        } else {
            log_info("net-rx-reservation self-test FAIL");
        }
        if task::ipc_pending_selftest::self_test() {
            log_info("ipc-pending self-test PASS (deferred delivery, bounds, quota)");
        } else {
            log_info("ipc-pending self-test FAIL");
        }
        task::cap_file_selftest::run();
        log_info("cap-file self-test PASS (read-only enforcement)");
        if task::ipc_guardrail_selftest::self_test() {
            log_info("ipc-guardrail self-test PASS (dead-peer wake, RecvScatter isolated)");
        } else {
            log_info("ipc-guardrail self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::vfs_lifecycle_selftest::self_test() {
            log_info("vfs-lifetime self-test PASS (exact lease, quarantine, owner watch)");
        } else {
            log_info("vfs-lifetime self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack::stack_probe_self_test() {
            log_info(
                "stack-probe self-test PASS (two guards, overflow target unmapped, watermark)",
            );
        } else {
            log_info("stack-probe self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack_sizing_policy_self_test() {
            log_info("stack-sizing policy self-test PASS (measured=16, unknown=64)");
        } else {
            log_info("stack-sizing policy self-test FAIL");
        }
    }

    // Bring actual secondaries online only after task::init. x86 uses the
    // Limine MP callback and qualifies bidirectional invalidation before tasks.
    #[cfg(cellos_wide)]
    crate::sched::smp::start_secondaries();
    arch::boot::fixtures_after_secondaries();

    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::activate();
    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::register_static_image()
        .expect("kernel static ranges must be disjoint");
    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::register(
        heap_virt,
        heap_virt
            .checked_add(heap_size)
            .expect("kernel heap range overflow"),
        memory::domain_supervisor_registry::SupervisorRangeKind::KernelHeap,
        memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
    )
    .expect("kernel heap registration must be unique");
    #[cfg(all(feature = "native-domains", cellos_wide))]
    if let Some(guard) = memory::frame::FRAME_ALLOCATOR.lock().as_ref() {
        // `[bitmap, heap)`: the bitmap, plus any early boot frames the next-fit
        // cursor handed out from the bitmap's range before `reserve_boot_heap`
        // placed the heap there. When the bitmap's range is not the first
        // usable one (UEFI maps: ≥ 1 GiB under VMware, 4 GiB under OVMF), the
        // early paging frames come from lower ranges and the span is just the
        // bitmap.
        let (bm_start, _) = guard.bitmap_range();
        memory::domain_supervisor_registry::register(
            bm_start,
            heap_virt,
            memory::domain_supervisor_registry::SupervisorRangeKind::StaticWritable,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        )
        .expect("frame allocator bitmap and early paging registration must succeed");
    }
    #[cfg(all(feature = "native-domains", cellos_wide))]
    for (base, end) in arch::boot::supervisor_device_spans() {
        let _ = memory::domain_supervisor_registry::register(
            base,
            end,
            memory::domain_supervisor_registry::SupervisorRangeKind::DeviceMmio,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        );
    }
    // Tier 2 admission posture (ADR-0019 §2.3). The backend alone does not
    // authorize admission. Fleet profiles stay closed unless the explicit x86
    // browser profile is selected; that profile also narrows every publication
    // to the reviewed QuickJS artifact and requires its signed policy row.
    #[cfg(all(
        feature = "native-domains",
        cellos_wide,
        not(any(
            feature = "policy-required",
            feature = "production-relay-image",
            feature = "browser-engine-profile"
        ))
    ))]
    {
        if crate::loader::domain_admission::enable_for_boot() {
            log_info("Tier 2 admission: ENABLED (development profile)");
        } else {
            // Phase-01: an unqualified raw switch ordering keeps the development
            // profile closed, and the posture has to be visible — the alternative
            // is a silently disabled feature that reads as verified.
            log_info(
                "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate) \
                 — domain-class artifacts are denied",
            );
        }
    }
    #[cfg(all(
        feature = "native-domains",
        cellos_wide,
        any(feature = "policy-required", feature = "production-relay-image")
    ))]
    if !arch::boot::browser_engine_admission() {
        log_info("Tier 2 admission: DISABLED (fleet profile) — domain-class artifacts are denied");
    }
    #[cfg(all(cellos_wide, feature = "native-domains", feature = "test-hooks"))]
    if !crate::sched::domain_switch_tests::run_primary() {
        log::error!("native-domain SAS scheduler fixture failed");
    }
    arch::boot::fixtures_domain();

    // Page-table teardown primitives behind runtime cap revoke (`.agents/260712-1901`
    // P01). The tables are plain kernel-heap memory, so this needs no IOMMU
    // hardware and runs on every arch whose lane enables hooks.
    #[cfg(feature = "test-hooks")]
    crate::dev::iommu_pt::run_selftest();

    // Last-but-one boot fixture: the all-hart park hook (riscv64).
    arch::boot::fixtures_park();

    // Phase 10: the timer sweep runs on any hart, so a deadline passes while
    // hart 0 is held. Last of the boot fixtures:
    // it needs the secondaries online and leaves a retiring task behind.
    #[cfg(all(feature = "test-hooks", cellos_wide))]
    crate::sched::off_hart0_timeout_selftest::run_primary();

    // 8. Spawn Embedded Init
    // RV32 Nano bring-up: no init binary — boot to idle loop.
    #[cfg(cellos_wide)]
    {
        log_info("Spawning Embedded Init...");

        // Power-on self-test of the Ed25519 verify primitive (RFC 8032 TEST 1 +
        // tamper-negative) before it is trusted to authenticate the signed
        // operator policy (P5). Cheap (~one verify); a FAIL means the crypto path
        // is broken and signed policy must not be trusted.
        if crate::ed25519::self_test() {
            log_info("ed25519 verify self-test PASS (RFC 8032 + tamper)");
        } else {
            log_info("ed25519 verify self-test FAIL — signed policy unsafe");
        }
        // Power-on self-test of the signed-policy path: verify + parse a known
        // dev-signed blob, and confirm a tampered blob is rejected.
        if crate::policy::self_test() {
            log_info("policy verify+parse self-test PASS (signed blob + tamper)");
        } else {
            log_info("policy verify+parse self-test FAIL");
        }
        // Cell binary signing self-test: known-good vector verifies + flipped
        // payload is rejected. Runs before any spawn_from_path is called.
        if crate::signing::self_test() {
            log_info("cell signing self-test PASS");
        } else {
            log_info("cell signing self-test FAIL — cell signature gate unsafe");
        }
        {
            let (posture, anchor) = crate::signing::trust_anchor();
            log::info!(
                "[signing] cell trust posture={} anchor={:02x?} unsigned={}",
                posture,
                anchor,
                if crate::signing::unsigned_admitted() {
                    "tier2"
                } else {
                    "denied"
                }
            );
        }
        #[cfg(feature = "test-hooks")]
        if crate::admission::self_test() {
            log_info("admission-core self-test PASS (fail-closed A/B floor model)");
        } else {
            log_info("admission-core self-test FAIL");
        }
        // P-TRUST: privileged path-caps are bounded by the spawn ceiling. Pure
        // CapSet logic (no scheduler), so it runs here alongside the crypto tests,
        // before any cap-bearing cell spawns.
        #[cfg(not(cellos_posture = "release"))]
        if crate::task::p_trust_selftest::self_test() {
            log_info("P-TRUST self-test PASS (privileged path-caps ceiling-bounded)");
        } else {
            log_info("P-TRUST self-test FAIL");
        }
        // Boot ceiling: the per-path table is per-path (not a union) and no boot
        // cell is over-tightened out of the cap it needs. Runs BEFORE the first
        // Root spawn below, so a bad row is reported before it breaks a cell.
        #[cfg(not(cellos_posture = "release"))]
        if crate::loader::boot_ceiling::self_test() {
            log_info("boot-ceiling self-test PASS (per-path table, no union collapse)");
        } else {
            log_info("boot-ceiling self-test FAIL — a boot cell may lose caps");
        }
        // MMIO allowlist: the production table must still authorize its known
        // windows and must keep denying the DWC2 USB window (USB host authority
        // awaits policy v3 with a signed byte). Runs on the REAL allowlist.
        #[cfg(not(cellos_posture = "release"))]
        if crate::resource_registry::self_test() {
            log_info("mmio-allowlist self-test PASS (known windows live, DWC2 denied)");
        } else {
            log_info("mmio-allowlist self-test FAIL — DWC2 window or allowlist shape changed");
        }
        // Manifest v2: v1-upcast/v2-parse + the tier-floor invariant. Pure logic,
        // no scheduler — runs alongside the other crypto/trust self-tests.
        #[cfg(not(cellos_posture = "release"))]
        if crate::task::manifest_v2_selftest::self_test() {
            log_info("Manifest-v2 self-test PASS (v1 upcast + v2 parse + tier-floor)");
        } else {
            log_info("Manifest-v2 self-test FAIL");
        }

        // Layer-2 hardware security self-tests (MTE on aarch64, PKU on x86_64)
        // and the x86 VMM smokes. Runs here: after all HW init, before
        // scheduler + first cell spawn.
        arch::boot::fixtures_hardware();
        arch::boot::framebuffer_console(&console);

        // Spawn the Platform Cell before init (PCIe ECAM scanner).
        // Failure is non-fatal: kernel-side PCI_DEVICES stays empty; Driver Cells
        // that rely on sys_find_pcie_device will simply not find their device.
        if let Some(pci) = &pci {
            spawn_platform_cell(pci, &iommu);
        }

        // `aligned_elf::bytes` borrows an already aligned embedded image and only
        // materializes an aligned copy when the linker did not provide one. Do not
        // unconditionally duplicate init on the kernel heap at this boot boundary.
        #[cfg(feature = "test-hooks")]
        {
            log_info("ATOMIC_PUBLICATION_AP-15: arming trusted init");
            crate::loader::atomic_publication_tests::arm_trusted_success();
            log_info("ATOMIC_PUBLICATION_AP-15: armed for trusted init");
        }
        log_info("Publishing trusted init");
        #[cfg(feature = "p01-trace")]
        let init_result = crate::loader::spawn_bootstrap_init(INIT_ELF);
        #[cfg(not(feature = "p01-trace"))]
        let init_result = crate::loader::spawn_trusted_init(INIT_ELF);
        match init_result {
            Ok(tid) => {
                #[cfg(feature = "test-hooks")]
                crate::loader::atomic_publication_tests::finish_trusted_success(tid);
                log::info!(
                    "Successfully spawned init with complete root authority (tid={})",
                    tid
                );
                arch::boot::diag_probe(b'V');
            }
            Err(_e) => {
                log_info("Failed to spawn init");
                // Failure occurs before init reaches any ready queue.
                arch::boot::diag_probe(b'F');
                #[cfg(feature = "test-hooks")]
                panic!("atomic-publication trusted-init success contract did not publish init");
            }
        }
    }

    arch::boot::fixtures_user_mode();

    log_info("Kernel initialization complete. Entering idle loop.");

    // 9. Start multitasking
    log_info("Starting scheduler...");

    // Quiet the shared console for interactive use. Kernel bring-up is done; the
    // remaining Info chatter is per-spawn noise ([loader] SpawnFromPath, Spawn:,
    // ELF LOAD) that floods the UART and buries the shell prompt. WARN/ERROR still
    // surface real problems. Raise back to Info when debugging the spawn path.

    // Enable interrupts before entering the idle loop.
    irq.enable(clock);

    // Test images keep kernel Info live.
    //
    // The quieting above exists for the interactive console, and production keeps
    // it. A test image cannot: the witnesses a domain-entry lane reads are emitted
    // *after* init and the cells are running, which is the point of the lane. On
    // AArch64 the boot happens to never reach this line with a task pending (the
    // interrupt enable just above drops straight into the scheduler), so its lane
    // has always read Info-level witnesses (`S22-AARCH64-DOMAIN-LIVE`,
    // `DOMAIN-FRAME-RELEASE`). On x86_64 the boot does continue into the idle loop,
    // so the same class of witnesses (`S22-X86-DOMAIN-LIVE`,
    // `S22-X86-DOMAIN-TEARDOWN`, `[selftest] DOMAIN-FRAME-RELEASE`) would be
    // silenced here. `test-hooks` only — no production image is affected.
    //
    // It *replaces* the quieting rather than undoing it afterwards: which of the
    // two statements ran first used to be a race — kmain's tail executes only
    // after the scheduler hands the boot context back — and a witness emitted
    // before the quieting landed was dropped, so the same image could lose or keep
    // a marker between runs.
    // AArch64 test images are in the same position as x86_64's, and for a
    // sharper reason: on one CPU the boot happens to never reach this line with a
    // task pending, so the witnesses were emitted *before* the quieting by luck.
    // The moment a second hart is online the boot does reach it and the
    // `S22-AARCH64-DOMAIN-*` witnesses disappear — a lane reading them fails on
    // an image that is working. Quieting is a console policy for interactive and
    // production boots; a test image is read by its own lane.
    // An explicit [logging] is authoritative for the whole cold boot. Only old
    // images / omitted sections retain this later interactive/test default.
    if !system_config::logging_explicit() {
        log::set_max_level(arch::boot::quiet_log_level());
    }

    arch::boot::idle_probes();
    loop {
        crate::task::yield_cpu();
        crate::hal::ARCH.wait_for_interrupt();
    }
}

/// Write `s` as one `[INFO]` line on the early console.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn log_info(s: &str) {
    arch::boot::puts("[INFO] ");
    arch::boot::puts(s);
    arch::boot::puts("\n");
}

/// The boot banner. CI greps for the first line.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn boot_banner() {
    // Stable banner — CI greps for this exact string.
    arch::boot::puts("[Cellos] kernel boot v");
    arch::boot::puts(env!("CARGO_PKG_VERSION"));
    arch::boot::puts("\n");
    arch::boot::puts("Kernel started (Hart: 0, DTB: ...)\n");
}

/// The bootloader's memory description (Limine, else the architecture's
/// fallback) and the frame allocator built from its usable ranges.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn boot_memory(dtb: usize) -> (boot::BootInfoSource, memory::frame::FrameAllocator) {
    // Parse bootloader information
    let boot_info = match boot::parse_bootloader_info() {
        Ok(info) => boot::BootInfoSource::Limine(info),
        // Limine failed: use fallback (SimpleBootInfo)
        Err(_) => {
            log_info("Limine not found, using QEMU/OpenSBI fallback");
            // aarch64-virt sizes the kernel region from the linker end symbol —
            // EMBEDDED_OVERRIDE images make the binary size unbounded.
            boot::BootInfoSource::Fallback(boot::fallback_boot_info(dtb))
        }
    };
    // Log physical base — non-default value confirms KASLR is active.
    {
        arch::boot::puts("[boot] kernel_phys_base=0x");
        let mut base = boot_info.get().kernel_base();
        let digits = b"0123456789abcdef";
        let mut hex_buf = [b'0'; 16];
        for i in (0..16).rev() {
            hex_buf[i] = digits[base & 0xf];
            base >>= 4;
        }
        if let Ok(s) = core::str::from_utf8(&hex_buf) {
            arch::boot::puts(s);
        }
        arch::boot::puts("\n");
    }

    // Initialize kernel subsystems

    // 1. Memory Management
    // Get memory map from Boot Info (Converted to Cellos format)
    let mmap_entries = boot_info.get().memory_map();

    // Initialize the frame allocator with every usable range in the map.
    let frame_allocator = memory::frame::FrameAllocator::new_from_map(mmap_entries);
    log::info!(
        "[boot] allocator: {} usable range(s), {} frames ({} MiB)",
        frame_allocator.managed_ranges().count(),
        frame_allocator.total_frames(),
        frame_allocator.total_frames() * 4096 / (1024 * 1024)
    );
    for range in frame_allocator.managed_ranges() {
        log::info!(
            "[boot]   range {:#x}..{:#x} ({} MiB)",
            range.start,
            range.start + range.frames * 4096,
            range.frames * 4096 / (1024 * 1024)
        );
    }
    (boot_info, frame_allocator)
}

/// Spawn the Platform Cell (PCIe ECAM scanner) for `pci`. It is the first cell
/// that can bring up DMA-capable drivers, so it takes the DMA isolation state
/// the platform phase left (`_iommu`).
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn spawn_platform_cell(pci: &boot::PciReady, _iommu: &boot::IommuReady) {
    match pci {
        // RISC-V uses the compile-time GPEX window (0x3000_0000).
        boot::PciReady::Gpex => match crate::loader::spawn_from_path(
            "/bin/platform",
            crate::loader::SpawnRequest::governed_boot(),
        ) {
            Ok(_) => log_info("Platform Cell spawned (PCIe ECAM scanner)"),
            Err(_) => log_info("Platform Cell absent — PCIe BARs will not be pre-registered"),
        },
        // x86_64 hands the validated runtime ACPI-MCFG base and bus range via argv.
        boot::PciReady::Ecam {
            base,
            bus_start,
            bus_end,
        } => {
            let argv_str = alloc::format!(
                "--ecam-base={:#x} --bus-start={} --bus-end={}",
                base,
                bus_start,
                bus_end,
            );
            match crate::loader::spawn_from_path(
                "/bin/platform",
                crate::loader::SpawnRequest::governed_boot().with_argv(argv_str.into_bytes()),
            ) {
                Ok(_) => log_info("Platform Cell spawned (x86 PCIe ECAM scanner)"),
                Err(_) => log_info("Platform Cell absent — PCIe BARs will not be pre-registered"),
            }
        }
    }
}

/// Kernel entry point called from HAL boot code (not yet split into the
/// `crate::arch::boot` phases on this architecture).
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
#[no_mangle]
pub extern "C" fn kmain(hartid: usize, dtb: usize) -> ! {
    #[cfg(target_arch = "riscv64")]
    crate::sched::smp::set_boot_physical_hart(hartid);
    #[cfg(not(target_arch = "riscv64"))]
    let _hartid = hartid;
    let dtb = boot::effective_dtb(dtb);
    cpu_features::detect(dtb);
    // Parse DTB for MMIO bases before any driver or paging init.
    #[cfg(target_arch = "riscv64")]
    crate::platform::init(dtb);
    // Set runtime PLIC base before the later RV64 PLIC initialization consumes it.
    #[cfg(target_arch = "riscv64")]
    crate::platform::with(|p| hal::common::plic::set_plic_base(p.plic_base));
    // 0. Initialize UART immediately for early logging
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    crate::dev::uart::init();
    #[cfg(target_arch = "arm")]
    crate::hal::uart_pl011::init();
    #[cfg(target_arch = "x86")]
    crate::hal::uart_16550::init();

    // 1. Initialize HAL (Architecture specific) - Early Trap Setup
    hal::ARCH.init();
    // Define puts helper — arch-specific character output.
    let puts = |s: &str| {
        for c in s.bytes() {
            #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
            {
                let _ = crate::hal::sbi::console_putchar(c);
            }
            #[cfg(target_arch = "arm")]
            {
                crate::hal::uart_pl011::putchar(c);
            }
            #[cfg(target_arch = "x86")]
            {
                crate::hal::uart_16550::putchar(c);
            }
        }
    };

    // Restore log_info helper
    let log_info = |s: &str| {
        puts("[INFO] ");
        puts(s);
        puts("\n");
    };

    // Stable banner — CI greps for this exact string.
    puts("[Cellos] kernel boot v");
    puts(env!("CARGO_PKG_VERSION"));
    puts("\n");
    puts("Kernel started (Hart: 0, DTB: ...)\n");
    #[cfg(target_arch = "riscv64")]
    if cpu_features::has_h_ext() {
        puts("[cpu] H-extension: detected\n");
    } else {
        puts("[cpu] H-extension: not present\n");
    }

    // Parse bootloader information
    let boot_info_result = boot::parse_bootloader_info();

    // Check if Limine failed, if so, use fallback (SimpleBootInfo)
    let boot_info: &dyn BootInfo = match &boot_info_result {
        Ok(info) => info,
        Err(_) => {
            log_info("Limine not found, using QEMU/OpenSBI fallback");
            // aarch64-virt sizes the kernel region from the linker end symbol —
            // EMBEDDED_OVERRIDE images make the binary size unbounded.
            boot::fallback_boot_info(dtb)
        }
    };
    // Log physical base — non-default value confirms KASLR is active.
    {
        puts("[boot] kernel_phys_base=0x");
        let mut base = boot_info.kernel_base();
        let digits = b"0123456789abcdef";
        let mut hex_buf = [b'0'; 16];
        for i in (0..16).rev() {
            hex_buf[i] = digits[base & 0xf];
            base >>= 4;
        }
        if let Ok(s) = core::str::from_utf8(&hex_buf) {
            puts(s);
        }
        puts("\n");
    }

    // Initialize kernel subsystems

    // 1. Memory Management
    // Get memory map from Boot Info (Converted to Cellos format)
    let mmap_entries = boot_info.memory_map();

    // Initialize the frame allocator with every usable range in the map.
    let frame_allocator = memory::frame::FrameAllocator::new_from_map(mmap_entries);
    log::info!(
        "[boot] allocator: {} usable range(s), {} frames ({} MiB)",
        frame_allocator.managed_ranges().count(),
        frame_allocator.total_frames(),
        frame_allocator.total_frames() * 4096 / (1024 * 1024)
    );
    for range in frame_allocator.managed_ranges() {
        log::info!(
            "[boot]   range {:#x}..{:#x} ({} MiB)",
            range.start,
            range.start + range.frames * 4096,
            range.frames * 4096 / (1024 * 1024)
        );
    }

    // Targets that already have usable memory attributes (or intentionally run
    // without an MMU) can publish the allocator immediately. AArch64 must keep
    // it local until paging is active because its Spinlock uses exclusive
    // atomics that require Normal memory attributes; RV64 follows the same
    // build-then-publish lifecycle.
    #[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm",))]
    unsafe {
        core::ptr::write(
            &mut *memory::frame::FRAME_ALLOCATOR.lock(),
            Some(frame_allocator),
        );
    }
    #[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm",))]
    log_info("Frame allocator initialized");

    // 3. Paging (Virtual Memory) Setup
    // x86_64 bring-up: Limine's PML4 already maps RAM via HHDM and the kernel
    // at 0xFFFFFFFF80000000. We skip building + activating our own page tables
    // until the full x86_64 port (Phase 09). init_kernel_paging uses physical
    // addresses as virtual pointers, which would fault under Limine's paging.
    #[cfg(not(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm",)))]
    {
        let mut frame_allocator = frame_allocator;
        log_info("Initializing paging...");
        let root_table_phys =
            memory::paging::init_kernel_paging(&mut frame_allocator, mmap_entries)
                .expect("Failed to initialize paging");
        log_info("Paging initialized");
        log_info("Activating paging...");
        unsafe {
            memory::paging::activate_paging(root_table_phys);
        }
        *memory::paging::KERNEL_ROOT.lock() = Some(root_table_phys);
        #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
        TEST_SAFE_ROOT.store(root_table_phys, core::sync::atomic::Ordering::Release);
        unsafe {
            core::ptr::write(
                &mut *memory::frame::FRAME_ALLOCATOR.lock(),
                Some(frame_allocator),
            );
        }
        log_info("Paging activated");
        log_info("Frame allocator initialized");
        // Set sstatus.SUM=1 so S-mode (kernel) can access USER-mapped pages throughout
        // the kernel lifetime. VirtIO/peripheral MMIO is mapped USER=1 for Driver Cells
        // (U-mode). Without SUM=1 the kernel's tech-debt VirtIO drivers fault at early-boot
        // MMIO init. In Cellos's SAS+LBI model security comes from Rust type safety, not
        // hardware USER-bit separation for kernel-vs-cell — SUM=1 is safe and intentional.
        #[cfg(target_arch = "riscv64")]
        crate::arch::user_access::boot_open();
    }
    // Bare physical: RV32 Nano (SATP=0), x86_32 (CR0.PG=0), AArch32 (MMU off).
    #[cfg(any(target_arch = "riscv32", target_arch = "x86", target_arch = "arm"))]
    {
        memory::paging::init_bare();
        log_info("Paging: bare physical");
    }

    // 4. Heap Allocator (Global) - MUST be after paging but before any allocations
    // 4 MiB = 1024 frames (reduced from 32 MiB / 8192 frames under Phase 01 memory footprint reduction).
    // Sized to hold kernel structures, scheduler state, IPC buffers, and cell signing scratch.
    #[cfg(target_pointer_width = "64")]
    const HEAP_FRAMES: usize = 8_192; // 32 MiB
    #[cfg(target_pointer_width = "32")]
    const HEAP_FRAMES: usize = 1_024; // 4 MiB
    let heap_start = {
        let mut allocator_guard = memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = allocator_guard
            .as_mut()
            .expect("Frame allocator not initialized");
        // One contiguous reservation: the heap below is initialized over
        // `HEAP_FRAMES * 4096` bytes starting at this frame, so a scattered
        // allocation would write over frames this kernel does not own. Refusing
        // here is the only safe outcome — never a smaller or fragmented heap.
        // The run is taken in the bitmap's range, right after the bitmap and the
        // early frames there: the shared `[bitmap, heap)` registration below
        // depends on that order.
        memory::frame::reserve_boot_heap(allocator, HEAP_FRAMES).expect(
            "OOM: no contiguous run for the boot heap in the frame bitmap's range — \
             refusing to boot with a fragmented heap",
        )
    };
    let heap_size = HEAP_FRAMES * 4096;
    // On x86_64, phys_to_virt adds HHDM offset (Limine maps RAM at HHDM+phys).
    // On RISC-V, phys_to_virt returns phys unchanged (identity-mapped before paging).
    let heap_virt = memory::frame::phys_to_virt(heap_start);
    unsafe {
        memory::heap::init_heap(heap_virt, heap_size);
    }
    log_info("Heap initialized");

    memory::rt_heap::init();
    log_info("RT heap initialized");

    // 5. Hardware Abstraction Layer (HAL) Initialization
    // GDT/IDT/SYSCALL already done at step 1. Initialize PLIC for RISC-V external IRQs.
    #[cfg(target_arch = "riscv64")]
    if crate::board::active().has_driver(cellos_boards::DriverId::PlicSifive) {
        if let Some((context, irqs, irq_count)) = crate::platform::riscv_plic_init_data() {
            crate::hal::common::plic::init(context, &irqs[..irq_count]);
        } else {
            log::warn!("[plic] no active RV64 context mapping; external IRQs stay disabled");
        }
    } else {
        log::info!("[plic] disabled by active board descriptor");
    }
    log_info("HAL initialized (PLIC enabled)");

    // 6. Logger & Drivers & FS
    // Registers the log backend on AArch64/ARM; RV64 and x86_64 registered it
    // at entry, so the call is a no-op there.
    crate::dev::uart::init();

    // Phase 07: reserve the warm-snapshot capture's scratch workspace from the
    // frame allocator — the capture stages its own stack and buffers there, so
    // they can never be inside a captured run.  After the frame allocator and
    // the heap (and now the log backend), before any cell or capture can run.
    // A no-op unless the build is `snapshot-qualified`; a build without a
    // workspace refuses every capture.
    boot::reserve_snapshot_scratch();
    #[cfg(target_arch = "riscv64")]
    crate::dev::uart::init_input();
    // RV32 Nano / x86_64 bring-up: skip VirtIO MMIO probing (PCIe transport not yet ported).
    // x86_64 gets VirtIO via the PCI path in virtio_pci::init() below.
    #[cfg(target_arch = "riscv64")]
    crate::dev::init();
    // PCIe ECAM scan + NVMe + e1000 + VirtIO PCI init.
    // ARM64 virt uses VirtIO MMIO (not PCIe); accessing 0x3F000000 on QEMU
    // virt 7+ triggers a Synchronous External Abort — skip on aarch64.
    #[cfg(target_arch = "riscv64")]
    {
        crate::dev::iommu::set_deferred_init_pending();
        // activate_isolation() is now called inside iommu::try_deferred_init() once the
        // IOMMU device has been registered by the Platform Cell. The call below is a no-op
        // (IOMMU not yet initialized at this point in boot).
        crate::dev::iommu::activate_isolation();
    }

    // Attempt warm boot from snapshot before any cell initialization.
    // RV32 Nano / x86_64 skip: no VirtIO block in bring-up.
    #[cfg(target_arch = "riscv64")]
    if snapshot::try_restore() {
        // try_restore() called yield_cpu() and should not return in a successful
        // warm boot.  If we reach here, fall through to cold boot as a safety net.
        log::warn!("[boot] snapshot restore returned unexpectedly → cold boot");
    }

    // Cross-check the on-disk MBR against the compiled-in partition layout
    // (warn-only — surfaces image/kernel drift at boot instead of as silent
    // corruption later).
    #[cfg(target_arch = "riscv64")]
    crate::loader::disk_layout::verify_mbr();

    // Probe the cell bootstrap table so SpawnFromPath works during init.
    // RV32 Nano / x86_64 bring-up: no disk.
    #[cfg(target_arch = "riscv64")]
    match crate::loader::early::EarlyLoader::probe() {
        Ok(()) => puts("[loader] cell bootstrap table loaded\n"),
        Err(_) => {
            puts("[loader] WARN: cell table not found — disk image may lack bootstrap section\n")
        }
    }

    // RV32 Nano: no FAT32 FS in bring-up.
    // x86_64 uses the ramdisk-backed embedded FS to serve cell ELFs via VIFS1.
    #[cfg(cellos_wide)]
    fs::init();

    #[cfg(cellos_wide)]
    system_config::load_from_vifs1();

    // Load + verify the signed operator policy (P5b) NOW: after VIFS1 is mounted,
    // before any cap-bearing cell spawns. Absent → dev-permissive (this G1 build);
    // invalid → fail-closed. Phase 04 folds policy::lookup into the spawn grant.
    #[cfg(cellos_wide)]
    policy::load_from_vifs1();

    // Phase 20: hot-migration state-transfer self-test.
    #[cfg(target_arch = "riscv64")]
    crate::cell::state_stash::self_test();

    log_info("Kernel subsystems initialized successfully.");

    // 7. Initialize Scheduler
    log_info("Initializing scheduler...");
    task::init();
    log_info("Scheduler initialized");
    #[cfg(feature = "test-hooks")]
    if crate::sched::scheduler::death_subscription_quota_self_test() {
        log_info("death-subscriber quota self-test PASS");
    } else {
        log_info("death-subscriber quota self-test FAIL");
    }
    // This test-only branch must precede every unrelated boot suite: its
    // terminal is the sole evidence for opcode 214's SAS ownership path.
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "getrandom-sas-test"
    ))]
    {
        crate::sched::smp::start_secondaries();
        // Erase the diverging return type so the generic boot path remains
        // typechecked without a feature-specific unreachable-code warning.
        let exit = |success| -> () { crate::qemu_exit(success) };
        exit(task::user_copy_tests::run_getrandom_primary());
    }

    #[cfg(feature = "test-hooks")]
    if crate::resource_registry::pcie_bar_window_self_test() {
        log_info("[selftest] PCIE-BAR-WINDOW: PASS (bounded, aligned, overflow-safe)");
    } else {
        log_info("[selftest] PCIE-BAR-WINDOW: FAIL");
    }

    // The loader corpus exercises real spawn_gated denials and snapshots the
    // scheduler around every malformed image. Run it after task::init(), while
    // the scheduler is available but still empty and before secondary harts or
    // unrelated boot tasks can add noise. Assertions make any failure boot-fatal.
    #[cfg(feature = "test-hooks")]
    crate::loader::elf_tests::run_all();
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if crate::arch::task::path_selftest::self_test() {
        log_info("cwd-path self-test PASS");
    } else {
        log_info("cwd-path self-test FAIL");
    }
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if crate::arch::task::fstat_selftest::self_test() {
        log_info("fstat self-test PASS");
    } else {
        log_info("fstat self-test FAIL");
    }

    // 7a. Trust-model self-tests (thread identity inheritance + honest revoke).
    // Runs HERE — after the scheduler exists but BEFORE secondaries start — so the
    // synthetic thread it spawns cannot be raced onto another hart before teardown.
    // Fixtures that drive the live scheduler: never compiled into a release
    // kernel (Spec 15 §3.5; architecture remediation Phase 09).
    #[cfg(all(not(cellos_posture = "release"), cellos_wide))]
    {
        if task::thread_cap_selftest::self_test() {
            log_info("thread-cap self-test PASS (thread-inherit + honest-revoke + spawn bound)");
        } else {
            log_info("thread-cap self-test FAIL");
        }
        if task::grant_reclaim_selftest::self_test() {
            log_info("grant-reclaim self-test PASS (owned reclaimed, received kept, pin withheld)");
        } else {
            log_info("grant-reclaim self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::mmio_revoke_selftest::self_test() {
            log_info("mmio-revoke self-test PASS (class selective, user access removed)");
        } else {
            log_info("mmio-revoke self-test FAIL");
        }
        if task::thread_quota_selftest::self_test() {
            log_info("thread-quota self-test PASS (charged, released, enforced)");
        } else {
            log_info("thread-quota self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::task_id_selftest::self_test() {
            log_info("task-id-reuse self-test PASS (monotonic across spawn/exit)");
        } else {
            log_info("task-id-reuse self-test FAIL");
        }
        match task::thread_user_entry_selftest::self_test() {
            Some(true) => log_info("thread-user-entry self-test PASS (U-mode entry+arg+exit)"),
            Some(false) => log_info("thread-user-entry self-test FAIL"),
            None => log_info("thread-user-entry self-test SKIP (RV64 runtime gate only)"),
        }
        if task::completion_selftest::self_test() {
            log_info("completion-queue self-test PASS (reserve, land, bound, defer)");
        } else {
            log_info("completion-queue self-test FAIL");
        }
        if task::net_rx_selftest::self_test() {
            log_info("net-rx-reservation self-test PASS (fill, remember, release)");
        } else {
            log_info("net-rx-reservation self-test FAIL");
        }
        if task::ipc_pending_selftest::self_test() {
            log_info("ipc-pending self-test PASS (deferred delivery, bounds, quota)");
        } else {
            log_info("ipc-pending self-test FAIL");
        }
        task::cap_file_selftest::run();
        log_info("cap-file self-test PASS (read-only enforcement)");
        if task::ipc_guardrail_selftest::self_test() {
            log_info("ipc-guardrail self-test PASS (dead-peer wake, RecvScatter isolated)");
        } else {
            log_info("ipc-guardrail self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::vfs_lifecycle_selftest::self_test() {
            log_info("vfs-lifetime self-test PASS (exact lease, quarantine, owner watch)");
        } else {
            log_info("vfs-lifetime self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack::stack_probe_self_test() {
            log_info(
                "stack-probe self-test PASS (two guards, overflow target unmapped, watermark)",
            );
        } else {
            log_info("stack-probe self-test FAIL");
        }
        #[cfg(feature = "test-hooks")]
        if task::stack_sizing_policy_self_test() {
            log_info("stack-sizing policy self-test PASS (measured=16, unknown=64)");
        } else {
            log_info("stack-sizing policy self-test FAIL");
        }
    }

    // Bring actual secondaries online only after task::init. x86 uses the
    // Limine MP callback and qualifies bidirectional invalidation before tasks.
    #[cfg(cellos_wide)]
    crate::sched::smp::start_secondaries();

    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::arch::memory::tlb_shootdown_selftest::run_primary();
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::arch::cell::provenance_selftest::run_primary();
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    crate::memory::address_space::address_space_tests::run_primary();

    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::loader::atomic_publication_tests::run_governed_success_after_secondaries();
    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::activate();
    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::register_static_image()
        .expect("kernel static ranges must be disjoint");
    #[cfg(all(feature = "native-domains", cellos_wide))]
    memory::domain_supervisor_registry::register(
        heap_virt,
        heap_virt
            .checked_add(heap_size)
            .expect("kernel heap range overflow"),
        memory::domain_supervisor_registry::SupervisorRangeKind::KernelHeap,
        memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
    )
    .expect("kernel heap registration must be unique");
    #[cfg(all(feature = "native-domains", cellos_wide))]
    if let Some(guard) = memory::frame::FRAME_ALLOCATOR.lock().as_ref() {
        // `[bitmap, heap)`: the bitmap, plus any early boot frames the next-fit
        // cursor handed out from the bitmap's range before `reserve_boot_heap`
        // placed the heap there. When the bitmap's range is not the first
        // usable one (UEFI maps: ≥ 1 GiB under VMware, 4 GiB under OVMF), the
        // early paging frames come from lower ranges and the span is just the
        // bitmap.
        let (bm_start, _) = guard.bitmap_range();
        memory::domain_supervisor_registry::register(
            bm_start,
            heap_virt,
            memory::domain_supervisor_registry::SupervisorRangeKind::StaticWritable,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        )
        .expect("frame allocator bitmap and early paging registration must succeed");
    }
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    {
        let (clint_base, plic_base, plic_size, uart_region, rtc_region) =
            crate::platform::with(|p| {
                (
                    p.clint_base,
                    p.plic_base,
                    p.plic_size,
                    p.uart_base & !0xFFFF,
                    p.rtc_base & !0xFFF,
                )
            });
        let _ = memory::domain_supervisor_registry::register(
            clint_base,
            clint_base + 0x10000,
            memory::domain_supervisor_registry::SupervisorRangeKind::DeviceMmio,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        );
        let _ = memory::domain_supervisor_registry::register(
            plic_base,
            plic_base + plic_size,
            memory::domain_supervisor_registry::SupervisorRangeKind::DeviceMmio,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        );
        if uart_region != 0 {
            let _ = memory::domain_supervisor_registry::register(
                uart_region,
                uart_region + 0x1000,
                memory::domain_supervisor_registry::SupervisorRangeKind::DeviceMmio,
                memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
            );
        }
        let _ = memory::domain_supervisor_registry::register(
            rtc_region,
            rtc_region + 0x1000,
            memory::domain_supervisor_registry::SupervisorRangeKind::DeviceMmio,
            memory::domain_supervisor_registry::SupervisorRangeOwner::SharedKernel,
        );
    }
    // Tier 2 admission posture (ADR-0019 §2.3). The backend alone does not
    // authorize admission. Fleet profiles stay closed unless the explicit x86
    // browser profile is selected; that profile also narrows every publication
    // to the reviewed QuickJS artifact and requires its signed policy row.
    #[cfg(all(
        feature = "native-domains",
        cellos_wide,
        not(any(
            feature = "policy-required",
            feature = "production-relay-image",
            feature = "browser-engine-profile"
        ))
    ))]
    {
        if crate::loader::domain_admission::enable_for_boot() {
            log_info("Tier 2 admission: ENABLED (development profile)");
        } else {
            // Phase-01: an unqualified raw switch ordering keeps the development
            // profile closed, and the posture has to be visible — the alternative
            // is a silently disabled feature that reads as verified.
            log_info(
                "Tier 2 admission: DISABLED (development profile, phase-02 switch-ordering gate) \
                 — domain-class artifacts are denied",
            );
        }
    }
    #[cfg(all(
        feature = "native-domains",
        cellos_wide,
        any(feature = "policy-required", feature = "production-relay-image")
    ))]
    {
        log_info("Tier 2 admission: DISABLED (fleet profile) — domain-class artifacts are denied");
    }
    #[cfg(all(cellos_wide, feature = "native-domains", feature = "test-hooks"))]
    if !crate::sched::domain_switch_tests::run_primary() {
        log::error!("native-domain SAS scheduler fixture failed");
    }
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    crate::arch::task::context_handoff_selftest::run_primary();
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    crate::arch::task::retirement_selftest::run_primary();
    #[cfg(all(
        target_arch = "riscv64",
        feature = "native-domains",
        feature = "test-hooks"
    ))]
    {
        task::user_copy_tests::run_primary();
        task::ipc_wire_selftest::run_primary(crate::sched::smp::online_hart_count());
        crate::loader::domain_admission::run_selftest();
        crate::ipc::futex::run_selftest();
        task::domain_grant::run_selftest();
        task::grant_gate_selftest::run_primary();
        task::rt_wake_selftest::self_test();
    }

    // Page-table teardown primitives behind runtime cap revoke (`.agents/260712-1901`
    // P01). The tables are plain kernel-heap memory, so this needs no IOMMU
    // hardware and runs on every arch whose lane enables hooks.
    #[cfg(feature = "test-hooks")]
    crate::dev::iommu_pt::run_selftest();

    // Phase 07 step 3: the all-hart park hook, driven on the real harts of this
    // boot. The fixture requests, verifies and releases a park through the same
    // protocol and the same trap-path hook the capture preflight uses, and emits
    // `S22-RV64-PARK: PASS harts=<n>` (or `FAIL`) itself — the native-domain lane
    // requires that terminal, so a fixture that stops running, or a call site that
    // stops being reached, turns the lane red instead of silently disappearing.
    // Placed with the other boot fixtures: after every fixture that needs the
    // second hart (the park is a bounded stop, not a background stall) and before
    // init exists, so it has the machine to itself.
    #[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
    if !crate::sched::quiesce::lane::run_primary() {
        // The fixture's own marker is the verdict: either `S22-RV64-PARK: FAIL`
        // (a step of the witness did not hold) or `S22-RV64-PARK: UNAVAILABLE`
        // (the readiness gate found the second hart not taking traps before any
        // request was made, so this boot could not witness the hook at all).
        log::error!("park-hook fixture did not witness the park hook; see its marker");
    }

    // Phase 10: the timer sweep runs on any hart, so a deadline passes while
    // hart 0 is held. Last of the boot fixtures:
    // it needs the secondaries online and leaves a retiring task behind.
    #[cfg(all(feature = "test-hooks", cellos_wide))]
    crate::sched::off_hart0_timeout_selftest::run_primary();

    // 8. Spawn Embedded Init
    // RV32 Nano bring-up: no init binary — boot to idle loop.
    #[cfg(cellos_wide)]
    {
        log_info("Spawning Embedded Init...");

        // Power-on self-test of the Ed25519 verify primitive (RFC 8032 TEST 1 +
        // tamper-negative) before it is trusted to authenticate the signed
        // operator policy (P5). Cheap (~one verify); a FAIL means the crypto path
        // is broken and signed policy must not be trusted.
        if crate::ed25519::self_test() {
            log_info("ed25519 verify self-test PASS (RFC 8032 + tamper)");
        } else {
            log_info("ed25519 verify self-test FAIL — signed policy unsafe");
        }
        // Power-on self-test of the signed-policy path: verify + parse a known
        // dev-signed blob, and confirm a tampered blob is rejected.
        if crate::policy::self_test() {
            log_info("policy verify+parse self-test PASS (signed blob + tamper)");
        } else {
            log_info("policy verify+parse self-test FAIL");
        }
        // Cell binary signing self-test: known-good vector verifies + flipped
        // payload is rejected. Runs before any spawn_from_path is called.
        if crate::signing::self_test() {
            log_info("cell signing self-test PASS");
        } else {
            log_info("cell signing self-test FAIL — cell signature gate unsafe");
        }
        {
            let (posture, anchor) = crate::signing::trust_anchor();
            log::info!(
                "[signing] cell trust posture={} anchor={:02x?} unsigned={}",
                posture,
                anchor,
                if crate::signing::unsigned_admitted() {
                    "tier2"
                } else {
                    "denied"
                }
            );
        }
        #[cfg(feature = "test-hooks")]
        if crate::admission::self_test() {
            log_info("admission-core self-test PASS (fail-closed A/B floor model)");
        } else {
            log_info("admission-core self-test FAIL");
        }
        // P-TRUST: privileged path-caps are bounded by the spawn ceiling. Pure
        // CapSet logic (no scheduler), so it runs here alongside the crypto tests,
        // before any cap-bearing cell spawns.
        #[cfg(not(cellos_posture = "release"))]
        if crate::task::p_trust_selftest::self_test() {
            log_info("P-TRUST self-test PASS (privileged path-caps ceiling-bounded)");
        } else {
            log_info("P-TRUST self-test FAIL");
        }
        // Boot ceiling: the per-path table is per-path (not a union) and no boot
        // cell is over-tightened out of the cap it needs. Runs BEFORE the first
        // Root spawn below, so a bad row is reported before it breaks a cell.
        #[cfg(not(cellos_posture = "release"))]
        if crate::loader::boot_ceiling::self_test() {
            log_info("boot-ceiling self-test PASS (per-path table, no union collapse)");
        } else {
            log_info("boot-ceiling self-test FAIL — a boot cell may lose caps");
        }
        // MMIO allowlist: the production table must still authorize its known
        // windows and must keep denying the DWC2 USB window (USB host authority
        // awaits policy v3 with a signed byte). Runs on the REAL allowlist.
        #[cfg(not(cellos_posture = "release"))]
        if crate::resource_registry::self_test() {
            log_info("mmio-allowlist self-test PASS (known windows live, DWC2 denied)");
        } else {
            log_info("mmio-allowlist self-test FAIL — DWC2 window or allowlist shape changed");
        }
        // Manifest v2: v1-upcast/v2-parse + the tier-floor invariant. Pure logic,
        // no scheduler — runs alongside the other crypto/trust self-tests.
        #[cfg(not(cellos_posture = "release"))]
        if crate::task::manifest_v2_selftest::self_test() {
            log_info("Manifest-v2 self-test PASS (v1 upcast + v2 parse + tier-floor)");
        } else {
            log_info("Manifest-v2 self-test FAIL");
        }

        // Layer-2 hardware security self-tests (test-hooks feature only).
        // MTE (aarch64) and PKU (x86_64) — each prints PASS or SKIP.
        // Runs here: after all HW init, before scheduler + first cell spawn.
        #[cfg(feature = "test-hooks")]
        {}

        // Spawn the Platform Cell before init (PCIe ECAM scanner).
        // RISC-V uses the compile-time GPEX window (0x3000_0000).
        // x86_64 hands the validated runtime ACPI-MCFG base and bus range via argv.
        // Failure is non-fatal: kernel-side PCI_DEVICES stays empty; Driver Cells
        // that rely on sys_find_pcie_device will simply not find their device.
        #[cfg(target_arch = "riscv64")]
        match crate::loader::spawn_from_path(
            "/bin/platform",
            crate::loader::SpawnRequest::governed_boot(),
        ) {
            Ok(_) => log_info("Platform Cell spawned (PCIe ECAM scanner)"),
            Err(_) => log_info("Platform Cell absent — PCIe BARs will not be pre-registered"),
        }

        // `aligned_elf::bytes` borrows an already aligned embedded image and only
        // materializes an aligned copy when the linker did not provide one. Do not
        // unconditionally duplicate init on the kernel heap at this boot boundary.
        #[cfg(feature = "test-hooks")]
        {
            log_info("ATOMIC_PUBLICATION_AP-15: arming trusted init");
            crate::loader::atomic_publication_tests::arm_trusted_success();
            log_info("ATOMIC_PUBLICATION_AP-15: armed for trusted init");
        }
        log_info("Publishing trusted init");
        #[cfg(feature = "p01-trace")]
        let init_result = crate::loader::spawn_bootstrap_init(INIT_ELF);
        #[cfg(not(feature = "p01-trace"))]
        let init_result = crate::loader::spawn_trusted_init(INIT_ELF);
        match init_result {
            Ok(tid) => {
                #[cfg(feature = "test-hooks")]
                crate::loader::atomic_publication_tests::finish_trusted_success(tid);
                log::info!(
                    "Successfully spawned init with complete root authority (tid={})",
                    tid
                );
            }
            Err(_e) => {
                log_info("Failed to spawn init");
                #[cfg(feature = "test-hooks")]
                panic!("atomic-publication trusted-init success contract did not publish init");
            }
        }
    }

    // Ring-3 smoke test: spawn a minimal U-mode task that logs and exits.
    // RISC-V only — task writes RISC-V machine code directly.
    // Expected serial output: "Hi from U-mode!\n" followed by task exit.
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    match task::user_hello::spawn() {
        Ok(tid) => {
            puts("[task] spawning user_hello at ");
            // Print tid as decimal (max 20 digits for usize)
            let mut buf = [0u8; 20];
            let mut n = tid;
            let mut i = 20usize;
            if n == 0 {
                i -= 1;
                buf[i] = b'0';
            } else {
                while n > 0 {
                    i -= 1;
                    buf[i] = b'0' + (n % 10) as u8;
                    n /= 10;
                }
            }
            let _ = core::str::from_utf8(&buf[i..]).map(puts);
            puts("\n");
            let _ = tid; // suppress unused warning
        }
        Err(_) => log_info("[task] user_hello spawn failed"),
    }

    // Deliberately underflow a U-mode test task into the upper guard page. The
    // fault handler must terminate only that task and let boot continue.
    #[cfg(all(target_arch = "riscv64", feature = "test-hooks"))]
    if crate::arch::task::stack_overflow_probe::spawn().is_err() {
        log_info("[stack-guard] deliberate overflow probe spawn FAIL");
    }

    log_info("Kernel initialization complete. Entering idle loop.");

    // 9. Start multitasking
    log_info("Starting scheduler...");

    // Quiet the shared console for interactive use. Kernel bring-up is done; the
    // remaining Info chatter is per-spawn noise ([loader] SpawnFromPath, Spawn:,
    // ELF LOAD) that floods the UART and buries the shell prompt. WARN/ERROR still
    // surface real problems. Raise back to Info when debugging the spawn path.

    // Enable interrupts before entering the idle loop.
    crate::arch::irq::enable();

    // Test images keep kernel Info live.
    //
    // The quieting above exists for the interactive console, and production keeps
    // it. A test image cannot: the witnesses a domain-entry lane reads are emitted
    // *after* init and the cells are running, which is the point of the lane. On
    // AArch64 the boot happens to never reach this line with a task pending (the
    // interrupt enable just above drops straight into the scheduler), so its lane
    // has always read Info-level witnesses (`S22-AARCH64-DOMAIN-LIVE`,
    // `DOMAIN-FRAME-RELEASE`). On x86_64 the boot does continue into the idle loop,
    // so the same class of witnesses (`S22-X86-DOMAIN-LIVE`,
    // `S22-X86-DOMAIN-TEARDOWN`, `[selftest] DOMAIN-FRAME-RELEASE`) would be
    // silenced here. `test-hooks` only — no production image is affected.
    //
    // It *replaces* the quieting rather than undoing it afterwards: which of the
    // two statements ran first used to be a race — kmain's tail executes only
    // after the scheduler hands the boot context back — and a witness emitted
    // before the quieting landed was dropped, so the same image could lose or keep
    // a marker between runs.
    // AArch64 test images are in the same position as x86_64's, and for a
    // sharper reason: on one CPU the boot happens to never reach this line with a
    // task pending, so the witnesses were emitted *before* the quieting by luck.
    // The moment a second hart is online the boot does reach it and the
    // `S22-AARCH64-DOMAIN-*` witnesses disappear — a lane reading them fails on
    // an image that is working. Quieting is a console policy for interactive and
    // production boots; a test image is read by its own lane.
    // An explicit [logging] is authoritative for the whole cold boot. Only old
    // images / omitted sections retain this later interactive/test default.
    if !system_config::logging_explicit() {
        log::set_max_level(log::LevelFilter::Warn);
    }

    loop {
        crate::task::yield_cpu();
        crate::hal::ARCH.wait_for_interrupt();
    }
}

/// Emit an allocation-free RV64 fault snapshot before the panic path.
#[cfg(all(feature = "test-hooks", target_arch = "riscv64"))]
#[no_mangle]
pub extern "C" fn vi_rv64_kernel_fault_snapshot(satp: usize, stval: usize) {
    fn put_hex(value: usize) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for shift in (0..16).rev().map(|n| n * 4) {
            let _ = crate::hal::sbi::console_putchar(HEX[(value >> shift) & 0xf]);
        }
    }

    fn put_label(label: &[u8], value: usize) {
        for byte in label {
            let _ = crate::hal::sbi::console_putchar(*byte);
        }
        put_hex(value);
    }

    unsafe fn read_pte(table_phys: usize, index: usize) -> usize {
        (table_phys as *const usize).add(index).read_volatile()
    }

    fn next_table(entry: usize) -> usize {
        ((entry >> 10) & 0x003f_ffff_ffff_ffff) << 12
    }

    for byte in b"\n[selftest] RV64-FAULT satp=0x" {
        let _ = crate::hal::sbi::console_putchar(*byte);
    }
    put_hex(satp);
    for byte in b" stval=0x" {
        let _ = crate::hal::sbi::console_putchar(*byte);
    }
    put_hex(stval);

    let kernel_root = TEST_SAFE_ROOT.load(core::sync::atomic::Ordering::Acquire);
    if kernel_root != 0 {
        let kernel_satp = (8usize << 60) | (kernel_root >> 12);
        // SAFETY: this test-only panic snapshot switches to the known-safe
        // shared root so it can inspect private page-table memory by physical VA.
        unsafe {
            core::arch::asm!(
                "csrw satp, {satp}",
                "sfence.vma zero, zero",
                satp = in(reg) kernel_satp,
                options(nostack)
            );
        }
        let root_phys = (satp & ((1usize << 44) - 1)) << 12;
        let vpn2 = (stval >> 30) & 0x1ff;
        let vpn1 = (stval >> 21) & 0x1ff;
        let vpn0 = (stval >> 12) & 0x1ff;
        let pte2 = unsafe { read_pte(root_phys, vpn2) };
        let pte1 = if pte2 & 1 != 0 {
            unsafe { read_pte(next_table(pte2), vpn1) }
        } else {
            0
        };
        let pte0 = if pte1 & 1 != 0 {
            unsafe { read_pte(next_table(pte1), vpn0) }
        } else {
            0
        };
        put_label(b" pte2=0x", pte2);
        put_label(b" pte1=0x", pte1);
        put_label(b" pte0=0x", pte0);
    }
}

/// Non-recoverable kernel panic handler.
///
/// `current_cell_id` is allocation attribution, not an execution-mode proof:
/// kernel code can hold scheduler or other kernel locks while servicing a
/// Cell. Therefore this handler never schedules or retires a Cell. Only the
/// architecture trap path, after proving an interrupted U-mode context, may
/// enter the deferred retirement funnel.
#[cfg(not(all(test, not(target_os = "none"))))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // True kernel panic: print diagnostics and halt.
    #[inline(always)]
    fn panic_putchar(c: u8) {
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        {
            let _ = crate::hal::sbi::console_putchar(c);
        }
        #[cfg(target_arch = "aarch64")]
        {
            crate::hal::backend::Board::early_putc(c);
        }
        #[cfg(target_arch = "arm")]
        {
            crate::hal::uart_pl011::putchar(c);
        }
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            crate::hal::uart_16550::putchar(c);
        }
    }
    let puts = |s: &str| {
        for c in s.bytes() {
            panic_putchar(c);
        }
    };
    puts("\n[KERNEL PANIC] ");
    puts("Critical failure.\n");
    use core::fmt::Write;
    struct PanicWriter;
    impl core::fmt::Write for PanicWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for c in s.bytes() {
                panic_putchar(c);
            }
            Ok(())
        }
    }
    let _ = writeln!(PanicWriter, "{}", info);

    // Reboot or spin: RISC-V uses SBI SRST; ARM64 / x86_64 spin.
    puts("[KERNEL PANIC] halting...\n");
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
    // On riscv this is unreachable after system_reset.
    loop {
        crate::arch::cpu::halt();
    }
}
