use super::super::types::{IngressDecision, QueuedReply, RequestKey, WorkerRequest};
use super::BrokerState;
use crate::local_ingress::ReplyStatus;
use api::caller_identity::CallerIdentity;

impl BrokerState {
    pub(super) fn immediate(
        &mut self,
        sender_tid: usize,
        request_id: u64,
        client_sequence: u64,
        status: ReplyStatus,
    ) -> IngressDecision {
        match status {
            ReplyStatus::Busy => self.counters.busy += 1,
            ReplyStatus::Indeterminate | ReplyStatus::NotSupported => {
                self.counters.indeterminate += 1
            }
            ReplyStatus::Success => self.counters.terminal += 1,
        }
        IngressDecision::Immediate(QueuedReply::new(
            sender_tid,
            request_id,
            client_sequence,
            status,
            &[],
            self.bump_reply_order(),
        ))
    }

    pub(super) fn active_for(&self, identity: CallerIdentity) -> usize {
        self.inflight
            .iter()
            .flatten()
            .filter(|key| key.cell_id == identity.cell_id && key.generation == identity.generation)
            .count()
    }

    pub(super) fn alloc_request_id(&mut self) -> Option<u64> {
        let candidate = self.next_request_id;
        if candidate == 0 {
            return None;
        }
        self.next_request_id = candidate.wrapping_add(1);
        if self
            .inflight
            .iter()
            .flatten()
            .any(|key| key.request_id == candidate)
            || self
                .stale
                .iter()
                .flatten()
                .any(|key| key.request_id == candidate)
        {
            return None;
        }
        Some(candidate)
    }

    pub(super) fn stale_contains(&self, key: RequestKey) -> bool {
        self.stale.iter().flatten().any(|entry| *entry == key)
    }

    pub(super) fn push_request(&mut self, request: WorkerRequest) {
        self.requests[self.requests.iter().position(Option::is_none).unwrap()] = Some(request);
    }

    pub(super) fn push_reply(&mut self, reply: QueuedReply) {
        self.replies[self.replies.iter().position(Option::is_none).unwrap()] = Some(reply);
    }

    pub(super) fn insert_inflight(&mut self, key: RequestKey) {
        self.inflight[self.inflight.iter().position(Option::is_none).unwrap()] = Some(key);
    }

    pub(super) fn bump_request_order(&mut self) -> u64 {
        let order = self.next_request_order;
        self.next_request_order = self.next_request_order.wrapping_add(1);
        order
    }

    pub(super) fn bump_reply_order(&mut self) -> u64 {
        let order = self.next_reply_order;
        self.next_reply_order = self.next_reply_order.wrapping_add(1);
        order
    }

    pub(super) fn refresh_peaks(&mut self) {
        self.counters.peak_request_queue = self.counters.peak_request_queue.max(self.request_len());
        self.counters.peak_reply_queue = self.counters.peak_reply_queue.max(self.reply_len());
        self.counters.peak_in_flight = self.counters.peak_in_flight.max(self.inflight_len());
        self.counters.peak_bytes = self.counters.peak_bytes.max(
            self.request_len() * crate::local_ingress::MAX_REQUEST_BODY
                + self.reply_len() * crate::local_ingress::MAX_REPLY_BODY,
        );
    }

    pub fn note_try_send_busy(&mut self) {
        self.counters.try_send_busy += 1;
    }

    pub fn note_terminal_reply(&mut self) {
        self.counters.terminal += 1;
    }

    pub fn note_heartbeat_miss(&mut self) {
        self.counters.heartbeat_miss += 1;
    }

    pub fn note_network_poll(&mut self) {
        self.counters.network_progress += 1;
    }
}
