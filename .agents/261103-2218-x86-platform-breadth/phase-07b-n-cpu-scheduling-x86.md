---
phase: 7b
title: "N-CPU scheduling on x86"
status: implemented (S2 checkpoint: review PASS-with-fixes; owner decision pending)
priority: P1
effort: "5d"
dependencies: ["7a", 2]
tier: thinking
checkpoint: true
---

# Phase 07b: N-CPU scheduling on x86

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- `kernel/src/task/smp.rs` (IPI, TLB flush epochs, retirement switch, `start_secondaries`), `kernel/src/task/hart_local/ready.rs`
- `docs/specs/04-hardware.md` §5 (work stealing; RT tasks excluded)
- `scripts/x86/qemu-domain-test.sh:76-277` (topology gate), `tests/integration/src/lib.rs:882` (`boot_x86_bios_smp`)

## Overview

Bring every CPU the firmware reports online on x86, up to `MAX_HARTS`: logical 0 = boot normal hart, logical 1 = RT hart, logical 2..N = extra normal harts with work stealing. Closes S2.

## Key Insights

- The existing work-stealing scheme (RV64) handles two queues. Victim selection must generalise without changing the RT exclusion rule.
- TLB shootdown and retirement switches are per-hart epochs already. They need broadcast loops over online harts and a completion wait over a set instead of a single remote.
- kvmclock pages and the TSC sync check from phase 02 must run on every AP.

## Requirements

- Functional:
  - `start_secondaries` (x86) starts every Limine MP CPU up to `MAX_HARTS`. Extra CPUs are logged `[x86-smp] cpu N ignored: MAX_HARTS`. APIC ID above 255 without IR stays offline (phase 03 rule).
  - Work stealing: an idle normal hart steals from the busiest normal hart's queue (bounded scan, round-robin start). The RT hart neither steals nor is stolen from (unchanged rule).
  - TLB shootdown: IPI to every online hart except self; wait until all epochs complete, with the existing timeout semantics.
  - Retirement switch and preempt-pend: same generalisation.
  - Per-AP: kvmclock registration, TSC sync check, LAPIC timer/TSC-deadline init.
  - Topology log: `[x86-smp] topology online=N processor_ids=…`.
  - `cells/apps/c2c-render`: `ProcessorIds { ids: [Option<u32>; 2] }` (`cells/apps/c2c-render/src/main.rs:462-466`) is raised to the machine's online CPU count (bounded `Vec` or `MAX_HARTS`-sized array), and its smoke scripts (`scripts/x86/run-c2c-render-smoke.py`, `run-c2c-concurrent-input-smoke.py`) run at `-smp 4` (owner decision, validation 2026-11-03).
- Non-functional: no lost wakeups (existing tests); TLB shootdown latency at 8 CPUs recorded.

## Architecture

The scheduler's per-hart queues stay as they are; stealing becomes `steal_from(victims: impl Iterator)`. Shootdown waits over an `online_harts()` snapshot taken under the request epoch.

## Assumptions

- **Claim:** the RISC-V work-stealing implementation is shared code, not RV-only. **Confidence:** medium. **How to verify:** read `kernel/src/task/hart_local/ready.rs` and scheduler steal call sites for `cfg(target_arch)`.
- **Claim:** KVM on this host runs `-smp 8`. **Confidence:** high. **How to verify:** `nproc` ≥ 8 on the host.

## Related Code Files

- Modify: `kernel/src/task/smp.rs`, `kernel/src/task/hart_local/*.rs`, scheduler steal sites, `hal/arch/x86/src/x86_64/clock/kvmclock.rs`, `scripts/x86/qemu-domain-test.sh`, `tests/integration/src/lib.rs`, CI x86 job (add `-smp 4` lane), `cells/apps/c2c-render/src/main.rs`, `scripts/x86/run-c2c-render-smoke.py`, `scripts/x86/run-c2c-concurrent-input-smoke.py`

## Implementation Steps

1. Generalise shootdown and retirement-switch waits to sets; unit-test the epoch logic with a simulated 4-hart set.
2. Generalise stealing; keep the RT exclusion; unit-test victim selection.
3. Start all APs on x86; per-AP clock init.
4. Lanes: TCG `-smp 4`, KVM `-smp 8`; run `x86_64-boot`, `local-service-lifecycle-x86`, the C2C local suites, and the domain test with `online=N`.
5. Stress: run the existing lost-wakeup and preemption tests at `-smp 8` ten times.

## Todo List

- [x] Set-based shootdown/retirement (`RemoteEpochs`: broadcast, then wait over the set; retirement waits were already per-hart over all slots in pinned `scheduler.rs`, D7)
- [x] N-way stealing (bounded victim scan, RT tasks excluded, pair locked in hart-id order; signature kept)
- [x] All APs online + per-AP clock (sequential handshake; per-AP pvclock slot, TSC skew check and LAPIC/TSC-deadline tick)
- [x] Lanes + stress (see `reports/phase-07b-evidence.md`)

## Success Criteria

- [x] KVM `-smp 8`: `online=8`; TCG `-smp 4` green (lanes and limits in evidence).
- [x] Stress loop: 10/10 passes, zero `invalidation unconfirmed` lines. Final run after the review fixes and D12: 10 × (KVM `-smp 4` boot + domain lane `X86_CPUS=4` KVM), 10/10 + 10/10, 0 unconfirmed lines in 20 logs. The first run's 3/10 domain failures were fixture timing (D12).
- [x] riscv64/aarch64 lanes unchanged (still 2 harts).
- [x] Review: independent race review of shootdown and stealing — PASS-with-fixes, no Critical; Minor fixes D13–D15 applied. Batching ELF-load/W^X flushes (reviewer R1) is left to the owner.

## Risk Assessment

- Latent single-remote assumptions in code that only ever saw 2 harts. Mitigation: the stress loop plus review; any new race found is logged as a Surprise and fixed in this phase.
- Interaction with the locked TLB-shootdown plan. Do not start until its handoff is recorded.

## Security Considerations

A missed TLB shootdown can leave stale mappings into revoked cell memory, which is an isolation bug. The completion-wait timeout must stay fail-stop, not continue silently.

## Next Steps

S3 (VMware) can run with 4 vCPUs; phase 20 uses real core counts.

## Deviation Log

Line cites are as of the S2 request base `28dee8b3d`; the code was re-located by symbol on `b47a74e01`.

- **D1 Deviation (integrator grant 1):** `kernel/src/loader/atomic_publication_tests/denials.rs` — `let victim = 1 - hart` becomes `usize::from(hart == 0)` (the other member of the steal scan's always-visited pair {0, 1}).
  Why: 07a open risk; underflows for a hart ≥ 2. The probe only runs when the RT hart is offline, i.e. single-hart, so the value is unchanged (1).
  Impact: no behaviour change; same edit in `ready.rs::test_rollback_keeps_fifo_position` (in set).
  Revert: restore the line.
- **D2 Decision (quiesce bound):** bring-up does not cap online harts at `MAX_QUIESCE_HARTS` (8); x86 brings up to `MAX_HARTS` (64).
  Why: `quiesce.rs` (pinned) has no x86 park hook (`park_hook_available` is `cfg!(target_arch = "riscv64")`), so x86 already refuses every multi-hart set with `Unsupported` at 2..8 harts; above 8 it refuses with `TooManyHarts`. Both are fail-closed refusals of a snapshot capture; no lane runs it on x86.
  Impact: KVM `-smp 8` gives `online=8` either way; above 8 only the refusal variant changes. Recorded in `start_secondaries` docs. `quiesce.rs:53,366,1354` comments (07a S1) stay stale (pinned).
  Revert: cap the loop at 8 in `start_secondaries`.
- **D3 Deviation (integrator grant 2):** `cells/apps/c2c-render/src/main.rs` `save_ppm` reaches the file by handle under the cwd grant.
  Why: proven cause of `ERROR VFS write /tmp/c2c-render.ppm failed: Err(3)`: `ERR_DENIED = 3`; the VfsMutate check passes (`declare_syscalls![… VfsMutate]`, launch edge `CapSet::LEGACY_OBSERVE_MUTATE`); the shell passes its cwd to every foreground child, `reserve_prepared` seals it (`vfs/src/dirs/bind.rs:127`), and `dispatch.rs:67-68` refuses any path-addressed request from a sealed caller. The unmodified save reproduced Err(3) on TCG `--cpus 2`; the handle path passes.
  Impact: with a grant: `OpenDir` from the cwd grant, `WriteAt` header, `OpenFileAt`, 4 KiB `WriteGrant`s at explicit offsets, `StatAt`, close handles. Without a grant: the old path-string sequence. `WriteHandleGrant` is not used: the `/tmp` RAM backend has no positional write (`ERR_IO`). Same file: `ProcessorIds` raised to 64 entries.
  Revert: revert the file.
- **D4 Deviation (integrator grant, as 07a D7):** `scripts/mutable-globals.toml` text of `X86_READY`, `X86_QUALIFIED`, `X86_AP_ROOT_REFUSED`, `X86_SECONDARY` (now `mutation = "atomic"`), `HART_ONLINE`, `global_asm!#1`; `docs/generated/mutable-globals.md` regenerated with the tool.
  Why: those statics changed type or meaning (logical-id handshake words, one context per AP). `PVCLOCK_VIRT/PHYS` have no entry (they are among the 35 pre-existing unclassified statics), so nothing was edited for them. No new static was added.
  Impact: `--check`: `635 declarations, 252 need review, 37 problems`, identical to base `b47a74e01` (35 unclassified + 2 stale `IOAPIC_*` entries, all pre-existing).
  Revert: revert both files with the code.
- **D5 Deviation (owner grant `s2-phase07b-paging-grant.md`):** `kernel/src/memory/paging.rs` — `map_mmio_x86_flags` writes every leaf through a non-flushing `map_page_x86_unflushed`, then issues one `flush_range` for the window; `map_page_x86` is that variant plus its per-page flush (unchanged behaviour).
  Why: Surprise S1 below.
  Impact: one broadcast per MMIO window instead of one per page; fail-stop unchanged; host test `mmio_window_covers_every_mapped_page_once`.
  Revert: revert the two functions.
- **D6 Decision (RT rule wording):** the "unchanged rule" kept is the code's: RT *tasks* are never stolen. The RT hart (1) still steals and is stolen from for Normal/Background work, as on two harts. The phase text "The RT hart neither steals nor is stolen from" does not describe the current two-hart code; following it would idle hart 1 for normal work on riscv64/aarch64 and break the x86 handoff fixture (`steal_from_busiest(1)`).
  Impact: none at two harts. Revert: n/a.
- **D7 Decision (retirement / preempt waits):** no code change was needed for these waits. `scheduler.rs:2002-2036` (pinned) already arms and checks one retirement epoch per hart over `0..MAX_HARTS`; `request_preemption` is a per-hart mailbox + IPI with no wait. The set shape (`smp::RemoteEpochs`) is shared by the shootdown and the boot broadcast and is unit-tested on the real retirement epochs over four simulated harts.
  Revert: n/a.
- **D8 Decision (bring-up shape):** APs start one at a time in Limine MP order; each handshake word is the logical id of the AP in flight; the first AP that cannot be started or refuses root operation stops bring-up. The online set is always `0..online`, so the RT hart is online whenever any AP is. An AP that was released but never answers a step within its bounded budget panics the boot (`x86_bounded_wait`): fail-closed, never a silently missing CPU. After step 3 the BSP issues one catch-up flush (D14).
  Revert: n/a.
- **D9 Decision (pvclock):** the BSP's 4 KiB kvmclock page (allocated in `main.rs`, unchanged) is split into one 32-byte slot per logical CPU; each AP registers its own slot (the MSR is per vCPU) only while the BSP's registration is active, and a slot without the stable bit demotes a kvmclock clocksource. Reads keep using the BSP slot (stable-bit rule).
  Revert: drop `register_secondary_pvclock`.
- **D10 Decision (stress gate observability):** the `[tlb] tag … invalidation unconfirmed` log of `confirm_tag_invalidation` is extended from riscv64 to x86_64 under `test-hooks`, and `qemu-domain-test.sh` fails on any `invalidation unconfirmed` line.
  Revert: restore the cfg.
- **D11 Decision (fixtures):** the shootdown fixture still pairs the BSP with the RT hart only (other APs just acknowledge); its markers print the real count (`harts=N`). The AP's boot-complete wait gets `4 × MAX_HARTS` bounded budgets, because later APs come up while it waits.
  Revert: n/a.
- **S1 Surprise:** boot-to-prompt time grew with the CPU count (KVM 1/2/4/8: 3.5 / 7.3 / 89.7 / >180 s). Proven with `info registers -a` + `addr2line` during the stall: CPU1 in `tlb_shootdown::flush_range`, the BSP spinning on `FRAME_ALLOCATOR` in `prepare_elf_task`, CPUs 2-7 halted. Cause: the platform Cell's 256 MiB ECAM `RequestMmio` mapped 65,536 pages with one global shootdown each. Fixed under D5 (owner grant).
- **S2 Surprise:** `x86_64-boot` was not run by CI at all (the `qemu-x86_64-boot` job runs `scripts/qemu-x86_64-test.sh`); the new `-smp 4` step runs only the new test. `run-c2c-concurrent-input-smoke.py` passed no `-smp`; it now takes `--cpus` (default 4) and checks `online=N`.
- **S3 Surprise:** an early relative-path edit landed in `/home/dmin/cellos` (`smp.rs`, `tlb_shootdown.rs`; and the c2c helper's first `main.rs` hunk). Each was moved into the worktree and reverted there within minutes; `git status` of `/home/dmin/cellos` shows no tracked change.
- **D12 Deviation (integrator grant, after review):** `cells/tests/tier2-rpc-driver/src/main.rs` — the stale-descriptor leg waits on a wall-clock bound (`STALE_DEADLINE_MS = 5000` via `sys_get_time_ms`, `GetTime` added to `declare_syscalls!`, explicit fail without a clock) instead of 2000 yields; failure text: "the provider did not retire its registry entry within the 5 s bound".
  Why: stress root cause (integrator's debugger, proven): fixture timing, not kernel. At `-smp 4` KVM the driver burns 2000 yields in 1.3–1.8 ms on an idle hart while the provider is still Running in its last two serial `Log` syscalls (~1.5–2 ms each); the registry is cleared inside the provider's own `Exit` funnel within ~11 µs of the syscall. Base: 21/100 FAIL; with the fix 0/80.
  Impact: only the x86 domain-test image builds and runs this driver (`build-x86_64-domain-test-ci.sh`, init feature `tier2-rpc-entry`); no rv64/aarch64 lane uses it.
  Revert: restore the poll loop.
- **D13 Decision (review minor a):** the x86 self-service in `await_remote_invalidation` re-reads `current_hart_id()` each pass, with IF masked around the read and `service_tlb_flush`, rather than a `debug_assert!` that IF is clear: callers run with IF set as well as clear, and with IF set the waiter may resume on another CPU, where servicing the stale id would forge an acknowledgement for a hart this CPU never flushed.
- **D14 Decision (review minor b):** after the BSP observes `HART_ONLINE[logical]`, it issues one more `request_tlb_flush(logical)` and waits for it: until then every shootdown excluded the AP. The AP's boot-complete wait budget grows to `5 × MAX_HARTS`.
- **D15 Decision (review minor c):** a failed AP pvclock registration makes kvmclock unusable system-wide (`kvmclock::retire` clears the read pointer, so `fallback_source` and later APs see `now_ns() == 0`), after demoting an active kvmclock source; an active kvmclock with no fallback stays (retiring it would stall time). No new static.
