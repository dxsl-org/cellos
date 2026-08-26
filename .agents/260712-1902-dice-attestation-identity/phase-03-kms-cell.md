# Phase 03 — KMS Cell (thin Silo veneer; first client = TLS)

## Context Links
- Plan: [plan.md](plan.md)
- Dossier "KMS Cell — thin wrapper" (dossier:90-97): Tier-1 service over `SiloHandle`; Wrap/Unwrap/
  Derive over typed IPC; first client = TLS (replace hardcoded keys); home for root-CDI sealing.
- Silo client: `libs/ostd/src/silo.rs` (whole file — KMS calls this, adds no crypto)
- Service registry: `libs/api/src/abi/syscall.rs:691-722` (next free `service::` = **13**)
- Service pattern precedent: `service::SILO=6` (`libs/types/src/silo.rs:108`), IPC wire (`silo.rs:75-108`)

## Overview
- **Priority**: P2
- **Status**: pending
- **Testability**: partial CI — the KMS Cell + IPC contract test on the software-fallback signer;
  full Wrap/Unwrap needs Silo (G2 ARM64/x86).
- **🔶 ABI-additive (not a break, but ABI-visible)**: adds `service::KMS = 13` + KMS wire types. Flag
  to user (additive; no existing value changes). No new *syscall* — uses `sys_send`/`sys_recv` +
  `RegisterService`(205)/`LookupService`(206).
- A small Tier-1 service Cell wrapping `SiloHandle`, exposing Wrap/Unwrap/Derive over typed IPC,
  registering `service::KMS`, and owning the Silo key lifecycle (resolves P02's single-key-reuse risk).

## Key Insights
- **KMS is a veneer, not a subsystem** (dossier:90) — it holds a `SiloHandle` and forwards. The value
  it adds over raw Silo: (a) a stable service ID so clients resolve via `LookupService` (survives
  respawn, like all services — `syscall.rs:688-690`), (b) it *owns* the one-time `init_key` so TLS,
  attestation, and net-broker don't each try to seed Silo (the P02 collision risk), (c) it is the home
  for sealing the root CDI.
- **First client = TLS**: today TLS uses hardcoded keys (memory: TLS data-path). KMS Wrap/Unwrap lets
  TLS store a wrapped key blob and unwrap at use. This is the proof-of-value client and the migration
  target — but keep it a *follow-on* within this phase, gated behind KMS being green.
- Follows the exact 128-byte raw-buffer IPC discipline Silo uses (`silo.rs:8-18`) OR the typed
  postcard path used by other services — **pick the raw 128-byte shape** to match Silo and avoid a
  postcard parser in a key-handling cell (KISS + smaller TCB surface).

## Requirements
- Functional (KMS IPC ops):
  - `Derive(label, context) -> 32B` — HKDF over a KMS-held root (Silo-sealed where available).
  - `Wrap(plaintext_key) -> wrapped_blob` / `Unwrap(wrapped_blob) -> plaintext_key` — AEAD under a
    KMS-held wrapping key. (Encryption primitive = the `p256`/AEAD userspace path; no kernel crypto.)
  - Registers `service::KMS=13` at startup; resolvable via `LookupService`.
- Non-functional: `#![forbid(unsafe_code)]`; one request at a time (FIFO), matching Silo's serialized
  contract (`silo.rs:93-98`); `Drop` zeros any transient key material (Law 8).

## Architecture
`client (TLS / attestation / net-broker)` → `LookupService(KMS=13)` → `sys_send(kms_tid, req[128])`
→ KMS Cell → (Derive|Wrap|Unwrap) via its `SiloHandle` → `sys_send` reply[128]. KMS owns the single
`SiloHandle::init_key` call; all other cells consume via KMS, never seed Silo directly.

## Related Code Files
- **Create**: `cells/services/kms/` (Cell), `libs/types/src/kms.rs` (wire types + `KMS_SERVICE_ID`),
  KMS client helper in `libs/ostd/src/clients/`.
- **Modify (additive ABI)**: `libs/api/src/abi/syscall.rs` — add `service::KMS = 13` const + doc.
- **Reference**: `libs/ostd/src/silo.rs`, `libs/types/src/silo.rs`.
- **Follow-on modify**: TLS key-load site (replace hardcoded keys with KMS Unwrap) — separate commit.

## Implementation Steps
1. **[GATE: confirm additive `service::KMS=13` + wire types with user]** Define `libs/types/src/kms.rs`
   (opcodes, 128-byte req/resp, `KMS_SERVICE_ID=13`), mirroring `types/src/silo.rs:75-108`.
2. Build the KMS Cell: connect `SiloHandle`, do the one-time `init_key` (entropy via `GetRandom=214`),
   `RegisterService(KMS)`, FIFO request loop.
3. Implement Derive (HKDF via `libs/attestation::hkdf`), Wrap/Unwrap (AEAD).
4. KMS client helper in `ostd`; contract test cell exercises Derive round-trip on the software path.
5. **Follow-on**: migrate TLS to `KMS::Unwrap`; verify HTTPS end-to-end still green (memory: TLS
   must `flush()`), gated behind KMS smoke passing.

## Todo List
- [ ] `service::KMS=13` + `libs/types/src/kms.rs` (ABI-additive confirmed)
- [ ] KMS Cell: SiloHandle + one-time init_key + RegisterService + FIFO loop
- [ ] Derive / Wrap / Unwrap implemented
- [ ] KMS client + Derive round-trip contract test (software path, CI)
- [ ] Follow-on: TLS uses KMS Unwrap; HTTPS suite green

## Success Criteria
- A client resolves `service::KMS` and gets a deterministic `Derive` result (software path, CI).
- On QEMU ARM64 with Silo: KMS owns the sole `init_key`; Wrap/Unwrap round-trips.
- TLS (follow-on) loads its key via KMS Unwrap; existing TLS/HTTPS tests stay green.

## Risk Assessment
- **Silo single-key ownership contention (High × High → mitigated)**: KMS becomes the *sole* caller of
  `init_key`; attestation (P02/P05) and TLS consume via KMS. Enforce in review — no other cell calls
  `SiloHandle::init_key`.
- **Key-handling cell TCB (Med × High)**: mitigation — raw 128-byte IPC (no postcard parser), forbid
  unsafe, zero transient material on Drop, one-request-at-a-time.
- **TLS migration regression (Med)**: mitigation — follow-on commit, gated behind KMS smoke; revert is
  isolated to the TLS key-load site.

## Security Considerations
- Wrapped blobs may be persisted to VFS; the wrapping key must be Silo-sealed (or dev-seed in CI) and
  never written plaintext. Unwrap output lives only transiently in the requesting cell.
- KMS is a capability chokepoint: only cells that resolve `service::KMS` and are allowed to send it IPC
  can derive/unwrap. Consider a policy entry (`policy.rs`) restricting who may talk to KMS.

## Next Steps
- P05 (K3) uses `KMS::Derive`/the Silo-held Alias key for the enrollment token instead of seeding Silo.
- Root-CDI sealing lands here as the wrapping-key provisioning step (hardware-informed).
