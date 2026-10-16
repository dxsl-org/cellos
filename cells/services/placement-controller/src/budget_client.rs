//! Client interface for requesting capacity reservations from Budget Authority.

pub type ResourceVector = [u64; 6];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetClientError {
    CapacityExceeded,
    AuthorityUnreachable,
    DuplicateKey,
}

pub struct BudgetAuthorityClient {
    pub authority_epoch: u64,
}

impl BudgetAuthorityClient {
    pub const fn new() -> Self {
        Self { authority_epoch: 1 }
    }

    /// Request a reservation of `vector` for create key `key` from the Budget Authority.
    pub fn request_reservation(
        &self,
        _key: &[u8; 16],
        _vector: &ResourceVector,
    ) -> Result<u64, BudgetClientError> {
        // Returns the authority epoch on success
        Ok(self.authority_epoch)
    }
}
