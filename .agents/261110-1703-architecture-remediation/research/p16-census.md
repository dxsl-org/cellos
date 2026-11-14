# Phase 16 census: frame ownership and shootdown contract (S7c step 1)

Measured read-only on worktree `p16-frame-run` at `94536305eb`. Paths are relative to `kernel/src/` unless they start with `hal/`, `scripts/`, `tests/`, `cells/`, `libs/`, `docs/` or `.github/`. **[I]** marks an inference that was not executed or observed at runtime. Everything else was read in the cited lines.

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| F1 | **Claim (a) is false.** At least seven different conditions gate a free: a confirmed private tag, a synchronous shared-root ack, explicit pin/lease release keys, an IOTLB drain ack, the domain-record gate, SAS copy windows and copy-reader counts, and "VM not running" plus entry-time invalidation. Some retentions keep only VAs, not frames. | §1, §6 |
| F2 | **On x86, "no flush under `FRAME_ALLOCATOR`" is broken today, not just a comment.** x86 `map_page` always does a global shootdown (`memory/paging.rs:910-918`), and it is called with the allocator held by Stack alloc/free, `alloc_grant_pages`, `shm_map`, the loader unwind and MMIO maps. `unmap_page` flushes while holding `KERNEL_ROOT` (`memory/paging.rs:733-742`). | §1.2 |
| F3 | **x86 `Stack::drop` does one broadcast per page, under the allocator, and leaves a lower-half f→f kernel-RWX alias.** This breaks the x86 no-alias rule that grants follow. | `task/stack.rs:80-91,433-458`; `memory/paging.rs:917`; rule at `task/syscall/grant.rs:321-326,1302-1305` |
| F4 | **`CellSegments` does two broadcasts per page on x86.** `eager_unmap` runs under `SCHEDULER`, and nothing waits for SAS copy windows. | `task/stack.rs:614-659`; `memory/paging.rs:740-742`; `sched/scheduler.rs:1796-1802` |
| F5 | **Existing retry machinery is weak.** The deferred reaper only steps the queue head. A tag is abandoned for good after 512 attempts. `QUARANTINED_FRAMES` is never drained. Task-stack leaks are `mem::forget`, never retried, although the log says "until confirmed". The domain sweep runs only on a task exit. | `memory/deferred_release.rs:335-372,418-458`; `memory/address_space.rs:14-19`; `task/tcb.rs:813-841`; `task/syscall/grant.rs:476` |
| F6 | **The retry mechanism for shared roots already exists.** Each remote CPU serves a per-hart epoch with a *full* flush, so one epoch ack covers shared-root and tag invalidations alike. | `sched/smp.rs:149-164,194-207`; `arch/x86_64/tlb.rs:6-8` |
| F7 | **The lock-rank table contradicts the DMA-export code.** `DMA_EXPORTS` (rank 121) is held while `PAGE/REG_GRANT_TABLE` (54/55) are taken, and `FRAME_ALLOCATOR`/`KERNEL_ROOT` (71/73) via `free_released_range`. The test-hooks rank check panics on that. **[I]** No test-hooks lane reaches it. | `sync/rank.rs:54-55,71-73,121,148-161`; `task/syscall/dma_export.rs:316,407,684-686,777-797,869-876` |
| F8 | **A domain owner-map rollback frees the backing after one probe, whatever the outcome.** The stale-private-tag rule is broken. | `task/syscall/mod.rs:216-224`; `task/syscall/grant.rs:1988-1991,2341-2346` |
| F9 | **The `ShmAlloc` frame is not zeroed before `ShmMap` publishes it USER RW.** Information leak; fixing it changes behaviour, so it is an owner decision. | `task/syscall/grant.rs:1713-1718,1742-1753` |
| F10 | **The dead per-task `grant_table` is already deleted** (commit `e88613a26b`, `tcb::GrantEntry`). Only a stale doc reference is left. | `git show e88613a26b -- kernel/src/task/tcb.rs`; `task/syscall/grant.rs:131` |
| F11 | **SHM has two cell callers, both fault-injection.** No third caller exists: the plan's "three" is not reproduced. The third may be the `ostd` wrapper. | §4.2 |
| F12 | **Claim (c) holds.** VM-entry hot paths only read PAs captured at create. Separate defects: `create_vcpu` leaks frames on its error paths; the AArch64 teardown TLBI is scoped to the VMID currently in VTTBR **[I]**. | §5 |
| F13 | **`cellos-kernel` is a bin crate (`kernel/src/main.rs`, no `lib.rs`), so it has no doctests.** "Does not compile" needs a rustc-script witness, which has precedent in `scripts/x86/p01_ledger_smoke.rs:8,104-110`. | `ls kernel/src/lib.rs` (absent) |
| F14 | **Plan line refs are stale.** The plan cites `tlb_shootdown.rs:460-465` and `:378`. The panic is at `:432-437` (x86) and `:409-413` (RV64), and the bound at `:354-359`. | `memory/tlb_shootdown.rs` |
| F15 | **The plan's drop order needs one amendment.** Pins and leases held by *other* cells (VFS/GrantSlice raw pointers, DMA) must be checked **before** unpublish, because holders still dereference the SAS VA. Today withhold happens before the unmap. Short copy windows are drained **after** the flush. | `task/syscall/misc.rs:145-168`; `task/syscall/grant.rs:613-618`; `task/user_copy/sas-lease.rs:9-16` |

## 1. Release protocols

### 1.1 Protocol table

Flush-scope key:
- **tag:** local `flush_asid` plus a per-hart epoch from every online remote CPU (`memory/tlb_shootdown.rs:119-167,210-229,246-283`).
- **shared:** the x86 all-PCID+global local flush plus a synchronous remote ack (`:424-438`); on RV64, a per-page `sfence` plus SBI RFENCE (`:393-414`); on AArch64, a broadcast TLBI per page with no ack (`:416-423`).

| # | Protocol, sites | Waits for: no PTE in / TLB confirmed / lease | Scope | Locks at flush | Locks at free | Timeout | Normalise before free |
|---|---|---|---|---|---|---|---|
| P1 | **Private-root deferred release.** `map_private_page` rollback `memory/address_space.rs:757-771`; stack map rollback `:908-940`; `unmap_existing_task_stacks` `:959-1007`; `unmap_private_page` `:1009-1067`; grant map rollback `:1151-1160`; `unmap_grant_page` `:1178-1219`; root `Drop` `:1310-1366`; `AsidLease` `:1434-1467` | PTE removed from **that private root only** (`:1032-1045`). Tag confirmed. `copy_readers==0` spin (`:972-974,1012-1014,1180-1182`). | tag | Unmap paths: none (`:1047-1048,1203`). Map rollback: instance locks `ledger`/`table_frames`/`frames` still held (`:727,742-743` → probe `:762`). From domain drain: the record gate (`task/domain_grant.rs:257-261`). | `OwnedFrame::drop`, `FRAME_ALLOCATOR` alone (`memory/frame.rs:673-693`); reaper frees outside `DEFERRED` (`memory/deferred_release.rs:393-406`) | Probe bound: RV64 200 ms (`memory/tlb_shootdown.rs:343-346`), AArch64 200 ms (`:349-352`), x86 2e9 TSC (`:354-359`). Then **defer** (`memory/deferred_release.rs:102-190`): ≤32 tags (`:35`), hart-0 timer reaper (`task.rs:1054-1057`), reissue every 32 attempts (`:51`). After 512 attempts (~5 s): **quarantine forever** and the tag stays unconfirmed for the boot (`:418-458,467-490`). Queue full: quarantine at once (`:185-188`). | None. Leaves were never in the kernel root; zeroed through `phys_to_virt` at alloc (`memory/address_space.rs:189-196`). |
| P2 | **Pin quarantine** (`memory/pin.rs`). Entry `withhold_or_free` (`task/syscall/misc.rs:156-168`) → `withhold_pinned_frames` (`memory/pin.rs:956-1054`), precedence GrantLease > DmaExport > pin > cache-sync > VfsLease. Owner frees refuse instead: `refuse_if_pinned` (`task/syscall/grant.rs:357-381`, used at `:411,2211`). | **Event key only** (`memory/pin.rs:256-280`): `Owner` ← IOMMU ack (`task/syscall/grant.rs:812-820`; `memory/pin.rs:1064-1072`); `GrantLease` ← holder end (`task/syscall/grant.rs:824-860,1454-1466`; `memory/pin.rs:1324-1359`); `VfsLease` (`task/syscall/grant.rs:865-873,951-969`); `DmaExport` (`memory/pin.rs:1455-1481`); `CacheSyncBootLifetime` ← never (`:259-261,1062-1063`). No TLB condition: frames stay published while held. | none at hold; P3 at release | `REGISTRY` leaf (`memory/pin.rs:28-36`) | P3 via `free_released_range` (`task/syscall/grant.rs:340-347`) | None ("never on a timer", `memory/pin.rs:23-26`). Full (64 regions, `:67`) → **leak**, counted in `LEAKED_PAGES` (`task/syscall/misc.rs:160-166`; `task/syscall/mod.rs:392`). | via P3 |
| P3 | **`free_grant_pages(_inner)`** (`task/syscall/grant.rs:331-347,1290-1356`). Callers `:440,444,617,685,726,758,789,1990,2002,2241,2245,2345,2357`; `task/syscall/mod.rs:312`. | Caller checked no pin. Published-in-kernel-root test (`:1315-1317`). Unpublish: unmap each leaf (`task/stack.rs:112-114`), `flush_range` over the run (`:115`), SAS window quiescence (`:116-117`; `task/user_copy/sas-lease.rs:59-71`). x86: prune, then `flush_page` per detached table (`task/syscall/grant.rs:1275-1288`). | shared | None (`task/syscall/grant.rs:328-330`) | `FRAME_ALLOCATOR` → `KERNEL_ROOT` (identity `map_page`, `:1328-1345`), then a local `tlb_flush_all` (`:1346`) | **x86 panic** (`memory/tlb_shootdown.rs:432-437`); **RV64 panic** on RFENCE error (`:409-413`); AArch64 none | RV64/AArch64: map f→f kernel RWX (`task/syscall/grant.rs:1306,1336-1338`). x86: none, prune tables (`:1321-1323`). Domain run with no kernel-root leaf: dealloc only (`:1307-1320`). |
| P4 | **`alloc_grant_pages` partial rollback** (`task/syscall/grant.rs:1220-1256`) | Unpublish the mapped prefix (`:1230`); x86 prune (`:1231-1238`) | shared | none | `FRAME_ALLOCATOR` (`:1245-1254`), then local `tlb_flush_all` (`:1255`) | as P3 | as P3. The map loop itself flushes per page under `FRAME_ALLOCATOR` on x86 (`:1205-1218` → `memory/paging.rs:917`). |
| P5 | **Domain-grant revoke and deferred sweep.** `DomainGrant::revoke` (`task/domain_grant.rs:330-350`), receiver drain `:268-302`, owner drain `:305-325`. Callers via `revoke_domain_record` (`task/syscall/authority.rs:15-26`): GrantFree (`task/syscall/grant.rs:2228-2243`), GrantUnregister (`:427-442`), exit (`task/syscall/mod.rs:299-358`), sweep (`task/syscall/grant.rs:699-763`), CapRevoke (`task/syscall/authority.rs:475,493`). | Receiver then owner PTE removed in their private roots (P1 `unmap_grant_page`). Both tags confirmed (`memory/deferred_release.rs:277-288` checked at `task/domain_grant.rs:290,317`). Single free via row take (`task/syscall/misc.rs:80-94`). Then pins (`task/syscall/mod.rs:311`), then P3. | tag | Gate spinlock, IRQ masked, held across unmap and probe (`task/domain_grant.rs:257-261`; `task/domain-grant-gate.rs:18-23`). No table lock (`task/syscall/grant.rs:578-580`). | P3 | Record stays `Revoking`, row owner zeroed (`task/syscall/mod.rs:332-357`). **Retried only** from `reap_grants_for_task` (`task/syscall/grant.rs:476`), i.e. on *some* task's exit; no timer. | P3. RV64 restores the f→f RWX alias even for domain backing (Phase 15 residual, `reports/phase-15-evidence.md` §5). |
| P6 | **Stack teardown.** `Stack::drop` (`task/stack.rs:433-473`), `release_frames` (`:80-91`), `unpublish_user_frames` (`:107-118`). Alloc-failure releases at `:240-246,262-274,283-287,298-309`. | P3's unpublish over every page, guards included (`:446-449`). Then per page: `unmap_page` + `map_page` f→f RWX + dealloc (`:84-89`), local `tlb_flush_all` (`:90`), `STACK_RESERVATIONS` removed (`:455`). | shared | Unpublish: none. **x86 `map_page` flush per page under `FRAME_ALLOCATOR`** (`memory/paging.rs:917`). | `FRAME_ALLOCATOR` → `KERNEL_ROOT` | x86 panic / RV64 panic (via `flush_range`) | f→f kernel RWX on **every** arch, including x86, where it creates the lower-half alias (F3). |
| P6b | **Domain-task stacks.** `Task::drop` (`task/tcb.rs:797-845`) and spawn rollback (`sched/scheduler.rs:1284-1338`) | P1 (`unmap_existing_task_stacks` / `map_existing_task_stacks`) | tag | none | P6 if confirmed | Unconfirmed → `note_leaked_stack_frames` + `mem::forget` (`task/tcb.rs:813-841`; `sched/scheduler.rs:1304-1336`). **Permanent leak, never retried.** The log text says "until … confirmed" (`task/tcb.rs:836`). The `STACK_RESERVATIONS` row is never removed **[I]** (forget skips `task/stack.rs:455`). | — |
| P7 | **`CellSegments` teardown.** `eager_unmap` (`task/stack.rs:614-626`), also called under `SCHEDULER` from `sched/scheduler.rs:1796-1802`; `Drop` `:628-659` | Leaf unmapped if still ours (`:618-620`), then `flush_page` **per page** (`:623`). x86 `unmap_page` already flushed (`memory/paging.rs:740-742`): **2 broadcasts/page**. Prune per page, `flush_page`, free tables (`task/stack.rs:633-642`). Free backing (`:643-647`). VA slot (`:655-657`). **No SAS-window wait** (unlike `:116-117`) **[I]** low risk: the copy targets only the current caller (`task/user_copy/sas-lease.rs:113-128`). | shared | `SCHEDULER` (eager path) or none (drop) | `FRAME_ALLOCATOR` | x86/RV64 panic | None (VA≠PA; the identity/HHDM alias is never removed) |
| P8 | **DMA export release.** `revoke`/`drop_lease` (`task/syscall/dma_export.rs:305-352`); `unmap_window` (`:936-946`); retried every scheduler pass (`task.rs:1510-1512`; `task/syscall/dma_export.rs:654-665,725-730`); owner death first (`task.rs:1891-1894`). | IOVA leaves cleared + IOTLB drain acked (`task/syscall/dma_export.rs:336-343`). Range lock in pin `REGISTRY` (`memory/pin.rs:1424-1446`). Owner frames released only when nothing else protects them (`:1455-1481`). | IOTLB (VT-d), then shared via P3 | **`EXPORTS` held** across `unlock_range` → P3 unpublish (`task/syscall/dma_export.rs:316,869-876`; `task/syscall/grant.rs:1319`) | `EXPORTS` → `FRAME_ALLOCATOR` (rank inversion, F7) | Lease `Revoking`, `TryAgain` (`task/syscall/dma_export.rs:338-342`), retried **unbounded** | P3 |
| P9 | **SHM release.** `release_shm_for` (`task/syscall/grant.rs:93-127`) at retirement (`task.rs:1624-1627`) | Identity VA ≠ frame → `flush_page` only (`task/syscall/grant.rs:108-112`); else P3 unpublish (`:114`). **No pin check.** | shared | none (`SHM_HANDLES` dropped, `:63-76`) | `FRAME_ALLOCATOR` (`:116-125`) | x86/RV64 panic | RV64/AArch64 identity RWX (`:115-119`). x86: none, **no table prune**, unlike P3 `:1321-1323` **[I]** kernel-root tables made by `ShmMap` (`:1742-1753`) stay. |
| P10 | **Loader failure unwind** (`loader/elf.rs:392-408`), called under `FRAME_ALLOCATOR` (`loader/elf.rs:255,293,313,350,364`; `task/elf_prepare.rs:78-86,132-134`) | Per page `unmap_page` + `flush_page` (`loader/elf.rs:396-399`); prune, then **local** `tlb_flush_all` only before freeing tables (`:401-405`). Map-failure frame freed unflushed (`:312`): it was never published (`memory/paging.rs:644-663`). | shared (leaves), local (tables) **[I]** x86 remote paging-structure caches are not covered | `FRAME_ALLOCATOR` | `FRAME_ALLOCATOR` | x86/RV64 panic | none |
| P11 | **Kernel-root page-table frames.** RV64/AArch64 `map_page` failure frees tables with no flush (`memory/paging.rs:397-402`). x86 failure: `flush_page`, then free (`:644-663`). `prune_empty_tables`/`ReclaimedPageTables::release` (`:1007-1028`); users `task/stack.rs:634-640`, `task/syscall/grant.rs:1280-1285`, `loader/elf.rs:401-405`. | detached from `KERNEL_ROOT` | per caller | x86 failure path: `KERNEL_ROOT` + `FRAME_ALLOCATOR` | `FRAME_ALLOCATOR` | per caller | none |
| P12 | **EPT/NPT** (`arch/x86_64/memory/ept.rs:447-480`) | VM removed from `VM_REGISTRY`; `run_vcpu_hal` holds it across VMRUN (`arch/x86_64/hypervisor/svm_registry.rs:422-451`); reap removes under it (`:586-597`). EPT: INVEPT on *this* CPU (`arch/x86_64/memory/ept.rs:454-463`); other CPUs are covered by entry-time single-context INVEPT (`arch/x86_64/hypervisor/svm_registry.rs:404-418,438-449`; `hal/arch/x86/src/x86_64/invept.rs:8-10`). NPT: ASIDs never reused (`svm_registry.rs:90-100`) + `TLB_FLUSH_ASID` on every VMRUN (`hal/arch/x86/src/x86_64/vmcb.rs:174`). | local + entry-time | none (drop is outside `VM_REGISTRY`, `svm_registry.rs:597`) | `FRAME_ALLOCATOR` | INVEPT error only logged, frames freed anyway (`ept.rs:456-462`) | none (HHDM) |
| P13 | **Stage-2** (`arch/aarch64/vm.rs:849-868`; `memory/stage2.rs:421-441`) | `disable_stage2` per dying VM: `tlbi vmalls12e1is` for the VMID **currently in VTTBR_EL2** (`hal/arch/arm/src/aarch64/stage2_regs.rs:93-107`), which is the last `enable_stage2` (`arch/aarch64/vm.rs:439-443`; `stage2_regs.rs:57-84`) **[I]** wrong VMID with more than one VM | broadcast, VMID-scoped | none | `FRAME_ALLOCATOR` | n/a | none (`allocate_guest_ram` never remaps, `memory/frame.rs:1425-1431`) |
| P14 | **vCPU frames** (`SvmVm::drop`, `arch/x86_64/hypervisor/svm_registry.rs:115-153`) | VMCS: VMCLEAR on this CPU (`:138-142`). `VmxVcpu::run` VMCLEARs before it returns (`hal/arch/x86/src/x86_64/vmx_vcpu.rs:362-374`). | — | — | `FRAME_ALLOCATOR` | — | none |
| P15 | **VA-only retentions** (same shape, no frames): private VA `invalidation_pending` (`memory/address_space.rs:630-684`), `STACK_RESERVATIONS` (`task/stack.rs:283,455`), `va_slot` (`:655-657`), ASID slot (`memory/address_space.rs:1474-1504`), IOVA window (`task/syscall/dma_export.rs:291-302`) | — | — | — | — | — | — |

### 1.2 Lock order and flushes under locks

The rank table (`sync/rank.rs:44-131`), outermost first: `VM_REGISTRY` 45, `SVM_VM_REGISTRY` 46, `PAGE_GRANT_TABLE` 54, `REG_GRANT_TABLE` 55, `SCHEDULER` 56, `FRAME_ALLOCATOR` 71, `KERNEL_ROOT` 73, `QUARANTINED_FRAMES` 98, `LIVE_ASIDS` 99, `DEFERRED_RELEASE` 100, `ABANDONED_TAGS` 101, `SHM_HANDLES` 114, `DMA_EXPORTS` 121, `PIN_REGISTRY` 124.

A shared-root flush that waits for a remote ack runs while each of these locks is held:

| Held | Sites |
|---|---|
| `FRAME_ALLOCATOR` | `task/stack.rs:87` (`release_frames`), `task/stack.rs:240` (Stack alloc), `task/syscall/grant.rs:1209` (`alloc_grant_pages`), `task/syscall/grant.rs:1751` (`shm_map`), `loader/elf.rs:370` (`flush_deferred_maps`, documented in `memory/paging.rs:927-929`), `loader/elf.rs:397-398` (unwind), `memory/paging.rs:710-724` (MMIO), `memory/paging.rs:658` (map failure), `task.rs:2834` (`spawn_synthetic`, no callers) |
| `KERNEL_ROOT` | `memory/paging.rs:733-742` (x86 `unmap_page`), `memory/paging.rs:626-663` |
| `SCHEDULER` | `sched/scheduler.rs:1800` → `task/stack.rs:614-624` |
| `DMA_EXPORTS` | `task/syscall/dma_export.rs:316` → `:873` → `task/syscall/grant.rs:1319` |
| Domain-record gate (tag probe) | `task/domain_grant.rs:257-261` |
| `AddressSpace` instance locks (tag probe) | `memory/address_space.rs:762` |

None of these deadlock. Every ranked-lock spin answers TLB requests (`task.rs:802-807` → `sched/smp.rs:181-191`), and the masked-uncontended fixture proves it (`arch/x86_64/smp_bringup.rs:513-523`). The rationale in `task/stack.rs:103-106` ("a peer may spin on that lock") is therefore already covered. The real costs are holding the global allocator for up to 2e9 TSC ticks per flush, and the panic.

## 2. Raw frame alloc/free sites outside `memory/frame*.rs`

### 2.1 Production

| File | Alloc sites | Free sites | Owner | Protocol |
|---|---|---|---|---|
| `task/stack.rs` | `:199` | `:88,639,646` (`release_frames` called at `:244,271,285,306,452`) | `Stack`, `CellSegments` | P6, P7 |
| `task/syscall/grant.rs` | `:1200,1715` | `:123,1252,1284,1343` | PAGE/REG rows, SHM | P3, P4, P9 |
| `task/syscall/mod.rs` | `:247` | (via `free_grant_pages`) | domain backing | P5 |
| `memory/paging.rs` | boot root `:127,182,216,483,543`; kernel-root tables `:388,635`; `remap_range_user` `:1096`; framebuffer `:1164`; #PF demand frame `:1501` (no owner; VMA list empty in practice, `:1490-1493`) | `:401,660,1009` | kernel root | P11 |
| `memory/address_space.rs` | `OwnedFrame::allocate` `:188` | (`Drop`) | private root | P1 |
| `memory/deferred_release.rs` | — | `OwnedFrame` drop `:406` (holder `:74`) | queue | P1 |
| `memory/stage2.rs` | `:187,224,235` | `:427,431-432,437` | `Stage2Table` | P13 |
| `arch/x86_64/memory/ept.rs` | `:146,193,239` | `:467,469,473` | `NestedPageTable` | P12 |
| `arch/x86_64/hypervisor/svm_registry.rs` | `alloc_contig` `:179`, called at `:214,267,314-317` | `:127-133,143,148,232` | `SvmVm` | P14 |
| `loader/elf.rs` | `:285` | `:312,405-406` | `LoadedPage` → `CellSegments` | P10 |
| `snapshot_engine.rs` | `:1340` | `:1353` | scratch (never mapped) | none |
| `arch/x86_64/boot.rs` | `:187` (HHDM firmware tables), `:640` (pvclock) | — | boot lifetime | — |
| `arch/x86_64/boot/cpu_features.rs` | `:75` (HSAVE/VMXON per CPU) | — | boot lifetime | — |
| `main.rs` | `:164` (boot heap) | — | boot lifetime | — |
| `task.rs` | `:2784` (`spawn_synthetic`, dead: no caller found by `rg 'spawn_synthetic\('`) | — | leaked | — |

Totals: raw frees in **8 production files / 31 sites** (svm 10, grant 4, stage2 4, stack 3, paging 3, ept 3, elf 3, snapshot 1). `OwnedFrame` appears in `memory/{frame,address_space,deferred_release}.rs` only. The request's per-file counts match, except grant (4 vs 3, because `reclaimed.release` at `:1284` is counted here).

### 2.2 Tests and fixtures (listed separately)

`arch/riscv64/memory/tlb_shootdown_selftest.rs:116 / 86,121,134`; `arch/riscv64/task/context_handoff_selftest.rs:222 / 406`; `arch/riscv64/task/getrandom-sas-tests.rs:38,41`; `arch/riscv64/task/stack_overflow_probe.rs:77 / 87` (test-hooks, `arch/riscv64/task.rs:15-16`); `arch/riscv64/task/thread_user_entry_selftest.rs:94 / 121`; `arch/x86_64/smp_bringup.rs:554,556 / 600,601` (test-hooks, `:535`); `arch/x86_64/task/x86_idt_cpl3.rs:22` (feature `x86-idt-cpl3-test`, `arch/x86_64/mod.rs:76-77`); `task/user_copy_tests.rs:366 / 398`; `task/user_hello.rs:46` (test-hooks, `task.rs:102-103`); `task/syscall/grant.rs:1382` (`getrandom-sas-test`). Allocator swaps: `memory/stage2_tests.rs:31-51`, `memory/address_space_tests.rs:15-20`, `loader/atomic_publication_tests/snapshot.rs:496-498`.

## 3. Shared-root flush callers and what Unconfirmed would mean

The x86 panic is at `memory/tlb_shootdown.rs:432-437`, after `await_remote_invalidation(tag_probe_ticks())` with a bound of 2e9 TSC (`:354-359`). The RV64 panic is at `:409-413`; it fires on an SBI error, not on a timeout. RV64 `map_page`/`unmap_page` never flush (`memory/paging.rs:375-427`), so RV64 reaches this panic only from rows 7-12 below.

| # | Caller | Op | Frames at risk (owner) | Frees after flush? | Fallback if Unconfirmed |
|---|---|---|---|---|---|
| 1 | x86 `paging::unmap_page` (`memory/paging.rs:733-744`); users `task/stack.rs:86,113,264,619`, `loader/elf.rs:397` | remove leaf | frame behind the VA (caller) | yes (stack, loader) | Return the batch; the caller keeps the run `Quarantined(SharedUnconfirmed)` |
| 2 | x86 `paging::map_page` (`memory/paging.rs:910-918`); users `task/stack.rs:87,240`, `task/syscall/grant.rs:1209,1751`, `task.rs:2834` | new leaf / raise | none for a previously absent leaf **[I]** (x86 caches no not-present entries) | no | New leaf: benign, proceed. Overwrite of a present leaf: treat as row 6. |
| 3 | `map_page_unflushed` failure (`memory/paging.rs:644-663`) | detach ≤3 tables | table frames (kernel root) | yes | Quarantine the tables |
| 4 | `map_mmio_x86_flags` (`memory/paging.rs:709-726`); users `arch/x86_64/boot.rs:598`, `arch/x86_64/dev/iommu_x86/unit.rs:58`, `arch/x86_64/dev/pcie_msi.rs:137`, `arch/x86_64/dev/pcie_msi/virtio_gate.rs:116`, `task/syscall/dev.rs:1058,1077` | MMIO rights (kernel init or user grant = raise) | none (MMIO) | no | Raise: proceed. Lowering: row 6. |
| 5 | `flush_deferred_maps` (`memory/paging.rs:968-972`; `loader/elf.rs:370`) | new leaves / rights merge (`loader/elf.rs:271-280`) | none | no | proceed (precondition `memory/paging.rs:936-940`) |
| 6 | `protect_page`/`protect_pages` (`memory/page_protect.rs:56-62,108-120`): W^X (`loader/wx.rs:191`), `clear_mmio_user`/`revoke_mmio_user` (`memory/paging.rs:762-781,889-893`; `task/syscall/authority.rs:390`), `restore_mmio_boot_posture` (`resource_registry.rs:502`) | **permission lowering** | none freed; a remote CPU may still write | no | **Fail closed**: W^X → spawn fails, unwind quarantines its frames (`loader/wx.rs:191-196`). MMIO revoke → keep the window/BDF owned (not reassigned) until the epoch confirms. Never report success. |
| 7 | `CellSegments` (`task/stack.rs:623,636`) | remove leaf, detach tables | segment backing (`CellSegments`), tables (kernel root); **VA slot** | yes | Quarantine backing + tables. Keep the VA unusable until confirmed (respawn reuses it, `task/stack.rs:607-613`). |
| 8 | `unpublish_user_frames` (`task/stack.rs:115`), from `Stack::drop` (`:449`), `release_shm_for` (`task/syscall/grant.rs:114`), `alloc_grant_pages` (`:1230`), `free_grant_pages_inner` (`:1319`) | remove USER leaves | stack run / SAS grant run / SHM frame | yes | Quarantine the run; skip normalisation until confirmed |
| 9 | `release_shm_for` (`task/syscall/grant.rs:111`) | stale-only flush | SHM frame | yes | Quarantine |
| 10 | `prune_unpublished_tables` (`task/syscall/grant.rs:1282`) | detach tables | tables | yes | Quarantine |
| 11 | loader unwind (`loader/elf.rs:398`) | remove leaf | segment frames + tables | yes | Quarantine. Also use a global flush for tables (P10). |
| 12 | `smp_bringup` fixture (`arch/x86_64/smp_bringup.rs:580,593`), RV64 selftest (`arch/riscv64/memory/tlb_shootdown_selftest.rs:83,108,125`) | test | fixture frames | yes | assert the outcome (the fixture must keep failing loudly) |

## 4. Region tables

### 4.1 Records

| Table | Static | Key (syscall-visible id) | Fields | Owner semantics | Receivers | Pin interplay | Syscalls |
|---|---|---|---|---|---|---|---|
| SHM | `SHM_HANDLES` (`task/syscall/mod.rs:104-105`) | frame PA = handle (`task/syscall/grant.rs:1715-1718`) | `owner: Incarnation`, `recipient: Option<cell>`, `size`, `rights` (always `0x3`) (`:18-23,1717`) | live incarnation only (`:50-60`) | one cell (`:1734-1740`) | none (P9 never checks pins) | ShmAlloc 20, ShmMap 21 (`libs/abi/abi/syscalls.toml:606-616`) |
| Page grants | `PAGE_GRANT_TABLE` (`task/syscall/mod.rs:136-137`) | base PA (`task/syscall/grant.rs:2014-2026`) | `base`, `size`, `owner: tid`, `owner_cell`, `owner_generation`, `shared_to: Option<(tid, GrantPerm)>`, `domain: Option<Arc<DomainGrant>>` (`:135-151`) | owner tid + live binding (`:1997-2005`) | one tid + perm | `refuse_if_pinned` on free (`:2211`); withhold on reap (`:614`) | GrantAlloc 208, GrantShare 209, GrantSlice 210, GrantFree 211, GrantDma 233, cache sync 249/250 (`syscalls.toml:252,869-934,1956-1975`) |
| Registered grants | `REG_GRANT_TABLE` (`task/syscall/mod.rs:286-287`) | base PA (`task/syscall/grant.rs:2369-2381`) | same as page grants (`:289-299`); `owner==0` = orphan / VFS handoff (`:288`) | ownership can **transfer** to a live grantee on owner death (`:659-664`) | one tid + perm | VFS/GrantSlice lease → orphan + withhold (`:645-658`) | GrantRegister 215, GrantUnregister 216 (`syscalls.toml:994-1007`); also read by Share/Slice/Dma |
| DMA exports | `EXPORTS` (`task/syscall/dma_export.rs:243-244`) | export id, never reused (`:245-247`) | `owner_tid`, `owner/holder: Incarnation`, `base`, `len`, `flags`, `state: Live\|Revoking`, `mapping: Option<{holder_tid, iova, quota_bytes, confirmed, bdfs}>` (`:157-182`) | range must lie inside a caller-owned live grant (`:742-812`) | one holder incarnation | range lock `DmaExport` key (`memory/pin.rs:1424-1446`) | 437/438/439 (`syscalls.toml:1678-1712`) |
| Domain grants | **embedded** in page/reg rows (`task/syscall/grant.rs:150,298`), not a separate table | = row key | `owner: DomainRef`, `owner_base`, `owner_size`, `owner_drained`, `receiver: Option<{root, cell, generation, base, size, rights, drained}>`, `lifecycle` (`task/domain_grant.rs:110-117,240-248`) | owner root | one receiver root | as row | via 208-216; CapRevoke 219 reclaims (`task/syscall/authority.rs:475,493`) |
| *(6th custodian)* | pin `REGISTRY` (`memory/pin.rs:344-366`) | — | pins, cache_sync, vfs_leases, grant_leases, quarantine `[FrameHold; 64]`, dma_exports | — | — | — | — |

### 4.2 SHM callers

- Cells: `cells/demos/recovery-demo/src/bin/worker/fault.rs:34-41` (the `crash::SHM_EXPORT` crash mode) and `cells/drivers/ahci/src/fault_inject.rs:96-101` (only in fault-inject builds, `cells/drivers/ahci/src/main.rs:284,319`). Both are fault-injection.
- The wrappers are `libs/ostd/src/syscall.rs:1157,1168`. Kernel host tests: `task/syscall/tests.rs:2465-2506`.
- No other references: `rg -l` over the repo found only these, plus docs and toml.
- **Dead per-task `grant_table`:** removed in `e88613a26b` ("the per-task lease and grant tables"). The only leftover is the doc reference `task/syscall/grant.rs:131`.

## 5. Hypervisor frames

| Object | Alloc | Free | Hot path that must not change |
|---|---|---|---|
| NPT/EPT root + tables + guest RAM | `arch/x86_64/memory/ept.rs:144-147,193,236-247` | `arch/x86_64/memory/ept.rs:447-480` | `run_vcpu_hal` reads `vm.npt.eptp()` and does INVEPT on entry (`arch/x86_64/hypervisor/svm_registry.rs:422-451`) |
| xAPIC frame | `arch/x86_64/hypervisor/svm_registry.rs:214` | `:148,232` | the VA is captured into `SvmVcpu` at create (`:340,355`) |
| VMCB, host-save, MSRPM×2, IOPM×3 | `:314-317` | `:127-133` | captured PAs in `SvmVcpu::new` (`:344-359`) |
| VMCS | `:267` | `:143` | `VmxVcpu::run` does vmptrld/VMCLEAR per run (`hal/arch/x86/src/x86_64/vmx_vcpu.rs:362-374`) |
| Stage-2 root (2 frames, 8 KiB aligned) + tables + guest RAM | `memory/stage2.rs:185-188,224,232-236` | `memory/stage2.rs:421-441` | `El2Monitor::run(vcpu, vm.vmid, vm.stage2.root_pa(), …)` (`arch/aarch64/vm.rs:612`) |

**Defects outside the migration itself (raw `usize` ownership):**
- **VMX leaks.** The VMCS leaks when `VM_REGISTRY` NotFound, `VmxVcpu::new` or `init_pvh` fails (`arch/x86_64/hypervisor/svm_registry.rs:267-290`).
- **SVM leaks.** A partial vmcb/host/msrpm/iopm allocation leaks when a later allocation hits OOM or NotFound (`:314-335`).
- **AArch64.** `NEXT_VMID` is a wrapping `u16 fetch_add` (`arch/aarch64/vm.rs:74,439`) **[I]**.

## 6. The plan's three claims

| Claim | Verdict | Evidence |
|---|---|---|
| (a) Every retain-until-safe site waits for the same three conditions | **False.** The extra keys become `reason`s: TagUnconfirmed (P1); SharedUnconfirmed (P3/P6/P7); RfenceFailed (`memory/tlb_shootdown.rs:409`); Pinned{Owner, GrantLease, VfsLease, DmaExport, CacheSyncBootLifetime} (`memory/pin.rs:256-280`); IotlbUnacked (`task/syscall/dma_export.rs:336-343`); DomainRevoking (`task/domain_grant.rs:330-350`); CopyWindow / copy_readers (`task/user_copy/sas-lease.rs:59-71`; `memory/address_space.rs:1012-1014`); VmQuiesced (P12-P14); plus VA-only holds (P15). | §1.1 |
| (b) Quarantined frames drain within bounded epochs on a healthy system | **Not true today; achievable.** Today: (1) the tag reaper checks only the head entry per tick (`memory/deferred_release.rs:335-372`: the second loop pass returns because `stepped==head`), so N stalled tags drain in up to N×5 s, and each is abandoned for good after 512 attempts (`:53-57,418-458`). (2) `QUARANTINED_FRAMES` has no drain (`memory/address_space.rs:14-19`; no other user, `rg QUARANTINED_FRAMES`). (3) Stack leaks are forgotten (`task/tcb.rs:840-841`). (4) The domain sweep runs on task exit only (`task/syscall/grant.rs:476`). (5) The DMA retry runs every pass, unbounded (`task.rs:1512`). (6) The shared root has no retry; it panics. Mechanism to reuse: per-hart epochs (`sched/smp.rs:149-164`), served by a full flush (`:194-207`), already recorded per entry (`memory/deferred_release.rs:64-70`). Counters to add: §7.6. | |
| (c) Hypervisor frames adopt the type without touching VM-entry hot paths | **Holds (by reading).** Hot paths consume only PAs/VAs captured at create (§5) and `eptp()`/`root_pa()` accessors. Frees happen only in `Drop` after removal from the registry (`arch/x86_64/hypervisor/svm_registry.rs:586-598`; `arch/aarch64/vm.rs:849-868`). Condition: `FrameRun::base()` stays an inlined field read. | §5 |

## 7. Design

### 7.1 `FrameRun`

```rust
pub struct FrameRun {           // memory/frame_run.rs
    base: PhysAddr, pages: u32,
    owner: RunOwner,            // {cell, generation, tid} | Kernel | Vm{owner,vm_id} — incarnation, not tid alone
    kind: RunKind,              // Leaf | PageTable | VmStructure
    alias: KernelAlias,         // Intact | Removed | User  (drives normalise, RV64/AArch64 only)
    state: RunState,
}
pub enum RunState { Mapped(Publications), Unpublished(Pending), Quarantined(Reason), Free }
pub enum Reason {
    TagUnconfirmed { asid: u16, epochs: Epochs }, SharedUnconfirmed { epochs: Epochs },
    RfenceFailed { epochs: Epochs }, Pinned(PinKey /* = memory/pin.rs:256-280 ReleaseKey */),
    IotlbUnacked { export_id: u64 }, DomainRevoking, VmLive,
}
```

`Epochs = [usize; MAX_HARTS]` (as in `memory/deferred_release.rs:70`). There is no `Abandoned` or `QueueFull`: state lives in the run, so no queue capacity exists to fill. The current queue caps are at `memory/deferred_release.rs:35,244-252`.

### 7.2 Drop order (amends the plan, F15)

1. **Long holders:** check pins, leases and DMA exports while still `Mapped` (`task/syscall/misc.rs:156-168`). If held → `Quarantined(Pinned)` and the PTEs stay.
2. **Unpublish** every root: kernel-root leaves; private-root ledger entries (`memory/address_space.rs:630-652`); VA reservations (P15).
3. **Flush, batched:** one tag flush per private root plus one shared epoch broadcast **per operation**. A cell death is one operation covering stacks, segments, grants and SHM. Today the cost is O(pages): `task/stack.rs:623,636`, `memory/paging.rs:741,917`.
4. **Bounded probe:** the same spin as `memory/tlb_shootdown.rs:246-283`. If unconfirmed → `Quarantined(…Unconfirmed{epochs})`, logged once per operation.
5. **Short leases:** `copy_readers`, then SAS windows (`task/user_copy/sas-lease.rs:59-71`).
6. **Normalise:** RV64/AArch64 restore f→f kernel RWX only when `alias != Intact`. x86 never adds an alias; its pruned lower-half tables become `PageTable` runs in the *same* batch (`task/syscall/grant.rs:1275-1288`).
7. **Free** under the allocator guard.

### 7.3 Allocator guard (flush under `FRAME_ALLOCATOR` unrepresentable)

- `FRAME_ALLOCATOR` becomes private to `memory/frame*.rs`.
- A per-hart, move-only `FlushCap` is taken from hart-local storage (`take() -> Option`). Interrupt context finds it taken and skips, which is safe: ranked locks mask IRQs (`sync/rank.rs:172-173`).
- `frame::lock(&'c mut FlushCap) -> AllocGuard<'c>` borrows the cap mutably. `shared_flush(&Batch, &mut FlushCap) -> FlushOutcome` and `confirm_tag(&mut FlushCap)` also need it. Calling either while an `AllocGuard` lives is E0499 at compile time, and a second cap cannot exist.
- `paging::map_page(&mut FrameAllocator, …)` loses its internal x86 flush and returns the leaf in a `Batch` (`memory/paging.rs:910-918`). The caller flushes after dropping the guard.
- Belt: a test-hooks runtime assert that rank 71 is not held (`sync/rank.rs:142-161`).
- Proof: a rustc-script witness (F13), in the pattern of `scripts/x86/p01_ledger_smoke.rs`. It is a file-set addition: log it as a deviation.

### 7.4 Quarantine cap (1/64 of RAM)

- One `AtomicUsize QUARANTINED` (all reasons, pin holds included) and `REFUSED` in `frame_run`.
- The check `QUARANTINED * 64 > total_frames` (`memory/frame.rs:405`) goes into `allocate_frame` (`:165`), `allocate_contiguous` (`:540`), `allocate_contiguous_aligned` (`:580`) and `allocate_guest_ram` (`:1425`). A refusal returns `None` with a rate-limited log.
- Refusals surface as the existing OOM results:

| Path | Result | Ref |
|---|---|---|
| grant | `Ok(0)` | `task/syscall/grant.rs:1978-1981` |
| shm | `BufferTooSmall` | `:1721-1722` |
| spawn | `Err(OOM)` | `task.rs:1912-1913`; `task/stack.rs:199-207` |
| VM | `OutOfMemory` | `arch/x86_64/hypervisor/svm_registry.rs:193-217` |
| private root | `OutOfMemory` | `memory/address_space.rs:188` |

- Paths that **panic** on OOM: x86 #PF demand (`memory/paging.rs:1499-1502`, dormant) and boot (`:127,483`).
- Exempt normalisation table allocations (RV64 identity re-map may need a table) and boot.

### 7.5 Retry on epoch

- A single `pending` list (replaces the `DEFERRED` queue and `QUARANTINED_FRAMES`) of `(Epochs, Option<asid>, Vec<FrameRun>)`, built with `try_reserve` because the teardown must not panic on heap.
- Completion is `∀h: tlb_flush_completed(h, epochs[h])` (`sched/smp.rs:159-164`). The remote service is a full flush (`:200`), so this covers both shared and tag invalidations.
- The hart-0 timer reaper (`task.rs:1054-1057`) steps **every** entry. Each check is MAX_HARTS loads, fixing the head-only stepping. It reissues IPIs (`sched/smp.rs:149-156`) every 32 attempts and **never abandons**: escalate the log once.
- `tag_invalidation_unconfirmed` stays true while an entry is pending (`memory/deferred_release.rs:277-288`).
- Domain sweep and DMA retry move to the housekeeping pass (`task.rs:1509-1512`), not the ISR.
- RV64 RFENCE error falls back to the IPI epoch path instead of panicking.

### 7.6 Counters and logs

`frame_run.{quarantined, released_after_retry, refused}`, one per reason. Replace `ABANDONED_FRAMES`/`LEAKED_STACK_FRAMES` (`memory/deferred_release.rs:93-95`) and `LEAKED_PAGES` (`task/syscall/mod.rs:392`).

### 7.7 Region record (behind the five tables; keys unchanged)

```rust
pub struct Region {               // memory/region.rs
    id: RegionId,                 // grant/SHM: base PA (task/syscall/grant.rs:2026,2381,1718); export: id (dma_export.rs:245-247)
    kind: Shm | PageGrant | RegGrant | DmaExport { holder: Incarnation, mapping: Option<Window> },
    run: FrameRun,                // owner incarnation lives here
    rights: Rights,               // GrantPerm / SHM u32 / export flags
    receivers: Receiver,          // None | Sas{tid, perm} | Shm{cell} | Domain(Arc<DomainGrant>) — all single today
    pin: PinState,                // mirrors REGISTRY custody key
    lifecycle: Live | Revoking | Revoked,
}
```

Table locks and ranks stay (`sync/rank.rs:54-55,114,121`). The receiver is single-valued in every table today (§4.1).

### 7.8 Per-arch normalisation

| Arch | Rule |
|---|---|
| RV64, AArch64 | A free frame must be identity-mapped kernel RWX (`task/stack.rs:68-79`; `task/syscall/grant.rs:305-320`), because the loader zeroes through `phys_to_virt` (`loader/elf.rs:324-331`; `memory/frame.rs:26-28`). Domain backing never loses that alias, so skip the unpublish+restore round trip (P5 residual). |
| x86 | HHDM only (`memory/paging.rs:431-434`). Never restore f→f (fix P6/F3). Prune lower-half tables. ShmMap tables (P9). |

## 8. Coverage and the new x86 fixture

### 8.1 Existing coverage

| Kind | Coverage | Ref |
|---|---|---|
| Host | pin 28 tests (`memory/pin_tests.rs`); DMA export 15 with a fake failing ack (`task/syscall/dma_export_tests.rs`; trait `task/syscall/dma_export.rs:98-131`); grant lease 8, owner 3, row 5; gate 3; SAS lease 3; stage2 3; ept 3; `withhold_or_free`/SHM (`task/syscall/tests.rs:1950,2465-2506`). **No host tests** for `deferred_release` or `tlb_shootdown`. | — |
| QEMU RV64 | aspace deferred release with withheld ack (`memory/address_space_tests.rs:299-392`, run from `arch/riscv64/boot.rs:199`); skip-RFENCE control (`memory/tlb_shootdown.rs:22-24,93-96,405-408`) | — |
| QEMU x86 | `S22-X86-REMOTE-SHOOTDOWN` / `MASKED-SHOOTDOWN` (`arch/x86_64/smp_bringup.rs:471-610`, called `:278-281`), required when `X86_CPUS>=2` (`scripts/x86/qemu-domain-test.sh:439-446`); grant deferred/negatives (`task/grant_negative_selftest.rs:248-327`), lane withheld-window exemption (`scripts/x86/qemu-domain-test.sh:449-470`); `DOMAIN-TEARDOWN` (`sched/domain_switch_tests.rs:358`); `-smp 4` boot forbids "invalidation unconfirmed" (`.github/workflows/ci.yml:1088-1101`; `tests/integration/tests/x86_64-boot.rs:275-315`); hypervisor smoke (`scripts/lanes.toml:178`). | — |

### 8.2 Fixture `S22-X86-QUARANTINE-SILENT-CPU`

Domain-test kernel, test-hooks, `X86_CPUS>=2`.

1. Run it after `x86_shootdown_fixture_primary` (`arch/x86_64/smp_bringup.rs:278-281`), on `HART_RT` (`:458`), with the `X86_PROBE_STAGE` handshake (`:480-533`).
2. The AP does `cli`, then spins on an atomic with `spin_loop` only. It takes **no spinlock and does no logging**, because every lock spin answers TLB requests (`task.rs:802-807`).
3. The BSP builds the owners directly: `Stack::new_user/new_kernel`, a `CellSegments` mapped via `map_page_deferred` at a fixture VA, and one `alloc_grant_pages` run. It snapshots `used_bitmap` (`memory/frame.rs:426`) and drops them in one teardown operation.
4. Assert, all **while the AP is still held**:
   - the drop returns (no panic);
   - `quarantined == pages`;
   - `changed_frames` shows nothing freed (`memory/frame.rs:1451`);
   - K fresh allocations never return a quarantined PA.

   Then release the AP and reap in a budget loop, like `task/grant_negative_selftest.rs:248-260`. Assert every frame freed, the counter at 0, and a teardown wall time of ≈1 bound, not pages × bound.
5. Optional second round: kill a real `user_hello` cell (`task/user_hello.rs:34`) under the hold, through `reap_retired_task_resources` (`task.rs:1885-1910`) and the zombie drop (`:1612-1627`).
6. Kernel log token `… unconfirmed: held (fixture)` inside `[frame-run] silent-cpu window OPEN/CLOSED`. Extend the lane awk (`scripts/x86/qemu-domain-test.sh:456-470`) and the SMP marker list (`:439-446`). Run TCG and KVM at 2 and 4 CPUs.
7. Negative control: the same fixture on base `94536305eb` must panic with `shared mapping invalidation unconfirmed` (`memory/tlb_shootdown.rs:433-436`), which the lane rejects (`scripts/x86/qemu-domain-test.sh:172-174`).
8. RV64 counterpart: a `TEST_FAIL_RFENCE` seam next to `TEST_SKIP_REMOTE_RFENCE` (`memory/tlb_shootdown.rs:22-24`).

## 9. Ordered commit plan (steps 2-6)

| # | Step | Commit |
|---|---|---|
| 1 | 2 | `memory/frame_run.rs` with states, reasons, pending list, cap counter; no callers. `frame_run_tests.rs` covers each transition, using `allocator_for_tests` (`memory/frame.rs:902`) and a fake flush backend in the style of `task/syscall/dma_export.rs:98-131`. |
| 2 | 2 | `FlushCap`/`AllocGuard`; `FRAME_ALLOCATOR` made private behind accessors; rustc compile-fail witness script; rank assert. |
| 3 | 3 | `tlb_shootdown`: `flush_range` → `FlushOutcome`; x86 bounded probe → Unconfirmed; RV64 RFENCE → epoch fallback. Permission-lowering callers fail closed (§3 row 6). Panic removed. |
| 4 | 3 | Batching: x86 `unmap_page`/`map_page` return leaves instead of flushing; `CellSegments` and Stack teardown use one batch; reaper steps every entry; 1/64 cap in the allocator; x86 fixture + lane; RV64 seam. |
| 5 | 4 | **Stacks:** `Stack` owns a `FrameRun`; no x86 f→f restore; TCB/spawn-rollback `mem::forget` → `Quarantined(TagUnconfirmed)` run. |
| 6 | 4 | **Cell segments:** move to `memory/cell_segments.rs`; batch; hold the VA until confirmed (risk 2); loader unwind uses a global table flush. |
| 7 | 4 | **Grants/SHM:** `free_grant_pages_inner`, `alloc_grant_pages` (flush outside the allocator), `release_shm_for` (pins, x86 prune); pin withhold becomes `Quarantined(Pinned)` before unpublish. SHM zeroing (F9) → **owner stop**. |
| 8 | 4 | **DMA exports:** `unlock_range` returns runs, freed after dropping `EXPORTS`; resolve the rank inversion (F7). |
| 9 | 4 | **Domain grants:** the record holds a `FrameRun`; owner-map rollback respects the tag (F8); sweep in housekeeping; RV64 skips the domain-backing alias round trip. |
| 10 | 4 | **Hypervisor:** EPT/NPT, Stage-2, VMCB/VMCS/IOPM/MSRPM/APIC frames as `VmStructure` runs; `create_vcpu` leaks fixed by RAII; AArch64 TLBI scope (load the dying VMID or `alle1is`). |
| 11 | 4 | Delete per-site lists: `QUARANTINED_FRAMES`, the `DEFERRED`/`ABANDONED_TAGS` queue, `LEAKED_STACK_FRAMES`, the pin `quarantine[]` custody (`REGISTRY` keeps pins/leases only), `LEAKED_PAGES`. |
| 12 | 5 | `memory/region.rs`; tables store `Region`; ids unchanged. |
| 13 | 5 | SHM as `Region{kind: Shm}` on the grant path (syscalls 20/21 unchanged); drop the stale doc `task/syscall/grant.rs:131`. |
| 14 | 6 | Spec 02 §3, Spec 19. |

After each owner commit, run the x86 SMP4 and grant suites (TCG and KVM), keep the old debug asserts, and never weaken a wait.

## 10. Risks

| # | Risk |
|---|---|
| 1 | **Wrong order frees a live frame.** Pins must be checked before unpublish, copy windows drained after the flush (F15). Domain-grant single-free (`task/syscall/misc.rs:80-94`) must survive the move into `Region`. |
| 2 | **Batching versus VA reuse.** `eager_unmap` frees segment VAs at death for respawn (`task/stack.rs:607-613`; `sched/scheduler.rs:1796-1802`). A deferred or unconfirmed flush with a reused VA lets a remote CPU resolve the *new* cell's VA to the old frame. The VA must stay reserved until confirmed, like `invalidation_pending` (`memory/address_space.rs:630-684`). |
| 3 | **Permission lowering cannot be quarantined.** W^X and MMIO revoke must fail closed, a behaviour change from panic (§3 row 6). It is in scope of the approved panic→quarantine difference **[I]**; confirm with the owner if a spawn refusal becomes observable. |
| 4 | **Heap use in teardown.** Pending lists need `try_reserve`. A heap OOM in teardown must not panic. |
| 5 | **The cap can trigger OOM panics on panicking paths** (`memory/paging.rs:1499-1502`; boot). |
| 6 | **Rank inversion** `DMA_EXPORTS` → grant tables / allocator (F7). The fix changes the documented order (`task/syscall/dma_export.rs:25-28`). |
| 7 | **Hypervisor TLBI scope** (P13), VMID wrap, INVEPT failure ignored (`arch/x86_64/memory/ept.rs:456-462`). |
| 8 | **Reaper ISR cost** grows with entries × MAX_HARTS. Today it is head-only (`memory/deferred_release.rs:335-372`). |
| 9 | **Compile-fail proof** needs a script outside the §3 file set (F13): deviation. |
| 10 | **No abandonment.** Tags that are permanently refused today (`memory/deferred_release.rs:467-490`) become reusable after a late ack. This changes ASID-pool exhaustion under a dead CPU (`memory/address_space.rs:1374,1413-1430`). |
| 11 | **Conflicting WIP.** Intel AnonMemory/XSAVE plans touch the same files (`s7c-handoff-request.md` §5). |
| 12 | **Behaviour-difference stops for the owner:** SHM zeroing (F9); x86 stack teardown without the f→f alias (F3) **[I]** not observable unless something relied on that alias. |
