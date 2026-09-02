use super::{
    consume_ipc_burst_grace, idle_ipc_wake_is_provably_early, IDLE_IPC_WAKE_PROOF_CEILING_TICKS,
    IPC_BURST_GRACE_YIELDS, MTIME_TICKS_PER_MS, NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS,
    SCHEDULER_TICK_MS, SMOLTCP_MAINTENANCE_INTERVAL_MS, SMOLTCP_MAINTENANCE_TICKS,
};

#[test]
fn net_rx_wait_is_finite_and_maintenance_aligned() {
    assert!(NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS > 0);
    assert_eq!(NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS, 10);
    assert_eq!(SCHEDULER_TICK_MS, 10);
    assert_eq!(
        NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS * SCHEDULER_TICK_MS,
        SMOLTCP_MAINTENANCE_INTERVAL_MS
    );
}

#[test]
fn idle_ipc_wake_proof_ceiling_precedes_earliest_phase_aligned_deadline() {
    assert_eq!(
        IDLE_IPC_WAKE_PROOF_CEILING_TICKS,
        (NET_RX_MAINTENANCE_WAIT_SCHEDULER_TICKS - 1) * SCHEDULER_TICK_MS * MTIME_TICKS_PER_MS
    );
    assert_eq!(IDLE_IPC_WAKE_PROOF_CEILING_TICKS, 900_000);
    assert!(IDLE_IPC_WAKE_PROOF_CEILING_TICKS < SMOLTCP_MAINTENANCE_TICKS);
    assert!(idle_ipc_wake_is_provably_early(
        IDLE_IPC_WAKE_PROOF_CEILING_TICKS - 1
    ));
    assert!(!idle_ipc_wake_is_provably_early(
        IDLE_IPC_WAKE_PROOF_CEILING_TICKS
    ));
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
