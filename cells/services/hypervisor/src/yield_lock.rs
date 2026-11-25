//! Taking a lock the vCPU threads share (the emulated devices).
//!
//! A waiter spins briefly, since the holder is usually serving one exit on
//! another CPU, then yields: a holder preempted inside the critical section
//! must get the CPU back to release it, which plain spinning would deny it on
//! a busy host. The same discipline as the Cell heap's lock (`ostd`).

use ostd::sync::{Mutex, MutexGuard};

/// Spin iterations before a waiter yields.
const SPINS_BEFORE_YIELD: u32 = 64;

pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    let mut spins = 0;
    loop {
        if let Some(guard) = mutex.try_lock() {
            return guard;
        }
        if spins < SPINS_BEFORE_YIELD {
            spins += 1;
            core::hint::spin_loop();
        } else {
            spins = 0;
            ostd::task::yield_now();
        }
    }
}
