//! Non-secret diagnostics for fatal guest HVC exits.

use crate::layout::{HVC_SILO_DONE, HVC_SILO_FAULT, HVC_SILO_READY};

/// A recognized private Silo HVC. Unknown register values are never retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognizedSiloHvc {
    /// Guest reported successful initialization.
    Ready,
    /// Guest reported successful signing.
    Done,
    /// Guest reported a bounded fault code.
    Fault { detail_code: u64 },
}

/// Redacted diagnostic for an unexpected HVC exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnexpectedHvc {
    /// Immediate encoded in the HVC instruction.
    pub imm: u16,
    /// Recognized Silo function, or `None` with all guest registers discarded.
    pub recognized: Option<RecognizedSiloHvc>,
}

/// Retain x0/x1 only when x0 is one of the private Silo function identifiers.
pub fn diagnose_hvc(imm: u16, regs: [u64; 8]) -> UnexpectedHvc {
    let recognized = match regs[0] {
        HVC_SILO_READY => Some(RecognizedSiloHvc::Ready),
        HVC_SILO_DONE => Some(RecognizedSiloHvc::Done),
        HVC_SILO_FAULT => Some(RecognizedSiloHvc::Fault {
            detail_code: regs[1],
        }),
        _ => None,
    };
    UnexpectedHvc { imm, recognized }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_hvc_discards_guest_registers() {
        let first = diagnose_hvc(7, [0x1111; 8]);
        let second = diagnose_hvc(7, [0x2222; 8]);
        assert_eq!(
            first,
            UnexpectedHvc {
                imm: 7,
                recognized: None
            }
        );
        assert_eq!(first, second);
    }

    #[test]
    fn recognized_fault_retains_only_bounded_detail() {
        let mut regs = [0xaaaa; 8];
        regs[0] = HVC_SILO_FAULT;
        regs[1] = 0x41;
        assert_eq!(
            diagnose_hvc(0, regs),
            UnexpectedHvc {
                imm: 0,
                recognized: Some(RecognizedSiloHvc::Fault { detail_code: 0x41 }),
            }
        );
    }
}
