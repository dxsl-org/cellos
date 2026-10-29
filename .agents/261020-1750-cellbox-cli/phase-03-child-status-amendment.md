# Phase 03 amendment A4 — preparer-queryable child terminal record (+ private stdin-filter fixture)

Status: **CP1 approved by the user on 2026-10-21 (“Duyệt cả A và B”) for Part A and Part B exactly as written; implemented and verified on the private image (see Results).** This document changes no source, ABI, selector, lock or image. It is a **protected ABI/kernel proposal**: it adds one `KernelOp` discriminant and one kernel handler. Source identity: HEAD `fabd6ab7e3f2d9ea90a931fe53b7d43e509888dd` plus the dirty tree; anchor hashes (SHA-256 prefix): `libs/api/src/abi/cell_session.rs` `7c5530ee…`, `kernel/src/task/cell_session.rs` `57aeb7d4…`, `libs/terminal/src/pipe_child.rs` `cc157ffc…`, `kernel/src/loader/launch_profile/{targets,profiles,mod}.rs` `8892a225…`/`0e524309…`/`53e9e41a…`, `kernel/Cargo.toml` `bc59c142…`, `Cargo.toml` `31d831c4…`, `Cargo.lock` `119fee53…`. Follows [A3 results](phase-03-external-bytes-amendment.md) (G1 pass, **G2 fail**, G3 unproven).

## 1. Evidence driving the proposal
- G2: `sys_wait(root_tid)` returns `Stale` for a child that already exited and was reaped (`kernel/src/task/syscall.rs:5411-5430`); the shell reports `exit status unavailable`.
- The kernel **already records** each committed child's terminal state: `Prepared { state, reason, exit_code, terminal_sequence }` (`kernel/src/task/cell_session.rs:80`), set to state 2 at `retiring` (`:1430-1443`) and state 3 with a canonical sequence at `retired` (`:1462-1472`).
- The only reader is `KernelOp::QueryReceiptChild = 24` (`:870-878`), which requires a receipt bound by a named issuer service (input/VFS) and the issuer's own principal. A shell-owned pipeline child has no such receipt, so the record exists but its preparer cannot read it.
- Stage failure on spawn (`Cancelled`) when a consumer stage exits before its producer is committed is the existing final-reader-close teardown (`close_one`/`close_producers`, `:230-240`). That is correct pipe semantics (producer sees a closed reader); it needs **userspace policy only** (treat as "consumer finished early", the producer stage is not started), not a kernel change.

## 2. Proposed ABI delta (candidate; numeric selector is a candidate, not a reservation until approved)
`KernelOp::QueryChild = 30` (next free; 1..29 are used, `libs/api/src/abi/cell_session.rs:64-76`).
- Request body: `child:ChildRefV1` (32 B). Common 24-byte request header and result header unchanged.
- Authorization: caller principal must equal the child's recorded **preparer** (`Prepared.preparer`); anything else → `WrongOwner`; unknown/stale generation → `Stale`; non-consuming and idempotent; never reveals any other principal's data.
- Result body (24 B): `state:u32` (1 live, 2 retiring, 3 terminal), `reason:u32`, `exit_code:i32`, `reserved:u32=0`, `terminal_sequence:u64`. Same encoding as selector 24's trailer minus `child`.
- Retention: the record must remain readable by its preparer until the child's operation is closed (`CloseOperation`); **gate R1:** verify in source/image that `prepared[]` records are not dropped before then for receipt-less children; if they are, extending retention is part of this amendment and must be reported before use.
- No new right, grant, allowlist bit, quota class or syscall id. Existing per-operation record and request-ledger bounds apply (the call is an ordinary operation-scoped request through `CellSessionV1`).

## 3. Exact file closure (no recursive unlock)
- ABI: `libs/api/src/abi/cell_session.rs` (+ its tests): add the discriminant and a codec test for the 24-byte result.
- Kernel: `kernel/src/task/cell_session.rs`: one match arm (preparer check + read of the existing record) and tests; `kernel/src/loader/atomic_publication_tests/*` only to add a case `AP-PQ` (preparer-only, stale generation, foreign principal denied, record readable after reap, readable after the preparer's operation still open).
- SDK: `libs/terminal/src/pipe_child.rs`: `PipeChild::wait` queries `QueryChild` (retrying `WouldBlock` until state 3, bounded) instead of `sys_wait`; falls back to an explicit `Status` (never a guessed 0).
- Shell: `cells/tools/shell/src/{executor,typed}.rs`: treat spawn `Cancelled` of an upstream stage as "consumer already finished; stage not started" with status 141-equivalent policy per the existing pipeline-status rules (last stage decides), instead of a spawn failure.
- `libs/ostd`, input/VFS services, other kernel modules, locks other than item B: untouched.

## 4. Part B — private stdin-filter fixture (needed for G3; separate approval line)
Approving part A does not approve part B. Part B adds only test-private artifacts:
- New package `cells/tests/phase03-filter` (no_std cell, modes by `argv`: `cat` passthrough of stdin→stdout, `upper`, `noread` (exits without reading), `exit N`), root `Cargo.toml` workspace member and the matching `Cargo.lock` package entry.
- `kernel/src/loader/launch_profile/{targets,profiles,mod}.rs` + `kernel/Cargo.toml`: a new Cargo feature `phase03-filter-profile` (never default, never in canonical images) that adds one `shell → /bin/p03-filter` edge with `CapSet::EMPTY` ceiling, mirroring the `phase02-terminal-profile` mechanism.
- `scripts/qemu-phase03-pipeline-test.sh` builds a private image with that feature and a throw-away key; ordinary builds contain no such row and the canonical quarantined artifacts are not touched, rebuilt or re-signed.

## 5. Verification (after approval; private image only)
Host: ABI codec test, kernel `AP-PQ`, terminal-sdk and shell suites. Image: exit code propagation (`exit 0`, `exit 7`, fast-exit `noread`, killed by Ctrl-C = 130), exact-bytes pass-through (NUL, non-UTF-8, no trailing newline) via `cat` mode, `rows → to jsonl → p03-filter cat`, `p03-filter cat | from jsonl --typed | to text`, ext|ext, early consumer exit (producer not started or `BrokenPipe`, no leak), `take` closes an endless producer, Ctrl-C on a long pipeline, child count back to baseline; regression: Phase 02 fixture 48/48, Phase 03 typed 14/14, kernel AP lane all PASS. Only observed results are recorded; any gate that fails stops the slice.

## 6. Rollback
Revert the files in §3 (and part B's); the numeric discriminant is released only if no image shipped it (none ships: fixtures are private). Bytes delivered through pipelines and files written by redirects are not recalled. This amendment does not lift the rollout quarantine, approve Phase 03 CP2, or change CP2-approved Phase 02 scope.

## Results (2026-10-21; [raw](reports/phase-03-evidence/pipeline-commands-a4.json))
- **Kernel/ABI:** `KernelOp::QueryChild = 30` (preparer-only, non-consuming, 24-byte result) + AP-PQ; kernel AP lane **36/36** (the lane previously stopped at AP-PS and silently skipped later cases; it now waits for AP-PX and requires PC…PX).
- **Gate R1 FAILED as suspected, retention extended (in scope per §2):** (1) `reclaim()` dropped receipt-less terminal children and, separately, children bound only to a VFS cwd receipt; terminal records are now retained until an input receipt is bound or `CloseOperation`; (2) aborted/expired records (states 4/5) are releasable by `CloseOperation`. Terminal records count toward the global 512-record `prepared` cap until their operation is closed, so a shell that never closes pipeline operations would exhaust it — the shell closes every pipeline operation on all exit paths.
- **SDK restructure beyond the written text (same closure):** `Pipeline` is now a tree of Operations (root: shell output/diagnostics; per-stage; ring operations nested toward the last stage) so the kernel's final-reader-close cancellation reaches only upstream stages and never kills downstream ones or the shell's own reader; `stdout` is required; empty-stdin placeholder is closed after commit; `wait_status` reports consumer-lost stops as 141 and other stops as 130.
- **Shell:** tail form streams typed rows into stage 0's stdin ring on demand (bounded memory; earlier version buffered the segment and exhausted the shell heap, killing the shell).
- **Part B:** `/bin/p03-filter` (modes `cat upper noread exit N gen hex endless rows typed`) exists only under feature `phase03-filter-profile`; not in canonical builds.
- **Image:** Phase 03 runner **39/39 PASS** (exit codes 0/7/200 exact; last stage decides pipeline status; consumer-exit/endless-producer; byte-exact pass-through; Ctrl-C cancel + prompt recovery; typed rows streaming into an external stage; admitted external-head shapes; refusals; child count back to baseline). Regression: Phase 02 fixture 48/48, host api 128, terminal-sdk 41, app-shell 65, text-engine 74, ostd/service-input/service-vfs pass; kernel compiles on x86_64/riscv64/aarch64.
- **Not exercised:** in-VM `shell_test` scenarios; cellbox standalone transforms on an image (cellbox name is refused by the signer); background/`fg`/`bg` (still refused before effects); multi-hart/KVM for the Phase 03 runner; `2>`/`2>&1` on typed pipelines. Rollout and Phase 03 CP2 remain pending.

Phase 03 CP2 approved 2026-10-21; see [phase file](phase-03-typed-pipelines.md).
