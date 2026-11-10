# Scout report — architecture remediation

Recon is reused from the [architecture review](../reports/architecture-review-261110.md) (7 read-only subsystem passes + lead pass, HEAD `8a005f998`). No new scouting was run for this plan. Evidence state per finding is carried over unchanged: **V** = lead re-read the cited source; **R** = reviewer-cited `file:line`, not re-verified. Every **R** item is an assumption for the phase that consumes it and is re-read in that phase's first step.

## Orientation

| Area | Entry points | Size / shape |
|---|---|---|
| Kernel | `kernel/src/main.rs` (`kmain` :151-1781), `task/syscall.rs` (dispatch :5470-10724), `task.rs` (`SCHEDULER` :548) | 282 files, 135k lines; `task/` ≈55% |
| Authority | `task/cap.rs`, `loader/{governed_spawn,boot_ceiling}.rs`, `loader/launch_profile/`, `policy.rs`, `signing.rs`, `admission/`, `cell/cspace.rs` | 7 path-keyed tables + `scripts/sign-policy.py` scraper |
| IPC | `task/async_ipc.rs`, `task/completion*.rs`, `task/cell_session.rs`, `cell/service_registry.rs` | exact-operation path is the coherent one |
| Memory | `memory/{address_space,paging,frame,pin,tlb_shootdown,deferred_release}.rs`, `task/user_copy/` | `AddressSpace` is the model owner type |
| HAL | `hal/core` (facade), `hal/arch/{x86,arm,riscv}`, `hal/traits/*`, `boards/` | traits mostly unimplemented |
| SDK | `libs/types ← libs/api ← libs/ostd` | acyclic; `api` = ABI + wire + libc shim |
| Services | `cells/tools/init`, `cells/services/{vfs,net,supervisor,kms,compositor,input}` | hand-rolled loops |
| Build/CI | `.github/workflows/ci.yml` (27 jobs), `scripts/assemble-x86-image.py`, `gen_disk.ps1`, `scripts/gen-disk*-ci.sh`, `kernel/build.rs` | x86 recipe is the single-source model |

## Contracts that constrain the plan

- **Law 1** (`docs/code-standards.md:40-43`): any change under `libs/api/` or `libs/types/` needs two owner confirmations (CP1 design, CP2 after evidence).
- **CELL-CONTRACT-0024** (`.agents/cell-contract-0024-locks.md`): edits to locked surfaces need a file-bounded exception per slice; exceptions end with the slice.
- **ADR-0022**: Intel x86-64 C2C is the sole direction; existing non-Intel code and regressions are preserved.
- **Spec 15**: kernel whitelist/blacklist; currently out of date (review H-8).
- Frozen surfaces with digest guards: `scripts/check-{lookupservicebound,multi-source-wait,ai}-law1-digests.sh`.

## Reusable verification commands (exist in repo; not run for this plan)

- `bash scripts/check-baseline.sh` — RV64 baseline.
- `bash scripts/build-x86_64-domain-test-ci.sh` then `bash scripts/x86/qemu-domain-test.sh` — x86 Tier-2 domain lane.
- `bash scripts/qemu-two-node-c2c-test.sh` (with and without `C2C_NEGATIVE=1`) — two-node C2C, TCG.
- `bash scripts/build-x86_64-c2c-lifecycle-ci.sh` then `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test local-service-lifecycle-x86`.
- `bash scripts/qemu-x86_64-test.sh` — x86 boot lane.
- Guards: `python3 scripts/check-spec-anchors.py`, `bash scripts/check-hal-boundaries.sh`, `python3 scripts/check-cargo-config-parity.py`, `python3 scripts/check-cell-va-layout.py`, `python3 scripts/generate-code-metrics.py`.

## Finding → phase traceability

| Finding | State | Phase |
|---|---|---|
| C-1 name-keyed launch authority | V | 06 |
| C-2 governance records vs code | V | 01 |
| H-1 two authority models, tid-scoped revoke | R | 07 |
| H-2 seven path tables + source scraper | R | 05 |
| H-3 ambient `PcieDriverCap` | V | 06 |
| H-4 additive posture, optional guarded copy | R | 04 |
| H-5 syscall monolith, two gates, 62/64 bits | R | 08 |
| H-6 ~12 IPC mechanisms, unbounded Send | R | 11 |
| H-7 global lock, comment-only order | R | 10 |
| H-8 residents vs Spec 15, selftests on boot | V (selftests) / R | 09 |
| H-9 HAL traits not the seam | R | 14 |
| H-10 Tier-2 maturity on parked arch | R | 15 |
| H-11 frame protocols, x86 shootdown panic | V (panic) / R | 16 |
| H-12 Tier-1 TCB perimeter | R | 18 |
| H-13 service loop, wire styles, VFS block, init SPOF, hotswap | R | 12, 13 |
| H-14 no x86 gate, tracked build outputs, recipe duplication, local≠remote contract | R | 02, 03, 11 |
| M kernel shape (`task` catch-all, `Task` struct, raw cfgs, service-aware core) | R | 09, 11 |
| M authority (service rights in CapSet, principal type, fixture rows, audit subject) | R | 05, 07 |
| M IPC/sched (discovery conventions, switch ladders, RT pinning, CPU budget, region tables) | R | 10, 11, 14, 16 |
| M memory (two x86 PT paths, Tier-3 stack, ambient quota, shared-root ASLR/W^X/SMAP) | R | 14, 17 |
| M hardware (board accessors, DMA remapper, NIC scaffolding, NIC singleton, empty ID tables) | R | 05, 14, 18 |
| M userspace (`api` fusion, VFS cohesion, net god-loop, C-runtime tables) | R | 08, 12, 13 |
| M build/repo (disk literals, RV64-shaped checks, `--locked`, CI monolith, artifacts in git) | R | 02, 03 |
| M docs/governance (doc mass, stale architecture docs, spec contradictions) | V | 01, 19 |
| Low (dead residue, junk files, naming, 32-bit targets) | R | 19 (+ decision 4 in 00) |

## Known unknowns

- Whether the C-1 chain is reachable on a shipped image (writable `/home`, `Path` route). Phase 06 step 1 answers it with a negative test before any fix.
- How kernel host tests are invoked (CI step reported "kernel host 497"); each phase reads the `ci.yml` step rather than assuming a command.
- Whether `tests/integration` targets not named in any workflow still pass; Phase 02 measures before gating.
