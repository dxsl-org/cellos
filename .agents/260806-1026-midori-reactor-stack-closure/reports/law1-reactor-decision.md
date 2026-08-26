# Law 1 Reactor Decision

## Decision Context

- Date: 2026-08-06
- Branch: `main`
- Gate entry HEAD: `d4cc2aa3`
- Remote relation at gate entry: `main...origin/main [ahead 2]`
- Decision scope: public ABI and semantic work for generic completions and executor-visible wait behavior.

## Evidence Entering The Gate

- Phase 01 completed at `280b8c61`: NIC IRQs drive bounded `NET_RX` completion wakeups, with QEMU producer and regression evidence.
- Phase 02 completed at `d4cc2aa3`: blocked senders resume with an error after peer exit; `Recv`, `RecvTimeout`, and `RecvScatter` remain mailbox semantics; the real shell input path and peer-death path pass QEMU validation.
- Phase 02 final gates: tester PASS, standard review PASS, domain-risk review PASS, and artifact validation PASS.
- No public ABI, executor, VFS implementation, async DMA, or grant-lifecycle migration was included in Phases 01–02.

## Proposed Narrow Contract

If and only if both confirmations are explicit YES:

1. Generalize `WaitCompletion` beyond the existing `NET_RX` source while preserving the current syscall number and allowlist compatibility.
2. Define source identity, peer-target identity, generation safety, timeout behavior, target-gone behavior, and stable completion encoding.
3. Keep `Recv`, `RecvTimeout`, and `RecvScatter` on their current mailbox path; do not migrate them to the completion queue.
4. Preserve boot compatibility for existing `WaitForEvent` cells and preserve current `WaitCompletion(NET_RX)` behavior.
5. Replace the executor dummy-waker loop with a parked wait only after the generic completion contract is validated.
6. Exclude async VFS, grant migration, async DMA, and cancellable grant work.

## Exact Phase 04 Public And Semantic Surfaces

- `libs/api/src/abi/syscall.rs`
- `libs/api/src/abi/completion.rs`
- `libs/api/src/abi/syscall_tests.rs`
- `libs/api/src/abi.rs`
- `kernel/src/task/completion_wait.rs`
- `kernel/src/task/completion.rs`
- `kernel/src/task/scheduler.rs`
- `libs/ostd/src/syscall.rs`

## Exact Phase 05 Implementation Surfaces

- `libs/ostd/src/executor.rs`
- `libs/ostd/src/syscall.rs`
- `libs/ostd/src/ipc.rs`
- `cells/tools/shell/src/main.rs`
- `cells/tools/shell/src/async_utils.rs`
- `cells/services/net/src/main.rs`
- `tests/integration/tests/boot.rs`

## Confirmations

### Confirmation 1 Of 2

- Question: `Authorize public ABI/semantic work for generic completion/executor wait semantics?`
- Answer: `Ủy quyền 1/2 (Recommended)`
- Recorded: 2026-08-06T21:16:28+07:00

### Confirmation 2 Of 2

- Question: `Với đúng danh sách file và contract vừa nêu, bạn có cho phép bắt đầu các chỉnh sửa public ABI/semantic của Phase 04–05 không?`
- Answer: `Ủy quyền 2/2 (Recommended)`
- Recorded: 2026-08-06T21:19:10+07:00

## Gate State

- Phase 03: completed; both explicit confirmations recorded
- Phase 04: authorized and in progress
- Phase 05: blocked until Phase 04 completes
- Public ABI diff before authorization: none; validated with `git diff -- libs/api libs/types` before confirmation 1.
