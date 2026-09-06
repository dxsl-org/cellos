#[path = "../../../cells/tests/bench/src/scenarios/lab_transfer_contract.rs"]
mod contract;

use contract::{
    Admission, AdmissionObservation, Configuration, ContractError, JobState, PlacementObservation,
    Principal, ReconcileDecision, TransferContract, TransferRequest, MAX_RETAINED_JOBS,
};
use std::path::PathBuf;
use vicell_integration_tests::{qemu_binary, QemuRunner};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root resolves")
}

fn kernel_path() -> String {
    repo_root()
        .join("target/riscv64gc-unknown-none-elf/release/cellos-kernel-native-workload")
        .to_string_lossy()
        .into_owned()
}

fn disk_path() -> String {
    repo_root()
        .join("build/disk_srv.img")
        .to_string_lossy()
        .into_owned()
}

fn qemu_ok() -> bool {
    std::process::Command::new(qemu_binary())
        .arg("--version")
        .output()
        .is_ok()
}

const OBSERVER: Principal = Principal {
    id: 71,
    generation: 4,
};
const RECONCILER: Principal = Principal {
    id: 72,
    generation: 9,
};

fn configuration(id: u64) -> Configuration {
    Configuration {
        id,
        observer: OBSERVER,
        reconcile_authority: RECONCILER,
        observation_max_age_ticks: 20,
    }
}

fn request(job_id: u64) -> TransferRequest {
    TransferRequest {
        run_id: 100,
        job_id,
        carrier_id: 9001,
        payload_identity_digest: 9001,
        payload_item_count: 1,
        source_slot: 1,
        destination_slot: 2,
        configuration_id: 44,
        expected_trace_digest: 0xA55A,
    }
}

#[derive(Clone, Copy)]
struct Plant {
    source: Option<u64>,
    destination: Option<u64>,
    released: bool,
    sequence: u64,
}

impl Plant {
    fn ready() -> Self {
        Self {
            source: Some(9001),
            destination: None,
            released: true,
            sequence: 0,
        }
    }

    fn execute_transfer(&mut self) {
        self.destination = self.source.take();
        self.released = true;
    }
    fn admission_observation(&mut self, at: u64) -> AdmissionObservation {
        self.sequence += 1;
        AdmissionObservation {
            configuration_id: 44,
            producer: OBSERVER,
            sequence: self.sequence,
            captured_at_ticks: at,
            source_slot: 1,
            source_carrier: self.source,
            destination_slot: 2,
            destination_carrier: self.destination,
        }
    }

    fn observe(&mut self, slot: u32, at: u64) -> PlacementObservation {
        self.sequence += 1;
        let carrier_id = if slot == 2 {
            self.destination.unwrap_or(0)
        } else {
            self.source.unwrap_or(0)
        };
        PlacementObservation {
            configuration_id: 44,
            producer: OBSERVER,
            sequence: self.sequence,
            captured_at_ticks: at,
            carrier_id,
            slot,
            released: self.released,
        }
    }
}

fn dispatched(
    contract: &mut TransferContract,
    plant: &mut Plant,
    job_id: u64,
) -> AdmissionObservation {
    let admission = plant.admission_observation(90);
    assert_eq!(
        contract.admit(request(job_id), admission, 91),
        Ok(Admission::Accepted)
    );
    contract.prepare_command(job_id).unwrap();
    contract.begin_dispatch(job_id).unwrap();
    contract.command_result(job_id, true).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::AwaitingPlacement));
    admission
}

#[test]
fn command_acceptance_is_not_placement_or_completion() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    let admission = dispatched(&mut contract, &mut plant, 1);

    assert_eq!(contract.active_state(), Some(JobState::AwaitingPlacement));
    plant.execute_transfer();
    let observed = plant.observe(2, 100);
    contract.observe_placement(1, observed, 101).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));

    contract.acknowledge_trace(1, 0xA55A, 0xA55A).unwrap();
    assert_eq!(contract.active_state(), None);
    assert_eq!(contract.retained_count(), 1);
    let duplicate_observation = plant.admission_observation(102);
    assert_eq!(
        contract.admit(request(1), duplicate_observation, 103),
        Ok(Admission::Duplicate(JobState::Completed))
    );
    assert_eq!(
        contract.admit(request(2), admission, 103),
        Err(ContractError::ReorderedObservation)
    );
}

#[test]
fn admission_rejects_invalid_identity_state_and_provenance() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    let older_ready = plant.admission_observation(10);
    let mut wrong_source = plant.admission_observation(11);
    wrong_source.source_carrier = Some(7);
    assert_eq!(
        contract.admit(request(1), wrong_source, 12),
        Err(ContractError::WrongSource)
    );
    assert_eq!(
        contract.admit(request(1), older_ready, 12),
        Err(ContractError::ReorderedObservation)
    );
    let mut occupied = plant.admission_observation(12);
    occupied.destination_carrier = Some(33);
    assert_eq!(
        contract.admit(request(1), occupied, 13),
        Err(ContractError::DestinationOccupied)
    );
    let stale = plant.admission_observation(14);
    assert_eq!(
        contract.admit(request(1), stale, 35),
        Err(ContractError::StaleObservation)
    );
    let mut wrong_observer = plant.admission_observation(36);
    wrong_observer.producer.generation += 1;
    assert_eq!(
        contract.admit(request(1), wrong_observer, 37),
        Err(ContractError::WrongObserver)
    );
    let mut wrong_configuration = plant.admission_observation(38);
    wrong_configuration.configuration_id += 1;
    assert_eq!(
        contract.admit(request(1), wrong_configuration, 39),
        Err(ContractError::WrongConfiguration)
    );
    let mut invalid = request(1);
    invalid.destination_slot = invalid.source_slot;
    let observed = plant.admission_observation(40);
    assert_eq!(
        contract.admit(invalid, observed, 41),
        Err(ContractError::InvalidIdentity)
    );
}

#[test]
fn accepted_command_without_placement_requires_reconciliation() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 2);
    contract.timeout(2).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    let next_observation = plant.admission_observation(100);
    assert_eq!(
        contract.admit(request(3), next_observation, 101),
        Err(ContractError::ActiveJob)
    );
}

#[test]
fn wrong_stale_reordered_and_wrong_generation_observations_never_complete() {
    for (job_id, mutate, expected) in [
        (10, 0u8, ContractError::WrongPlacement),
        (11, 1, ContractError::StaleObservation),
        (12, 2, ContractError::ReorderedObservation),
        (13, 3, ContractError::WrongObserver),
    ] {
        let mut contract = TransferContract::new(configuration(44)).unwrap();
        let mut plant = Plant::ready();
        dispatched(&mut contract, &mut plant, job_id);
        plant.execute_transfer();
        let mut observation = plant.observe(2, 100);
        let now = match mutate {
            0 => {
                observation.slot = 3;
                101
            }
            1 => 121,
            2 => {
                observation.sequence = 0;
                101
            }
            _ => {
                observation.producer.generation += 1;
                101
            }
        };
        assert_eq!(
            contract.observe_placement(job_id, observation, now),
            Err(expected)
        );
        assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    }
}

#[test]
fn interruption_windows_preserve_known_and_uncertain_effects() {
    let mut pre_dispatch = TransferContract::new(configuration(44)).unwrap();
    let mut pre_dispatch_plant = Plant::ready();
    let pre_dispatch_observation = pre_dispatch_plant.admission_observation(1);
    pre_dispatch
        .admit(request(19), pre_dispatch_observation, 2)
        .unwrap();
    pre_dispatch.prepare_command(19).unwrap();
    pre_dispatch.interrupt(19).unwrap();
    assert_eq!(pre_dispatch.active_state(), None);
    assert_eq!(
        pre_dispatch.admit(request(19), pre_dispatch_observation, 3),
        Ok(Admission::Duplicate(JobState::FailedSafe))
    );

    let mut before = TransferContract::new(configuration(44)).unwrap();
    let mut before_plant = Plant::ready();
    let before_observation = before_plant.admission_observation(10);
    before.admit(request(20), before_observation, 11).unwrap();
    before.prepare_command(20).unwrap();
    before.begin_dispatch(20).unwrap();
    before.interrupt(20).unwrap();
    assert_eq!(before.active_state(), Some(JobState::ReconcileRequired));
    let source_observation = before_plant.observe(1, 14);
    before
        .reconcile(
            20,
            RECONCILER,
            ReconcileDecision::ConfirmedAtSource,
            Some(source_observation),
            15,
        )
        .unwrap();
    let duplicate_observation = before_plant.admission_observation(16);
    assert_eq!(
        before.admit(request(20), duplicate_observation, 17),
        Ok(Admission::Duplicate(JobState::FailedSafe))
    );

    let mut after = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut after, &mut plant, 21);
    after.interrupt(21).unwrap();
    assert_eq!(after.active_state(), Some(JobState::ReconcileRequired));

    let mut placed = TransferContract::new(configuration(44)).unwrap();
    let mut moved = Plant::ready();
    dispatched(&mut placed, &mut moved, 22);
    moved.execute_transfer();
    let observation = moved.observe(2, 100);
    placed.observe_placement(22, observation, 101).unwrap();
    placed.trace_failed(22).unwrap();
    assert_eq!(placed.active_state(), Some(JobState::PlacementObserved));
    placed.interrupt(22).unwrap();
    assert_eq!(placed.active_state(), Some(JobState::ReconcileRequired));
}

#[test]
fn duplicates_conflicts_and_retention_exhaustion_never_replay() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    for job_id in 1..=MAX_RETAINED_JOBS as u64 {
        let req = TransferRequest {
            job_id,
            ..request(job_id)
        };
        let observed = plant.admission_observation(job_id * 2);
        assert_eq!(
            contract.admit(req, observed, job_id * 2 + 1),
            Ok(Admission::Accepted)
        );
        assert_eq!(
            contract.admit(req, observed, job_id * 2 + 1),
            Ok(Admission::Duplicate(JobState::Admitted))
        );
        let mut conflict = req;
        conflict.carrier_id += 1;
        assert_eq!(
            contract.admit(conflict, observed, job_id * 2 + 1),
            Err(ContractError::ConflictingDuplicate)
        );
        contract.prepare_command(job_id).unwrap();
        contract.begin_dispatch(job_id).unwrap();
        contract.command_result(job_id, false).unwrap();
    }
    assert_eq!(contract.retained_count(), MAX_RETAINED_JOBS);
    let full_observation = plant.admission_observation(1000);
    assert_eq!(
        contract.admit(request(1000), full_observation, 1001),
        Err(ContractError::RetentionFull)
    );
}

#[test]
fn configuration_change_and_reconciliation_require_current_authority() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 30);
    let mut current_configuration = configuration(45);
    current_configuration.observer.generation += 1;
    current_configuration.reconcile_authority.generation += 1;
    contract.reconfigure(current_configuration).unwrap();
    assert_eq!(contract.configuration().id, 45);
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));

    assert_eq!(
        contract.reconcile(
            30,
            RECONCILER,
            ReconcileDecision::ConfirmedAtSource,
            None,
            100,
        ),
        Err(ContractError::UnauthorizedReconcile)
    );
    contract
        .reconcile(
            30,
            current_configuration.reconcile_authority,
            ReconcileDecision::Unknown,
            None,
            100,
        )
        .unwrap();
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    let old_observation = plant.observe(1, 100);
    assert_eq!(
        contract.reconcile(
            30,
            current_configuration.reconcile_authority,
            ReconcileDecision::ConfirmedAtSource,
            Some(old_observation),
            101,
        ),
        Err(ContractError::WrongConfiguration)
    );
    let mut current_observation = old_observation;
    current_observation.configuration_id = current_configuration.id;
    current_observation.producer = current_configuration.observer;
    current_observation.sequence = 1;
    contract
        .reconcile(
            30,
            current_configuration.reconcile_authority,
            ReconcileDecision::ConfirmedAtSource,
            Some(current_observation),
            101,
        )
        .unwrap();

    let mut new_request = request(31);
    new_request.configuration_id = current_configuration.id;
    let mut new_observation = plant.admission_observation(102);
    new_observation.configuration_id = current_configuration.id;
    new_observation.producer = current_configuration.observer;
    assert_eq!(
        contract.admit(new_request, new_observation, 103),
        Ok(Admission::Accepted)
    );
}

#[test]
fn retired_configuration_domain_cannot_be_reactivated_for_stale_readiness() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    let older_ready = plant.admission_observation(10);
    let mut newer_missing = plant.admission_observation(11);
    newer_missing.source_carrier = None;
    assert_eq!(
        contract.admit(request(70), newer_missing, 12),
        Err(ContractError::WrongSource)
    );
    contract.reconfigure(configuration(45)).unwrap();
    assert_eq!(
        contract.reconfigure(configuration(44)),
        Err(ContractError::ConfigurationReused)
    );
    assert_eq!(
        contract.admit(request(70), older_ready, 13),
        Err(ContractError::WrongConfiguration)
    );

    let mut bounded = TransferContract::new(configuration(1)).unwrap();
    for configuration_id in 2..=MAX_RETAINED_JOBS as u64 {
        bounded
            .reconfigure(configuration(configuration_id))
            .unwrap();
    }
    assert_eq!(
        bounded.reconfigure(configuration(MAX_RETAINED_JOBS as u64 + 1)),
        Err(ContractError::ConfigurationCapacityFull)
    );
}

#[test]
fn confirmed_destination_still_requires_observation_and_frozen_trace_readback() {
    let mut contract = TransferContract::new(configuration(44)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 40);
    contract.timeout(40).unwrap();
    assert_eq!(
        contract.reconcile(
            40,
            RECONCILER,
            ReconcileDecision::ConfirmedAtDestination,
            None,
            100,
        ),
        Err(ContractError::MissingObservation)
    );
    plant.execute_transfer();
    let destination_observation = plant.observe(2, 100);
    contract
        .reconcile(
            40,
            RECONCILER,
            ReconcileDecision::ConfirmedAtDestination,
            Some(destination_observation),
            101,
        )
        .unwrap();
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));
    assert_eq!(
        contract.acknowledge_trace(40, 7, 7),
        Err(ContractError::TraceMismatch)
    );
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));
    contract.acknowledge_trace(40, 0xA55A, 0xA55A).unwrap();
}

#[test]
fn riscv64_lab_carrier_transfer_qemu() {
    let kernel = PathBuf::from(kernel_path());
    let disk = PathBuf::from(disk_path());
    if !kernel.exists() || !disk.exists() || !qemu_ok() {
        return;
    }

    let tmp = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()
        .expect("create temp disk");
    std::fs::copy(disk_path(), tmp.path()).expect("copy srv disk");

    let mut qemu = QemuRunner::boot_rv64_with_disk(&kernel_path(), tmp.path().to_str().unwrap());

    qemu.wait_for("Cellos >", 60)
        .unwrap_or_else(|e| panic!("shell not reached: {e}\n{}", qemu.dump()));

    std::thread::sleep(std::time::Duration::from_millis(500));

    // Run the native LAB-01 carrier transfer bench
    qemu.send_line("bench lab-carrier-transfer");

    qemu.wait_for("[lab-carrier-transfer] ALL CRITERIA PASSED", 60)
        .unwrap_or_else(|e| {
            panic!(
                "LAB-01 transfer failed or timed out: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    let serial = qemu.dump();
    assert!(serial.contains("[lab-carrier-transfer] nominal transfer completed and trace verified on VFS"));
    assert!(serial.contains("[lab-carrier-transfer] duplicate and conflicting admissions verified"));
    assert!(serial.contains("[lab-carrier-transfer] reconciliation required and resolved via authoritative observation"));
    assert!(serial.contains("[lab-carrier-transfer] configuration change invalidates old observations"));
    assert!(serial.contains("[lab-carrier-transfer] all trace records verified on CellosFS Native"));
}
