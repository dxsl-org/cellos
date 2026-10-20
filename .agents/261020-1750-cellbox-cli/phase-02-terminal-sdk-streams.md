---
phase: 2
title: "Public Terminal SDK and real child/session streams"
status: pending
priority: P1
tier: thinking
dependencies: [1]
---
# Phase 02: Terminal SDK and streams

## Overview and requirements

Deliver public headless session/I/O facilities for the actual shell and independent apps. CBX-03..05 apply. Phase 01's contract/owner/ABI gates are required before affected source changes.

## Architecture and related files

Create proposed `libs/terminal` with data-path/session APIs independent of viUI/Ratatui. Reuse/adapt `libs/ostd/src/{io,args,syscall}.rs`, current pipe primitives and concrete admitted task/Cell ownership providers. Shared kernel/API/input/launch/VFS changes, if required, belong to one integrating owner and their approved delta, not to an app-private workaround. Update existing `cells/tools/shell` consumers as needed for a real target smoke before the later path cutover.

## Implementation steps

1. Implement session attachment and lifetime with authenticated owner/foreground identity, one effective input reader, duplex output and separately routed diagnostics. Keep data bytes distinct from console logs.
2. Wire child stdin/stdout/stderr and completion to bounded approved pipe/session endpoints. Endpoints are non-inheritable by default and delegated only through explicit typed child grants; specify duplicate holders, descendant delegation, generation/reuse protection and last-authorized-writer EOF. Retain exact argv/transport limits and explicit cwd/environment; never delegate by an untrusted numeric field.
3. Define raw/echo modes and size/resize events with capability queries. Track per-session previous modes for restoration; do not let a child's normal exit restore another session's state.
4. Implement foreground shell/child handoff, interrupt/cancellation delivery, half-close/EOF and peer death cleanup. Reconcile the current input-service focus path with stream readiness to avoid blocking one provider while waiting on another.
5. Preserve timeouts and partial I/O with bounded backpressure. Cancellation/consumer close revokes the operation's delegated endpoint subtree, wakes blocked readers/writers with documented outcomes and rejects stale generations; unrelated sessions/resources are unaffected. A retained parent/descendant writer must not strand canceled work. Do not retry uncertain mutations.
6. Exercise an actual child and child-created descendant with duplicated/retained endpoints: parent close, last-writer close, consumer close, cancellation, holder death and stale reuse. Prove exact bytes, separate stderr/status, bounded wakeup/cleanup, two-session independence and abnormal foreground-child loss on a signed x86 image.
7. Update API/SDK docs and actual callers for approved deltas in the same slice; remove any temporary transport scaffold after the smoke.

## Success criteria

- [ ] No cross-session input/output or stranded focus on exit, timeout, interrupt or death.
- [ ] Empty/space-containing arguments, cwd/environment and non-UTF-8 payload reach the child unchanged or fail with an explicit documented limit before launch.
- [ ] Backpressure/EOF/close/partial-write behavior is observed at target runtime; no log-only stdout fallback.
- [ ] Retained/duplicated/descendant holders obey last-writer EOF and operation-scoped revocation; blocked holders wake and stale endpoint generations fail.
- [ ] No GUI/compositor/font dependency is introduced into Terminal SDK or headless shell.

## Verification

Keep deterministic ownership/lifetime/byte-boundary regressions where they catch real errors. Use a throwaway producer/consumer scenario and a real serial shell smoke on private signed x86 images; P04 proves the actual GUI shell bridge and P05 completes TUI/emulator capabilities. Record exact features, image/policy and observed transitions. Host mocks alone do not pass this phase.

## Assumptions

Existing pipe/session attachment and input provider can satisfy the approved contract. Verify actual owner/lifetime and callable routes before reuse; if not, adopt the exact prerequisite under the controlling owner without changing the selected outcome.

## Security and risk assessment

Secret entry must not enter logs, history or scrollback. New session labels/stream IDs are not authority. Roll back by restoring a matched SDK/kernel/input/image set and cold boot; do not leave dual transports or reuse stale handles. Consumed input or completed external effects cannot be reconstructed; preserve unknown outcomes.

## Deviation log

None; implementation pending. Log future decisions/deviations/surprises with reason, impact and revert before proceeding.
