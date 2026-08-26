# D25 — Bind `machine_id` to authenticated NodeId

**Status:** approved/applied 2026-08-01. No code or ABI changed.

## Finding

Spec 14 makes lower `machine_id` the Primary tiebreak but never defines its trust root
(`docs/specs/14-distributed.md:24-27,92-99`). `EnrollRequest::decode` accepts the peer's
wire value directly (`cells/services/net-broker/src/enrollment.rs:45-75`), while Noise
authenticates the 32-byte NodeId, not that independent u64.

A malicious peer can therefore advertise a smaller value and win elections if this code
is wired unchanged. D16 matters here: enrollment is still dormant behind `main::dispatch`
TODOs, so this is a latent release-blocking vulnerability rather than an exploitable
shipped two-node runtime hole.

## Recommended ruling [FINAL]

**Approve recommendation A: amend Spec 14 now, independently of Spec 20.**

Normative invariant:

1. Election/lease/routing `machine_id` is derived locally from the Noise-authenticated
   NodeId; a peer-supplied value is never authoritative.
2. Pin one versioned derivation before implementation, recommended as
   `u64::from_le_bytes(SHA256("cellos-machine-id-v1" || NodeId)[0..8])`.
3. If the wire field remains for diagnostics/compatibility, ingress must reject a mismatch
   with the locally derived value. Prefer removing the redundant field in the later D24
   Law-1 package.
4. Add negative tests for a forged lower value, plus stability and collision-handling
   tests, before enrollment is wired into `dispatch`.

The docs invariant is authorized by D25; changing the enrollment wire layout or public API
still requires the separate Law-1 process in D24.
