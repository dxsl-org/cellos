# Tier-2 large anonymous memory (D-G0) — CP1 confirmation 1 approval

Recorded 2026-11-04. The accountable owner (the user) answered **"duyệt"** ("approve") to the request to decide D-M1…D-M14 of [cp1-tier2-large-memory.md](cp1-tier2-large-memory.md), presented with the recommendations below.

| ID | Approved value |
|---|---|
| D-M1/2 | One new op-multiplexed syscall `AnonMemory = 440` (map / unmap / protect); 441–442 not requested |
| D-M3 | Eager commit; reservation with `prot = 0`; per-op commit cap |
| D-M4 | Budget from signed `POLICY.BIN` v9 `anon_limit_mib`; development fallback is the system-config default 0 |
| D-M5 | Frames and table frames charged to a new generation-tagged anonymous row; global free-frame floor |
| D-M6 | 4 KiB pages only in v1 |
| D-M7 | `prot` ∈ {none, R, RW}; never EXEC; a Cell protects only its own anonymous pages |
| D-M8 | `MappingKind::Anonymous`; unmap/protect under a live DMA export returns `TryAgain`; frames of an unacknowledged revoke are withheld |
| D-M9 | v1 scope: x86_64 private roots only; SAS and other architectures get `NotSupported` |
| D-M10 | Admission of the inference Cell is a separate decision; the spec 22 status drift is to be fixed |
| D-M11 | No allowlist bit |
| D-M12 | No kernel aliasing; the shim serves Mesa's memfd as one reservation with the same base |
| D-M13 | Extent-indexed ledger and one invalidation per operation (kernel-internal) |
| D-M14 | Window 192–256 GiB, 64 MiB per op, 1024 extents, as placeholders sized at CP2 |

**Authorizes:**
- Law-1 confirmation 1 for `AnonMemory = 440` and the `POLICY.BIN` v9 field.
- The file-bounded exception for exactly the package's touched-files list. It is effective only when the owner gives this slice the Intel WIP lane, x86 breadth S1 has released the shared kernel and ABI files, and the re-planned breadth 04b plus 04a and 12 have landed.

**Does not authorize:**
- CP2 (required after implementation);
- admission of any inference Cell;
- the XSAVE work (runtime plan R5);
- hardware, production or remote claims;
- release of any CELL-CONTRACT-0024 whole-plan relation.

Per ADR-0013, the integrating session ratifies nothing itself.

**Source HEAD at binding:** `fb79d0faec23a3ec31bfe373e341e57d2887393c`, with other sessions' uncommitted edits in the tree. Digests are SHA-256 of whole files as bound.

| Path | SHA-256 |
|---|---|
| `.agents/261104-0801-x86-igpu-compute/cp1-tier2-large-memory.md` | `ff388c1a95793bda61e5947d719481050d3f8a8f6b224fa6e742ac16acb1e129` |
| `docs/abi-boundary-records.md` (with the §7 row for 440) | `1d8e8b48a2f012803d98aba2520287001b3bbfe149a668a39a592d2e5616a62a` |
| `.agents/cell-contract-0024-locks.md` | `c12306cd22bb9375251e3d8037dea3dcffa509a17e51ace669be9cafb36dc730` |
| `docs/code-standards.md` | `3be252a9ed9e9efb3075c48a0f1a2bf19bf426431306dd5b075bdfd302f9106b` |
