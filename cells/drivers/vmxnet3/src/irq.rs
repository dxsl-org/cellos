//! MSI-X vector routing for the bounded bring-up, link and TX-completion waits
//! (H-S3-4 b); the waits themselves are `nic_driver::irq::BoundedWait`.
//!
//! RX is **not** interrupt-driven: the net service asks for frames (`OP_RX`),
//! so the RX vector stays masked (`rx=request-driven`).
//!
//! Vectors ([`IntrPlan`]): TX queue 0 → 0, RX queue 0 → 1 (masked), events
//! (link change, queue errors) → 2. `DriverShared.automask` is 0, so the
//! device sends a message for every cause while its vector is unmasked and
//! the cell never has to re-arm a vector.

/// MSI-X entries asked for: TX queue 0, RX queue 0, events.
pub(crate) const WANT_VECTORS: usize = 3;
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
