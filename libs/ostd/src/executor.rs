#![allow(unsafe_code)]

use alloc::sync::Arc;
use api::completion::source;
use core::future::Future;
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

const DEFAULT_PARK_TICKS: usize = 1;
const SCHEDULER_TICK_MS: u64 = 10;

static PARK_PROOF_EMITTED: AtomicBool = AtomicBool::new(false);

struct ExecutorState {
    wake_requested: AtomicBool,
}

/// Block the current thread until `future` completes.
///
/// A pending poll parks through `WaitCompletion(TIMER)` instead of repeatedly
/// yielding. Futures without a specific deadline use a one-tick maintenance
/// park; [`sleep`] requests its full duration. `Recv` futures remain on their
/// existing non-blocking mailbox probe and are not migrated to the completion
/// queue by this executor.
///
/// # Panics
///
/// Panics when the cell has neither `WaitCompletion` nor `SetTimer` authority;
/// a pending future cannot be parked safely without one of those syscalls.
pub fn block_on<F: Future>(mut future: F) -> F::Output {
    let state = Arc::new(ExecutorState {
        wake_requested: AtomicBool::new(false),
    });
    let waker = executor_waker(Arc::clone(&state));
    let mut context = Context::from_waker(&waker);
    // SAFETY: `future` stays in this stack slot until the function returns.
    let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };

    loop {
        state.wake_requested.store(false, Ordering::Release);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => {
                if state.wake_requested.swap(false, Ordering::AcqRel) {
                    continue;
                }
                // TIMER is the only authorized generic wake source. One-tick
                // parks bound the latency of a userland waker that fires after
                // this check but before the kernel wait is installed.
                park_on_timer(DEFAULT_PARK_TICKS);
            }
        }
    }
}

fn park_on_timer(ticks: usize) {
    match crate::syscall::sys_wait_completion(source::TIMER, ticks as u64) {
        Some(completion) if completion.source == source::TIMER && completion.result == 0 => {
            if !PARK_PROOF_EMITTED.swap(true, Ordering::AcqRel) {
                let _ = crate::syscall::sys_log(
                    "[executor] dummy-waker=absent executor=parked source=TIMER PASS\n",
                );
            }
        }
        Some(_) => {}
        None => {
            // Coordinated builds have TIMER completion authority. This bounded
            // blocking fallback keeps older kernels or restricted cells from
            // regressing to a CPU-yield loop.
            if let crate::syscall::SyscallResult::Err(_) = crate::syscall::sys_set_timer(ticks) {
                let _ = crate::syscall::sys_log(
                    "[executor] missing WaitCompletion and SetTimer authority FAIL\n",
                );
                panic!("executor cannot park without WaitCompletion or SetTimer authority");
            }
        }
    }
}

fn executor_waker(state: Arc<ExecutorState>) -> Waker {
    let raw = RawWaker::new(Arc::into_raw(state).cast(), &EXECUTOR_WAKER_VTABLE);
    // SAFETY: every vtable operation reconstructs exactly one Arc reference
    // from the data pointer and follows RawWaker clone/drop ownership rules.
    unsafe { Waker::from_raw(raw) }
}

unsafe fn clone_waker(data: *const ()) -> RawWaker {
    // SAFETY: `data` came from Arc::into_raw in `executor_waker` or this clone.
    let state = ManuallyDrop::new(unsafe { Arc::from_raw(data.cast::<ExecutorState>()) });
    let cloned = Arc::clone(&state);
    RawWaker::new(Arc::into_raw(cloned).cast(), &EXECUTOR_WAKER_VTABLE)
}

unsafe fn wake(data: *const ()) {
    // SAFETY: wake consumes the RawWaker's one Arc reference.
    let state = unsafe { Arc::from_raw(data.cast::<ExecutorState>()) };
    state.wake_requested.store(true, Ordering::Release);
}

unsafe fn wake_by_ref(data: *const ()) {
    // SAFETY: wake_by_ref must retain the RawWaker's Arc reference.
    let state = ManuallyDrop::new(unsafe { Arc::from_raw(data.cast::<ExecutorState>()) });
    state.wake_requested.store(true, Ordering::Release);
}

unsafe fn drop_waker(data: *const ()) {
    // SAFETY: drop consumes the RawWaker's one Arc reference.
    drop(unsafe { Arc::from_raw(data.cast::<ExecutorState>()) });
}

static EXECUTOR_WAKER_VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone_waker, wake, wake_by_ref, drop_waker);

/// Yield once through the executor's parked TIMER path.
pub fn yield_now() -> impl Future<Output = ()> {
    YieldFuture { yielded: false }
}

struct YieldFuture {
    yielded: bool,
}

impl Future for YieldFuture {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(())
        } else {
            self.yielded = true;
            Poll::Pending
        }
    }
}

/// Sleep for `ticks` scheduler ticks without a busy-yield loop.
///
/// # Panics
///
/// Panics when neither monotonic `GetTime` nor blocking `SetTimer` is permitted.
pub fn sleep(ticks: usize) -> impl Future<Output = ()> {
    SleepFuture {
        ticks,
        deadline_ms: None,
    }
}

struct SleepFuture {
    ticks: usize,
    deadline_ms: Option<u64>,
}

impl Future for SleepFuture {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.ticks == 0 {
            return Poll::Ready(());
        }

        let Some(now_ms) = crate::syscall::sys_get_time_ms() else {
            // Cells using the previous executor already needed SetTimer
            // authority for sleep. Preserve that fail-safe path when GetTime
            // is unavailable instead of creating a future that never wakes.
            return match crate::syscall::sys_set_timer(self.ticks) {
                crate::syscall::SyscallResult::Ok(_) => Poll::Ready(()),
                crate::syscall::SyscallResult::Err(_) => {
                    panic!("sleep requires GetTime or SetTimer authority")
                }
            };
        };

        match self.deadline_ms {
            Some(deadline) if now_ms >= deadline => Poll::Ready(()),
            Some(_) => Poll::Pending,
            None => {
                let duration_ms = (self.ticks as u64).saturating_mul(SCHEDULER_TICK_MS);
                self.deadline_ms = Some(now_ms.saturating_add(duration_ms));
                Poll::Pending
            }
        }
    }
}
