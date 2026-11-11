// SPDX-License-Identifier: MPL-2.0
//! What one request/reply exchange did, as far as the caller can know (Phase 11 CP1 §3).
//!
//! An exchange ends in exactly one of three ways:
//! - a reply arrived;
//! - the request was refused, and the provider never executed it;
//! - the result is uncertain, and the request may have run.
//!
//! Callers that only need an error code collapse the outcome with
//! [`CallOutcome::into_result`]. Callers that retry, reconcile or report must match on it.

use crate::ipc::IpcError;
use crate::{ViError, ViResult};

/// The end of one request/reply exchange.
#[must_use = "an exchange outcome says whether the request may have run"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome<T> {
    /// The provider answered; `T` is its decoded reply.
    Reply(T),
    /// The provider never executed the request. Retrying is safe for the
    /// caller to decide on, because there are no side effects to reconcile.
    Refused(Refusal),
    /// The request may have run. Reconcile application state before any retry.
    Uncertain(Uncertainty),
}

/// Why a request was refused before the provider could execute it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// No live provider is bound for the service. This includes the
    /// death→respawn window and a provider paused behind a hot-swap.
    NoBinding,
    /// Admission was busy, so nothing was accepted.
    Busy,
    /// The request did not encode into the request buffer.
    TooLarge,
    /// The request expired or was cancelled before the provider took it.
    NotDispatched,
    /// The kernel rejected the submission.
    Rejected,
}

/// Why the result of an accepted request is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Uncertainty {
    /// The provider incarnation the call named is no longer the live one.
    ProviderReplaced,
    /// The provider failed or exited without replying. It is still the live
    /// provider, or the call held no binding to compare against.
    ProviderFailed,
    /// The request was dispatched, but no reply settled it before expiry.
    Expired,
    /// A terminal arrived but its reply could not be taken or decoded.
    Malformed,
    /// Waiting for the terminal failed.
    WaitFailed,
}

/// A failed exchange, before it is given the reply type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Refused(Refusal),
    Uncertain(Uncertainty),
}

impl<T> CallOutcome<T> {
    /// Collapse the outcome into the SDK's error codes.
    ///
    /// A reply is `Ok`. A refusal for `NoBinding` and an uncertain
    /// `ProviderReplaced` are `NotFound`, and `TooLarge` is `InvalidArgument`.
    /// Every other refusal and uncertainty is `IO`.
    ///
    /// # Errors
    /// Returns the code for every outcome that is not a reply.
    pub fn into_result(self) -> ViResult<T> {
        match self {
            Self::Reply(reply) => Ok(reply),
            Self::Refused(Refusal::NoBinding) => Err(ViError::NotFound),
            Self::Refused(Refusal::TooLarge) => Err(ViError::InvalidArgument),
            Self::Refused(_) => Err(ViError::IO),
            Self::Uncertain(Uncertainty::ProviderReplaced) => Err(ViError::NotFound),
            Self::Uncertain(_) => Err(ViError::IO),
        }
    }
}

impl<T> From<Failure> for CallOutcome<T> {
    fn from(failure: Failure) -> Self {
        match failure {
            Failure::Refused(reason) => Self::Refused(reason),
            Failure::Uncertain(reason) => Self::Uncertain(reason),
        }
    }
}

/// Whether classifying `error` needs to know if the binding the call used is stale.
///
/// Only failures that can come from the provider's lifecycle do. Encoding,
/// admission and reply-decoding failures are the same whichever provider is live,
/// so they cost no registry lookup.
pub const fn needs_binding_check(error: IpcError) -> bool {
    !matches!(
        error,
        IpcError::Encode
            | IpcError::Busy
            | IpcError::Decode
            | IpcError::InvalidOperation
            | IpcError::BufferTooSmall
    )
}

/// Classify a failed exchange.
///
/// `stale` is true when the provider binding the call used is no longer the live
/// one. It is ignored when [`needs_binding_check`] is false. A stale binding turns
/// a refusal into `NoBinding`, and an uncertain result into `ProviderReplaced`.
pub const fn classify(error: IpcError, stale: bool) -> Failure {
    match error {
        IpcError::Encode => Failure::Refused(Refusal::TooLarge),
        IpcError::Busy => Failure::Refused(Refusal::Busy),
        IpcError::Decode | IpcError::InvalidOperation | IpcError::BufferTooSmall => {
            Failure::Uncertain(Uncertainty::Malformed)
        }
        IpcError::PreDispatchTimeout | IpcError::Cancelled | IpcError::Send if stale => {
            Failure::Refused(Refusal::NoBinding)
        }
        IpcError::PreDispatchTimeout | IpcError::Cancelled => {
            Failure::Refused(Refusal::NotDispatched)
        }
        IpcError::Send => Failure::Refused(Refusal::Rejected),
        IpcError::PeerGone | IpcError::Indeterminate | IpcError::Recv | IpcError::WrongSender
            if stale =>
        {
            Failure::Uncertain(Uncertainty::ProviderReplaced)
        }
        IpcError::PeerGone => Failure::Uncertain(Uncertainty::ProviderFailed),
        IpcError::Indeterminate => Failure::Uncertain(Uncertainty::Expired),
        IpcError::Recv | IpcError::WrongSender => Failure::Uncertain(Uncertainty::WaitFailed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [IpcError; 12] = [
        IpcError::Encode,
        IpcError::Send,
        IpcError::Recv,
        IpcError::WrongSender,
        IpcError::Decode,
        IpcError::InvalidOperation,
        IpcError::BufferTooSmall,
        IpcError::Busy,
        IpcError::PeerGone,
        IpcError::PreDispatchTimeout,
        IpcError::Indeterminate,
        IpcError::Cancelled,
    ];

    #[test]
    fn only_failures_before_acceptance_are_refusals() {
        for stale in [false, true] {
            for error in ALL {
                let refused = matches!(classify(error, stale), Failure::Refused(_));
                let before_acceptance = matches!(
                    error,
                    IpcError::Encode
                        | IpcError::Busy
                        | IpcError::Send
                        | IpcError::PreDispatchTimeout
                        | IpcError::Cancelled
                );
                assert_eq!(refused, before_acceptance, "{error:?} stale={stale}");
            }
        }
    }

    #[test]
    fn a_stale_binding_names_the_replacement_only_for_lifecycle_failures() {
        assert_eq!(
            classify(IpcError::PeerGone, true),
            Failure::Uncertain(Uncertainty::ProviderReplaced)
        );
        assert_eq!(
            classify(IpcError::PeerGone, false),
            Failure::Uncertain(Uncertainty::ProviderFailed)
        );
        assert_eq!(
            classify(IpcError::PreDispatchTimeout, true),
            Failure::Refused(Refusal::NoBinding)
        );
        for error in ALL {
            if !needs_binding_check(error) {
                assert_eq!(classify(error, true), classify(error, false), "{error:?}");
            }
        }
    }

    #[test]
    fn collapsing_keeps_the_sdk_error_codes() {
        let code = |error, stale| CallOutcome::<()>::from(classify(error, stale)).into_result();
        assert_eq!(code(IpcError::Encode, false), Err(ViError::InvalidArgument));
        assert_eq!(code(IpcError::PeerGone, true), Err(ViError::NotFound));
        assert_eq!(code(IpcError::Send, true), Err(ViError::NotFound));
        assert_eq!(code(IpcError::Busy, false), Err(ViError::IO));
        assert_eq!(code(IpcError::Indeterminate, false), Err(ViError::IO));
        assert_eq!(CallOutcome::Reply(7).into_result(), Ok(7));
    }
}
