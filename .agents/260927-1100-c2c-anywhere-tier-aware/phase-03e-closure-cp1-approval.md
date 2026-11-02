# C2C Phase 03 slice e CP1 approval record (lock handoff; no Law-1 surface)

Recorded 2026-11-02. The accountable maintainer/owner (the user) answered **YES**, through the integrating session, to every item E-1..E-8 of `phase-03e-closure-cp1.md` §12. The approved question:

> Approve `phase-03e-closure-cp1.md` as the design that closes C2C Phase 03:
> - the G1 fix: every receive arm commits the delivered record by `delivery_id` (`PendingMailbox::remove_delivered`), so a masked receive can no longer deliver an operation twice;
> - dispatched cancellation stays `Indeterminate`, with no cancel-ack and no ABI change (§4);
> - `PendingCall` drop cancels and takes an unsettled operation (§3);
> - peer-queue admission headroom H = 8 of 64 for operation submits, with bounds B1-B3 (§5);
> - the §8 order and the §9 acceptance matrix, including the rewritten `PEER-PRESSURE` leg (§7.2).
>
> And approve the §7 lock handoff, including the kernel-repair line-bounded exception for `kernel/src/task/syscall.rs:5452-5456, 5609-5613, 5815-5819, 5949-5962`.

Recorded values:

| ID | Decision |
|---|---|
| E-1 | adopt slice e under controlling P04 (P03 for the receive-commit and cancel-race rows) with the §7.1 closure; one scheduling record |
| E-2 | dispatched cancellation stays `Indeterminate`; no cancel-ack, no ABI |
| E-3 | kernel-repair handoff granted for `kernel/src/task/syscall.rs:5452-5456, 5609-5613, 5815-5819, 5949-5962` only |
| E-4 | headroom H = 8 of 64; identity `(cell, generation)`; operation submits only |
| E-5 | `PendingCall` drop releases an unsettled operation; detach via `mem::forget` |
| E-6 | registry restart + `ServiceRef` re-lookup stays in C2C Phase 02 step 3 |
| E-7 | on CP2, Phase 03 is done at the x86 QEMU/TCG ceiling with CP1 §10's named exclusions |
| E-8 | the `CHANGELOG.md` entry lands in this slice (no commit) |

**Rulings recorded with the approval:**
- The P04/P03 owner is the user. The integrating owner is the current C2C session.
- Row `260927-0739-kernel-architecture-repair` is ruled for E-3: the four `syscall.rs` hunks only; every other line of that file stays pinned.
- Check rows `261006-0934-httpd-single-cell-concurrency`, `260615-1325-vicell-app-sdk-v1`, `260727-2101-midori-lessons-cellos` and `260801-parallel-midori-closure` are ruled compatible.
- Row `260927-1100-c2c-anywhere-tier-aware` is narrowed for slice e only. All 38 whole-plan relations stay.
- The scheduling record is made in `cell-contract-0024-locks.md`, `plan-portfolio.md` and `260927-1100-c2c-anywhere-tier-aware/plan.md`. These edits are uncommitted at binding, and their post-edit digests are bound below.
- At slice start, the integrator re-checks that no other admitted slice claims a closure path; the slice stops if one does.
- Before implementing (a) and (b), the integrating session requires failing-before tests for the masked-receive double delivery and the dropped `PendingCall` slot leak; a defect that does not reproduce stops that item.

**Authorizes:**
- the §3-§5 design; no Law-1 confirmation is requested because no exported ABI changes (CP1 §6);
- the file-bounded exception for the CP1 §7.1 paths, in the §8 order: P0 baseline, P1 tests-first, P2 fixes, P3 lanes, then CP2.

**Does not authorize:**
- any `libs/api` edit, new syscall, flag, allowlist bit, completion source or `IPC_STATUS` change;
- any file outside §7.1, including `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `smp.rs`, `completion*.rs`, `tcb.rs`, `ipc_pending_selftest.rs`, `kernel/src/cell/hotswap.rs`, `libs/ostd/src/{service,executor}.rs`, `cells/services/**`, Spec 17 and `Cargo.lock`, and any `syscall.rs` line outside the four hunks;
- raw-mailbox admission changes, provider cancel-ack, registry restart work, the broker `NetworkPoller` cutover, an x86 NET_RX producer;
- network access, keys, signing routes, canonical images, physical or production claims;
- a commit;
- release of any whole-plan lock relation.

Pinned files stay byte-identical. P0 captures the host suites (`-p api`, `-p ostd`, `-p cellos-kernel`, `--target x86_64-unknown-linux-gnu`, `RUSTFLAGS` and `CARGO_TARGET_DIR` unset) and the three `local-service-lifecycle-x86` tests before any edit.

**Source HEAD at binding:** `5638153118921e53c510571ba5dc50c26d73b0a7`. `git status --short` at binding shows no source file dirty, only these documents:
- `M` for `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md`, `.agents/cell-contract-0024-locks.md` and `.agents/plan-portfolio.md`;
- `??` for the two slice-e documents and `.agents/archive/`.

**Pre-edit hashes of the closure:** CP1 §7.1, re-taken in full at the P0 baseline (`reports/phase-03e-cp2-package.md` §1).

**Digest form:** SHA-256 of whole files including the final newline, with no normalization; no digest is stored inside the file it hashes.

## Bound documents
| Path | SHA-256 |
|---|---|
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03e-closure-cp1.md` | `195e7763f4f0b383738a4196b6beb98ee8c07575e4daaf8e109e46d669db9463` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md` (post scheduling edit) | `e73e5290ab50e424da7e2340a8f28b57b9c94217da6e4b1c5f220ab3c1a6920e` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | `691e0dda9967aa818409e1892607494c328c3d80444e2e1981354caf3333aba8` |
| `.agents/cell-contract-0024-locks.md` (post scheduling edit) | `8b90a8508b10237274c1e63e93c9d51f87751e30450f2eacc1a3524cba3cba9c` |
| `.agents/plan-portfolio.md` (post scheduling edit) | `18322f56002859a62ce3c042ce3961f27d2b00be4e952e3a6c45b85929e4796f` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp2-approval.md` | `daf825a5925aec368ff695b0f2f1ad5b711f974ef6a96ad2829c29cd563c3537` |
| `docs/specs/17-ipc-wire-contract.md` | `916f4cb1db49108632d31080676a2b38b027c94bb46a89281798e676ea4095dd` |
| `docs/specs/20-unified-ipc-contract.md` | `4dc8b4cd904bcd9ec0b0088ab1efd4ef51942d6788648441b00bf979d0f18e75` |
| `docs/decisions/0023-local-service-generation-binding.md` | `138bed58e53bb3056991e7f15169a56cb0597d5b0c9bd63f2812159cf7ab3d26` |
| `docs/decisions/0024-cell-kernel-authority-contract.md` | `275b068c1f7672a7c6381d3cb80abb7118dd10e983ad3a23941c4790554c32dc` |
| `docs/code-standards.md` | `3be252a9ed9e9efb3075c48a0f1a2bf19bf426431306dd5b075bdfd302f9106b` |
