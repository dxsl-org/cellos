//! LAB-01 Carrier Transfer Native Benchmark Scenario (Phase 06B).
//!
//! Orchestrates the LAB-01 carrier transfer workload on QEMU:
//! 1. Nominal transfer: dummy carrier 42 from Rack A Slot 1 to Rack B Slot 2.
//! 2. Real VFS persistence: write trace record to `/srv/lab_trace.log`, read back, and verify.
//! 3. Duplicate job handling: identical query returns cached Completed state; conflicting job rejected.
//! 4. Interruption & Reconciliation: timeout transitions to ReconcileRequired; blocks new work.
//! 5. Authoritative Reconciliation: out-of-band operator authority clears uncertainty to FailedSafe.
//! 6. Configuration Epoch Rotation: rotating configuration epoch invalidates prior observations.
//! 7. Final VFS Log Verification: all committed records verified on CellosFS Native.

extern crate alloc;

use alloc::format;
use api::syscall::service;
use ostd::io::println;
use ostd::syscall::{sys_heartbeat, sys_lookup_service};

use super::lab_transfer_contract::{
    Admission, AdmissionObservation, Configuration, JobState, PlacementObservation, Principal,
    ReconcileDecision, TransferContract, TransferRequest,
};

const TRACE_LOG_PATH: &str = "/srv/lab_trace.log";

const OBSERVER: Principal = Principal {
    id: 10,
    generation: 1,
};

const RECONCILE_AUTHORITY: Principal = Principal {
    id: 20,
    generation: 1,
};

const INITIAL_CONFIG: Configuration = Configuration {
    id: 101,
    observer: OBSERVER,
    reconcile_authority: RECONCILE_AUTHORITY,
    observation_max_age_ticks: 1000,
};

pub fn run() {
    println("[lab-carrier-transfer] START: LAB-01 native QEMU witness");
    sys_heartbeat(0);

    // Verify VFS service is available
    let Some(vfs_tid) = sys_lookup_service(service::VFS) else {
        fail("VFS service is not registered");
    };
    println(&format!(
        "[lab-carrier-transfer] VFS service registered: tid={vfs_tid}"
    ));

    // Ensure clean start for trace log
    let mut vfs_client = ostd::clients::VfsClient::new();
    let _ = vfs_client.unlink(TRACE_LOG_PATH);

    let mut contract = TransferContract::new(INITIAL_CONFIG)
        .unwrap_or_else(|_| fail("failed to initialize TransferContract"));

    let mut current_ticks: u64 = 100;

    // ── Scenario 1: Nominal LAB-01 Transfer ──────────────────────────────────
    println("[lab-carrier-transfer] Scenario 1: Nominal transfer of carrier 42 (slot 1 -> slot 2)");
    let req_1 = TransferRequest {
        run_id: 1,
        job_id: 1001,
        carrier_id: 42,
        payload_identity_digest: 0xCAFE_0001,
        payload_item_count: 1,
        source_slot: 1,
        destination_slot: 2,
        configuration_id: 101,
        expected_trace_digest: 0xD00D_0001,
    };

    let adm_obs_1 = AdmissionObservation {
        configuration_id: 101,
        producer: OBSERVER,
        sequence: 1,
        captured_at_ticks: current_ticks,
        source_slot: 1,
        source_carrier: Some(42),
        destination_slot: 2,
        destination_carrier: None,
    };

    let admission = contract
        .admit(req_1, adm_obs_1, current_ticks)
        .unwrap_or_else(|_| fail("nominal admit failed"));
    assert_eq!(admission, Admission::Accepted);
    assert_eq!(contract.active_state(), Some(JobState::Admitted));

    contract
        .prepare_command(1001)
        .unwrap_or_else(|_| fail("prepare_command failed"));
    assert_eq!(contract.active_state(), Some(JobState::CommandPending));

    contract
        .begin_dispatch(1001)
        .unwrap_or_else(|_| fail("begin_dispatch failed"));
    assert_eq!(contract.active_state(), Some(JobState::Dispatching));

    contract
        .command_result(1001, true)
        .unwrap_or_else(|_| fail("command_result failed"));
    assert_eq!(contract.active_state(), Some(JobState::AwaitingPlacement));

    current_ticks += 20;
    let place_obs_1 = PlacementObservation {
        configuration_id: 101,
        producer: OBSERVER,
        sequence: 2,
        captured_at_ticks: current_ticks,
        carrier_id: 42,
        slot: 2,
        released: true,
    };

    contract
        .observe_placement(1001, place_obs_1, current_ticks)
        .unwrap_or_else(|_| fail("observe_placement failed"));
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));

    // Real VFS Trace Write and Readback
    let trace_record_1 = format!("TRACE:job=1001,carrier=42,src=1,dst=2,digest=0xD00D_0001\n");
    if vfs_client
        .append_file(TRACE_LOG_PATH, trace_record_1.as_bytes())
        .is_err()
    {
        fail("failed to append trace record 1 to VFS");
    }

    let readback_1 = vfs_client
        .read_file(TRACE_LOG_PATH)
        .unwrap_or_else(|_| fail("failed to read trace log from VFS"));
    if !readback_1.starts_with(trace_record_1.as_bytes()) {
        fail("trace log readback mismatch on VFS");
    }

    contract
        .acknowledge_trace(1001, 0xD00D_0001, 0xD00D_0001)
        .unwrap_or_else(|_| fail("acknowledge_trace failed"));
    assert_eq!(contract.active_state(), None); // Terminal job retained
    assert_eq!(contract.retained_count(), 1);
    println("[lab-carrier-transfer] nominal transfer completed and trace verified on VFS");

    // ── Scenario 2: Duplicate & Conflicting Requests ─────────────────────────
    println("[lab-carrier-transfer] Scenario 2: Duplicate query & conflict rejection");
    let dup_check = contract.existing_admission(req_1);
    assert_eq!(
        dup_check,
        Some(Ok(Admission::Duplicate(JobState::Completed)))
    );

    let mut conflicting_req = req_1;
    conflicting_req.carrier_id = 99; // Different carrier for same job ID
    let conflict_check = contract.existing_admission(conflicting_req);
    assert!(matches!(conflict_check, Some(Err(_))));
    println("[lab-carrier-transfer] duplicate and conflicting admissions verified");

    // ── Scenario 3: Interrupted Dispatch & Out-of-Band Reconciliation ─────────
    println("[lab-carrier-transfer] Scenario 3: Interrupted dispatch and reconciliation");
    current_ticks += 50;
    let req_2 = TransferRequest {
        run_id: 1,
        job_id: 1002,
        carrier_id: 43,
        payload_identity_digest: 0xCAFE_0002,
        payload_item_count: 1,
        source_slot: 3,
        destination_slot: 4,
        configuration_id: 101,
        expected_trace_digest: 0xD00D_0002,
    };

    let adm_obs_2 = AdmissionObservation {
        configuration_id: 101,
        producer: OBSERVER,
        sequence: 3,
        captured_at_ticks: current_ticks,
        source_slot: 3,
        source_carrier: Some(43),
        destination_slot: 4,
        destination_carrier: None,
    };

    contract
        .admit(req_2, adm_obs_2, current_ticks)
        .unwrap_or_else(|_| fail("admit job 1002 failed"));
    contract
        .prepare_command(1002)
        .unwrap_or_else(|_| fail("prepare job 1002 failed"));
    contract
        .begin_dispatch(1002)
        .unwrap_or_else(|_| fail("dispatch job 1002 failed"));

    // Interruption occurs mid-dispatch: timeout triggered
    contract
        .timeout(1002)
        .unwrap_or_else(|_| fail("timeout job 1002 failed"));
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));

    // While ReconcileRequired, new admissions must be blocked
    let req_blocked = TransferRequest {
        run_id: 1,
        job_id: 1003,
        carrier_id: 44,
        payload_identity_digest: 0xCAFE_0003,
        payload_item_count: 1,
        source_slot: 5,
        destination_slot: 6,
        configuration_id: 101,
        expected_trace_digest: 0xD00D_0003,
    };
    let adm_obs_3 = AdmissionObservation {
        configuration_id: 101,
        producer: OBSERVER,
        sequence: 4,
        captured_at_ticks: current_ticks,
        source_slot: 5,
        source_carrier: Some(44),
        destination_slot: 6,
        destination_carrier: None,
    };
    assert!(contract
        .admit(req_blocked, adm_obs_3, current_ticks)
        .is_err());

    // Out-of-band operator reconciles: carrier 43 observed safely retained at source slot 3
    current_ticks += 30;
    let recon_obs = PlacementObservation {
        configuration_id: 101,
        producer: OBSERVER,
        sequence: 5,
        captured_at_ticks: current_ticks,
        carrier_id: 43,
        slot: 3, // Verified still at source
        released: true,
    };

    contract
        .reconcile(
            1002,
            RECONCILE_AUTHORITY,
            ReconcileDecision::ConfirmedAtSource,
            Some(recon_obs),
            current_ticks,
        )
        .unwrap_or_else(|_| fail("reconcile failed"));

    assert_eq!(contract.active_state(), None); // Job 1002 retained as FailedSafe
    assert_eq!(contract.retained_count(), 2);

    let trace_record_2 =
        format!("RECONCILE:job=1002,decision=ConfirmedAtSource,carrier=43,slot=3\n");
    if vfs_client
        .append_file(TRACE_LOG_PATH, trace_record_2.as_bytes())
        .is_err()
    {
        fail("failed to append reconcile record to VFS");
    }
    println(
        "[lab-carrier-transfer] reconciliation required and resolved via authoritative observation",
    );

    // ── Scenario 4: Configuration Epoch Rotation ─────────────────────────────
    println("[lab-carrier-transfer] Scenario 4: Configuration epoch rotation");
    let rotated_config = Configuration {
        id: 102, // New epoch ID
        observer: OBSERVER,
        reconcile_authority: RECONCILE_AUTHORITY,
        observation_max_age_ticks: 1000,
    };
    contract
        .reconfigure(rotated_config)
        .unwrap_or_else(|_| fail("reconfigure failed"));

    // Stale observation from old configuration 101 must be rejected
    let stale_req = TransferRequest {
        run_id: 2,
        job_id: 1004,
        carrier_id: 45,
        payload_identity_digest: 0xCAFE_0004,
        payload_item_count: 1,
        source_slot: 1,
        destination_slot: 2,
        configuration_id: 101, // Stale config ID
        expected_trace_digest: 0xD00D_0004,
    };
    assert!(contract.admit(stale_req, adm_obs_1, current_ticks).is_err());
    println("[lab-carrier-transfer] configuration change invalidates old observations");

    // ── Scenario 5: Trace Log Persistence Check ──────────────────────────────
    println("[lab-carrier-transfer] Scenario 5: Verifying trace log durability on VFS");
    let full_trace = vfs_client
        .read_file(TRACE_LOG_PATH)
        .unwrap_or_else(|_| fail("failed to read complete trace log from VFS"));

    assert!(full_trace
        .windows(trace_record_1.len())
        .any(|w| w == trace_record_1.as_bytes()));
    assert!(full_trace
        .windows(trace_record_2.len())
        .any(|w| w == trace_record_2.as_bytes()));
    println("[lab-carrier-transfer] all trace records verified on CellosFS Native");

    println("[lab-carrier-transfer] Summary: nominal transfer, VFS logging, idempotency, reconciliation, and config rotation verified");
    println("[lab-carrier-transfer] ALL CRITERIA PASSED");
    ostd::syscall::sys_exit(0);
}

fn fail(message: &str) -> ! {
    println(&format!("[lab-carrier-transfer] FAIL: {message}"));
    ostd::syscall::sys_exit(1)
}
