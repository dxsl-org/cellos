# Phase 07b evidence — N-CPU scheduling on x86

Branch `feat/x86-s2-phase07b-nsmp` from main `b47a74e01`. Code commit `c0fe65fce`. No Law-1 delta.
Ceilings: host tests, QEMU 8.2.2 TCG (`qemu64,+pdpe1gb`), local KVM (`-cpu host`; the host is WSL2, so KVM is nested under Hyper-V). Not physical evidence. Regenerated `kernel/src/embedded*/` were restored before committing.

## What changed

- `smp.rs`
  - x86 `start_secondaries` starts every Limine MP CPU up to `MAX_HARTS` (64), one at a time, in MP order. A CPU past the bound logs `[x86-smp] cpu N ignored: MAX_HARTS (apic_id=…)`. A LAPIC ID above 255 stays offline (phase 03). The first AP that cannot be started or that refuses root operation ends bring-up, so the online set is always `0..online` (D8).
  - Handshake words carry the logical id of the AP in flight: `X86_READY`, `X86_QUALIFIED` (monotone) and `X86_AP_ROOT_REFUSED`. `X86_SECONDARY` holds one context per slot.
  - `RemoteEpochs` adds a set of per-hart epochs: broadcast every request, then wait. It is used by the shootdown and by a boot-time broadcast proof, which logs `[x86-smp] transport IPI-invalidate broadcast targets=N-1 ack_ns=…`.
  - The shootdown fixture still pairs the BSP with the RT hart. Its markers now print `harts=N`.
- `tlb_shootdown.rs`: `await_remote_invalidation` broadcasts to every online remote hart first, then waits over that set under one deadline. It keeps the x86 self-service in the wait loop. Errors stay fail-stop: `flush_range` panics and tag releases defer. Under `test-hooks`, the `[tlb] tag … invalidation unconfirmed` log now also covers x86 (D10).
- `ready.rs`: `steal_from_busiest_with` keeps its signature.
  - `pick_victim` is a bounded scan: each other slot once, starting at `thief+1`, and ties go to the first slot found.
  - `steal_candidate`: harts 0 and 1 are always candidates; harts above 1 only while online.
  - RT tasks are never stolen.
  - At most two leaf locks are held, taken in ascending hart id.
- `clock/{kvmclock,mod}.rs`: each AP registers its own 32-byte slot of the BSP's 4 KiB pvclock page. An AP slot without the stable bit demotes a kvmclock source. The TSC skew check and the LAPIC/TSC-deadline tick were already per AP (D9).
- `paging.rs` (owner grant, D5): `map_mmio_x86_flags` writes every leaf, then issues one `flush_range` for the whole window.
- `kernel/src/loader/atomic_publication_tests/denials.rs:310` (grant 1, D1): the `1 - hart` victim index is removed. `c2c-render` (grant 2, D3): the image is saved through the cwd grant, and `ProcessorIds` holds 64 entries.
- Review fixes (second commit, PASS-with-fixes review):
  - D13: the shootdown wait services the CPU it is running on (`current_hart_id()` re-read each pass, IF masked around read + service), so a migrated waiter cannot forge an acknowledgement.
  - D14: after `HART_ONLINE[logical]` the BSP issues one more acknowledged flush to that AP; an AP that never answers a released step panics the boot (fail-closed).
  - D15: a failed AP pvclock registration retires kvmclock system-wide (`kvmclock::retire`) after demoting an active kvmclock source.
  - D12 (integrator grant): `tier2-rpc-driver` stale-descriptor leg waits on a 5 s wall-clock bound instead of 2000 yields.
- Lanes:
  - `qemu-domain-test.sh` accepts `X86_CPUS` 1..64. With 2 or more CPUs it requires `topology online=N`, `broadcast targets=N-1` and `harts=N` on both shootdown markers, and it fails on any `invalidation unconfirmed` line.
  - `x86_64-boot` adds `x86_smp_brings_every_cpu_online` (`CELLOS_X86_SMP`, default 4), and CI job `qemu-x86_64-boot` adds a step that runs it at `-smp 4`.
  - Both c2c smokes default to `--cpus 4` and check `online=N`.
- Records: `docs/specs/04-hardware.md` §5, `scripts/mutable-globals.toml` with the regenerated md (D4), and this phase's file.

## Host

Re-run after the review fixes, on the second commit:
- cellos-kernel host tests: `ok. 451 passed`; hal-x86: `ok. 26 passed`.
- clippy `-D warnings` for x86_64, riscv64 and aarch64 kernels, hal-x86 and `tier2-rpc-driver` (x86_64-unknown-none): each `Finished`.
- `cellos-sign --check --strict`: `OK: F5`, `OK: F1 … 77 allowlisted files`.
- Law-1 digests: all three OK/PASS.
- mutable-globals: `635 declarations, 252 need review, 37 problems`, unchanged; no new static (D15 reuses `kvmclock::INFO`).

Lines below are from the first commit:

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `test result: ok. 451 passed; 0 failed`. New tests, all `ok`:
  - `task::smp::tests::{epoch_set_broadcasts_before_waiting_and_completes_over_four_harts, epoch_set_ignores_stale_completions_and_out_of_range_targets, tlb_flush_set_over_four_simulated_harts, retirement_switch_set_over_four_simulated_harts}`
  - `task::hart_local::ready::tests::{victim_is_the_busiest_other_candidate_over_four_harts, victim_ties_rotate_with_the_thief, stealable_count_excludes_rt_and_unpermitted_tasks}`
  - `memory::paging::tests::mmio_window_covers_every_mapped_page_once`
- `cargo test -p hal-x86 --target x86_64-unknown-linux-gnu`: `test result: ok. 26 passed`.
- `cargo test -p app-c2c-render --target x86_64-unknown-linux-gnu` (run by the helper on the same patch): `ok. 8 passed`.
- clippy `-p cellos-kernel -- -D warnings`: x86_64-unknown-none, riscv64gc-unknown-none-elf and aarch64-unknown-none-softfloat each report `Finished` with no warning. `-p hal-x86` also reports `Finished`.
- `--features test-hooks`: `cargo check` is clean. clippy reports the same three errors as the base, all in untouched files (`domain_switch_tests.rs:27`, `svm_registry.rs:620`, `cell_session.rs:3398`).
- `cellos-sign --check --strict`:
  - `OK: F5 — rustc f53b654a8882 is the pinned nightly-2026-05-01`
  - `OK: F1 — 139 crates and 999 files scanned; unsafe confined to 77 allowlisted files`
- Law-1 digests:
  - `OK: the frozen AI interface matches its Law 1 record`
  - `PASS: LookupServiceBound Law-1 items intact …`
  - `PASS: multi-source WaitCompletion Law-1 items intact …`
- `tools/mutable-globals --check`: `635 declarations, 252 need review, 37 problems`. The base `b47a74e01` gives the same line (35 unclassified, 2 stale `IOAPIC_*`). No new static was added.
- Pinned files are byte-identical to `b47a74e01` (`git diff --quiet`): `task/{quiesce,cpu_accounting,scheduler}.rs`, `kernel/src/snapshot.rs` and `cells/services/vfs/**`. Opcode 502 is untouched.

## Boot-to-prompt time (canonical headless ISO, `ps` answered)

"Before" is 07b without D5; "after" is `c0fe65fce`. Each cell is one run; on this nested host, TCG times at 4 CPUs varied 24–73 s across runs before the fix and 4–25 s after it.

| | KVM 1 | KVM 2 | KVM 4 | KVM 8 | TCG 1 | TCG 2 | TCG 4 |
|---|---|---|---|---|---|---|---|
| before | 3.5 s | 7.3 s | 89.7 s | 226.5 s | 7.3 s | 17.9 s | 59.5 s |
| after | 2.3 s | 4.2 s | 36.1 s | 45.6 s | 3.7 s | 6.7 s | 24.0 s |

The cause was found from `info registers -a` plus `addr2line` during the stall:
- CPU1 was in `tlb_shootdown::flush_range`.
- The BSP was spinning on `FRAME_ALLOCATOR` in `prepare_elf_task`.
- CPUs 2–7 were halted.

The platform Cell's 256 MiB ECAM `RequestMmio` paid one global shootdown per page (65,536). Most of the remaining growth is per-page shootdowns in ELF loading and W^X lowering (open risk R1). Example after the fix, KVM 4: `ELF LOAD 0x100084000-0x101184040 flags=RW-` (vfs) took 5.7 s, and `[wx] 'vfs': 131/4486` took 4.3 s.

## Guest (images built as in CI job `qemu-x86_64-boot`, plus the domain-test, prod-refusal and c2c build scripts)

| Lane | Result | Decisive lines |
|---|---|---|
| TCG `-smp 4`, canonical ISO | PASS | `[x86-smp] transport IPI-invalidate broadcast targets=3 ack_ns=315240`, `[x86-smp] topology online=4 processor_ids=0;1;2;3 qualified=yes`, `[sched] hart 0/1/2/3 dispatched a task` |
| KVM `-smp 8`, canonical ISO | PASS (`online=8`, no cap: D2) | `[x86-gate] timer: clock=kvmclock clockevent=TscDeadline …`, `[x86-smp] AP logical=1..7 … pvclock=own`, `broadcast targets=7 ack_ns=9556883`, `topology online=8 processor_ids=0;1;2;3;4;5;6;7 qualified=yes`, harts 0-4,6,7 dispatched a task |
| `x86_64-boot` TCG (`CI=true`, prod-refusal ISO, `CELLOS_X86_SMP=4`) | `test result: ok. 10 passed` | includes `x86_smp_brings_every_cpu_online ... ok` |
| `x86_64-boot` KVM (same) | `test result: ok. 10 passed` | same |
| `x86_64-boot` `-smp 2` lane (regression helper) | `ok. 1 passed` | `online=2`, `broadcast targets=1` |
| domain lane `X86_CPUS=4` TCG | PASS | `S22-X86-CONTEXT-HANDOFF: PASS …`, `S22-X86-REMOTE-SHOOTDOWN: PASS harts=4 …`, `S22-X86-MASKED-SHOOTDOWN: PASS harts=4 …`, `S22-X86-DOMAIN-LIVE: PASS … hart=2 harts=4` (domain cells also ran on harts 3, 1, 0) |
| domain lane `X86_CPUS=4` KVM | PASS (before D5 as well) | `S22-X86-DOMAIN-LIVE: PASS … pcid=1 pcid_usable=true … harts=4` |
| domain lane `X86_CPUS=8` KVM | PASS | `REMOTE-SHOOTDOWN: PASS harts=8`, `MASKED-SHOOTDOWN: PASS harts=8`, `broadcast targets=7 ack_ns=90897`, `topology online=8` |
| c2c-render smoke `--cpus 4 --workers 4` TCG | PASS | `c2c-render: saved /tmp/c2c-render.ppm bytes=2304 save_ns=10766310`, `C2C_RENDER_SMOKE=PASS (… actual qualified online CPUs=4 processor_ids=0;1;2;3, per-worker-mode render endpoints on two or more admitted CPUs …)` |
| c2c-render smoke `--cpus 4 --workers 4` KVM | PASS | `saved … save_ns=598597`, `C2C_RENDER_SMOKE=PASS (… CPUs=4 processor_ids=0;1;2;3 …)` |
| c2c-render `--cpus 2` TCG/KVM (helper, base kernel + D3) | PASS | `saved /tmp/c2c-render.ppm bytes=2304`, `COMPARE OK baseline=copy=shared`; the unmodified save reproduced `Err(3)` |

## Stress

### Final run (after the review fixes and D12; images rebuilt from the second commit)

10 × [canonical headless ISO, KVM `-smp 4`, prompt + `ps` answered] + [domain lane `X86_CPUS=4 X86_ACCEL=kvm`]:

- Canonical boots: **10/10 PASS**. Prompt times: 60.1, 16.5, 3.6, 67.6, 75.6, 49.7, 8.9, 48.8, 48.8, 41.4 s. The spread comes from the per-page ELF/W^X shootdowns (open risk R1) on this nested host.
- Domain lane: **10/10** `PASS: x86_64 Tier-2 domain-entry lane (x86-domain-test-kvm-cpu4) — every marker present`. `[tier2-rpc] FAIL` appears 0 times.
- `invalidation unconfirmed` lines: **0** in all 20 logs (`STRESS domain pass=10 fail=0 unconfirmed_total=0`).
- Decisive lines, run 1: `S22-X86-REMOTE-SHOOTDOWN: PASS harts=4 …`, `[x86-smp] transport IPI-invalidate broadcast targets=3 ack_ns=60456`, `[x86-smp] topology online=4 processor_ids=0;1;2;3 qualified=yes`, `[tier2-rpc] PROVIDER-GONE=none`, `[tier2-rpc] DRIVER-DONE`.
- `x86_64-boot`, once each, with `CELLOS_X86_SMP=4`: TCG `test result: ok. 10 passed`, KVM `test result: ok. 10 passed`.

### First run (`c0fe65fce`, before D12) and root cause

Setup: 10 × [canonical TCG `-smp 4` boot + domain lane `X86_CPUS=4` KVM].
- Boots: 10/10.
- `invalidation unconfirmed` lines: 0.
- Domain lane: 7/10. Each of the 3 failures was `[tier2-rpc] FAIL — the fixture's registry entry stayed live after the provider exited`, followed by `Syscall::Exit: task 10 …` and `[service-registry] cleared stale entries for dead tid 10`.
- A 2-CPU control on the same kernel passed 4/4.

The integrator's debugger proved the cause is fixture timing, not the kernel:
- **What happened.** The driver's stale-descriptor leg counted 2000 `yield_now()` calls. On an otherwise idle hart these took 1.3–1.8 ms in total (about 667 ns per poll). Meanwhile the provider was still `Running` on its own hart, with all ready queues empty, inside its last two serial `Log` syscalls (about 1.5–2 ms each under nested KVM).
- **Exit path.** `clear_tid` runs synchronously inside the provider's own `Exit` funnel. The registry entry was gone within about 11 µs of the Exit syscall entry.
- **Why the other configurations passed.** On TCG one poll costs about 35 µs, so the budget is about 69 ms. At 2 CPUs the driver resumed only after the provider had already exited.

| Debugger run | FAIL |
|---|---|
| Baseline | 21/100 |
| Silent kernel probe, `-smp 4` KVM | 10/40 |
| Silent kernel probe, `-smp 2` KVM | 0/20 |
| Silent kernel probe, `-smp 4` TCG | 0/12 |
| With the D12 wall-clock bound | 0/80 |

## Regression (helper worktree at `c0fe65fce`)

- rv64 CI lane: `PASS: FAT16 mounted — kernel booted (no disk)`.
- rv64 `-smp 2`: same PASS, with `[smp] hart 1 online, parked`.
- aarch64 default lane: `PASS: aarch64 shell prompt reached — full boot successful`.
- aarch64 `QEMU_SMP=2`: same PASS, with `[selftest] SMP-IPI: PASS hart=1 epoch=1`. The `[smp]` lines match 07a.
- `ahci-x86` and `nvme-x86` `*_smp2_x86` with the fault ISO: 2/2 each, with `[x86-smp] topology online=2 processor_ids=0;1 qualified=yes`.
- `ahci-x86` as CI runs it: `ok. 15 passed`. `nvme-x86`: `ok. 11 passed`.
- `virtio-x86`: `ok. 4 passed`, including the KVM `-smp 2` acceptance.
- `power-x86`: `ok. 3 passed`. This suite only boots 1 CPU.

## Not run

- `run-c2c-concurrent-input-smoke.py` at `--cpus 4`. It needs `build/vicell-x86-c2c-lifecycle.iso`, which was not built.
- `local-service-lifecycle-x86`.
- The xhci, igb, iommu-dmar, machine-pin and serial suites.
- Any lane above 8 CPUs, and the `cpu N ignored` path (QEMU ran at most `-smp 8`).
- A refused-AP case on N CPUs.

## R1 flush batching

Branch `feat/x86-s2-flush-batching` from main `a09627f6a` (07b merged, plus the security remediation `7d995b991..6b2191c4c`). Code commit `3bd6d132d`. Owner grant: [s2-phase07b-flush-batching-grant.md](../s2-phase07b-flush-batching-grant.md); Deviation Log D16. No Law-1 delta.
Ceilings unchanged: host tests, QEMU 8.2.2 TCG (`qemu64,+pdpe1gb`), local KVM (`-cpu host`, nested under Hyper-V on WSL2). Not physical evidence. Regenerated `kernel/src/embedded*/` were restored before committing.

### What changed

- `loader/elf.rs` `load_segments`: each segment's leaves are written through `paging::map_page_deferred`, then ONE `paging::flush_deferred_maps(start_page, end_page)` runs before the next segment, under the caller's `FRAME_ALLOCATOR` guard. Before, x86 `map_page` paid one global shootdown per page.
- `loader/wx.rs` `enforce`: one call to `paging::protect_pages` instead of `protect_page` per page.
- `memory/page_protect.rs` (the body of `paging::protect_page`, in set by symbol, Main 2026-11-05):
  - The x86 `protect_page` is split into `rewrite_leaf_x86` plus its flush. Behaviour is unchanged.
  - New `protect_pages`. On x86 it holds `KERNEL_ROOT` from the first leaf write through the last flush. It writes every leaf, then calls `flush_range` once per contiguous run of written pages, and it does so before returning on the error path too. On the other arches it is the old per-page `protect_page` loop.
- `memory/paging.rs`: new `map_page_deferred` and `flush_deferred_maps`. On x86 they are the non-flushing leaf write plus one `flush_range`. On riscv64/aarch64 they are `map_page` (which never flushed a new leaf there) and a no-op. `protect_pages` is re-exported.
- Shootdowns per boot: the canonical boot to `ps` loads 16 Cells (12,953 pages, 80 PT_LOADs). Before, each page cost one global shootdown when mapped and one when lowered, so at least 25,906 shootdowns. After, there is one per PT_LOAD plus one per contiguous W^X run. Every vfs segment is page-contiguous, so its 4,486-page lowering is one flush. These counts are derived from the code and the `ELF LOAD`/`[wx]` lines; nothing counted them at run time.

### Host

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `test result: ok. 464 passed; 0 failed`. New, all `ok`:
  - `memory::page_protect::tests::contiguous_runs_coalesce_only_adjacent_pages`
  - `lowering_writes_every_leaf_then_flushes_each_run_before_return`
  - `failed_lowering_flushes_the_written_prefix_before_returning`
  - `unaligned_addresses_are_written_and_flushed_as_their_page`
- `cargo test -p hal-x86`: `ok. 29 passed`.
- clippy `-p cellos-kernel -- -D warnings`: x86_64-unknown-none, riscv64gc-unknown-none-elf and aarch64-unknown-none-softfloat each `Finished`. `-p hal-x86` `Finished`. `cargo check --features test-hooks` (x86_64) `Finished`.
- `cellos-sign --check --strict`: `OK: F5 — rustc f53b654a8882 is the pinned nightly-2026-05-01`, `OK: F1 — 139 crates and 1009 files scanned; unsafe confined to 77 allowlisted files`.
- Law-1: `OK: the frozen AI interface matches its Law 1 record`, `PASS: LookupServiceBound Law-1 items intact …`, `PASS: multi-source WaitCompletion Law-1 items intact …`.
- `mutable-globals --check`: `636 declarations, 253 need review, 38 problems`, the same line as base `a09627f6a`. No static was added.
- `cargo fmt --all --check`: the four changed files are clean. Diffs remain in files this change does not touch (for example `kernel/src/acpi/power.rs`, `cells/services/supervisor/src/power.rs`).
- Pinned paths are byte-identical to `a09627f6a`: `task/{quiesce,cpu_accounting,scheduler}.rs`, `kernel/src/snapshot.rs`, `cells/services/vfs/**`. Opcode 502 is untouched.

### Boot-to-prompt time (canonical headless ISO as CI builds it, `ps` answered)

Each cell is one run of the 07b runner. The step columns come from serial arrival times:
- **vfs RW**: from `ELF LOAD: 0x100082000…` to `ELF LOAD: 0x100084000-0x101184040 flags=RW-`. This is the 4,353-page segment.
- **vfs W^X**: from the last vfs `ELF LOAD` to `[wx] 'vfs': 131/4486`. This includes relocation.

Host load from other sessions varied between runs: another session's cargo build was running during before A, and the 5-minute load average was 12.6 right after after B.

| | KVM 1 | KVM 2 | KVM 4 | KVM 8 | TCG 1 | TCG 2 | TCG 4 |
|---|---|---|---|---|---|---|---|
| before A (`a09627f6a`) | 2.4 s | 4.4 s | 15.9 s | 5.5 s | 1.8 s | 5.0 s | 13.9 s |
| before A, vfs RW / W^X | 0.02 s / – | 0.32 / 0.26 s | 2.67 / 3.32 s | 0.52 / 0.50 s | – | 0.58 s / – | 0.96 s / – |
| before B | 1.3 s | 1.8 s | 2.6 s | 4.8 s | 1.7 s | 2.4 s | 5.5 s |
| before B, vfs RW / W^X | 0.01 / 0.00 s | 0.13 / 0.08 s | 0.20 / 0.20 s | 0.45 / 0.47 s | 0.07 / 0.04 s | 0.19 / 0.17 s | 0.57 / 0.43 s |
| after A | 1.0 s | 1.2 s | 1.4 s | 2.1 s | 1.7 s | 2.1 s | 3.0 s |
| after B (`3bd6d132d`) | 2.0 s | 2.9 s | 4.7 s | 10.3 s | 3.1 s | 4.5 s | 3.9 s |
| after A and B, vfs RW / W^X | ≤ 0.02 / 0.00 s | ≤ 0.02 / 0.00 s | ≤ 0.02 / 0.00 s | ≤ 0.02 / 0.00 s | ≤ 0.05 / 0.00 s | ≤ 0.05 / 0.00 s | ≤ 0.04 / 0.00 s |

Notes on the table:
- Before A was measured with the 07b `.times` file, and some step lines fell inside a chunk ("–"). Later runs record byte offsets.
- After A is the same code before `rustfmt` of `page_protect.rs`. After B is the committed tree. In the slowest after-B boot (KVM 8) the largest single gap was 1.4 s before `/bin/vfs`, and no ELF or W^X step stood out.

Controlled host contention: one busy loop per host CPU (27), KVM `-smp 4`, runs alternated.

| | runs | prompt | vfs RW | vfs W^X |
|---|---|---|---|---|
| before | 3 | 42.5, 44.7, 33.7 s | 2.53, 4.79, 1.60 s | 5.80, 3.66, 1.44 s |
| after | 6 | 9.9, 10.8, 13.4, 16.0, 14.8, 22.0 s | 0.01–0.13 s | 0.00–0.03 s |

Before the fix, R1's spread (3.6–75.6 s) appears on this host whenever vCPU threads lose the CPU: each of about 26 k shootdowns waits for every vCPU. After the fix, the ELF and W^X steps stay under 0.15 s under the same load. 10 × KVM `-smp 4` on the idle host before the fix: 2.5–2.7 s.

### Guest acceptance (images built as in CI: headless, prod-refusal, domain-test)

| Lane | Result | Decisive lines |
|---|---|---|
| `qemu-x86_64-test.sh` TCG / KVM, `BOOT_WINDOW=90` | PASS / PASS | `PASS: x86_64 shell prompt reached — full boot successful` |
| `x86_64-boot` TCG (`CI=true`, prod-refusal ISO, `CELLOS_X86_SMP=4`) | `test result: ok. 10 passed` | `test x86_smp_brings_every_cpu_online ... ok` |
| `x86_64-boot` KVM (same) | `test result: ok. 10 passed` | same |
| domain lane `X86_CPUS=4` TCG | PASS | `S22-X86-REMOTE-SHOOTDOWN: PASS harts=4 global=true irq_masked_lock=true …`, `S22-X86-MASKED-SHOOTDOWN: PASS harts=4 …`, `broadcast targets=3 ack_ns=211970`, `S22-X86-DOMAIN-LIVE: PASS … hart=2 harts=4` |
| domain lane `X86_CPUS=4` KVM | PASS | `S22-X86-DOMAIN-LIVE: PASS … pcid=1 pcid_usable=true … hart=2 harts=4`, `REMOTE-SHOOTDOWN: PASS harts=4`, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` |

Stress: 10 × [canonical ISO, KVM `-smp 4`, prompt and `ps` within **45 s**] + [domain lane `X86_CPUS=4 X86_ACCEL=kvm`]:
- Boots: **10/10 PASS**. Prompt times: 1.5, 1.8, 1.9, 8.3, 10.5, 6.1, 1.4, 5.9, 4.6, 5.3 s. vfs RW ≤ 0.05 s and W^X 0.00 s in every run. First run: `broadcast targets=3 ack_ns=40436`, `topology online=4 processor_ids=0;1;2;3 qualified=yes`.
- Domain lane: **10/10** `PASS: x86_64 Tier-2 domain-entry lane (x86-domain-test-kvm-cpu4) — every marker present`.
- `STRESS boot pass=10/10 domain pass=10 fail=0 unconfirmed_total=0`: no `invalidation unconfirmed` line in the 20 logs.

### Post-merge regression of main (`a09627f6a` + R1)

Every x86 suite ran with each QEMU boot at 2 CPUs, enforced by a throwaway `PATH` shim:
- The shim adds `-smp 2` when a boot passes none, and rewrites an explicit `-smp 1` to 2.
- The `*smp2*` E5 lanes ran unmodified.
- For KVM, `CELLOS_X86_ACCEL=kvm` was set, and the shim forced `-accel kvm -cpu host` on boots that hard-code TCG.

The shim's argument log shows 2 CPUs on every boot of every suite (for example `ahci` 18 + 6 boots, `nvme` 13 + 6). There were 0 `invalidation unconfirmed` lines in all 24 logs.

| Lane | TCG `-smp 2` | KVM `-smp 2` |
|---|---|---|
| `nvme-x86` | `ok. 9 passed` + E5 `ok. 2 passed` | `ok. 9 passed` + E5 `ok. 2 passed` |
| `ahci-x86` | `ok. 13 passed` + E5 `ok. 2 passed` | `ok. 13 passed` + E5 `ok. 2 passed` |
| `virtio-x86` | `ok. 4 passed` | `ok. 4 passed` |
| `xhci-x86` | `ok. 4 passed` | `ok. 4 passed` |
| `igb-x86` | `ok. 2 passed` | `ok. 2 passed` |
| `nic-x86` | `ok. 2 passed` | `ok. 2 passed` |
| `iommu-dmar-x86` | `ok. 3 passed` | `ok. 3 passed` |
| `serial-x86` | `ok. 1 passed` | `ok. 1 passed` |
| `machine-pin-x86` | `ok. 5 passed` | `ok. 5 passed` |
| `power-x86` | `ok. 3 passed` | `ok. 3 passed` |

E5 decisive lines: `09C E5 [Nvme smp=2] … topology online=2 processor_ids=0;1 qualified=yes` and `09C E5 [Ahci smp=2] …` (same). A first pass also passed every suite with the same test counts (`nvme` 11, `ahci` 15, …), under both `CELLOS_X86_ACCEL` settings. That pass kept the harness's own CPU counts and hard-coded accelerators, with no rewrite and no forced KVM.

| Lane (other) | Result | Decisive lines |
|---|---|---|
| `qemu-mlibc-witness.sh`, KVM `-cpu host` | PASS | `MLIBC-WITNESS: PASS (posix runs: 1 …)`, `MLIBC-SMOKE: 3/3 pass`, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` |
| `qemu-cpp-hosted-smoke.sh`, KVM `-cpu host` | PASS | `CPP-HOSTED-SMOKE: PASS (runs: 1 …)`, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` |
| `drm-lite-x86/run-qemu-x86.sh`, KVM `-cpu host` | PASS | `DRM-LITE: 12/12 pass`, `DRM-LITE-WITNESS: PASS (kvm; …)`, `X86-VMM-SMOKE: PASS (api-path PortOut+Hlt)` |
| c2c-render smoke `--cpus 4` TCG / KVM | PASS / PASS | `C2C_RENDER_SMOKE=PASS (native baseline/copy/shared equality, actual qualified online CPUs=4 processor_ids=0;1;2;3, …)`, `c2c-render: COMPARE OK baseline=copy=shared` |
| rv64 CI lane / `-smp 2` | PASS / PASS | `PASS: FAT16 mounted — kernel booted (no disk)`; at 2 harts `[smp] hart 1 online, parked` |
| aarch64 default lane / `QEMU_SMP=2` | PASS / PASS | `PASS: aarch64 shell prompt reached — full boot successful`; at 2 PEs `[smp] hart 1 online, parked`, `[selftest] SMP-IPI: PASS hart=1 epoch=1` |

Notes on the other lanes:
- **Witness inputs.** The worktree's `libc.a` was rebuilt with `scripts/build-mlibc.sh` (pin `ae1092eb2`). The cpp-hosted witness linked the gitignored `target/llvm-runtimes/x86_64` copied from the main checkout, because `build-llvm-runtimes.sh` was not run.
- **Observed, not caused by R1.** The TCG (SVM) domain lane logs `X86-VMM-SMOKE: FAIL got_port=false got_hlt=false last_exit=Shutdown`. The lane does not assert this marker. Base `a09627f6a`, built and run the same way without R1, gives the same line. Under 07b it read `last_exit=Preempted`, so the exit reason changed with the security remediation. KVM (VMX) passes.

### Independent race review

Independent race review: PASS (4 Minor, fixed in `ea6fdc533`). The fixes are documentation and visibility only:
- the W^X module doc describes the x86 acknowledged shootdown;
- the `page_protect` header and its re-export list include `protect_pages`;
- `map_page_deferred`/`flush_deferred_maps` are `pub(crate)`, and `map_page_deferred` documents its precondition (a new leaf or one that only gains rights; no frame freed before the batch flush);
- the coalescer test's run ends below the top page.

Re-run on `ea6fdc533`:
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `test result: ok. 464 passed; 0 failed`, with the four `page_protect::tests` `ok`.
- clippy `-p cellos-kernel -- -D warnings`: x86_64, riscv64 and aarch64 each `Finished`.

### Not run

- Guest lanes on `ea6fdc533`. That commit changes only comments, visibility and a host test; the code paths the guest lanes ran are those of `3bd6d132d`.
- Physical hardware; lanes above 8 CPUs.
- The riscv64/aarch64 shootdown fixtures at 2 harts. Their ELF/W^X paths are unchanged by construction (`map_page_deferred` is `map_page`, and `protect_pages` is the old loop).
