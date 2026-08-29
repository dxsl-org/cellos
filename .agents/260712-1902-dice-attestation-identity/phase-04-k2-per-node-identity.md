# Phase 04 — K2 per-node identity (first-boot machine-id)

## Context Links
- Historical plan: [plan.md](plan.md)
- Superseding authority:
  `.agents/260819-1409-cell-to-cell-anywhere-core/reports/api-contract-phase-02b-secure-node-identity-2026-08-19.md`.
- Existing implementation seam:
  `cells/services/net-broker/src/kms_dh.rs`.

## Overview
- **Priority**: P2
- **Status**: superseded for Cell-to-Cell identity
- **Authority**:
  `.agents/260819-1409-cell-to-cell-anywhere-core/reports/api-contract-phase-02b-secure-node-identity-2026-08-19.md`.
- The earlier plaintext first-boot `machine-id` proposal is retired as a C2C
  identity root. Cloneable or exposed VFS state cannot satisfy the current
  clone-resistant, fail-closed requirement. If a machine identifier is later
  needed as non-authoritative metadata, it requires a separate approved scope
  and must not feed X25519, `CellNetId`, Noise PSK, or remote admission.

## Key Insights
- First-boot random VFS state is cloneable and exposed with copied storage.
- Service-specific ACLs do not make a cloned secret clone-resistant.
- The existing KMS ABI and `KmsBackedX25519` adapter provide the required
  non-exportable static-DH boundary.

## Requirements
- Do not implement the historical plaintext identity proposal.
- Do not derive X25519, `CellNetId`, Noise PSK, or admission authority from VFS
  machine metadata.
- Keep remote operation fail closed until a qualified protected provider is
  ready and all later transport/governance gates pass.

## Architecture
Historical proposal superseded by:
`broker registration → KMS ready snapshot → opaque handle/epoch/public key →
KMS static DH`.

## Related Code Files
- `cells/services/net-broker/src/kms_dh.rs`
- `cells/services/net-broker/src/main.rs`
- `cells/services/net-broker/src/transport.rs`
- `cells/services/kms/src/`
- `libs/ostd/src/clients/kms.rs`

## Implementation Steps
1. Retire the plaintext VFS identity root.
2. Validate exact KMS register/status/acquire snapshots.
3. Pass only opaque metadata into the Clatter DH adapter.
4. Preserve ephemeral local-only fallback with remote disabled.

## Todo List
- [x] Supersede plaintext VFS K2 for Cell-to-Cell identity.
- [x] Route the broker through the opaque KMS static-DH seam.
- [ ] Reopen only for separately approved non-authoritative metadata.

## Success Criteria
- No plaintext VFS secret is used as the C2C identity root.
- The broker holds only KMS handle/epoch/public metadata for a ready identity.
- Clone, policy, provider, epoch, or revision mismatch keeps remote disabled.

## Risk Assessment
- **Clone collision:** any VFS-rooted identity can be copied with its storage.
  Mitigation: it is not an identity authority.
- **Provider unavailable:** KMS may have no qualified root. Mitigation: local
  ephemeral operation continues while remote remains disabled.

## Security Considerations
The node private scalar remains KMS-owned and never enters broker or VFS memory.
Any future machine identifier is metadata only.

## Next Steps
Qualify the protected provider and recovery policy in their governed lanes.
Do not reopen this historical plaintext design for remote identity.
