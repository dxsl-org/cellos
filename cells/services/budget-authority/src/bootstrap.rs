//! Genesis precommit adoption and one-use bootstrap reservation.

use super::ledger::{LedgerError, ResourceVector};

pub struct BootstrapGrant {
    pub genesis_key: [u8; 16],
    pub vector: ResourceVector,
    pub consumed: bool,
}

impl BootstrapGrant {
    pub fn new(genesis_key: [u8; 16], vector: ResourceVector) -> Self {
        Self {
            genesis_key,
            vector,
            consumed: false,
        }
    }

    /// Consume the one-use bootstrap grant during supervisor initialization.
    pub fn consume(&mut self) -> Result<ResourceVector, LedgerError> {
        if self.consumed {
            return Err(LedgerError::UnauthorizedMutation);
        }
        self.consumed = true;
        Ok(self.vector)
    }
}
