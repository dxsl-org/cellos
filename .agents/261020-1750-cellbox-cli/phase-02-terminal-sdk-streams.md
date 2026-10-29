---
phase: 2
title: "Public Terminal SDK and real child/session streams"
status: completed-private-evidence
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

- [x] No cross-session input/output or stranded focus on exit, timeout, interrupt or death. — evidence: [runtime evidence](reports/phase-02-runtime-evidence.md) (private signed-x86 fixture, 48 observations; kernel AP lane 28/28).
- [x] Empty/space-containing arguments, cwd/environment and non-UTF-8 payload reach the child unchanged or fail with an explicit documented limit before launch. — evidence: [runtime evidence](reports/phase-02-runtime-evidence.md) (private signed-x86 fixture, 48 observations; kernel AP lane 28/28).
- [x] Backpressure/EOF/close/partial-write behavior is observed at target runtime; no log-only stdout fallback. — evidence: [runtime evidence](reports/phase-02-runtime-evidence.md) (private signed-x86 fixture, 48 observations; kernel AP lane 28/28).
- [x] Retained/duplicated/descendant holders obey last-writer EOF and operation-scoped revocation; blocked holders wake and stale endpoint generations fail. — evidence: [runtime evidence](reports/phase-02-runtime-evidence.md) (private signed-x86 fixture, 48 observations; kernel AP lane 28/28).
- [x] No GUI/compositor/font dependency is introduced into Terminal SDK or headless shell. — `libs/terminal` depends only on `types`/`api`/`ostd`; the serial shell adds no viUI/Ratatui/font dependency (its pre-existing GUI branch is unchanged).

## Verification

Keep deterministic ownership/lifetime/byte-boundary regressions where they catch real errors. Use a throwaway producer/consumer scenario and a real serial shell smoke on private signed x86 images; P04 proves the actual GUI shell bridge and P05 completes TUI/emulator capabilities. Record exact features, image/policy and observed transitions. Host mocks alone do not pass this phase.

## Assumptions

Existing pipe/session attachment and input provider can satisfy the approved contract. Verify actual owner/lifetime and callable routes before reuse; if not, adopt the exact prerequisite under the controlling owner without changing the selected outcome.

## Security and risk assessment

Secret entry must not enter logs, history or scrollback. New session labels/stream IDs are not authority. Roll back by restoring a matched SDK/kernel/input/image set and cold boot; do not leave dual transports or reuse stale handles. Consumed input or completed external effects cannot be reconstructed; preserve unknown outcomes.

## Deviation log

2026-10-20 — **Completion and automatic source-admission claims withdrawn; incomplete/blocked.** User candidate-design consent is recorded, but the allocated/owner-handoff dossier is incomplete and CP2 remains pending/not eligible. `terminal-sdk` source was added at `libs/terminal`; historical observations record 14 host unit tests and bare-metal PIC compilation for `x86_64-unknown-none`. Those results are retained only as unit/build evidence, not authenticated session integration or proof of subtree cancellation, lifetime/EOF, argv/cwd/environment handoff or target wakeup/restoration. Review identified stream authentication/cancellation flaws; no signed-image child/descendant or two-session runtime proof closes CBX-03..05. All criteria remain unchecked and rollout is blocked. See [reports/rollout-containment.md](reports/rollout-containment.md).

Subsequent continuation — the user explicitly selected **“Giao slice Phase 02”** for the current integrating owner. The [scoped allocation/source/private-fixture annex](phase-02-source-admission.md) is being reconciled against the canonical publication/input/VFS owners and independently reviewed. This accepts the bounded handoff, not numeric allocation or source/execution admission. Protected source edits await exact CP1 approval; all criteria remain unchecked, CBX-03..05 pending, rollout blocked and CP2 not eligible.

The exact [annex](phase-02-source-admission.md) is now independently reviewed and ready to request CP1, SHA-256 `73bc7483ae6d05d6435c27f12a1c2890e3b550e63666b4339ed75f9b8371ae67`; [review evidence](reports/phase-02-admission-review.md) records proposal corrections and read-only integrity checks only. No protected source changes, Build-entry test baseline, signed-image execution or new acceptance evidence have occurred in this admission continuation.


2026-10-21 — **Implemented under the exact CP1 approval; private-fixture evidence recorded; CP2 and canonical rollout remain blocked.** The user approved annex `73bc7483…` (“Duyệt CP1 và fixture riêng”) and amendment A1 `ae6be1c3…` (“Duyệt bổ sung 3 file”) — see [phase-02-cp1-approval.md](phase-02-cp1-approval.md). The SDK, kernel D01/D02 owner (29 selectors, allocation-free publication reservations), input SessionV1, VFS prepared-cwd binder and serial shell consumer now run on a freshly built, privately signed x86 image: [runtime evidence](reports/phase-02-runtime-evidence.md). Two independent implementation reviews found 27 defects; confirmed ones were fixed and the fixtures re-run. A follow-up pass fixed the in-closure limitations (cancel complexity, measurement-log share, cancel granularity, reclamation, FIFO restore, secret scrubbing, KVM/40-command/stray-escape runs); the rest (UART CR→LF/echo, legacy builtin output, issuer death on image, wall-clock race) is listed in the report and carried into CP2 review / needs an amendment. Criterion checkboxes record attributable private-fixture observations, **not** independent acceptance; CBX-03..05 stay pending until that review.

2026-10-25 — **CP2 approved by the user** ("duyệt CP2 phase 02"). Phase 02 is `completed-private-evidence`. Every criterion is checked on private signed-x86 evidence. Since then, every later phase lane has exercised the SDK: P02 48, P04 459, GUI 290 and the kernel lane 39/39 (latest 2026-10-25). Rollout stays blocked; canonical image packaging belongs to Phase 04 criterion 4.

2026-10-26 — **Amendment A2 item 1 superseded by Phase 10 S10-3a** (user decision (e), relayed by Main; [S0 record](phase-10-s0-proposal.md) S10-3a and the S10-3 decisions):
- The decoder grammar is unchanged: UART CR is Enter, and a lone LF is still decoded as Control+J (`libs/api/src/services/session.rs:245-249`).
- Cellbox's canonical line readers (command line, `read -s`, prompts, ssh password) now treat Control+J as accept-line, so a lone LF submits a line.
- The A2 image check `ctrl_j_not_enter` is replaced by `ctrl_j_submits` (`scripts/qemu-phase02-session-test.sh`). It sends LF-terminated commands one at a time, because input typed ahead while a command runs is not kept (S10-3b finding F-TA).
- P02 rerun in import mode on the fleet candidate: 48 passes plus `ctrl_j_submits` (`reports/s10-3-evidence/rerun-fleet/lanes/L1-p02-session-after-e-v2/`).

A2 items 2–5 are unchanged.
