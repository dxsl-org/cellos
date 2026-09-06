//! Pure, finite LAB-01 transfer contract shared by host and guest fixtures.
//!
//! This models evidence and admission only. It neither drives hardware nor
//! treats command acceptance as placement.

pub const MAX_RETAINED_JOBS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Principal {
    pub id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Configuration {
    pub id: u64,
    pub observer: Principal,
    pub reconcile_authority: Principal,
    pub observation_max_age_ticks: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferRequest {
    pub run_id: u64,
    pub job_id: u64,
    pub carrier_id: u64,
    pub payload_identity_digest: u64,
    pub payload_item_count: u16,
    pub source_slot: u32,
    pub destination_slot: u32,
    pub configuration_id: u64,
    pub expected_trace_digest: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionObservation {
    pub configuration_id: u64,
    pub producer: Principal,
    pub sequence: u64,
    pub captured_at_ticks: u64,
    pub source_slot: u32,
    pub source_carrier: Option<u64>,
    pub destination_slot: u32,
    pub destination_carrier: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementObservation {
    pub configuration_id: u64,
    pub producer: Principal,
    pub sequence: u64,
    pub captured_at_ticks: u64,
    pub carrier_id: u64,
    pub slot: u32,
    pub released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Admitted,
    CommandPending,
    Dispatching,
    AwaitingPlacement,
    PlacementObserved,
    Completed,
    FailedSafe,
    ReconcileRequired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    Accepted,
    Duplicate(JobState),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    InvalidIdentity,
    WrongConfiguration,
    ConfigurationReused,
    ConfigurationCapacityFull,
    WrongSource,
    DestinationOccupied,
    ActiveJob,
    ConflictingDuplicate,
    RetentionFull,
    InvalidTransition,
    StaleObservation,
    ReorderedObservation,
    WrongObserver,
    WrongPlacement,
    MissingObservation,
    TraceMismatch,
    UnauthorizedReconcile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileDecision {
    ConfirmedAtSource,
    ConfirmedAtDestination,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JobRecord {
    request: TransferRequest,
    configuration: Configuration,
    state: JobState,
}

pub struct TransferContract {
    configuration: Configuration,
    active: Option<JobRecord>,
    retained: [Option<JobRecord>; MAX_RETAINED_JOBS],
    used_configuration_ids: [Option<u64>; MAX_RETAINED_JOBS],
    last_observation_sequence: u64,
}

impl TransferContract {
    pub fn new(configuration: Configuration) -> Result<Self, ContractError> {
        if !valid_configuration(configuration) {
            return Err(ContractError::InvalidIdentity);
        }
        let mut used_configuration_ids = [None; MAX_RETAINED_JOBS];
        used_configuration_ids[0] = Some(configuration.id);
        Ok(Self {
            configuration,
            active: None,
            retained: [None; MAX_RETAINED_JOBS],
            used_configuration_ids,
            last_observation_sequence: 0,
        })
    }

    pub fn configuration(&self) -> Configuration {
        self.configuration
    }

    pub fn active_state(&self) -> Option<JobState> {
        self.active.map(|record| record.state)
    }

    pub fn retained_count(&self) -> usize {
        self.retained.iter().flatten().count()
    }
    pub fn existing_admission(
        &self,
        request: TransferRequest,
    ) -> Option<Result<Admission, ContractError>> {
        self.find(request.job_id).map(|record| {
            if record.request == request {
                Ok(Admission::Duplicate(record.state))
            } else {
                Err(ContractError::ConflictingDuplicate)
            }
        })
    }

    pub fn admit(
        &mut self,
        request: TransferRequest,
        observation: AdmissionObservation,
        now_ticks: u64,
    ) -> Result<Admission, ContractError> {
        validate_request(request)?;
        if request.configuration_id != self.configuration.id {
            return Err(ContractError::WrongConfiguration);
        }
        if let Some(admission) = self.existing_admission(request) {
            return admission;
        }
        if self.active.is_some() {
            return Err(ContractError::ActiveJob);
        }
        if self.retained.iter().all(Option::is_some) {
            return Err(ContractError::RetentionFull);
        }
        validate_provenance(
            self.configuration,
            observation.configuration_id,
            observation.producer,
            observation.sequence,
            observation.captured_at_ticks,
            now_ticks,
        )?;
        if observation.sequence <= self.last_observation_sequence {
            return Err(ContractError::ReorderedObservation);
        }
        self.last_observation_sequence = observation.sequence;
        if observation.source_slot != request.source_slot
            || observation.source_carrier != Some(request.carrier_id)
        {
            return Err(ContractError::WrongSource);
        }
        if observation.destination_slot != request.destination_slot
            || observation.destination_carrier.is_some()
        {
            return Err(ContractError::DestinationOccupied);
        }
        self.active = Some(JobRecord {
            request,
            configuration: self.configuration,
            state: JobState::Admitted,
        });
        Ok(Admission::Accepted)
    }

    pub fn prepare_command(&mut self, job_id: u64) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        transition(record, JobState::Admitted, JobState::CommandPending)
    }

    /// Call immediately before handing the command to the external channel.
    /// From this point until an authoritative observation, effects are uncertain.
    pub fn begin_dispatch(&mut self, job_id: u64) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        transition(record, JobState::CommandPending, JobState::Dispatching)
    }

    /// Record the independent command channel outcome.
    ///
    /// `accepted=true` means a physical effect is now possible; it is not task
    /// completion. `accepted=false` proves no command was dispatched.
    pub fn command_result(&mut self, job_id: u64, accepted: bool) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        if record.state != JobState::Dispatching {
            return Err(ContractError::InvalidTransition);
        }
        if accepted {
            record.state = JobState::AwaitingPlacement;
            Ok(())
        } else {
            record.state = JobState::FailedSafe;
            self.retain_terminal()
        }
    }

    pub fn observe_placement(
        &mut self,
        job_id: u64,
        observation: PlacementObservation,
        now_ticks: u64,
    ) -> Result<(), ContractError> {
        let record = self
            .active
            .filter(|record| record.request.job_id == job_id)
            .ok_or(ContractError::InvalidTransition)?;
        if record.state != JobState::AwaitingPlacement {
            return Err(ContractError::InvalidTransition);
        }
        if let Err(error) = validate_observation(
            record.configuration,
            self.last_observation_sequence,
            observation,
            now_ticks,
        ) {
            self.active_for(job_id)?.state = JobState::ReconcileRequired;
            return Err(error);
        }
        self.last_observation_sequence = observation.sequence;
        let wrong_placement = observation.carrier_id != record.request.carrier_id
            || observation.slot != record.request.destination_slot
            || !observation.released;
        let record = self.active_for(job_id)?;
        if wrong_placement {
            record.state = JobState::ReconcileRequired;
            return Err(ContractError::WrongPlacement);
        }
        record.state = JobState::PlacementObserved;
        Ok(())
    }

    /// Complete only after the trace write was acknowledged and exact bytes
    /// were independently read back.
    pub fn acknowledge_trace(
        &mut self,
        job_id: u64,
        acknowledged_digest: u64,
        readback_digest: u64,
    ) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        if record.state != JobState::PlacementObserved {
            return Err(ContractError::InvalidTransition);
        }
        if acknowledged_digest != record.request.expected_trace_digest
            || readback_digest != record.request.expected_trace_digest
        {
            return Err(ContractError::TraceMismatch);
        }
        record.state = JobState::Completed;
        self.retain_terminal()
    }

    /// A missing/uncertain trace acknowledgement never reverses physical truth.
    pub fn trace_failed(&mut self, job_id: u64) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        if record.state != JobState::PlacementObserved {
            return Err(ContractError::InvalidTransition);
        }
        Ok(())
    }

    pub fn timeout(&mut self, job_id: u64) -> Result<(), ContractError> {
        let record = self.active_for(job_id)?;
        match record.state {
            JobState::Admitted | JobState::CommandPending => {
                record.state = JobState::FailedSafe;
                self.retain_terminal()
            }
            JobState::Dispatching | JobState::AwaitingPlacement | JobState::PlacementObserved => {
                record.state = JobState::ReconcileRequired;
                Ok(())
            }
            _ => Err(ContractError::InvalidTransition),
        }
    }

    pub fn interrupt(&mut self, job_id: u64) -> Result<(), ContractError> {
        self.timeout(job_id)
    }

    /// Configuration change invalidates in-flight commands and observations.
    /// It never clears an uncertain active job.
    pub fn reconfigure(&mut self, configuration: Configuration) -> Result<(), ContractError> {
        if !valid_configuration(configuration) {
            return Err(ContractError::InvalidIdentity);
        }
        if configuration == self.configuration {
            return Ok(());
        }
        if self
            .used_configuration_ids
            .iter()
            .flatten()
            .any(|used_id| *used_id == configuration.id)
        {
            return Err(ContractError::ConfigurationReused);
        }
        let domain_slot = self
            .used_configuration_ids
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(ContractError::ConfigurationCapacityFull)?;
        *domain_slot = Some(configuration.id);
        if let Some(record) = self.active.as_mut() {
            record.state = JobState::ReconcileRequired;
        }
        self.last_observation_sequence = 0;
        self.configuration = configuration;
        Ok(())
    }

    pub fn reconcile(
        &mut self,
        job_id: u64,
        authority: Principal,
        decision: ReconcileDecision,
        observation: Option<PlacementObservation>,
        now_ticks: u64,
    ) -> Result<(), ContractError> {
        let current_configuration = self.configuration;
        let record = self
            .active
            .filter(|record| record.request.job_id == job_id)
            .ok_or(ContractError::InvalidTransition)?;
        if record.state != JobState::ReconcileRequired {
            return Err(ContractError::InvalidTransition);
        }
        if authority != current_configuration.reconcile_authority {
            return Err(ContractError::UnauthorizedReconcile);
        }
        if decision == ReconcileDecision::Unknown {
            return Ok(());
        }
        let observation = observation.ok_or(ContractError::MissingObservation)?;
        validate_observation(
            current_configuration,
            self.last_observation_sequence,
            observation,
            now_ticks,
        )?;
        self.last_observation_sequence = observation.sequence;
        let expected_slot = match decision {
            ReconcileDecision::ConfirmedAtSource => record.request.source_slot,
            ReconcileDecision::ConfirmedAtDestination => record.request.destination_slot,
            ReconcileDecision::Unknown => unreachable!(),
        };
        if observation.carrier_id != record.request.carrier_id
            || observation.slot != expected_slot
            || !observation.released
        {
            return Err(ContractError::WrongPlacement);
        }
        let record = self.active_for(job_id)?;
        match decision {
            ReconcileDecision::Unknown => unreachable!(),
            ReconcileDecision::ConfirmedAtDestination => {
                record.state = JobState::PlacementObserved;
                Ok(())
            }
            ReconcileDecision::ConfirmedAtSource => {
                record.state = JobState::FailedSafe;
                self.retain_terminal()
            }
        }
    }

    fn active_for(&mut self, job_id: u64) -> Result<&mut JobRecord, ContractError> {
        self.active
            .as_mut()
            .filter(|record| record.request.job_id == job_id)
            .ok_or(ContractError::InvalidTransition)
    }

    fn find(&self, job_id: u64) -> Option<JobRecord> {
        self.active
            .filter(|record| record.request.job_id == job_id)
            .or_else(|| {
                self.retained
                    .iter()
                    .flatten()
                    .copied()
                    .find(|record| record.request.job_id == job_id)
            })
    }

    fn retain_terminal(&mut self) -> Result<(), ContractError> {
        let record = self.active.take().ok_or(ContractError::InvalidTransition)?;
        let slot = self
            .retained
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(ContractError::RetentionFull)?;
        *slot = Some(record);
        Ok(())
    }
}

fn valid_configuration(configuration: Configuration) -> bool {
    configuration.id != 0
        && configuration.observer.id != 0
        && configuration.observer.generation != 0
        && configuration.reconcile_authority.id != 0
        && configuration.reconcile_authority.generation != 0
        && configuration.observation_max_age_ticks != 0
}

fn validate_request(request: TransferRequest) -> Result<(), ContractError> {
    if request.run_id == 0
        || request.job_id == 0
        || request.carrier_id == 0
        || request.source_slot == 0
        || request.destination_slot == 0
        || request.payload_identity_digest == 0
        || request.payload_item_count == 0
        || request.source_slot == request.destination_slot
        || request.configuration_id == 0
        || request.expected_trace_digest == 0
    {
        return Err(ContractError::InvalidIdentity);
    }
    Ok(())
}

fn validate_observation(
    configuration: Configuration,
    last_observation_sequence: u64,
    observation: PlacementObservation,
    now_ticks: u64,
) -> Result<(), ContractError> {
    validate_provenance(
        configuration,
        observation.configuration_id,
        observation.producer,
        observation.sequence,
        observation.captured_at_ticks,
        now_ticks,
    )?;
    if observation.sequence <= last_observation_sequence {
        return Err(ContractError::ReorderedObservation);
    }
    Ok(())
}

pub(crate) fn validate_provenance(
    configuration: Configuration,
    configuration_id: u64,
    producer: Principal,
    sequence: u64,
    captured_at_ticks: u64,
    now_ticks: u64,
) -> Result<(), ContractError> {
    if configuration_id != configuration.id {
        return Err(ContractError::WrongConfiguration);
    }
    if producer != configuration.observer {
        return Err(ContractError::WrongObserver);
    }
    if sequence == 0 {
        return Err(ContractError::ReorderedObservation);
    }
    if captured_at_ticks > now_ticks
        || now_ticks - captured_at_ticks > configuration.observation_max_age_ticks
    {
        return Err(ContractError::StaleObservation);
    }
    Ok(())
}

fn transition(record: &mut JobRecord, from: JobState, to: JobState) -> Result<(), ContractError> {
    if record.state != from {
        return Err(ContractError::InvalidTransition);
    }
    record.state = to;
    Ok(())
}
