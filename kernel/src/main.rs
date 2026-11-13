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
    crate::arch::power::qemu_exit(success)
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
    // The 32-bit targets spawn no cells, so nothing below reads these there.
    #[cfg(not(cellos_wide))]
    let _ = (console, pci, iommu);

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
fn log_info(s: &str) {
    arch::boot::puts("[INFO] ");
    arch::boot::puts(s);
    arch::boot::puts("\n");
}

/// The boot banner. CI greps for the first line.
fn boot_banner() {
    // Stable banner — CI greps for this exact string.
    arch::boot::puts("[Cellos] kernel boot v");
    arch::boot::puts(env!("CARGO_PKG_VERSION"));
    arch::boot::puts("\n");
    arch::boot::puts("Kernel started (Hart: 0, DTB: ...)\n");
}

/// The bootloader's memory description (Limine, else the architecture's
/// fallback) and the frame allocator built from its usable ranges.
fn boot_memory(dtb: usize) -> (boot::BootInfoSource, memory::frame::FrameAllocator) {
    // Parse bootloader information
    let boot_info = match boot::parse_bootloader_info() {
        Ok(info) => boot::BootInfoSource::Limine(info),
        // Limine failed: use fallback (SimpleBootInfo)
        Err(_) => {
            log_info("Limine not found, using QEMU/OpenSBI fallback");
            // aarch64-virt sizes the kernel region from the linker end symbol —
            // EMBEDDED_OVERRIDE images make the binary size unbounded.
            boot::BootInfoSource::Fallback(arch::boot::firmware::fallback_boot_info(dtb))
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
#[cfg(cellos_wide)]
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
    let puts = |s: &str| {
        for c in s.bytes() {
            crate::arch::boot::putc(c);
        }
    };
    puts("\n[KERNEL PANIC] ");
    puts("Critical failure.\n");
    use core::fmt::Write;
    struct PanicWriter;
    impl core::fmt::Write for PanicWriter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            for c in s.bytes() {
                crate::arch::boot::putc(c);
            }
            Ok(())
        }
    }
    let _ = writeln!(PanicWriter, "{}", info);

    // Reboot or spin: RISC-V uses SBI SRST; ARM64 / x86_64 spin.
    puts("[KERNEL PANIC] halting...\n");
    crate::arch::power::reset_after_panic();
    // On riscv this is unreachable after system_reset.
    loop {
        crate::arch::cpu::halt();
    }
}
