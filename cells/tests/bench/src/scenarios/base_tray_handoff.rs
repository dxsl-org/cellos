//! Pure BASE-01 tray handoff rules layered on the shared workflow contract.
//!
//! This is a model-only contract. It does not command a drive, establish
//! physical station arrival, or provide a safety mechanism.

use super::lab_transfer_contract::{
    validate_provenance, Admission, AdmissionObservation, Configuration, ContractError, JobState,
    PlacementObservation, Principal, ReconcileDecision, TransferContract, TransferRequest,
};

pub const MAX_TRAY_ITEMS: u16 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InventoryIdentity {
    pub item_count: u16,
    pub digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseRequest {
    pub run_id: u64,
    pub job_id: u64,
    pub tray_id: u64,
    pub inventory: InventoryIdentity,
    pub source_station: u32,
    pub destination_station: u32,
    pub configuration_id: u64,
    pub expected_trace_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseAdmissionObservation {
    pub common: AdmissionObservation,
    pub inventory: InventoryIdentity,
    pub stationary: bool,
    pub tray_secure: bool,
    pub local_safety_eligible: bool,
    pub arm_parked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaseStationObservation {
    pub common: PlacementObservation,
    pub inventory: InventoryIdentity,
    pub stationary: bool,
    pub tray_secure: bool,
    pub local_handoff_eligible: bool,
    pub arm_parked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaseError {
    Common(ContractError),
    InvalidInventory,
    WrongInventory,
    NotStationary,
    TrayInsecure,
    LocalSafetyIneligible,
    ArmActive,
}

impl From<ContractError> for BaseError {
    fn from(error: ContractError) -> Self {
        Self::Common(error)
    }
}

pub struct BaseHandoffContract {
    common: TransferContract,
    active_request: Option<BaseRequest>,
    configuration: Configuration,
    last_observation_sequence: u64,
}

impl BaseHandoffContract {
    pub fn new(configuration: Configuration) -> Result<Self, BaseError> {
        Ok(Self {
            common: TransferContract::new(configuration)?,
            active_request: None,
            configuration,
            last_observation_sequence: 0,
        })
    }
    pub fn configuration(&self) -> Configuration {
        self.common.configuration()
    }

    pub fn active_state(&self) -> Option<JobState> {
        self.common.active_state()
    }

    pub fn retained_count(&self) -> usize {
        self.common.retained_count()
    }

    pub fn admit(
        &mut self,
        request: BaseRequest,
        observation: BaseAdmissionObservation,
        now_ticks: u64,
    ) -> Result<Admission, BaseError> {
        let common_request = common_request(request)?;
        if request.configuration_id != self.configuration.id {
            return Err(BaseError::Common(ContractError::WrongConfiguration));
        }
        validate_inventory(request.inventory)?;
        self.validate_and_consume(
            observation.common.configuration_id,
            observation.common.producer,
            observation.common.sequence,
            observation.common.captured_at_ticks,
            now_ticks,
        )?;

        let had_active = self.active_request.is_some();
        self.interrupt_active_if_excluded(observation);
        if let Some(existing) = self.common.existing_admission(common_request) {
            return existing.map_err(BaseError::Common);
        }
        if had_active {
            return Err(BaseError::Common(ContractError::ActiveJob));
        }

        Self::validate_admission_contents(request, observation)?;
        let admission = self
            .common
            .admit(common_request, observation.common, now_ticks)?;
        if admission == Admission::Accepted {
            self.active_request = Some(request);
        }
        Ok(admission)
    }

    pub fn prepare_command(&mut self, job_id: u64) -> Result<(), BaseError> {
        self.common
            .prepare_command(job_id)
            .map_err(BaseError::Common)
    }

    pub fn begin_dispatch(&mut self, job_id: u64) -> Result<(), BaseError> {
        self.common
            .begin_dispatch(job_id)
            .map_err(BaseError::Common)
    }

    pub fn command_result(&mut self, job_id: u64, accepted: bool) -> Result<(), BaseError> {
        self.common.command_result(job_id, accepted)?;
        if !accepted {
            self.active_request = None;
        }
        Ok(())
    }

    pub fn observe_arrival(
        &mut self,
        job_id: u64,
        observation: BaseStationObservation,
        now_ticks: u64,
    ) -> Result<(), BaseError> {
        if self.common.active_state() != Some(JobState::AwaitingPlacement) {
            return Err(BaseError::Common(ContractError::InvalidTransition));
        }
        let request = self
            .active_request
            .filter(|request| request.job_id == job_id)
            .ok_or(BaseError::Common(ContractError::InvalidTransition))?;
        if let Err(error) = self.validate_station_observation(request, observation, now_ticks) {
            let _ = self.common.interrupt(job_id);
            return Err(error);
        }
        self.common
            .observe_placement(job_id, observation.common, now_ticks)
            .map_err(BaseError::Common)
    }

    pub fn timeout(&mut self, job_id: u64) -> Result<(), BaseError> {
        self.common.timeout(job_id)?;
        if self.common.active_state().is_none() {
            self.active_request = None;
        }
        Ok(())
    }

    pub fn interrupt(&mut self, job_id: u64) -> Result<(), BaseError> {
        self.timeout(job_id)
    }

    pub fn reconfigure(&mut self, configuration: Configuration) -> Result<(), BaseError> {
        let provenance_changed = configuration.id != self.configuration.id
            || configuration.observer != self.configuration.observer;
        self.common.reconfigure(configuration)?;
        if provenance_changed {
            self.last_observation_sequence = 0;
        }
        self.configuration = configuration;
        Ok(())
    }

    pub fn reconcile(
        &mut self,
        job_id: u64,
        authority: Principal,
        decision: ReconcileDecision,
        observation: Option<BaseStationObservation>,
        now_ticks: u64,
    ) -> Result<(), BaseError> {
        if self.common.active_state() != Some(JobState::ReconcileRequired) {
            return Err(BaseError::Common(ContractError::InvalidTransition));
        }
        if authority != self.configuration.reconcile_authority {
            return Err(BaseError::Common(ContractError::UnauthorizedReconcile));
        }
        if decision == ReconcileDecision::Unknown {
            return self
                .common
                .reconcile(job_id, authority, decision, None, now_ticks)
                .map_err(BaseError::Common);
        }
        let request = self
            .active_request
            .filter(|request| request.job_id == job_id)
            .ok_or(BaseError::Common(ContractError::InvalidTransition))?;
        let observation =
            observation.ok_or(BaseError::Common(ContractError::MissingObservation))?;
        self.validate_station_observation(request, observation, now_ticks)?;
        self.common.reconcile(
            job_id,
            authority,
            decision,
            Some(observation.common),
            now_ticks,
        )?;
        if decision == ReconcileDecision::ConfirmedAtSource {
            self.active_request = None;
        }
        Ok(())
    }

    pub fn trace_failed(&mut self, job_id: u64) -> Result<(), BaseError> {
        self.common.trace_failed(job_id).map_err(BaseError::Common)
    }

    pub fn acknowledge_trace(
        &mut self,
        job_id: u64,
        acknowledged_digest: u64,
        readback_digest: u64,
    ) -> Result<(), BaseError> {
        self.common
            .acknowledge_trace(job_id, acknowledged_digest, readback_digest)?;
        self.active_request = None;
        Ok(())
    }

    fn validate_admission_contents(
        request: BaseRequest,
        observation: BaseAdmissionObservation,
    ) -> Result<(), BaseError> {
        if observation.common.source_slot != request.source_station
            || observation.common.source_carrier != Some(request.tray_id)
        {
            return Err(BaseError::Common(ContractError::WrongSource));
        }
        if observation.common.destination_slot != request.destination_station
            || observation.common.destination_carrier.is_some()
        {
            return Err(BaseError::Common(ContractError::DestinationOccupied));
        }
        if observation.inventory != request.inventory {
            return Err(BaseError::WrongInventory);
        }
        if !observation.stationary {
            return Err(BaseError::NotStationary);
        }
        if !observation.tray_secure {
            return Err(BaseError::TrayInsecure);
        }
        if !observation.local_safety_eligible {
            return Err(BaseError::LocalSafetyIneligible);
        }
        if !observation.arm_parked {
            return Err(BaseError::ArmActive);
        }
        Ok(())
    }

    fn interrupt_active_if_excluded(&mut self, observation: BaseAdmissionObservation) -> bool {
        let Some(active) = self.active_request else {
            return false;
        };
        let state = self.common.active_state();
        let globally_ineligible = !observation.stationary
            || !observation.local_safety_eligible
            || !observation.arm_parked;
        let source_station_has_wrong_carrier = observation.common.source_slot
            == active.source_station
            && observation.common.source_carrier != Some(active.tray_id)
            || observation.common.destination_slot == active.source_station
                && observation.common.destination_carrier != Some(active.tray_id);
        let source_station_has_active_tray = observation.common.source_slot
            == active.source_station
            && observation.common.source_carrier == Some(active.tray_id)
            || observation.common.destination_slot == active.source_station
                && observation.common.destination_carrier == Some(active.tray_id);
        let destination_station_has_wrong_carrier = observation.common.source_slot
            == active.destination_station
            && observation.common.source_carrier != Some(active.tray_id)
            || observation.common.destination_slot == active.destination_station
                && observation.common.destination_carrier != Some(active.tray_id);
        let destination_station_is_occupied = observation.common.source_slot
            == active.destination_station
            && observation.common.source_carrier.is_some()
            || observation.common.destination_slot == active.destination_station
                && observation.common.destination_carrier.is_some();
        let destination_station_has_active_tray = observation.common.source_slot
            == active.destination_station
            && observation.common.source_carrier == Some(active.tray_id)
            || observation.common.destination_slot == active.destination_station
                && observation.common.destination_carrier == Some(active.tray_id);
        let active_at_observed_source = observation.common.source_carrier == Some(active.tray_id);
        let active_at_observed_destination =
            observation.common.destination_carrier == Some(active.tray_id);
        let active_at_unexpected_station = active_at_observed_source
            && observation.common.source_slot != active.destination_station
            || active_at_observed_destination
                && observation.common.destination_slot != active.destination_station;
        let active_at_unexpected_predispatch_station = active_at_observed_source
            && observation.common.source_slot != active.source_station
            || active_at_observed_destination
                && observation.common.destination_slot != active.source_station;
        let location_or_custody_excluded = if state == Some(JobState::PlacementObserved) {
            active_at_unexpected_station
                || source_station_has_active_tray
                || destination_station_has_wrong_carrier
                || destination_station_has_active_tray
                    && (observation.inventory != active.inventory || !observation.tray_secure)
        } else {
            active_at_unexpected_predispatch_station
                || source_station_has_wrong_carrier
                || destination_station_is_occupied
                || source_station_has_active_tray
                    && (observation.inventory != active.inventory || !observation.tray_secure)
        };
        if !globally_ineligible && !location_or_custody_excluded {
            return false;
        }
        let _ = self.common.interrupt(active.job_id);
        if self.common.active_state().is_none() {
            self.active_request = None;
        }
        true
    }

    fn validate_station_observation(
        &mut self,
        request: BaseRequest,
        observation: BaseStationObservation,
        now_ticks: u64,
    ) -> Result<(), BaseError> {
        self.validate_and_consume(
            observation.common.configuration_id,
            observation.common.producer,
            observation.common.sequence,
            observation.common.captured_at_ticks,
            now_ticks,
        )?;
        if observation.inventory != request.inventory {
            return Err(BaseError::WrongInventory);
        }
        if !observation.stationary {
            return Err(BaseError::NotStationary);
        }
        if !observation.tray_secure {
            return Err(BaseError::TrayInsecure);
        }
        if !observation.local_handoff_eligible {
            return Err(BaseError::LocalSafetyIneligible);
        }
        if !observation.arm_parked {
            return Err(BaseError::ArmActive);
        }
        Ok(())
    }

    fn validate_and_consume(
        &mut self,
        configuration_id: u64,
        producer: Principal,
        sequence: u64,
        captured_at_ticks: u64,
        now_ticks: u64,
    ) -> Result<(), BaseError> {
        validate_provenance(
            self.configuration,
            configuration_id,
            producer,
            sequence,
            captured_at_ticks,
            now_ticks,
        )?;
        if sequence <= self.last_observation_sequence {
            return Err(BaseError::Common(ContractError::ReorderedObservation));
        }
        self.last_observation_sequence = sequence;
        Ok(())
    }
}

fn common_request(request: BaseRequest) -> Result<TransferRequest, BaseError> {
    validate_inventory(request.inventory)?;
    Ok(TransferRequest {
        run_id: request.run_id,
        job_id: request.job_id,
        carrier_id: request.tray_id,
        payload_identity_digest: request.inventory.digest,
        payload_item_count: request.inventory.item_count,
        source_slot: request.source_station,
        destination_slot: request.destination_station,
        configuration_id: request.configuration_id,
        expected_trace_digest: request.expected_trace_digest,
    })
}

fn validate_inventory(inventory: InventoryIdentity) -> Result<(), BaseError> {
    if inventory.item_count == 0 || inventory.item_count > MAX_TRAY_ITEMS || inventory.digest == 0 {
        return Err(BaseError::InvalidInventory);
    }
    Ok(())
}

#[cfg(target_os = "none")]
#[allow(unused_imports)]
pub use runner::run;

#[cfg(target_os = "none")]
mod runner {
    use super::*;
    const TRACE_LOG_PATH: &str = "/srv/base_tray_trace.log";

    const OBSERVER: Principal = Principal {
        id: 30,
        generation: 1,
    };

    const RECONCILE_AUTHORITY: Principal = Principal {
        id: 40,
        generation: 1,
    };

    const INITIAL_CONFIG: Configuration = Configuration {
        id: 201,
        observer: OBSERVER,
        reconcile_authority: RECONCILE_AUTHORITY,
        observation_max_age_ticks: 1000,
    };

    pub fn run() {
        ostd::io::println("[base-tray-handoff] START: BASE-01 native QEMU witness");
        ostd::syscall::sys_heartbeat(0);

        let Some(vfs_tid) = ostd::syscall::sys_lookup_service(api::syscall::service::VFS) else {
            fail("VFS service is not registered");
        };
        ostd::io::println(&alloc::format!(
            "[base-tray-handoff] VFS service registered: tid={vfs_tid}"
        ));

        let mut vfs_client = ostd::clients::VfsClient::new();
        let _ = vfs_client.unlink(TRACE_LOG_PATH);

        let mut contract = BaseHandoffContract::new(INITIAL_CONFIG)
            .unwrap_or_else(|_| fail("failed to initialize BaseHandoffContract"));

        let mut current_ticks: u64 = 100;

        // ── Scenario 1: Nominal BASE-01 Tray Delivery ───────────────────────────
        ostd::io::println(
            "[base-tray-handoff] Scenario 1: Nominal tray delivery (station 1 -> station 2)",
        );
        let req_1 = BaseRequest {
            run_id: 1,
            job_id: 2001,
            tray_id: 55,
            inventory: InventoryIdentity {
                item_count: 4,
                digest: 0xBEEF_0001,
            },
            source_station: 1,
            destination_station: 2,
            configuration_id: 201,
            expected_trace_digest: 0xE00D_0001,
        };

        let adm_obs_1 = BaseAdmissionObservation {
            common: AdmissionObservation {
                configuration_id: 201,
                producer: OBSERVER,
                sequence: 1,
                captured_at_ticks: current_ticks,
                source_slot: 1,
                source_carrier: Some(55),
                destination_slot: 2,
                destination_carrier: None,
            },
            inventory: InventoryIdentity {
                item_count: 4,
                digest: 0xBEEF_0001,
            },
            stationary: true,
            tray_secure: true,
            local_safety_eligible: true,
            arm_parked: true,
        };

        let admission = contract
            .admit(req_1, adm_obs_1, current_ticks)
            .unwrap_or_else(|_| fail("nominal admit failed"));
        assert_eq!(admission, Admission::Accepted);
        assert_eq!(contract.active_state(), Some(JobState::Admitted));

        contract
            .prepare_command(2001)
            .unwrap_or_else(|_| fail("prepare_command failed"));
        assert_eq!(contract.active_state(), Some(JobState::CommandPending));

        contract
            .begin_dispatch(2001)
            .unwrap_or_else(|_| fail("begin_dispatch failed"));
        assert_eq!(contract.active_state(), Some(JobState::Dispatching));

        contract
            .command_result(2001, true)
            .unwrap_or_else(|_| fail("command_result failed"));
        assert_eq!(contract.active_state(), Some(JobState::AwaitingPlacement));

        current_ticks += 25;
        let arrival_obs_1 = BaseStationObservation {
            common: PlacementObservation {
                configuration_id: 201,
                producer: OBSERVER,
                sequence: 2,
                captured_at_ticks: current_ticks,
                carrier_id: 55,
                slot: 2,
                released: true,
            },
            inventory: InventoryIdentity {
                item_count: 4,
                digest: 0xBEEF_0001,
            },
            stationary: true,
            tray_secure: true,
            local_handoff_eligible: true,
            arm_parked: true,
        };

        contract
            .observe_arrival(2001, arrival_obs_1, current_ticks)
            .unwrap_or_else(|_| fail("observe_arrival failed"));
        assert_eq!(contract.active_state(), Some(JobState::PlacementObserved));

        // Real VFS Trace Write and Readback
        let trace_record_1 =
            alloc::format!("TRACE:job=2001,tray=55,src=1,dst=2,items=4,digest=0xE00D_0001\n");
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
            .acknowledge_trace(2001, 0xE00D_0001, 0xE00D_0001)
            .unwrap_or_else(|_| fail("acknowledge_trace failed"));
        assert_eq!(contract.active_state(), None);
        assert_eq!(contract.retained_count(), 1);
        ostd::io::println(
            "[base-tray-handoff] nominal tray delivery completed and trace verified on VFS",
        );

        // ── Scenario 2: Safety & Arm-Active Exclusions ───────────────────────────
        ostd::io::println("[base-tray-handoff] Scenario 2: Safety & arm-active exclusions");
        let req_arm_active = BaseRequest {
            run_id: 1,
            job_id: 2002,
            tray_id: 56,
            inventory: InventoryIdentity {
                item_count: 2,
                digest: 0xBEEF_0002,
            },
            source_station: 1,
            destination_station: 2,
            configuration_id: 201,
            expected_trace_digest: 0xE00D_0002,
        };

        current_ticks += 10;
        let mut arm_active_obs = adm_obs_1;
        arm_active_obs.common.sequence = 3;
        arm_active_obs.common.captured_at_ticks = current_ticks;
        arm_active_obs.common.source_carrier = Some(56);
        arm_active_obs.inventory = req_arm_active.inventory;
        arm_active_obs.arm_parked = false; // ARM ACTIVE during base transit!
        assert_eq!(
            contract.admit(req_arm_active, arm_active_obs, current_ticks),
            Err(BaseError::ArmActive)
        );

        let mut insecure_obs = adm_obs_1;
        insecure_obs.common.sequence = 4;
        insecure_obs.common.captured_at_ticks = current_ticks;
        insecure_obs.common.source_carrier = Some(56);
        insecure_obs.inventory = req_arm_active.inventory;
        insecure_obs.tray_secure = false; // TRAY INSECURE!
        assert_eq!(
            contract.admit(req_arm_active, insecure_obs, current_ticks),
            Err(BaseError::TrayInsecure)
        );

        let mut moving_obs = adm_obs_1;
        moving_obs.common.sequence = 5;
        moving_obs.common.captured_at_ticks = current_ticks;
        moving_obs.common.source_carrier = Some(56);
        moving_obs.inventory = req_arm_active.inventory;
        moving_obs.stationary = false; // NOT STATIONARY at handoff!
        assert_eq!(
            contract.admit(req_arm_active, moving_obs, current_ticks),
            Err(BaseError::NotStationary)
        );
        ostd::io::println("[base-tray-handoff] arm-active and insecurity exclusions verified");

        // ── Scenario 3: Interrupted Dispatch & Out-of-Band Reconciliation ─────────
        ostd::io::println("[base-tray-handoff] Scenario 3: Interrupted transit and reconciliation");
        current_ticks += 50;
        let mut valid_obs_2 = adm_obs_1;
        valid_obs_2.common.sequence = 6;
        valid_obs_2.common.captured_at_ticks = current_ticks;
        valid_obs_2.common.source_slot = 3;
        valid_obs_2.common.source_carrier = Some(56);
        valid_obs_2.common.destination_slot = 4;
        valid_obs_2.inventory = req_arm_active.inventory;

        let mut req_2 = req_arm_active;
        req_2.source_station = 3;
        req_2.destination_station = 4;

        contract
            .admit(req_2, valid_obs_2, current_ticks)
            .unwrap_or_else(|_| fail("admit job 2002 failed"));
        contract
            .prepare_command(2002)
            .unwrap_or_else(|_| fail("prepare job 2002 failed"));
        contract
            .begin_dispatch(2002)
            .unwrap_or_else(|_| fail("dispatch job 2002 failed"));

        // Interruption mid-transit: timeout triggered
        contract
            .timeout(2002)
            .unwrap_or_else(|_| fail("timeout job 2002 failed"));
        assert_eq!(contract.active_state(), Some(JobState::ReconcileRequired));

        // While ReconcileRequired, new admissions must be blocked
        let req_blocked = BaseRequest {
            run_id: 1,
            job_id: 2003,
            tray_id: 57,
            inventory: InventoryIdentity {
                item_count: 1,
                digest: 0xBEEF_0003,
            },
            source_station: 1,
            destination_station: 2,
            configuration_id: 201,
            expected_trace_digest: 0xE00D_0003,
        };
        let mut obs_blocked = adm_obs_1;
        obs_blocked.common.sequence = 7;
        obs_blocked.common.captured_at_ticks = current_ticks;
        assert!(contract
            .admit(req_blocked, obs_blocked, current_ticks)
            .is_err());

        // Out-of-band operator reconciles: tray 56 confirmed retained at H-A (station 3)
        current_ticks += 20;
        let recon_obs = BaseStationObservation {
            common: PlacementObservation {
                configuration_id: 201,
                producer: OBSERVER,
                sequence: 8,
                captured_at_ticks: current_ticks,
                carrier_id: 56,
                slot: 3, // Retained safely at station 3
                released: true,
            },
            inventory: req_2.inventory,
            stationary: true,
            tray_secure: true,
            local_handoff_eligible: true,
            arm_parked: true,
        };
        contract
            .reconcile(
                2002,
                RECONCILE_AUTHORITY,
                ReconcileDecision::ConfirmedAtSource,
                Some(recon_obs),
                current_ticks,
            )
            .unwrap_or_else(|_| fail("reconcile failed"));

        assert_eq!(contract.active_state(), None);
        assert_eq!(contract.retained_count(), 2);

        let trace_record_2 =
            alloc::format!("RECONCILE:job=2002,decision=ConfirmedAtSource,tray=56,station=3\n");
        if vfs_client
            .append_file(TRACE_LOG_PATH, trace_record_2.as_bytes())
            .is_err()
        {
            fail("failed to append reconcile record to VFS");
        }
        ostd::io::println("[base-tray-handoff] reconciliation required and resolved via authoritative observation");

        // ── Scenario 4: Configuration Epoch Rotation ─────────────────────────────
        ostd::io::println("[base-tray-handoff] Scenario 4: Configuration epoch rotation");
        let rotated_config = Configuration {
            id: 202, // New epoch
            observer: OBSERVER,
            reconcile_authority: RECONCILE_AUTHORITY,
            observation_max_age_ticks: 1000,
        };
        contract
            .reconfigure(rotated_config)
            .unwrap_or_else(|_| fail("reconfigure failed"));

        let stale_req = BaseRequest {
            run_id: 2,
            job_id: 2004,
            tray_id: 58,
            inventory: InventoryIdentity {
                item_count: 3,
                digest: 0xBEEF_0004,
            },
            source_station: 1,
            destination_station: 2,
            configuration_id: 201, // Stale config
            expected_trace_digest: 0xE00D_0004,
        };
        assert!(contract.admit(stale_req, adm_obs_1, current_ticks).is_err());
        ostd::io::println("[base-tray-handoff] configuration change invalidates old observations");

        // ── Scenario 5: VFS Durability Check ─────────────────────────────────────
        ostd::io::println("[base-tray-handoff] Scenario 5: Verifying trace log durability on VFS");
        let full_trace = vfs_client
            .read_file(TRACE_LOG_PATH)
            .unwrap_or_else(|_| fail("failed to read complete trace log from VFS"));

        assert!(full_trace
            .windows(trace_record_1.len())
            .any(|w| w == trace_record_1.as_bytes()));
        assert!(full_trace
            .windows(trace_record_2.len())
            .any(|w| w == trace_record_2.as_bytes()));
        ostd::io::println("[base-tray-handoff] all trace records verified on CellosFS Native");

        ostd::io::println("[base-tray-handoff] Summary: nominal delivery, arm exclusion, reconciliation, and config rotation verified");
        ostd::io::println("[base-tray-handoff] ALL CRITERIA PASSED");
        ostd::syscall::sys_exit(0);
    }

    fn fail(message: &str) -> ! {
        ostd::io::println(&alloc::format!("[base-tray-handoff] FAIL: {message}"));
        ostd::syscall::sys_exit(1)
    }
}
