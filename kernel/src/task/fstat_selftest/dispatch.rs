use super::fixture::{
    backend_calls, check, install_failing_file, reset_backend_calls, FAILING_FD, SENTINEL, TASK_A,
    TASK_B,
};
use api::syscall::{ViFstatV1, VI_FSTAT_ACCESS_READ, VI_FSTAT_KIND_REGULAR, VI_FSTAT_V1_LEN};

use super::super::syscall::{handle_syscall, Syscall};

fn dispatch_fstat(fd: usize, output: &mut [u8], out_len: usize) -> bool {
    handle_syscall(
        TASK_A,
        Syscall::Fstat {
            fd,
            out_ptr: output.as_mut_ptr() as usize,
            out_len,
        },
    ) == Ok(VI_FSTAT_V1_LEN)
}

fn decode(bytes: &[u8; VI_FSTAT_V1_LEN]) -> ViFstatV1 {
    unsafe { core::ptr::read_unaligned(bytes.as_ptr().cast::<ViFstatV1>()) }
}

pub(super) fn test_dispatch(ok: &mut bool, file_fd: usize) {
    let mut success = [SENTINEL; VI_FSTAT_V1_LEN];
    let wrote_exact = dispatch_fstat(file_fd, &mut success, VI_FSTAT_V1_LEN);
    let metadata = decode(&success);
    check(
        ok,
        wrote_exact
            && metadata.kind == VI_FSTAT_KIND_REGULAR
            && metadata.access == VI_FSTAT_ACCESS_READ
            && metadata.size > 0
            && metadata.reserved == [0; 2],
        "exact wire write and zero reserved",
    );
    let mut oversized = [SENTINEL; VI_FSTAT_V1_LEN];
    check(
        ok,
        dispatch_fstat(file_fd, &mut oversized, VI_FSTAT_V1_LEN + 8),
        "oversized output accepted with exact wire return",
    );

    let mut short = [SENTINEL; VI_FSTAT_V1_LEN];
    let short_failed = !dispatch_fstat(file_fd, &mut short, VI_FSTAT_V1_LEN - 1);
    check(
        ok,
        short_failed && short.iter().all(|byte| *byte == SENTINEL),
        "short output unchanged",
    );

    let mut missing = [SENTINEL; VI_FSTAT_V1_LEN];
    let missing_failed = !dispatch_fstat(89, &mut missing, VI_FSTAT_V1_LEN);
    check(
        ok,
        missing_failed && missing.iter().all(|byte| *byte == SENTINEL),
        "invalid descriptor output unchanged",
    );

    let mut isolated = [SENTINEL; VI_FSTAT_V1_LEN];
    let isolated_failed = handle_syscall(
        TASK_B,
        Syscall::Fstat {
            fd: file_fd,
            out_ptr: isolated.as_mut_ptr() as usize,
            out_len: VI_FSTAT_V1_LEN,
        },
    )
    .is_err();
    check(
        ok,
        isolated_failed && isolated.iter().all(|byte| *byte == SENTINEL),
        "foreign descriptor output unchanged",
    );

    reset_backend_calls();
    install_failing_file();
    let mut backend_short = [SENTINEL; VI_FSTAT_V1_LEN];
    let backend_short_failed = !dispatch_fstat(FAILING_FD, &mut backend_short, VI_FSTAT_V1_LEN - 1);
    check(
        ok,
        backend_short_failed
            && backend_short.iter().all(|byte| *byte == SENTINEL)
            && backend_calls() == 0,
        "output validation precedes backend gather",
    );

    let backend_null_failed = handle_syscall(
        TASK_A,
        Syscall::Fstat {
            fd: FAILING_FD,
            out_ptr: 0,
            out_len: VI_FSTAT_V1_LEN,
        },
    )
    .is_err();
    let backend_overflow_failed = handle_syscall(
        TASK_A,
        Syscall::Fstat {
            fd: FAILING_FD,
            out_ptr: usize::MAX - 15,
            out_len: VI_FSTAT_V1_LEN,
        },
    )
    .is_err();
    check(
        ok,
        backend_null_failed && backend_overflow_failed && backend_calls() == 0,
        "pointer validation precedes backend gather",
    );

    let mut backend = [SENTINEL; VI_FSTAT_V1_LEN];
    let backend_failed = !dispatch_fstat(FAILING_FD, &mut backend, VI_FSTAT_V1_LEN);
    check(
        ok,
        backend_failed && backend.iter().all(|byte| *byte == SENTINEL) && backend_calls() == 1,
        "backend error output unchanged",
    );
}
