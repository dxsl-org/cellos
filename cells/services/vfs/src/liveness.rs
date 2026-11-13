//! VFS heartbeat: the kernel kills a VFS that stops turning, and init restarts it.
//!
//! Armed when the request loop starts (mount and discovery run unarmed, each
//! block request already bounded). The loop re-arms every turn and before
//! every block request, and never waits longer than half the interval, so only
//! a VFS that is truly stuck (a lock cycle, an unbounded wait) misses it.

use core::sync::atomic::{AtomicBool, Ordering};

/// 60 s: longer than one block request's bound (`BLOCK_REPLY_TICKS`, 35 s)
/// plus the rest of the turn.
pub const VFS_HEARTBEAT_TICKS: u64 = 6_000;

static ARMED: AtomicBool = AtomicBool::new(false);

/// Arm the heartbeat; from now on [`keepalive`] re-arms it.
pub fn arm() {
    ARMED.store(true, Ordering::Relaxed);
    keepalive();
}

/// Re-arm the heartbeat if the loop armed it.
pub fn keepalive() {
    if ARMED.load(Ordering::Relaxed) {
        ostd::syscall::sys_heartbeat(VFS_HEARTBEAT_TICKS);
    }
}
