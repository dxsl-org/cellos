//! MSI-X wake-ups for bounded bring-up, link and TX-completion waits (H-S3-4 b).
//!
//! An interrupt is only a hint to re-read the device: every park is bounded by
//! [`PARK_TICKS`], every return re-reads the condition, and every wait has its
//! own park budget, so a lost or revoked interrupt degrades to slow polling,
//! never to a hang. RX is **not** interrupt-driven: the net service asks for
//! frames (`OP_RX`), so the RX vector stays masked (`rx=request-driven`).
//!
//! Vectors (`shared::IntrPlan`): TX queue 0 → 0, RX queue 0 → 1 (masked),
//! events (link change, queue errors) → 2. `DriverShared.automask` is 0, so
//! the device sends a message for every cause while its vector is unmasked
//! and the cell never has to re-arm a vector.

use abi::completion::source::IRQ;
use abi::pcie_function::IRQ_MODE_ALLOW_MSIX;
use ostd::io::print_fmt;
use ostd::ipc::{wait_any, Wake};
use ostd::syscall::sys_alloc_pcie_irq;
use types::ViResult;

/// Longest single park, in scheduler ticks (10 ms each).
pub(crate) const PARK_TICKS: u64 = 10;
/// MSI-X entries asked for: TX queue 0, RX queue 0, events.
pub(crate) const WANT_VECTORS: usize = 3;
/// Condition polls before the first park (QEMU completes TX synchronously).
const SPIN_BEFORE_PARK: u32 = 4_096;
/// Park expiries logged per cell lifetime.
const EXPIRY_LOG_LIMIT: u32 = 3;
/// `DriverShared.ictrl`: every interrupt disabled (the polled configuration).
const ICTRL_DISABLE_ALL: u32 = 0x01;

/// Vector routing (`nintr`, per-queue `intr_idx`, `evintr`) and `ictrl`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IntrPlan {
    pub nintr: u8,
    pub tx: u8,
    pub rx: u8,
    pub event: u8,
    pub ictrl: u32,
}

impl IntrPlan {
    /// `granted` MSI-X vectors (`None`: polled). With three: TX → 0, RX → 1,
    /// events → 2. Fewer fold onto the last granted vector; the RX vector
    /// stays masked unless it is shared with TX or events (RX is
    /// request-driven).
    pub(crate) const fn for_vectors(granted: Option<u8>) -> Self {
        let (nintr, ictrl) = match granted {
            None | Some(0) => (1, ICTRL_DISABLE_ALL),
            Some(n) if n > 3 => (3, 0),
            Some(n) => (n, 0),
        };
        let last = nintr - 1;
        let rx = if last >= 1 { 1 } else { 0 };
        Self {
            nintr,
            tx: 0,
            rx,
            event: last,
            ictrl,
        }
    }

    /// Whether the RX vector is masked: only when it serves RX alone. Masking
    /// a vector RX shares with TX or events would silence those causes too
    /// (with two vectors RX and events share vector 1).
    pub(crate) const fn masks_rx(&self) -> bool {
        self.rx != self.tx && self.rx != self.event
    }
}

/// How the controller's waits learn that something changed.
pub(crate) struct NicIrq {
    /// MSI-X entries held, or `None` when polled.
    vectors: Option<u8>,
    /// Why the cell is polled (`irq=polled reason=…`).
    pub reason: &'static str,
    /// Parks ended by an interrupt / by their deadline (bind-line evidence).
    pub wakes: u32,
    pub expiries: u32,
}

impl NicIrq {
    /// Ask 435 for [`WANT_VECTORS`] MSI-X entries of the owned function `bdf`.
    /// MSI is not accepted: per-queue vectors need MSI-X.
    pub(crate) fn setup(bdf: u32) -> Self {
        let (vectors, reason) = match sys_alloc_pcie_irq(bdf, WANT_VECTORS, IRQ_MODE_ALLOW_MSIX) {
            Ok(alloc) => (Some(alloc.count), ""),
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[vmxnet3] MSI-X refused ({:?}); polling\n",
                    error
                ));
                (None, "alloc-refused")
            }
        };
        Self {
            vectors,
            reason,
            wakes: 0,
            expiries: 0,
        }
    }

    /// MSI-X entries held (`None` when polled).
    pub(crate) fn vectors(&self) -> Option<u8> {
        self.vectors
    }

    /// Wait (bounded) until `ready` holds: a short spin, then up to `parks`
    /// interrupt parks with MSI-X, or `polls` spins when polled. The device is
    /// re-read after every return. `Ok(false)` means the budget ran out.
    pub(crate) fn wait_for(
        &mut self,
        parks: u32,
        polls: u32,
        mut ready: impl FnMut() -> ViResult<bool>,
    ) -> ViResult<bool> {
        for _ in 0..SPIN_BEFORE_PARK {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        if self.vectors.is_some() {
            for _ in 0..parks {
                if !self.park() {
                    break;
                }
                if ready()? {
                    return Ok(true);
                }
            }
            if self.vectors.is_some() {
                return Ok(false);
            }
        }
        for _ in 0..polls {
            if ready()? {
                return Ok(true);
            }
            core::hint::spin_loop();
        }
        Ok(false)
    }

    /// Park on this task's MSI-X lines for at most [`PARK_TICKS`]. `false`
    /// means the wait was refused and the cell is now polled.
    fn park(&mut self) -> bool {
        match wait_any(IRQ, PARK_TICKS) {
            Ok(Wake::Irq(_)) => {
                self.wakes += 1;
                true
            }
            Ok(_) => {
                self.expiries += 1;
                if self.expiries <= EXPIRY_LOG_LIMIT {
                    let _ = print_fmt(format_args!(
                        "[vmxnet3] irq wait expired after {} ticks; re-reading the device\n",
                        PARK_TICKS
                    ));
                }
                true
            }
            Err(error) => {
                let _ = print_fmt(format_args!(
                    "[vmxnet3] irq=polled reason=wait-refused ({:?})\n",
                    error
                ));
                self.vectors = None;
                self.reason = "wait-refused";
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_plan_folds_onto_granted_vectors_and_polls_without_msix() {
        let three = IntrPlan::for_vectors(Some(3));
        assert_eq!(
            (three.nintr, three.tx, three.rx, three.event, three.ictrl),
            (3, 0, 1, 2, 0)
        );
        assert_eq!(
            IntrPlan::for_vectors(Some(8)),
            three,
            "never more than three"
        );
        let two = IntrPlan::for_vectors(Some(2));
        assert_eq!((two.nintr, two.tx, two.rx, two.event), (2, 0, 1, 1));
        let one = IntrPlan::for_vectors(Some(1));
        assert_eq!(
            (one.nintr, one.tx, one.rx, one.event, one.ictrl),
            (1, 0, 0, 0, 0)
        );
        let polled = IntrPlan::for_vectors(None);
        assert_eq!((polled.nintr, polled.ictrl), (1, ICTRL_DISABLE_ALL));
        assert_eq!(IntrPlan::for_vectors(Some(0)), polled);
        // RX alone on its vector is masked; a vector it shares never is.
        assert!(three.masks_rx());
        assert!(!two.masks_rx(), "vector 1 also carries events");
        assert!(!one.masks_rx(), "vector 0 also carries TX and events");
    }
}
