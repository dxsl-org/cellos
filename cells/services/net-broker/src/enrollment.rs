// reason: implements the P09 enrollment/merge-split wire protocol
// (EnrollRequest/EnrollResponse) for the net-broker robot-swarm feature;
// main.rs's dispatch() still has this as a TODO ("handle enrollment /
// merge-split messages") — not yet reachable from the binary's entry point.
#![allow(dead_code)]

/// P09 — Runtime enrollment / merge-split.
///
/// ## Enrollment protocol (4-message, over Noise KKpsk0 session)
///
/// A node joining (or re-joining) an existing cluster performs:
///
///   1. **Beacon discovery** (P05) — node broadcasts a beacon, receives
///      responses from existing cluster peers.
///
///   2. **Noise handshake** (P04) — node initiates a KKpsk0 handshake with
///      each discovered peer (using that peer's public key from the beacon).
///
///   3. **Enrollment request** — node sends `EnrollRequest` over the session:
///      `{ machine_id, cluster_id, boot_epoch, capability_flags }`.
///
///   4. **Enrollment response** — peer replies with `EnrollResponse`:
///      `{ accepted: bool, assigned_id: u64, cluster_id: u64 }`.
///
/// On acceptance the new node is added to the cluster's routing table and
/// begins receiving gossip.
///
/// ## Merge-split semantics (docs/specs/14-distributed.md §9)
///
/// - **Merge**: two independent clusters with the same PSK discover each other.
///   The smaller cluster (by machine count) is the "joiner". The joiner's
///   leases are re-negotiated from scratch; the larger cluster's leases prevail.
///   If counts are equal, the cluster with the lower `cluster_id` prevails.
///
/// - **Split**: a partition is detected when PEER_LOSS_MS elapses with no
///   beacon from a majority of peers. Each partition operates independently
///   (degrade mode from docs/specs/14-distributed.md §7). On reconnect,
///   whichever partition was active longer retains its leases.
///
/// P09 ships the data structures; the full protocol is wired in the
/// dispatch loop once P07 gate is green.
use ostd::syscall::sys_get_time;

// ── Wire types (32B request, 16B response) ────────────────────────────────────

/// Request sent by a joining node to an existing peer's broker.
#[derive(Clone, Copy, Debug)]
pub struct EnrollRequest {
    pub machine_id: u64,
    pub cluster_id: u64,
    pub boot_epoch: u64,
    pub capability_flags: u32, // bit 0 = rt_capable, bits 1+ reserved
    pub pad: [u8; 4],
}

impl EnrollRequest {
    pub const WIRE_LEN: usize = 32;

    pub fn encode(&self) -> [u8; Self::WIRE_LEN] {
        let mut w = [0u8; Self::WIRE_LEN];
        w[0..8].copy_from_slice(&self.machine_id.to_le_bytes());
        w[8..16].copy_from_slice(&self.cluster_id.to_le_bytes());
        w[16..24].copy_from_slice(&self.boot_epoch.to_le_bytes());
        w[24..28].copy_from_slice(&self.capability_flags.to_le_bytes());
        w
    }

    pub fn decode(w: &[u8; Self::WIRE_LEN]) -> Self {
        Self {
            machine_id: u64::from_le_bytes(w[0..8].try_into().unwrap()),
            cluster_id: u64::from_le_bytes(w[8..16].try_into().unwrap()),
            boot_epoch: u64::from_le_bytes(w[16..24].try_into().unwrap()),
            capability_flags: u32::from_le_bytes(w[24..28].try_into().unwrap()),
            pad: [0; 4],
        }
    }
}

/// Response sent by the accepting peer.
#[derive(Clone, Copy, Debug)]
pub struct EnrollResponse {
    /// True if the peer accepted this node into the cluster.
    pub accepted: bool,
    /// Cluster's canonical cluster_id (may differ if merge occurred).
    pub cluster_id: u64,
    /// Assigned machine index within the cluster (1-based; 0 = rejected).
    pub assigned_id: u32,
    pub pad: [u8; 3],
}

impl EnrollResponse {
    pub const WIRE_LEN: usize = 16;

    pub fn accept(cluster_id: u64, assigned_id: u32) -> Self {
        Self {
            accepted: true,
            cluster_id,
            assigned_id,
            pad: [0; 3],
        }
    }

    pub fn reject(cluster_id: u64) -> Self {
        Self {
            accepted: false,
            cluster_id,
            assigned_id: 0,
            pad: [0; 3],
        }
    }

    pub fn encode(&self) -> [u8; Self::WIRE_LEN] {
        let mut w = [0u8; Self::WIRE_LEN];
        w[0] = if self.accepted { 1 } else { 0 };
        w[1..9].copy_from_slice(&self.cluster_id.to_le_bytes());
        w[9..13].copy_from_slice(&self.assigned_id.to_le_bytes());
        w
    }

    pub fn decode(w: &[u8; Self::WIRE_LEN]) -> Self {
        Self {
            accepted: w[0] != 0,
            cluster_id: u64::from_le_bytes(w[1..9].try_into().unwrap()),
            assigned_id: u32::from_le_bytes(w[9..13].try_into().unwrap()),
            pad: [0; 3],
        }
    }
}

// ── EnrollmentState ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnrollmentStatus {
    /// Bootstrap: no peers discovered yet.
    Alone,
    /// Enrollment request sent, awaiting response.
    Pending { sent_at: u64 },
    /// Enrolled and active in a multi-node cluster.
    Active,
    /// Enrollment timed out; will retry on next beacon round.
    Failed,
}

pub struct EnrollmentState {
    pub status: EnrollmentStatus,
    pub my_machine_id: u64,
    pub my_cluster_id: u64,
    /// Counter: how many machines have successfully enrolled with us.
    pub peer_count: usize,
    /// Next assigned_id to hand out (1-based).
    next_id: u32,
}

impl EnrollmentState {
    const ENROLL_TIMEOUT_MS: u64 = 5_000;

    pub fn new(machine_id: u64, cluster_id: u64) -> Self {
        Self {
            status: EnrollmentStatus::Alone,
            my_machine_id: machine_id,
            my_cluster_id: cluster_id,
            peer_count: 0,
            next_id: 2, // 1 is reserved for the first-boot "primary" node
        }
    }

    /// Call after sending an EnrollRequest to a peer.
    pub fn on_request_sent(&mut self) {
        self.status = EnrollmentStatus::Pending {
            sent_at: sys_get_time(),
        };
    }

    /// Call when an EnrollResponse is received.
    ///
    /// Returns `true` if enrollment was accepted.
    pub fn on_response(&mut self, resp: &EnrollResponse) -> bool {
        if resp.accepted {
            // Accept: update canonical cluster_id in case of merge.
            self.my_cluster_id = resp.cluster_id;
            self.status = EnrollmentStatus::Active;
            true
        } else {
            self.status = EnrollmentStatus::Failed;
            false
        }
    }

    /// Produce an accept response for an inbound EnrollRequest.
    ///
    /// Validates: cluster_id must match (same PSK means same cluster).
    pub fn evaluate_request(&mut self, req: &EnrollRequest) -> EnrollResponse {
        if req.cluster_id != self.my_cluster_id {
            return EnrollResponse::reject(self.my_cluster_id);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.peer_count += 1;
        EnrollResponse::accept(self.my_cluster_id, id)
    }

    /// Tick: check for enrollment timeout. Returns `true` if we transitioned
    /// from Pending → Failed (caller should retry enrollment).
    pub fn tick(&mut self) -> bool {
        if let EnrollmentStatus::Pending { sent_at } = self.status {
            if sys_get_time().wrapping_sub(sent_at) > Self::ENROLL_TIMEOUT_MS {
                self.status = EnrollmentStatus::Failed;
                return true;
            }
        }
        false
    }

    /// Degrade to standalone mode (split detected / peer loss).
    pub fn enter_degrade(&mut self) {
        self.status = EnrollmentStatus::Alone;
        self.peer_count = 0;
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status, EnrollmentStatus::Active)
    }
}
