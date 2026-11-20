//! When the net service may claim `[net-rx-producer] irq->completion PASS`.
//!
//! The claim needs a NET_RX completion record, which only the NIC interrupt
//! produces, tied to a frame the service really received. The frame may be
//! polled on either side of that record. Replies to the service's own
//! transmissions usually arrive inside the turn that sent the request, and
//! the RX poll drains them before the service parks. Their interrupt then
//! surfaces only through the level flag on a later wait. A `restrict=on`
//! QEMU network carries nothing else (the DHCP OFFER and ACK), so a rule that
//! accepted only frames drained *after* the record never passed there.

#[derive(Debug, Default)]
pub(super) struct NetRxProof {
    proved: bool,
    /// Frames were drained since the last NET_RX completion was consumed.
    rx_since_completion: bool,
    /// A NET_RX completion awaits the next turn's drain; one turn only.
    completion_pending: bool,
}

impl NetRxProof {
    /// One turn drained `frames` frames. Returns true exactly once: on the
    /// call that completes the proof.
    pub(super) fn drained(&mut self, frames: usize) -> bool {
        let completion_before = core::mem::take(&mut self.completion_pending);
        if frames == 0 {
            return false;
        }
        self.rx_since_completion = true;
        self.prove(completion_before)
    }

    /// A wait returned a NET_RX completion record. Returns true exactly once:
    /// on the call that completes the proof.
    pub(super) fn completion(&mut self) -> bool {
        let frames_before = core::mem::take(&mut self.rx_since_completion);
        self.completion_pending = true;
        self.prove(frames_before)
    }

    fn prove(&mut self, tied: bool) -> bool {
        if tied && !self.proved {
            self.proved = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::NetRxProof;

    #[test]
    fn frame_drained_before_the_completion_proves() {
        let mut proof = NetRxProof::default();
        assert!(!proof.drained(1));
        assert!(proof.completion());
    }

    #[test]
    fn frame_drained_after_the_completion_proves() {
        let mut proof = NetRxProof::default();
        assert!(!proof.completion());
        assert!(proof.drained(1));
    }

    #[test]
    fn completion_without_any_frame_does_not_prove() {
        let mut proof = NetRxProof::default();
        assert!(!proof.completion());
        assert!(!proof.drained(0));
        assert!(!proof.completion());
        assert!(!proof.drained(0));
    }

    #[test]
    fn completion_expires_after_one_empty_turn() {
        let mut proof = NetRxProof::default();
        assert!(!proof.completion());
        assert!(!proof.drained(0));
        assert!(
            !proof.drained(1),
            "a later frame is not tied to the stale record"
        );
    }

    #[test]
    fn frames_count_once_toward_a_completion() {
        let mut proof = NetRxProof::default();
        assert!(!proof.drained(0));
        assert!(!proof.completion());
        assert!(!proof.drained(0));
        assert!(!proof.completion(), "no frame since the previous record");
    }

    #[test]
    fn proof_is_reported_once() {
        let mut proof = NetRxProof::default();
        assert!(!proof.drained(1));
        assert!(proof.completion());
        assert!(!proof.drained(1));
        assert!(!proof.completion());
    }
}
