# 2026-10-20 — Cellbox Phase 01 Candidate Contracts & Verification

## What happened
Completed reachable Phase 01 preparation for the Cellbox CLI workstream under interactive hc-cook. Reconciled and froze candidate engine semantics, terminal input/control codecs, launch/session authority, VFS conditional-save/trust protocols, and command compatibility/right matrices across five SHA-256 digest-bound specification documents. Conducted comprehensive in-memory verification and resolved four formal review findings without modifying protected source, building dependencies, or minting unapproved authority.

## Decisions
- Frozen candidate digests in `contract-deltas.md`: Bound 5 specification documents (`spec.md`, `abi-session-launch.md`, `abi-vfs-telemetry.md`, `command-catalog.md`, `command-contracts.md`) to immutable SHA-256 hashes to prevent ad-hoc scope widening during downstream phases.
- Trusted confirmation actor: Selected existing `service-input` as the trusted prompt actor for SSH unknown-host confirmation (`RegisterTrustChallenge`, `BeginTrustConfirmation`, `CollectTrustDecision`), preventing untrusted client forgeability or ambient input access.
- Authority separation: Explicitly split KMS credential signing (`A`) from VFS-hosted host-trust storage (`Vt`), preserving the single-source-of-truth model for persistent trust without creating duplicate daemons.
- Input profile: Fully froze the `cellos-vt1` raw input specification in `spec.md` with explicit logical key symbols, UTF-8 scalars, CSI modifier parameters ($m = 1 + \text{Shift} + 2\cdot\text{Alt} + 4\cdot\text{Control}$), and bracketed paste framing.
- Stock ripgrep constraint: Documented that upstream ripgrep components mandate `std`/OS facilities; established that a bounded source adaptation (not a simple VFS adapter) is mandatory.

## Lessons
- Upstream component dependency analysis must check deep transitive manifests; even `grep-matcher` names `std`.
- Kernel raw dispatch gaps (such as unauthenticated raw opcode 502 for shutdown) must be formally recorded as owner-side prerequisites rather than assumed enforced by userland allowlists.

## Next steps
- Submit candidate digest dossier for maintainer Checkpoint 1 (CP1 Design Approval).
- Await formal file-owner handoffs adopting overlapping lock relations for loader, API, supervisor, VFS, and scheduler.
- Authorize and execute isolated bare-metal target dependency probes for `ratatui-core` and Sunset SSH entropy integration.
