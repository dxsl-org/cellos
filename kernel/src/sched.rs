//! Scheduling mechanism (Spec 15 category A): ready queues and the context-switch
//! funnel, SMP bring-up, per-hart state, all-hart quiescence and CPU accounting.

#[cfg(feature = "g1-cpu-accounting")]
pub(crate) mod cpu_accounting;
#[cfg(all(
    feature = "native-domains",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) mod domain_switch;
#[cfg(all(
    feature = "native-domains",
    feature = "test-hooks",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) mod domain_switch_tests;
pub mod hart_local;
/// Phase 10 fixture: a timeout fires while hart 0 is held.
#[cfg(all(
    feature = "test-hooks",
    any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )
))]
pub(crate) mod off_hart0_timeout_selftest;
/// All-hart quiescence for the snapshot capture preflight (phase 07 step 3).
pub mod quiesce;
pub mod scheduler;
pub mod smp;
