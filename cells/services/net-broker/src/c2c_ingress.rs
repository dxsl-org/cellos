// SPDX-License-Identifier: Apache-2.0
//! Non-activating C2C broker ingress decision core.
//!
//! Validates authenticated peer identity, provisioned method policy, export registry,
//! live destination generation, protected source epoch provenance, deduplication, and
//! per-peer / global work and byte quotas BEFORE any dispatch is admitted.
//!
//! This module contains no remote transport, network transmission, or public ABI.
//! Production constructors for trust proofs are deliberately absent.

use api::services::cluster::{CellNetId, ClusterId};
use types::c2c::ServerEpoch;

use crate::c2c_dedup::{
    C2cStatus, CachedReply, DedupCache, DedupDecision, DedupError, DedupKey,
};
use crate::c2c_envelope::{
    self, C2cEnvelope, EnvelopeError, EnvelopeKind, RetryClass, MAX_C2C_FRAME,
};
use crate::export_registry::RemoteExports;

pub const MAX_TRACKED_PEERS: usize = 16;
pub const MAX_PER_PEER_WORK: usize = 4;
pub const MAX_PER_PEER_BYTES: usize = 2 * MAX_C2C_FRAME;
pub const MAX_GLOBAL_WORK: usize = 16;

/// Proven authenticated peer identity supplied by the transport boundary (Noise KKpsk0).
///
/// Fields are private so arbitrary untrusted bytes cannot synthesize an authenticated peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthenticatedPeer {
    node_id: CellNetId,
    cluster_id: ClusterId,
}

impl AuthenticatedPeer {
    #[cfg(test)]
    pub const fn synthetic(node_id: CellNetId, cluster_id: ClusterId) -> Self {
        Self {
            node_id,
            cluster_id,
        }
    }

    pub const fn node_id(&self) -> CellNetId {
        self.node_id
    }

    pub const fn cluster_id(&self) -> ClusterId {
        self.cluster_id
    }
}

/// Explicit allowed method rule for an authorized peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisionedMethod {
    pub service_id: u16,
    pub export_id: u16,
    pub version: u8,
    pub retry_class: RetryClass,
}

pub const MAX_POLICY_METHODS: usize = 8;

/// Independently provisioned method authorization policy for a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvisionedPeerPolicy {
    peer_node: CellNetId,
    allowed: [Option<ProvisionedMethod>; MAX_POLICY_METHODS],
}

impl ProvisionedPeerPolicy {
    #[cfg(test)]
    pub const fn synthetic(peer_node: CellNetId, allowed: [Option<ProvisionedMethod>; MAX_POLICY_METHODS]) -> Self {
        Self { peer_node, allowed }
    }

    pub fn allows(
        &self,
        peer: &AuthenticatedPeer,
        service_id: u16,
        export_id: u16,
        version: u8,
        retry_class: RetryClass,
    ) -> bool {
        if self.peer_node != peer.node_id() {
            return false;
        }
        self.allowed.iter().flatten().any(|m| {
            m.service_id == service_id
                && m.export_id == export_id
                && m.version == version
                && m.retry_class == retry_class
        })
    }
}

/// Kernel/service-registry proof that the target local service is registered and live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveDestinationProof {
    service_id: u16,
    server_epoch: ServerEpoch,
}

impl LiveDestinationProof {
    #[cfg(test)]
    pub const fn synthetic(service_id: u16, server_epoch: ServerEpoch) -> Self {
        Self {
            service_id,
            server_epoch,
        }
    }

    pub fn matches(&self, service_id: u16, server_epoch: ServerEpoch) -> bool {
        self.service_id == service_id && self.server_epoch == server_epoch
    }
}

/// Monotonic, non-rollback source epoch provenance verified by the protected authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectedEpochProof {
    src_node: CellNetId,
    src_boot_epoch: u64,
}

impl ProtectedEpochProof {
    #[cfg(test)]
    pub const fn synthetic(src_node: CellNetId, src_boot_epoch: u64) -> Self {
        Self {
            src_node,
            src_boot_epoch,
        }
    }

    pub fn verifies(&self, src_node: CellNetId, src_boot_epoch: u64) -> bool {
        self.src_node == src_node && self.src_boot_epoch == src_boot_epoch
    }
}

/// Opaque token returned on admission, required to release reserved work and byte quotas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionToken {
    key: DedupKey,
    peer_node: CellNetId,
    frame_len: usize,
}

impl AdmissionToken {
    pub const fn key(&self) -> DedupKey {
        self.key
    }
}

/// Outcome of evaluating an ingress frame.
#[derive(Debug)]
pub enum IngressAdmission<'a, 'b> {
    Dispatch(AdmissionToken, C2cEnvelope<'a>),
    Replay(CachedReply<'b>),
}

/// Deterministic rejection reasons enforced before dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngressRejection {
    MalformedEnvelope(EnvelopeError),
    UnsupportedKind(EnvelopeKind),
    PeerIdentityMismatch,
    DestinationMismatch,
    UnauthorizedMethod,
    ExportNotAvailable,
    DestinationNotLive,
    UnverifiedEpochProvenance,
    DedupBusy,
    ReplayRejected,
    GlobalCapacityExceeded,
    PeerWorkQuotaExceeded,
    PeerByteQuotaExceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PeerQuotaState {
    node: CellNetId,
    inflight_work: usize,
    inflight_bytes: usize,
}

/// Bounded decision core managing dedup and per-peer ingress quotas.
pub struct IngressDecisionCore {
    local_node: CellNetId,
    local_cluster: ClusterId,
    dedup: DedupCache,
    peers: [Option<PeerQuotaState>; MAX_TRACKED_PEERS],
    global_inflight: usize,
}

impl IngressDecisionCore {
    pub const fn new(local_node: CellNetId, local_cluster: ClusterId) -> Self {
        Self {
            local_node,
            local_cluster,
            dedup: DedupCache::new(),
            peers: [None; MAX_TRACKED_PEERS],
            global_inflight: 0,
        }
    }

    /// Evaluate an incoming frame through all policy, authority, and capacity gates.
    pub fn evaluate_frame<'a, 'b>(
        &'b mut self,
        frame: &'a [u8],
        peer: &AuthenticatedPeer,
        policy: &ProvisionedPeerPolicy,
        exports: &RemoteExports,
        live_dst: &LiveDestinationProof,
        epoch_proof: &ProtectedEpochProof,
        now_ms: u64,
    ) -> Result<IngressAdmission<'a, 'b>, IngressRejection> {
        let envelope = c2c_envelope::decode(frame).map_err(IngressRejection::MalformedEnvelope)?;

        if envelope.kind != EnvelopeKind::Request {
            return Err(IngressRejection::UnsupportedKind(envelope.kind));
        }

        // Gate 1: Source identity must match transport-authenticated peer.
        if envelope.src_node != peer.node_id() || envelope.cluster_id != peer.cluster_id() {
            return Err(IngressRejection::PeerIdentityMismatch);
        }

        // Gate 2: Destination must match local broker.
        if envelope.dst_node != self.local_node {
            return Err(IngressRejection::DestinationMismatch);
        }

        // Gate 3: Independently provisioned peer policy must allow this method.
        if !policy.allows(
            peer,
            envelope.service_id,
            envelope.export_id,
            c2c_envelope::C2C_VERSION,
            envelope.retry_class,
        ) {
            return Err(IngressRejection::UnauthorizedMethod);
        }

        // Gate 4: Local export registry must have an active remote export matching the method.
        if !exports.prototype_method_matches(
            envelope.service_id,
            envelope.export_id,
            c2c_envelope::C2C_VERSION,
            envelope.retry_class,
        ) {
            return Err(IngressRejection::ExportNotAvailable);
        }

        // Gate 5: Live target local service generation must match destination epoch.
        if !live_dst.matches(envelope.service_id, envelope.dst_server_epoch) {
            return Err(IngressRejection::DestinationNotLive);
        }

        // Gate 6: Source boot epoch must have protected authority provenance proof.
        if !epoch_proof.verifies(envelope.src_node, envelope.src_boot_epoch) {
            return Err(IngressRejection::UnverifiedEpochProvenance);
        }

        // Gate 7: Check for cached completed replay before quota checks.
        let dedup_key = DedupKey {
            src_node: envelope.src_node,
            src_boot_epoch: envelope.src_boot_epoch,
            request_id: envelope.request_id,
            dst_server_epoch: envelope.dst_server_epoch,
        };

        if let Some(slot) = self.dedup.has_completed_replay(dedup_key, now_ms) {
            let reply = self.dedup.replay(slot, dedup_key, now_ms).expect("verified");
            return Ok(IngressAdmission::Replay(reply));
        }

        // Gate 8: Capacity and quota checks before admitting to dedup and dispatch.
        if self.global_inflight >= MAX_GLOBAL_WORK {
            return Err(IngressRejection::GlobalCapacityExceeded);
        }

        let peer_slot = self.find_or_alloc_peer_slot(peer.node_id())?;
        let peer_state = self.peers[peer_slot].as_mut().expect("allocated slot");

        if peer_state.inflight_work >= MAX_PER_PEER_WORK {
            return Err(IngressRejection::PeerWorkQuotaExceeded);
        }

        if peer_state.inflight_bytes.saturating_add(frame.len()) > MAX_PER_PEER_BYTES {
            return Err(IngressRejection::PeerByteQuotaExceeded);
        }

        // Gate 9: Dedup and replay window evaluation.
        match self.dedup.begin(dedup_key, envelope.retry_class, now_ms) {
            DedupDecision::Dispatch => {}
            DedupDecision::Replay(slot) => {
                if let Some(reply) = self.dedup.replay(slot, dedup_key, now_ms) {
                    return Ok(IngressAdmission::Replay(reply));
                }
                return Err(IngressRejection::ReplayRejected);
            }
            DedupDecision::Busy => return Err(IngressRejection::DedupBusy),
            DedupDecision::Indeterminate => return Err(IngressRejection::ReplayRejected),
        }

        // Reserve quotas and mark dispatched in dedup.
        peer_state.inflight_work += 1;
        peer_state.inflight_bytes += frame.len();
        self.global_inflight += 1;

        if let Err(_e) = self.dedup.mark_dispatched(dedup_key) {
            self.dedup.cancel_admitted(dedup_key);
            peer_state.inflight_work -= 1;
            peer_state.inflight_bytes -= frame.len();
            self.global_inflight -= 1;
            return Err(IngressRejection::DedupBusy);
        }
        let token = AdmissionToken {
            key: dedup_key,
            peer_node: peer.node_id(),
            frame_len: frame.len(),
        };

        Ok(IngressAdmission::Dispatch(token, envelope))
    }

    /// Complete an admitted request with status and optional cached reply payload.
    pub fn release_completion(
        &mut self,
        token: AdmissionToken,
        status: C2cStatus,
        reply_payload: &[u8],
    ) -> Result<(), DedupError> {
        self.dedup.complete(token.key, status, reply_payload)?;
        self.release_quotas(token.peer_node, token.frame_len);
        Ok(())
    }

    /// Release reserved quotas if dispatch failed or was dropped without completing.
    pub fn release_dropped(&mut self, token: AdmissionToken) {
        self.release_quotas(token.peer_node, token.frame_len);
    }

    pub fn global_inflight(&self) -> usize {
        self.global_inflight
    }

    pub fn peer_inflight(&self, peer_node: CellNetId) -> usize {
        self.peers
            .iter()
            .flatten()
            .find(|p| p.node == peer_node)
            .map_or(0, |p| p.inflight_work)
    }

    fn find_or_alloc_peer_slot(&mut self, peer_node: CellNetId) -> Result<usize, IngressRejection> {
        if let Some(index) = self.peers.iter().position(|p| p.is_some_and(|p| p.node == peer_node)) {
            return Ok(index);
        }
        if let Some(index) = self.peers.iter().position(Option::is_none) {
            self.peers[index] = Some(PeerQuotaState {
                node: peer_node,
                inflight_work: 0,
                inflight_bytes: 0,
            });
            return Ok(index);
        }
        // If all slots are full, try to reclaim an idle peer slot with 0 in-flight work.
        if let Some(index) = self
            .peers
            .iter()
            .position(|p| p.is_some_and(|p| p.inflight_work == 0 && p.inflight_bytes == 0))
        {
            self.peers[index] = Some(PeerQuotaState {
                node: peer_node,
                inflight_work: 0,
                inflight_bytes: 0,
            });
            return Ok(index);
        }
        Err(IngressRejection::GlobalCapacityExceeded)
    }

    fn release_quotas(&mut self, peer_node: CellNetId, frame_len: usize) {
        self.global_inflight = self.global_inflight.saturating_sub(1);
        if let Some(peer_state) = self
            .peers
            .iter_mut()
            .flatten()
            .find(|p| p.node == peer_node)
        {
            peer_state.inflight_work = peer_state.inflight_work.saturating_sub(1);
            peer_state.inflight_bytes = peer_state.inflight_bytes.saturating_sub(frame_len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::services::cluster::{CellNetId, ClusterId};
    use types::c2c::RelativeDeadline;
    use crate::c2c_envelope::MAX_C2C_PAYLOAD;

    fn node(id: u8) -> CellNetId {
        let mut b = [0u8; 32];
        b[0] = id;
        CellNetId(b)
    }

    fn cluster(id: u64) -> ClusterId {
        ClusterId(id)
    }

    fn sample_envelope(
        src_node: CellNetId,
        dst_node: CellNetId,
        request_id: u64,
        src_epoch: u64,
        dst_epoch: u64,
    ) -> C2cEnvelope<'static> {
        C2cEnvelope {
            kind: EnvelopeKind::Request,
            retry_class: RetryClass::Idempotent,
            request_id,
            src_node,
            dst_node,
            src_boot_epoch: src_epoch,
            dst_server_epoch: ServerEpoch::new(dst_epoch).unwrap(),
            cluster_id: cluster(100),
            service_id: 8,
            export_id: 1,
            relative_deadline: RelativeDeadline::new(5000).unwrap(),
            payload: b"ping",
        }
    }

    fn make_exports() -> RemoteExports {
        const CFG: &[u8] = b"c2c_exports_version=1\n\
export_0_service_id=8\n\
export_0_export_id=1\n\
export_0_version=1\n\
export_0_retry_class=idempotent\n\
export_0_scope=remote\n";
        RemoteExports::from_bytes(Some(CFG))
    }

    #[test]
    fn happy_path_dispatch_and_completion() {
        let local = node(1);
        let remote = node(2);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(
            remote,
            [
                Some(ProvisionedMethod {
                    service_id: 8,
                    export_id: 1,
                    version: 1,
                    retry_class: RetryClass::Idempotent,
                }),
                None, None, None, None, None, None, None,
            ],
        );
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 555);

        let env = sample_envelope(remote, local, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let res = core.evaluate_frame(
            &frame_buf[..len],
            &peer,
            &policy,
            &exports,
            &live_dst,
            &epoch_proof,
            1_000,
        );

        match res {
            Ok(IngressAdmission::Dispatch(token, decoded)) => {
                assert_eq!(decoded.request_id, 1);
                assert_eq!(core.global_inflight(), 1);
                assert_eq!(core.peer_inflight(remote), 1);

                // Complete request
                core.release_completion(token, C2cStatus::Success, b"pong").unwrap();
                assert_eq!(core.global_inflight(), 0);
                assert_eq!(core.peer_inflight(remote), 0);
            }
            _ => panic!("expected dispatch admission"),
        }

        // Second time: exact duplicate replay
        let res2 = core.evaluate_frame(
            &frame_buf[..len],
            &peer,
            &policy,
            &exports,
            &live_dst,
            &epoch_proof,
            1_050,
        );
        match res2 {
            Ok(IngressAdmission::Replay(reply)) => {
                assert_eq!(reply.status, C2cStatus::Success);
                assert_eq!(reply.payload, b"pong");
            }
            _ => panic!("expected replay admission"),
        }
    }

    #[test]
    fn rejects_peer_identity_mismatch() {
        let local = node(1);
        let remote = node(2);
        let imposter = node(3);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(remote, [None; 8]);
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(imposter, 555);

        let env = sample_envelope(imposter, local, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let err = core
            .evaluate_frame(
                &frame_buf[..len],
                &peer,
                &policy,
                &exports,
                &live_dst,
                &epoch_proof,
                1_000,
            )
            .unwrap_err();

        assert_eq!(err, IngressRejection::PeerIdentityMismatch);
    }

    #[test]
    fn rejects_destination_mismatch() {
        let local = node(1);
        let remote = node(2);
        let wrong_dst = node(99);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(remote, [None; 8]);
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 555);

        let env = sample_envelope(remote, wrong_dst, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let err = core
            .evaluate_frame(
                &frame_buf[..len],
                &peer,
                &policy,
                &exports,
                &live_dst,
                &epoch_proof,
                1_000,
            )
            .unwrap_err();

        assert_eq!(err, IngressRejection::DestinationMismatch);
    }

    #[test]
    fn rejects_unauthorized_method() {
        let local = node(1);
        let remote = node(2);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        // Empty policy
        let policy = ProvisionedPeerPolicy::synthetic(remote, [None; 8]);
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 555);

        let env = sample_envelope(remote, local, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let err = core
            .evaluate_frame(
                &frame_buf[..len],
                &peer,
                &policy,
                &exports,
                &live_dst,
                &epoch_proof,
                1_000,
            )
            .unwrap_err();

        assert_eq!(err, IngressRejection::UnauthorizedMethod);
    }

    #[test]
    fn rejects_destination_not_live() {
        let local = node(1);
        let remote = node(2);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(
            remote,
            [
                Some(ProvisionedMethod {
                    service_id: 8,
                    export_id: 1,
                    version: 1,
                    retry_class: RetryClass::Idempotent,
                }),
                None, None, None, None, None, None, None,
            ],
        );
        let exports = make_exports();
        // Server epoch is 11, but envelope targets 10
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(11).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 555);

        let env = sample_envelope(remote, local, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let err = core
            .evaluate_frame(
                &frame_buf[..len],
                &peer,
                &policy,
                &exports,
                &live_dst,
                &epoch_proof,
                1_000,
            )
            .unwrap_err();

        assert_eq!(err, IngressRejection::DestinationNotLive);
    }

    #[test]
    fn rejects_unverified_epoch_provenance() {
        let local = node(1);
        let remote = node(2);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(
            remote,
            [
                Some(ProvisionedMethod {
                    service_id: 8,
                    export_id: 1,
                    version: 1,
                    retry_class: RetryClass::Idempotent,
                }),
                None, None, None, None, None, None, None,
            ],
        );
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        // Epoch proof says 999, envelope says 555
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 999);

        let env = sample_envelope(remote, local, 1, 555, 10);
        let mut frame_buf = [0u8; 256];
        let len = env.encode(&mut frame_buf).unwrap();

        let err = core
            .evaluate_frame(
                &frame_buf[..len],
                &peer,
                &policy,
                &exports,
                &live_dst,
                &epoch_proof,
                1_000,
            )
            .unwrap_err();

        assert_eq!(err, IngressRejection::UnverifiedEpochProvenance);
    }

    #[test]
    fn peer_work_quota_isolation() {
        let local = node(1);
        let peer_a = node(2);
        let peer_b = node(3);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let auth_a = AuthenticatedPeer::synthetic(peer_a, cluster(100));
        let auth_b = AuthenticatedPeer::synthetic(peer_b, cluster(100));

        let method = Some(ProvisionedMethod {
            service_id: 8,
            export_id: 1,
            version: 1,
            retry_class: RetryClass::Idempotent,
        });
        let policy_a = ProvisionedPeerPolicy::synthetic(peer_a, [method, None, None, None, None, None, None, None]);
        let policy_b = ProvisionedPeerPolicy::synthetic(peer_b, [method, None, None, None, None, None, None, None]);
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_a = ProtectedEpochProof::synthetic(peer_a, 100);
        let epoch_b = ProtectedEpochProof::synthetic(peer_b, 100);

        let mut tokens_a = [None; 4];
        for i in 0..4 {
            let env = sample_envelope(peer_a, local, (i + 1) as u64, 100, 10);
            let mut buf = [0u8; 256];
            let len = env.encode(&mut buf).unwrap();
            match core.evaluate_frame(&buf[..len], &auth_a, &policy_a, &exports, &live_dst, &epoch_a, 1000) {
                Ok(IngressAdmission::Dispatch(token, _)) => tokens_a[i] = Some(token),
                _other => panic!("expected dispatch for req {i}"),
            }
        }
        assert_eq!(core.peer_inflight(peer_a), 4);

        // 5th request from peer A exceeds quota
        let env_a5 = sample_envelope(peer_a, local, 5, 100, 10);
        let mut buf_a5 = [0u8; 256];
        let len_a5 = env_a5.encode(&mut buf_a5).unwrap();
        let err = core
            .evaluate_frame(&buf_a5[..len_a5], &auth_a, &policy_a, &exports, &live_dst, &epoch_a, 1000)
            .unwrap_err();
        assert_eq!(err, IngressRejection::PeerWorkQuotaExceeded);

        // Peer B is NOT blocked by peer A's saturation
        let env_b1 = sample_envelope(peer_b, local, 1, 100, 10);
        let mut buf_b1 = [0u8; 256];
        let len_b1 = env_b1.encode(&mut buf_b1).unwrap();
        let res_b = core.evaluate_frame(&buf_b1[..len_b1], &auth_b, &policy_b, &exports, &live_dst, &epoch_b, 1000);
        assert!(matches!(res_b, Ok(IngressAdmission::Dispatch(_, _))));

        // Release one token for peer A -> peer A can now dispatch again
        core.release_completion(tokens_a[0].unwrap(), C2cStatus::Success, b"ok").unwrap();
        assert_eq!(core.peer_inflight(peer_a), 3);

        let res_a5_retry = core.evaluate_frame(&buf_a5[..len_a5], &auth_a, &policy_a, &exports, &live_dst, &epoch_a, 1000);
        assert!(matches!(res_a5_retry, Ok(IngressAdmission::Dispatch(_, _))));
    }

    #[test]
    fn peer_byte_quota_enforced() {
        let local = node(1);
        let remote = node(2);
        let mut core = IngressDecisionCore::new(local, cluster(100));

        let peer = AuthenticatedPeer::synthetic(remote, cluster(100));
        let policy = ProvisionedPeerPolicy::synthetic(
            remote,
            [
                Some(ProvisionedMethod {
                    service_id: 8,
                    export_id: 1,
                    version: 1,
                    retry_class: RetryClass::Idempotent,
                }),
                None, None, None, None, None, None, None,
            ],
        );
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());
        let epoch_proof = ProtectedEpochProof::synthetic(remote, 555);

        // Frame 1: max frame payload
        let large_payload = [0xAAu8; MAX_C2C_PAYLOAD];
        let mut env1 = sample_envelope(remote, local, 1, 555, 10);
        env1.payload = &large_payload;
        let mut buf1 = [0u8; MAX_C2C_FRAME + 32];
        let len1 = env1.encode(&mut buf1).unwrap();

        let res1 = core.evaluate_frame(&buf1[..len1], &peer, &policy, &exports, &live_dst, &epoch_proof, 1000);
        assert!(matches!(res1, Ok(IngressAdmission::Dispatch(_, _))));

        // Frame 2: another large frame (fits within 2 * MAX_C2C_FRAME)
        let mut env2 = sample_envelope(remote, local, 2, 555, 10);
        env2.payload = &large_payload;
        let mut buf2 = [0u8; MAX_C2C_FRAME + 32];
        let len2 = env2.encode(&mut buf2).unwrap();

        let res2 = core.evaluate_frame(&buf2[..len2], &peer, &policy, &exports, &live_dst, &epoch_proof, 1000);
        assert!(matches!(res2, Ok(IngressAdmission::Dispatch(_, _))));

        // Frame 3: even a tiny 1-byte frame now exceeds MAX_PER_PEER_BYTES
        let env3 = sample_envelope(remote, local, 3, 555, 10);
        let mut buf3 = [0u8; 256];
        let len3 = env3.encode(&mut buf3).unwrap();

        let err3 = core.evaluate_frame(&buf3[..len3], &peer, &policy, &exports, &live_dst, &epoch_proof, 1000).unwrap_err();
        assert_eq!(err3, IngressRejection::PeerByteQuotaExceeded);
    }

    #[test]
    fn global_capacity_enforced_across_peers() {
        let local = node(1);
        let mut core = IngressDecisionCore::new(local, cluster(100));
        let exports = make_exports();
        let live_dst = LiveDestinationProof::synthetic(8, ServerEpoch::new(10).unwrap());

        // Fill global capacity: 4 peers each dispatching 4 requests = 16 in-flight
        let method = Some(ProvisionedMethod {
            service_id: 8,
            export_id: 1,
            version: 1,
            retry_class: RetryClass::Idempotent,
        });

        for p in 2..=5 {
            let peer_node = node(p);
            let auth = AuthenticatedPeer::synthetic(peer_node, cluster(100));
            let policy = ProvisionedPeerPolicy::synthetic(peer_node, [method, None, None, None, None, None, None, None]);
            let epoch_proof = ProtectedEpochProof::synthetic(peer_node, 100);

            for r in 1..=4 {
                let env = sample_envelope(peer_node, local, r, 100, 10);
                let mut buf = [0u8; 256];
                let len = env.encode(&mut buf).unwrap();
                let res = core.evaluate_frame(&buf[..len], &auth, &policy, &exports, &live_dst, &epoch_proof, 1000);
                assert!(matches!(res, Ok(IngressAdmission::Dispatch(_, _))));
            }
        }
        assert_eq!(core.global_inflight(), 16);

        // 17th request from a 5th peer (or any peer) hits global capacity
        let peer_6 = node(6);
        let auth_6 = AuthenticatedPeer::synthetic(peer_6, cluster(100));
        let policy_6 = ProvisionedPeerPolicy::synthetic(peer_6, [method, None, None, None, None, None, None, None]);
        let epoch_6 = ProtectedEpochProof::synthetic(peer_6, 100);

        let env_overflow = sample_envelope(peer_6, local, 1, 100, 10);
        let mut buf_overflow = [0u8; 256];
        let len_overflow = env_overflow.encode(&mut buf_overflow).unwrap();

        let err = core.evaluate_frame(&buf_overflow[..len_overflow], &auth_6, &policy_6, &exports, &live_dst, &epoch_6, 1000).unwrap_err();
        assert_eq!(err, IngressRejection::GlobalCapacityExceeded);
    }
}
