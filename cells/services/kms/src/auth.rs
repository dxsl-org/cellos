use api::caller_identity::CallerIdentity;
use types::kms::{
    BindingEpoch, BrokerBindingPayload, KmsErrorCode, ServiceNetBindingEpoch,
    ServiceNetBindingPayload,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceRegistrySnapshot {
    pub net_broker_tid: Option<usize>,
    pub supervisor_tid: Option<usize>,
    pub net_tid: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerBinding {
    pub epoch: BindingEpoch,
    pub cell_id: u64,
    pub generation: u64,
    pub service_tid: usize,
}

impl BrokerBinding {
    pub fn payload(self) -> BrokerBindingPayload {
        BrokerBindingPayload {
            binding_epoch: self.epoch,
            bound_cell_id: self.cell_id,
            bound_generation: self.generation,
            bound_service_tid: self.service_tid as u64,
        }
    }

    pub fn authorizes(
        self,
        sender: usize,
        caller: Option<CallerIdentity>,
        live_broker_tid: Option<usize>,
    ) -> Result<(), KmsErrorCode> {
        let caller = validated_caller(sender, caller)?;
        if live_broker_tid != Some(self.service_tid) {
            return Err(KmsErrorCode::BindingStale);
        }
        if caller.cell_id != self.cell_id || caller.generation != self.generation {
            return Err(KmsErrorCode::PermissionDenied);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceNetBinding {
    pub epoch: ServiceNetBindingEpoch,
    pub cell_id: u64,
    pub generation: u64,
    pub service_tid: usize,
}

impl ServiceNetBinding {
    pub fn payload(self) -> ServiceNetBindingPayload {
        ServiceNetBindingPayload {
            binding_epoch: self.epoch,
            bound_cell_id: self.cell_id,
            bound_generation: self.generation,
            bound_service_tid: self.service_tid as u64,
        }
    }

    pub fn authorizes(
        self,
        sender: usize,
        caller: Option<CallerIdentity>,
        live_net_tid: Option<usize>,
    ) -> Result<(), KmsErrorCode> {
        let caller = validated_caller(sender, caller)?;
        if live_net_tid != Some(self.service_tid) {
            return Err(KmsErrorCode::ServiceBindingStale);
        }
        if caller.cell_id != self.cell_id || caller.generation != self.generation {
            return Err(KmsErrorCode::PermissionDenied);
        }
        Ok(())
    }
}

pub fn register_broker(
    epoch: BindingEpoch,
    sender: usize,
    caller: Option<CallerIdentity>,
    live_broker_tid: Option<usize>,
) -> Result<BrokerBinding, KmsErrorCode> {
    let caller = validated_caller(sender, caller)?;
    if live_broker_tid != Some(sender) {
        return Err(KmsErrorCode::PermissionDenied);
    }
    Ok(BrokerBinding {
        epoch,
        cell_id: caller.cell_id,
        generation: caller.generation,
        service_tid: sender,
    })
}

pub fn register_service_net(
    epoch: ServiceNetBindingEpoch,
    sender: usize,
    caller: Option<CallerIdentity>,
    live_net_tid: Option<usize>,
) -> Result<ServiceNetBinding, KmsErrorCode> {
    let caller = validated_caller(sender, caller)?;
    if live_net_tid != Some(sender) {
        return Err(KmsErrorCode::PermissionDenied);
    }
    Ok(ServiceNetBinding {
        epoch,
        cell_id: caller.cell_id,
        generation: caller.generation,
        service_tid: sender,
    })
}

pub fn authorize_supervisor(
    sender: usize,
    caller: Option<CallerIdentity>,
    live_supervisor_tid: Option<usize>,
) -> Result<(), KmsErrorCode> {
    validated_caller(sender, caller)?;
    if live_supervisor_tid != Some(sender) {
        return Err(KmsErrorCode::PermissionDenied);
    }
    Ok(())
}

fn validated_caller(
    sender: usize,
    caller: Option<CallerIdentity>,
) -> Result<CallerIdentity, KmsErrorCode> {
    let caller = caller.ok_or(KmsErrorCode::CallerUnattested)?;
    if caller.generation == 0 || caller.sender_tid != sender as u64 {
        return Err(KmsErrorCode::CallerUnattested);
    }
    Ok(caller)
}
