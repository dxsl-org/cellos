use super::{
    consume_ipc_burst_grace, IPC_BURST_GRACE_YIELDS, MTIME_TICKS_PER_MS,
    NET_RX_IDLE_WAIT_SCHEDULER_TICKS, SMOLTCP_MAINTENANCE_INTERVAL_MS, SMOLTCP_MAINTENANCE_TICKS,
};

#[test]
fn idle_ipc_wait_is_one_scheduler_tick() {
    assert_eq!(NET_RX_IDLE_WAIT_SCHEDULER_TICKS, 1);
}

#[test]
fn one_post_reply_yield_precedes_idle_completion_wait() {
    let mut remaining = IPC_BURST_GRACE_YIELDS;

    assert!(consume_ipc_burst_grace(&mut remaining));
    assert!(!consume_ipc_burst_grace(&mut remaining));
}

#[test]
fn smoltcp_maintenance_remains_one_hundred_milliseconds() {
    assert_eq!(SMOLTCP_MAINTENANCE_INTERVAL_MS, 100);
    assert_eq!(MTIME_TICKS_PER_MS, 10_000);
    assert_eq!(SMOLTCP_MAINTENANCE_TICKS, 1_000_000);
}
