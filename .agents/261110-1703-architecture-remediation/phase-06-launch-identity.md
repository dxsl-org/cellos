---
phase: 6
title: "Launch by measured identity, per-device PCIe handout"
status: in-progress
priority: P1
effort: "6–9d"
dependencies: [5]
tier: thinking
checkpoint: true
---

# Phase 06: Launch by measured identity, per-device PCIe handout

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs.

## Context Links

[Review C-1, H-3](../reports/architecture-review-261110.md) · `kernel/src/loader/launch_profile/mod.rs` · `kernel/src/loader/governed_spawn.rs` · `kernel/src/task/syscall.rs` · `kernel/src/task/drivers/pcie_ecam.rs` · `cells/services/supervisor/src/main.rs` · `cells/services/vfs/src/access/srv-owner.rs` · [Security model](../../docs/security-model.md) · [Red team](reports/red-team.md) #3

## Overview

Stop deriving authority from a spawner-chosen basename, and stop handing any unowned PCI function to any holder of `PcieDriverCap`. Closes both items that break a stated guarantee.

## Key Insights

- Verified: task name = `path.rsplit('/').next()` (`governed_spawn.rs:201`); `authorize` matches `caller.name`, six names with no capability (`launch_profile/mod.rs:59-103`); `Spawner::Ceiling` is not intersected with the caller (`governed_spawn.rs:148`); `/apps/`, `/data/bin/`, `/home/` are admitted targets (`targets.rs:155`).
- Verified: `pcie_usable_by` is "unowned or mine" (`task/syscall.rs:2611-2613`); `find_vendor_device` filters only Ethernet IDs with a driver (`pcie_ecam.rs:1553-1574`).
- The kernel already records per incarnation the image hash, anchor and signed name (audit remediation, syscall 441 `ResolveCellImage`).

## Requirements

- Functional: launch edges keyed on the caller's kernel-measured image identity (signed image name + verifying anchor), never on task name; unsigned or dev-anchored callers match only rows explicitly marked for that posture; supervisor and the raw power-off gate use the same identity; PCI handout requires the function's `(vendor, device, class)` to belong to the caller's catalog row.
- Non-functional: no new syscall unless CP1 approves one; denial paths log caller incarnation.

## Architecture

- `CallerLaunchState { name }` → `CallerLaunchState { image: Option<MeasuredImage> }`; catalog edge rows gain `caller = <signed image name>`.
- Supervisor: replace `GetProcs`-name checks with identity from the attested caller trailer; if the trailer lacks a usable role bit, that is a Law-1 delta (CP1) — otherwise no ABI change.
- PCIe: `pcie_usable_by(caller, bdf)` additionally consults `catalog::pci_row_for(caller_image)`.

## Assumptions

- **Claim:** the C-1 chain is reachable: a capability-free cell at `/home/x/cellbox` obtains `cellbox` launch rows on the `Path` route.
  **Confidence:** low (inferred, not exercised)
  **How to verify:** step 1 negative test in an x86 test image. If it cannot be made to succeed on the current tree, log a Deviation, downgrade C-1 in the review, and continue (the key change is still required by H-2).
- **Claim:** every production caller with launch rows is signed with a name the kernel records.
  **Confidence:** medium
  **How to verify:** list callers in `launch_profile/mod.rs:59-103`; check each artifact's `__ViCell_image` name via the signer output.
- **Claim:** the caller trailer exposes enough identity for the supervisor without an ABI change.
  **Confidence:** low
  **How to verify:** read `libs/api/src/abi/caller_identity.rs` and syscall 441's contract.

## Related Code Files

- Modify: `kernel/src/loader/launch_profile/{mod,profiles,targets,tests}.rs`, `kernel/src/loader/governed_spawn.rs`, `kernel/src/loader/spawn_request.rs`, `kernel/src/task/syscall.rs` (launch-edge authorisation, name gates, `pcie_usable_by`), `kernel/src/task/drivers/pcie_ecam.rs`, `catalog/cells.toml`, `cells/services/supervisor/src/main.rs`, `docs/security-model.md`
- Create: test cell under `cells/tests/` for the negative control (fixture catalog row only), `reports/phase-06-evidence.md`
- Conditional (CP1): `libs/api/src/abi/caller_identity.rs`

## Implementation Steps

1. Write the negative test: a sandbox-path binary with a privileged basename attempts each name-keyed edge and the raw power-off gate; record observed result on the unmodified tree.
2. Change the launch key to measured identity; update catalog rows; remove dead `Spawner::User`.
3. Replace kernel name gates (power-off, argv staging) with the same identity check.
4. Supervisor: switch to attested identity. If the trailer is insufficient, stop and file CP1.
5. PCIe: add the catalog-row check at handout; a second test cell holding `PcieDriverCap` for a different ID must be refused the NVMe/AHCI function.
6. Update `security-model.md` to state the real rule.

## Todo List

- [x] Negative test, before-result recorded
- [x] Identity-keyed launch
- [x] Name gates removed (kernel: 502, argv staging, launch edges)
- [x] Supervisor identity — CP1 option A ([CP1](cp1-supervisor-caller-identity.md), [CP2 evidence](cp2-supervisor-caller-identity.md))
- [x] PCIe handout check
- [x] Docs

## Success Criteria

Evidence: [reports/phase-06-evidence.md](reports/phase-06-evidence.md). Supervisor half: [CP2 record](cp2-supervisor-caller-identity.md); CP2 confirmation pending.

- [x] The negative test fails to obtain any reviewed edge or the power-off gate after the change (and its before-result is on record).
- [x] Cross-ID PCI claim is refused; the legitimate driver still binds (x86 NVMe/AHCI and NIC boot lanes).
- [x] Checkpoint: full `pr`-tier suite, `x86-c2c`, release-posture boot lane, RV64 baseline, AArch64 boot; independent review of the authority diff (haily-reviewer) recorded in the evidence file.

## Risk Assessment

- A legitimate launcher is unsigned in dev images and loses its rows: dev posture rows cover it explicitly; the boot lanes catch misses. Rollback: restore the name key (table-key change, no persistent state).
- If CP1 is needed, the supervisor part waits; kernel-side fix still ships.

## Security Considerations

Core of the phase. Dev posture remains spoofable by design (public dev seed); the docs must say so next to the rule.

## Next Steps

Phase 07 replaces per-thread capability copies with one record per incarnation.

## Deviation Log

- **Decision (step 1):** the negative control is a kernel unit test, not a QEMU cell. Before-result recorded with a throwaway probe on the unmodified tree: a caller named `cellbox` with no spawn right got `/bin/curl` (net ceiling) and `/bin/hypervisor` (hypervisor ceiling); a task named `shutdown` from `/home/x/shutdown` passed the raw-502 gate. After: `raw_shutdown_is_accepted_only_for_the_signed_shutdown_image` (unsigned, other-signed, ELF-route and correctly signed callers) and `catalog::tests::launch_identity_needs_a_non_alias_install_path`. Claim "C-1 reachable" upgraded from low to confirmed.
- **Decision (identity shape):** the key is the signed program name, honoured only when the kernel resolved a `/bin/` install path (boot, Path, Pinned, replacement) or trusted init supplied the bytes (`Task::launch_named`). Not "catalog row exists": `/bin/shutdown`, `/bin/kill`, `/bin/ai` have no row. ELF/Mem bytes carry no identity (review finding 2: signed `cellbox` bytes under an advisory path would otherwise gain shell edges).
- **Surprise:** `/bin/cellbox-ssh` is the signed `cellbox` file; by image name alone it gained every shell edge (review finding 1, High). `catalog::IDENTITY_ALIASES` gives it no identity, preserving its old "no edges" behaviour.
- **Deviation:** hypha caller keys renamed to the signed names `hypha-core` / `hypha-tool-spawn`; every other edge key already equalled its signed name (reviewer verified).
- **Decision:** trusted-init latch now published inside `publish_prepared` before `push_ready` (review finding 4), so init never runs with an empty identity.
- **Deviation (PCIe):** the check is on the query (418 class triple / 424 vendor:device) against `catalog::PCI_DRIVERS` keyed by program name, before the BDF search, instead of inside `pcie_usable_by`. Same effect, no ECAM lookup per candidate. `check-catalog.py` now ties each `pcie_driver` row to one PCI row (mutation-tested). No second test cell: the cross-ID refusal is a pure table check covered by `catalog::tests`.
- **Decision (step 4):** the supervisor needed Law-1 CP1, because the trailer carries no image name and 441 answered only VFS. The kernel half was committed first (`7dc57d730`). The owner approved option A on 2026-11-11, plus a file-set extension for `libs/ostd/src/app.rs` (opt-in caller attestation in the app loop). 441 answers a `SupervisorCap` holder only for launch-named Cells.
- **Surprise (step 4):** the attested trailer broke `parse_power_request`, which requires zero bytes after the request, and `power-x86` refused the operator. The ostd payload now excludes the kernel-owned tail.
- **Deviation:** the supervisor identity rule moved to `cells/services/supervisor/src/sender_identity.rs` (new module in the same crate), which keeps `main.rs` near 200 lines.
- **Surprise:** `xhci-x86` hub/second-controller tests (3) fail locally on the original HEAD `8a005f998` too (same timeout) — pre-existing, not this phase.
- **Surprise:** init starts every driver through `SpawnFromElf` with bytes it read from VFS, so "Path route only" left all drivers unnamed and every 424 query was refused (x86 NIC/xhci lanes failed). Children of trusted init are named on any route; init is trusted core and reads `/bin` itself.
- **Removed:** dead `Spawner::User` (no constructor anywhere).
