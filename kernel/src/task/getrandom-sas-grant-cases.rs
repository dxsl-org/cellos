use super::super::{syscall, tcb::TaskState, SCHEDULER};

const RAW_GETRANDOM: usize = 214;
const RAW_GRANT_REGISTER: usize = 215;

/// Exercise live-grant ownership through the public raw syscall decoder.
pub(super) fn run(caller_id: usize, caller_generation: u64) -> bool {
    let grant_base =
        syscall::dispatch_raw_for_test(caller_id, RAW_GRANT_REGISTER, 128, 0, 0, 0).unwrap_or(0);
    if grant_base == 0 {
        return false;
    }

    {
        let mut scheduler = SCHEDULER.lock();
        let caller = scheduler
            .as_mut()
            .and_then(|scheduler| scheduler.tasks.get_mut(&caller_id));
        let Some(caller) = caller else {
            return false;
        };
        caller.cell_generation = caller_generation.wrapping_add(1);
    }
    let entropy_before_stale = crate::task::drivers::virtio_rng::test_entropy_requests();
    let stale_rejected =
        syscall::dispatch_raw_for_test(caller_id, RAW_GETRANDOM, grant_base + 32, 65, 0, 0)
            == Err(syscall::SyscallError::InvalidInput);
    let stale_skipped_entropy =
        crate::task::drivers::virtio_rng::test_entropy_requests() == entropy_before_stale;
    if let Some(caller) = SCHEDULER
        .lock()
        .as_mut()
        .and_then(|scheduler| scheduler.tasks.get_mut(&caller_id))
    {
        caller.cell_generation = caller_generation;
    }

    let grant_ok =
        syscall::dispatch_raw_for_test(caller_id, RAW_GETRANDOM, grant_base + 32, 65, 0, 0)
            == Ok(64);
    let retire_entropy_before = crate::task::drivers::virtio_rng::test_entropy_requests();
    {
        let mut scheduler = SCHEDULER.lock();
        let caller = scheduler
            .as_mut()
            .and_then(|scheduler| scheduler.tasks.get_mut(&caller_id));
        let Some(caller) = caller else {
            return false;
        };
        caller.state = TaskState::Retiring;
    }
    let retiring_rejected =
        syscall::dispatch_raw_for_test(caller_id, RAW_GETRANDOM, grant_base + 32, 65, 0, 0)
            == Err(syscall::SyscallError::PermissionDenied);
    let retire_entropy_after = crate::task::drivers::virtio_rng::test_entropy_requests();
    if let Some(caller) = SCHEDULER
        .lock()
        .as_mut()
        .and_then(|scheduler| scheduler.tasks.get_mut(&caller_id))
    {
        caller.state = TaskState::Ready;
    }
    let retire_skipped_entropy = retire_entropy_before == retire_entropy_after;

    let revoke_race_ok = super::getrandom_sas_revoke_race::run(caller_id, grant_base);
    let ok = stale_rejected
        && stale_skipped_entropy
        && grant_ok
        && retiring_rejected
        && retire_skipped_entropy
        && revoke_race_ok;
    if !ok {
        log::warn!(
            "[selftest] getrandom-sas-grant-cases: grant_base={:#x} stale_rejected={} stale_skipped_entropy={} grant_ok={} retiring_rejected={} retire_skipped_entropy={} revoke_race_ok={}",
            grant_base,
            stale_rejected,
            stale_skipped_entropy,
            grant_ok,
            retiring_rejected,
            retire_skipped_entropy,
            revoke_race_ok
        );
    }
    ok
}
