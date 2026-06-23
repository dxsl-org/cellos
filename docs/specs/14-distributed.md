# Distributed Cells — Lifecycle, Lease, and Split-Brain Spec

> **Canonical reference** for all phases that touch cluster lifecycle, lease semantics, peer-loss, and split-brain resolution. Every numeric constant in P05/P08/P09 MUST cite this doc — never invent local values.

## Scope

L.0 (net-broker foundation) + L.1 (2-node robot swarm). Two SAS machines federate into a Private cluster. "Merge = federation, not unification." Two SAS machines **never** become one address space. No shared memory across machines; all cross-machine communication is explicit (RemoteServiceProxy only).

## Monotonic Time Discipline

- **All lease/timeout math uses monotonic ms-since-boot** (`GetTime op=1`). Wall-clock (op=2/3, RTC) is diagnostics-only and MUST NOT appear in any timeout path.
- Monotonic clocks are **per-machine** — they do NOT agree across nodes. Leases are owner-relative: the holder tracks its own renewal deadline; the grantor tracks last-heard-from in its own clock. No cross-node clock comparison ever.
- `RecvTimeout=201` (deadline-recv) is the mechanism for "lose peers → degrade".

## Lifecycle States

```
                    [EnrollRequest]
Isolated ──────────────────────────► Discovering
                                           │
                    [BeaconAck + Noise HShake]
                                           │
                                           ▼
                          ┌─── Member(Secondary) ◄── [lower machine_id wins]
                          │
                          └─── Member(Primary) ──► [lower machine_id]
                                           │
                    [PEER_LOSS_MS silence or partition]
                                           │
                                           ▼
                                       Isolated
```

| Transition | Trigger | Timer |
|---|---|---|
| Isolated → Discovering | `EnrollRequest` sent after beacon heard | — |
| Discovering → Member | Noise KKpsk0 handshake complete | Enrollment timeout: 5000ms |
| Member → Isolated | Peer beacons missing > `PEER_LOSS_MS` | Monotonic, grantor-side |
| Isolated → Discovering | `REJOIN_COOLDOWN_MS` elapsed after degrade | Monotonic |

## Constants (all monotonic ms)

| Constant | Value | Meaning |
|---|---|---|
| `LEASE_TTL_MS` | 3000 | Holder must renew within this window |
| `LEASE_RENEW_MS` | 1000 | Renew at TTL/3 |
| `LEASE_MISS_THRESHOLD` | 3 | N consecutive misses → degrade |
| `PEER_LOSS_MS` | 9000 | `LEASE_TTL_MS × LEASE_MISS_THRESHOLD` |
| `BEACON_INTERVAL_MS` | 1000 | How often each broker multicasts a beacon |
| `REJOIN_COOLDOWN_MS` | 3000 | Anti-flap: wait after degrade before re-enrolling |
| `ENROLLMENT_TIMEOUT_MS` | 5000 | Max time for a Noise handshake to complete |

> **Justification:** local IPC ~2µs; cross-machine ~500µs–5ms. `PEER_LOSS_MS=9s` >> RTT, so no false positives from transient network hiccup. `BEACON_INTERVAL_MS=1s` << `PEER_LOSS_MS` (9 beacons before degrade). RT watchdog heartbeat is 500ms (net cell pattern) << all of these.

## Lease Model

```
Lease = (resource_id: u64, holder_machine_id: u64, granted_at_mono: u64, ttl_ms: u64)
```

- **Holder** tracks: must call `LeaseRenew` before `granted_at_mono + LEASE_TTL_MS` (in holder's clock).
- **Grantor** tracks: expires lease independently if no renewal heard for `PEER_LOSS_MS` (in grantor's clock).
- Leases are identified by `resource_id` — a u64 FNV-1a hash of a resource name string.
- A lease is an **optimistic coordination hint**, NOT mutual-exclusion (see Physical Safety below).

## Peer-Loss / Degrade Rule

When a peer's beacon is missing for `> PEER_LOSS_MS`:
1. Broker declares peer lost.
2. Release all leases held *from* that peer.
3. Drop all shared tasks claimed *via* that peer.
4. Return local node to `Isolated`.
5. Degrade is **idempotent**: running the degrade sweep twice is safe.
6. Degrade is **non-cascading**: dropping peer-sourced leases/tasks must NOT affect local-only Cells. The broker iterates only the peer-scoped slice of its tables.

## ⚠️ PHYSICAL-SAFETY HARD INVARIANT (non-negotiable)

> The cluster lease is an **OPTIMISTIC coordination HINT, NOT a hard mutual-exclusion guarantee.**

In a coordinator-less 2-node partition, **both** nodes can self-grant the same role for up to `PEER_LOSS_MS` before either degrades. For a robot swarm this is a **double-execution safety hazard** — two robots could drive the same actuator simultaneously.

**MANDATE** (review gate for P08/P09): Any shared task that drives a PHYSICAL actuator MUST gate every actuation on a **local safety interlock independent of the cluster lease**. Before actuating, the local Cell MUST:

1. Re-confirm exclusive **local** ownership of the actuator (local resource lock, not the cluster lease).
2. Confirm the actuator's own safety state permits the action.
3. If a partition is **suspected** (N consecutive missed beacons, even before the full `PEER_LOSS_MS` degrade), default to **SAFE/STOP** — never "continue".

The lease decides *who SHOULD* own a role for coordination/efficiency. It **NEVER** by itself authorizes physical motion. Physical safety is a local, fail-safe (default-STOP) property that holds with **zero** cluster connectivity.

The bounded `PEER_LOSS_MS` window is acceptable ONLY because the local interlock catches the overlap.

## Split-Brain Resolution

When a partition heals and both nodes believe they are Primary:

1. **Tiebreak:** lower `machine_id` wins Primary; the loser re-enrolls as Secondary.
2. **Re-negotiate from scratch:** the loser does NOT retain stale lease claims — all leases are re-requested after re-enrollment. No automatic state merge.
3. **Anti-flap:** a node that just degraded waits `REJOIN_COOLDOWN_MS` before re-enrolling.
4. Re-enrolling node MUST complete the Noise KKpsk0 handshake again — no trust carried across a partition boundary.

## Beacon Anti-Replay Window

Each beacon carries `(boot_epoch: u64, mono_counter: u64)` authenticated by the AEAD. The `boot_epoch` is the sender's monotonic time at boot (sourced from `GetTime op=1` at Init — unique per boot since the clock resets).

Receiver rules per `machine_id`:
- **Same epoch, non-increasing counter** → replay, drop silently.
- **Higher epoch** → re-baseline: accept as a fresh sender (reboot detected).
- **epoch=0 fallback**: if RTC epoch is 0 (no battery / no clock), rely purely on `(boot_epoch, counter)` monotonicity + AEAD freshness. Never use wall-clock in the acceptance window.
- AEAD gives **integrity + authenticity**; the `(epoch, counter)` pair gives **freshness**. Both are required.

> A replayed beacon from a prior boot is detectable by `boot_epoch` mismatch. A replayed beacon from the current boot is detectable by non-increasing `mono_counter`. A forged beacon fails AEAD. A stale-but-authentic beacon from the current boot that was delayed is accepted if the counter advances (liveness, not strict real-time ordering).

## Broker Timer Table

| Timer name | Source | Period | Action on expiry |
|---|---|---|---|
| `beacon_send` | Monotonic | `BEACON_INTERVAL_MS` | Multicast XChaCha beacon |
| `peer_loss[machine_id]` | Monotonic | Set to `last_heard_mono + PEER_LOSS_MS` | Declare peer lost → degrade |
| `lease_renew[resource_id]` | Monotonic | `granted_at_mono + LEASE_RENEW_MS` | Send `LeaseRenew` |
| `lease_expire[resource_id]` | Monotonic | `granted_at_mono + LEASE_TTL_MS` | If no renewal heard → release lease |
| `rejoin_cooldown` | Monotonic | `degrade_at + REJOIN_COOLDOWN_MS` | Allow re-enrollment |
| `enrollment_timeout` | Monotonic | Per-attempt, `ENROLLMENT_TIMEOUT_MS` | Abort and retry enrollment |

All timer deadlines are absolute monotonic values. No wall-clock timer is permitted.

## Invariants Summary (for code review)

1. No cross-node clock comparison (`peer_mono` NEVER compared to `local_mono`).
2. Degrade touches ONLY peer-scoped lease/task tables; local Cells unaffected.
3. Split-brain loser re-authenticates via Noise on re-enroll — no stale trust.
4. Physical actuation requires local interlock, not just a lease.
5. A replayed claim MUST NOT directly actuate — local interlock is the backstop.
6. All numeric constants trace to this doc. No phase invents local timeout values.
