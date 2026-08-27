#[path = "getrandom-sas-grant-cases.rs"]
mod getrandom_sas_grant_cases;
#[path = "getrandom-sas-revoke-race.rs"]
mod getrandom_sas_revoke_race;

use super::{stack::Stack, syscall, tcb::Task, SCHEDULER};
use crate::memory::paging::{Flags, PAGE_SIZE};
use alloc::{boxed::Box, vec, vec::Vec};
use types::CellId;

const CALLER: usize = 91_301;
const SIBLING: usize = 91_302;
const PEER: usize = 91_303;
const RAW_GETRANDOM: usize = 214;
const MAX_USER_BUF: usize = 64 * 1024 * 1024;

/// Run raw opcode 214 against live SAS tasks, root pages, and owned grants.
///
/// Every rejected descriptor must fail before entropy is requested. Valid
/// output covers a same-Cell stack, adjacent root pages, and a live grant.
pub(crate) fn run_fixture() -> bool {
    let caller_stack = match Stack::new_user(1) {
        Ok(stack) => stack,
        Err(_) => return false,
    };
    let sibling_stack = match Stack::new_user(1) {
        Ok(stack) => stack,
        Err(_) => return false,
    };
    let peer_stack = match Stack::new_user(1) {
        Ok(stack) => stack,
        Err(_) => return false,
    };
    let (cross_segment_base, read_only_base) = {
        let mut frames = crate::memory::frame::FRAME_ALLOCATOR.lock();
        let allocator = frames.as_mut().expect("frame allocator");
        let base = allocator
            .allocate_contiguous(2)
            .expect("two adjacent frames for GetRandom");
        let read_only = allocator
            .allocate_frame()
            .expect("read-only frame for GetRandom");
        let writable = Flags::from_bits(
            Flags::VALID
                | Flags::USER
                | Flags::READ
                | Flags::WRITE
                | Flags::ACCESSED
                | Flags::DIRTY,
        );
        let read_only_flags =
            Flags::from_bits(Flags::VALID | Flags::USER | Flags::READ | Flags::ACCESSED);
        for page in [base, base + PAGE_SIZE] {
            crate::memory::paging::map_page(allocator, page, page, writable)
                .expect("map GetRandom writable segment page");
        }
        crate::memory::paging::map_page(allocator, read_only, read_only, read_only_flags)
            .expect("map GetRandom read-only segment page");
        (base, read_only)
    };
    let caller_ptr = cross_segment_base + PAGE_SIZE - 32;
    let sibling_ptr = sibling_stack.usable_start() + 64;
    let peer_ptr = peer_stack.usable_start() + 64;
    let read_only_ptr = read_only_base + 64;
    let unowned_ptr = read_only_base + PAGE_SIZE;
    let mut caller = Box::new(Task::new(CALLER, CellId(91_301), "rng-caller", Vec::new()));
    caller.user_stack = Some(caller_stack);
    caller.segment_mem = Some(super::stack::CellSegments::with_writable_pages(
        vec![
            (cross_segment_base, cross_segment_base),
            (cross_segment_base + PAGE_SIZE, cross_segment_base + PAGE_SIZE),
            (read_only_base, read_only_base),
        ],
        vec![cross_segment_base, cross_segment_base + PAGE_SIZE],
        0,
    ));
    let caller_generation = caller.cell_generation;
    let mut sibling = Box::new(Task::new(SIBLING, CellId(91_301), "rng-sibling", Vec::new()));
    sibling.cell_generation = caller_generation;
    sibling.root_tid = CALLER;
    sibling.user_stack = Some(sibling_stack);
    let mut peer = Box::new(Task::new(PEER, CellId(91_302), "rng-peer", Vec::new()));
    peer.user_stack = Some(peer_stack);
    {
        let mut guard = SCHEDULER.lock();
        let Some(scheduler) = guard.as_mut() else {
            return false;
        };
        if scheduler.tasks.contains_key(&CALLER)
            || scheduler.tasks.contains_key(&SIBLING)
            || scheduler.tasks.contains_key(&PEER)
        {
            return false;
        }
        scheduler.tasks.insert(CALLER, caller);
        scheduler.tasks.insert(SIBLING, sibling);
        scheduler.tasks.insert(PEER, peer);
    }

    let _entropy = crate::task::drivers::virtio_rng::enable_test_entropy();
    let entropy_before_rejections = crate::task::drivers::virtio_rng::test_entropy_requests();
    let rejected = [
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, 0, 64, 0, 0),
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, usize::MAX - 31, 64, 0, 0),
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, caller_ptr, MAX_USER_BUF + 1, 0, 0),
        syscall::dispatch_raw_for_test(
            CALLER,
            RAW_GETRANDOM,
            run_fixture as *const () as usize,
            64,
            0,
            0,
        ),
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, peer_ptr, 64, 0, 0),
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, read_only_ptr, 64, 0, 0),
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, unowned_ptr, 64, 0, 0),
    ];
    let rejection_results_ok = rejected[0] == Err(syscall::SyscallError::InvalidInput)
        && rejected[1] == Err(syscall::SyscallError::InvalidInput)
        && rejected[2] == Err(syscall::SyscallError::BufferTooSmall)
        && rejected[3] == Err(syscall::SyscallError::InvalidInput)
        && rejected[4] == Err(syscall::SyscallError::InvalidInput)
        && rejected[5] == Err(syscall::SyscallError::InvalidInput)
        && rejected[6] == Err(syscall::SyscallError::InvalidInput)
        && crate::task::drivers::virtio_rng::test_entropy_requests()
            == entropy_before_rejections;
    let sibling_ok =
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, sibling_ptr, 65, 0, 0) == Ok(64);
    let caller_ok =
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, caller_ptr, 65, 0, 0) == Ok(64);
    let max_capacity_ok =
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, caller_ptr, MAX_USER_BUF, 0, 0)
            == Ok(64);
    let caller_bytes_ok = caller_ok
        && unsafe {
            core::slice::from_raw_parts(caller_ptr as *const u8, 64)
                .iter()
                .enumerate()
                .all(|(index, byte)| *byte == 0xA5 ^ index as u8)
        };
    let grant_cases_ok = getrandom_sas_grant_cases::run(CALLER, caller_generation);
    drop(_entropy);
    let zero_entropy_before = crate::task::drivers::virtio_rng::test_entropy_requests();
    let zero_entropy_invalid =
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, peer_ptr, 64, 0, 0)
            == Err(syscall::SyscallError::InvalidInput)
            && crate::task::drivers::virtio_rng::test_entropy_requests() == zero_entropy_before;
    let no_entropy_result = if cfg!(feature = "dev-weak-rng") { 64 } else { 0 };
    let zero_entropy_valid =
        syscall::dispatch_raw_for_test(CALLER, RAW_GETRANDOM, sibling_ptr, 65, 0, 0)
            == Ok(no_entropy_result)
            && crate::task::drivers::virtio_rng::test_entropy_requests() == zero_entropy_before + 1;
    let passed = rejection_results_ok
        && sibling_ok
        && caller_bytes_ok
        && max_capacity_ok
        && grant_cases_ok
        && zero_entropy_invalid
        && zero_entropy_valid;
    let removed = {
        let mut guard = SCHEDULER.lock();
        guard.as_mut().map(|scheduler| {
            (
                scheduler.tasks.remove(&CALLER),
                scheduler.tasks.remove(&SIBLING),
                scheduler.tasks.remove(&PEER),
            )
        })
    };
    drop(removed);
    if !passed {
        log::error!("S22-RV64-COPY: FAIL getrandom-sas hostile-matrix");
    }
    passed
}
