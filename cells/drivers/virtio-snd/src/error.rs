//! Bring-up and control-request failures.

use crate::dma::DmaError;
use core::fmt;
use virtio_pci_transport::{InitFailure, TransportError};

/// Why bring-up stopped.
#[derive(Debug)]
pub enum InitError {
    Transport(TransportError),
    /// The device rejected feature negotiation.
    Negotiation(Option<InitFailure>),
    /// A queue is absent or too short: queue index and device maximum.
    QueueSize(u16, u32),
    Dma(DmaError),
    QueueSetup(InitFailure),
    Control(CtrlError),
    /// No output stream plays 48 kHz S16 stereo.
    NoStream,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Negotiation(Some(failure)) => write!(f, "feature negotiation: {failure}"),
            Self::Negotiation(None) => write!(f, "feature negotiation: FEATURES_OK not kept"),
            Self::QueueSize(queue, max) => write!(f, "queue {queue} max size {max} too small"),
            Self::Dma(error) => write!(f, "{error}"),
            Self::QueueSetup(failure) => write!(f, "queue setup: {failure}"),
            Self::Control(error) => write!(f, "control request: {error:?}"),
            Self::NoStream => write!(f, "no output stream supports 48000 Hz S16 stereo"),
        }
    }
}

/// Why a control request failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlError {
    /// No completion within the budget, or an earlier one never completed:
    /// the device may still own the control chain.
    Timeout,
    /// The used ring returned something never submitted.
    BadCompletion,
    /// The device answered with this non-OK status.
    Status(u32),
}
