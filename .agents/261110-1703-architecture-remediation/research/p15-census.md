# Phase 15 census — x86-64 Tier-2 grant parity (S7b step 1)

Measured on `p15-x86-tier2` worktree HEAD `bb3bb73215`. Paths are relative to the repo root. "Scratch" = an unmodified `git archive HEAD` copy at `/tmp/p15/scratch` with only the edits stated in §0 (the worktree was not modified).

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| F1 | The grant state machine has **no RV64/AArch64 instruction or page-table call**. Every step goes through `AddressSpace`, `hal::paging::flush_tlb_page`, `tlb_shootdown::confirm_tag_invalidation` and `deferred_release`. The plan's low-confidence claim holds **at compile time**. | `kernel/src/task/domain_grant.rs:28-34,193-209,245-262,277-287`; `kernel/src/task/domain-grant-gate.rs:25-26` |
| F2 | Widening the 30 grant predicates to `x86_64` compiles clean with `-D warnings` for x86 production (default features, which include `native-domains`), for x86 `test-hooks` (fixtures wired into `fixtures_domain`), and for the host test build. | Scratch: `cargo check -p cellos-kernel --target x86_64-unknown-none` (lane RUSTFLAGS, `build-x86_64-domain-test-ci.sh:180`) → `Finished`; same with `--features test-hooks` → `Finished`; `cargo test -p cellos-kernel --features compile-only --target x86_64-unknown-linux-gnu --no-run` (`ci.yml:2041`) → `Finished`. Without wiring the fixtures, only dead-code errors (`domain_grant.rs:48` `ARCH_TAG`, `:327` `run_selftest`, `grant_gate_selftest.rs:40-71`). |
| F3 | **Runtime blocker 1:** domain-owner backing is zeroed through an identity VA. x86 RAM is not identity-mapped, and x86 syscalls run on the kernel CR3. The first domain `GrantAlloc`/`GrantRegister` would take a kernel #PF. The fix is `memory::frame::phys_to_virt` (HHDM on x86, identity on RV64/AArch64). | `task/syscall/mod.rs:266-270` (`paddr as *mut u8`); `memory/paging.rs:432` ("Physical RAM is mapped at HHDM_BASE + phys (NOT identity-mapped)"); `memory/paging.rs:48-49`; `hal/arch/x86/src/x86_64/syscall.rs:363` and `idt/entry.rs:122` (`movq VI_KERNEL_CR3`); `memory/frame.rs:26-28` |
| F4 | **Runtime blocker 2 (fixtures):** the kernel fixtures assert `present && !user` in the kernel root and read freed frames through the identity VA. On x86 `present` is false, so `ALLOC`/`REGISTER`/`OWNER-MAPPED` would FAIL and `FRAME-REUSE` would fault. | `domain_grant.rs:406-413,513-515`; `grant_gate_selftest.rs:219-224,229-230`; `memory/paging.rs:863-875` |
| F5 | x86 private-root invalidation already supports **probe-then-defer** through the same generic path RV64 uses. Only the deferred-path **test hook** is RV64-only. | §3 |
| F6 | No grant syscall was retired. x86 needs **no ABI change**: Law-1 is not triggered. | §2 |
| F7 | RV64/AArch64 grant fixtures have **no "disable the check" controls**. The existing fault-injection hooks cover map rollback, deferred release, RFENCE and copy faults, not the grant checks. Step 4 must add them. | §5.3 |
| F8 | The witness cells build for x86_64 today; nothing blocks them. | §6 |
| F9 | No shell syntax backgrounds an external cell inside a serial session. The RV64 `grant-pair` fix is the AArch64 boot-order launcher (script-only change). | §8 |
| F10 | Ratchet: `target_arch` lines outside `kernel/src/arch/` are at the ceiling (775/775). Adding `target_arch = "x86_64"` to each predicate fails `check-hal-boundaries.sh`, so step 2 needs a single cfg alias. | `scripts/check-hal-boundaries.sh:69,77-80`; run: `PASS`, count = 775 |

## 1. Gate inventory (grant path)

Class: **N** = neutral (remove the arch condition, or fold it into one alias); **H** = needs a HAL-supplied value or a per-arch fix; **G** = genuinely arch-specific (keep); **U** = unrelated to grants (keep, listed because the handoff counted it).

### 1.1 `kernel/src/task.rs`
| Lines | Gate | Class | Reason |
|---|---|---|---|
| 14-21 | `mod domain_grant`: `native-domains ∧ (rv64 ∨ aarch64∧test-hooks)` | N | No arch call inside (F1); compiles on x86 (F2) |
| 22-34 | `mod domain_grant_gate`: `test ∨` same | N | Pure atomics plus `Spinlock` (`domain-grant-gate.rs:25-26`); already host-built under `cfg(test)` |
| 37-44 | `mod grant_gate_selftest`: `native-domains ∧ test-hooks ∧ (rv64 ∨ aarch64) ∧ ¬release` | N (+F4 runtime fix) | Drives `handle_syscall` only (`grant_gate_selftest.rs:29-36`) |
| 45-46 | `mod grant_reclaim_selftest`: `¬release` | already x86 | Runs on x86 now (`main.rs:271-283`, `cellos_wide`); SAS-only grants (`grant_reclaim_selftest.rs:46-63`) |
| 105-106 | `mod user_copy`: `native-domains ∧ cellos_wide` | already x86 | — |
| 107-112 | `mod user_copy_tests`: `… ∧ riscv64` | split | Forced-fault subset N (`user_copy_tests.rs:300-358`). Race fixture G (SBI IPI, `:548-552`). SAS rows rely on RV64 SUM (`syscall/mod.rs:2008,2018`) |
| 57-63 | `rt_wake_selftest` riscv64 | U | Not grant path |

### 1.2 `kernel/src/task/domain_grant.rs`
| Lines | Gate | Class | Reason |
|---|---|---|---|
| 43-46 | `ARCH_TAG` rv64/aarch64 only | N | Add `S22-X86` (x86 already uses `X86`: `loader/domain_admission.rs:418-423`) |
| 320-324 | `run_selftest`: `test-hooks ∧ native-domains ∧ (rv64 ∨ aarch64)` | N (+F4) | Uses syscalls plus ledger; identity read at `:513-515` → `phys_to_virt` |
| 16-20, 316-319 | doc says RV64/AArch64 only, "grant-pair belongs to RV64" | stale | Update with the code |

### 1.3 `kernel/src/task/domain-grant-gate.rs`, `grant_gate_selftest.rs`, `grant_reclaim_selftest.rs`
No `target_arch` inside (grep: 0). Only the module gates in §1.1 apply.

### 1.4 `kernel/src/task/syscall/` (the handoff's "13 arms" are mostly here)
| File:lines | Gate | Class | Reason |
|---|---|---|---|
| mod.rs:13-20 | `use Arc` for lifecycle | N | — |
| mod.rs:174-184 | AArch64-production const pin (`!lifecycle_supported`) | G | Keep: AArch64 production stays closed |
| mod.rs:202-241 | `map_domain_owner_pages` | N | `map_grant_page`, `flush_tlb_page`, `confirm_tag_invalidation` only |
| mod.rs:250-272 | `alloc_grant_pages_supervisor_only` | **H** | Identity-VA zeroing (F3) |
| mod.rs:275-294 | `DomainRecord` = `Arc<DomainGrant>` / `()` | N | — |
| mod.rs:325-386 | `release_deferred_domain_row` (+ stub) | N | — |
| mod.rs:717-759 | `domain_grant_share_row` | N | Calls `DomainGrant::publish` (H-5, §4) |
| mod.rs:768-849 | `domain_grant_slice_row` | N | Calls `publish` at `:834` |
| mod.rs:854-904 | `resolve_domain_grant_slice` | N | — |
| mod.rs:91-107, 1805-1830 | `getrandom-sas-test ∧ riscv64` | U/G | RV64 SUM fixture |
| mod.rs:2008, 2018 | `riscv64` `user_access::syscall_open/close` | G | `sstatus.SUM`. x86 analogue is STAC, only under `x86-smap` (`hal/arch/x86/src/x86_64.rs:244`) |
| misc.rs:23-44 | `clone_domain_record` | N | — |
| misc.rs:47-84 | `new_domain_record` | N | — |
| misc.rs:86-109 | `drain_domain_receiver` | N | — |
| authority.rs:14-43 | `revoke_domain_record` | N | — |
| grant.rs:179-188 | `domain_grant_task`: `native-domains ∧ cellos_wide` | already x86 | — |
| grant.rs:203-211 | `domain_grant_lifecycle_supported` (`cfg!`) | N | The single switch; doc `:190-202` says "x86_64 stays closed … no root-switch witness" — stale (`S22-X86-DOMAIN-LIVE`, `scripts/x86/qemu-domain-test.sh:205-227`) |
| grant.rs:248-276 | `domain_grant_records_live` (`cellos_wide`) | already x86 | — |
| grant.rs:1889-1912 | GrantAlloc domain branch | N | — |
| grant.rs:1916-1929 | GrantAlloc owner map | N | — |
| grant.rs:1983-2031 | GrantShare domain pair | N | — |
| grant.rs:2089-2108 | GrantSlice domain receiver | N | — |
| grant.rs:2263-2302 | GrantRegister domain branch plus owner map | N | — |
| grant.rs:1084 | x86 BAR-assigned check in DMA publication | U | `GrantDma` path |
| grant.rs:1685, 1712 | x86 `AnonMemory` | U | Not grants |
| grant.rs:1732-1735 | AArch64 DMA granule | U/G | — |
| grant.rs:2212-2241 | AArch64 cache-sync | U/G | Cache maintenance; x86 returns `Unknown` |
| grant.rs:1280, 1293 | `getrandom-sas-test ∧ riscv64` | U | — |
| grant.rs:851, 867 | `not(riscv32)` | U | — |

### 1.5 `kernel/src/memory/address_space.rs`
| Lines | Gate / literal | Class | Reason |
|---|---|---|---|
| 10 | `USER_LIMIT = 1usize << 38` | **H** | §7 |
| 52-59 | `quarantined_frame_count` `cfg_attr(not(riscv64), allow(dead_code))` | N | Attribute only; reason text goes stale |
| 61-75 | `asid_width()`: x86 → 12, else 16 | G→H | Per-arch tag width; HAL already has `PCID_WIDTH_BITS = 12` (`hal/arch/x86/src/x86_64/domain.rs:27`) |
| 79-86 | `domain_identity_counter` riscv64 | N (fixture) | x86/AArch64 substitute `frames_in_use` (`loader/domain_admission.rs:607-617`) |
| 150-168 | `SupervisorMapping::identity_page` riscv64 | G (fixture) | Identity-map semantics |
| 392-399 | x86 `copy_kernel_higher_half` into the private PML4 | G | Already present |
| 417 | `mapping.virtual_address < 0x0000_8000_0000_0000` | **H** | x86 canonical lower-half literal in shared code |
| 692-699, 706-712 | `cfg_attr(not(riscv64), allow(dead_code))` on `owned_frame_count` / `release_reserved_address` | N | Attribute only; `release_reserved_address` becomes live on x86 (`domain_grant.rs:261,286`) |
| 1388-1398 | `live_tag_owner` riscv64 | N (fixture) | — |
| 1514-1530 | `FixtureAsidLease` aarch64 | G (fixture) | AArch64 invalidation witness |
| 1701-1716 | `fail_allocation_after` / `fail_next_map` `native-domains ∧ test-hooks ∧ riscv64` | N | Fault-injection controls; needed on x86 (§5.3) |
| 1717-1719 | `address_space_tests` riscv64 | N/H | `hal::common::timer::read_mtime` (`address_space_tests.rs:349-357`) needs a neutral counter |
| 1724-1762 | `tag_policy_tests` x86 host | G | — |

### 1.6 `kernel/src/memory/tlb_shootdown.rs`
| Lines | Gate | Class | Reason |
|---|---|---|---|
| 23-24, 84-87, 386-389 | `TEST_SKIP_REMOTE_RFENCE` riscv64 | G | SBI RFENCE negative control |
| 35-47, 119-130, 182-185 | `TEST_WITHHOLD_TAG_ACK` riscv64 | **N** | The deferred-release fault-injection seam, needed on x86 (§3, §5.3) |
| 73-76 | `test_tag_flush_observed` riscv64 | N | — |
| 133-136 | unconfirmed log: rv64 ∨ x86 | x86 already | Logs on x86 test-hooks — interacts with the lane fail rule (risk R5) |
| 226-345 | `cellos_wide` + per-arch `counter_now` / `tag_probe_ticks` | G | x86 arms exist (`:288-291`, `:335-340`) |
| 372-419 | `flush_range` per arch | G | x86 shared-mapping fail-stop panic (`:405-419`) |

### 1.7 `kernel/src/memory/deferred_release.rs`
| Lines | Gate | Class |
|---|---|---|
| 295-298 | withhold-ack check riscv64 | N (with §1.6) |
| 500-595 | eight `cfg_attr(not(riscv64), allow(dead_code))` | N (attributes) |
| module | `native-domains ∧ cellos_wide` (`memory.rs:10-11`) | already x86 |

The reaper runs on hart 0 for `native-domains ∧ cellos_wide`, x86 included (`task.rs:1064-1068`).

### 1.8 `kernel/src/task/user_copy/{range,guard}.rs` (+ `mod.rs`, `page_probe.rs`)
| File:lines | Gate / literal | Class | Reason |
|---|---|---|---|
| range.rs:8-11, 95 | `USER_LIMIT = 1usize << 38` | **H** | §7 |
| guard.rs:1-4 | Header: "RV64 trap handler routes guard faults" | stale doc | x86 already routes them (`hal/arch/x86/src/x86_64/idt/dispatch.rs:34-37`); x86 `guarded_byte_copy` exists (`kernel/src/arch/x86_64/user_access.rs:20-58`) |
| guard.rs:57-58 | `riscv64` `guard_open` (SUM) | G | x86 STAC only under `x86-smap` |
| guard.rs:89-113 | `forced_guard_fault_recovers_for_test` riscv64 | N (fixture) | Body is neutral (`phys_to_virt`), but the hole PA must be unmapped in the x86 HHDM (risk R7) |
| mod.rs:51-71 | fixture re-exports riscv64 | N | — |
| page_probe.rs:123-142 | `stage_domain_for_test` riscv64 | N | — |

### 1.9 `kernel/src/loader/domain_admission.rs` (no change needed for grants)
| Lines | Gate | Class |
|---|---|---|
| 146-150 | `switch_ordering_qualified`: rv64 ∨ **x86_64** ∨ aarch64∧test-hooks | x86 already production-qualified (doc `:141-145`) |
| 157-162 | AArch64-production pin | G |
| 364-367 / 375-378 | AArch64 EL2 refusal / x86 `kernel_cr3()==0 ∨ !x86_substrate_qualified()` | G |
| 406-423 | `ADMISSION_TAG` incl. `X86` | done |
| 524-617 | x86/AArch64 `frames_in_use` vs RV64 `domain_identity_counter` | G (fixture) |

Admission is not relaxed by this phase: grants change no admission predicate. `domain_grant_records_live` only blocks on non-managed records (`grant.rs:238-267`, `mod.rs:299-309`).

## 2. Grant syscalls and ABI

| Syscall | No. | Bit | Decode | Dispatch |
|---|---|---|---|---|
| GrantAlloc | 208 | 39 | `decode.rs:1264` | `mod.rs:1693` → `grant.rs:1870` |
| GrantShare | 209 | 39 | `decode.rs:1265` | `mod.rs:1694` → `grant.rs:1966` |
| GrantSlice | 210 | 39 | `decode.rs:1270` | `mod.rs:1700` → `grant.rs:~2060` |
| GrantFree | 211 | 39 | `decode.rs:1276` | `mod.rs:1707` → `grant.rs:2131` |
| GrantRegister | 215 | 39 | `decode.rs:1302` | → `grant.rs:2245` |
| GrantUnregister | 216 | 39 | `decode.rs:1303` | → `grant.rs:2339` |

The table rows are at `libs/abi/abi/syscalls.toml:868-940,993-1014`. The witness cells use only these numbers plus Log/Exit/StateRestore/GetProcs/Send/Recv (`cells/tests/tier2-grant-owner/src/main.rs:75-88`; `tier2-grant-receiver/src/main.rs:75-84`).

Retired (`e88613a26b`, `git log -S'Syscall::Map {'`): the kernel-internal `Lend`/`BorrowRead`/`BorrowWrite`/`Grant`/`Map` variants and the per-task lease/grant tables. They were `[[internal]]` variants with no number. The parent decode has no `Syscall::Grant {`/`Syscall::Map {` producer (grep of `git show e88613a26b^:kernel/src/task/syscall/decode.rs`: 0 hits). The phase log confirms this (`phase-11-ipc-model.md`, diff line "Dead kernel mechanisms removed …"). The retired numbers (2, 3, 6, 206, 218, 310, 311; `syscalls.toml:50-127,767-787,842-850,1033-1040`) are none of the grant numbers.

**Verdict: no ABI or syscall change is needed. No Law-1 stop.**

## 3. Arch-specific calls inside the grant code, and the x86 equivalents

| Operation | Call site | RV64 | AArch64 | x86_64 (exists today) |
|---|---|---|---|---|
| Local page flush after a new receiver/owner PTE | `domain_grant.rs:199`, `syscall/mod.rs:225` → `hal::paging::flush_tlb_page` | `sfence.vma va, zero` (`hal/arch/riscv/src/rv64/paging.rs:24-30`) | `tlbi` | `invlpg` (`hal/arch/x86/src/x86_64/paging.rs:129-135`) — current PCID only. Sufficient: the PTE is new (not-present→present), and the receiver root is not current on the syscall CR3 (`syscall.rs:363`) |
| Tag invalidation after unmap (probe) | `address_space.rs:1151,1203` → `tlb_shootdown::confirm_tag_invalidation` (`tlb_shootdown.rs:110-148`) | `hal::domain::flush_asid` + SBI/IPI epoch | TLBI by ASID + SGI epoch | `flush_asid`: INVPCID type 1 when PCID is usable, else CR3 reload (`hal/arch/x86/src/x86_64/domain.rs:198-217`). Remote: IPI epoch (`sched/smp.rs:149-155,194-206`), serviced by `tlb_flush_all` → `flush_all_global` (INVPCID type 2) (`arch/x86_64/tlb.rs:6-8`) |
| Probe bound | `tlb_shootdown.rs:112` | 200 ms (`:324-327`) | freq/5 (`:330-333`) | 2·10⁹ TSC ticks (`:335-340`) |
| Defer on probe failure | `address_space.rs:1203-1214` → `deferred_release::defer_frames` (`deferred_release.rs:102`) | generic | generic | generic (`cellos_wide`, `memory.rs:10-11`) |
| Deferred-release reaper | `task.rs:1064-1068` | hart-0 timer | hart-0 timer | hart-0 timer (same cfg) |
| Revoke completion check | `domain_grant.rs:255,282` → `tag_invalidation_unconfirmed` (`deferred_release.rs:277-287`) | generic | generic | generic |
| Tag lease (ASID/PCID) | `AsidLease` (`address_space.rs:1405-1466`), width via `asid_width()` (`:69-75`) | 16-bit | 16-bit | 12-bit PCID; `cr3_for` drops the tag when PCID is unusable (`hal/arch/x86/src/x86_64/domain.rs:97`; tests `address_space.rs:1724-1762`) |
| Non-global private leaves | `user_flags` adds `NON_GLOBAL` (`address_space.rs:1591-1599`) | — | `PTE_nG` | G bit only if `PageFlags::GLOBAL` (`hal/arch/x86/src/x86_64/paging.rs:186-190`) |
| Deferred-path test seam | `TEST_WITHHOLD_TAG_ACK` | yes (`tlb_shootdown.rs:35-47`) | **no** | **no** — riscv64 only (`:119-130,182-185`; `deferred_release.rs:295-298`) |

**Probe-then-defer on x86: yes, already wired.** It is the same code path RV64 uses (`confirm_tag_invalidation` → `defer_frames` → reaper), and x86 takes it today for private-root teardown and lease release (`address_space.rs:1433-1466`). The lane asserts the outcome: `S22-X86-DOMAIN-TEARDOWN: PASS`, nothing quarantined (`qemu-domain-test.sh:273`). The plan's "tlb_shootdown.rs x86 branch" is `flush_range` (`:405-419`), the **shared-mapping** fail-stop that panics. Grants never use it for private roots. Step 3 therefore adds no invalidation mechanism. It needs:
1. the withhold-ack seam on x86;
2. an x86 deferred-path fixture;
3. the lane carve-out (R5).

Not on the private-root path but reachable from grant free: `free_grant_pages_inner` maps every freed frame `f→f` kernel-RWX in `KERNEL_ROOT` and does a local-only `tlb_flush_all` (`grant.rs:1239-1264`). See risk R3.

## 4. Audit H-5 per-record gate

| Item | Where |
|---|---|
| Gate type `GrantLifecycle { state: AtomicU8, gate: Spinlock<()> }` | `task/domain-grant-gate.rs:35-39` |
| Invariant and lock order (table → gate → receiver slot → root locks/`KERNEL_ROOT`; revoke takes the gate with no table lock; never `SCHEDULER`; gate-holder waits bounded) | `domain-grant-gate.rs:11-23` |
| `while_live` (Live check + publish under the gate) | `:61-71` |
| `serialized` (drain under the gate) | `:75-78` |
| `begin_revoke` (only `Live→Revoking`, under the gate) | `:82-91` |
| `finish_revoke` | `:94-97` |
| Publish uses it | `DomainGrant::publish` → `while_live` (`domain_grant.rs:154-164`); body `publish_live` maps, then records the receiver (`:167-224`) |
| Callers of publish | GrantShare row `syscall/mod.rs:749-756`; GrantSlice row `syscall/mod.rs:834` (both under the table lock) |
| Revoke uses it | `DomainGrant::revoke`: `begin_revoke` → `drain_receiver` (serialized) → `drain_owner` → `finish_revoke` (`domain_grant.rs:295-305`) |
| Callers of revoke | `revoke_domain_record` (`syscall/authority.rs:21-32`) from GrantFree `grant.rs:2173`, GrantUnregister `:422`, task-exit `syscall/mod.rs:340`, deferred sweep `grant.rs:698,725`. Receiver death drains only: `grant.rs:536` → `misc.rs:93-103` |
| Host proof | `task/domain-grant-gate-tests.rs:57-105` (500-round race; already built on the x86 host via `cfg(test)`, `task.rs:22-33`) |
| Audit record | `docs/project-changelog.md:80`; `docs/security-model.md:53` |

**What an x86 port must carry:** compile the **same** module, with no fork and no copy (F2 shows it compiles unchanged). Keep `publish` as the only receiver-map entry point. Keep revoke outside the table locks. Keep receiver drains `serialized`. x86-specific wrinkle: the gate is a `Spinlock`, which masks interrupts (`sync.rs:173-176` `saved_int`). On a re-share, the gate holder runs `confirm_tag_invalidation` (`domain_grant.rs:186` → `:245` → `address_space.rs:1203`), whose x86 bound is 2·10⁹ TSC ticks against RV64's 200 ms (risk R4).

## 5. Fixtures, markers, controls, x86 lane

### 5.1 Existing grant fixtures and markers
| Fixture | Arch / lane | Markers | Source |
|---|---|---|---|
| Lifecycle matrix (kernel) | RV64 `--case grant-revoke`; AArch64 test-hooks | `S22-{RV64,AARCH64}-GRANT-REVOKE[-OWNER-MAPPED,-OWNER-SLICE,-SLICE-RO,-SLICE-RW,-WO-REFUSED,-FOREIGN-PEER,-REVOKE,-FRAME-REUSE,-PARTIAL-MAP,-DEAD-ROOT]: PASS`, plus `-DMA-QUARANTINE: DENY` | `domain_grant.rs:392-585`; call sites `arch/riscv64/boot.rs:249`, `arch/aarch64/boot.rs:212` |
| Capability gate (kernel) | same | `…-GRANT-GATE[-ALLOC,-REGISTER,-WO,-SHARE,-SLICE,-RETIRED,-SAS,-RETIRE-REFUSAL,-FRAMES]`, `-RETIRE-OUTCOME: COMPLETED|DEFERRED` | `grant_gate_selftest.rs:404-460`; `arch/riscv64/boot.rs:250`, `arch/aarch64/boot.rs:213`; lane `qemu-aarch64-test-hooks.sh` (GRANT-GATE-* list) |
| Reclaim (SAS) | all wide incl. x86 | `GRANT-RECLAIM-{OWNED,RECEIVED,PINNED}` | `grant_reclaim_selftest.rs:111-130`; `main.rs:279` |
| Two-cell public pair | RV64 interactive (red); AArch64 boot order (fixture launcher) | `S22-<arch>-GRANT-PAIR-{OWNER-*,RECEIVER-*,HANDOFF}` | `cells/tests/tier2-grant-{owner,receiver}`; `qemu-native-domain-test.sh:284-422`; `qemu-aarch64-test-hooks.sh:150-262,446-480` |
| Copy fault injection | RV64 only | `S22-RV64-COPY`, `S22-RV64-COPY-RACE` | `user_copy_tests.rs:92-430,479+` |
| Deferred release | RV64 only | `S22-RV64-ASPACE` (deferred block) | `address_space_tests.rs:295-360` |
| x86 today | — | **none** for grants: `fixtures_domain` is empty | `arch/x86_64/boot.rs:699` |

### 5.2 Required negative cases → current witness
| Case | Guarded check | Current witness |
|---|---|---|
| Revoke while mapped | `revoke` drains the receiver before the owner (`domain_grant.rs:295-305,233-267`) | `GRANT-REVOKE-REVOKE` (`:494-508`); `GRANT-GATE-RETIRE-REFUSAL` (`grant_gate_selftest.rs:174-196`); pair `RECEIVER-REVOKE-FAULT`, `-UNREGISTER-FAULT`, `-DOWNGRADE-WRITE` (`tier2-grant-receiver/src/main.rs:337-341`; RV64 script `:371-400`) |
| Receiver death | `clear_grantee_refs` → `drain_domain_receiver` (`grant.rs:495-538`, `misc.rs:93-103`) | **No explicit domain fixture.** Implicit only: every pair generation dies and the next re-shares. `GRANT-RECLAIM-RECEIVED` is SAS-only (`grant_reclaim_selftest.rs:88-89`) |
| Stale grant id | Row lookup and `Live` state (`syscall/mod.rs:788-799`); live binding/generation (`:820-822`) | `RECEIVER-SLICE-UNKNOWN` (`IMPOSSIBLE_ID=0x1000`, `tier2-grant-receiver/src/main.rs:123-125,302-309`); `RECEIVER-FRAME-REUSE` (`:329-334`); `GATE-RETIRE-REFUSAL` |
| Cross-domain forgery | Owner check (`syscall/mod.rs:737`), `shared_to` check (`:811-816`), `expected_owner_tid` (`:789`), mixed-pair refusal (`grant.rs:1990-1999`) | `GATE-SHARE`/`GATE-SLICE` (`grant_gate_selftest.rs:257-271`); `REVOKE-FOREIGN-PEER` (`domain_grant.rs:480-492`); pair `OWNER-SHARE-FOREIGN` |
| Fault during copy | Commit re-check plus guard window (`user_copy/guard.rs:15-40`) | `S22-RV64-COPY` forced fault (`user_copy_tests.rs:300-335`) |

### 5.3 Fault-injection controls that exist (none disables a grant check)
| Control | Def | Used by |
|---|---|---|
| `fail_allocation_after`, `fail_next_map` | `address_space.rs:1701-1716` (riscv64), consumed `:176-186,1644-1647` | `address_space_tests.rs:29-36` |
| `set_test_withhold_tag_ack` | `tlb_shootdown.rs:35-47` (riscv64) | `address_space_tests.rs:327,347` |
| `set_test_skip_remote_rfence` | `tlb_shootdown.rs:84-87` (riscv64) | `arch/riscv64/memory/tlb_shootdown_selftest.rs:179-188` |
| `force_unmap_without_drain_for_test` | `address_space.rs:1075-1100` (test-hooks, **any arch**) | `user_copy_tests.rs:312-323`; control = the same transaction without injection commits (`:336-358`) |

The plan says "fault-injection control already used on RV64" (`phase-15-x86-tier2-parity.md:61`). That holds for copy, aspace and TLB, **not** for the grant checks. Step 4 must add `cfg(feature = "test-hooks")` switches, default off, one per guarded check above. Each fixture then runs twice: control off → `PASS`; control on → the fixture must report the violation (`…-CONTROL-<case>: CAUGHT`). The lane fails if a control run reports `PASS`.

### 5.4 x86 domain lane today
- Build: `scripts/build-x86_64-domain-test-ci.sh`.
  - Builds `fixture-launcher --features tier2-entry,tier2-rpc-entry` (`:56-58`) and the cells `tier2-smoke`, `tier2-exploit`, `cpp-smoke` and `tier2-rpc-*` (`:68-77`).
  - The image lists 18 cells and contains no `/bin/silo` or `/bin/net-broker` (`:79-118`).
  - Kernel: `--features test-hooks` (default features incl. `native-domains`, `kernel/Cargo.toml:81`), isolated target dir (`:177-185`).
- Run: `scripts/x86/qemu-domain-test.sh`.
  - Knobs: `X86_ACCEL` (`""|kvm|tcg`, `:42,54-57`); KVM implies `-cpu host` and `X86_EXPECT_PCID=1` (`:46-52`); `X86_CPUS` 1..64 (`:44,62-66`).
  - Asserts exactly **1** `[fault] Cell`, at `addr=0x0` (`:157-168`).
  - Fails on **any** "invalidation unconfirmed" (`:290-293`).
  - Required markers (`:230-275`); SMP extras for ≥2 CPUs (`:276-284`).
- CI: TCG, 1 CPU only (`.github/workflows/ci.yml:1363-1367`).
- `lanes.toml`: build `:73-76` and run `:381-384` are both `pr`.
- Host: `/dev/kvm` is `root:kvm` and this user is in `kvm` (checked: accessible).

**Adding the grant suite (x86):**
1. Kernel fixtures: wire `domain_grant::run_selftest` and `grant_gate_selftest::run_primary` (plus the new negative/control fixtures) into `arch/x86_64/boot.rs:699`, mirroring `arch/aarch64/boot.rs:197-215`. Markers become `S22-X86-GRANT-*`.
2. Public pair: build `fixture-launcher --features tier2-entry,tier2-rpc-entry,tier2-grant-pair` and add the owner at `/bin/silo` and the receiver at `/bin/net-broker`. The launch edges are arch-neutral under test-hooks (`kernel/src/loader/launch_profile/profiles.rs:83-95`), and the launcher's pair module is arch-neutral (`cells/tests/fixture-launcher/src/main.rs:158-259`, `Cargo.toml:21-24`).
3. `qemu-domain-test.sh`:
   - replace the fault rule `:157-168` with the AArch64 accounting: 1 NULL + 5 pair faults classified by `HANDOFF` address and ordered after their announcements (`qemu-aarch64-test-hooks.sh:150-262`);
   - keep the cause regex generic (the RV64 script hardcodes `cause=0xf`, `qemu-native-domain-test.sh:307`);
   - add the `S22-X86-GRANT-{REVOKE,GATE}…: PASS`, pair and `-CONTROL-*: CAUGHT` markers;
   - carve the withhold-ack window out of the `:290-293` rule (R5).
4. Matrix: `X86_CPUS ∈ {1,2,4}` × `X86_ACCEL ∈ {tcg,kvm}`. The script already supports both; the evidence run loops them, and CI stays TCG.

## 6. Witness cells on x86_64

| Cell | Command (lane flags, `CARGO_TARGET_DIR=/tmp/p15/ct`, `CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS="-C relocation-model=pic"` per `build-x86_64-domain-test-ci.sh:40`) | Result |
|---|---|---|
| tier2-grant-owner | `cargo check/build --release --target x86_64-unknown-none -Z build-std=core,alloc -p …` | OK; `ELF 64-bit LSB pie executable, x86-64, static-pie` (39000 B) |
| tier2-grant-receiver | same | OK; static-pie (47624 B) |
| tier2-smoke | same (check) | OK (already shipped in the x86 lane, `:68-69`) |

**Nothing blocks the build.**
- x86 tags already exist: `S22-X86` (`tier2-grant-owner/src/main.rs:100-101`; `tier2-grant-receiver/src/main.rs:95-96`). The owner doc's marker grammar still says "`RV64` or `AARCH64`" (`owner main.rs:38-39`).
- Domain class comes from the manifest `tier = PROTECTION_CLASS_UNTRUSTED` (`owner main.rs:68-73`).
- Runtime caveat: the receiver's `IMPOSSIBLE_ID = 0x1000` (`receiver main.rs:123-125`) is a manageable frame on x86, where only frame 0 is excluded (`memory/frame.rs:984-993`). The DENY still holds, because a slice needs `shared_to == caller` (`syscall/mod.rs:811-816`). Low risk; noted as R9.

## 7. User limit

| Site | Literal | Use | User-limit? |
|---|---|---|---|
| `memory/address_space.rs:10`, used `:1533` | `1usize << 38` | `validate_user_mapping` (every private/grant map) | yes |
| `task/user_copy/range.rs:8-11`, used `:95` | `1usize << 38` | copy-range end bound | yes |
| `memory/address_space_tests.rs:426` | `USER_LIMIT` | fixture | yes (follows) |
| `task/user_copy_tests.rs:105-106` | `1 << 39`, `(1usize << 38) - 4` | fixture | yes |
| `loader/image-va.rs:25-30`, used `:86`; tests `image-va-tests.rs:90-91` | `0x40_0000_0000` (= 1<<38) as `USER_VADDR_MAX`; 32-bit → `usize::MAX` | image placement ceiling | yes (hex form) |
| `loader/va_alloc.rs:41,72` | `0x40_0000_0000` static assert | slot table fits the user half | yes (hex form) |
| `memory/address_space.rs:417` | `0x0000_8000_0000_0000` | filters supervisor mappings to the lower half (x86 canonical) | related, lower-half bound |
| Excluded | `dma_export.rs:43-45` (IOVA window), `dev/iommu_pt.rs:787`, intel-gpu/mesa/libs IOVAs, virtio `NOTIFICATION_DATA` (`third_party/virtio_drivers/**`) | not user VA | no |

**What the HAL exposes today:** nothing.
- No `USER_LIMIT` or VA-width constant in `hal/arch/riscv/src/rv64/paging.rs` (Sv39, `:1`).
- None in `hal/arch/x86/src/x86_64/paging.rs` (4-level, `:1`).
- AArch64 has only the runtime TTBR0 config (`t0sz` 25 → 39-bit or 16 → 48-bit, `hal/arch/arm/src/aarch64/translation_config.rs:9-12,38-70`).
- The kernel reaches each arch module as `crate::hal::paging` (`hal/core/src/lib.rs:28-45`; e.g. `domain_grant.rs:199`, `memory/paging.rs:479`).

**Proposal:** `pub const USER_LIMIT: usize` in each 64-bit HAL `paging` module. Kernel sites use `crate::hal::paging::USER_LIMIT`; `image-va.rs` keeps its 32-bit `usize::MAX` arm; `va_alloc.rs:72` asserts against it.

| HAL | Value (step 2, behaviour-preserving) | Hardware bound |
|---|---|---|
| rv64 | `1 << 38` | Sv39 lower half |
| aarch64 | `1 << 38` | ≤ default TTBR0 39-bit |
| x86_64 | `1 << 38` | 4-level canonical `1 << 47` |

Raising x86 to `1 << 47` changes x86 user-copy and map validation outside grants. Under the S7b approval that is a pause point, so record it as a decision (D2), not part of step 2. Add `hal::paging::USER_HALF_END` (x86 `1<<47`) for `address_space.rs:417`, or fold that site into `USER_LIMIT`. Success criterion "no `1 << 38` outside the RV64 HAL" (`phase-15…md:79`): this proposal also places one in the AArch64 HAL, so either accept that or derive AArch64 from `TranslationConfig` (D3).

## 8. RV64 `grant-pair` launch

- Script: `printf 'tier2-smoke &\n'` (`scripts/qemu-native-domain-test.sh:338-341`).
- Shell route:
  - `Ast::Background` sets `bg_spawn` (`cells/tools/cellbox/src/executor.rs:1380-1411`) → `exec_cmd` → `dispatch_builtin` → `spawn_external`;
  - with a serial session, `spawn_external` always goes to `spawn_serial_foreground` (`:2902-2905`), and `run_foreground` refuses whenever `bg_spawn` is set (`:4275-4279`).
- The image has a serial session by design: `/bin/input` is packaged for exactly that (`scripts/build-native-domain-test-ci.sh:47-57,104-106`).
- **No syntax works.**
  - Redirections only capture **built-in** output into VFS and still dispatch the same way (`executor.rs:2370-2420`).
  - `exec` in a session also calls `spawn_external` (`:2611-2617`).
  - `hv` spawns detached but is hard-wired to `/bin/hypervisor` (`:3011-3050`).
- The cellbox spec requires explicit nonterminal streams for `&` (`.agents/261020-1750-cellbox-cli/spec.md:48`). Building it is "H3 … a product change under CBX-09 and needs its own CP1" (`.agents/261020-1750-cellbox-cli/phase-06-s5-proposal.md:18,59`).

**Minimal fix (script-only, mirrors AArch64):**
1. `build-native-domain-test-ci.sh`:
   - build `fixture-launcher --features tier2-grant-pair` into its own target dir, so the shared test-hooks image's launcher (`build-test-hooks-ci.sh:50,102`) is untouched;
   - pack it as `/bin/fixture-launcher`;
   - install the owner at `/bin/silo` and the receiver at `/bin/net-broker` instead of `/bin/tier2-smoke`/`/bin/tier2-exploit` (`:101-102,117`). Neither real service is in this image (`:93-104`); the edges are test-hooks rows (`profiles.rs:83-95`).
2. `qemu-native-domain-test.sh`: replace the FIFO typing in `run_grant_pair_interactive` (`:284-422`) with passive waits. Keep the same marker order and `wait_fault` address classification (`:300-321`); the receiver gets its ids over IPC (`WANT_IDS`, `owner main.rs:293-299`). Update the usage text (`:40-46`).

Cost: the pair now runs in every case's boot of that image (+5 classified faults). No other RV64 case asserts on fault lines (grep `\[fault\] Cell` in that script: grant-pair only, `:307`).

## 9. Spec 22 statements to update (`docs/specs/22-native-domain-cell-implementation-gate.md`)

| Lines | Current text (gist) | Why stale / what changes |
|---|---|---|
| 3-15 | "domain grants, non-RV64 admission … gated off"; "keeps AArch64/x86_64 Tier-2 admission closed"; "RV64 is therefore the only architecture with Tier-2 admission" | x86 admission qualified (`domain_admission.rs:141-149`; lane `S22-X86-DOMAIN-LIVE`); x86 grants added by this phase |
| 85 | x86 row: "**Admission refused.** No x86_64 image has entered a real domain task … `INVPCID` path stays unexecuted" | Domain entry witnessed; KVM PCID-on path asserted (`qemu-domain-test.sh:188-227`); add grant/invalidation status |
| 161-163 | Admission "stays closed on AArch64 and x86_64 until §2.2 …" | x86 open |
| 165-171 | "Private-root grant lifecycle (… phase 03 — RV64 only)"; "AArch64 and x86_64 … keep the phase-01 sentinel" | Add x86 (and the existing AArch64 test-image reopen, `grant.rs:192-199`) |
| 172-174 | Record lives in `kernel/src/task/syscall.rs` | Now `task/syscall/{mod,grant}.rs` |
| 194-198 | Deferral cause "(the SBI remote-fence transport …)" | Arch-neutral wording: x86 is IPI epoch + INVPCID/CR3 |
| 204-206 | "a base at or above `USER_LIMIT`" | Name `hal::paging::USER_LIMIT` |
| 208-215 | grant-pair "drives it from the guest shell. The owner is backgrounded" | No longer possible (§8); boot-order launcher |
| 219-221 | Faults classified as `cause=0xf` | RV64 only; x86/AArch64 use a generic cause |
| 228-235 | Witnesses listed as `S22-RV64-*` only | Add `S22-X86-GRANT-*` and the control markers |

## 10. Implementation plan (steps 2-6, one commit each)

The decisions below are flagged for the owner (D1-D3). Under the S7b approval only D2 is a pause point. The other two are recorded as Decisions in the phase deviation log.

- **D1** — Does x86 production (no `test-hooks`) get the lifecycle? The AArch64 precedent is test-images only, because AArch64 admission is test-only (`grant.rs:192-199`). x86 admission is production (`domain_admission.rs:146-149`), so the consistent cfg is `native-domains ∧ x86_64`. This is "x86 grants" behaviour, within scope. F2 shows the production build compiles.
- **D2** — x86 `USER_LIMIT` value (§7). Keep `1<<38` in step 2.
- **D3** — the AArch64 `USER_LIMIT` source (§7).

1. **Step 2 — `refactor(kernel): one domain-grant cfg, HAL user limit, HHDM-safe owner backing`**
   - `kernel/build.rs:183-188`: emit `cfg(cellos_domain_grants)` = `native-domains ∧ cellos_wide ∧ ¬(aarch64 ∧ ¬test-hooks)` (or an x86-test-hooks-only variant per D1).
   - Replace the 30 predicates in §1.1/§1.4 (`task.rs:14-34`; `syscall/mod.rs` 10 sites; `misc.rs` 6; `authority.rs` 2; `grant.rs` 9 incl. `cfg!` `:204-210`). This lowers the `target_arch` count; lower `arch_cfg_max` in `scripts/check-hal-boundaries.sh:69`.
   - HAL `USER_LIMIT` in three paging modules; replace every site in §7.
   - `syscall/mod.rs:266-270` → `phys_to_virt(paddr)` (F3).
   - Fixtures: `ARCH_TAG` `S22-X86`; replace the `mapping_state` `present` assertions with "not USER in the SAS root" (F4); `domain_grant.rs:513-515` → `phys_to_virt`; wire `arch/x86_64/boot.rs:699`.
   - Refresh the stale docs: `grant.rs:190-202`, `syscall/mod.rs:161-173`, `domain_grant.rs:16-20,316-319`, `user_copy/guard.rs:1-4`, `tier2-grant-owner/src/main.rs:38-39`.
   - Verify: x86 build + `qemu-domain-test.sh` at 1 CPU TCG shows `S22-X86-GRANT-REVOKE: PASS` and `S22-X86-GRANT-GATE: PASS`; RV64/AArch64 builds; host tests.
2. **Step 3 — `test(kernel): x86 deferred tag invalidation for grant revoke`**
   - Widen `TEST_WITHHOLD_TAG_ACK` / `test_tag_flush_observed` to `cellos_wide` (`tlb_shootdown.rs:35-47,73-76,119-130,182-185`; `deferred_release.rs:295-298`).
   - Expose a neutral counter (`tlb_shootdown.rs:277-292`) so `address_space_tests.rs:349-357` stops using `read_mtime`.
   - Add an x86 grant deferred-revoke fixture: withhold ack → `GrantFree` returns `PermissionDenied`, the record stays `Revoking`, the frames are retained, the receiver slice is refused; release ack → the sweep frees. Assert the INVPCID/CR3 path ran (`ASID_FLUSHES`, `hal/arch/x86/src/x86_64/domain.rs:24,216`).
   - The H-5 module is untouched.
3. **Step 4 — `test(kernel): x86 grant negative fixtures with fault-injection controls`**
   - New test-hooks switches (§5.3) and a receiver-death fixture.
   - Add the forced-fault copy subset on x86 (choose a hole PA unmapped in the HHDM, or rely on `force_unmap_without_drain_for_test` alone, R7).
   - Add the pair cells and the `tier2-grant-pair` launcher to the x86 image.
   - Each case emits `PASS` plus `CONTROL-…: CAUGHT`.
4. **Step 5 — `ci(x86): grant suite at 1/2/4 CPUs on TCG and KVM; fix RV64 grant-pair launch`**
   - `build-x86_64-domain-test-ci.sh` (§5.4.2); `qemu-domain-test.sh` (§5.4.3); `lanes.toml:73-76,381-384` (stay `pr`).
   - RV64 `build-native-domain-test-ci.sh` + `qemu-native-domain-test.sh` (§8). This could be a separate commit, since it is a different lane.
5. **Step 6 — `docs(spec22): x86 Tier-2 grants`**
   - §9 rows; `docs/project-changelog.md`; `.agents/plan-portfolio.md`; phase file.

## 11. Risks

| # | Risk | Mitigation |
|---|---|---|
| R1 | F3 missed → kernel #PF on the first x86 domain `GrantAlloc` (production if D1 = production) | Ship the `phys_to_virt` fix in the same commit that widens the cfg |
| R2 | Per-predicate `target_arch` additions break the 775 ratchet (`check-hal-boundaries.sh:69-80`) | Single cfg alias (step 2) |
| R3 | `free_grant_pages_inner` maps freed frames `f→f` kernel-**RWX** in the x86 kernel-root lower half with a local-only flush (`grant.rs:1239-1264`): lower-half W^X debt on x86. A later identity read can "work by accident" (masks F4) | Use `phys_to_virt` in the fixtures. Track the RWX remap as a Phase 16 frame-lifetime item; do not widen it here |
| R4 | The x86 probe bound (2·10⁹ TSC, `tlb_shootdown.rs:335-340`) runs under the interrupt-masking gate spinlock on a re-share (`domain_grant.rs:186`; `sync.rs:176`): long masked spins at 2-4 CPUs on TCG | Measure at 4 CPUs on TCG; if needed, use a grant-path probe bound or drain before taking the gate (keeps H-5: drain stays `serialized`) |
| R5 | The x86 lane fails on any "invalidation unconfirmed" (`qemu-domain-test.sh:290-293`), and the withhold-ack seam logs exactly that (`tlb_shootdown.rs:133-146`) | Bracket the seam with fixture markers and exclude lines inside the bracket; still fail on any unconfirmed line outside it |
| R6 | The x86 lane asserts exactly one cell fault (`:157-168`); the pair adds 5 | AArch64-style address-classified accounting (`qemu-aarch64-test-hooks.sh:150-262`) |
| R7 | The `forced_guard_fault_recovers_for_test` hole PA `0x2000_0000` (`user_copy_tests.rs:33,331`) may be mapped in the Limine HHDM, so there is no fault and the guard probe goes false-red | Pick the PA from the memory map (first non-RAM gap), or keep only the `force_unmap` injection on x86 |
| R8 | VA = PA receiver identity (Spec 22 §2.5.5): on x86 hosts with RAM ≥ 64 GiB a grant PA can collide with cell slots (`va_alloc.rs:49` `CELL_VA_START = 0x10_0000_0000`) or exceed `USER_LIMIT` | Fail-closed today (`address_space.rs:1121-1132,1533`); the evidence ceiling is QEMU 256 MiB (`qemu-domain-test.sh:104`). Document in Spec 22 |
| R9 | `IMPOSSIBLE_ID = 0x1000` is a valid x86 frame (`frame.rs:984-993`) | The DENY still holds (shared_to check); optionally pick an id above the x86 RAM top |
| R10 | D1 production enablement widens the attack surface on shipped x86 images | Gate on D1; the fixtures plus controls are the safeguard (phase rollback: re-gate) |
| R11 | Cross-lane coupling: the RV64 grant-pair now runs on every boot of the native-domain image; the boot window grows (`BOOT_WINDOW` 55 s default, `qemu-native-domain-test.sh:13`) | Measure; raise the window for that lane only if needed |
| R12 | KVM at 2/4 vCPUs is untested for this image (CI is TCG/1 CPU, `ci.yml:1363-1367`) | Evidence run locally (KVM accessible); TCG is the CI ceiling |
| R13 | Intel WIP plans (`261104-0801`, `261104-0802`) touch `syscall/grant.rs` and `address_space.rs` (`s7b-handoff-request.md:69`) | Pause at a commit boundary per the approval |
