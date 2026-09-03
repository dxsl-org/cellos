# ADR-0007: Use development-first hardware-constrained execution

**Date**: 2026-08-28
**Status**: Accepted

## Context

Cellos has useful software, platform, and hardware-integration work that can be
performed with the assets already available: QEMU, two owner-reported Raspberry
Pi 3 Model B+ boards, and incoming sensors. Both current boards still require
exact serial, revision, and condition reconciliation. The prior exact-device
record reports revision `a22082` / Raspberry Pi 3 Model B / serial
`000000003d042795` and remains unassigned to either current board. No additional
hardware procurement is planned now.

The production architecture has stronger requirements than these development
assets can prove. QEMU is a software/emulation environment. The Raspberry Pi 3
can support G1 development and exact-board integration, but it is not a
production-security qualification target and cannot provide the independent,
authenticated, rollback-resistant external floor required for production
admission. Sensor exercise can prove behavior only for the exact device and
interface exercised.

Roadmap language that lists production prerequisites beside development work can
be misread as one global queue. That interpretation would halt useful QEMU,
RPi3, sensor, and local-runtime progress even though those lanes do not claim
production admission. The opposite response—relaxing production gates so
available development hardware can satisfy them—would violate the existing
fail-closed security decisions in
[ADR-0005](./0005-mutual-tls-relay-identity.md) and
[ADR-0006](./0006-block-production-root-pending-exact-product-evidence.md).

## Decision Drivers

- Continue useful development and hardware-integration work with available
  assets instead of waiting for production-only hardware and services.
- Preserve truthful evidence ceilings for host, QEMU, and exact-device physical
  exercise.
- Keep production admission and release fail-closed without turning their gates
  into global development blockers.
- Distinguish current defects from intentionally later capabilities, external
  prerequisites, and production release invariants.
- Avoid speculative procurement before a lane has an exact, evidence-backed
  hardware requirement.
- Preserve ADR-0005's protected relay identity boundary and ADR-0006's refusal
  to select a production root without exact-product evidence.

## Considered Options

### Option A (chosen): Advance independent development lanes to explicit evidence ceilings

- **Pro**: Uses QEMU, both owner-reported Raspberry Pi 3 Model B+ boards, and
  incoming sensors immediately.
- **Pro**: Keeps software, development-hardware, and production evidence
  distinguishable.
- **Pro**: Leaves every production-admission and release invariant mandatory.
- **Pro**: Avoids buying hardware before an exact lane and qualification contract
  justify it.
- **Con**: Some advanced remote, protected-root, and production evidence remains
  unavailable.
- **Con**: Roadmap owners must state a lane's planning class and evidence ceiling
  instead of relying on one global phase order.
- **Chosen because**: It maximizes useful work supported by current assets
  without overstating evidence or weakening a security boundary.

### Option B: Block all development until production security and hardware prerequisites exist

- **Pro**: Produces a superficially simple single queue.
- **Con**: Prevents unrelated QEMU, RPi3, sensor, and local-runtime work that does
  not depend on production identity or root qualification.
- **Con**: Conflates production admission with software development and discards
  useful bounded evidence.
- **Rejected because**: Production-only prerequisites have no causal dependency
  on many current development lanes and therefore must not serialize them.

### Option C: Weaken or remove production gates so current assets can qualify

- **Pro**: Would make a production-readiness claim appear reachable sooner.
- **Con**: QEMU cannot prove physical protection, exact-product lifecycle,
  secure/measured boot, hostile physical behavior, or independent protected
  state.
- **Con**: RPi3 is not a qualified external floor and cannot satisfy the
  production root and protected relay requirements.
- **Con**: Relabeling local evidence would contradict ADR-0005 and ADR-0006 and
  make admission fail open by policy.
- **Rejected because**: Schedule convenience cannot replace a mandatory
  production-security invariant or evidence class.

### Option D: Buy speculative advanced hardware now

- **Pro**: Might expose future APIs or board constraints earlier.
- **Con**: No currently selected stock TPM, generic secure-element counter,
  accelerator, or additional board is proven to satisfy the exact production
  contracts.
- **Con**: Procurement without a pinned lane, product identity, support package,
  and acceptance contract risks buying hardware that cannot qualify.
- **Rejected because**: Current assets support the next useful work, while exact
  production hardware selection remains evidence-gated under ADR-0006.

## Decision

Cellos adopts development-first, hardware-constrained execution.

1. **Use the available inventory.** QEMU, two owner-reported Raspberry Pi 3
   Model B+ boards, and incoming sensors are the active development platform.
   Reconcile both boards' exact serial, revision, and condition before
   attributing exact-device evidence. No additional hardware procurement is
   authorized by this decision.
2. **Schedule by lane, not by a global production queue.** QEMU, RPi3, sensor,
   and local Cell-to-Cell runtime lanes may advance independently when their own
   dependencies are met. An unavailable advanced capability blocks only the
   milestone that requires it.
3. **Classify roadmap work explicitly.** Every routed item uses one primary
   planning class:
   - **current executable work** for useful work supported by present assets;
   - **current-scope technical debt** for confirmed defects or maintainability
     gaps in the current supported scope;
   - **future capability** for intentionally later functionality;
   - **external-gated prerequisite** for a named unavailable asset, product,
     account, service, or vendor evidence package; or
   - **production release gate** for a mandatory production-admission or
     production-release invariant.
4. **Enforce evidence ceilings.** Host and QEMU results are software evidence
   only. They cannot qualify physical hardware, a service, a protected root, or
   production. RPi3 and sensor exercise may establish development and
   hardware-integration behavior only for the exact exercised device. RPi3 is
   never a production-security qualification target, production KMS/root,
   secure/measured-boot witness, or qualified independent external floor.
5. **Keep production admission disabled and fail-closed.** Remote C2C identity
   where applicable, protected relay identity, the production KMS/root,
   secure/measured boot, a qualified authenticated rollback-resistant external
   floor and persistent recovery, physical hostile evidence, an authenticated
   runner, required human approvals, and governed release-ledger closure remain
   mandatory before the applicable production-admission or production-release
   claim.
6. **Select no substitute production floor.** This decision does not select a
   stock TPM, generic secure-element counter, RPi3, QEMU model, or sensor as
   the floor.
   ADR-0006's exact-product evidence package and superseding GO ADR remain
   required before production root implementation or qualification resumes.

## Consequences

### Positive

- The next session can identify and reconcile both current Raspberry Pi 3
  Model B+ boards and incoming sensors, with QEMU used for bounded software
  counterparts and fixtures.
- Local-runtime and other software lanes can continue without waiting for
  protected relay, cloud, accelerator, or production-root assets.
- Evidence remains auditable because each result stops at a stated ceiling.
- Production admission, protected identity, and release governance remain as
  strong as before this decision.
- Roadmap language distinguishes actual current-scope debt from later product
  capability and external prerequisites.

### Negative / Risks

- Development progress will not by itself shorten every external production
  dependency.
- Exact-device RPi3 or sensor results may need to be repeated on a future
  production candidate.
- Maintainers must reject wording that silently promotes host/QEMU evidence or
  treats RPi3 as a production-security target.
- Future hardware procurement remains deferred until an exact lane and evidence
  contract justify it.

## Review Rule

A roadmap or architecture update violates this decision if it either (a) makes
an advanced production prerequisite a global blocker for unrelated QEMU, RPi3,
sensor, or local-runtime work, or (b) weakens, bypasses, or relabels a production
admission/release gate. Reopening the production-root selection still requires
the evidence and superseding decision specified by ADR-0006.

## Links

- [ADR-0005: Use mutual TLS for external relay identity](./0005-mutual-tls-relay-identity.md) — protected external identity remains a production prerequisite; local-runtime work does not satisfy or wait on that claim.
- [ADR-0006: Block production root selection pending exact product evidence](./0006-block-production-root-pending-exact-product-evidence.md) — no root product is selected and development assets remain non-qualifying.
- [ADR-0008](./0008-protected-relay-tls-endpoint-ownership.md) — the protected TLS endpoint may advance only to its lane-specific DEV_REFERENCE ceiling until production gates pass.
- [ADR-0011](./0011-use-cloudflare-roughtime-for-dev-signed-time.md) — Cloudflare Roughtime remains a `DEV_REFERENCE` software/live evidence source and cannot satisfy production time-authority or admission gates.
- [ADR-0012](./0012-use-external-lineage-table-and-kms-key.md) — allocator lineage is a separate `DEV_REFERENCE` table/key contract; its host tests do not prove live AWS isolation, restore safety, or production rollback resistance.
- [ADR-0013](./0013-solo-first-development-independent-promotion.md) — extends lane-local execution to a solo maintainer while retaining GitHub-recorded independent promotion gates.
- [Project roadmap](../project-roadmap.md#development-first-hardware-constrained-decision) — authoritative capability routing and planning classes.
- [Current focus](../roadmap/current-focus.md#development-first-solo-first-execution-boundary) — active inventory, solo execution rule, and next-session work order.
- [System architecture](../system-architecture.md#development-execution-and-production-admission-boundary) — architecture-level evidence ceilings and production boundary.
- [Tier 1 baseline and production admission plan](../../.agents/260821-1700-tier1-baseline-admission/plan.md#child-contract) — production-admission blockers remain local to that plan.
