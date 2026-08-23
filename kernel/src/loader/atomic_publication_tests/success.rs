use core::sync::atomic::{AtomicBool, Ordering};

use super::harness::{
    arm_observations, disarm_observations, observation_complete, release_competing_hart,
};
use super::snapshot::{snapshot, StateSnapshot};

const GOVERNED_IDLE: u8 = 0;
const GOVERNED_PRE_READY: u8 = 1;
const GOVERNED_SMP: u8 = 2;

static GOVERNED_CASE: core::sync::atomic::AtomicU8 =
    core::sync::atomic::AtomicU8::new(GOVERNED_IDLE);
static GOVERNED_PENDING: crate::sync::Spinlock<Option<StateSnapshot>> =
    crate::sync::Spinlock::new(None);
static TRUSTED_PENDING: crate::sync::Spinlock<Option<StateSnapshot>> =
    crate::sync::Spinlock::new(None);
static TRUSTED_ARMED: AtomicBool = AtomicBool::new(false);

fn ready_contains(state: &StateSnapshot, tid: usize) -> bool {
    state
        .ready
        .iter()
        .flat_map(|queues| queues.values())
        .any(|queue| queue.contains(&tid))
}

fn observed_success(cases: &[&str], before: &StateSnapshot, tid: usize) -> bool {
    let after = snapshot();
    let audit_delta = after.audit.0.wrapping_sub(before.audit.0);
    let expected_evidence = if cases.contains(&"AP-15") {
        audit_delta == 36
    } else {
        audit_delta >= 36 && audit_delta.is_multiple_of(18)
    };
    cases.iter().all(|case| observation_complete(case))
        && after.tasks.iter().any(|task| task.id == tid)
        && after.next_task_id == before.next_task_id + 1
        && ready_contains(&after, tid)
        && after.quota != before.quota
        && after.measurements.0 == before.measurements.0 + 1
        && expected_evidence
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GovernedSuccess {
    PreReady,
    Smp,
}

impl GovernedSuccess {
    fn cases(self) -> &'static [&'static str] {
        match self {
            Self::PreReady => &["AP-12", "AP-14"],
            Self::Smp => &["AP-13"],
        }
    }

    #[cfg(target_arch = "riscv64")]
    fn code(self) -> u8 {
        match self {
            Self::PreReady => GOVERNED_PRE_READY,
            Self::Smp => GOVERNED_SMP,
        }
    }
}

#[cfg(target_arch = "riscv64")]
fn arm_governed_success(case: GovernedSuccess) {
    assert!(
        GOVERNED_CASE
            .compare_exchange(
                GOVERNED_IDLE,
                case.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok(),
        "governed publication observation already armed",
    );
    arm_observations(case.cases());
}

#[cfg(target_arch = "riscv64")]
pub(super) fn arm_pre_ready_success() {
    arm_governed_success(GovernedSuccess::PreReady);
}

#[cfg(target_arch = "riscv64")]
pub(super) fn arm_smp_success() {
    arm_governed_success(GovernedSuccess::Smp);
}

pub(super) fn begin_governed_attempt() {
    if GOVERNED_CASE.load(Ordering::Acquire) != GOVERNED_IDLE {
        let before = snapshot();
        let mut pending = GOVERNED_PENDING.lock();
        assert!(pending.is_none(), "nested governed publication attempt");
        *pending = Some(before);
    }
}

pub(super) fn governed_attempt_pending() -> bool {
    GOVERNED_PENDING.lock().is_some()
}

pub(super) fn abort_governed_attempt() {
    GOVERNED_PENDING.lock().take();
}

pub(super) fn finish_governed_success(tid: usize) -> Option<(GovernedSuccess, bool)> {
    let observed = match GOVERNED_CASE.swap(GOVERNED_IDLE, Ordering::AcqRel) {
        GOVERNED_PRE_READY => GovernedSuccess::PreReady,
        GOVERNED_SMP => GovernedSuccess::Smp,
        GOVERNED_IDLE => return None,
        unexpected => panic!("unknown governed publication observation state: {unexpected}"),
    };
    let before = GOVERNED_PENDING.lock().take();
    let passed = before.is_some_and(|before| observed_success(observed.cases(), &before, tid));
    if observed == GovernedSuccess::Smp {
        release_competing_hart();
    }
    Some((observed, passed))
}

#[cfg(target_arch = "riscv64")]
pub(super) fn skip_smp_success() {
    assert_eq!(
        GOVERNED_CASE.swap(GOVERNED_IDLE, Ordering::AcqRel),
        GOVERNED_IDLE,
        "AP-13 must not be armed before hart 1 is online",
    );
    GOVERNED_PENDING.lock().take();
    disarm_observations(&["AP-13"]);
    release_competing_hart();
}

pub(super) fn arm_trusted_success() {
    assert!(
        !TRUSTED_ARMED.swap(true, Ordering::AcqRel),
        "trusted publication observation already armed",
    );
    let before = snapshot();
    let mut pending = TRUSTED_PENDING.lock();
    assert!(
        pending.is_none(),
        "trusted publication observation already armed"
    );
    *pending = Some(before);
    arm_observations(&["AP-15"]);
}

pub(super) fn finish_trusted_success(tid: usize) -> bool {
    let before = TRUSTED_PENDING.lock().take();
    let passed = before.is_some_and(|before| observed_success(&["AP-15"], &before, tid));
    TRUSTED_ARMED.store(false, Ordering::Release);
    passed
}

/// Regression for the recursive quota-lock path: arm while allocator
/// attribution names a Cell and confirm that arming returns synchronously.
pub(super) fn trusted_arming_completes_from_cell_context() -> bool {
    let previous_cell_id = crate::task::hart_local::current_cell_id();
    crate::task::hart_local::set_current_cell_id(1);

    arm_trusted_success();

    let armed = TRUSTED_ARMED.load(Ordering::Acquire) && TRUSTED_PENDING.lock().is_some();
    TRUSTED_PENDING.lock().take();
    TRUSTED_ARMED.store(false, Ordering::Release);
    disarm_observations(&["AP-15"]);
    crate::task::hart_local::set_current_cell_id(previous_cell_id);
    armed
}
