# Phase 04: Runtime allocation registry

## Context Links

- `.agents/debug/debug-260823-rv64-private-root-fault.md`
- `kernel/src/memory/frame.rs`
- `kernel/src/memory/address_space.rs`

## Overview

Replace inferred address ranges with a kernel-owned registry of supervisor
allocations that private roots may map.

## Key Insights

The faulting worker root had a zero leaf PTE for a late runtime allocation.
Mapping an allocation prefix only moved the fault to an unregistered page-table
frame. A static-image/heap/stack list cannot express kernel allocation lifetime.

## Requirements

- Record every supervisor allocation that a private-root execution path may
  dereference: static ranges, kernel heap, scheduler/runtime state, selected
  stack, and private root/page-table frames.
- Associate each record with an owner and lifetime; reject USER pages,
  non-page-aligned ranges, unknown physical frames, stale records, and broad
  allocator-prefix imports.
- Retire a record only after its root/hart lifetime closes.

## Architecture

`kernel allocation event → typed registry record → immutable root snapshot →
AddressSpaceBuilder supervisor mappings`. The builder receives records, never a
raw allocator range or copied `KERNEL_ROOT` level.

## Related Code Files

`kernel/src/memory/frame.rs`, `kernel/src/memory/address_space.rs`,
`kernel/src/task/stack.rs`, `kernel/src/task/scheduler.rs`, and a new focused
registry module under `kernel/src/memory/`.

## Implementation Steps

1. Define the range record, owner enum, validation, and lifetime contract.
2. Register existing kernel heap and scheduler-owned allocations at creation.
3. Register selected task stacks and private page-table frames transactionally.
4. Build roots only from a validated snapshot; add negative tests for unknown,
   USER, stale, and prefix-derived ranges.
5. Re-run the two-hart handoff and require the post-resume domain tuple marker.

## Todo List

- [ ] Design registry API and lock order.
- [ ] Instrument owned allocation producers.
- [ ] Snapshot records into private roots.
- [ ] Add fault-negative tests and QEMU migration verification.

## Success Criteria

No private-root execution fault is caused by an omitted kernel allocation; every
mapped supervisor page is attributed to a typed live record.

## Risk Assessment

The registry touches allocator and scheduler lifetimes. Incorrect removal can
produce use-after-unmap; stale retention weakens isolation. Both require tests.

## Security Considerations

The registry is kernel-only. It does not accept caller-supplied physical ranges,
map free allocator space, copy global page-table levels, or expose USER PTEs.

## Next Steps

Implement the registry before reopening Phases 01–03.
