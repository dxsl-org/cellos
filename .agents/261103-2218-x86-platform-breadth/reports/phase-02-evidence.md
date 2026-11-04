# Phase 02 evidence — timekeeping without HPET (2026-11-04)

Base HEAD `64ad34027`; working tree uncommitted. Evidence ceiling: host + QEMU 8.2.2 TCG + QEMU/KVM (local `/dev/kvm`, Intel host TSC ≈ 2.112 GHz).

## Boot lanes (`scripts/qemu-x86_64-test.sh`; `X86_HPET` and `X86_EXPECT_CLOCK`/`X86_EXPECT_TICK` are new)

| Lane | Env | Result | Kernel line |
|---|---|---|---|
| TCG, HPET on | — | PASS | `[x86-gate] timer: clock=hpet clockevent=LapicPeriodic { ticks_per_ms: 62680 }` |
| TCG, HPET off | `X86_HPET=off X86_EXPECT_CLOCK=pmtimer` | PASS | `clock=pmtimer clockevent=LapicPeriodic { ticks_per_ms: 62815 }` |
| KVM `-cpu host`, HPET on | `X86_ACCEL=kvm X86_CPU_MODEL=host` | PASS | `clock=kvmclock clockevent=TscDeadline { period_ticks: 21120000 }` |
| KVM `-cpu host`, HPET off | `+ X86_HPET=off` | PASS | `clock=kvmclock clockevent=TscDeadline { period_ticks: 21120001 }` |
| KVM `-cpu host,+invtsc` | `X86_EXPECT_CLOCK=tsc X86_EXPECT_TICK=TscDeadline` | PASS | `clock=tsc clockevent=TscDeadline { period_ticks: 21120000 }` |
| KVM `-smp 2`, HPET on and off (manual QEMU) | — | `Cellos >` | `[x86-smp] topology online=2 processor_ids=0;1 qualified=yes` (AP tick armed in deadline mode) |

Every lane printed `[INFO] x86_64: ramdisk + UART RX IRQ initialised`, so the UART RX IRQ gate opened with and without HPET.

## Integration suites (`tests/integration`, default TCG runner)

- `serial-x86`: 1/1 (multi-port TX/RX, console unchanged).
- `local-service-lifecycle-x86`: 3/3, including `x86_multi_source_wait_holds_on_two_cpus`. Needs `scripts/build-x86_64-c2c-lifecycle-ci.sh` first.
- `x86_64-boot`: 7/9.
  - The two failures (`x86_tier2_admission_is_refused_until_switch_is_qualified`, `x86_tier2_fault_isolation_fixture_is_refused`) boot `build/vicell-x86-prod-refusal.iso`, built locally by `scripts/build-x86_64-prod-refusal-ci.sh` (not a CI lane).
  - That image starts its timer correctly (`clock=hpet clockevent=LapicPeriodic`), but the shell exits with `shell: authenticated serial session failed: NotFound`, so `Cellos >` never appears.
  - **[INFERENCE]** This is image-recipe drift against the current Cellbox authentication, not a phase 02 regression. Nothing in that path reads the clock, and the timer line is identical to the passing lanes. The baseline was not re-run with this witness image.

## Suite matrix across lane variants (`CELLOS_X86_ACCEL` × `CELLOS_X86_HPET`, harness `x86_lane()`)

| Variant | x86_64-boot | serial-x86 | local-service-lifecycle-x86 |
|---|---|---|---|
| tcg / hpet on | 7/9 (prod-refusal witness ×2) | 1/1 | 3/3 |
| tcg / hpet off | 7/9 (same 2) | 1/1 | 2/3 first run (two-CPU CANCEL-RACE), then 3/3 ×3 and 2/3 ×1 after the PM-timer fix |
| kvm / hpet on | 7/9 (same 2) | 1/1 | 3/3 |
| kvm / hpet off | 7/9 (same 2) | 1/1 | 3/3 |

**Attributing the two-CPU flake** (`x86_multi_source_wait_holds_on_two_cpus`):

- Clean worktree of HEAD `64ad34027` with its own lifecycle image: **1/8** failures, `FAIL — dispatched cancellation reported a delivered request as not executed`.
- This tree with HPET: 1/6. With the PM timer, after the fix: 1/4.

The failure exists at baseline, so it is not a phase 02 regression. It belongs to the C2C cancellation path.

**PM-timer bug found by this lane and fixed.** The first `pmtimer::extend` treated a racing CPU's stale raw read as a wrap, so time jumped forward ≈4.7 s. It now detects staleness at half a wrap, with a unit test. Before the fix, the TCG hpet-off two-CPU lane failed 2/3; after it, 1/4, which matches the baseline rate.

## Host

- `cargo test -p hal-x86 --target x86_64-unknown-linux-gnu`: 20 passed. New tests:
  - PM timer 24/32-bit wrap extension, and a racing stale read;
  - CPUID 0x15 crystal frequency, including the crystal-0 refusal;
  - TSC tick→ns conversion;
  - pvclock scaling with positive and negative shift;
  - pvclock seqlock retry on odd or changed versions.
- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 408 passed in repeated runs. `task::scheduler::retirement_tests::{blocked_task_requeued_from_boot_is_selectable, faulted_task_to_boot_allows_successor_selection}` fail intermittently.
  - **Pre-existing:** on a clean worktree of HEAD `64ad34027` the same test failed 1 of 25 runs. The assertion reads shared hart-local state (`current_cell_id() == 0`, saw 61).
- `cargo fmt -p cellos-kernel -p hal-x86 -- --check`: clean.
- `cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc --features policy-required`: no warnings.

## G1 private lane

- Not buildable locally: `kernel/build.rs:212` needs the private `p01_manifest.bin`.
- A type-check with `--cfg cellos_g1_cpu --cfg cellos_p01_trace` reports only:
  - the missing manifest;
  - two pre-existing `E0614` errors in `kernel/src/task/scheduler.rs:2131,2611` (file untouched by this phase).
- The G1 code path keeps `init_timers_observed` (HPET-only, periodic) and its HPET requirement. Not exercised.
