---
phase: 3
title: "VM entry fast path"
status: completed
priority: P1
effort: "3-5d"
dependencies: [2]
tier: thinking
checkpoint: true
---

# Phase 3: VM entry fast path

> **Required — deviation-log:** log every Decision / Deviation / Surprise in § Deviation Log when it happens.

## Overview

Measured with temporary counters (scout-report §1): from the click to Chromium's UI the Cell
made 60,000 `RunVcpu` calls in 25.0 s. 12.6 s of that was the guest; 4.6 s was VMX bookkeeping
around it (`VMPTRLD`, 17 host-state `VMWRITE`s, `VMREAD`s, `VMCLEAR`: 76 µs per call here,
where each VMX instruction traps); 7.8 s was outside the run scope (`INVEPT`, syscalls, the
Cell's device work). More than half of the calls were the guest's serial console. This phase
makes a call cheaper and makes fewer of them; its first step is already in the tree.

## Requirements

- Functional (done 2026-11-25, see Deviation Log): the Chrome guest's kernel logs at
  `loglevel=4` and seatd logs errors only.
- Functional (done): `INVEPT` runs only when this CPU last invalidated at another epoch of the
  nested page table, or the vCPU last ran on another CPU. An epoch is drawn fresh at table
  creation and at every leaf change and is never drawn twice, so a reused root frame cannot
  match a CPU's record.
- Functional (done, added): a VPID per vCPU where the CPU offers one.
- Functional (done): host-state fields are rewritten only when CR0, CR3, CR4, FS.base or
  GS.base differ from what the VMCS last got.
- Functional (done, moved here from phase 04): `RunVcpu`'s `budget_ns` ends the run on time
  through the VMX-preemption timer.
- Dropped (see Deviation Log): carrying a pending port read into the next `RunVcpu`; serving
  host-interrupt exits in the kernel.
- Non-functional: no wire or guest-visible change. The SVM backend's entry path is unchanged.

## Architecture

`VmxVcpu::run` keeps one `with_vmcs` scope per call. Each CPU records the table epoch it last
ran `INVEPT` for (`EPT_VALIDATED`), each vCPU the CPU it last entered on, and each VMCS what
its host-state area was written from (`host_state::Stamp`); those three decide what an entry
skips. The registry lock is still held across the call (phase 05 changes that).

## Assumptions

- **Claim:** a per-CPU "last validated EPT" record is sound: translations cached on a CPU were
  made under the instance and generation it last entered. **Confidence:** high (SDM vol. 3,
  "Operations that Invalidate Cached Mappings"). **Verify:** hostile-runner reset and remap
  scenarios; a host test for the record's transitions.
- **Claim:** host-state fields survive `VMCLEAR`/`VMPTRLD` (only the launch state resets).
  **Confidence:** high (SDM vol. 3, "VMCS Types" / `VMCLEAR`). **Verify:** lanes.
- **Claim:** the kernel can take a host interrupt inside a syscall without switching tasks,
  and can ask whether a reschedule is due and whether the caller's mailbox is non-empty.
  **Confidence:** low. Read so far: `X86_PREEMPT` per-hart flag (`smp_bringup.rs:36`),
  `has_pending_notification` (`task/async_ipc.rs:219`). **Verify:** before step 4.

## Related Files

- Modify: `hal/arch/x86/src/x86_64/vmx_vcpu.rs`, `vmx_vcpu/vmcs_scope.rs`,
  `vmx_vcpu/host_state.rs`, `kernel/src/arch/x86_64/hypervisor/svm_registry.rs`,
  `kernel/src/arch/x86_64/vm.rs` (budget), `kernel/src/arch/x86_64/memory/ept.rs` (generation)
- Modify: `scripts/mutable-globals.toml` and `docs/generated/mutable-globals.md` for the
  per-CPU record; `docs/guides/tier3b-linux-vm.md`; `docs/project-changelog.md`
- Done: `cells/services/hypervisor/src/boot_x86_profile.rs`, `scripts/chrome-guest/init.sh`

## Implementation Steps

1. (done) Quiet guest console; measure.
2. (done) `INVEPT` by epoch, VPID, host-state stamp; measured together (evidence).
3. (dropped) Pending port read carried by the next `RunVcpu`.
4. (dropped) In-kernel host-interrupt handling. (done instead) `budget_ns` by the
   VMX-preemption timer.
5. Lanes: Chrome, display, `HV_ACCEL=kvm` hypervisor smoke, the four x86 Tier 3 CI jobs.

## Success Criteria

- [x] VMX bookkeeping per `RunVcpu` call (run scope minus guest time) reported before and
      after: 84 µs -> 38 µs (`evidence/phase03-entry-path.md`).
- [x] Click -> Chromium UI and the bench rows before and after (two rounds in turns; 20
      launches each for the soak).
- [x] Chrome lane, display lane, hypervisor smoke (KVM) and the four x86 Tier 3 CI jobs pass.
      (2026-11-26, current tree: Chrome lane 24/24, display lane, `qemu-x86-hypervisor-boot`
      and the four Tier 3 jobs.)

## Security Considerations

Skipping `INVEPT` is the one change that can leave a stale translation: the generation must
move on every unmap, on table free and on root reuse. A guest that never exits is still
bounded by `budget_ns` and host interrupts.

## Risk Notes

The gain is largest under nested virtualization, where each VMX instruction traps; on a
physical machine it is smaller and unmeasured. Each step is its own commit.

## Deviation Log

- Measured (2026-11-25, temporary counters in `vmx_vcpu.rs` and `svm_registry.rs`, removed
  after the runs): exits from the click to Chromium's UI were port I/O 47.5k (UART 32.5k, 8259
  14.8k), EPT violations in the virtio window 7.0k, HLT 3.9k, host interrupts 1.6k, CPUID 1.3k.
  The guest wrote 21 kB to its console; each byte is a line-status read and a data write.
  The earlier assumption that host interrupts dominate was wrong.
- Decision (done early, 2026-11-25): `loglevel=4` on the Chrome command line and
  `seatd -l error`. They need no kernel or interface change and were the largest single item.
  Result, two runs each: UART exits 32.5k -> 2.6k, guest console 242 lines -> 15, click ->
  Chromium UI 28.9-30.9 s -> 20.8-22.3 s.
- Measured: an idle Chromium costs 2,000-2,300 HLT round trips a second and a host CPU
  (45.6k HLT exits and 10.7 s "in guest" over 20 s). Not fixed here: see Architecture.
- Measured (bench lane, two rounds in turns, quiet host): epoch `INVEPT` + VPID + host-state
  stamp take the cost around the guest from 84 µs to 38 µs per call; launch 24.9 s -> 23.4 s,
  `js1` 577 ms -> 525 ms. The hypothesis that the per-entry `INVEPT` and the missing VPID made
  the guest itself slow (shadow-EPT rebuilds under KVM) did not hold: guest time did not drop.
- Surprise: Chromium died of SIGSEGV in 1 of 20 launches, with this phase and with the
  phase-02 image alike. Not caused here, not explained. The guest init now restarts Chromium
  when a signal kills it (at most twice) and prints the kernel's fault line.
- Dropped: pending port read (step 3). `cell_pic_ms` 191 ms for 21,915 exits: the Cell's part
  of an 8259 access is 9 µs; the trip is the cost, and phase 04 removes the trips.
- Dropped: in-kernel host-interrupt handling (step 4). Host-interrupt exits are 3-4 % of the
  trips and 0.11 ms each in the Cell. x86 syscalls run with IF=0 (`syscall.rs`: no `sti`
  before the dispatcher), so serving the interrupt inside `RunVcpu` would add an interrupt
  window, and with it a preemption point, to a syscall.
- Decision: `budget_ns` (ignored on x86 until now) ends a run through the VMX-preemption
  timer. It was planned for phase 04 and is what lets the Cell be on time for a PIT tick
  without waiting for the host's 100 Hz tick.
- Surprise (host): the build host was 98 % busy with another process for hours of this work
  (see `evidence/phase02-baseline.md`). The bench runs its targets in turns for that reason;
  the plan's earlier figures (reference 6.9 s, CellOS 21-22 s) were from a quiet hour.
- Found: the block path. 7,823 virtio-blk exits took the Cell 27.3 s of a 65 s launch on the
  loaded host (about 6 s of 25 s quiet): the VFS reads from its block driver 7 sectors per
  IPC, each an NVMe command (`types::block_wire::MAX_RUN_SECTORS`, ABI record §20). A larger
  run is a `libs/types` change or a driver DMA change: owner decision, not taken here.
