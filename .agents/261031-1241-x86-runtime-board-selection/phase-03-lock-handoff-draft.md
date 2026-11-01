# Lock handoff — DRAFT, not approved — x86-64 runtime board selection, Phase 03

**Status:** draft for the accountable owner (the user), 2026-11-01. **Nothing in this file is effective.** It becomes effective only after (1) the user approves [phase-03-cp1.md](phase-03-cp1.md) (Law-1 confirmation 1), and (2) the user approves this handoff, naming the file set below, and an approval record is appended. Until then, no path below may be edited for this plan.
**Plan:** [plan.md](plan.md) · [phase 03](phase-03-fingerprint-pinning.md) · [CP1](phase-03-cp1.md) · ADR-0029 §3 · ADR-0022 amendment 2026-10-31
**Precedent:** [phases 01–02 handoff](lock-handoff-approval.md) (effective 2026-11-01; its line 29 excludes Phase 03 and requires this separate handoff) · lock inventory entry [`.agents/cell-contract-0024-locks.md:131-134`](../cell-contract-0024-locks.md)

## Activation condition (sequencing)

1. Phases 01 and 02 are committed (`6eaf663a2`, `a0e009d64`).
2. The phase 01–02 handoff stays effective for its own paths. This draft does not widen it.
3. CP1 approval comes first. The file set below is the CP1 recommended option (Option A) with the decisions shown as defaults in CP1 §10. If the user picks another option or changes a decision, this file set must be re-derived before approval. Q-dependent rows are marked.
4. Whoever starts `/skill:hc-cook` for Phase 03 checks the appended approval record first and stops if it is missing.

## File-bounded exception (proposed)

### Law-1 surface (`libs/api`): exact delta in CP1 §4.1–§4.3
- `libs/api/src/abi/syscall.rs`: new `QueryMachineFacts = 433` variant and its doc, the `433 =>` decode arm (decode table today ends at `:1393`), `allowlist_bit` → `None` (dispatch-gated like 431/432), and `service::MACHINE_PIN = 20` with the "next free" doc line updated (`:1476`).
- New `libs/api/src/abi/machine_facts.rs` (the `ViMachineFactsV1` record, encoder and decoder) and new `libs/api/src/abi/machine_facts_tests.rs`. Plus `libs/api/src/abi.rs` (two `mod` lines; module list `:14-40`).
- `libs/api/src/abi/syscall_tests.rs`: change the "433 stays unmapped" assertion at `:327-335` to 433 → `QueryMachineFacts`, 434 unmapped.
- New `libs/api/src/services/machine_pin.rs` (the cellbox↔init request/response frame) and `libs/api/src/services.rs` (one `mod` line).

### SDK
- `libs/ostd/src/syscall.rs`: new `sys_query_machine_facts` wrapper, placed next to `sys_lookup_block_route` (`:2555-2575`).

### Kernel (x86 call sites only)
- `kernel/src/task/syscall.rs`, 433 only:
  - the `Syscall` enum variant (next to `:3975-3980`);
  - the `map_syscall` arm (next to `:9656-9665`);
  - the "always-permitted, gated at dispatch" list (`:4451-4467`);
  - `has_typed_errors` (`:1700-1706`);
  - the dispatch arm, gated on `caller_has_spawn` (`:1853-1855`);
  - host tests.
  - The `RegisterService` arm (`:6278-…`) only for the `MACHINE_PIN` restriction (CP1 §4.4, Q9).
- New `kernel/src/machine_facts.rs`: canonical encoding of the kernel's facts, SHA-256 via `crate::sha256::sha256` (`kernel/src/sha256.rs:87`), and host tests. Plus one `mod` line in `kernel/src/main.rs`.
- `kernel/src/board.rs`: keep `ProfileSource` and `PlatformIdentity` after binding in a write-once cell next to `BOUND_X86_PROFILE` (`:114`). Today the source is only logged (`:290-299`).
- `kernel/src/main.rs`: x86 binding call site only (`:448-466`). It hands `PlatformIdentity` and `acpi_info.dmar_present` to the cell above. ECAM validity is read through the existing `pcie_ecam::ecam_base_x86()` (used at `pcie_ecam.rs:995`), so the ECAM call site (`main.rs:555-560`) is not edited.
- `kernel/src/task/drivers/pcie_ecam.rs`: one read-only accessor that re-reads `(class, subclass, prog_if)` from config space for the facts encoder (CP1 §1.3). No change to `PCI_DEVICES`, registration or `PLATFORM_REGISTERED`.
- `kernel/src/loader/governed_spawn.rs` (`spawn_init`, `:282-315`): record the trusted-init tid once, in a write-once static, so the `RegisterService(MACHINE_PIN)` restriction does not key on a task name. **Only if Q9 = kernel restriction.**
- `scripts/mutable-globals.toml` and the regenerated `docs/generated/mutable-globals.md`: the new write-once statics (facts cell; trusted-init tid if Q9).

### Init
- New `cells/tools/init/src/machine_pin.rs` (gate, diff, safe-mode set, confirm handler, host tests).
- `cells/tools/init/src/main.rs`: the gate call between `configuration::load`/ordering (`:94-117`) and the first `launch_order` (`:122`); the safe-mode branch; the `mod` line.
- `cells/tools/init/src/supervisor.rs`: route `MachinePin` requests in `run` before `activator.handle` (`:95-105`).
- `cells/tools/init/src/activation.rs`: only if the shared `Message` receive (`:21-55`) needs a classifier for the new frame.

### VFS (record store protection)
- `cells/services/vfs/src/access.rs` (`decide`, `can_remove_*`, `can_read_fast`, `:69-141`) and `cells/services/vfs/src/access/rules.rs` (exact and prefix rows next to `/srv/.cellos-trust`, `:44-50`, `:103-107`).
- New `cells/services/vfs/src/access/machine_pin.rs`, modelled on `access/kms.rs:1-56`, with tests.

### Cellbox (ADR-0028 CLI client of init)
- New `cells/tools/cellbox/src/cmd_machine.rs`.
- `cells/tools/cellbox/src/executor.rs`: the builtin name list (`:971-973`) and the dispatch table (`:2252-2269`).
- `cells/tools/cellbox/src/main.rs`: `mod` line only.

### Test fixtures and lanes (A3 fallback; CP1 §1.5)
- New `scripts/x86/machine_pin_disk.py`, modelled on `scripts/phase09_trust_disk.py`: P1 FAT32 + P5 CellosFS by `tools/cellos-fs-provision`, blank `/srv`.
- New `tests/integration/tests/machine-pin-x86.rs`.
- `tests/integration/src/lib.rs`: one VT-d helper variant with an optional NIC. `boot_x86_bios_with_vtd` always adds e1000 (`:1437-1440`).
- `.github/workflows/ci.yml`: **only if Q10 = add a CI lane.**
- Existing x86 lanes whose disks have no P5 (`tests/integration/src/fat_disk.rs:1-4`): **only if Q5 = strict on every profile.** Each fixture builder then needs P5, which is a much larger set. The default Q5 answer needs none.

### Docs
- `docs/abi-boundary-records.md`: census rows for `ViMachineFactsV1` (K), the pin file (P) and the `MachinePin` frame (C).
- `docs/decisions/0029-platform-image-families-and-runtime-detection.md`: consequences amendment through `/skill:hc-adr`. It records the store location, the safe-mode semantics and the trust note.
- `docs/hardware-compatibility-list.md`: profile plus pin per machine, and a note under R3.
- `boards/pc/x86_64-pc/README.md`: the operator re-confirm procedure.
- `docs/vfs-api.md`: the protected `/srv/.cellos-machine` namespace.
- `CHANGELOG.md`; the phase/plan files.

### Explicitly not handed off
- `kernel/src/acpi.rs` and `kernel/src/smbios.rs`: no change. SMBIOS type 0 is excluded unless Q8 adds it. If it does, `smbios.rs` joins this list by amendment.
- `kernel/src/task/drivers/iommu*.rs`: read through `iommu::is_active()` only (`iommu.rs:254`).
- `scripts/x86/limine*.conf`, `scripts/x86/make-iso-ci.sh`, `hal/**`, `boards/src/**`, `libs/types/**`, `Cargo.lock`.
- Any Secure Boot, `limine enroll-config` or measured-boot work. That is the separate Security track (HCL R3, `docs/hardware-compatibility-list.md:73`).

## Constraints

- **Law 1:** CP1 approval before any `libs/api` edit; CP2 approval on the exact delta and evidence. The delta is additive: a new number, record, module and service id. No existing byte, number or error meaning changes.
- `dma_isolation` semantics are unchanged. The default profile stays `Required`. The pin grants nothing and never changes a profile, driver gate or DMA decision (ADR-0029 §2–3).
- Safe mode only withholds services. It never adds authority. Boot-critical storage drivers keep running under catalog policy (plan.md:29).
- No new `RegisterService` writer. `MACHINE_PIN` is registered only by trusted init (CP1 §4.4).
- **Lock relations.** All 38 `CELL-CONTRACT-0024` relations keep their whole-plan scope. Rows touched in substance:
  - `260607-1200-phase-27-protocol-hardening` (syscall authority);
  - `260603-1803-phase-h-vfs-hardening` (VFS path authority);
  - `260605-2107-full-reliability-track` (init lifecycle);
  - `260624-0630-kernel-boundary-cleanup` (registry authority);
  - `261004-1957-x86-pc-lane` (qualification).

  This exception executes none of them. It is a file-bounded slice of the new plan, like the phase 01–02 entry.
- Evidence ceiling: QEMU/TCG q35 with SeaBIOS (CP1 §6). It makes no physical, UEFI, durability or tamper-resistance claim.

## Proposed lock-inventory entry (appended to `.agents/cell-contract-0024-locks.md` only on approval)

> **Bounded adopted slice — x86 runtime board selection, Phase 03 fingerprint pinning (approved YYYY-MM-DD):** the accountable owner approved [CP1](261031-1241-x86-runtime-board-selection/phase-03-cp1.md) (Law-1 confirmation 1) and the [Phase 03 handoff](261031-1241-x86-runtime-board-selection/phase-03-lock-handoff-approval.md) for plan 261031-1241 (ADR-0029 §3).
> - File-bounded exception for exactly the paths in the record: the additive syscall 433 `QueryMachineFacts` with `ViMachineFactsV1`, service id 20 `MACHINE_PIN` and `api::services::machine_pin`; the ostd wrapper; the x86 facts encoder and binding call site; the init gate and safe mode; the VFS `/srv/.cellos-machine` rule; the cellbox `machine` command; the x86 P5 fixture and lane; and the docs.
> - `dma_isolation` semantics are unchanged. The pin carries no authority.
> - All 38 relations keep their whole-plan scope.

## Approval record

*(Empty. Append the date, the approver's words, the CP1 revision approved, and any file-set amendments.)*
