//! Task spawning and yielding.
//!
//! A "thread" in Cellos is another task of the *same* Cell: the kernel gives it
//! its own stacks and a new TID, and inherits the cell's identity on every axis it
//! gates (CellId, capability set, syscall allowlist, PKU domain). Threads therefore
//! share the cell's heap, quota, and address space — the isolation boundary stays
//! the cell, not the thread.
//!
//! Per-thread state is the cell's own business, carried in the user thread pointer
//! (`SetTlsBase`): a thread that wants its own TLS block allocates one and claims it
//! as its first action. A thread inherits its creator's base until it does.
//!
//! A thread that returns from its closure exits on its own; only the cell's *root*
//! task exit retires the whole cell generation.

#![allow(unsafe_code)]

use crate::syscall::{sys_exit, sys_spawn, SyscallResult};
use alloc::boxed::Box;

extern "C" fn thread_entry(arg: usize) {
    // SAFETY: `spawn` boxes exactly this shape and hands over the raw pointer; the
    // thread owns it from here and drops it when the closure is moved out.
    let outer: Box<Box<dyn FnOnce() + Send + 'static>> = unsafe { Box::from_raw(arg as *mut _) };
    let inner: Box<dyn FnOnce() + Send + 'static> = *outer;
    inner();

    // A worker exit terminates only this thread and wakes any `Wait` joiner with
    // the exit code; it does not retire the cell (the root task's exit does that).
    sys_exit(0);
}

/// Spawns a new thread with a closure.
pub fn spawn<F>(f: F) -> SyscallResult
where
    F: FnOnce() + Send + 'static,
{
    // 1. Box the closure (Fat Pointer to Closure)
    let inner: Box<dyn FnOnce() + Send + 'static> = Box::new(f);

    // 2. Box the Fat Pointer (Thin Pointer to Fat Pointer)
    let outer = Box::new(inner);
    // 3. Get raw pointer
    let ptr = Box::into_raw(outer) as usize;

    // 4. Call syscall with static entry point
    sys_spawn(thread_entry as *const () as usize, ptr)
}

/// Yield the current task, letting the scheduler run another one.
pub fn yield_now() {
    crate::syscall::sys_yield();
}
