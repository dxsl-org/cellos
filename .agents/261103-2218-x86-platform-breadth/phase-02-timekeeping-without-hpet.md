---
phase: 2
title: "Timekeeping without HPET (TSC, kvmclock, PM timer, TSC-deadline)"
status: completed
priority: P1
effort: "3d"
dependencies: [1]
tier: thinking
checkpoint: false
---

# Phase 02: Timekeeping without HPET

> **Required — deviation-log:** Log every Decision / Deviation / Surprise in § Deviation Log the moment it occurs — not at report time. On an edge case that diverges from this plan, choose the smallest reversible option, log four lines, and continue. Escalate only irreversible or contract-breaking divergence.

## Context Links

- Gate: `kernel/src/main.rs:512,596-656`
- Calibration: `hal/arch/x86/src/x86_64/hpet.rs:150-224`
- Clock consumers: `kernel/src/task/syscall.rs:8120,8143`; `task/telemetry.rs:41`; `hal/arch/x86/src/x86_64/svm_vcpu.rs:126`; `hal/arch/x86/src/x86_64.rs:346`. Not migrated: `task/cpu_accounting.rs` (the module is `#[cfg(cellos_g1_cpu)]`, `kernel/src/task.rs:104`) and `main.rs:604-649`. Both belong to the G1 private lane, which keeps its HPET contract.
- Intel SDM Vol 3 §18.7 (TSC), §11.5.4.1 (TSC-deadline); KVM `Documentation/virt/kvm/x86/msr.rst` (kvmclock)

## Overview

HPET stops being mandatory. A clocksource is chosen from what the machine offers, and the LAPIC timer uses TSC-deadline when it can. This unblocks default libvirt guests, Hyper-V-style VMs and Intel firmware that hides HPET.

## Key Insights

- HPET is both the calibration reference and the monotonic clock. The two roles must be split.
- KVM guests usually lack the invariant-TSC bit unless `+invtsc` is set (libvirt host-model does not set it by default [assumption]). kvmclock (pvclock) is the standard guest clock and is cheap to read.
- VMware publishes the TSC frequency in CPUID 0x40000010. Linux treats that TSC as reliable.
- The G1 private lane asserts HPET (`main.rs:604-649`). That contract stays as it is.

## Requirements

- Functional:
  - `hal::clock` API: `now_ns() -> u64` (monotonic, cross-CPU consistent), `source() -> ClockSource`, `checked_sample()` with the same error contract as `hpet::checked_sample`.
  - Selection order:
    1. invariant TSC with known frequency: CPUID 0x15 crystal×ratio, or 0x40000010 under a hypervisor, else calibrated;
    2. kvmclock (`KVM_FEATURE_CLOCKSOURCE2`, MSR 0x4b564d01) under KVM;
    3. HPET;
    4. ACPI PM timer (FADT), 24/32-bit wrap handled.
  - When none of these exist, the gate stays closed with a named log line.
  - Clockevent: LAPIC TSC-deadline mode when CPUID.1:ECX[24] is set and the clocksource is TSC. Otherwise LAPIC one-shot, calibrated against the chosen clocksource (fallback PIT). Calibration result logged.
  - `x86_timer_ready = lapic && ioapic && clock_ready`.
  - All listed consumers call `hal::clock`. `hpet::*` stays only as a backend.
  - TSC sync check: each AP compares its TSC with the BSP's at bring-up. On a skew above 1 µs, TSC is demoted to the next source and the reason is logged.
  - Source switching is monotonic: when a source is demoted at runtime, the new source is offset so that `now_ns()` continues from the last value returned (never goes backwards, never jumps forward by more than the switch latency). Unit test: a forced demotion mid-sequence produces a non-decreasing series.
  - The G1 private lane (`cellos_g1_cpu`: `cpu_accounting.rs`, `main.rs:604-649`) keeps using `hal::hpet` directly and keeps requiring HPET. Its in-memory capture (`G1-*` log lines, `cpu_accounting.rs:1410-1445`) is unchanged.
- Non-functional: `now_ns` is lock-free; TSC/kvmclock reads take under 100 ns; the PM timer path stays correct but slow (port I/O).

## Architecture

`hal/arch/x86/src/x86_64/clock/{mod.rs, tsc.rs, kvmclock.rs, pmtimer.rs}` plus the existing `hpet.rs`. One `static SOURCE: AtomicU8` + per-source state written once before APs start. kvmclock uses one per-CPU `pvclock_vcpu_time_info` page, registered by each CPU at bring-up (phase 07 extends this to N CPUs).

## Assumptions

- **Claim:** QEMU 8.2 exposes CPUID 0x40000010 with `-cpu host,+invtsc` and does not expose it by default. **Confidence:** low. **How to verify:** log the leaf in a KVM boot both ways.
- **Corrected 2026-11-04 (cook, phase 00 recon):** the validation finding "cpu_accounting binds to HPET" is true but scoped to the G1 private lane. `mod cpu_accounting` is `#[cfg(cellos_g1_cpu)]` (`kernel/src/task.rs:104`), the lane already asserts HPET (`main.rs:604-649`), and the journal is an in-memory capture printed as `G1-*` log lines, not persisted. No journal format change is needed.
- **Verified (2026-11-03):** QEMU 8.2.2 `-machine q35,help` lists `hpet=<bool>`.

## Related Code Files

- Create: `hal/arch/x86/src/x86_64/clock/{mod,tsc,kvmclock,pmtimer}.rs`
- Modify: `hal/arch/x86/src/x86_64/hpet.rs` (calibration split), `apic.rs` (TSC-deadline mode), `hal/arch/x86/src/x86_64.rs:346`, `kernel/src/main.rs:512,596-656` (non-G1 branch only), `kernel/src/cpu_features.rs`, the consumer files above, `tests/integration/src/lib.rs` (constructor `boot_x86_bios_no_hpet(iso, accel)`), `scripts/qemu-x86_64-test.sh` (`X86_HPET=off`), `docs/hardware-compatibility-list.md` R2

## Implementation Steps

1. Add the CPUID probes to `cpu_features.rs`: invariant TSC, TSC-deadline, hypervisor vendor string, max hypervisor leaf, 0x15/0x16, KVM features leaf 0x40000001.
2. Add the PM timer backend (FADT GAS; IO or MMIO).
3. Add the TSC backend. Frequency order: 0x15 (crystal non-zero) → 0x40000010 → calibrate 50 ms against HPET or PM timer, whichever is present.
4. Add the kvmclock backend (pvclock version/seqlock read loop, `PVCLOCK_TSC_STABLE_BIT`).
5. Add `clock::select()`, called from `main.rs` after ACPI. Change the gate. Switch the consumers.
6. Make TSC-deadline the clockevent when the source is TSC. Keep LAPIC periodic as the fallback.
7. Add the AP TSC sync check. In this phase only 1 AP exists; phase 07b generalises it.
8. Lanes: add `x86_64-boot` variants for `{tcg, kvm} × {hpet on, hpet off}`. Update HCL R2 wording to "reliable clocksource (invariant TSC, kvmclock, HPET or ACPI PM timer)".

## Todo List

- [x] CPUID probes
- [x] PM timer, TSC, kvmclock backends
- [x] Selection + gate + consumer cutover
- [x] TSC-deadline clockevent
- [x] Lanes + HCL R2

## Success Criteria

- [x] KVM `-machine q35,hpet=off -cpu host` boots to `Cellos >`; log shows `clock=kvmclock clockevent=TscDeadline`; UART RX IRQ opened; `serial-x86` passes in that lane. With `+invtsc`: `clock=tsc`.
- [x] TCG `hpet=off` boots with `clock=pmtimer` (also a CI step).
- [x] `serial-x86` passes in all four lane variants (`CELLOS_X86_ACCEL`, `CELLOS_X86_HPET`).
  - `local-service-lifecycle-x86` passes in all four variants, except the intermittent `x86_multi_source_wait_holds_on_two_cpus` CANCEL-RACE failure, which is **pre-existing**: baseline HEAD fails 1/8, this tree 1/6 with HPET and 1/4 with the PM timer.
  - `x86_64-boot`: 7/9 in every variant. The 2 failures come from the locally built prod-refusal witness image (Cellbox auth `NotFound`) and are independent of the clocksource.
  - The G1 private build cannot be compiled locally (private `p01_manifest.bin`, pre-existing `scheduler.rs` cfg errors). Its code path is unchanged and keeps the HPET assertion. See [evidence](reports/phase-02-evidence.md).
- [x] Unit tests: PM timer 24/32-bit wrap and racing stale read; TSC frequency from 0x15 (crystal 0 → `None`); pvclock seqlock retry on odd/changed versions; pvclock scaling; TSC tick conversion (`hal-x86` 20/20).

## Risk Assessment

- The G1 private lane must stay on HPET. Mitigation: `hal::clock` selection is not used under `cellos_g1_cpu`, and the success criterion covers the G1 compile.
- Migrating a KVM guest changes the TSC frequency. Out of scope; prefer kvmclock under KVM when invariant TSC is not advertised.

## Security Considerations

The hypervisor supplies the guest clock and is trusted, consistent with the threat model for guests. No new cell-facing surface.

## Next Steps

Phase 05 relies on this for no-HPET KVM acceptance; phase 07b extends the kvmclock pages and sync check to N CPUs.

## Deviation Log

- Surprise: the validation finding behind "journal v2" was scoped wrongly. `cpu_accounting` is G1-private (`#[cfg(cellos_g1_cpu)]`), not persisted, and its lane already requires HPET.
  Why: phase 00 recon on 2026-11-04 read `kernel/src/task.rs:104` and `cpu_accounting.rs:1-20,1410-1445`.
  Impact: the journal v2 requirement, durable-charge review and "irreversible effect" are removed (phase 00, plan.md). The owner is asked to confirm at the phase 00 sign-off.
  Revert: restore the journal v2 requirement if the owner wants G1 to run without HPET (new scope).
- Surprise: the first PM-timer wrap extension took a racing CPU's stale reading for a wrap, so time jumped forward ≈4.7 s.
  Why: the TCG `hpet=off` two-CPU lane exposed it through timeouts. Fixed with half-wrap staleness detection (`clock/pmtimer.rs::extend`), plus a unit test.
  Impact: none outside the PM timer.
  Revert: n/a.
- Deviation: the CPUID probes live in `hal/arch/x86/src/x86_64/clock/{tsc,kvmclock}.rs`, not `kernel/src/cpu_features.rs`.
  Why: the HAL selects the clock and cannot depend on the kernel crate.
  Impact: none.
  Revert: n/a.
- Deviation: the PM timer is used only in port I/O space. A system-memory PM timer is logged and skipped.
  Why: those addresses are not identity-mapped by `init_kernel_paging_x86`, and the platforms in scope use port I/O.
  Impact: a hardware-reduced or MMIO-PM-timer machine needs TSC or HPET.
  Revert: map the GAS address in `init_kernel_paging_x86` and accept space 0.
- Decision: kvmclock registers only the BSP's time-info structure and is used only when the host declares `PVCLOCK_TSC_STABLE_BIT`; otherwise selection falls through to HPET or the PM timer.
  Why: no per-AP pvclock area or current-CPU lookup is needed (same rule as the Linux vDSO).
  Impact: phase 07b does not need per-AP kvmclock pages.
  Revert: n/a.
- Decision: the AP TSC check is one-sided (an AP behind the published clock demotes TSC). A global `fetch_max` keeps `now_ns` non-decreasing across CPUs and source changes.
  Why: avoids a BSP/AP handshake while `start_secondaries` waits.
  Impact: an AP ahead of the BSP is absorbed by the guard, not detected.
  Revert: add a handshake in phase 07b.
- Decision: TSC-deadline is used whenever the CPU supports it and the TSC rate is known, even when kvmclock is the clocksource (the rate is measured against kvmclock).
  Why: KVM `-cpu host` advertises TSC-deadline but not invariant TSC.
  Impact: the KVM lanes show `clock=kvmclock clockevent=TscDeadline`.
  Revert: gate deadline mode on `source == Tsc`.
- Deviation: the lane variants are environment-selected in the existing harness constructors (`x86_lane()`, `CELLOS_X86_ACCEL`, `CELLOS_X86_HPET`), not a new `boot_x86_bios_no_hpet` constructor.
  Why: one switch covers every suite that uses the plain constructors.
  Impact: `tests/integration/src/lib.rs`.
  Revert: n/a.
