//! Application ledger data structures and multi-dimensional capacity vectors.

pub const VECTOR_DIMENSIONS: usize = 6;
pub type ResourceVector = [u64; VECTOR_DIMENSIONS];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    CapacityExceeded,
    StaleRevision,
    MonotonicityViolation,
    UnauthorizedMutation,
    InvalidDimension,
}

/// An immutable logical application ledger managed by the Budget Authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplicationLedger {
    pub ledger_id: [u8; 32],
    pub genesis_id: [u8; 32],
    pub revision: u64,
    /// Absolute immutable ceiling vector (M)
    pub max_vector: ResourceVector,
    /// Currently admitted budget ceiling (B <= M)
    pub ceiling_vector: ResourceVector,
    /// Currently allocated / reserved charge vector across all dimensions (A <= B)
    pub allocated_vector: ResourceVector,
    /// Non-regressing enforcement fence
    pub enforcement_fence: u64,
}

impl ApplicationLedger {
    pub fn new(
        ledger_id: [u8; 32],
        genesis_id: [u8; 32],
        max_vector: ResourceVector,
        ceiling_vector: ResourceVector,
        fence: u64,
    ) -> Result<Self, LedgerError> {
        for d in 0..VECTOR_DIMENSIONS {
            if ceiling_vector[d] > max_vector[d] {
                return Err(LedgerError::CapacityExceeded);
            }
        }
        Ok(Self {
            ledger_id,
            genesis_id,
            revision: 1,
            max_vector,
            ceiling_vector,
            allocated_vector: [0; VECTOR_DIMENSIONS],
            enforcement_fence: fence,
        })
    }

    /// Check whether allocating `delta` fits within the current ceiling.
    pub fn can_allocate(&self, delta: &ResourceVector) -> bool {
        self.allocated_vector
            .iter()
            .zip(delta)
            .zip(&self.ceiling_vector)
            .all(|((&allocated, &d), &ceiling)| {
                matches!(allocated.checked_add(d), Some(sum) if sum <= ceiling)
            })
    }

    /// Atomically debit the ledger for a new reservation.
    pub fn allocate(&mut self, delta: &ResourceVector) -> Result<(), LedgerError> {
        if !self.can_allocate(delta) {
            return Err(LedgerError::CapacityExceeded);
        }
        for (allocated, &d) in self.allocated_vector.iter_mut().zip(delta) {
            *allocated = allocated.checked_add(d).unwrap();
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Release an allocated delta back to the ledger (credit once).
    pub fn release(&mut self, delta: &ResourceVector) -> Result<(), LedgerError> {
        for (allocated, &d) in self.allocated_vector.iter_mut().zip(delta) {
            if d > *allocated {
                return Err(LedgerError::MonotonicityViolation);
            }
            *allocated -= d;
        }
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}
