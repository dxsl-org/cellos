# Phase 01: Supervisor allowlist

## Context Links

- `.agents/260823-phase07-rv64-domain-qemu/phase-01-rv64-address-space.md`
- `.agents/260823-phase07-rv64-domain-qemu/phase-02-scheduler-domain-transitions.md`

## Overview

Define the exact supervisor pages required after SATP selects a private root.

## Key Insights

`__switch` changes SATP before loading the incoming stack and invoking
`vi_context_switch_complete`; the private root must therefore map kernel text,
read-only data, writable globals/hart-locals, kernel allocator backing, and the
incoming kernel stack before a domain task can run.

## Requirements

- Export page-aligned linker bounds for text, readonly, and writable kernel ranges.
- Record the bounded kernel heap allocation after boot and map it supervisor-only.
- Resolve physical pages only for those fixed ranges; reject absent mappings,
  USER permissions, range overlap, unaligned ranges, and overflow.
- Preserve RX/RO/RW flags; never infer a mapping from arbitrary `KERNEL_ROOT` data.

## Architecture

A kernel-owned layout records fixed ranges. `AddressSpaceBuilder` consumes that
layout and a selected stack to create explicit `SupervisorMapping` entries.

## Related Code Files

`kernel/linker.ld`, `kernel/src/main.rs`, `kernel/src/memory.rs`,
`kernel/src/memory/address_space.rs`, new focused mapping-policy module.

## Implementation Steps

1. Add linker bounds and verify every bound is page aligned.
2. Record heap and required UART range after their authoritative boot setup.
3. Build only fixed static, heap, stack, and UART mappings with non-USER flags.
4. Unit-test rejection and W^X properties without executing a domain.

## Todo List

- [ ] Implement layout recording.
- [ ] Implement fixed mapping construction.
- [ ] Add rejection tests.

## Success Criteria

An executable root has no peer USER page, broad usable-RAM mapping, or copied
page-table level; every mapped supervisor page has an explicit source range.

## Risk Assessment

Missing one supervisor page faults closed. Overbroad mapping violates the
isolation contract and is rejected.

## Security Considerations

Kernel-only pages are necessary for supervisor execution, not a capability grant.
No untrusted task field selects virtual or physical supervisor ranges.

## Next Steps

Bind the handoff worker before it reaches hart 1.
