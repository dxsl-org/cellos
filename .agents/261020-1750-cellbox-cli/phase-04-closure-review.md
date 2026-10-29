# Phase 04 closure review — criteria 1, 2, 4 and remaining CP2s

Status: **D-CL1 and D-CL2 accepted (user “tiếp tục”, 2026-10-22): criteria 1 and 2 checked; S4a/C5 CP2 dossiers in preparation. D-CL3 pending.** No source or image change. Inputs: [Phase 04](phase-04-multicall-cutover.md), [S4 results](phase-04-s4-retirement-proposal.md), [C5 results](phase-04-c5-desktop-parity-proposal.md), saved logs `reports/{s4-final-evidence,c5-evidence}/`.

## 1. Criterion-by-criterion evidence
| # | Criterion | Evidence | Proposed state |
|---|---|---|---|
| 1 | Every catalog entry/caller migrated or explicitly retained; no silent loss | `cells/tools/cellbox/src/ledger.rs`: 73 entries (56 builtins + 11 sys-tools + 6 net-tools), one disposition each; host tests diff the registry against the ledger and byte-check catalog §3.1 (`app-cellbox` 89/89). S4 census: old shell crate gone, no `/bin/shell` caller in kernel/init/launch profiles, stale recipes and docs fixed, `applets` removed. Desktop callers reconciled by C5 | **Check** |
| 2 | Shell state and external byte pipelines correct under the new identity/path | Boot identity is `/bin/cellbox ["sh"]`. Phase02 session runner `passes: 48`; Phase03 pipeline runner 60/60 (exit codes, last-stage status, consumer exit, byte-exact pass-through, Ctrl-C, typed→external); Phase04 terminal runner 354/354 (state, redirects, pipelines, endless sink, rights); GUI 173/173 | **Check** (private QEMU/TCG only) |
| 4 | One suite executable packaged; no fallback to an obsolete artifact; independent apps/services still work | Private images package one `/bin/cellbox` (no `/bin/shell`; `gen-disk-ci.sh` now asserts `cellbox`). Independent workers (curl, httpd, stock tools, Files, Terminal) run in the runners. **Canonical packaging is blocked by rollout containment** (`reports/rollout-containment.md`); canonical images still carry the quarantined cellbox bytes | **Leave unchecked**: "packaged" is only true for private fixtures. Closing it needs the release conditions, which this phase cannot meet (Phases 05–10 incomplete) |

## 2. CP2 still owed
Phase 04's final cutover needs CP2 for every delta implemented since the last approval:
- **S4a** (applets deletion, recipe/doc fixes, `cat` exit status, driver check): covered by the S4 CP1 only.
- **C5** (Files on VFS + `ostd` VfsClient additions + Files ceiling +`VfsMutate`; desktop removals; runner checks).

Proposal: prepare two CP2 dossiers in the established format, using hashing only and evidence from the saved logs, including the repeated GUI runs below.

## 3. GUI stability
S3 practice was repeated GUI runs (6/6) before calling the GUI stable. C5 has one click run (173/173). Four more click runs and one autoload run then all exited 0 with **173/173** each, so **6/6 runs** pass in total (logs `reports/c5-evidence/gui*.log`).

## 4. What Phase 04 status becomes
With 1, 2, 3, 5, 6 checked and 4 open, Phase 04 stays `in_progress`, blocked only on criterion 4, i.e. on rollout containment. This is the plan's intended end state: packaging and release wait for Phases 05–10 and their CP2s. The next work then moves to Phase 05 (Ratatui terminal), which depends on 02 and 04 and does not need criterion 4 closed. **Decision needed:** whether Phase 05 may start with Phase 04 in this state. The plan DAG lists 04 as a dependency of 05.

## 5. Decisions requested
- **D-CL1:** check criteria 1 and 2 as above (recommended), or keep them open.
- **D-CL2:** prepare the S4a and C5 CP2 dossiers (recommended).
- **D-CL3:** allow Phase 05 entry with Phase 04 = in_progress (criterion 4 waiting on containment) (recommended), or hold.

## 6. Update 2026-10-29 (S10-5, CP2 granted by the user)
Criterion 4 is now checked: the qualified dev-posture desktop bytes were promoted to the canonical x86 paths and verified (hash-identical to candidate `4fae8b20…`, V1–V7 PASS, [S10-5.13](phase-10-s0-proposal.md)); Phase 04 is `completed-private-evidence`. Section 1 row 4 above is the 2026-10-22 state. Ceiling: x86 dev posture, QEMU/TCG only; non-Intel recipes not covered; CBX-27 and CBX-23/24/25 are separate and unchanged.
