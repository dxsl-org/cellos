// reason: this module implements the P08 task-claiming gossip protocol for the
// net-broker robot-swarm feature. `main.rs` only declares `mod gossip;` (main.rs:64)
// and never references `gossip::` — the dispatch loop's lease-renewal/peer-loss
// tick is still a TODO (main.rs:136). Not wired yet.
#![allow(dead_code)]

/// P08 — Task-claiming gossip over the encrypted cluster channel.
///
/// ## Protocol (binary, all fields LE)
///
/// A GossipMessage is serialized to a fixed 32-byte wire frame:
///   [0]     type: 0x01 = TaskClaim, 0x02 = TaskRelease
///   [1]     pad
///   [2..10] task_id   (u64 LE)
///   [10..18] machine_id (u64 LE, claimer)
///   [18..26] epoch    (u64 LE, anti-replay: increment per claim/release)
///   [26..34] mono_ts  (u64 LE, sender's sys_get_time at send time)
///   [34..40] pad[6]
///
/// Wire frame total: 40 bytes — fits in one Noise transport record (max payload
/// ≈ 4080 B).  Gossip messages are sent via the established Noise sessions
/// (ConnectionPool) once P04 transport is fully wired.  Until then, messages
/// are queued in `PendingGossip` and drained when sessions become available.
///
/// ## Safety invariant (from docs/specs/14-distributed.md §6)
///
/// A TaskClaim lease is an **optimistic hint** only.  Physical actuation MUST
/// gate on a local hardware safety interlock, NOT solely on holding a lease.
/// A claim reaching 100% of peers does NOT prove that a physical resource is
/// exclusive — only that the cluster agrees on *intent*.
use ostd::syscall::sys_get_time;

const TYPE_TASK_CLAIM: u8 = 0x01;
const TYPE_TASK_RELEASE: u8 = 0x02;

const WIRE_LEN: usize = 40;

// ── GossipMessage ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GossipType {
    TaskClaim,
    TaskRelease,
}

/// A single gossip message (claim or release of a task_id by a machine).
#[derive(Clone, Copy, Debug)]
pub struct GossipMessage {
    pub kind: GossipType,
    pub task_id: u64,
    pub machine_id: u64,
    pub epoch: u64,
    pub mono_ts: u64,
}

impl GossipMessage {
    pub fn claim(task_id: u64, machine_id: u64, epoch: u64) -> Self {
        Self {
            kind: GossipType::TaskClaim,
            task_id,
            machine_id,
            epoch,
            mono_ts: sys_get_time(),
        }
    }

    pub fn release(task_id: u64, machine_id: u64, epoch: u64) -> Self {
        Self {
            kind: GossipType::TaskRelease,
            task_id,
            machine_id,
            epoch,
            mono_ts: sys_get_time(),
        }
    }

    pub fn encode(&self) -> [u8; WIRE_LEN] {
        let mut w = [0u8; WIRE_LEN];
        w[0] = match self.kind {
            GossipType::TaskClaim => TYPE_TASK_CLAIM,
            GossipType::TaskRelease => TYPE_TASK_RELEASE,
        };
        w[2..10].copy_from_slice(&self.task_id.to_le_bytes());
        w[10..18].copy_from_slice(&self.machine_id.to_le_bytes());
        w[18..26].copy_from_slice(&self.epoch.to_le_bytes());
        w[26..34].copy_from_slice(&self.mono_ts.to_le_bytes());
        w
    }

    pub fn decode(w: &[u8; WIRE_LEN]) -> Option<Self> {
        let kind = match w[0] {
            TYPE_TASK_CLAIM => GossipType::TaskClaim,
            TYPE_TASK_RELEASE => GossipType::TaskRelease,
            _ => return None,
        };
        Some(Self {
            kind,
            task_id: u64::from_le_bytes(w[2..10].try_into().ok()?),
            machine_id: u64::from_le_bytes(w[10..18].try_into().ok()?),
            epoch: u64::from_le_bytes(w[18..26].try_into().ok()?),
            mono_ts: u64::from_le_bytes(w[26..34].try_into().ok()?),
        })
    }
}

// ── ClaimRecord ───────────────────────────────────────────────────────────────

/// One active (or recently seen) task claim.
#[derive(Clone, Copy)]
pub struct ClaimRecord {
    pub task_id: u64,
    pub machine_id: u64,
    /// Monotonically increasing per (task_id, machine_id) pair — anti-replay.
    pub epoch: u64,
    pub last_heard: u64,
}

// ── GossipTable ───────────────────────────────────────────────────────────────

const MAX_CLAIMS: usize = 32;

/// Per-broker view of all active task claims in the cluster.
///
/// A claim is "active" until:
///   (a) a `TaskRelease` with epoch ≥ claim.epoch is received from the same
///       machine for the same task_id, OR
///   (b) the sender's peer entry times out (peer loss → all its claims evicted).
pub struct GossipTable {
    claims: [Option<ClaimRecord>; MAX_CLAIMS],
    epoch_counter: u64,
    my_machine_id: u64,
}

impl GossipTable {
    pub const fn new(my_machine_id: u64) -> Self {
        Self {
            claims: [const { None }; MAX_CLAIMS],
            epoch_counter: 0,
            my_machine_id,
        }
    }

    /// Apply an incoming gossip message. Returns `true` if state changed.
    pub fn apply(&mut self, msg: &GossipMessage) -> bool {
        let now = sys_get_time();
        match msg.kind {
            GossipType::TaskClaim => {
                // Upsert: accept if epoch is newer or no existing claim for this task.
                for r in self.claims.iter_mut().flatten() {
                    if r.task_id == msg.task_id {
                        if msg.epoch > r.epoch {
                            *r = ClaimRecord {
                                task_id: msg.task_id,
                                machine_id: msg.machine_id,
                                epoch: msg.epoch,
                                last_heard: now,
                            };
                            return true;
                        }
                        return false; // older epoch — ignore
                    }
                }
                // No existing claim for this task_id — insert in first free slot.
                for slot in self.claims.iter_mut() {
                    if slot.is_none() {
                        *slot = Some(ClaimRecord {
                            task_id: msg.task_id,
                            machine_id: msg.machine_id,
                            epoch: msg.epoch,
                            last_heard: now,
                        });
                        return true;
                    }
                }
                false // table full
            }
            GossipType::TaskRelease => {
                for slot in self.claims.iter_mut() {
                    if let Some(r) = slot {
                        if r.task_id == msg.task_id
                            && r.machine_id == msg.machine_id
                            && msg.epoch >= r.epoch
                        {
                            *slot = None;
                            return true;
                        }
                    }
                }
                false
            }
        }
    }

    /// Evict all claims belonging to `machine_id` (peer timed out).
    pub fn evict_peer(&mut self, machine_id: u64) {
        for slot in self.claims.iter_mut() {
            if slot.map(|r| r.machine_id == machine_id).unwrap_or(false) {
                *slot = None;
            }
        }
    }

    /// Construct a TaskClaim for this node and record it locally.
    ///
    /// Returns `None` if another machine already holds the claim.
    pub fn try_claim(&mut self, task_id: u64) -> Option<GossipMessage> {
        // Check if already claimed by another.
        for rec in self.claims.iter().flatten() {
            if rec.task_id == task_id && rec.machine_id != self.my_machine_id {
                return None;
            }
        }
        self.epoch_counter += 1;
        let msg = GossipMessage::claim(task_id, self.my_machine_id, self.epoch_counter);
        self.apply(&msg);
        Some(msg)
    }

    /// Construct a TaskRelease for this node and remove the claim locally.
    pub fn release(&mut self, task_id: u64) -> Option<GossipMessage> {
        let epoch = self
            .claims
            .iter()
            .flatten()
            .find(|r| r.task_id == task_id && r.machine_id == self.my_machine_id)
            .map(|r| r.epoch)?;
        let msg = GossipMessage::release(task_id, self.my_machine_id, epoch);
        self.apply(&msg);
        Some(msg)
    }

    /// Does this node currently hold the claim for `task_id`?
    pub fn is_claimed_by_me(&self, task_id: u64) -> bool {
        self.claims
            .iter()
            .flatten()
            .any(|r| r.task_id == task_id && r.machine_id == self.my_machine_id)
    }

    pub fn active_count(&self) -> usize {
        self.claims.iter().filter(|s| s.is_some()).count()
    }
}

// ── PendingGossip ─────────────────────────────────────────────────────────────

const MAX_PENDING: usize = 8;

/// Outbound gossip queue — messages generated locally that must be broadcast
/// to all peers via Noise sessions once ConnectionPool has active sessions.
pub struct PendingGossip {
    msgs: [Option<GossipMessage>; MAX_PENDING],
}

impl PendingGossip {
    pub const fn new() -> Self {
        Self {
            msgs: [const { None }; MAX_PENDING],
        }
    }

    pub fn push(&mut self, msg: GossipMessage) {
        for slot in self.msgs.iter_mut() {
            if slot.is_none() {
                *slot = Some(msg);
                return;
            }
        }
        // Queue full — overwrite oldest (slot 0).
        self.msgs[0] = Some(msg);
    }

    pub fn pop(&mut self) -> Option<GossipMessage> {
        for slot in self.msgs.iter_mut() {
            if slot.is_some() {
                return slot.take();
            }
        }
        None
    }

    pub fn is_empty(&self) -> bool {
        self.msgs.iter().all(|s| s.is_none())
    }
}
