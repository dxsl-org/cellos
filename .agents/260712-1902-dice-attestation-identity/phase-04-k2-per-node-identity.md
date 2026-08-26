# Phase 04 — K2 per-node identity (first-boot machine-id)

## Context Links
- Plan: [plan.md](plan.md)
- Dossier Decision 3 (dossier:71-88): K2 = per-node secret, first-boot random → `/etc/cellos/machine-id`,
  no crypto novelty, cheap independent step.
- Derisk candidate: `.agents/260623-0907-net-broker-robot-swarm/phase-00-derisk-spikes.md:38-47` (S2 —
  machine_id provenance; no per-device-unique-ID syscall in tree; first-boot-random-to-disk is the path)
- Seam: `net-broker/transport.rs:53-74` (`ClusterKeySource` — add a K2 impl, zero call-site change)
- Config precedent: `net-broker/identity.rs:52-83` (`/etc/cellos/cluster.cfg` load via VfsClient)

## Overview
- **Priority**: P2
- **Status**: pending
- **Testability**: G1 / CI — uses VFS + `GetRandom=214`; no Silo, no hardware. **Independent of P00-P03;
  can be built in parallel.**
- Give each node a per-device unique secret by persisting a first-boot random value to
  `/etc/cellos/machine-id`, and add a `ClusterKeySource` variant that derives K2 from it.

## Key Insights
- **No per-device-unique-ID source exists in tree** (derisk S2 red-team finding, phase-00:40). First-
  boot-random-to-disk is the accepted G1/G2 path; DICE (K3) supersedes it for *attested* identity.
- **Zero crypto novelty** — this is provisioning: read `/etc/cellos/machine-id`; if absent, draw 32
  bytes from `sys_get_random` (`GetRandom=214`), persist via VFS, use it. K2 = `SHA256(machine-id)` or
  the raw 32 bytes as the per-node secret.
- **Deployment footgun (red-team #5, phase-00:42)**: two nodes flashed from one image must NOT share a
  machine-id. First-boot generation (not image-baked) is exactly what guarantees divergence — the file
  must be created *after* flashing, on first boot, from device entropy. State this as the invariant.
- Slots cleanly into the existing seam: `VfsFileKeySource` (K1) → add `MachineIdKeySource` (K2). The
  `BrokerIdentity` (`identity.rs:20-37`) already keys off a 32-byte value.

## Requirements
- Functional:
  - First-boot provisioning: read `/etc/cellos/machine-id`; if missing/short, generate 32 bytes via
    `GetRandom`, write via `VfsClient`, re-read to confirm.
  - `MachineIdKeySource: ClusterKeySource` returning the per-node K2 (`transport.rs:54`).
  - Entropy fail-closed: mirror `BrokerRng` discipline (derisk S1, phase-00:31) — refuse to proceed if
    `GetRandom` returns non-device/zero entropy.
- Non-functional: `#![forbid(unsafe_code)]`; idempotent (second boot reads the same id); `no_std`.

## Architecture
Boot: `net-broker Init` → provision machine-id (`GetRandom` → VFS write once) → `MachineIdKeySource::
load()` → 32-byte K2 → `BrokerIdentity`/Noise PSK slot. Fleet-shared K1 (`VfsFileKeySource`) can remain
for the cluster prologue; K2 supplies the per-node uniqueness the nonce-prefix/routing/replay-epoch
need (derisk S2, phase-00:42).

## Related Code Files
- **Create**: `cells/services/net-broker/src/machine_id.rs` (provision + `MachineIdKeySource`).
- **Modify**: `net-broker` Init path to provision + select the key source; possibly
  `net-broker/identity.rs` if K2 feeds `node_id`.
- **Reference**: `net-broker/transport.rs:53-74`, `net-broker/identity.rs:52-83`.
- **No Law 1** (uses existing `GetRandom=214`, VFS, no new syscall/ABI).

## Implementation Steps
1. Implement first-boot provisioning in `machine_id.rs`: read-or-generate-and-persist, entropy
   fail-closed, idempotent.
2. Add `MachineIdKeySource` implementing `ClusterKeySource`.
3. Wire net-broker Init to provision then select K2 (behind a config flag so K1-only still works).
4. Test: fresh FS → machine-id created + non-zero; reboot → same id (persistence); two separate FS
   images → different ids (uniqueness) — in the 3-arch net-broker CI where present.

## Todo List
- [ ] First-boot provisioning (read-or-generate-persist, entropy fail-closed)
- [ ] `MachineIdKeySource: ClusterKeySource`
- [ ] net-broker Init wired (config-gated)
- [ ] Persistence + uniqueness tests

## Success Criteria
- First boot on a fresh FS creates a non-zero `/etc/cellos/machine-id`; reboot reads the identical id.
- Two independently-booted images produce different machine-ids.
- net-broker starts with K2 selected and completes a handshake (existing net-broker suite green).

## Risk Assessment
- **Image-baked clone collision (High × High → mitigated)**: generation is first-boot from device
  entropy, never baked; document that the file must NOT be included in flash images.
- **Low-entropy `GetRandom` (Med × High)**: mitigation — fail-closed per `BrokerRng` discipline
  (derisk S1); do not persist a low-entropy id.
- **Persistence on read-only rootfs (Med)**: `/etc` must be writable (memory: RO-rootfs is a Tier-3b
  concern); confirm `/etc/cellos/` is on a writable backend before relying on persistence.

## Security Considerations
- machine-id is a per-node secret when used as a key seed — treat the file as sensitive (not
  world-readable); never transmit it plaintext (only derived public identity leaves the node).

## Next Steps
- K3 (P05) *attests* identity and supersedes K2 for join decisions; K2 remains the cheaper
  non-attested rung and the machine-id source K3's nonce/routing still consume.
