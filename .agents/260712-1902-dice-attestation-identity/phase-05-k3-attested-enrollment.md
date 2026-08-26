# Phase 05 — K3 attested enrollment (DICE token in the ticket)

## Context Links
- Plan: [plan.md](plan.md)
- Dossier Decision 3 (dossier:81-88): K3 binds the node's **attested measurement**; `CDI_final` signed
  into the enrollment ticket → node proves *what it runs*, not just *that it knows a key*. "K3's payload
  IS the DICE token" — plan DICE + K3 together.
- Enrollment ticket type: `api::cluster::PeerTicket` (`net-broker/identity.rs:12,119-131`)
- Handshake binding: `net-broker/transport.rs:139-159` (prologue = cluster_id ‖ local ‖ remote node_id)
- Depends on: P00 (token lib), P01 (aggregate syscall), P02 (Silo/root + Alias key), P04 (machine-id)

## Overview
- **Priority**: P1 (the convergence point of DICE + node identity)
- **Status**: pending
- **Testability**: G2 / hardware-informed — real attestation needs the Silo-held Alias key (ARM64/x86).
  A **software-signer variant** proves the wire path + verifier logic in CI.
- **🔶 HARDWARE-INFORMED**: the enrollment provisioning + fleet-verifier trust anchor are deployment
  decisions — specify the token binding + verify logic now; leave provisioning ceremony to hardware.
- Embed the P00/P02 attestation token (binding the P01 aggregate + the Alias pubkey + machine-id) into
  the swarm enrollment ticket, and verify it on the joining side before admitting a peer.

## Key Insights
- **K3 = DICE token as enrollment payload.** The producer flow from P02 yields `AttestToken{ node_id,
  aggregate, alias_pub, nonce, sig }`. K3 attaches this to enrollment; the receiver runs P00
  `parse_and_verify` against the fleet trust anchor + checks the aggregate against an expected/allowed
  set (measured-boot policy) before completing the Noise handshake.
- **node_id becomes the Alias key.** Today `CellNetId` = X25519 static pub (`identity.rs:29-31`). Under
  K3 the *attested* identity is the P-256 Alias pubkey. Decide the binding: either (a) bind the X25519
  static key into the token body (proving the transport key belongs to the attested node), or (b)
  migrate `node_id` to the P-256 Alias key. **(a) is lower-churn** — keeps the Noise KKpsk0 transport
  (`transport.rs:97`) unchanged and only *adds* an attestation proof over the existing static key.
- The **nonce** in the token defeats replay of a stale attestation — the verifier issues a challenge
  nonce at enrollment; the token must bind it (freshness). This is why P00's body carries `nonce(16)`.
- Reuses the prologue-binding discipline already in place (`transport.rs:120-124` — identity spoofing
  defense); K3 extends it from "knows the key" to "is the attested node".

## Requirements
- Functional:
  - Producer: at enrollment, read aggregate (P01), obtain challenge nonce, build+sign token (P02 via
    KMS/Silo), attach to the enrollment/`PeerTicket` exchange.
  - Verifier: `parse_and_verify` (P00) against fleet anchor → aggregate ∈ allowed set → nonce matches
    the challenge → static key in token == handshake static key → THEN admit. Fail-closed at each step.
  - Measured-boot policy: an allowed-aggregate list (or a signed reference-value blob, VPOL-shaped).
- Non-functional: fail-closed like `policy.rs` (an unverifiable/mismatched token → reject join, never
  panic); software-signer variant keeps CI green.

## Architecture
Join flow: `joiner` reads aggregate (P01) → requests challenge nonce from `verifier` → KMS/Silo signs
token body(node_id ‖ aggregate ‖ alias_pub ‖ static_pub ‖ nonce) → sends token in enrollment → verifier
`parse_and_verify` + aggregate-policy + nonce + static-key match → on success, proceed to KKpsk0
handshake (`transport.rs:172`). On any failure: reject, audit, no session.

## Related Code Files
- **Modify**: `net-broker` enrollment path (attach/verify token); possibly extend `PeerTicket`
  (`api::cluster`) with an attestation field — **check if this touches `libs/api` (Law 1)**: if
  `PeerTicket` lives in `libs/api`, adding a field is ABI-additive → flag to user.
- **Create**: `net-broker/src/attest_enroll.rs` (producer + verifier glue), allowed-aggregate policy.
- **Reference**: `libs/attestation` (P00), `libs/ostd/src/silo.rs` / KMS (P02/P03), `net-broker/
  transport.rs:120-159`, `net-broker/identity.rs:119-131`.

## Implementation Steps
1. Decide the node_id binding (recommend option (a): bind X25519 static key into token body).
2. Extend enrollment with a challenge-nonce round-trip (freshness).
3. Producer: assemble + sign the token (KMS/Silo path from P02/P03; software signer in CI).
4. Verifier: full fail-closed chain (verify → aggregate-policy → nonce → static-key match).
5. Allowed-aggregate policy source (start with a compiled dev list; VPOL-shaped signed blob later).
6. Test: matched aggregate + fresh nonce → join succeeds; wrong aggregate / stale nonce / tampered
   token → join rejected (negative tests). Software-signer variant in CI; Silo variant on QEMU ARM64.

## Todo List
- [ ] node_id binding decision recorded (option a/b)
- [ ] Challenge-nonce round-trip in enrollment
- [ ] Producer builds+signs token (KMS/Silo + software CI variant)
- [ ] Verifier fail-closed chain (verify → aggregate → nonce → static-key)
- [ ] Allowed-aggregate policy
- [ ] Positive + 3 negative enrollment tests

## Success Criteria
- A node with an allowed aggregate + fresh nonce completes enrollment and the KKpsk0 handshake.
- Wrong aggregate, stale/replayed nonce, tampered token, or static-key mismatch each reject the join
  (four negative tests) with an audit event, no panic.
- CI green on the software-signer variant across arches where net-broker builds.

## Risk Assessment
- **Aggregate instability across benign updates (High × Med)**: any cell-set change alters the
  aggregate → legitimate nodes rejected. Mitigation — the allowed-aggregate policy is a *set* (multiple
  approved boot configs), updatable via signed reference values; document the update ceremony.
- **Replay of stale attestation (High × High → mitigated)**: challenge-nonce freshness binding; verify
  nonce == issued challenge.
- **Trust-anchor bootstrap (Med × High)**: who signs the fleet reference values / whose pubkey verifies
  the token? Reuse the `FLEET_ROOT_PUBKEY` discipline (`policy.rs:45-54`, dev vs prod). Hardware-informed.
- **PeerTicket ABI touch (Med)**: if it's in `libs/api`, flag the additive field to user (Law 1-adjacent).

## Security Considerations
- Attestation proves *measured boot state*, not runtime integrity — document the limitation (a node
  measured-clean at boot can still be exploited later). Pairs with the LBI/rustc TCB model, not a
  replacement for it.
- Fail-closed everywhere; an unverifiable token must never degrade to "admit anyway".

## Next Steps
- P06 layers a COSE/EAT encoding of this same token IF/WHEN a third-party (Veraison) verifier is wired.
