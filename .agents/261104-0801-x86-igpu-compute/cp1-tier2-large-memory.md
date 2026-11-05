# CP1 — Tier-2 large anonymous memory on x86-64 (prerequisite D-G0)

Status: **Law-1 confirmation 1 APPROVED by the owner on 2026-11-04 ("duyệt"), exactly as the §13 recommendations D-M1…D-M14** ([approval record](cp1-tier2-large-memory-approval.md)). The lock handoff is approved but **not yet effective**. Implementation needs the Intel WIP lane, S1's release of the shared kernel and ABI files, and CP2 afterwards. Sizes (D-M14) are fixed at CP2. Before approval this was a draft, written 2026-11-04 against a dirty working tree (x86-breadth S1 edits `kernel/src/task{,/syscall}.rs`, `libs/api/src/abi/syscall.rs`); every Cellos `file:line` was read in this session and may shift when S1 commits. `[inference]` marks claims not verified in source. Upstream: Mesa `main` @ `e5d557857074767553ccadcc5e80014f86a69d52`, files with the MIT permission notice only (`src/intel/vulkan/anv_allocator.c:1-21`, `anv_private.h:1-6`).

Serves: [ADR-0033](../../docs/decisions/0033-x86-intel-igpu-native-compute.md) §3 (anv runs as a Tier-2 C Cell); [GPU CP1](cp1-gpu-kernel-abi.md) D-G0 option (i), approved as "separate CP1 (prerequisite)" on 2026-11-04 (`cp1-gpu-kernel-abi.md:72-81,293`); [port gaps](research/mesa-anv-port-gaps.md) P2 (`:189-191,242`); [phase G3](phase-03-mesa-anv-tier2.md). Consumer of this primitive in user space: the sibling runtime plan `.agents/261104-0802-x86-tier2-gpu-runtime/` (mlibc `sys_vm_map`/`vm_unmap`/`vm_protect`; path given by its author, not read here).

## 1. Scope

**In:**
- one append-only syscall `AnonMemory = 440` (op-multiplexed: MAP / UNMAP / PROTECT) and a constants module `api::anon_memory` (§6);
- kernel-internal rules (no wire change, listed for review): anonymous mapping kind, VA window, eager commit, per-Cell anonymous-frame budget row, budget source, indexed ledger, batched invalidation, export lock and frame withholding for the GPU CP1 (§5, §7);
- death semantics and the ordering contract with `ExportDmaRange = 437` / `RevokeDmaExport = 439` (§7);
- lock relations (§10).

**Out:**
- x86 Tier-2 **admission** of the inference Cell (profile/artifact decision, D-M10);
- file-backed `mmap`, `MAP_SHARED` between Cells, two VAs aliasing the same frames, `PROT_EXEC`, demand paging, huge pages (D-M3, D-M6, D-M7, D-M12);
- mlibc, C++ runtime, hard-float/XSAVE (owner Q4/Q5, separate plans);
- SAS (Tier-1) callers: they keep `GrantAlloc`/`ShmAlloc`;
- RV64/AArch64 enablement (D-M9).

## 2. Survey: how a Tier-2 Cell gets memory today

| # | Fact | Evidence |
|---|---|---|
| S1 | Highest syscall in code is `AllocPcieIrq = 435`; 436 approved for `SystemPower`, 437-439 approved design for the GPU DMA export; 440+ unused in `libs/api/src/abi/syscall.rs` and both igpu/breadth plan dirs (grep) | `libs/api/src/abi/syscall.rs:684,698`; `cp1-gpu-kernel-abi.md:10` |
| S2 | mlibc (Tier B) anonymous memory = a static 4 MiB bump arena in `.bss`; `AnonFree` and `vm_unmap` are no-ops; `vm_map` accepts only `MAP_ANONYMOUS` and ignores address/prot; the ABI comment covers riscv64/aarch64 only | `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:7-9,30-34,155-187` |
| S3 | Rust Cell heap (`ostd`) is a 1 MiB static arena | `libs/ostd/src/heap.rs:25-28` |
| S4 | `ShmAlloc = 20` allocates **one** frame bound to the caller's incarnation and returns its physical address; `ShmMap = 21` maps that frame at VA == PA through `paging::map_page` (the generic SAS/kernel-root API, which on x86 forwards to `map_page_x86`); no unmap, no size, no domain branch | `kernel/src/task/syscall.rs:6379-6437`; `kernel/src/memory/paging.rs:970-992`; `libs/api/src/abi/syscall.rs:222-223` |
| S5 | `GrantAlloc`: ≤ `MAX_GRANT_PAGES = 4096` (16 MiB) per call, physically contiguous, returns the physical base | `kernel/src/task/syscall.rs:257-259,9206-9300` |
| S6 | **RV64 (and AArch64 test images)**: a domain caller's `GrantAlloc` backing is allocated supervisor-only and mapped into its **own** private root at VA == PA, RW+NX. **x86_64**: the same call returns the denial sentinel `Ok(0)` before any frame exists; the domain grant lifecycle is `cfg`-closed on x86 ("no root-switch witness") | `kernel/src/task/syscall.rs:9215-9267,447-523,327-348` |
| S7 | Private roots are `AddressSpace` objects (generic over RV64/AArch64/x86_64): per-page `ledger: Vec<MappingEntry>`, `frames: Vec<OwnedFrame>`, `table_frames`, ASID/PCID lease; `MappingKind = {Private, ImmutableImage, SharedAbi, Grant}`; `USER_LIMIT = 2^38`; W^X enforced at map time | `kernel/src/memory/address_space.rs:10,100-120,485-504,1476-1484` |
| S8 | `map_private_page` allocates and zeroes one frame per call and checks the ledger by linear scan; `unmap_private_page` linear-scans `frames`, then one tag invalidation **per page**; unconfirmed invalidation defers frames and keeps the VA reserved | `address_space.rs:175-196,703-775,989-1047` |
| S9 | `map_private_page` has **no syscall caller**: only `domain_grant.rs:521` and selftests. A domain root maps only kernel stack, user stack (kind `Private`), ELF segments (`Private` if writable, else `ImmutableImage`) at build time | grep `map_private_page`; `address_space.rs:1206-1251` |
| S10 | Kernel heap is 32 MiB on 64-bit; every heap allocation is charged to the **current Cell's** heap quota | `kernel/src/main.rs:750-756`; `kernel/src/memory/heap.rs:145-148,171-181` |
| S11 | The domain copy boundary (spec 22 §2.4) validates **every page** with `page_proof_for`, a linear ledger scan; x86 shares this probe | `kernel/src/task/user_copy/sv39_probe.rs:150-195`; `address_space.rs:562-566`; spec `docs/specs/22-native-domain-cell-implementation-gate.md:112-129` |
| S12 | x86 #PF: permission faults and no-VMA faults kill the Cell; the VMA demand path maps into the active/SAS root with `map_page_x86`, takes `SCHEDULER` and `FRAME_ALLOCATOR` in the handler and **panics on OOM**; "the VMA list is empty for ELF-backed cells today" | `kernel/src/memory/paging.rs:1494-1503,1555-1607` |
| S13 | Per-Cell quota = heap bytes (`DEFAULT_QUOTA_BYTES = 16 MiB`, boot config may only lower it) plus a DMA row (1× heap quota). Every governed spawn passes `default_quota_bytes()`; there is **no per-Cell memory budget** in the manifest or policy | `kernel/src/memory/cell_quota.rs:27-45,392-449`; `kernel/src/loader/governed_spawn.rs:212,308`; `kernel/src/system_config.rs:31-33` |
| S14 | `CellManifest` v2 is 16 bytes: caps, protection class, `cap_args_off` and `reserved` both "must be 0" | `libs/api/src/abi/manifest.rs:11-22,46-65` |
| S15 | `POLICY.BIN` (signed, per path) is versioned v1-v8 by appending cap bytes; `Permit` intersects | `kernel/src/policy.rs:32-65` |
| S16 | "Durable charge" exists only as host libraries for a proposed schema ("not deployment approval"); no kernel or `libs/api` reference | `libs/durable-charge-codec/src/lib.rs:1-4`; grep `durable_charge` in `kernel`, `libs/api` (none) |
| S17 | Root retirement: member reap (`reap_retired_task_resources` → `teardown_dma_authority` → IRQ waiters → `reap_grants_for_task` → VMs) → VFS holder leases → **then** zombie drop → SHM release → quota row deregistered | `kernel/src/task.rs:1484-1536,1569,2013-2033` |
| S18 | A task's root is `TaskAddressSpace::Domain(Arc<AddressSpace>)`; `AddressSpace::drop` releases the tag first; frames and tables return to the allocator only if acknowledged, else `defer_frames_and_tag` | `kernel/src/task/tcb.rs:179-182`; `address_space.rs:1253-1308` |
| S19 | x86 Tier-2 admission **in code**: switch ordering qualified on x86_64; `enable_for_boot` requires a recorded kernel CR3 and `x86_substrate_qualified` (single hart, or SMP proof); enabled in the **development** profile and in `browser-engine-profile` (artifact gate admits only `/bin/ocel-quickjs` with empty caps); fleet (`policy-required`) stays closed. Domain roots refuse device/DMA authority | `kernel/src/loader/domain_admission.rs:154-158,186-230,352-399`; `kernel/src/main.rs:1269-1320`; `kernel/src/task/smp.rs:735-738` |
| S20 | **Spec 22 status still says x86 admission is closed** ("no non-RV64 image has entered a real domain task") — stale against S19 and the x86 domain lane (`scripts/x86/qemu-domain-test.sh:1-16`) | `docs/specs/22-native-domain-cell-implementation-gate.md:3-16,85` |
| S21 | Allowlist full (0-62 + 63). No-bit precedent for self-only calls: `SetTlsBase`, `FutexWait/Wake`, pipes | `libs/api/src/abi/syscall.rs:1258-1313` |
| S22 | Typed sentinels: `MAX` denied, `MAX-1` try again, `MAX-2` not found, `MAX-3` invalid, `MAX-4` too small; 434/435 add `MAX-5` not supported, `MAX-6` exhausted. The Cell stub carries four arguments | `kernel/src/task/syscall.rs:1725-1746`; `libs/api/src/abi/pcie_function.rs:68-76`; `docs/abi-boundary-records.md:202-203` |
| S23 | GPU CP1 requires of D-G0: export gate accepts "anonymous pages of its own root"; never ELF-segment, grant-received or lease pages; the owner range is **locked** (no unmap / protection change) while a lease is Live or Revoking; no frame list stored ("the locked owner mapping is the source of truth"); owner death revokes leases before the root's frames are released | `cp1-gpu-kernel-abi.md:190-191,244,250` |

Upstream (MIT, Mesa pin):

| # | Fact | Source |
|---|---|---|
| M1 | `BLOCK_POOL_MEMFD_SIZE = 1 GiB` (the comment at the memfd creation still says "2GB") | `src/intel/vulkan/anv_private.h:705`; `anv_allocator.c:132-135` |
| M2 | The state table grows by mapping the **whole** memfd again (`MAP_SHARED|MAP_POPULATE`, size doubles); old maps stay live until `anv_state_table_finish` unmaps all of them; readers may still use an old map | `anv_allocator.c:183-199,236-249` |
| M3 | Placed-map unmap replaces the BO map with `mmap(PROT_NONE, MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED)` (`VK_EXT_map_memory_placed` only) | `anv_allocator.c:1898-1906`; port gaps `:139` |

**RV64/AArch64 vs x86 in one line:** RV64 Tier-2 can get ≤ 16 MiB contiguous identity-VA chunks through `GrantAlloc` (S6) but mlibc still uses the arena (S2); x86 Tier-2 has only `.bss`, the stack and the 4 MiB arena. No architecture has a reserve/commit/protect primitive.

## 3. What is missing

| Need | Today | Gap |
|---|---|---|
| GiB-range anonymous RW memory in a private root | 4 MiB arena (S2); `GrantAlloc` refused on x86 (S6); `ShmAlloc` one frame at a physical VA in the wrong root (S4) | allocate + map into the caller's own root |
| Sparse reserve, commit sub-range, unmap, protect | none | reserve/commit/unmap/protect ops |
| Budget "admitted" for the Cell | only the ≤ 16 MiB heap quota (S13) | a per-Cell anonymous-frame limit and its source |
| Scale to 2^18 pages per GiB | per-page `Vec` ledger, linear scans, per-page shootdown (S7, S8, S11) in a 32 MiB kernel heap charged to the Cell (S10) | indexed ledger, batched invalidation, bounded metadata |
| Exportable via 437 | GPU CP1 needs an "anonymous" kind and a range lock (S23) | `MappingKind::Anonymous`, lock counts, frame withholding |

## 4. Requirements

- **R1** A live x86_64 private-root Cell can reserve VA, commit (eager, zero-filled), unmap and re-protect anonymous memory, page-granular (4 KiB), in the GiB range.
- **R2** Committed frames + their page-table frames never exceed the Cell's admitted anonymous budget; exhaustion is an error at the call, never a later fault, panic or kernel OOM.
- **R3** mlibc maps `mmap(MAP_ANONYMOUS[,MAP_FIXED], PROT_NONE|READ|READ|WRITE)`, `munmap`, `mprotect` 1:1 onto the ops; anv's 1 GiB state-table memfd is served as one reservation with growing commits (M1-M2) — same base on every re-map (shim, not kernel).
- **R4** Committed pages pass the domain copy boundary (S11) and are exportable through 437 under the GPU CP1 rules (S23).
- **R5** No frame returns to the allocator while a CPU translation (any hart) or a VT-d translation (GPU CP1) may still reach it.
- **R6** No physical address is disclosed; no `PROT_EXEC`; nothing enters the SAS root or the `task.vma` demand path (S12).

## 5. Decisions

### D-M1 — Reuse or a new primitive

| Option | Assessment |
|---|---|
| Reuse `ShmAlloc`/`ShmMap` | One frame per call, handle = physical address, VA == PA in the SAS root, no unmap (S4). Would need a new meaning for every argument: a new API under an old number. Rejected |
| Open `GrantAlloc`/`GrantFree` owner-only for x86 domains (RV64 path, S6) | Smallest kernel delta, but ≤ 16 MiB **contiguous** per call (fragmentation for a 4 GiB model: 256 contiguous runs), VA == PA (needs PA < 2^38, discloses physical layout), no reserve/protect, no budget, one grant-table row per chunk, and it reopens the x86 domain-grant `cfg` that phase-01 closed (S6). Viable only as a stopgap |
| **New `AnonMemory = 440`** on `AddressSpace` | The only shape that meets R1-R6. Mechanism exists (`map_private_page`/`unmap_private_page`, S8) but has no syscall |

**Recommendation:** new 440. Nothing existing fits without reinterpreting its wire meaning (owner rule: new syscall only where nothing fits).

### D-M2 — Wire shape

| Option | Assessment |
|---|---|
| **One op-multiplexed `AnonMemory = 440`** (`a0 = op`) | One number; precedents `FileOp = 107` (rename/remove, `libs/api/src/abi/syscall.rs:235`), `GetTime` ops (`generic.cpp:134-145`). Four registers suffice (S22) |
| Three calls `VmMap = 440`, `VmUnmap = 441`, `VmProtect = 442` | Linux-like; three enum arms, three numbers for one authority |

**Recommendation:** one syscall (§6). Numbers 441-442 stay unused and are not requested.

### D-M3 — Eager commit vs demand paging

| Option | Assessment |
|---|---|
| **Eager**: MAP/PROTECT with READ commits (allocate, zero, map) every page before returning; `prot = 0` reserves VA only | Errors surface at the call (R2); every committed page is present for the copy boundary (S11) and for 437 (S23); no #PF work. Cost: no overcommit, so sparse users must reserve then commit (the shim does this for M2) |
| Demand-zero on #PF in reserved ranges | Needs a new domain-aware fault path: today's x86 path takes scheduler/allocator locks in the handler, maps into the wrong root and panics on OOM (S12); every kernel copy and 437 would have to pre-fault; OOM becomes a Cell kill at an arbitrary instruction |

**Recommendation:** eager. A per-call commit cap (`ANON_COMMIT_MAX_BYTES`, placeholder 64 MiB) bounds syscall latency; mlibc loops. Demand paging can be added later behind the same wire (a reserved range simply stops being "PROT_NONE" in a future flag) — not requested.

### D-M4 — Budget source

| Option | Assessment |
|---|---|
| Heap quota row (`cell_quota`) | Wrong unit and capped at 16 MiB by `set_default_quota_bytes` (S13). Rejected |
| `CellManifest` (`cap_args_off`/`reserved`) | Self-declared by the artifact — a request, not an admission; Law-1 manifest change; locked rows `260712-1900-manifest-v2` P03 and `260621-0830-cell-perms-p2-p5` (§10) |
| **`POLICY.BIN` v9**: per-path `anon_limit_mib` (u32 LE) appended to the signed row; absent/older version ⇒ 0 | Operator-signed grant, keyed by the same path that admission checks; follows the v1→v8 append pattern (S15). Format change in `kernel/src/policy.rs` + `scripts/sign-policy.py` (precedent K3, `cell-contract-0024-locks.md:117`) |
| System-config default only | One number for every Tier-2 Cell; too coarse for fleet |

**Recommendation:** policy v9 row is the limit; when no verified policy row names the Cell (development profile), the limit is a new system-config `memory.tier2_anon_default_mib` (beside `default_cell_heap_mib`, S13), default **0** (= op refused `NotSupported`). The limit is bound at publication into a new per-Cell row; it is not re-read at runtime.

### D-M5 — What is charged

**Recommendation:**
- New row `ANON_IN_USE[MAX_CELLS]` in `cell_quota` (beside `DMA_IN_USE`, S13), packed with the row generation like `IN_USE` (`cell_quota.rs:54-73`), so a deferred free after retirement never credits a later incarnation.
- Charged: every committed leaf frame **and** every page-table frame the anonymous window allocates (x86: ≈ 1 PT per 2 MiB → ≈ 2 MiB per GiB).
- Range metadata (extents) stays on the kernel heap and is charged to the Cell's heap quota as today (S10); bounded by `MAX_ANON_EXTENTS` per root (placeholder 1024).
- Global floor: a commit is refused (`OutOfMemory`) if it would leave fewer than `ANON_GLOBAL_FLOOR` free frames (placeholder max(64 MiB, 2 % of RAM)) — keeps the kernel and Tier-1 services alive when several Cells hold budgets that together exceed RAM.
- Exported memory stays charged to the owner; the holder's SLPT pages go to the holder's DMA row (GPU CP1 §5.2).

### D-M6 — Huge pages

**Recommendation:** 4 KiB leaves only in v1. 2 MiB leaves (from `allocate_contiguous_aligned`, `kernel/src/memory/frame.rs:554`) are a later **kernel-internal** optimisation with no wire change; they also help the GPU window (`cp1-gpu-kernel-abi.md:314`). Not in v1 because unmap/protect/export of a sub-2 MiB range would need leaf splitting under SMP invalidation.

### D-M7 — Protections, W^X, `mprotect`

**Recommendation:** `prot ∈ {0 (none), READ, READ|WRITE}`; WRITE without READ and any EXEC bit are `InvalidInput` (`AddressSpace` already refuses W+X, S7; anonymous memory is never executable — Mesa compiles GPU code, not CPU code; JIT is out of scope). PROTECT applies only to pages of the caller's own anonymous ranges; `0 → READ[|WRITE]` commits (eager); `READ[|WRITE] → 0` clears the PTE but **keeps** the frame and its charge (POSIX contents-preserving); downgrades invalidate before returning. ELF, stack, grant and export-locked pages are never re-protected.

### D-M8 — Interplay with the GPU export (437/439)

**Recommendation (kernel-internal):**
- New `MappingKind::Anonymous`. Writable ELF segments and stacks are `Private` (S9), so 437's "anonymous pages only" test is `kind == Anonymous`.
- Each anonymous extent carries an export-lock count. MAP(FIXED)/UNMAP/PROTECT touching a locked page → `TryAgain` (`MAX-1`), no partial effect.
- A frame lookup by VA (PTE walk under the root lock) serves 438 (no frame list, S23).
- A `withhold_range(va, len) -> frames` hook detaches the frames of a lease whose revoke was not acknowledged, so they go to the GPU CP1's `DmaExport{holder,id}` quarantine instead of `AddressSpace::drop` (§7).

### D-M9 — Callers and architectures

**Recommendation:** v1 = live **x86_64** private-root callers only. SAS callers and other architectures get `NotSupported` (`MAX-5`). The code lives on the generic `AddressSpace`, so RV64/AArch64 enablement is a later `cfg` widening plus their own lanes, not a wire change.

### D-M10 — Interaction with x86 Tier-2 admission

Code already admits x86 Tier-2 in development and browser-engine profiles (S19); fleet is closed and the inference Cell (llama.cpp/`ai` + anv) is not an eligible artifact anywhere.

| Option | Assessment |
|---|---|
| **This CP1 changes no admission rule**; the inference Cell's admission (profile + artifact row + policy row with `anon_limit_mib`) is a separate owner decision before G3 | Keeps this package to memory; the budget is inert (0) until such a row exists |
| Add a "GPU inference profile" here | Mixes admission policy into a memory ABI review |

**Recommendation:** separate. Also correct spec 22's status lines (S20) as a docs follow-up — doc drift, not new authority.

### D-M11 — Allowlist

| Option | Assessment |
|---|---|
| **No bit**, dispatch gate = live private root with nonzero budget | Self-only, affects only the caller's own root; precedent `SetTlsBase`/futex (S21). The budget (policy-signed) is the authority |
| Share bit 12 with `ShmAlloc` | Lets a Cell self-deny; but silently grants the op to every Cell already declaring bit 12 |

**Recommendation:** no bit.

### D-M12 — Mesa's 1 GiB memfd, aliasing, file mappings

- **In scope:** reserve 1 GiB (`prot = 0`), commit growing prefixes with `FIXED` inside the reservation (M1-M2); the placed-map replace (M3) as MAP(FIXED, prot 0) over the caller's own range.
- **Out of scope:** two VAs aliasing one frame. M2 is satisfied by the shim returning the **same base** for every `mmap(memfd, 0, size)` and unmapping the reservation only at the last `munmap` [inference: anv never compares map addresses; confirm in G3 drm-shim run]. File-backed `mmap` (llama.cpp model files): llama.cpp's `use_mmap=false` read path [inference]. M3 over an **exported** BO returns `TryAgain` (D-M8), so the shim must not advertise `VK_EXT_map_memory_placed` [inference].

**Recommendation:** no kernel aliasing in v1.

### D-M13 — Ledger representation (kernel-internal, load-bearing)

At 2^18 pages per GiB the per-page ledger costs ≈ 40 B/page [inference from field sizes] ≈ 10 MiB of a 32 MiB kernel heap per GiB, charged to the Cell's 16 MiB heap quota (S10), with O(n) scans on every map and every copied page (S8, S11). **Recommendation:** anonymous memory is recorded as extents `{va, len, prot, state, export_locks}` in an ordered map; the leaf PTEs are the source of truth for frames; `page_proof_for` consults the extent map first (O(log n)); UNMAP/PROTECT clear all affected leaves and issue **one** tag invalidation per op. Existing kinds keep the per-page ledger.

### D-M14 — Placement and limits (placeholders, kernel-internal)

- Anonymous window `[0x30_0000_0000, 0x40_0000_0000)` (192-256 GiB, 64 GiB) below `USER_LIMIT = 2^38` (S7), above the PIE slots (≤ 132 GiB with `large-cell-va`, `kernel/src/loader/va_alloc.rs:44-68`). Window creation asserts disjointness from every existing ledger entry [inference: x86 user stacks lie below it].
- Non-FIXED MAP = lowest-fit in the window. `ANON_COMMIT_MAX_BYTES = 64 MiB` per op, `MAX_ANON_EXTENTS = 1024`, reservation ≤ window.
- Sized from G4 workloads at CP2.

## 6. Wire layout — `AnonMemory = 440`

Class K, register envelope, four arguments (S22). Every op validates the whole range before any effect; on failure nothing changes (a commit that fails midway rolls back the pages it mapped, behind the tag invalidation). Errors use a 434/435-style encoder with constants `api::anon_memory::ERR_*`. A successful return is < 2^38, so it cannot collide with a sentinel.

| Reg | Meaning |
|---|---|
| a0 | `op`: 1 `ANON_MAP`, 2 `ANON_UNMAP`, 3 `ANON_PROTECT`; other values `InvalidInput` |
| a1 | `addr`: page-aligned. MAP without `FIXED`: must be 0 (no hints) |
| a2 | `len`: multiple of 4096, ≥ 4096 |
| a3 | MAP: `prot` (bit0 `PROT_READ`, bit1 `PROT_WRITE`) \| `ANON_FIXED` (bit 8). PROTECT: `prot` only. UNMAP: 0. All other bits 0 |

| Op | Effect | Return |
|---|---|---|
| MAP, no FIXED | reserve `len` in the window; if `prot != 0` commit every page (zeroed) with `prot` | base VA |
| MAP, FIXED | `[addr, addr+len)` must lie in the window and contain only free VA or the caller's own anonymous pages (not locked, not pending invalidation); existing pages are replaced (released, then zero pages committed if `prot != 0`) | `addr` |
| UNMAP | release the caller's anonymous pages in range (free VA holes ignored, POSIX); PTEs cleared and invalidated; frames return and the charge is refunded on acknowledgement | 0 |
| PROTECT | §5 D-M7 on every page; every page in range must be the caller's anonymous memory | 0 |

| Error | Sentinel | When |
|---|---|---|
| `InvalidInput` | `MAX-3` | bad op, alignment, zero/overflowing length, unknown bits, WRITE without READ, EXEC bit, hint without FIXED, FIXED outside the window, commit > `ANON_COMMIT_MAX_BYTES` |
| `PermissionDenied` | `MAX` | range touches a non-anonymous mapping (ELF, stack, grant, shared ABI, kernel) |
| `TryAgain` | `MAX-1` | page export-locked (437 lease Live/Revoking) or VA pending invalidation from an earlier unmap |
| `NotSupported` | `MAX-5` | caller is not a live x86_64 private root, root dying, or budget is 0 |
| `OutOfMemory` | `MAX-6` | budget, global floor, frames, window space or extent table exhausted |

Constants (`libs/api`, new `abi/anon_memory.rs`): `ANON_MAP = 1`, `ANON_UNMAP = 2`, `ANON_PROTECT = 3`, `PROT_READ = 1`, `PROT_WRITE = 2`, `ANON_FIXED = 1 << 8`, `ERR_PERMISSION_DENIED = MAX`, `ERR_TRY_AGAIN = MAX-1`, `ERR_INVALID_INPUT = MAX-3`, `ERR_NOT_SUPPORTED = MAX-5`, `ERR_OUT_OF_MEMORY = MAX-6`. `ostd`: `sys_anon_map`, `sys_anon_unmap`, `sys_anon_protect`. Allowlist: no bit (D-M11). Boundary row in `docs/abi-boundary-records.md` §7 at CP2.

Informative mlibc mapping (owned by the runtime plan): `sys_vm_map` → MAP (`MAP_FIXED` → `ANON_FIXED`; `PROT_EXEC` → `EINVAL`; non-anonymous → `EINVAL`); `sys_vm_unmap` → UNMAP; `sys_vm_protect` → PROTECT; `sys_anon_allocate/free` → MAP RW / UNMAP; `MAX-6` → `ENOMEM`, `MAX-3` → `EINVAL`, `MAX` → `EACCES`, `MAX-1` → `EBUSY`, `MAX-5` → `ENOMEM`.

## 7. Failure and death semantics

| Event | Ordering / effect | If invalidation is not acknowledged |
|---|---|---|
| UNMAP / PROTECT downgrade | clear leaves → one tag invalidation (`confirm_tag_invalidation`, as `address_space.rs:1029`) → release frames, refund charge | frames to `deferred_release` (`address_space.rs:1035-1040`), VA stays reserved (later FIXED → `TryAgain`), charge refunded when the reaper releases them |
| Commit fails midway | roll back this op's pages | same as above; error returned is the original one |
| Fault on reserved (`prot = 0`), downgraded or unmapped page | existing containment: Cell killed (S12); anonymous ranges never enter `task.vma` | — |
| Worker thread exits | nothing (root `Arc` shared, S18) | — |
| **Owner Cell dies** | (1) GPU CP1 step: revoke every export lease (first step of `reap_retired_task_resources`, S17); (2) for each lease whose revoke is unacknowledged, `withhold_range` moves its frames to the `DmaExport{holder,id}` quarantine; (3) existing member reap; (4) zombie drop → `AddressSpace::drop` releases tag, then remaining anonymous frames and tables (S18); (5) anon row refunds are generation-tagged, row deregistered with the quota row (`task.rs:1569`). This **confirms the GPU CP1 inference** (`cp1-gpu-kernel-abi.md:244,310`): root frames are released after `reap_retired_task_resources`, at zombie drop (`task.rs:1493-1528`) | (2) quarantined until the holder's revoke is acknowledged; (4) whole root deferred (`address_space.rs:1289-1306`). Death is never delayed |
| Holder (GPU driver) dies | GPU CP1 §6: leases dropped after its domain teardown acknowledges → owner extents unlocked | locks stay; owner's UNMAP gets `TryAgain` |
| Budget/policy row removed while running | no effect until next publication (limit bound at publication) | — |

Invariants (tested, §8): **I1** no anonymous frame reaches the allocator while any CPU tag or VT-d window may translate to it; **I2** committed + table frames ≤ limit at every instant; **I3** no anonymous mapping exists in the SAS/kernel root; **I4** no op has a partial effect on error.

## 8. Test plan

**Host (`cargo test`, pure modules):**
- `api::anon_memory`: op/prot/flag decode; refuses WRITE-only, EXEC bit, hint without FIXED, nonzero UNMAP `a3`, unknown bits; sentinel values.
- New extent manager: lowest-fit placement, split/merge on partial UNMAP/PROTECT, FIXED replace over own range, refusal over foreign kinds, lock counts → `TryAgain`, extent-table and window exhaustion, pending-invalidation VA reuse refusal.
- Budget row: charge/refund incl. table frames, generation-tagged refund after re-registration (mirror `heap.rs:490-558`), global floor.
- Policy v9 parse: `anon_limit_mib` present/absent, v1-v8 decode as 0, signature still covers it.

**QEMU x86_64 q35** (domain test image `scripts/build-x86_64-domain-test-ci.sh` + `scripts/x86/qemu-domain-test.sh`, `-m 4G`; TCG and local KVM; 1 and 2 harts). Test-hooks Tier-2 probe Cell with a 1 GiB development budget:

| Lane | Steps | Pass |
|---|---|---|
| A1 reserve/commit | reserve 1 GiB; commit 16 × 64 MiB with FIXED; pattern write/read; `Write` syscall from a committed buffer (copy boundary) | bytes intact; log `[anon] commit` totals; time per GiB reported (no target) |
| A2 budget | 256 MiB budget: commit past it | `MAX-6`; after UNMAP the same commit succeeds; free frames return to baseline |
| A3 protect | RW→R then store; NONE→RW commits; EXEC bit | contained fault (one `[fault] Cell` line); commit works; `MAX-3` |
| A4 death | `ForceExit` with 512 MiB committed | `DOMAIN-FRAME-RELEASE: PASS … quarantined=0`; anon row 0; frames back to baseline |
| A5 SMP | 2 harts (KVM): thread on hart B reads in a loop while hart A unmaps | B faults (contained), never reads a reused frame; one invalidation per op (`S22-X86-REMOTE-SHOOTDOWN` pattern, `kernel/src/task/smp.rs:1156`) |
| A6 refusals | SAS caller; FIXED over ELF/stack; unaligned; budget 0 | `MAX-5`, `MAX`, `MAX-3`, `MAX-5`; no mapping change |
| A7 unacked | `set_test_withhold_tag_ack` (precedent `kernel/src/memory/address_space_tests.rs:327`) during UNMAP | frames deferred, FIXED over the VA → `MAX-1`, released after ack |
| A8 export (after GPU CP1 lands) | 437 on an anonymous range; UNMAP/PROTECT; 439; owner death with a live lease (GPU CP1 L3 `edu` lane) | `MAX-1` while leased; success after revoke; frames withheld until ack |

Ceiling: QEMU TCG/KVM. Physical anv/llama.cpp use is G3/G4 (S2).

## 9. Files a later implementation would touch

- **ABI (Law 1, CP2):** `libs/api/src/abi/syscall.rs` (`AnonMemory = 440`, raw-number arm, no-bit allowlist arm with comment); new `libs/api/src/abi/anon_memory.rs`; `libs/api/src/abi.rs` (module); `libs/ostd/src/syscall.rs` (wrappers).
- **Kernel:** `kernel/src/task/syscall.rs` (decode, dispatch gate, encoder); new `kernel/src/memory/anon_range.rs` (extents, window, host tests); `kernel/src/memory/address_space.rs` (`MappingKind::Anonymous`, range map/unmap/protect with one invalidation, extent-aware `page_proof_for`, `withhold_range`); `kernel/src/task/user_copy/sv39_probe.rs` (only if the proof lookup signature changes); `kernel/src/memory/cell_quota.rs` (anon row); `kernel/src/policy.rs` (v9 field); `kernel/src/system_config.rs` + its config schema (`tier2_anon_default_mib`); `kernel/src/task/launch.rs` (bind limit at publication); `kernel/src/task.rs` (retirement tally only; the lease-revoke step is GPU CP1's); `kernel/src/cell/retirement_verdict.rs` (`RetirementTally` anon field; used at `kernel/src/task.rs:1488`).
- **Tools/tests:** `scripts/sign-policy.py`; `scripts/x86/qemu-domain-test.sh`, `scripts/build-x86_64-domain-test-ci.sh`; one probe Cell under the test-cell convention at implementation time.
- **Docs:** `docs/abi-boundary-records.md` §7, `docs/specs/22-native-domain-cell-implementation-gate.md` (§2.1 ledger note; status drift S20), `CHANGELOG.md`, this plan's G3 phase file.
- **Not here:** `third_party/mlibc/sysdeps/vicell/**` (runtime plan), any `iommu*` file (GPU CP1).

## 10. Lock relations and scheduling

- **WIP:** x86-breadth S1 holds the Intel lane and edits `kernel/src/task/{syscall,task}.rs` and `libs/api/src/abi/syscall.rs` (`cp1-pcie-irq-power-abi.md:122-131`; `cp1-gpu-kernel-abi.md:283`). This package needs its **own** file-bounded exception, effective only after this CP1 is approved and the owner gives it the lane. It touches no `iommu*` file, so it does not wait for breadth 12, but must land before the GPU CP1's client-side export (437 on Tier-2) and before G3.
- **CELL-CONTRACT-0024 controlling areas** (`.agents/cell-contract-0024-locks.md:95-105`): P02 (Cell substrate, domain roots) owns `AddressSpace`, extents and the budget row; P03 (SMP linearization) the cross-hart invalidation of UNMAP/PROTECT; P05 (resource handles, revocation, DMA) the export lock/withhold contract; P06 (lifecycle) the death ordering and quarantine; P07 (POSIX facade) the mlibc mapping. P08's durable application-wide budget is cluster-scoped and unused here (S16).
- **Rows to rule compatible or narrow at handoff** (`cell-contract-0024-locks.md:206,196,193,188,181,204,208,172`): `260927-0739-kernel-architecture-repair` (grant/domain slices — `address_space.rs`, grant reap), `260821-0642-app-tiers-completion` (admission, native-domain, manifest), `260808-1544-wx-cross-hart-tlb-shootdown` (cross-hart permission retirement), `260722-0917-g4-full-std-tier1` (std/PAL memory contract), `260707-1726-g2-loader-redesign` (address-access), `260827-1015-sas-caller-owned-range-ledger` (owner-scoped ranges), `261004-1957-x86-pc-lane`, `260528-2016-vicell-full-implementation`. Only if D-M4 picks the manifest: `260712-1900-manifest-v2` P03, `260621-0830-cell-perms-p2-p5`, `260822-phase08-manifest-predesign` (`:186,179,199`).
- **Frozen surface:** the 440 enum addition must leave `scripts/check-multi-source-wait-law1-digests.sh` green (no change to `WaitCompletion`/`completion`/`async_ipc`).
- **Not authorized by this draft:** any source or policy-format edit, the number 440 (provisional until CP2; recheck collisions then), admission changes, a lock handoff.

## 11. Risk assessment

| Risk | Mitigation |
|---|---|
| Kernel heap (32 MiB) or Cell heap quota exhausted by per-page metadata | D-M13 extents; `MAX_ANON_EXTENTS`; A1 measures heap use at 1 GiB |
| Long syscalls zeroing GiBs | per-op commit cap (D-M3); A1 reports time |
| Several budgets exceed RAM | global floor (D-M5); budgets are operator-signed |
| SMP stale translation on unmap/protect | one tag invalidation per op before release; deferred release on no-ack (I1, A5, A7) |
| S1 edits the same files | own handoff after S1 lands (§10) |
| Spec 22 vs code drift misleads reviewers | D-M10 docs follow-up |
| Shim same-base memfd assumption wrong | G3 drm-shim run on Linux first (port gaps §7); fallback: Mesa patch shrinking `BLOCK_POOL_MEMFD_SIZE` or a later aliasing op (new CP1) |

## 12. Security considerations

- Authority is the caller's own root only; no cross-Cell mapping is created (export stays the GPU CP1's reviewed authority).
- Frames are zeroed before mapping (`address_space.rs:186-194`); no physical address leaves the kernel (unlike `GrantAlloc`/`ShmAlloc`, S4-S5).
- W^X kept: no EXEC on anonymous memory; ELF/stack/grant pages cannot be re-protected (D-M7).
- Denial of service bounded by the per-Cell budget, the global floor and extent limits; exhaustion is an error, never a kernel panic (contrast S12).
- Nothing enters the SAS root or the x86 VMA demand path (I3).

## 13. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| D-M1 | New syscall vs reuse `ShmAlloc`/`GrantAlloc` | New `AnonMemory = 440` |
| D-M2 | One op-multiplexed syscall vs three (440-442) | One |
| D-M3 | Eager commit vs demand paging | Eager; reserve with `prot = 0`; per-op commit cap |
| D-M4 | Budget source | `POLICY.BIN` v9 `anon_limit_mib`; dev fallback system-config default 0 |
| D-M5 | Charge frames + table frames to a new generation-tagged anon row; global free-frame floor | Yes |
| D-M6 | Huge pages | 4 KiB only in v1 |
| D-M7 | `prot ∈ {none, R, RW}`, no EXEC, protect own anonymous pages only | Yes |
| D-M8 | `MappingKind::Anonymous`, export lock → `TryAgain`, frame withholding for unacked revoke | Yes |
| D-M9 | v1 scope: x86_64 private roots only; SAS/other arches `NotSupported` | Yes |
| D-M10 | Admission of the inference Cell decided separately; fix spec 22 status drift | Yes, separate |
| D-M11 | Allowlist | No bit |
| D-M12 | No kernel aliasing; memfd served as one reservation (same base) by the shim | Yes |
| D-M13 | Extent-indexed ledger + one invalidation per op (kernel-internal) | Yes |
| D-M14 | Window 192-256 GiB, 64 MiB/op, 1024 extents (placeholders) | Yes, size at CP2 |

## 14. Not checked / open questions

- Which root `ShmMap` publishes into for a **domain** caller on x86 (S4: generic kernel-root API, no domain branch). If reachable from a Tier-2 Cell it maps a USER page outside the caller's root; worth a separate containment check — not fixed or claimed here.
- Exact x86 private-root VA layout of user stacks (D-M14 disjointness assumed, asserted at implementation).
- Whether anv tolerates identical addresses for successive state-table maps (D-M12) — G3 drm-shim run.
- Whether the policy v9 format change is reviewed as Law 1 (it lives in `kernel/src/policy.rs`, not `libs/api`); treated here as a reviewed format change like K3.
- Placeholder limits and the per-Cell budget for the Haily inference Cell (model + GPU buffers both count under D-G1 B) come from G4 measurements.
