# C2C Phase 03 slice d CP1 approval record (Law-1 confirmation 1 + lock handoff)

Recorded 2026-11-02. The accountable maintainer/owner (the user) answered **YES**, through the integrating session, to every recommendation of `phase-03d-multi-source-wait-cp1.md` §12. The approved question:

> Approve `phase-03d-multi-source-wait-cp1.md` as the design for:
> - appending `api::completion::source::IPC = 1 << 2` to `libs/api/src/abi/completion.rs`;
> - widening the `WaitCompletion = 242` mask to a non-empty subset of `{NET_RX, TIMER, IPC}`;
> - keeping the record format: one source bit per v1 24-byte record, with IPC `result` = operation token (§6);
> - the §5 semantics;
> - the §8 prototype-before-ABI order;
> - the §9 acceptance matrix.
>
> And approve the §7 lock handoff.

Recorded values:

| ID | Decision |
|---|---|
| D-1 | (i): the `IPC` bit on 242, no new opcode |
| D-2 | IPC record `result` = operation token; terminal kind and bytes only via `IpcTake` |
| D-3 | one-shot latched notification, one per operation |
| D-4 | the SDK `wait`/`wait_and_take` (and `exchange`) cut over to `WaitCompletion` in this slice; `IpcWait = 258` stays as frozen ABI |
| D-5 | broker `NetworkPoller` cutover is a separate optional later slice (own CP1) |
| D-6 | no allowlist bit beyond the shared bit 42 |
| D-7 | kernel-repair overlap granted for `kernel/src/task.rs:343-347` and `kernel/src/task/scheduler.rs:2303-2309` only |
| D-8 | accept both x86 evidence limits, and label them in the evidence: NET_RX by test-hook injection only (no x86 producer); `-smp 2` lane unforced, with the deterministic host model as the interleaving proof |
| D-9 | adopt as a bounded slice under controlling P04 (P03 for A6), with the §7.1 file-bounded closure |

**Rulings recorded with the approval:**
- The P04/P03 owner is the user. The integrating owner is the current C2C session.
- Check rows `261006-0934-httpd-single-cell-concurrency`, `260727-2101-midori-lessons-cellos`, `260801-parallel-midori-closure` and `260615-1325-vicell-app-sdk-v1` are ruled compatible.
- Row `260927-1100-c2c-anywhere-tier-aware` is narrowed for slice d only. All 38 whole-plan relations stay.
- The scheduling record is made in `cell-contract-0024-locks.md`, `plan-portfolio.md` and `260927-1100-c2c-anywhere-tier-aware/plan.md`. These edits are uncommitted at binding, and their post-edit digests are bound below.
- At slice start, the integrator re-checks that no other admitted slice claims a closure path; the slice stops if one does.

**Authorizes:**
- the design (Law-1 confirmation 1);
- the file-bounded exception for the §7.1 paths, in the §8 order:
  - P0 baseline;
  - P1 kernel-private prototype (the syscall keeps refusing the IPC bit);
  - P2 measurement report with go/no-go;
  - P3 public ABI edit, only after a recorded P2 go.

**Does not authorize:**
- Law-1 confirmation 2;
- any `libs/api` edit or `CHANGELOG.md` edit before the P2 go;
- any file outside §7.1, including `kernel/src/task/syscall.rs`, `completion.rs`, `smp.rs`, `libs/ostd/src/executor.rs`, `cells/services/net/**`, `cells/services/net-broker/**` and `Cargo.lock`;
- the broker cutover;
- an x86 NET_RX producer;
- remote/relay/pipelining work;
- network access, keys, signing routes, canonical images, physical or production claims;
- release of any whole-plan lock relation.

Pinned files stay byte-identical. Before edits, a baseline capture covers the host suites (`-p api`, `-p ostd`, kernel x86 host incl. `async_ipc`) and both `local-service-lifecycle-x86` tests.

**Source HEAD at binding:** `0effc2250688cc351d64dd2733fcef66f9941939`. `git status --short` at binding shows no source file dirty, only these documents:
- `M` for `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md`, `.agents/TODO.md`, `.agents/cell-contract-0024-locks.md` and `.agents/plan-portfolio.md`;
- `??` for the two slice-d documents and `.agents/archive/`.

**Pre-edit hashes of the closure:** CP1 §7.1, re-taken in full at the P0 baseline (`reports/phase-03d-prototype.md` §1).

**Digest form:** SHA-256 of whole files including the final newline, with no normalization; no digest is stored inside the file it hashes.

## Bound documents
| Path | SHA-256 |
|---|---|
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03d-multi-source-wait-cp1.md` | `092486f136f1319f2cddf2b4c226c09d7c44d9447b951aef039c434517e21ce3` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md` (post scheduling edit) | `f07a01e520c0bebaa725551b1058551916612c3c5fd9bc600ddc2833a6967785` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | `e656359d5f401f66378d69ae391a12d48cfdea0723963370a2c683d7b07d02c8` |
| `.agents/cell-contract-0024-locks.md` (post scheduling edit) | `f81adccff1889927c53d24dd876e9f33434f459c9fdd15b3ffd5ef051d3e7b54` |
| `.agents/plan-portfolio.md` (post scheduling edit) | `1e05bb4cc15d56c126e8ef4e31f3b39ed9ace076a9c695fb8853269933d8bd92` |
| `docs/specs/17-ipc-wire-contract.md` | `9a228cfd6ce3f4520cae3aed9c4706adf90d867018c301857d5a23ba2af59f22` |
| `docs/specs/20-unified-ipc-contract.md` | `9ef8ba8d809aeea78c30875bfbfbf846f2b2e5ab2038784134068bf946af117a` |
| `docs/decisions/0023-local-service-generation-binding.md` | `138bed58e53bb3056991e7f15169a56cb0597d5b0c9bd63f2812159cf7ab3d26` |
| `docs/code-standards.md` | `3be252a9ed9e9efb3075c48a0f1a2bf19bf426431306dd5b075bdfd302f9106b` |
