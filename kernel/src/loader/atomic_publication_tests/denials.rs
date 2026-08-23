use super::baseline::with_populated_baseline;
use super::snapshot::{
    denied_with_full_rollback, denied_with_governed_rollback, denied_with_logged_denial,
    platform_singleton_denial,
};

const NO_AUDIT_RECORDS: usize = 0;
const LOGGED_DENIAL_AUDIT_RECORDS: usize = 1;
const GOVERNED_DENIAL_AUDIT_RECORDS: usize = 2;
const PLATFORM_SINGLETON_AUDIT_RECORDS: usize = 4;

fn record(case: &'static str, failures: &mut usize, contract: impl FnOnce() -> bool) {
    if contract() {
        log::info!("ATOMIC_PUBLICATION_{}: PASS", case);
    } else {
        *failures += 1;
        log::error!("ATOMIC_PUBLICATION_{}: FAIL", case);
    }
}

pub(super) fn run() -> usize {
    let mut failures = 0;
    record("AP-00", &mut failures, || {
        with_populated_baseline("AP-00", LOGGED_DENIAL_AUDIT_RECORDS, || {
            denied_with_logged_denial("AP-00", || {
                super::super::spawn_gated(
                    &[],
                    "/bin/atomic-malformed",
                    super::super::SpawnRequest::governed_boot(),
                )
                .map(|_| ())
            })
        })
    });
    for case in ["AP-01", "AP-02", "AP-03", "AP-04"] {
        record(case, &mut failures, || {
            with_populated_baseline(case, NO_AUDIT_RECORDS, || {
                denied_with_full_rollback(case, || {
                    crate::task::prepare_elf_task(
                        crate::INIT_ELF,
                        "atomic-denial",
                        types::CellId(0),
                        alloc::vec::Vec::new(),
                    )
                    .map(|_| ())
                })
            })
        });
    }
    record("AP-05", &mut failures, || {
        with_populated_baseline(
            "AP-05",
            PLATFORM_SINGLETON_AUDIT_RECORDS,
            platform_singleton_denial,
        )
    });
    record("AP-06", &mut failures, || {
        with_populated_baseline("AP-06", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-06", 2, || {
                super::spawn_governed_platform(super::super::SpawnRequest::governed_boot())
                    .map(|_| ())
            })
        })
    });
    for case in ["AP-07", "AP-08"] {
        record(case, &mut failures, || {
            with_populated_baseline(case, NO_AUDIT_RECORDS, || {
                denied_with_full_rollback(case, || {
                    super::super::spawn_trusted_init(crate::INIT_ELF).map(|_| ())
                })
            })
        });
    }
    record("AP-09", &mut failures, || {
        with_populated_baseline("AP-09", LOGGED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-09", 1, || {
                super::super::spawn_from_path(
                    "/bin/vfs",
                    super::super::SpawnRequest::governed_boot(),
                )
                .map(|_| ())
            })
        })
    });
    record("AP-10", &mut failures, || {
        with_populated_baseline("AP-10", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-10", 2, || {
                let mut request = super::super::SpawnRequest::governed_boot();
                request.priority = u8::MAX;
                super::spawn_governed_platform(request).map(|_| ())
            })
        })
    });
    record("AP-11", &mut failures, || {
        with_populated_baseline("AP-11", GOVERNED_DENIAL_AUDIT_RECORDS, || {
            denied_with_governed_rollback("AP-11", 2, || {
                let mut request = super::super::SpawnRequest::governed_boot();
                request.replacement =
                    Some(crate::cell::hotswap::ReplacementReservation::invalid_for_test());
                super::spawn_governed_platform(request).map(|_| ())
            })
        })
    });
    failures
}
