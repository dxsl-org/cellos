use core::sync::atomic::{AtomicU8, Ordering};

use super::snapshot::snapshot;
#[cfg(target_arch = "riscv64")]
use super::success::{arm_pre_ready_success, arm_smp_success, skip_smp_success};
use super::success::{
    arm_trusted_success, finish_governed_success, finish_trusted_success,
    trusted_arming_completes_from_cell_context, GovernedSuccess,
};

const SUCCESS_PRE_READY: u8 = 0b001;
const SUCCESS_SMP: u8 = 0b010;
const SUCCESS_TRUSTED: u8 = 0b100;

static SUCCESS_PARTS: AtomicU8 = AtomicU8::new(0);

fn complete_success_part(part: u8) {
    if SUCCESS_PARTS.fetch_or(part, Ordering::AcqRel) | part == 0b111 {
        log::info!("ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED");
        log::info!("ATOMIC_PUBLICATION_ALL: PASS");
    }
}

/// Warm-up cycles the x86_64 ledger check is allowed before it must settle.
///
/// Chosen with headroom over the measured three (two spending a table each, the
/// third spending none): enough that an unrelated layout change cannot fail the
/// case, small enough that a real per-preparation leak exhausts it.
#[cfg(target_arch = "x86_64")]
const LEDGER_WARMUP_CYCLES: usize = 6;

fn unaligned_elf_preparation_restores_state() -> bool {
    let mut storage = alloc::vec![0u64; (crate::INIT_ELF.len() + 8) / 8];
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            storage.as_mut_ptr().cast::<u8>(),
            crate::INIT_ELF.len() + 1,
        )
    };
    bytes[1..].copy_from_slice(crate::INIT_ELF);
    let unaligned = &bytes[1..];
    if (unaligned.as_ptr() as usize).is_multiple_of(8) {
        return false;
    }

    // On x86_64 the ledger also carries the frame allocator's own identity-map
    // bookkeeping, and there it is created on demand: the kernel root has no
    // identity map of RAM (RAM is reached through the HHDM), so the first mapping
    // that touches a 2 MiB window of low RAM — a stack's pages, or a frame handed
    // back to the free list, which `release_frames` re-establishes at VA == PA —
    // allocates that window's page table, and the table stays while the frames
    // under it are free and identity-mapped. Measured 2026-10-08 on the failing
    // boot: two cycles spent one PT each for the windows at VA 0x400000-0x800000
    // plus their shared PD, a third cycle spent nothing.
    //
    // So warm the ledger to its fixed point before measuring. That cost is bounded
    // by RAM and paid once per window; a *per-preparation* leak is not, and would
    // keep moving the ledger on every cycle and exhaust this bound instead of
    // settling — which is why the bound is a failure, not a shrug. RISC-V and
    // AArch64 identity-map RAM during boot, so their first cycle is already on the
    // fixed point and they need no warm-up.
    // Captured before the measurement for the failure path: the frames that never
    // come back are the ones it names.
    let baseline_bitmap = super::snapshot::frame_bitmap();
    #[cfg(target_arch = "x86_64")]
    let ledger_settled = {
        let mut settled = false;
        for _ in 0..LEDGER_WARMUP_CYCLES {
            let before_warmup = super::snapshot::free_frame_count();
            let Ok(warmup) = crate::task::prepare_elf_task(
                unaligned,
                "atomic-unaligned",
                types::CellId(0),
                alloc::vec::Vec::new(),
            ) else {
                return false;
            };
            drop(warmup);
            if super::snapshot::free_frame_count() == before_warmup {
                settled = true;
                break;
            }
        }
        settled
    };
    #[cfg(not(target_arch = "x86_64"))]
    let ledger_settled = true;

    let before = snapshot();
    let cycle_before = super::snapshot::free_frame_count();

    let Ok(prepared) = crate::task::prepare_elf_task(
        unaligned,
        "atomic-unaligned",
        types::CellId(0),
        alloc::vec::Vec::new(),
    ) else {
        return false;
    };
    drop(prepared);
    let cycle_after = super::snapshot::free_frame_count();
    let after = snapshot();
    // Reported field-by-field rather than as a bare `==`: this case is the only
    // one in the corpus that does not go through `snapshot_matches`, and an
    // architecture whose first test-hooks boot runs it (x86_64, phase 02) has no
    // other way to name the field that moved.
    let matched =
        super::snapshot::snapshot_matches("ALIGNMENT", "unaligned-prepare", &before, &after);
    #[cfg(target_arch = "x86_64")]
    if !ledger_settled {
        log::error!(
            "ATOMIC_PUBLICATION_ALIGNMENT: ledger never settled in {} warm-up cycles — the last one left it at {}",
            LEDGER_WARMUP_CYCLES,
            super::snapshot::free_frame_count(),
        );
    }
    if !matched {
        log::error!(
            "ATOMIC_PUBLICATION_ALIGNMENT: measured cycle {} -> {} (delta {})",
            cycle_before,
            cycle_after,
            cycle_after as i64 - cycle_before as i64,
        );
        super::snapshot::report_frame_delta("ALIGNMENT", &baseline_bitmap);
    }
    matched && ledger_settled
}

pub(super) fn run_all() {
    assert!(
        unaligned_elf_preparation_restores_state(),
        "unaligned ELF preparation must be atomic and parser-safe",
    );
    log::info!("ATOMIC_PUBLICATION_ALIGNMENT: PASS");

    assert!(
        crate::memory::cell_quota::reusable_cell_id_contract(),
        "bounded CellId slots must exhaust only while live and be reusable after release",
    );
    log::info!("ATOMIC_PUBLICATION_CELL_ID_REUSE: PASS");

    assert!(
        super::baseline::populated_baseline_teardown_restores_state(),
        "populated atomic-publication fixture must restore only its owned state",
    );
    log::info!("ATOMIC_PUBLICATION_FIXTURE_ROUNDTRIP: PASS");

    let failures = super::denials::run();
    assert_eq!(failures, 0, "atomic-publication denial contracts failed");

    assert!(
        trusted_arming_completes_from_cell_context(),
        "atomic-publication trusted arming must complete from a Cell context",
    );
    log::info!("ATOMIC_PUBLICATION_ARMING: PASS");

    // AP-12 and AP-14 prove pre-ready completeness on a governed probe before
    // secondaries exist. AP-13 is deliberately not armed until hart 1 is online.
    #[cfg(target_arch = "riscv64")]
    {
        arm_pre_ready_success();
        super::spawn_governed_probe().expect("atomic-publication pre-ready probe must publish");
    }
}

/// AP-13 requires a live secondary scheduler. It gets its own probe so an SMP
/// barrier cannot be mistaken for the single-hart pre-ready observation.
#[cfg(target_arch = "riscv64")]
pub(super) fn run_governed_success_after_secondaries() {
    if !crate::task::smp::is_rt_hart_online() {
        skip_smp_success();
        log::info!("ATOMIC_PUBLICATION_AP-13: SKIP (hart 1 not online; SMP probe not required)");
        return;
    }
    arm_smp_success();
    super::spawn_governed_probe().expect("atomic-publication SMP probe must publish");
}

pub(super) fn finish_governed_success_case(tid: usize) {
    let Some((kind, passed)) = finish_governed_success(tid) else {
        return;
    };
    match kind {
        GovernedSuccess::PreReady => {
            for case in ["AP-12", "AP-14"] {
                if passed {
                    log::info!("ATOMIC_PUBLICATION_{}: PASS", case);
                } else {
                    log::error!("ATOMIC_PUBLICATION_{}: FAIL", case);
                }
            }
            assert!(
                passed,
                "atomic-publication pre-ready success contract failed"
            );
            crate::task::hart_local::ready::remove_from_all(tid);
            if let Some(scheduler) = crate::task::SCHEDULER.lock().as_mut() {
                scheduler.exit_task(tid, 0);
            }
            crate::task::yield_cpu();
            complete_success_part(SUCCESS_PRE_READY);
        }
        GovernedSuccess::Smp => {
            if passed {
                log::info!("ATOMIC_PUBLICATION_AP-13: PASS");
            } else {
                log::error!("ATOMIC_PUBLICATION_AP-13: FAIL");
            }
            assert!(passed, "atomic-publication SMP success contract failed");
            complete_success_part(SUCCESS_SMP);
        }
    }
}

pub(super) fn arm_trusted_success_case() {
    arm_trusted_success();
}

pub(super) fn finish_trusted_success_case(tid: usize) {
    let passed = finish_trusted_success(tid);
    if passed {
        log::info!("ATOMIC_PUBLICATION_AP-15: PASS");
        complete_success_part(SUCCESS_TRUSTED);
    } else {
        log::error!("ATOMIC_PUBLICATION_AP-15: FAIL");
    }
    assert!(
        passed,
        "atomic-publication trusted-init success contract failed"
    );
}
