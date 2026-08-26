# EL2 fault root cause: IRQ-from-lower-EL path is not guest-aware

**Date:** 2026-07-29 · **Lane:** `QEMU Hypervisor Machinery Smoke (TCG)` · **Status:** root cause proven
from source + captured log; fix implemented in PR #16, awaiting runtime evidence from CI

## Verdict

`vt_irq_el2_lower` (`hal/arch/arm/src/aarch64/el2.rs:337`) never checks `TPIDR_EL2`. A physical
timer IRQ taken while the guest vCPU is executing therefore runs the kernel scheduler with
**guest EL1 translation state and `HCR_EL2.VM=1` still live**, so the next Cell scheduled at EL0
translates through the guest's page tables. That is the `Cell N terminated: scause=0x82000006`
signature the lane has been failing on — not a VFS defect, and not TCG noise.

## Chain of custody

1. `hal/arch/arm/src/aarch64/timer.rs:86` — at EL2 the kernel enables GIC PPI **26**
   (`CNTHP`, hypervisor physical timer).
2. `hal/arch/arm/src/aarch64/trap.rs:268-280` — `vi_aarch64_irq_handler` matches
   `irq == 26` when `el2::is_el2()`, EOIs, then calls `vi_timer_tick()`.
3. `kernel/src/task.rs:244` — `vi_timer_tick()` ends in `yield_cpu()`, i.e. **a context switch**.
4. `hal/arch/arm/src/aarch64/el2.rs:336-337` — `vt_irq_el2_cur` and `vt_irq_el2_lower` are the
   *same label*. The lower-EL IRQ vector pushes a frame and calls the handler directly.
   `TPIDR_EL2` is read in exactly one place in the whole tree (`el2.rs:328`, inside
   `vt_sync_el2_lower`) — the IRQ vector has no equivalent guard.
5. `HCR_EL2.IMO=1` is set for guest entry (`vcpu.rs:447`). An IRQ routed to EL2 is not maskable
   by the lower EL's `PSTATE.I`, so `SPSR_EL2=0x3C5` (guest DAIF all-masked) does **not** prevent
   the tick from firing mid-guest.
6. Consequently the scheduler runs a Cell at EL0 while:
   - `HCR_EL2.VM = 1` — stage-2 active against the guest's `VTTBR_EL2`. `vt_vcpu_trap` documents
     this as load-bearing (`vcpu.rs:527`: *"CRITICAL: must clear VM before Cell EL0 accesses run
     through Stage-2"*); the IRQ path never clears it.
   - EL1 sysregs still hold the **guest's** values — `TTBR0_EL1`, `TCR_EL1`, `SCTLR_EL1`,
     `VBAR_EL1`. The host bank is only restored at `run_vcpu_impl` step 4 (`vcpu.rs:326`), which
     this path never reaches.

   Either condition alone makes every Cell EL0 instruction fetch untranslatable →
   EC `0x20` / ISS `0x6` (instruction abort, translation fault level 2) = `scause=0x82000006`.

## Log evidence

From `.agents/hv-logs/attempt-2/qemu-hv.log:93-102` (identical in CI run `30415738344`):

```
[hv] vCPU ready — entering run loop
[ WARN] [gpu_cursor] GPU Driver Cell not registered          ← Cells running *while* the vCPU is live
USER: [compositor] hardware cursor unavailable — using software cursor
[ WARN] [hv] unhandled guest trap ec=0x20 iss=0x5 | guest ELR_EL1=0x413ff100 ESR_EL1=0x0 FAR_EL1=0x0 VBAR_EL1=0x0
[ WARN] [hv]   guest TTBR0_EL1=0x41510000 TTBR1_EL1=0x41402000
[ERROR] [fault] Cell 2 terminated: scause=0x82000006
[panic-in-cell 9] trap ec=0x22 esr=0x8A000000 elr=0x4153C0E9 far=0x4153C0E9
```

Two independent confirmations in that excerpt:

- **Cells printed output after `entering run loop`.** Cell scheduling during a live vCPU is only
  reachable via the IRQ vector — the sync vector diverts to `vt_vcpu_trap`. This is the direct
  observation of the bug, not an inference.
- **Guest `ESR_EL1=FAR_EL1=VBAR_EL1=0x0` alongside a non-zero `SCTLR_EL1`/`TTBR0_EL1`** — a guest
  that faulted would have a syndrome. Zeroed vector base with live translation registers is the
  half-swapped state this path leaves behind.

## Why it presented as flaky

The failure needs a tick to land inside the guest's run window *and* the scheduler to pick a Cell
that faults before the guest resumes. Same source tree, different interleaving → the 3-of-6
pattern recorded in `docs/TODO.md`. It is exclusive to the hypervisor lane because no other lane
runs a vCPU.

## The secondary EC 0x22 fault

`elr=far=0x4153C0E9` (misaligned, 1 mod 4) sits in **guest RAM** — same `0x41xx_xxxx` band as
guest `TTBR0_EL1=0x41510000` and guest `ELR_EL1=0x413ff100` — not kernel text. PR #15 proved via
run `30418266841` that `SPSR.M != 0`, i.e. EL2 origin. Consistent with a control transfer taken
during the half-swapped window (guest `VBAR_EL1` is live, and `vi_terminate_on_fault` force-unlocks
kernel locks before yielding, so a cascading fault can resume a context whose saved `elr_el2`/`x30`
came from that excursion). **Reported as a consequence, not independently proven** — expect it to
disappear once the primary defect is closed; if it survives, it is a separate bug.

## Fix shape

`ViVmExit::Preempted` already exists end-to-end and means exactly this
(`libs/api/src/abi/hypervisor.rs:55`, `hal/traits/hypervisor/src/lib.rs:56`,
`kernel/src/hypervisor/registry.rs:413`), and the hv cell already re-enters transparently on it
(`cells/services/hypervisor/src/run_loop.rs:153`). So:

1. In `vt_irq_el2_lower`, mirror `vt_sync_el2_lower`: save `x0`/`x1` scratch, read `TPIDR_EL2`,
   and `cbnz` → `vt_vcpu_trap`. That trampoline already saves guest GP + exit regs, clears
   `HCR_EL2.VM`, clears `TPIDR_EL2`, and returns to `run_vcpu_impl`, which restores the host EL1
   bank. Requires splitting the currently-aliased `vt_irq_el2_cur`/`vt_irq_el2_lower` labels.
2. Mark the exit as `Preempted` rather than decoding stale `ESR_EL2`. `vt_vcpu_trap` saves
   `ESR_EL2` from an IRQ entry, which carries no valid syndrome — `decode_vmexit` would return
   `Unknown{ec,iss}` and the hv cell would halt the VM. Needs a distinct signal (e.g. a flag field
   the trampoline sets, checked in `decode_exit` before `decode_vmexit`).
3. Guest PC must not advance: `run_vcpu_impl` step 6's `_` arm already sets
   `g_elr_el2 = exit_elr` — correct as-is.
4. The physical IRQ is left unclaimed at the GIC, so it re-fires immediately in host context and
   is serviced normally by `vt_irq_el2_cur`. No tick is lost.

Rejected: masking IRQs across the guest run window (`IMO=0`, or DAIF at EL2) — makes a runaway
guest unpreemptible and hangs the OS.

## Implemented — PR #16, commit `60306457`

`hal/arch/arm/src/aarch64/el2.rs` + `hal/arch/arm/src/aarch64/vcpu.rs`, +95/−9.

- Split the aliased `vt_irq_el2_cur`/`vt_irq_el2_lower` labels; the lower-EL one now makes the same
  `TPIDR_EL2` check as the sync vector and branches to `vt_vcpu_trap`.
- New `AArch64Vcpu::exit_is_irq` at offset 520, written by whichever lower-EL vector ran (1 from
  IRQ, 0 from sync). `decode_exit` consults it before `decode_vmexit` and reports `Preempted`.
  Needed because an IRQ entry does not write `ESR_EL2`, so decoding the stale value yields
  `Unknown{ec,iss}` → the hv cell halts the VM.
- `Preempted` was already plumbed end-to-end but never produced on aarch64 (`_budget_ns` in
  `registry.rs:273` is ignored). The hv cell's arm re-enters without `advance_pc` — correct, the
  guest must resume at the interrupted instruction, and `run_vcpu_impl` step 6's `_` arm already
  sets `g_elr_el2 = exit_elr`.
- Lower-EL FIQ/SError still route to `vt_sync_el2_lower` and so inherit the check; FIQ would decode
  a stale ESR, but GICv2 here delivers every source (timer PPIs included) on the IRQ line, so that
  path was left alone rather than changed without a way to exercise it. Documented in-place.

**Static verification:** `cargo check`/`clippy -D warnings`/`fmt --check` clean on `hal-arm` +
`vicell-kernel` for aarch64, `clippy` clean on `vicell-kernel` for x86_64, 24 host tests pass in
`types` + `api`. Disassembly of the release kernel confirms `__vectors_el2` slot 9 → the new
`vt_irq_el2_lower`, slots 1/5 → the now-distinct `vt_irq_el2_cur`, both lower-EL trampolines
storing to `[x0, #0x208]` (= 520) before branching to `vt_vcpu_trap`, and each no-guest path
restoring its scratch slots before falling back to the host handler.

**Not verified at runtime locally.** The smoke test needs clang, an aarch64 sysroot and the Alpine
artifacts; this machine has none and cannot `sudo apt-get install`. The local cells build fails at
link. CI was the first runtime exercise.

**Runtime verification — 4/4 green.** Run `30431236958`, the `QEMU Hypervisor Machinery Smoke (TCG)`
lane, one original execution plus three reruns, every one `success` and every one reporting
`PASS: machinery ran — VMM entered the guest; only the documented TCG address-size fault occurred`.
No `[fault] Cell` and no `panic-in-cell` line appears in any of them. That is conclusive on its own:
the script's *first* gate, checked before any mode-specific logic, is
`grep -qia "KERNEL PANIC\|\[fault\] Cell"` → exit 1, so a zero exit proves the fault is absent
rather than merely tolerated.

Against the recorded ~3-in-6 historical failure rate, four consecutive passes would happen by
chance about 6% of the time — suggestive but not decisive alone. It is the combination that settles
it: a mechanism traced end-to-end in source, disassembly confirming the emitted vector wiring, the
fault signature disappearing, and the lane converging on the previously-documented TCG behavior.

All 19 checks on PR #16 pass (the KVM boot-to-shell lane skips, as it does on every PR — no
hardware EL2 on the runner). `RedoxFS /srv Integration Test` failed once on the first execution
with `The action 'Install dependencies' has timed out after 10 minutes` — an apt-mirror timeout the
workflow already anticipates in a comment, on a lane that builds for
`x86_64-unknown-linux-gnu` + riscv and compiles none of the changed aarch64 code. It passed on
rerun in 3m1s.

## Follow-ups

- ~~`scripts/qemu-hypervisor-smoke.sh` tolerance clause needs revisiting~~ — **withdrawn.** The
  first green run on PR #16 (`30431236958`, job `90509215884`) reports
  `PASS: machinery ran — VMM entered the guest; only the documented TCG address-size fault
  occurred`, i.e. it matched `unknown vmexit ec=0x20 iss=0x6 pc=0x200` **and**
  `guest_fault_is_address_size()`. The clause was correct all along. The `iss=0x5` +
  `ESR_EL1=0x0` pair I read off the failing log was itself a symptom of the half-swapped
  translation state, not a signature drift — with the guest exited cleanly the lane reaches the
  genuine documented TCG behavior. No change needed here.
- `docs/TODO.md:3-14` attributes the lane to VFS flakiness and concludes main's red badge does not
  reflect code health. Both are wrong; rewrite once the repeat runs confirm the fix.
