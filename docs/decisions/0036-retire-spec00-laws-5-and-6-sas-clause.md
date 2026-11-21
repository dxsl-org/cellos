# ADR-0036 — Retire Spec 00 Law 5 (no `mod.rs`) and the Law 6 SAS-ownership clause

**Date:** 2026-11-21
**Status:** Accepted
**Amends:** [Spec 00](../specs/00-context.md) §3, Law 5 and Law 6 (the "Định danh SAS" bullet).
**Decider:** Cellos maintainer (owner), residual N-4 of the 2026-11-10 architecture review.

## Context

Spec 00 contained two rules that the code base never followed:

- **Law 5** banned `mod.rs` outright. The tree tracks 41 first-party `mod.rs` files outside `third_party/` (for example `kernel/src/arch/mod.rs`, `kernel/src/task/syscall/mod.rs`, `libs/ostd/src/service/mod.rs`). No CI check exists, and `6d4219819e` already removed the ban from the contributor docs (`docs/code-standards.md`, `docs/PATTERNS.md`, `CONTRIBUTING.md`).
- **Law 6, "Định danh SAS"** required every `Vi`-prefixed trait to support ownership transfer or Lease/Grant. There are 38 such traits, most of them synchronous device, UI or allocator interfaces over borrowed buffers or hardware state (`ViUart`, `ViSpi`, `ViCanvas`, `ViTheme`, `ViMte`). Ownership transfer has no meaning for them. Cross-Cell data already moves only through kernel-copied messages (`libs/wire`, [Spec 17](../specs/17-ipc-wire-contract.md)) and Grants (`libs/ostd/src/grant.rs`), which Law 2 and Law 7 already state.

On 2026-11-16 both rules were bannered "Historical" in place. The end-to-end review recorded this as residual N-4, because no phase had changed them deliberately.

## Decision

1. **Law 5 is retired.** `mod.rs` is permitted. New modules should prefer the `foo.rs` + `foo/` layout; that is a style preference with no lint behind it. File and directory names stay snake_case.
2. **The Law 6 SAS-ownership clause is retired.** The `Vi` prefix is a naming convention marking a Cellos contract, as the Law 6 naming table describes. Ownership and lifetime are specified by each actual boundary: messages per Spec 17 and Grants per `libs/ostd/src/grant.rs`. The trait name does not carry them. The rest of Law 6 (no name clashes with forks, wrap forked code behind `Vi` traits) stands.

Enforcement was rejected. It would mean moving 41 module roots and adding a lint, and redesigning 38 heterogeneous traits. A redesign touching `libs/abi` also needs Law 1's double confirmation. That is multi-phase API work with no defect behind it.

## Consequences

- Spec 00 states the actual convention instead of a crossed-out rule.
- Reviewers no longer cite Law 5 or the SAS clause; ownership questions go to the IPC or Grant contract of the boundary in question.
- A future reason to restrict `mod.rs` or to add ownership-transfer traits needs a new ADR.
