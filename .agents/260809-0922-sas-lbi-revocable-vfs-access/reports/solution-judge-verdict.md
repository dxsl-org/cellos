# Solution Judge Verdict

## Verdict

Simplicity-first wins by one point as the lowest-blast, most reversible current-code plan. Use **bounded copy-out only as the same-SAS migration tactic** and **file handle + bounded read as the target mechanism**.

## Ranking

1. Simplicity-first: 26/30 — blast radius 5, reversibility 5, complexity 5, current-code fit 5, security correctness 3, performance/Tier-2 migration 3.
2. Risk-first: 25/30 — blast radius 4, reversibility 4, complexity 3, current-code fit 4, security correctness 5, performance/Tier-2 migration 5.
3. Mechanism order: bounded copy-out now; file handle + bounded read after Law 1 approval; revocable `ReadGrant` only after a real opener, close/reaper, and pin/ack substrate.

The simplicity candidate wins the KISS tie-break because it reuses the only live bounded copy-out producer and avoids premature ABI/state expansion. Its weak point—deferring direct death pruning—must become a hard checkpoint before durable handles ship.

## Required Graft

Carry the risk-first endpoint invariants into the winning plan:

- directory-derived file handles;
- kernel-attested `Caller { cell, generation }` ownership;
- per-use authorization recheck;
- owner-only close/revoke indistinguishable from unknown handles;
- synchronous read lifetime ending at reply;
- cancellation either escapes no memory or retains pin/quarantine until acknowledgement.

## Rejected Alternatives

- Bounded copy-out is not the permanent endpoint because it remains path-addressed.
- `ReadGrant`-first is rejected because its current VFS handle producer is test-only.
- Kernel `OpenCap` is not a VFS replacement because it bypasses VFS mount, overlay, and ACL semantics.
- Identity-less fast IPC is invalid.

## Non-Negotiable Gates

- No public ABI change before two explicit Law 1 confirmations.
- No durable handle phase until cleanup is proven for Exit, ForceExit, fault, watchdog, and hot-swap.
- `NotifyOnExit` is SpawnCap-gated; using or replacing it requires a separate authority/design checkpoint. Generation-based lazy purge plus bounded sweep is containment, not immediate cleanup.
- No Tier 2, DMA, reactor, SMP, cancellable grant-read, or raw-`DataPtr` revocation expansion.

## Evidence

- `reports/solution-simplicity-first.md:22-30,64-81`
- `reports/solution-risk-first.md:25-47`
- `scout-report.md:49-80`
- `research/haily-researcher-01-vfs-surface-report.md:25-47,65-70`
- `research/haily-researcher-02-lifecycle-pinning-report.md:49-64`
- `kernel/src/task/syscall.rs:2281-2315` (`NotifyOnExit` SpawnCap gate)
