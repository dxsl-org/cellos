mod helpers;
#[cfg(test)]
mod tests;

use super::types::{
    BrokerCounters, CompletionError, IngressDecision, QueuedReply, RequestKey, WorkerRequest,
    IN_FLIGHT_CAP, LOCAL_REPLY_QUEUE_CAP, LOCAL_REQUEST_QUEUE_CAP, PER_CALLER_WINDOW,
    STALE_REPLY_RING_CAP,
};
use crate::local_ingress::{ParseError, ParsedLocalRequest, ReplyStatus};
use api::caller_identity::CallerIdentity;

#[derive(Clone, Copy)]
struct CallerKey {
    cell_id: u64,
    generation: u64,
}

pub struct BrokerState {
    pub counters: BrokerCounters,
    requests: [Option<WorkerRequest>; LOCAL_REQUEST_QUEUE_CAP],
    replies: [Option<QueuedReply>; LOCAL_REPLY_QUEUE_CAP],
    inflight: [Option<RequestKey>; IN_FLIGHT_CAP],
    stale: [Option<RequestKey>; STALE_REPLY_RING_CAP],
    stale_cursor: usize,
    next_request_id: u64,
    next_request_order: u64,
    next_reply_order: u64,
    last_served: Option<CallerKey>,
}

impl BrokerState {
    pub const fn new() -> Self {
        Self {
            counters: BrokerCounters {
                accepted: 0,
                completed: 0,
                busy: 0,
                terminal: 0,
                indeterminate: 0,
                duplicate: 0,
                stale_reply: 0,
                try_send_busy: 0,
                heartbeat_miss: 0,
                watchdog_expired: 0,
                peak_request_queue: 0,
                peak_reply_queue: 0,
                peak_in_flight: 0,
                peak_bytes: 0,
                network_progress: 0,
            },
            requests: [None; LOCAL_REQUEST_QUEUE_CAP],
            replies: [None; LOCAL_REPLY_QUEUE_CAP],
            inflight: [None; IN_FLIGHT_CAP],
            stale: [None; STALE_REPLY_RING_CAP],
            stale_cursor: 0,
            next_request_id: 1,
            next_request_order: 0,
            next_reply_order: 0,
            last_served: None,
        }
    }

    pub fn handle_ingress(
        &mut self,
        sender_tid: usize,
        identity: Option<CallerIdentity>,
        parsed: Result<ParsedLocalRequest, ParseError>,
    ) -> IngressDecision {
        let client_sequence = parsed.as_ref().map(|req| req.client_sequence).unwrap_or(0);
        let Some(identity) = identity else {
            return self.immediate(sender_tid, 0, client_sequence, ReplyStatus::Indeterminate);
        };
        if identity.sender_tid as usize != sender_tid || identity.generation == 0 {
            return self.immediate(sender_tid, 0, client_sequence, ReplyStatus::Indeterminate);
        }
        let Ok(parsed) = parsed else {
            return self.immediate(sender_tid, 0, client_sequence, ReplyStatus::Indeterminate);
        };
        if self.request_len() >= LOCAL_REQUEST_QUEUE_CAP
            || self.active_for(identity) >= PER_CALLER_WINDOW
            || self.inflight_len() >= IN_FLIGHT_CAP
        {
            return self.immediate(sender_tid, 0, parsed.client_sequence, ReplyStatus::Busy);
        }
        let Some(request_id) = self.alloc_request_id() else {
            return self.immediate(
                sender_tid,
                0,
                parsed.client_sequence,
                ReplyStatus::Indeterminate,
            );
        };
        let request = WorkerRequest {
            key: RequestKey {
                request_id,
                caller_tid: sender_tid,
                cell_id: identity.cell_id,
                generation: identity.generation,
            },
            client_sequence: parsed.client_sequence,
            payload_len: parsed.payload_len,
            payload: parsed.payload,
            order: self.bump_request_order(),
        };
        self.insert_inflight(request.key);
        self.push_request(request);
        self.counters.accepted += 1;
        self.refresh_peaks();
        IngressDecision::Accepted
    }

    pub fn take_next_request(&mut self) -> Option<WorkerRequest> {
        let pick = self
            .requests
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.map(|req| (idx, req)))
            .min_by_key(|(_, req)| {
                let penalty = self
                    .last_served
                    .filter(|last| {
                        last.cell_id == req.key.cell_id && last.generation == req.key.generation
                    })
                    .is_some() as u8;
                (penalty, req.order)
            })?;
        self.requests[pick.0].take().inspect(|req| {
            self.last_served = Some(CallerKey {
                cell_id: req.key.cell_id,
                generation: req.key.generation,
            });
        })
    }

    pub fn complete_request(
        &mut self,
        request: &WorkerRequest,
        reply: QueuedReply,
    ) -> Result<(), CompletionError> {
        if self.stale_contains(request.key) {
            self.counters.duplicate += 1;
            self.counters.stale_reply += 1;
            return Err(CompletionError::Stale);
        }
        let Some(slot) = self
            .inflight
            .iter()
            .position(|entry| *entry == Some(request.key))
        else {
            self.counters.stale_reply += 1;
            return Err(CompletionError::Stale);
        };
        if self.reply_len() >= LOCAL_REPLY_QUEUE_CAP {
            return Err(CompletionError::ReplyQueueFull);
        }
        self.inflight[slot] = None;
        self.stale[self.stale_cursor % STALE_REPLY_RING_CAP] = Some(request.key);
        self.stale_cursor = (self.stale_cursor + 1) % STALE_REPLY_RING_CAP;
        self.push_reply(reply);
        self.counters.completed += 1;
        self.refresh_peaks();
        Ok(())
    }

    pub fn take_next_reply(&mut self) -> Option<QueuedReply> {
        let idx = self
            .replies
            .iter()
            .enumerate()
            .filter_map(|(idx, slot)| slot.map(|reply| (idx, reply.order)))
            .min_by_key(|(_, order)| *order)?
            .0;
        self.replies[idx].take()
    }

    pub fn requeue_reply(&mut self, mut reply: QueuedReply) -> bool {
        if self.reply_len() >= LOCAL_REPLY_QUEUE_CAP {
            self.counters.terminal += 1;
            return false;
        }
        reply.order = self.bump_reply_order();
        reply.attempts = reply.attempts.saturating_add(1);
        self.push_reply(reply);
        self.refresh_peaks();
        true
    }

    pub fn request_len(&self) -> usize {
        self.requests.iter().flatten().count()
    }

    pub fn reply_len(&self) -> usize {
        self.replies.iter().flatten().count()
    }

    pub fn inflight_len(&self) -> usize {
        self.inflight.iter().flatten().count()
    }
}
