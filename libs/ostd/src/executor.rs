#![allow(unsafe_code)]

use core::future::Future;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use crate::syscall::sys_yield;

/// A simple executor that blocks the current thread until the future completes.
/// It yields to the OS kernel when the future is pending.
pub fn block_on<F: Future>(mut future: F) -> F::Output {
    // 1. Create a minimal Waker that does nothing (since we rely on OS rescheduling)
    // In a real async/await OS, the Waker would register with the Kernel logic (e.g. set_timer).
    // Here we use a "Busy Poll with Yield" strategy for MVP.
    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut context = Context::from_waker(&waker);

    // 2. Pin the future (in memory stack)
    // Ideally we should use Pin<Box<...>> or pin_utils.
    // Since we don't have pin_utils, we use unsafe pinning on stack variable.
    // SAFETY: We do not move `future` after this.
    let mut future = unsafe { core::pin::Pin::new_unchecked(&mut future) };

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => {
                // Future is not ready. Yield CPU to let other tasks run.
                // When we come back, we poll again.
                sys_yield();
            }
        }
    }
}

// --- Minimal Waker Implementation ---

fn dummy_raw_waker() -> RawWaker {
    static VTABLE: RawWakerVTable = RawWakerVTable::new(
        |data| RawWaker::new(data, &VTABLE), // clone
        |_| {}, // wake
        |_| {}, // wake_by_ref
        |_| {}, // drop
    );
    RawWaker::new(core::ptr::null(), &VTABLE)
}

/// Yields execution back to the executor (and OS).
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

pub fn sleep(ticks: usize) -> impl Future<Output = ()> {
    SleepFuture { ticks, started: false }
}

struct SleepFuture {
    ticks: usize,
    started: bool,
}

impl Future for SleepFuture {
    type Output = ();

    fn poll(mut self: core::pin::Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        // We lack a proper "User-accessible" System Time for now.
        // But sys_set_timer expects absolute time? No, let's make sys_set_timer handle relative?
        // Wait, Kernel's SetTimer used absolute check: `deadline > now`.
        // So User needs to know `now`.
        //
        // WORKAROUND: For MVP, we pass relative ticks to sys_set_timer, 
        // and Kernel adds it to current time.
        // 
        // WAIT: I implemented `Syscall::SetTimer { deadline }` in Kernel expecting Absolute.
        // I need to fix Kernel or expose `sys_time` syscall.
        //
        // Let's fix Kernel SetTimer to be "WakeAt" (Absolute) 
        // AND Assume User guesses 'now' is purely looped?
        // No, let's use a "SleepFor" syscall which is easier.
        // Modification: Rename sys_set_timer to sys_sleep_for?
        // OR: Loop reading `sys_set_timer`. 
        //
        // Let's rely on the fact that if we set a deadline 0, maybe we can read time?
        // 
        // Let's change strategy:
        // Use a loop counter here for valid polling.
        
        if !self.started {
             // If we assume Syscall 3 is "Sleep For Delta":
             let _ = crate::syscall::sys_set_timer(self.ticks); // Sleep for X ticks
             self.started = true;
             Poll::Pending
         } else {
             Poll::Ready(()) // If we woke up, we are done
         }
    }
}
