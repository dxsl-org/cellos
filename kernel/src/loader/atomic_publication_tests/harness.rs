#[cfg(target_arch = "riscv64")]
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::{AtomicU16, AtomicU8, Ordering};
use types::ViError;

static FAIL_AT: AtomicU8 = AtomicU8::new(0);
static OBSERVE_AT: AtomicU16 = AtomicU16::new(0);
static LAST_OBSERVED: AtomicU16 = AtomicU16::new(0);
static OBSERVATION_FAILED: AtomicU16 = AtomicU16::new(0);
#[cfg(target_arch = "riscv64")]
static AP13_BARRIER: AtomicU8 = AtomicU8::new(0);
#[cfg(target_arch = "riscv64")]
static AP13_TARGET_TID: AtomicUsize = AtomicUsize::new(usize::MAX);
#[cfg(target_arch = "riscv64")]
static AP13_REMOTE_ACK: AtomicU8 = AtomicU8::new(0);
pub(super) fn code(case: &str) -> u8 {
    let Some(raw) = case.strip_prefix("AP-") else {
        return 0;
    };
    raw.len()
        .eq(&2)
        .then(|| raw.parse::<u8>().ok())
        .flatten()
        .filter(|value| *value <= 15)
        .map_or(0, |value| value + 1)
}

pub(super) fn arm_failure(case: &'static str) {
    let value = code(case);
    assert_ne!(value, 0, "unknown atomic-publication injection");
    FAIL_AT.store(value, Ordering::Release);
}

pub(super) fn failure_is_armed(case: &str) -> bool {
    FAIL_AT.load(Ordering::Acquire) == code(case)
}

pub(super) fn arm_observations(cases: &[&'static str]) {
    let mut expected = 0u16;
    for &case in cases {
        let value = code(case);
        assert_ne!(value, 0, "unknown atomic-publication observation");
        expected |= 1u16 << value;
    }
    assert_ne!(expected, 0, "empty atomic-publication observation");
    LAST_OBSERVED.fetch_and(!expected, Ordering::AcqRel);
    OBSERVATION_FAILED.fetch_and(!expected, Ordering::AcqRel);
    OBSERVE_AT.fetch_or(expected, Ordering::AcqRel);
}

pub(super) fn disarm_observations(cases: &[&'static str]) {
    let mut expected = 0u16;
    for &case in cases {
        let value = code(case);
        assert_ne!(value, 0, "unknown atomic-publication observation");
        expected |= 1u16 << value;
    }
    assert_ne!(expected, 0, "empty atomic-publication observation");
    OBSERVE_AT.fetch_and(!expected, Ordering::AcqRel);
    LAST_OBSERVED.fetch_and(!expected, Ordering::AcqRel);
    OBSERVATION_FAILED.fetch_and(!expected, Ordering::AcqRel);
}

pub(super) fn observation_complete(case: &str) -> bool {
    let value = code(case);
    let mask = 1u16 << value;
    LAST_OBSERVED.load(Ordering::Acquire) & mask != 0
        && OBSERVATION_FAILED.load(Ordering::Acquire) & mask == 0
}

pub(super) fn failure_reached() -> bool {
    FAIL_AT.swap(0, Ordering::AcqRel) == 0
}

pub(crate) fn checkpoint(case: &'static str) -> Result<(), ViError> {
    let value = code(case);
    if value != 0
        && FAIL_AT
            .compare_exchange(value, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        return Err(ViError::Unknown);
    }
    Ok(())
}

/// Called immediately before a hart tries to take `SCHEDULER`. AP-13 uses this
/// actual scheduler entry point to make hart 1 inspect ready-queue visibility
/// while the publisher still owns `SCHEDULER`.
pub(crate) fn observe_schedule_attempt() {
    #[cfg(target_arch = "riscv64")]
    if AP13_BARRIER.load(Ordering::Acquire) == 1
        && crate::task::hart_local::current_hart_id() == crate::task::smp::HART_RT
    {
        let tid = AP13_TARGET_TID.load(Ordering::Acquire);
        let excluded = tid != usize::MAX
            && crate::task::hart_local::HART_LOCALS.iter().all(|hart| {
                hart.current_task_id.load(Ordering::Acquire) != tid
                    && hart
                        .ready
                        .lock()
                        .values()
                        .all(|queue| !queue.contains(&tid))
            });
        AP13_REMOTE_ACK.store(if excluded { 1 } else { 2 }, Ordering::Release);
        while AP13_BARRIER.load(Ordering::Acquire) == 1 {
            core::hint::spin_loop();
        }
    }
}

#[cfg(target_arch = "riscv64")]
fn disarm_competing_hart() {
    AP13_BARRIER.store(0, Ordering::Release);
    AP13_TARGET_TID.store(usize::MAX, Ordering::Release);
}

pub(crate) fn release_competing_hart() {
    #[cfg(target_arch = "riscv64")]
    disarm_competing_hart();
}

#[cfg(target_arch = "riscv64")]
fn competing_hart_schedule_attempt(tid: usize) -> bool {
    if !crate::task::smp::is_rt_hart_online() {
        return false;
    }
    AP13_TARGET_TID.store(tid, Ordering::Release);
    AP13_REMOTE_ACK.store(0, Ordering::Release);
    AP13_BARRIER.store(1, Ordering::Release);
    let target = crate::task::smp::logical_sbi_target(crate::task::smp::HART_RT);
    let Some((mask, base)) = target else {
        disarm_competing_hart();
        return false;
    };
    if hal::common::sbi::sbi_send_ipi(mask, base).is_err() {
        disarm_competing_hart();
        return false;
    }
    for _ in 0..20 {
        for _ in 0..5_000_000 {
            match AP13_REMOTE_ACK.load(Ordering::Acquire) {
                1 => return true,
                2 => {
                    disarm_competing_hart();
                    return false;
                }
                _ => {}
            }
            core::hint::spin_loop();
        }
        let _ = hal::common::sbi::sbi_send_ipi(mask, base);
    }
    disarm_competing_hart();
    false
}

#[cfg(not(target_arch = "riscv64"))]
fn competing_hart_schedule_attempt(_tid: usize) -> bool {
    false
}

pub(crate) fn observe_complete(sched: &crate::task::scheduler::Scheduler, tid: usize) {
    let Some(task) = sched.tasks.get(&tid) else {
        return;
    };
    let armed = OBSERVE_AT.load(Ordering::Acquire);
    let trusted = 1u16 << code("AP-15");
    let expected = if task.name == "init" {
        armed & trusted
    } else {
        armed & !trusted
    };
    if expected == 0 {
        return;
    }
    OBSERVE_AT.fetch_and(!expected, Ordering::AcqRel);
    let ap13 = 1u16 << code("AP-13");
    let competing_hart_observed = expected & ap13 == 0 || competing_hart_schedule_attempt(tid);
    let complete = competing_hart_observed
        && task.cell_id.0 != 0
        && task.kernel_stack.is_some()
        && task.user_stack.is_some()
        && task.segment_mem.is_some()
        && (expected & trusted == 0
            || (task.is_critical
                && task.supervisor_cap.is_some()
                && task.priority == api::TaskPriority::Normal as u8
                && crate::task::cap::CapSet::of_task(task)
                    == super::super::boot_ceiling::boot_ceiling("/bin/init")));
    if complete {
        LAST_OBSERVED.fetch_or(expected, Ordering::Release);
    } else {
        OBSERVATION_FAILED.fetch_or(expected, Ordering::Release);
    }
}
