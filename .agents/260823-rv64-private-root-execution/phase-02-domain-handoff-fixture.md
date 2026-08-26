# Phase 02: Domain handoff fixture

## Context Links

- [allowlist](phase-01-supervisor-allowlist.md)
- `kernel/src/task/context_handoff_selftest.rs`
- `kernel/src/task/domain_switch.rs`

## Overview

Bind the existing handoff worker to one immutable private root before its hart-1
dispatch, then verify the same tuple after its hart-0 resume.

## Key Insights

The existing fixture already proves the required save, release, and reselect
ordering. The only missing domain assertion is pre-dispatch binding plus
post-selection identity equality.

## Requirements

- Build one executable root from the explicit allowlist and the worker stack.
- Bind it while the scheduler lock owns the unpublished worker.
- On hart 0, require the TCB `Domain` identity/generation and hart-local tuple
  to match before emitting a terminal.
- A build/bind/tuple failure emits a precise FAIL terminal and never falls back
  to SAS for the requested domain task.

## Architecture

`configured worker → AddressSpaceBuilder(execution allowlist) → Domain Arc →
hart-1 dispatch → saved context → hart-0 selection/SATP → tuple verification`.

## Related Code Files

`kernel/src/task/tcb.rs`, `kernel/src/task/domain_switch.rs`,
`kernel/src/task/context_handoff_selftest.rs`,
`kernel/src/task/domain_switch_tests.rs`.

## Implementation Steps

1. Add a test-only helper that consumes a worker stack before queue publication.
2. Attach the resulting `Arc<AddressSpace>` once and reject a second bind.
3. Verify identical identity/generation in the resumed worker path.
4. Emit the distinct migration terminal only from that verified path.

## Todo List

- [ ] Bind before hart-1 dispatch.
- [ ] Verify post-selection tuple.
- [ ] Add negative fixture checks.

## Success Criteria

The worker reaches hart 0 through a real SATP selection and observes exactly its
pre-dispatch private root tuple.

## Risk Assessment

A root missing scheduler/trap dependencies faults before the equality check;
that is a fixture failure, not permission to bypass SATP.

## Security Considerations

Binding occurs before a task is reachable by work stealing. The test helper is
unavailable outside `native-domains,test-hooks`.

## Next Steps

Run the architecture- and hart-scoped QEMU matrix.
