# ADR: Signed Canonical Boot Manifest for Root Authority Ceilings

**Date**: 2026-10-31 | **Status**: Proposed, NOT accepted, NOT implemented | **Origin**: ginux-lessons Phase 07 step 5

Amends nothing until accepted. Relates to `docs/specs/25-cell-kernel-authority-contract.md` §2 and §3.1, and `docs/specs/18b-cell-admission-consent-adr.md`.

---

## Decision (proposed)

1. **Root ceilings move from code to one signed artifact.** The per-path table in `kernel/src/loader/boot_ceiling.rs` becomes the canonical *boot manifest*: one row per boot path, stating its ceiling (`CapSet` plus region and device masks) and its bootstrap-only grants. The manifest is signed by the **release key** (the same anchor as `release-posture`; never the dev key in a release kernel).
2. **A signature establishes provenance, not authority.** A valid manifest signature means "this ceiling table was reviewed". The effective grant stays `request ∩ manifest ceiling ∩ signed policy` (Spec 25 §3.1). Neither a cell's signature nor the manifest's signature grants a right by itself, and the intersection never becomes a union.
3. **Sealing.** Once init has received its bootstrap grants, the kernel seals the manifest for that boot: no further root issuance from manifest rows, except rows explicitly marked `recovery`.
4. **No runtime update path.** The manifest changes only with a new boot artifact. No syscall, service or operator API rewrites it at runtime.

## Boundaries

| Boundary | Rule |
|---|---|
| Root | The kernel verifies the manifest once at boot, before creating init. A missing or invalid signature in release posture fails the boot; dev posture logs `unsigned manifest` and uses the built-in table. |
| Update | A new manifest requires a new signed boot artifact. Rollback protection follows the artifact version (out of scope here). |
| Seal | Seal happens after the init handoff (Spec 25 §2: "Bootstrap-only rights are sealed/revoked after handoff"). A post-seal root-issuance attempt is refused and logged. |
| Recovery | Only rows marked `recovery` survive the seal, and only for the recovery authority Spec 25 §2 requires before an init restart is called safe. Until that authority is qualified, no row carries `recovery`. |

## Example: observer versus service

The "observer-read / service-write" split applies only to a real resource with separate operations and admitted consumers (Phase 07 requirement). Illustrative example, **not existing code**: `service-net` link statistics. No statistics handle exists today, and the current `/bin/net` row is `network: true` over `LEGACY` (`kernel/src/loader/boot_ceiling.rs:113`).
- The `/bin/net` row would bound the link-writing authority (`network`) and let `service-net` issue statistics handles.
- An observer cell's row would grant nothing on the link. The observer would receive a read-only statistics handle from `service-net`, the issuer, under that issuer's own table (Spec 25 §3.3); the kernel only attests the transfer.
- The manifest bounds what each path *can* ever hold. It never grants the observer a statistics right directly; that remains an issuer operation.

## Consequences

- Plus: the ceiling table becomes reviewable and attestable without rebuilding the kernel, and the boot log can name the manifest hash.
- Minus: a signature check before init (bounded, once per boot); a new artifact in the release pipeline that needs a CI secret, already blocked on release-key provisioning.
- No hot-path cost: invocation still checks per-incarnation CSpace state; no per-call signature verification (Spec 25 §3.2).

## Open questions (must be decided before acceptance)

1. Manifest encoding and its location in the boot artifact.
2. Whether device and region masks belong in the same manifest or in the signed `/POLICY.BIN`.
3. The recovery-authority definition (depends on Spec 25 §2 qualification).
4. Law-1: accepting this ADR changes boot semantics and needs its own CP1/CP2; this document grants nothing.
