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
- `denials.rs` (grant 1, D1): the `1 - hart` victim index is removed. `c2c-render` (grant 2, D3): the image is saved through the cwd grant, and `ProcessorIds` holds 64 entries.
- Lanes:
  - `qemu-domain-test.sh` accepts `X86_CPUS` 1..64. With 2 or more CPUs it requires `topology online=N`, `broadcast targets=N-1` and `harts=N` on both shootdown markers, and it fails on any `invalidation unconfirmed` line.
  - `x86_64-boot` adds `x86_smp_brings_every_cpu_online` (`CELLOS_X86_SMP`, default 4), and CI job `qemu-x86_64-boot` adds a step that runs it at `-smp 4`.
  - Both c2c smokes default to `--cpus 4` and check `online=N`.
- Records: `docs/specs/04-hardware.md` §5, `scripts/mutable-globals.toml` with the regenerated md (D4), and this phase's file.

## Host

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

## Stress (10 × [canonical TCG `-smp 4` boot + domain lane `X86_CPUS=4` KVM])

- Canonical boots: 10/10 PASS. Prompt times: 4.4, 5.0, 14.4, 7.2, 24.8, 21.8, 23.7, 21.1, 23.7, 19.9 s.
- `invalidation unconfirmed` lines: **0** in all 20 logs.
- Domain lane: **7/10 PASS, 3/10 FAIL** (runs 1, 3, 10). Each failure is the same fixture line, with every kernel marker present and no panic:

  ```
  USER: [tier2-rpc] provider-exiting
  USER: [tier2-rpc] FAIL — the fixture's registry entry stayed live after the provider exited
  [ INFO] Syscall::Exit: task 10 exited with code 0
  [ INFO] [service-registry] cleared stale entries for dead tid 10
  ```

  `tier2-rpc-driver` (`cells/tests/tier2-rpc-driver/src/main.rs:173-183`, outside the set) polls the registry `STALE_POLLS = 2000` times with `yield_now()`. That is a yield count, not a time bound, and it ran out before the provider's exit syscall. Once the exit landed, the registry was cleared. [INFERENCE] With four harts the driver can sit alone on an idle hart, where each yield returns at once, while the provider's Tier-2 exit runs elsewhere. The stress acceptance (10/10) is **not met** by the domain lane. See open risk R2.
- Control at two CPUs on the same `c0fe65fce` domain-test kernel: `X86_CPUS=2 X86_ACCEL=kvm`, 4/4 `PASS … (x86-domain-test-kvm-cpu2) — every marker present`, 0 `[tier2-rpc] FAIL`. So the fixture race shows up only at four CPUs. It is not proven to be fixture-only: the kernel exit path at N harts was not traced.

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
