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
    let before = snapshot();

    let Ok(prepared) = crate::task::prepare_elf_task(
        unaligned,
        "atomic-unaligned",
        types::CellId(0),
        alloc::vec::Vec::new(),
    ) else {
        return false;
    };
    drop(prepared);
    snapshot() == before
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
