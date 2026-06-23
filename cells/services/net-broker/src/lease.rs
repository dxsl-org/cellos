/// P08 — Lease lifecycle (TTL/renew/expire constants from docs/specs/14-distributed.md).
///
/// ## Lease semantics
///
/// A **Lease** is a time-bounded claim on a shared resource (task_id or
/// physical actuator). Leases are:
///
///   - Owned by the machine that claimed the task via `gossip::GossipTable`.
///   - Valid for TTL milliseconds from the last renewal.
///   - Renewed every RENEW_MS by the holder calling `renew()`.
///   - Expired if the holder has not renewed within TTL_MS from the last renewal.
///   - Forcibly evicted when the holder's beacon times out (peer_loss = 9000 ms).
///
/// ## Safety invariant (docs/specs/14-distributed.md §6)
///
/// A non-expired lease does NOT prove exclusive physical access — it is an
/// **optimistic distributed hint**.  Physical actuators MUST have an independent
/// hardware interlock (e-stop, mutex in the actuator driver, etc.) that operates
/// regardless of lease state.

use ostd::syscall::sys_get_time;

/// How long (ms) a lease remains valid after the last renewal.
pub const TTL_MS: u64 = 3_000;

/// How often (ms) the holder must renew to keep the lease live.
pub const RENEW_MS: u64 = 1_000;

/// After this many ms of peer-beacon silence the peer is declared lost and
/// all its leases are expired.
pub const PEER_LOSS_MS: u64 = 9_000;

// ── Lease ─────────────────────────────────────────────────────────────────────

/// One active lease entry.
#[derive(Clone, Copy, Debug)]
pub struct Lease {
    pub task_id:    u64,
    pub machine_id: u64,
    /// Monotonic timestamp of the last renewal (from `sys_get_time()`).
    pub renewed_at: u64,
}

impl Lease {
    pub fn new(task_id: u64, machine_id: u64) -> Self {
        Self { task_id, machine_id, renewed_at: sys_get_time() }
    }

    /// Extend the lease TTL from now.
    pub fn renew(&mut self) {
        self.renewed_at = sys_get_time();
    }

    /// Is this lease currently valid (not expired)?
    pub fn is_valid(&self) -> bool {
        sys_get_time().wrapping_sub(self.renewed_at) < TTL_MS
    }

    /// Time until expiry in ms; 0 if already expired.
    pub fn remaining_ms(&self) -> u64 {
        let age = sys_get_time().wrapping_sub(self.renewed_at);
        TTL_MS.saturating_sub(age)
    }
}

// ── LeaseTable ─────────────────────────────────────────────────────────────────

const MAX_LEASES: usize = 32;

/// Cluster-wide lease registry maintained by the broker.
///
/// Each broker tracks leases for all cluster nodes. When a TaskClaim gossip
/// message is applied, `grant()` is called. When a TaskRelease is applied,
/// `revoke()` is called. Expiry is checked via `sweep_expired()` which should
/// be called on each broker dispatch-loop tick.
pub struct LeaseTable {
    leases: [Option<Lease>; MAX_LEASES],
    my_machine_id: u64,
}

impl LeaseTable {
    pub const fn new(my_machine_id: u64) -> Self {
        Self { leases: [const { None }; MAX_LEASES], my_machine_id }
    }

    /// Grant a new lease for `(task_id, machine_id)`, or renew an existing one.
    pub fn grant(&mut self, task_id: u64, machine_id: u64) {
        // Renew existing.
        for slot in self.leases.iter_mut().flatten() {
            if slot.task_id == task_id && slot.machine_id == machine_id {
                slot.renew();
                return;
            }
        }
        // Insert new.
        for slot in self.leases.iter_mut() {
            if slot.is_none() {
                *slot = Some(Lease::new(task_id, machine_id));
                return;
            }
        }
        // Table full — evict the oldest expired lease to make room.
        if let Some(slot) = self.leases.iter_mut().find(|s| s.map(|l| !l.is_valid()).unwrap_or(false)) {
            *slot = Some(Lease::new(task_id, machine_id));
        }
    }

    /// Renew the lease held by this broker's machine for `task_id`.
    ///
    /// Must be called every RENEW_MS by the local task manager while the task
    /// is still running. Returns `false` if no lease is found (it expired).
    pub fn renew_local(&mut self, task_id: u64) -> bool {
        for slot in self.leases.iter_mut().flatten() {
            if slot.task_id == task_id && slot.machine_id == self.my_machine_id {
                slot.renew();
                return true;
            }
        }
        false
    }

    /// Revoke a lease (called on TaskRelease gossip).
    pub fn revoke(&mut self, task_id: u64, machine_id: u64) {
        for slot in self.leases.iter_mut() {
            if slot.map(|l| l.task_id == task_id && l.machine_id == machine_id).unwrap_or(false) {
                *slot = None;
                return;
            }
        }
    }

    /// Evict all leases for a peer that has timed out (beacon lost > PEER_LOSS_MS).
    pub fn evict_peer(&mut self, machine_id: u64) {
        for slot in self.leases.iter_mut() {
            if slot.map(|l| l.machine_id == machine_id).unwrap_or(false) {
                *slot = None;
            }
        }
    }

    /// Remove all expired leases. Returns the count of leases swept.
    ///
    /// Call once per dispatch-loop tick (every ~500 ms heartbeat interval).
    pub fn sweep_expired(&mut self) -> usize {
        let mut swept = 0;
        for slot in self.leases.iter_mut() {
            if slot.map(|l| !l.is_valid()).unwrap_or(false) {
                *slot = None;
                swept += 1;
            }
        }
        swept
    }

    /// Does any node currently hold a valid lease for `task_id`?
    pub fn is_held(&self, task_id: u64) -> bool {
        self.leases.iter().flatten()
            .any(|l| l.task_id == task_id && l.is_valid())
    }

    /// Which machine holds the lease for `task_id` (if any)?
    pub fn holder(&self, task_id: u64) -> Option<u64> {
        self.leases.iter().flatten()
            .find(|l| l.task_id == task_id && l.is_valid())
            .map(|l| l.machine_id)
    }

    pub fn active_count(&self) -> usize {
        self.leases.iter().filter(|s| s.map(|l| l.is_valid()).unwrap_or(false)).count()
    }

    /// Lease the broker should renew for `task_id` if it owns it and expiry
    /// is within 2×RENEW_MS — returns `true` if a renewal was triggered.
    pub fn maybe_renew_local(&mut self, task_id: u64) -> bool {
        for slot in self.leases.iter_mut().flatten() {
            if slot.task_id == task_id && slot.machine_id == self.my_machine_id
                && slot.remaining_ms() < 2 * RENEW_MS
            {
                slot.renew();
                return true;
            }
        }
        false
    }
}
