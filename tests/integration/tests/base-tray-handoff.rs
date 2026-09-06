#[path = "../../../cells/tests/bench/src/scenarios/base_tray_handoff.rs"]
mod base_tray_handoff;
#[allow(dead_code)]
#[path = "../../../cells/tests/bench/src/scenarios/lab_transfer_contract.rs"]
mod lab_transfer_contract;

use base_tray_handoff::{
    BaseAdmissionObservation, BaseError, BaseHandoffContract, BaseRequest, BaseStationObservation,
    InventoryIdentity,
};
use lab_transfer_contract::{
    Admission, AdmissionObservation, Configuration, ContractError, JobState, PlacementObservation,
    Principal, ReconcileDecision,
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
    id: 81,
    generation: 3,
};
const RECONCILER: Principal = Principal {
    id: 82,
    generation: 5,
};
const INVENTORY: InventoryIdentity = InventoryIdentity {
    item_count: 4,
    digest: 0x1A2B_3C4D,
};

fn configuration(id: u64) -> Configuration {
    Configuration {
        id,
        observer: OBSERVER,
        reconcile_authority: RECONCILER,
        observation_max_age_ticks: 20,
    }
}

fn request(job_id: u64) -> BaseRequest {
    BaseRequest {
        run_id: 200,
        job_id,
        tray_id: 7001,
        inventory: INVENTORY,
        source_station: 11,
        destination_station: 12,
        configuration_id: 55,
        expected_trace_digest: 0xBACE,
    }
}

#[derive(Clone, Copy)]
struct Plant {
    station: u32,
    tray_id: u64,
    inventory: InventoryIdentity,
    stationary: bool,
    secure: bool,
    safety_eligible: bool,
    arm_parked: bool,
    sequence: u64,
    configuration_id: u64,
    observer: Principal,
}

impl Plant {
    fn ready() -> Self {
        Self {
            station: 11,
            tray_id: 7001,
            inventory: INVENTORY,
            stationary: true,
            secure: true,
            safety_eligible: true,
            arm_parked: true,
            sequence: 0,
            configuration_id: 55,
            observer: OBSERVER,
        }
    }

    fn admission(&mut self, at: u64) -> BaseAdmissionObservation {
        self.sequence += 1;
        BaseAdmissionObservation {
            common: AdmissionObservation {
                configuration_id: self.configuration_id,
                producer: self.observer,
                sequence: self.sequence,
                captured_at_ticks: at,
                source_slot: 11,
                source_carrier: (self.station == 11).then_some(self.tray_id),
                destination_slot: 12,
                destination_carrier: (self.station == 12).then_some(self.tray_id),
            },
            inventory: self.inventory,
            stationary: self.stationary,
            tray_secure: self.secure,
            local_safety_eligible: self.safety_eligible,
            arm_parked: self.arm_parked,
        }
    }

    fn station_observation(&mut self, at: u64) -> BaseStationObservation {
        self.sequence += 1;
        BaseStationObservation {
            common: PlacementObservation {
                configuration_id: self.configuration_id,
                producer: self.observer,
                sequence: self.sequence,
                captured_at_ticks: at,
                carrier_id: self.tray_id,
                slot: self.station,
                released: true,
            },
            inventory: self.inventory,
            stationary: self.stationary,
            tray_secure: self.secure,
            local_handoff_eligible: self.safety_eligible,
            arm_parked: self.arm_parked,
        }
    }

    fn deliver(&mut self) {
        self.station = 12;
    }
}

fn dispatched(contract: &mut BaseHandoffContract, plant: &mut Plant, job_id: u64) {
    let observation = plant.admission(10);
    assert_eq!(
        contract.admit(request(job_id), observation, 11),
        Ok(Admission::Accepted)
    );
    contract.prepare_command(job_id).unwrap();
    contract.begin_dispatch(job_id).unwrap();
    contract.command_result(job_id, true).unwrap();
}

#[test]
fn accepted_delivery_requires_actual_arrival_and_frozen_trace_readback() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 1);
    assert_eq!(contract.active_state(), Some(JobState::AwaitingPlacement));

    plant.deliver();
    let arrival = plant.station_observation(20);
    contract.observe_arrival(1, arrival, 21).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));
    contract.trace_failed(1).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));
    let confirmed_destination = plant.admission(22);
    assert_eq!(
        contract.admit(request(2), confirmed_destination, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));
    assert_eq!(
        contract.acknowledge_trace(1, 7, 7),
        Err(BaseError::Common(ContractError::TraceMismatch))
    );
    contract.acknowledge_trace(1, 0xBACE, 0xBACE).unwrap();
    assert_eq!(contract.active_state(), None);
    assert_eq!(contract.retained_count(), 1);
}

#[test]
fn changed_destination_inventory_revokes_confirmed_placement() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 3);
    plant.deliver();
    let arrival = plant.station_observation(20);
    contract.observe_arrival(3, arrival, 21).unwrap();

    let mut changed_inventory = plant.admission(22);
    changed_inventory.inventory.digest += 1;
    assert_eq!(
        contract.admit(request(3), changed_inventory, 23),
        Ok(Admission::Duplicate(JobState::ReconcileRequired))
    );
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    assert_eq!(
        contract.acknowledge_trace(3, 0xBACE, 0xBACE),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
}

#[test]
fn duplicate_source_and_destination_custody_revokes_confirmed_placement() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 5);
    plant.deliver();
    let arrival = plant.station_observation(20);
    contract.observe_arrival(5, arrival, 21).unwrap();

    let mut duplicated = plant.admission(22);
    duplicated.common.source_carrier = Some(plant.tray_id);
    assert_eq!(
        contract.admit(request(6), duplicated, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    assert_eq!(
        contract.acknowledge_trace(5, 0xBACE, 0xBACE),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
}

#[test]
fn admission_rejects_insecure_moving_ineligible_and_arm_active_plants() {
    for (case, expected) in [
        (0u8, BaseError::TrayInsecure),
        (1, BaseError::NotStationary),
        (2, BaseError::LocalSafetyIneligible),
        (3, BaseError::ArmActive),
        (4, BaseError::WrongInventory),
        (5, BaseError::Common(ContractError::WrongSource)),
        (6, BaseError::Common(ContractError::WrongSource)),
        (7, BaseError::Common(ContractError::DestinationOccupied)),
    ] {
        let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
        let mut plant = Plant::ready();
        let mut observation = plant.admission(10);
        match case {
            0 => observation.tray_secure = false,
            1 => observation.stationary = false,
            2 => observation.local_safety_eligible = false,
            3 => observation.arm_parked = false,
            4 => observation.inventory.digest += 1,
            5 => observation.common.source_carrier = Some(99),
            6 => observation.common.source_slot = 99,
            _ => observation.common.destination_carrier = Some(77),
        }
        assert_eq!(
            contract.admit(request(10 + case as u64), observation, 11),
            Err(expected)
        );
        assert_eq!(contract.active_state(), None);
    }
}

#[test]
fn authoritative_newer_refusal_prevents_older_ready_replay() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    let older_ready = plant.admission(10);
    plant.secure = false;
    let newer_insecure = plant.admission(11);
    assert_eq!(
        contract.admit(request(20), newer_insecure, 12),
        Err(BaseError::TrayInsecure)
    );
    assert_eq!(
        contract.admit(request(20), older_ready, 12),
        Err(BaseError::Common(ContractError::ReorderedObservation))
    );
}

#[test]
fn dispatch_interruption_blocks_replay_until_observed_reconciliation() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    let admission = plant.admission(10);
    contract.admit(request(30), admission, 11).unwrap();
    contract.prepare_command(30).unwrap();
    contract.begin_dispatch(30).unwrap();
    contract.interrupt(30).unwrap();
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    let next = plant.admission(12);
    assert_eq!(
        contract.admit(request(31), next, 13),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(
        contract.reconcile(
            30,
            RECONCILER,
            ReconcileDecision::ConfirmedAtSource,
            None,
            14
        ),
        Err(BaseError::Common(ContractError::MissingObservation))
    );
    let source = plant.station_observation(14);
    contract
        .reconcile(
            30,
            RECONCILER,
            ReconcileDecision::ConfirmedAtSource,
            Some(source),
            15,
        )
        .unwrap();
    assert_eq!(contract.active_state(), None);

    let mut destination = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut moved = Plant::ready();
    dispatched(&mut destination, &mut moved, 32);
    destination.interrupt(32).unwrap();
    moved.deliver();
    let destination_observation = moved.station_observation(20);
    destination
        .reconcile(
            32,
            RECONCILER,
            ReconcileDecision::ConfirmedAtDestination,
            Some(destination_observation),
            21,
        )
        .unwrap();
    assert_eq!(
        destination.active_state(),
        Some(JobState::PlacementObserved)
    );
}

#[test]
fn wrong_stale_generation_inventory_and_station_observations_never_complete() {
    for (case, expected) in [
        (0u8, BaseError::Common(ContractError::WrongPlacement)),
        (1, BaseError::Common(ContractError::StaleObservation)),
        (2, BaseError::Common(ContractError::WrongObserver)),
        (3, BaseError::WrongInventory),
        (4, BaseError::NotStationary),
        (5, BaseError::TrayInsecure),
        (6, BaseError::ArmActive),
        (7, BaseError::LocalSafetyIneligible),
    ] {
        let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
        let mut plant = Plant::ready();
        dispatched(&mut contract, &mut plant, 40 + case as u64);
        plant.deliver();
        let mut observation = plant.station_observation(20);
        let now = match case {
            0 => {
                observation.common.slot = 13;
                21
            }
            1 => 41,
            2 => {
                observation.common.producer.generation += 1;
                21
            }
            3 => {
                observation.inventory.digest += 1;
                21
            }
            4 => {
                observation.stationary = false;
                21
            }
            5 => {
                observation.tray_secure = false;
                21
            }
            6 => {
                observation.arm_parked = false;
                21
            }
            _ => {
                observation.local_handoff_eligible = false;
                21
            }
        };
        assert_eq!(
            contract.observe_arrival(40 + case as u64, observation, now),
            Err(expected)
        );
        assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    }
}

#[test]
fn configuration_rotation_rejects_old_evidence_and_accepts_new_sequence_domain() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 50);
    contract.timeout(50).unwrap();

    let mut current = configuration(56);
    current.observer.generation += 1;
    current.reconcile_authority.generation += 1;
    contract.reconfigure(current).unwrap();
    assert_eq!(contract.configuration(), current);
    let old = plant.station_observation(20);
    assert_eq!(
        contract.reconcile(
            50,
            current.reconcile_authority,
            ReconcileDecision::ConfirmedAtSource,
            Some(old),
            21,
        ),
        Err(BaseError::Common(ContractError::WrongConfiguration))
    );
    let mut current_observation = old;
    current_observation.common.configuration_id = current.id;
    current_observation.common.producer = current.observer;
    current_observation.common.sequence = 1;
    contract
        .reconcile(
            50,
            current.reconcile_authority,
            ReconcileDecision::ConfirmedAtSource,
            Some(current_observation),
            21,
        )
        .unwrap();
    assert_eq!(
        contract.reconfigure(configuration(55)),
        Err(BaseError::Common(ContractError::ConfigurationReused))
    );
}

#[test]
fn newer_cross_job_exclusion_revokes_stale_dispatch_authority() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    let ready = plant.admission(10);
    contract.admit(request(60), ready, 11).unwrap();

    plant.arm_parked = false;
    let excluded = plant.admission(12);
    assert_eq!(
        contract.admit(request(61), excluded, 13),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(
        contract.prepare_command(60),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
    assert_eq!(contract.active_state(), None);

    let mut uncertain = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut uncertain_plant = Plant::ready();
    let uncertain_ready = uncertain_plant.admission(20);
    uncertain.admit(request(62), uncertain_ready, 21).unwrap();
    uncertain.prepare_command(62).unwrap();
    uncertain.begin_dispatch(62).unwrap();
    uncertain_plant.arm_parked = false;
    let uncertain_exclusion = uncertain_plant.admission(22);
    assert_eq!(
        uncertain.admit(request(63), uncertain_exclusion, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(uncertain.active_state(), Some(JobState::ReconcileRequired));
    assert_eq!(
        uncertain.command_result(62, true),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );

    let mut contradicted = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut contradicted_plant = Plant::ready();
    let contradicted_ready = contradicted_plant.admission(30);
    contradicted
        .admit(request(64), contradicted_ready, 31)
        .unwrap();
    let mut competing_request = request(65);
    competing_request.source_station = 13;
    competing_request.destination_station = 14;
    let mut competing_observation = contradicted_plant.admission(32);
    competing_observation.common.source_slot = 13;
    competing_observation.common.destination_slot = 14;
    assert_eq!(
        contradicted.admit(competing_request, competing_observation, 33),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(contradicted.active_state(), None);
    assert_eq!(
        contradicted.prepare_command(64),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
}

#[test]
fn overlapping_station_evidence_invalidates_active_custody_independently() {
    let mut source_before_dispatch = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut source_plant = Plant::ready();
    source_before_dispatch
        .admit(request(70), source_plant.admission(10), 11)
        .unwrap();
    let mut source_absent = source_plant.admission(12);
    source_absent.common.source_carrier = None;
    source_absent.common.destination_slot = 14;
    let mut source_query = request(71);
    source_query.destination_station = 14;
    assert_eq!(
        source_before_dispatch.admit(source_query, source_absent, 13),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(source_before_dispatch.active_state(), None);

    let mut destination_before_dispatch = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut destination_plant = Plant::ready();
    destination_before_dispatch
        .admit(request(72), destination_plant.admission(20), 21)
        .unwrap();
    let mut destination_occupied = destination_plant.admission(22);
    destination_occupied.common.source_slot = 13;
    destination_occupied.common.source_carrier = Some(8000);
    destination_occupied.common.destination_carrier = Some(9000);
    let mut destination_query = request(73);
    destination_query.tray_id = 8000;
    destination_query.source_station = 13;
    assert_eq!(
        destination_before_dispatch.admit(destination_query, destination_occupied, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(destination_before_dispatch.active_state(), None);

    let mut source_after_placement = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut placed_source_plant = Plant::ready();
    dispatched(&mut source_after_placement, &mut placed_source_plant, 74);
    placed_source_plant.deliver();
    source_after_placement
        .observe_arrival(74, placed_source_plant.station_observation(20), 21)
        .unwrap();
    let mut duplicated_at_source = placed_source_plant.admission(22);
    duplicated_at_source.common.source_carrier = Some(7001);
    duplicated_at_source.common.destination_slot = 14;
    duplicated_at_source.common.destination_carrier = None;
    let mut placed_source_query = request(75);
    placed_source_query.destination_station = 14;
    assert_eq!(
        source_after_placement.admit(placed_source_query, duplicated_at_source, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(
        source_after_placement.active_state(),
        Some(JobState::ReconcileRequired)
    );
    assert_eq!(
        source_after_placement.acknowledge_trace(74, 0xBACE, 0xBACE),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );

    let mut destination_after_placement = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut placed_destination_plant = Plant::ready();
    dispatched(
        &mut destination_after_placement,
        &mut placed_destination_plant,
        76,
    );
    placed_destination_plant.deliver();
    destination_after_placement
        .observe_arrival(76, placed_destination_plant.station_observation(30), 31)
        .unwrap();
    let mut absent_at_destination = placed_destination_plant.admission(32);
    absent_at_destination.common.source_slot = 13;
    absent_at_destination.common.source_carrier = Some(8000);
    absent_at_destination.common.destination_carrier = None;
    let mut placed_destination_query = request(77);
    placed_destination_query.tray_id = 8000;
    placed_destination_query.source_station = 13;
    assert_eq!(
        destination_after_placement.admit(placed_destination_query, absent_at_destination, 33,),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(
        destination_after_placement.active_state(),
        Some(JobState::ReconcileRequired)
    );
    assert_eq!(
        destination_after_placement.acknowledge_trace(76, 0xBACE, 0xBACE),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
}

#[test]
fn aliased_station_entries_cannot_hide_a_custody_contradiction() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    dispatched(&mut contract, &mut plant, 78);
    plant.deliver();
    contract
        .observe_arrival(78, plant.station_observation(20), 21)
        .unwrap();

    let mut contradiction = plant.admission(22);
    contradiction.common.source_slot = 12;
    contradiction.common.source_carrier = Some(7001);
    contradiction.common.destination_slot = 12;
    contradiction.common.destination_carrier = None;
    let mut aliased_query = request(79);
    aliased_query.source_station = 12;
    assert_eq!(
        contract.admit(aliased_query, contradiction, 23),
        Err(BaseError::Common(ContractError::ActiveJob))
    );
    assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));
    assert_eq!(
        contract.acknowledge_trace(78, 0xBACE, 0xBACE),
        Err(BaseError::Common(ContractError::InvalidTransition))
    );
}

#[test]
fn duplicate_and_conflicting_inventory_never_dispatch_again() {
    let mut contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut plant = Plant::ready();
    let observation = plant.admission(10);
    assert_eq!(
        contract.admit(request(60), observation, 11),
        Ok(Admission::Accepted)
    );
    let duplicate_observation = plant.admission(12);
    assert_eq!(
        contract.admit(request(60), duplicate_observation, 13),
        Ok(Admission::Duplicate(JobState::Admitted))
    );
    let mut conflict = request(60);
    conflict.inventory.digest += 1;
    let conflict_observation = plant.admission(14);
    assert_eq!(
        contract.admit(conflict, conflict_observation, 15),
        Err(BaseError::Common(ContractError::ConflictingDuplicate))
    );
    plant.arm_parked = false;
    let excluded = plant.admission(14);
    assert_eq!(
        contract.admit(request(60), excluded, 15),
        Ok(Admission::Duplicate(JobState::FailedSafe))
    );
    assert_eq!(contract.active_state(), None);
    assert_eq!(contract.retained_count(), 1);

    let mut source_contract = BaseHandoffContract::new(configuration(55)).unwrap();
    let mut source_plant = Plant::ready();
    let source_admission = source_plant.admission(20);
    source_contract
        .admit(request(61), source_admission, 21)
        .unwrap();
    source_plant.station = 12;
    let missing_source = source_plant.admission(22);
    assert_eq!(
        source_contract.admit(request(61), missing_source, 23),
        Ok(Admission::Duplicate(JobState::FailedSafe))
    );
    assert_eq!(source_contract.active_state(), None);
    let terminal_status = source_plant.admission(24);
    assert_eq!(
        source_contract.admit(request(61), terminal_status, 25),
        Ok(Admission::Duplicate(JobState::FailedSafe))
    );
}

#[test]
fn riscv64_base_tray_handoff_qemu() {
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

    // Run the native BASE-01 tray handoff bench
    qemu.send_line("bench base-tray-handoff");

    qemu.wait_for("[base-tray-handoff] ALL CRITERIA PASSED", 60)
        .unwrap_or_else(|e| {
            panic!(
                "BASE-01 handoff failed or timed out: {e}\n--- serial output ---\n{}",
                qemu.dump()
            )
        });

    let serial = qemu.dump();
    assert!(serial.contains("[base-tray-handoff] nominal tray delivery completed and trace verified on VFS"));
    assert!(serial.contains("[base-tray-handoff] arm-active and insecurity exclusions verified"));
    assert!(serial.contains("[base-tray-handoff] reconciliation required and resolved via authoritative observation"));
    assert!(serial.contains("[base-tray-handoff] configuration change invalidates old observations"));
    assert!(serial.contains("[base-tray-handoff] all trace records verified on CellosFS Native"));
}
