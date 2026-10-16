#![forbid(unsafe_code)]

use crate::JournalError;

pub const MAX_OBLIGATIONS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObligationStatus {
    Unknown,
    Applied,
    Closing,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateStatus {
    Inactive,
    Reconciling,
    Open,
    Fenced,
    Redirect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeOp {
    pub key: [u8; 16],
    pub before_vector: [u64; 2],
    pub target_vector: [u64; 2],
    pub global_debit: [u64; 2],
    pub local_growth: [u64; 2],
    pub credited: [u64; 2],
    pub applied: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obligation {
    pub id: [u8; 16],
    pub origin_ledger: [u8; 32],
    pub origin_genesis: [u8; 32],
    pub initiating_tx: [u8; 16],
    pub initiating_rev: u64,
    pub create_key: [u8; 16],
    pub base_vector: [u64; 2],
    pub pending_positive: [u64; 2],
    pub effective_vector: [u64; 2],
    pub status: ObligationStatus,
    pub resize: Option<ResizeOp>,
}

pub struct AuthorityGate {
    pub ledger_id: [u8; 32],
    pub genesis_id: [u8; 32],
    pub epoch: u64,
    pub writer: [u8; 32],
    pub gate_status: GateStatus,
    pub charged: [u64; 2],
    pub limit: [u64; 2],
    pub obligations: [Option<Obligation>; MAX_OBLIGATIONS],
    pub obligation_count: usize,
}

impl AuthorityGate {
    pub fn new(ledger_id: [u8; 32], genesis_id: [u8; 32], epoch: u64, writer: [u8; 32], limit: [u64; 2]) -> Self {
        Self {
            ledger_id,
            genesis_id,
            epoch,
            writer,
            gate_status: GateStatus::Open,
            charged: [0, 0],
            limit,
            obligations: [None; MAX_OBLIGATIONS],
            obligation_count: 0,
        }
    }

    pub fn find_obligation(&self, id: &[u8; 16]) -> Option<&Obligation> {
        self.obligations.iter().filter_map(|o| o.as_ref()).find(|o| &o.id == id)
    }

    pub fn find_obligation_index(&self, id: &[u8; 16]) -> Option<usize> {
        self.obligations.iter().enumerate().find_map(|(i, o)| {
            match o {
                Some(ob) if &ob.id == id => Some(i),
                _ => None,
            }
        })
    }

    pub fn check_gate(&self, writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        if self.gate_status != GateStatus::Open {
            return Err(JournalError::GateClosed);
        }
        if &self.writer != writer || self.epoch != epoch {
            return Err(JournalError::StaleWriter);
        }
        Ok(())
    }

    pub fn reserve(
        &mut self,
        id: [u8; 16],
        origin_ledger: [u8; 32],
        origin_genesis: [u8; 32],
        initiating_tx: [u8; 16],
        initiating_rev: u64,
        create_key: [u8; 16],
        vector: [u64; 2],
        writer: &[u8; 32],
        epoch: u64,
    ) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        if self.find_obligation(&id).is_some() {
            return Err(JournalError::Duplicate);
        }
        if self.obligation_count >= MAX_OBLIGATIONS {
            return Err(JournalError::Capacity);
        }

        let new_charged_0 = self.charged[0].checked_add(vector[0]).ok_or(JournalError::Overflow)?;
        let new_charged_1 = self.charged[1].checked_add(vector[1]).ok_or(JournalError::Overflow)?;
        if new_charged_0 > self.limit[0] || new_charged_1 > self.limit[1] {
            return Err(JournalError::Capacity);
        }

        self.charged[0] = new_charged_0;
        self.charged[1] = new_charged_1;

        let slot = self.obligations.iter_mut().find(|o| o.is_none()).ok_or(JournalError::Capacity)?;
        *slot = Some(Obligation {
            id,
            origin_ledger,
            origin_genesis,
            initiating_tx,
            initiating_rev,
            create_key,
            base_vector: vector,
            pending_positive: [0, 0],
            effective_vector: [0, 0],
            status: ObligationStatus::Unknown,
            resize: None,
        });
        self.obligation_count += 1;
        Ok(())
    }

    pub fn resolve_creation(&mut self, id: &[u8; 16], vector: [u64; 2], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_mut().unwrap();
        if ob.status != ObligationStatus::Unknown || ob.base_vector != vector {
            return Err(JournalError::State);
        }
        ob.effective_vector = vector;
        ob.status = ObligationStatus::Applied;
        Ok(())
    }

    pub fn resize(&mut self, id: &[u8; 16], key: [u8; 16], target: [u64; 2], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_ref().unwrap();
        if ob.status != ObligationStatus::Applied || ob.resize.is_some() {
            return Err(JournalError::State);
        }

        // Two-plane calculation:
        // G = max(Q - B, 0) (global prospective debit)
        // L = max(Q - E, 0) (local growth capacity)
        let g = [
            target[0].saturating_sub(ob.base_vector[0]),
            target[1].saturating_sub(ob.base_vector[1]),
        ];
        let l = [
            target[0].saturating_sub(ob.effective_vector[0]),
            target[1].saturating_sub(ob.effective_vector[1]),
        ];
        let before_vector = ob.effective_vector;

        let new_charged_0 = self.charged[0].checked_add(g[0]).ok_or(JournalError::Overflow)?;
        let new_charged_1 = self.charged[1].checked_add(g[1]).ok_or(JournalError::Overflow)?;
        if new_charged_0 > self.limit[0] || new_charged_1 > self.limit[1] {
            return Err(JournalError::Capacity);
        }

        self.charged[0] = new_charged_0;
        self.charged[1] = new_charged_1;

        let ob_mut = self.obligations[idx].as_mut().unwrap();
        ob_mut.pending_positive = g;
        ob_mut.resize = Some(ResizeOp {
            key,
            before_vector,
            target_vector: target,
            global_debit: g,
            local_growth: l,
            credited: [0, 0],
            applied: false,
        });

        Ok(())
    }

    pub fn resize_apply(&mut self, id: &[u8; 16], key: &[u8; 16], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_mut().unwrap();
        let resize = ob.resize.as_mut().ok_or(JournalError::State)?;
        if &resize.key != key || resize.applied {
            return Err(JournalError::State);
        }

        // Atomic fold: B' = B + P, P' = 0, E' = Q
        ob.base_vector[0] = ob.base_vector[0].checked_add(ob.pending_positive[0]).ok_or(JournalError::Overflow)?;
        ob.base_vector[1] = ob.base_vector[1].checked_add(ob.pending_positive[1]).ok_or(JournalError::Overflow)?;
        ob.pending_positive = [0, 0];
        ob.effective_vector = resize.target_vector;
        resize.applied = true;

        if ob.base_vector == resize.target_vector {
            ob.resize = None;
        }

        Ok(())
    }

    pub fn shrink_credit(&mut self, id: &[u8; 16], key: &[u8; 16], installment: [u64; 2], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_ref().unwrap();
        let resize = ob.resize.as_ref().ok_or(JournalError::State)?;
        if &resize.key != key || !resize.applied {
            return Err(JournalError::State);
        }

        // Validate installment: S <= B - Q
        let max_credit_0 = ob.base_vector[0].saturating_sub(resize.target_vector[0]);
        let max_credit_1 = ob.base_vector[1].saturating_sub(resize.target_vector[1]);
        if installment[0] > max_credit_0 || installment[1] > max_credit_1 || installment == [0, 0] {
            return Err(JournalError::Arithmetic);
        }

        self.charged[0] -= installment[0];
        self.charged[1] -= installment[1];

        let ob_mut = self.obligations[idx].as_mut().unwrap();
        ob_mut.base_vector[0] -= installment[0];
        ob_mut.base_vector[1] -= installment[1];
        let r_mut = ob_mut.resize.as_mut().unwrap();
        r_mut.credited[0] += installment[0];
        r_mut.credited[1] += installment[1];

        if ob_mut.base_vector == r_mut.target_vector {
            ob_mut.resize = None;
        }

        Ok(())
    }

    pub fn cancel_resize(&mut self, id: &[u8; 16], key: &[u8; 16], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_mut().unwrap();
        let resize = ob.resize.take().ok_or(JournalError::State)?;
        if &resize.key != key || resize.applied {
            ob.resize = Some(resize);
            return Err(JournalError::State);
        }

        // Return global positive debit
        self.charged[0] -= resize.global_debit[0];
        self.charged[1] -= resize.global_debit[1];
        ob.pending_positive = [0, 0];
        Ok(())
    }

    pub fn close_intent(&mut self, id: &[u8; 16], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_mut().unwrap();
        if ob.status != ObligationStatus::Applied || ob.resize.is_some() {
            return Err(JournalError::State);
        }
        ob.status = ObligationStatus::Closing;
        Ok(())
    }

    pub fn release_once(&mut self, id: &[u8; 16], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let idx = self.find_obligation_index(id).ok_or(JournalError::NotFound)?;
        let ob = self.obligations[idx].as_ref().unwrap();
        if ob.status != ObligationStatus::Closing || ob.resize.is_some() {
            return Err(JournalError::State);
        }

        let base = ob.base_vector;
        let new_charged_0 = self.charged[0].checked_sub(base[0]).ok_or(JournalError::Arithmetic)?;
        let new_charged_1 = self.charged[1].checked_sub(base[1]).ok_or(JournalError::Arithmetic)?;
        self.charged[0] = new_charged_0;
        self.charged[1] = new_charged_1;

        let ob_mut = self.obligations[idx].as_mut().unwrap();
        ob_mut.base_vector = [0, 0];
        ob_mut.effective_vector = [0, 0];
        ob_mut.status = ObligationStatus::Released;
        Ok(())
    }

    pub fn prepare_adoption(&mut self, target_authority: &[u8; 32], writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        self.check_gate(writer, epoch)?;
        let _ = target_authority;
        self.gate_status = GateStatus::Fenced;
        Ok(())
    }

    pub fn accept_adoption(&mut self, new_writer: [u8; 32], new_epoch: u64) -> Result<(), JournalError> {
        self.writer = new_writer;
        self.epoch = new_epoch;
        self.gate_status = GateStatus::Reconciling;
        Ok(())
    }

    pub fn activate(&mut self, writer: &[u8; 32], epoch: u64) -> Result<(), JournalError> {
        if self.gate_status != GateStatus::Reconciling {
            return Err(JournalError::GateClosed);
        }
        if &self.writer != writer || self.epoch != epoch {
            return Err(JournalError::StaleWriter);
        }
        // Reconciliation check: ensure no unresolved Unknown obligations remain
        for ob in self.obligations.iter().filter_map(|o| o.as_ref()) {
            if ob.status == ObligationStatus::Unknown {
                return Err(JournalError::UnresolvedObligation);
            }
        }
        self.gate_status = GateStatus::Open;
        Ok(())
    }
}
