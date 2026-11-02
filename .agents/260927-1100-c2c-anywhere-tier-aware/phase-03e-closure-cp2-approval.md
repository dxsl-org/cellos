# C2C Phase 03 slice e CP2 approval record (closure; Phase 03 done)

Recorded 2026-11-02. The accountable maintainer/owner (the user) answered **YES**, through the integrating session, to the question of `reports/phase-03e-cp2-package.md` §8:

> Do you accept the slice e delta (§3), its evidence (§4), the review resolutions (§7) and deviations E1–E12, and, per E-7, mark C2C Phase 03 done at the x86 QEMU/TCG ceiling with the named exclusions (registry restart + re-lookup in Phase 02 step 3, broker `NetworkPoller` cutover, x86 NET_RX producer, physical Intel)? On YES the file-bounded exception ends; nothing is committed by this record.

Recorded values:

| ID | Decision |
|---|---|
| C2-1 | The slice e delta is accepted: receive commits by delivery id in the four syscall receive arms (with a debug guard), `PendingCall` drop releases an unsettled operation, and operation submits keep 8 of 64 receiver-queue slots for Cell incarnations holding no queued wire there. **No Law-1 delta**: no syscall, flag, allowlist bit, completion source, record or `libs/api` byte changed. |
| C2-2 | Evidence accepted at its stated ceiling: host tests plus x86 QEMU/TCG; the receive-commit fix (A1) and the admission bounds (A7) are host-proven; the two-CPU lanes are unforced; `igb-x86` 2/2 ran on the round-1 tree. |
| C2-3 | The independent review (PASS_WITH_RISK) resolutions and deviations E1–E12 are accepted as recorded, including E12: the pre-existing settled-on-`InvalidOperation` leak stays unchanged. |
| C2-4 | Per E-7, **C2C Phase 03 is DONE** at the x86 QEMU/TCG ceiling. Excluded, not claimed: registry restart + re-lookup (C2C Phase 02 step 3), the broker `NetworkPoller` cutover (slice d D-5), an x86 NET_RX producer, physical Intel qualification. |
| C2-5 | The `CHANGELOG.md` entries (E-8) stand as written. |

**Rulings recorded with the approval (integrating session instruction, same day):**
- No commit is made. The slice stays uncommitted for the integrator.
- Status is marked done in the phase file, `plan.md`, `plan-portfolio.md`, `cell-contract-0024-locks.md` and `.agents/TODO.md`; the post-edit digests are bound below.

**Authorizes:**
- the delta and records listed in CP2 package §3, at the digests bound below;
- the Phase 03 `done` status at the stated ceiling.

**Does not authorize:**
- any further edit under the slice e file-bounded exception, which ends with this record, including any `kernel/src/task/syscall.rs` line;
- a public ABI change, provider cancel-ack, raw-mailbox admission changes, registry restart work, the broker cutover, an x86 NET_RX producer;
- remote/relay/pipelining, physical or production claims, canonical image or signing-policy changes;
- release of any whole-plan lock relation (all 38 stay), or unlocking any part of C2C Phases 04-10.

**Verification at binding (2026-11-02, HEAD plus the uncommitted delta):**
- `cargo fmt --all --check`: clean.
- `bash scripts/check-multi-source-wait-law1-digests.sh`: PASS.
- `bash scripts/check-lookupservicebound-law1-digests.sh`: PASS.
- Recorded in CP2 package §1 and §4 (review round 2, unchanged source since): host `-p api` 184 + 2, `-p ostd` 87 + 7 + 19, `-p cellos-kernel` 400; `local-service-lifecycle-x86` 3/3 (ISO `395b1b1f426f5526…`); independent serial sessions local + async on `-smp 1` and `-smp 2` PASS with 0 `denied for tid`; clippy `-D warnings` rc=0 on the three CI lanes.

**Source HEAD at binding:** `5638153118921e53c510571ba5dc50c26d73b0a7`. The diff from HEAD is uncommitted:
- source/test: 9 files under `kernel`, `libs/ostd`, `cells/tests/bench`, `tests/integration`;
- documents: Spec 20, `CHANGELOG.md` and the `.agents` records;
- untracked: the slice e documents, `reports/phase-03e-cp2-package.md`, `docs/evidence/c2c-phase03-closure-x86.txt` (the `.log` is git-ignored), and the pre-existing `.agents/archive/`.

**Digest form:** SHA-256 of whole files including the final newline, with no normalization; no digest is stored inside the file it hashes.

## Bound documents and source
| Path | SHA-256 |
|---|---|
| `.agents/260927-1100-c2c-anywhere-tier-aware/reports/phase-03e-cp2-package.md` | `8556b821ba2bad618db1c39efef1cb62a9102573fdaefb9ee2370ad8f7667ffe` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03e-closure-cp1.md` | `195e7763f4f0b383738a4196b6beb98ee8c07575e4daaf8e109e46d669db9463` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03e-closure-cp1-approval.md` | `3460ecf0418db36a7ed8208af5c1172c218e54bc92ab5864e3abc47652ea3f6f` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/phase-03-async-ipc.md` | `7f7a6ede7918a062a064a1560960090a73165e9873f63edd90bf6f2c3d4ad25a` |
| `.agents/260927-1100-c2c-anywhere-tier-aware/plan.md` | `ae12144802500c12445352cb1f63ccf41501752030f4680d5de19b30cd56ff2a` |
| `.agents/cell-contract-0024-locks.md` | `7d3efe5992f669f1f0368137fc3e675416a3fc42b214160a755a22f2e3a1e667` |
| `.agents/plan-portfolio.md` | `e9d6dae8b1731938281fd6d6f26ec64c0fa613ab5ad43a614331bc096a6d939e` |
| `.agents/TODO.md` | `87df658db09d5cf7ced55e173d17f111c16d2dc6ed964aa7d10981a907a7d2e9` |
| `docs/specs/20-unified-ipc-contract.md` | `eb564f79ff485c6d5d6590247f9014ab5f19979f8c4ef8a740b45fabc5dcbf4a` |
| `CHANGELOG.md` | `e281edbe37344ca9555693b5680e4876101c88d21760977d68178076f4309af5` |
| `docs/evidence/c2c-phase03-closure-x86.txt` | `91006a1b215c1fd3100008f740256e770d7c6eb6700b0dfadc0c2de26973bf5e` |
| `docs/evidence/c2c-phase03-closure-x86.log` | `f6c989ca910da110e76f3411ff6194afe9da734c8fa749b1c5c2f205a15e8cdd` |
| `kernel/src/task/async_ipc.rs` | `21ea90d9fedbf28ecc47b18f27ea510b122cfcc7e99654d374802174e2da1aa0` |
| `kernel/src/task/pending_mailbox.rs` | `5ab01cc30ec4488d111db1edbabcda4b4d8dbfdaa1bdf92d0c4090cfefafe91e` |
| `kernel/src/task/syscall.rs` | `0eb2b9e50fb3d888191f3c39d126a08b660abe98d0f6ec09971d3772c0b17d68` |
| `libs/ostd/src/ipc.rs` | `8feb88b6b56b96f237d0f8e8efd2350c6041dec90cdd152179afe696934e08da` |
| `cells/tests/bench/src/bench-probe.rs` | `b5759f977a091987bc2ab489bdb6f95173b6eef6d97290fa5fa36d2a116d128b` |
| `cells/tests/bench/src/scenarios/async_lifecycle.rs` | `dfefec4d4430d4b4f84fa094e281e44606adee22d663945a47ff9e962f19b7bf` |
| `cells/tests/bench/src/scenarios/local_service_lifecycle.rs` | `fbfd78cf67f53c5f8beda17916f11e4ff84409be006d30d7967ea03c298d125f` |
| `tests/integration/tests/local-service-lifecycle-x86.rs` | `6647fd1a6283a337cd9e0539998794094464264a591e9122cbf7fa0995ad672c` |
