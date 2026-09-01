use core::cell::RefCell;

use authority_protocol::{AdmittedProfileValidation, RootProfileVerifier};
use stm32_authority_journal::PendingEnrollmentSnapshot;

use crate::{validate_profile, PendingPublicReader, TrustedPolicy};

/// Internal bridge from the full validator into the authority state capability gate.
pub(crate) struct RootProfilePolicy<'a, R> {
    profile: &'a [u8],
    policy: TrustedPolicy<'a>,
    pending: &'a PendingEnrollmentSnapshot,
    public_reader: RefCell<&'a mut R>,
}

impl<'a, R: PendingPublicReader> RootProfilePolicy<'a, R> {
    /// Binds one admitted profile, trusted policy, bank-gated snapshot, and TPM reader.
    pub(crate) const fn new(
        profile: &'a [u8],
        policy: TrustedPolicy<'a>,
        pending: &'a PendingEnrollmentSnapshot,
        public_reader: &'a mut R,
    ) -> Self {
        Self {
            profile,
            policy,
            pending,
            public_reader: RefCell::new(public_reader),
        }
    }
}

impl<R: PendingPublicReader> RootProfileVerifier for RootProfilePolicy<'_, R> {
    fn verify_root_profile(&self, admitted: &AdmittedProfileValidation) -> bool {
        let Ok(mut reader) = self.public_reader.try_borrow_mut() else {
            return false;
        };
        validate_profile(
            admitted,
            self.profile,
            self.policy,
            self.pending,
            &mut **reader,
        )
        .is_ok()
    }
}
