# G1 implementation review and finite capture domain

Recorded 2026-10-14. Scope: actual scoped Intel-private implementation, not downstream CP1/CP2 or hardware qualification. Entry manifest and84source baseline matched before edits; baseline trace suite98/98. Initial independent kernel/clock reviews found genuine source blockers. Their corrected-design follow-up services failed before delivering a review; no independent corrected-source PASS is claimed. Integration owner therefore re-read the actual corrected source and adjudicated the findings below before issuing any private capture receipt.

## Actual corrected paths and findings

- `kernel/src/task/scheduler.rs` stages accounting before Running/current identity/publication pins. Expected stale eligibility returns Ok(false), restores the exact ready queue reservation and refreshes eligibility; retry bounded by64Cell slots. Same-TID/no-switch does not stage. Ready local pick and steal both use the private predicate; existing fixture/normal APIs remain live.
- `cpu_accounting.rs::segment` computes checked charge/control/departure rows and reserves witness slots before committing; at most two occupied candidate rows, no41304byte stack copy. `settle` and analytical idle skip preflight rows/arithmetic/capacity before commit. Failure seals rather than resuming unaccounted incoming work.
- Immutable events1/16 plus header retain full HPET base/capabilities/configuration/period. Every advance and cutoff compares the tuple/raw progression. HAL observed timer functions are x86_64-gated; private boot requires GenuineIntel and actual nonfallback LAPIC initialization. Normal initializer does not perform the private checked observation.
- `service_summary` coalesces kind17 under ACCOUNTING into one row per hart/period with maximum observed service gap and checked no-switch count; individual5/13 records are absent in accepted schema. Mutable summaries are read only after freeze; decoder separates them from immutable ownership chronology.
- Workload workers use one existing attested blocking receive; root sends one worker-token release to each exact TID only after both Spawn calls. Consumer joins sender TID/Cell/generation to the measured root, not a task-name inference.
- Retirement retains the last completed exact-owner outgoing kind4 serial in event order, not the next unused or numerical maximum. It requires no active/pending owner and retains full terminal state before clearing the live row.
- Callback fatal path is UART-free/allocation-free, masks interrupts, emits distinct private debug-exit error and never returns. A caller may still hold SCHEDULER; no allocating retirement runs in the callback.

These are source-reviewed corrections; runtime/tests remain separate observations. Config/caller closure is the inspected local build matrix, not exhaustive external semantic closure. LSP rust-analyzer references failed(code0), recorded as unavailable.

## Source-enforced finite experiment certificate

The preserved16-period development action schedule has one selected bench root and two same-generation workers. The private source refuses elapsed time >=18P and more than64workload-involving raw stages. Completion and proven pre-raw abort are exclusive. Policy-change events are not part of this fixed workload. Control-only transitions are omitted from the workload ownership journal but remain in aggregate control charging. No record is evicted or silently truncated.

| Successful record family | Source maximum |
|---|---:|
| Raw stage plus completion/abort |128|
| Two admitted harts ×18period service summaries |36|
| Period/analytical idle recurrence |18|
| Throttle once pergroup/period |18|
| Enable/full-clock/bind/begin-retire/retired/cutoff |6|
| Total |206|

The same ceiling covers oneCPU. This is a bounded **accepted experiment domain**, not a promise every workload completes: extra stages, elapsed bound, capacity/H/S/D/clock/identity failure invalidates the run with preserved failure exit. The source gates are independent of measured production limits. The receipt pins actual build/input/review bytes and does not manufacture issuer authority. Capture uses q35/TCG with synthetic GenuineIntel vendor; it is not genuine Intel hardware or VMX proof.

## Execution and promotion boundaries

Before accepted capture, verify exact producer source pins and review receipt. Consumer must independently reconcile per-hart intervals/aggregate charge, period/bank/debt, birth proration, retirement, attested workload actions and observed overlap; PASS markers alone cannot qualify. TwoCPU uses separately qualified same-build oneCPU G1 evidence, never relaxes legacy P01 settled-resource gate.

HPET drift/liveness/physical accuracy, source-complete service-delay WCET, physical exhaustion-to-final-departure D/W, production vector fit and external protected/issuer/backend admission are unqualified. A runtime sample can witness its actual occupancy and bounds only. Any corruption or retained uncertainty remains refusal, not zero.

Rollback disables private G1 selection and reverts only G1-created hooks/config/workload/producer changes; preserve failed builds/captures. No public ABI, persistent bytes, real ledger or remote/production entitlement is introduced. Normal-image exclusion is verified independently from private runtime.

## Superseding actual runtime corrections and final proof

The original pre-capture text/digest referenced by historical capture receipts is preserved at `build/g1-pre-capture-review-original.md`; this continuation does not rewrite those receipts. Each later receipt's runtime_corrections field pins its actual producer/source changes separately.

Actual r3boot revealed missing private StateRestore allowance for the existing argv carrier and a clock receipt emitted before logger registration; corrected existing syscall permission and allocation-free boot formatting. r4/r5 revealed that160scheduler ticks are not a16HPET-period certificate and that sequential Wait can miss an already-reaped outcome. The private source now releases the exact root's two-byte indefinite receive at checked HPET period16, and releases each exact-generation worker's final receive only after the real root Wait waiter is installed. Existing mailbox/quiescent reaper deliver the real message and exit code; no fabricated return value or normal IPC semantics change.

r6SMP overlap had one genuine no-overlap observation. The private workload now has a three-byte source-owned compute barrier after both attested root releases; transient eligibility derives the two live same-generation workers from scheduler records and selects distinct admitted harts. TwoCPU shared STARTED rendezvous occurs only after both workers are admitted/released; oneCPU skips it. This is private experiment placement, not production affinity or throughput policy.

r7SMP yield exposed a real application allocator panic and malformed record. All G1records now use256byte stack formatting plus one Log syscall including newline; no concurrent worker heap formatting. This removes avoidable allocation/race rather than weakening the decoder or suppressing the panic. Normal library allocator behavior is outside this scope.

Final r8complete matrices:9/9oneCPU and9/9twoCPU accepted, including allthree actual distinct-hart overlap witnesses, with16periods, exact independent aggregate/period/bank/debt/retirement replay and source206event ceiling. Historical failures remain immutable. No H/S/D tolerances or production budgets were increased. Final tests9actual accounting module and107decoder pass; five mutations of real SMPraw bytes reject. Normal-image build excludes private tags. Full hosted-kernel suite compilation remains unsuccessful for the recorded cfg/out-of-scope type issues.

The payload remains41304bytes; current eligibility adds only a fixed transient worker-pair/mask, no persistent store/per-task allocation. The additional private notifications are existing inline mailbox messages outside ACCOUNTING ownership; source namespace checks and exact receive shapes prevent general grant/route authority. Production/physical/clock-liveness/WCET/D/W and downstream CP1/CP2 remain blocked.
