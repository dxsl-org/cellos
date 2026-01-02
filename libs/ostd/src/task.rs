#![allow(unsafe_code)]

use alloc::boxed::Box;
use crate::syscall::{sys_spawn, SyscallResult};

extern "C" fn thread_entry(arg: usize) {
    crate::println!("OSTd: Entered thread_entry with Arg 0x{:X}", arg);
    crate::println!("OSTd: About to unbox outer...");
    let outer: Box<Box<dyn FnOnce() + Send + 'static>> = unsafe { Box::from_raw(arg as *mut _) };
    
    crate::println!("OSTd: About to unbox inner...");
    // 2. Move inner Box out
    let inner: Box<dyn FnOnce() + Send + 'static> = *outer;
    
    crate::println!("OSTd: About to run closure...");
    // 3. Run closure
    inner();
    
    crate::println!("OSTd: Closure finished, entering exit loop...");
    // 4. Exit
    loop { crate::syscall::sys_yield(); }
}

/// Spawns a new thread with a closure.
pub fn spawn<F>(f: F) -> SyscallResult 
where
    F: FnOnce() + Send + 'static
{
    // 1. Box the closure (Fat Pointer to Closure)
    let inner: Box<dyn FnOnce() + Send + 'static> = Box::new(f);
    
    // 2. Box the Fat Pointer (Thin Pointer to Fat Pointer)
    let outer = Box::new(inner);
    // 3. Get raw pointer
    let ptr = Box::into_raw(outer) as usize;
    
    // 4. Call syscall with static entry point
    sys_spawn(thread_entry as usize, ptr)
}
