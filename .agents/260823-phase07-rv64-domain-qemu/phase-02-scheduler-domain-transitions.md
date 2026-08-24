---
phase: 2
title: "Scheduler domain transitions"
status: pending
priority: P1
effort: 2d
dependencies: [1]
tier: thinking
---

# Phase 02: Scheduler domain transitions

## Overview

Bind each task to either SAS or one immutable `Arc<AddressSpace>` generation and make the
scheduler derive, then atomically publish, the required root transition before user return.

## Requirements

- Add `TaskAddressSpace::Sas | Domain(Arc<AddressSpace>)` to `Task`; task construction
  defaults to `Sas`. A domain task records identity/generation once and never mutates to
  SAS in place.
- Replace raw two-context switch return with `SwitchPlan { outgoing, incoming,
  transition: SasToSas|Activate(DomainRef)|SameDomain|ToSafeRoot }`. Selection happens
  while scheduler state is stable; RV64 activation occurs after outgoing save and before
  incoming user restore.
- `SasToSas` and same live-generation domain switches MUST issue no SATP write and no
  mandatory flush. SAS→domain, domain→SAS, and domain A→B activate destination root.
- Per-hart state contains `(current_domain_id, generation)` and is updated with selected
  task identity. Interrupt, syscall, idle, migration, task→boot, deferred fault, nested
  preemption, and remote work steal all consume the same SwitchPlan.
- `Dying` tasks are removed from queues before scheduling, may not migrate, and select the
  safe root. The existing RV64 incoming switch completion hook is the only point allowed
  to clear outgoing attribution after raw context save.

## Architecture

`pick next task → derive SwitchPlan → publish task/domain tuple → save outgoing Context → RV64 activate if required → restore incoming Context`; safe-root completion owns outgoing attribution release.

## Assumptions

None — scheduler’s current RV64 switch-completion dependency is directly identified in the source inventory.

## Related Files

- Modify: `kernel/src/task/tcb.rs`, `kernel/src/task/scheduler.rs`,
  `kernel/src/task/hart_local.rs`, `kernel/src/task.rs`, `hal/arch/riscv/src/rv64/asm/switch.S`.
- Create: `kernel/src/task/domain_switch.rs`, `kernel/src/task/domain_switch_tests.rs`.

## Implementation Steps

1. Construct `SwitchPlan` beside `pick_next_local`; prohibit architecture assembly from
   looking up a task or a root after the scheduler lock is released.
2. Install current-domain state on destination selection and clear it only on the safe-root
   completion path, preserving existing task/context handoff ordering.
3. Route activation through Phase 01 HAL API and instrument test hooks with per-hart SATP
   write/flush counters and last `(id,generation)` acknowledgement.
4. Make migration and exit reject/evict `Dying` generations before any ready-queue choice.
5. Emit `S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0`,
   `S22-RV64-SWITCH: PASS`, and `S22-RV64-DYING-NONSCHEDULABLE: PASS` only from test
   fixtures, with hart count in each line.

## Test Matrix

| Runner | Cases | Gate |
|---|---|---|
| `cargo test -p cellos-kernel domain_switch --features native-domains,test-hooks` | all four transitions, task→boot, fault, idle, migration rejection | non-QEMU |
| `bash scripts/qemu-native-domain-test.sh --harts 1 --case switch,sas-fastpath` | real RV64 SATP counter and user-return order | RV64 QEMU, 1 hart |
| `bash scripts/qemu-native-domain-test.sh --harts 2 --case switch,migration` | remote selection/migration preserves domain tuple | RV64 QEMU, 2 harts |

## Success Criteria

- [ ] Existing SAS test fixture observes zero root writes/mandatory flushes in its loop.
- [ ] No task runs with a root/tag differing from its live TCB generation.
- [ ] A Dying generation cannot be selected, stolen, or migrated.

## Security Considerations

Never activate a root from an untrusted task field or a stale queue record. Counter hooks
are test-only and cannot be an admission signal.

## Risk Notes

The current scheduler has RV64-specific post-save attribution; maintain that ABI exactly
before adding root activation. A two-hart pass says nothing about a larger hart count.

## Deviation Log

None.
