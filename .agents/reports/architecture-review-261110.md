# Cellos — Whole-Architecture Review

**Date:** 2026-11-10 · **HEAD:** `8a005f998` (main) · **Mode:** codebase, findings only (no fixes, nothing built or run)
**Method:** 7 parallel read-only subsystem reviews plus a lead pass over docs/governance. Findings marked ✓ were re-read against source by the lead; the rest carry the reviewer's `file:line` evidence and are not independently re-verified. `[INFERENCE]` marks conclusions drawn from source reading that were not exercised.
**Relation to prior work:** the STRIDE audit `security-261105-1635-cellos-full-audit.md` is not repeated; it is cited only where a bug class has a structural cause.

Severity: Critical = the structure invalidates a stated core guarantee; High = structural cause of recurring defects or a blocker for the stated direction (ADR-0022, Intel x86-64 C2C); Medium = drift/maintainability cost; Low = hygiene.

**Totals:** 2 Critical · 14 High · 22 Medium · 8 Low (after merging cross-slice duplicates).

---

## 1. Critical

### C-1 Launch authority is keyed on a task name the spawner chooses ✓
- `kernel/src/loader/governed_spawn.rs:201` — task name is `path.rsplit('/').next()`.
- `kernel/src/loader/launch_profile/mod.rs:59-103` — `authorize` matches on `caller.name`; `"cellbox"`, `"terminal"`, `"desktop"`, `"pipe-test"`, `"c-spawn"`, `"periph-demo"` need no capability.
- `mod.rs:33-35` + `governed_spawn.rs:148` — child ceiling is "deliberately independent of the caller capability snapshot"; `Spawner::Ceiling` is not intersected with the caller's caps. `Spawner::User` (the only `manifest ∩ spawner` variant) is constructed only in tests.
- `launch_profile/targets.rs:155` — any `/apps/`, `/data/bin/`, `/home/` path is an admitted target.
- Same name key gates raw power-off (`task/syscall.rs:2219-2231`) and supervisor hotswap/power (`cells/services/supervisor/src/main.rs:182-195`).
- VFS documents the opposite rule: a name "is the last component of the path_hint its spawner passed, which the spawner chooses freely" (`cells/services/vfs/src/access/rules.rs:13-18`).

**Consequence:** `task/cap.rs:216-221` and `docs/security-model.md:254` promise monotonic delegation. As built, a capability-free cell whose basename matches a reviewed row selects that row's child ceilings. `[INFERENCE]` exploit chain (`/home/x/cellbox`) not exercised; the `Elf` route does refuse capability-bearing ceilings (`mod.rs:111-116`), the `Path` route does not.
**Fix:** key edges on the kernel-measured image identity that already exists (`api::cell_image`, used by `vfs/src/access/srv-owner.rs:66-77`). Table-key change, no persistent state; undo by restoring the name match.

### C-2 Governance records assert completion that the code contradicts ✓
- `.agents/261011-1200-cell-contract-authority/plan.md:4-5,78-87` — `status: complete`, all ten phases complete, P10 "200+ clients witness, AC-43 autospawn/drain, CP2 ratified". `.agents/plan-portfolio.md:29` repeats it.
- Same plan body: "All ten phases remain pending" (`plan.md:206`, `228`, `238`). `phase-10-qualification.md:4` is `status: complete` with every success criterion unchecked (`:45-49`).
- `docs/system-architecture.md:20,566,575` and `docs/roadmap/current-focus.md:40` still say pending / "Spec 26 selected design only".
- Code: `cells/services/{budget-authority,workload-router,placement-controller}/src/main.rs:25-31` are heartbeat loops with no IPC; 157 / 302 / 297 lines total; no consumer anywhere outside root `Cargo.toml`. `request_reservation` always returns `Ok`, `OriginProof::verifies` is vacuously true (reviewer evidence: `placement-controller/src/budget_client.rs:28-35`, `workload-router/src/admission.rs:19-20,36`).
- CSpace (P02 "CapTable cutover") is used in production only for BootFS file tickets (`task/syscall.rs:8094-8160`); its control dispatchers have test callers only (`cell/cspace.rs:830,938`).

**Consequence:** the project's evidence ladder (`none → contract → host → QEMU → physical`) is its main safety argument. The controlling plan — the document that gates 38 locks — cannot be used to decide what is implemented. Budget Authority is the declared enforcement TCB and exists as a fail-open skeleton.
**Fix:** reset the phase table to observed state; make `status:` generated from checked criteria; remove the three skeleton crates from the workspace or reduce them to doc-tested pure functions with fail-closed defaults.

---

## 2. High

| # | Finding | Key evidence | Structural fix |
|---|---|---|---|
| H-1 | **Two authority models, one write-only.** Caps are copied per thread; `CapRevoke` clears one tid; CSpace ceiling is written, never read for a decision. | `task/syscall.rs:1977-1988, 7101-7152`; `scheduler.rs:1171`; `cell/cspace.rs:404-405` | One per-incarnation record behind `Arc` from every TCB, keyed `(cell_id, generation)`; delete `apply_to`/`of_task`/`from_task`. |
| H-2 | **Seven path-keyed tables must agree; a Python script scrapes Rust source to sign policy.** 149 distinct `"/bin/…"` literals in the kernel. | `task/cap.rs:469-626`; `loader/boot_ceiling.rs:56-253`; `launch_profile/{profiles,targets}.rs`; `policy.rs:1063`; `scripts/sign-policy.py:489-524`; `task/drivers/pcie_ecam.rs:42-52` | One declarative catalog (path → request, ceiling, edges, PCI IDs) consumed by kernel and signer. |
| H-3 | **PCIe device authority is ambient.** `PcieDriverCap` + "unowned or mine" is the whole check; 12 paths hold the cap. ✓ | `task/syscall.rs:2611-2613`; `pcie_ecam.rs:1553-1574`; `task/cap.rs:473-489` | Require `(vendor, device, class)` to belong to the caller's admitted row at handout. |
| H-4 | **Trust posture is additive features, not one selector.** Only `production-relay-image` rejects dev/test features; policy key is a zero placeholder outside dev; owner gate permanently permissive; guarded user-copy is optional (`native-domains` off ⇒ raw memcpy). Release posture is never booted. | `kernel/src/main.rs:17-29`; `policy.rs:115-118`; `admission/gate.rs:24,59-62`; `task/copy_glue/mod.rs:245,296`; `kernel/Cargo.toml:108-111,172-175` | `build.rs` derives exactly one posture cfg and panics on mixes; guarded copy unconditional on MMU targets; a boot lane for the release posture. |
| H-5 | **Syscall surface is one 13,924-line file with seven hand-synchronised tables and two allowlist gates.** 153-arm match; the `u64` allowlist already uses 62 bits for 142 syscalls. | `task/syscall.rs:5470-10724, 10772, 11448-11540`; `libs/api/src/abi/syscall.rs:8,1196,1442` | One declarative syscall table generating enum/decoder/reverse map; single gate; split arms by domain. |
| H-6 | **No single IPC model.** ~12 mechanisms from three generations; 11 parked `TaskState` variants; unbounded blocking `Send` is the default (67 `sys_send` sites in services/drivers); the kernel itself blocks on VFS in `Rename`. | `libs/api/src/abi/syscall.rs:10-30,934-957`; `task/tcb.rs:40-137`; `task/syscall.rs:7573-7596`; `docs/specs/17-ipc-wire-contract.md:181-184` | Exact-operation + `WaitCompletion` as the only request/reply model; raw Send/Recv event-only with a mandatory bound. |
| H-7 | **One global `SCHEDULER` lock, order enforced by comments.** 336+ lock sites; `reply_in_sched` scans every task; deadline sweep on hart 0 only; lowest primitive calls up into `task::smp`. | `task.rs:548`; `task/async_ipc.rs:626-646`; `scheduler.rs:2185-2189,2262`; `sync.rs:55`; 51 "Lock order" comments | `const RANK` on `Spinlock` with a test-hooks held-rank assert; token→owner index; deadline heap. |
| H-8 | **Kernel residents no longer match Spec 15; selftests run on every production boot.** ✓ PS/2, fb console, ACPI power/SCI/HPET/SPCR, SMBIOS, a kernel FD/cwd layer and `snapshot.rs` (5,769 lines, disabled in every profile) are in-kernel. The size metric drops any file whose stem contains "test". | `main.rs:1062-1125`; `task/drivers.rs:11-56`; `snapshot.rs:333`; `docs/specs/15-kernel-boundary.md:162,218-235,282-283`; `scripts/generate-code-metrics.py:103-104` | Checked-in resident manifest with a CI guard; `#[cfg(feature="test-hooks")]` on a `selftest/` root; cfg-gate `snapshot`. |
| H-9 | **HAL traits are not the seam.** `Timer`, `InterruptController`, `BootController` have zero implementors; kernel names `hal::aarch64::` 103× and `hal::x86_64::` 36×; 1,807 `target_arch` lines, riscv64 (667) ahead of x86_64 (630); `kmain` is one 1,630-line function with 124 cfgs. | `hal/traits/{timer,interrupt,arch}/src/lib.rs`; `kernel/src/main.rs:151-1781`; `docs/specs/04-hardware.md:7-15,89` | `kernel/src/arch/{x86_64,aarch64,riscv64}` with one cfg at `mod arch`; delete unimplemented traits. |
| H-10 | **Tier-2 maturity sits on the parked architecture.** Domain grants compile only for RV64/AArch64 test images; x86-64 Tier-2 cells have copied IPC only; the "arch-neutral" copy boundary speaks Sv39 on x86. | `kernel/src/task.rs:14-21`; `task/syscall.rs:385-393`; `task/user_copy/sv39_probe.rs:18,99`; `memory/address_space.rs:10` | Port grant lifecycle and negative fixtures to x86-64 before adding Tier-2 consumers. |
| H-11 | **Frame lifetime is nine hand-rolled protocols; x86 shared-root shootdown panics on a silent CPU.** ✓ (panic) | `memory/tlb_shootdown.rs:378,460-465`; `memory/{deferred_release,pin}.rs`; `task/{dma_export,domain_grant,stack}.rs` | One `FrameRun` owner type whose `Drop` does unpublish → flush → wait → normalise → free; on timeout defer, never panic. |
| H-12 | **Tier-1 wall perimeter is a convention.** Unsafe scan covers `cells/` only; `libs/ostd` (231 `unsafe`) and `libs/api` (164) link into every cell; no gate ties a `c-ffi` allowlist class to a Tier-2 manifest. | `scripts/cellos_sign/__init__.py:31`; `docs/specs/16-rustc-tcb.md:40-42,143-144`; `loader/governed_spawn.rs:68-70` | Signer refuses `c-ffi` crates without an FFI/UNTRUSTED manifest; publish and scan the real TCB closure. |
| H-13 | **Userspace services re-implement the platform.** Service loop copy-pasted per service (SDK `run_service`/`service_entry!` have zero users); five wire styles with opcodes duplicated in clients and the kernel; VFS blocks unbounded on the block driver and never heartbeats; init is an unrestartable SPOF hosting endpoints and fixtures; hotswap/state-transfer is wired to no production service. | `libs/ostd/src/dispatch.rs:73-77`; `vfs/src/blk_router.rs:80`; `vfs/src/main.rs:177-259`; `net/src/tls_wire.rs:8-10` vs `libs/ostd/src/tls.rs:28-31`; `kernel/src/acpi/sci.rs:40`; `init/src/main.rs:60-66,149`; `supervisor/src/main.rs:171-179` | One `ostd::service::serve(handler)`; one proto module per service; bounded block receive; shrink init to spawn+supervise. |
| H-14 | **The stated direction has no per-commit gate, and the kernel binary is not a function of HEAD.** Two-node C2C and x86 domain tests appear in no workflow; 53 of 71 lane scripts unreferenced; release workflow builds an RV64 ELF with a stub ramdisk; `kernel/src/embedded*` holds tracked, CI-overwritten binaries selected by fallback chain; RV64 image recipe duplicated in PowerShell and bash "byte-for-byte in sync" by comment. Local and remote calls do not share a contract in code (`RemoteEndpoint::call` ⇒ `NotSupported`; outcomes collapsed to `ViError::IO`). | `.github/workflows/ci.yml:450-453,956-976,2149`; `release.yml:57-68`; `kernel/build.rs:163-250`; `scripts/gen-disk-ci.sh:135,307`; `libs/ostd/src/cluster_endpoint.rs:133-141,199-206` | One `x86-c2c` CI job + lane manifest; explicit `EMBEDDED_DIR` under `build/`, untrack binaries; one Python assembler per arch; `CellMethod` as the single SDK call surface with typed outcomes. |

---

## 3. Medium (22, grouped)

- **Kernel shape:** `task/` is a catch-all (55% of the kernel; IOMMU and PCIe live under `task::drivers`); `Task` is a 375-line struct; `cellos_p01_trace`/`cellos_g1_cpu` raw cfgs at 478 sites on top of 36 features; `memory` calls back into `task::*`; kernel is service-aware through an inert `fast_ipc` module (`async_ipc.rs:640`, `tcb.rs:925`).
- **Authority:** `CapSet` doubles as a carrier for service rights (each new one costs a policy wire version, v5–v8); no single principal type (tid vs `(cell, generation)` vs name vs measured image); fixture authority compiled into production tables (30 `test-hooks` cfgs); audit records lack an incarnation subject (`governed_spawn.rs:51-120`).
- **IPC/sched:** four service-discovery conventions (134 tid-only lookups vs 57 bound); context-switch contract inlined as cfg ladders in a 540-line `yield_cpu`; all RT tasks pinned to hart 1; CPU budget not in the default build; five region tables plus dead per-task grant table.
- **Memory/tiers:** x86 has two page-table paths and walkers copied into `task/`; Tier 3 is a third parallel stack with a dead HAL trait and no quota on guest RAM; quota charges whichever cell is current (14 pin-to-0 workarounds); identity-mapped shared root caps ASLR/W^X/SMAP by construction.
- **Hardware:** three board-selection models with four accessor names; DMA remapper API is VT-d-shaped with an empty second design on aarch64; NIC cells copy scaffolding (5 cells within 6–38 lines of each other); NIC role is a last-writer-wins singleton; five packaged NIC drivers have empty ID tables.
- **Userspace:** `api` fuses frozen ABI + evolvable wire + a libc and the kernel links all three; VFS owns SSH trust and calls up into input; net is a 13-argument god-loop with oracles in the hot path; three C-runtime paths and four hand-copied syscall tables, one RISC-V-only.
- **Build/repo:** disk layout literals retyped in 7 tools (`tools/write-mbr.py:11` is stale); workspace clippy and baseline are RV64-shaped; `--locked` appears zero times in `ci.yml`; `ci.yml` is a 27-job, 2,447-line monolith with no `concurrency`; git stores artifacts (49.9 MB `gh`, 28.8 MB WAD, `limine/` objects, 271 files under `build/`).
- **Docs/governance (lead):** 193k lines of tracked Markdown under `.agents/` (7,346 files) against 636k lines of Rust; 186 of 566 commits in the last 30 days are docs-only; `docs/system-architecture.md` last updated 2026-10-11 and `current-focus.md` 2026-10-19 while the plan flipped on 2026-10-16; specs 02/03/04/07/09/10/15/18/19/22 each contradict current code on at least one point; spec number 14 is used twice.

## 4. Low (8)
Dead residue (`CELL_REGISTRY`, `Spawner::User`, `ViGlobalMemoryManager`, `spawn_from_file` stub, orphaned `force_unlock_locks`); `async-trait` unused in kernel; `PAGE_SIZE` redefined in four files; root junk tracked (`invalid.json`, `dummy.jsonl`, `sha256:…jsonl`, `__build*.bat`, `doom_build_debug.txt`); `Dockerfile` references nonexistent paths; "Supervisor" names three things; `CONTRIBUTING.md:118` points at an absent directory; 32-bit targets wired but unbuilt.

---

## 5. Strengths
- Exact-operation IPC primitive is coherent: storage reserved at submit, token bound to owner and peer incarnation, cancel-on-drop in the SDK (`task/async_ipc.rs:9-25`; `libs/ostd/src/ipc.rs:306-318`).
- One admission funnel with re-check under lock at publication (`loader/governed_spawn.rs:36`; `task/launch.rs:606-637`).
- User bytes move through one two-pass boundary with a recoverable fault guard (`task/user_copy/mod.rs:1-37`).
- `AddressSpace` is a real ownership object; private-root release never frees on timeout (`memory/address_space.rs:344-480`; `memory/deferred_release.rs:14-24`).
- Services authorise on kernel-attested `(cell, generation)`, never request bytes (`vfs/src/caller.rs:55-66`; `kms/src/auth.rs:137-146`).
- Layering `types ← api ← ostd` is acyclic; one Rust source for syscall numbers shared by kernel and SDK.
- Cheap machine-checked guards run before any build: writable-global inventory, VA layout, config parity, spec anchors (`ci.yml:44-90`).
- Storage, NIC and GPU drivers are out of the kernel; the aarch64 backend split landed as ADR-0029 describes.

## 6. Suggested order
1. **C-2** — make the plan and architecture docs state what exists. Everything below depends on a trustworthy baseline.
2. **C-1 + H-2 + H-3** — one catalog, keyed on measured identity; this also removes the per-driver kernel edits.
3. **H-4** — one posture selector, guarded copy unconditional, boot the release posture.
4. **H-14** — an x86 C2C gate in CI and explicit embed inputs, so the next steps are regression-protected.
5. **H-5 + H-8** — generated syscall table, split the file, gate selftests and snapshot. Pure refactors.
6. **H-1 + H-6 + H-7** — one authority record per incarnation; one IPC operation model; lock ranks.
7. **H-13** — shared service loop and proto modules; bound VFS's block wait.
8. **H-9 + H-10 + H-11** — `arch/` quarantine; port Tier-2 grants and fixtures to x86; `FrameRun`.
9. **H-12** — tie allowlist classes to manifest tiers; scan the real TCB.

## 7. Not covered
Line-level correctness of any subsystem; HAL arch internals (paging, traps, VMX/SVM, GIC); `viui`, apps, demos, vendored engines; net-broker crypto; branch-protection settings and measured CI runtimes. Nothing was built, booted or tested, so every runtime consequence above is derived from source.

## 8. Delegation record
- `Delegate: haily-reviewer ×7 | reason: parallel | trigger: 636k-line tree, seven independent subsystems (kernel boundary, authority, IPC/sched, memory/tiers, HAL/boards, userspace SDK, build/CI)`
- Full per-slice outputs: `agent://KernelBoundary`, `agent://AuthorityModel`, `agent://IpcSchedSmp`, `agent://MemoryTiers`, `agent://HalBoards`, `agent://UserspaceSdk`, `agent://BuildCiRepo` (session-scoped).
