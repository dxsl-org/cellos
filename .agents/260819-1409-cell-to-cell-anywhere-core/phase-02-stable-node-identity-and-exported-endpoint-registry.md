---
title: "Phase 02 - Stable Node Identity and Exported Endpoint Registry"
status: in_progress
priority: P1
effort: 4
depends_on: [01]
owner: "identity-and-registry"
---

# Phase 02 - Stable Node Identity and Exported Endpoint Registry

## Context Links

- Research: `research/research-audit.md`
- Semantics: `research/semantics-report.md`
- Assumptions: `reports/assumptions.md`

## Overview

Priority P1. Integrate the existing opaque KMS-owned X25519 node identity and
retain explicit, fail-closed service exports before remote calls. Plaintext VFS
`machine-id` state is not an identity root.

## Key Insights

- `kms_dh.rs` is the authoritative static-DH seam: the broker retains only a
  handle, binding epoch, and public key; KMS performs static DH.
- Broker startup now registers with KMS, validates matching
  register/status/acquire snapshots, and constructs the opaque Clatter key only
  when readiness, provider, revision, epoch, and public key all agree.
- KMS absence or any mixed snapshot selects an ephemeral local-only identity;
  remote remains disabled without insecure fallback.
- The export registry remains read-only, versioned, bounded, and fail-closed.
  A valid registry still does not enable remote transport.

## Requirements

- Functional: opaque KMS acquisition, stable public `CellNetId` when the
  protected provider is ready, clone/policy mismatch rejection by KMS, export
  registry versioning, retry class per export, and remote disabled until every
  transport and governance gate opens.
- Non-functional: no private scalar in broker or VFS, no plaintext identity
  persistence, bounded fixed-frame KMS IPC, and no Law 1 change in this slice.

## Entry Decisions

- The Phase 02B secure-node-identity contract and existing KMS ABI own the key
  lifecycle; DICE P04 plaintext `machine-id` is retired as a C2C identity root.
- `/etc/cellos/c2c-exports.cfg` remains init/supervisor-provisioned and
  broker-read-only with fail-closed absence/invalid handling.
- Public and remote operation remain disabled even after opaque identity
  acquisition until relay/transport entry gates are separately satisfied.

## Architecture

`live broker registration → KMS register/status/acquire snapshot validation →
OpaqueStaticKey(handle, binding_epoch, public_key) → KmsBackedX25519 static DH`.
KMS-unavailable or inconsistent state yields an ephemeral local-only key.
Separately, the broker loads the export allowlist, but no current path promotes
it into enabled remote routing.

## Related Code Files

- `cells/services/net-broker/src/main.rs`
- `cells/services/net-broker/src/kms_dh.rs`
- `cells/services/net-broker/src/transport.rs`
- `cells/services/net-broker/src/export_registry.rs`
- `libs/ostd/src/clients/kms.rs`
- `cells/services/kms/src/dispatch.rs`

## Implementation Steps

1. Bind the live broker instance through the fixed KMS ABI.
2. Read status and acquisition snapshots, then validate every authority field.
3. Adapt Clatter static DH to the opaque KMS handle without exporting the scalar.
4. Preserve local-only ephemeral startup when KMS is absent or non-ready.
5. Keep export registry parsing bounded and remote routing disabled.
6. Qualify a clone-resistant provider and recovery flow in the separately
   governed provider lane before any remote enablement.

## Todo List

- [x] Retire plaintext VFS `machine-id` as the C2C identity root.
- [x] Wire broker registration and opaque KMS identity acquisition.
- [x] Require exact register/status/acquire snapshot agreement.
- [x] Keep invalid/absent export registries fail closed.
- [x] Preserve local-only ephemeral fallback with remote disabled.
- [ ] Provision a qualified clone-resistant KMS provider and retained evidence.
- [ ] Define operator lost-key/clone recovery under supervisor authority.
- [ ] Exercise an approved two-node relay oracle before enabling remote.

## Success Criteria

- A ready KMS snapshot produces the stable public `CellNetId` without exporting
  private scalar bytes.
- KMS absence, stale epochs, mixed revisions/providers/public keys, or
  non-ready state leaves remote disabled.
- No service is remotely callable merely because it appears in the export
  registry.
- Phase 03 may use the local broker runtime, but remote phases remain gated on
  qualified provider and relay-entry evidence.

## Risk Assessment

- Risk: a mixed KMS snapshot enables stale authority. Mitigation: exact
  register/status/acquire agreement and binding-epoch checks.
- Risk: ephemeral fallback is mistaken for remote identity. Mitigation:
  `has_secure_identity=false`, explicit remote-disabled logging, and no remote
  dispatch wiring.

## Security Considerations

The private X25519 scalar remains KMS-owned. Clatter receives only an opaque
handle/epoch representation; static DH calls KMS. Plaintext VFS secrets are not
accepted as node identity.

## Rollback

Disable remote exports and retain the local-only ephemeral broker behavior.
Existing local services remain unaffected; never replace KMS with plaintext VFS state.

## Next Steps

Proceed with the already bounded local ingress work. Remote envelope/relay work
remains gated by provider readiness and its separately governed entry conditions.
