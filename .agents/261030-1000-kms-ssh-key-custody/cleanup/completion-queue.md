# Cleanup item 4 CP1 proposal: `[selftest] COMPLETION-QUEUE: FAIL` (documents only; nothing applied)

Written 2026-10-30 at HEAD `ca4b872e6361174af46b8984a0f02bcdbc84a76f`, tree clean when read. **Documents only.** No source, manifest, lock, script, census pin, image or commit action was made. No QEMU was run and no cargo build or test was run. The only commands run were read-only: `grep`, `git log/show`, `sha256sum`, and `python3 scripts/verify-cell-contract-p01.py` (no flags; it does not write unless `--refresh-census` is passed).

Context read: `reports/k8-cp2-dossier-part1.md` line 65 (the disclosure), `reports/k8-evidence/*` serial logs, `k8-cp1.md` (Law-1 and lock wording), `.agents/cell-contract-0024-locks.md`, `.agents/plan-portfolio.md`, `.agents/261011-1200-cell-contract-authority/` (N04 handoff r4, validation.md).

## 0. Summary

| Item | Answer |
|---|---|
| What fails | One row of the boot self-test: `waiter_cleanup_preserves_replacement` (`kernel/src/task/completion_selftest.rs:372-397`). Failing assertion: `completion_selftest.rs:387-389` (`q.clear_waiter(TID_A)` is expected to return `false`, now returns `true`). The other eight rows pass: the log carries exactly one reason line. |
| Test bug or real defect | **Test bug (stale expectation) plus stale doc comments.** The production behaviour the row was written to protect still holds, and the replacement semantics the row asserts were removed on purpose by the approved N04 multi-waiter change. Evidence in section 2. |
| When it started | Commit `4846c56f9` ("implement N02-N07 ...", 2026-10-16) added `waiters: Spinlock<Vec<usize>>` and changed `clear_waiter`. It changed `completion.rs` and **did not touch `completion_selftest.rs`**. The row was last edited before it. Last tracked boot log without the FAIL: 2026-10-09 (`docs/evidence/c2c-*-x86.log`, all `pass=1 fail=0`). Earliest tracked boot log with the FAIL: 2026-10-22 (`.agents/261020-1750-cellbox-cli/reports/s3-gui-rerun-evidence/p02/serial.log`). So "pre-existing" for K3-K8 and S10 is correct, but it is a regression of 2026-10-16, not an old defect. |
| Blast radius | Boot-only: `kernel/src/main.rs:1012-1016` logs `completion-queue self-test FAIL` and continues. Nothing gates on it. Every consumer of the real queue (`wait_completion`) ignores `clear_waiter`'s return value (section 2.3). |
| Minimal fix | Rewrite that one row to the multi-waiter contract (exact text in section 4). **Closure for the required fix: exactly one source file, `kernel/src/task/completion_selftest.rs`.** Optional extras (comment-only edits, a host test, docs) are separate decisions D-2 to D-4. |
| Locks / Law 1 | No lock row names the path. It sits in the P04 (wait/completion) replacement area. It is **not** Law 1 (`libs/api`, `libs/types` untouched). Needs a named baseline-repair slice and handoff (section 5). |
| Side effect to accept | The edit changes the pinned bytes of the file in the P01 census, so the (already red) document gate gains 2 drift entries for this path (section 5.3). |
| Verification | One x86_64 boot expecting `COMPLETION-QUEUE: PASS`, plus kernel check builds. Section 6. |

## 1. Findings (with evidence)

### 1.1 The failing line and where it is printed

- `completion_selftest.rs:81-84` `fail()` prints `[selftest] COMPLETION-QUEUE: FAIL — <reason>`. `completion_selftest.rs:492-513` `self_test()` ANDs nine rows with `&` (no short-circuit, so every row runs), then prints the bare `COMPLETION-QUEUE: FAIL` at `:510`. `kernel/src/main.rs:1012-1016` prints `completion-queue self-test FAIL`.
- Boot logs: `reports/k8-evidence/rt0/serial.log:109-111` shows the reason line, the bare line and `completion-queue self-test FAIL`. Across `k8-evidence`, `grep` finds 54 + 5 copies of the reason `an older waiter erased its replacement`, and no other COMPLETION-QUEUE reason. Untracked ignored `qemu-x86_64.log` (2026-10-19) has the same two lines at `:119-120`.
- The self-test is compiled for `riscv64`, `aarch64` and `x86_64` (`main.rs:974-979`), so it fails on every such boot, not only the x86_64 lane.

### 1.2 Why the row fails (code)

The row (`completion_selftest.rs:372-397`): insert tasks A and B in one cell, get the shared queue, then

```
q.register_waiter(TID_A);
q.register_waiter(TID_B);
if q.clear_waiter(TID_A) { fail("an older waiter erased its replacement") }   // expects false
if !q.clear_waiter(TID_B) { fail("the current waiter could not clear its registration") }
```

Current implementation (`completion.rs:347-368`):

```
register_waiter(tid): self.waiter.store(tid); push tid on `waiters` list if absent and len < MAX_WAITERS (32)
clear_waiter(tid):    CAS(self.waiter: tid -> 0) (result ignored);
                      remove tid from `waiters`; return true iff it was in the list
```

Trace: after the two registers `waiter = B`, list = `[A, B]`. `clear_waiter(A)`: the CAS fails (waiter is B) and is ignored; A is in the list, so it is removed and the function returns **`true`**. The row expects `false`, so it reports FAIL. `clear_waiter(B)` then returns `true`, so the second assertion passes. This matches the single reason in the logs.

Before `4846c56f9`, `clear_waiter` was only the CAS (`git show 4846c56f9 -- kernel/src/task/completion.rs`, minus lines at `-345..-352`): `compare_exchange(tid, 0, ...).is_ok()`. Then A's clear failed (false) and B's succeeded, and the row passed. [Read from the diff; the old kernel was not rebuilt or run.]

### 1.3 The old property still holds, structurally

The row's intent: *one waiter unwinding must not erase another waiter's wake registration.* With the list, `clear_waiter(A)` removes only A; B stays in the list, and `waiter` keeps pointing at B because A's CAS fails. `drain_wake_requests` (`completion.rs:382-396`) wakes the list members plus the `waiter` slot, so after A clears, a completion names exactly B. I checked these interleavings by reading the code: A,B then clear A / clear B / re-register A; B cleared first (waiter becomes 0, A stays in the list). No lost registration. The only non-covered case is the list being full (32 waiters), where the extra waiter is held only by the single `waiter` slot; see 1.5(b).

So the contract changed from "`false` when another waiter replaced you" to "`true` iff you were registered", and the invariant moved from a single-slot CAS to list membership. The row is checking the old mechanism.

### 1.4 Intent of the change: it is deliberate

`4846c56f9`'s completion hunk is the N04 "multi-waiter completion" work: new `MAX_WAITERS`, the list, `drain_wake_requests`, and `deliver_pending_wakes` switched to it (`completion.rs:482-487`). Handoff r4 (`.agents/261011-1200-cell-contract-authority/native-development-handoff-r4.md`, SHA-256 `43388ef5...3a1`, approved in `native-development-handoff-decisions.jsonl` line 4) names `completion.rs` and `completion_wait.rs` as allowed paths and "bounded multi-waiter registration" as an invariant. Its allowed-path list does **not** include `completion_selftest.rs`, which is why the stale row was never updated. Its verification list is host harness, kernel compile, one VMware boot, and the P01 document check; none of them compares the boot self-test line, and `validation.md` records no COMPLETION-QUEUE observation for N04.

### 1.5 Adjacent observations (NOT part of the minimal fix; [INFERENCE] unless noted)

a. **Stale docs, verified by reading.** `completion.rs:355-358` ("if it still owns the registration. Returns `false` when another waiter replaced `tid`") describes the removed behaviour. `completion.rs:112-119` has two overlapping field docs for `waiters` / `waiter`, the first of them garbled. `completion_wait.rs:26-27` ("without erasing a newer waiter that may have replaced it") and `completion_selftest.rs:370-371` carry the same stale wording.
b. **Capacity.** `register_waiter` silently drops a waiter when the list holds 32 (`completion.rs:350`); the extra waiter is then reachable only through the single `waiter` slot, which the next registration overwrites. Whether a cell can have more than 32 concurrent waiters depends on the per-cell thread cap, which I did not check. Not exercised by any test.
c. **Wake-all.** `drain_wake_requests` wakes every registered waiter of the cell on any completion, not only the waiter whose slot completed. Handoff r4 invariant 3 says "no foreign wake or wildcard drain". `wait_completion` tolerates spurious wakes (`completion_wait.rs:270-296`: TIMER loops again; NET_RX returns the documented raw-zero), and `wait_completion` uses `queue.drain()` with no per-slot filter. This is a design question for P04, not a self-test fault; I propose no change.
d. **Dead code.** `take_wake_request` (`completion.rs:371-379`) was made `pub` by `4846c56f9` and has no caller in `kernel/`, `scripts/` (grep). Left alone.
e. Other self-tests the K8 logs show as FAIL-looking (NET-RX etc.) are not this item. `rt0/serial.log:113` shows NET-RX-RESERVATION PASS.

## 2. Root-cause classification

| Question | Verdict | Basis |
|---|---|---|
| Does production code depend on `clear_waiter`'s return? | No | all call sites discard it: `completion_wait.rs:55`, `:63`; `task.rs:1402`; `completion_selftest.rs:199`, `:366`, `:430` use `let _ =` |
| Is a waiter lost or a foreign waiter erased? | No (list-wise) | section 1.3 |
| Is the row's expectation reachable under the intended design? | No | `register_waiter` of a different tid no longer replaces anything |
| Real defect elsewhere found? | No | 1.5 items are design/docs/capacity notes only |

Therefore: **fix the test, and optionally the doc comments. Do not change `completion.rs` behaviour, and do not revert the multi-waiter design** (it fixes the lost-wake for sibling threads that a single waiter slot could not).

### 2.3 Call-site evidence (grep `clear_waiter|register_waiter`)

`completion_wait.rs:41,55,63`, `task.rs:1402`, and in the self-test `:165,:199,:307,:366,:384-385,:387,:390,:410,:430`. NET_RX self-test, IPC-pending and IPC-guardrail self-tests reuse only `insert/queue/remove` (`net_rx_selftest.rs:19`, `ipc_pending_selftest.rs:8`, `ipc_guardrail_selftest.rs:8`), which this fix does not change.

## 3. Options

| Option | Effect | Verdict |
|---|---|---|
| A. Rewrite the row to the multi-waiter contract (below) | Test again protects the real invariant: siblings survive each other's cleanup, and a completion wakes exactly the remaining waiters | **Recommended** |
| B. Delete the row | Green, loses coverage of the one concurrency property this queue exists for | Reject |
| C. Make `clear_waiter` return `false` when "replaced" (re-add single-owner bookkeeping) | Behaviour change in N04-owned code to satisfy a stale test | Reject |
| D. Revert multi-waiter (`4846c56f9` hunk) | Re-opens the lost-wake for sibling threads; contradicts the approved N04 | Reject |
| E. Leave as known defect | Boot logs keep a FAIL that masks any future real regression of this module | Reject (the user accepted it only as a stop-gap, D3-5) |

## 4. Exact minimal fix (option A)

File: `kernel/src/task/completion_selftest.rs` (current SHA-256 `abdce49cdc0117c6e6a9a0b527c9df706fa9e8b8b4fbbde1ed796b90d50addf5`). Three hunks.

**Hunk 1** (add a private helper above the row, after `fail()` at `:84` or just before `:369`):

```rust
/// Append one completion and report which tasks its wake request names.
///
/// `drain_wake_requests` consumes the request, so the caller must hold
/// `SCHEDULER` for the whole call: with interrupts off, no tick can run the real
/// deferred wake and consume the request first.
fn wake_targets(q: &CompletionQueue) -> Option<alloc::vec::Vec<usize>> {
    let slot = q.reserve()?;
    if !q.complete(slot, 1) {
        return None;
    }
    let mut out = alloc::vec::Vec::new();
    q.drain_wake_requests(&mut out);
    let _ = q.drain();
    Some(out)
}
```

**Hunk 2** replace lines `370-397` (doc comment and the whole row) with:

```rust
/// Cleanup is registration-exact: one waiter unwinding must not erase another
/// waiter on the cell's shared queue, and a completion wakes exactly the
/// waiters that remain.
fn waiter_cleanup_preserves_siblings() -> bool {
    insert(TID_A, CELL_ONE);
    insert(TID_B, CELL_ONE);
    let q = match queue(TID_A) {
        Some(q) => q,
        None => {
            remove(TID_A);
            remove(TID_B);
            return fail("no queue could be reached from the task record");
        }
    };

    q.register_waiter(TID_A);
    q.register_waiter(TID_B);
    let mut ok = true;
    {
        let mut guard = super::SCHEDULER.lock();
        match guard.as_mut() {
            Some(sched) => {
                // Start from a lowered scan flag and no outstanding request.
                completion::deliver_pending_wakes(sched);
                if !q.clear_waiter(TID_A) {
                    ok = fail("a registered waiter could not clear its own registration");
                }
                match wake_targets(&q) {
                    Some(targets) if targets == [TID_B] => {}
                    _ => ok = fail("clearing one waiter changed who a completion wakes"),
                }
                if q.clear_waiter(TID_A) {
                    ok = fail("a cleared waiter was cleared a second time");
                }
                if !q.clear_waiter(TID_B) {
                    ok = fail("the remaining waiter could not clear its registration");
                }
                match wake_targets(&q) {
                    Some(targets) if targets.is_empty() => {}
                    _ => ok = fail("a completion still named a cleared waiter"),
                }
                // The appends raised the global scan flag; lower it again.
                completion::deliver_pending_wakes(sched);
            }
            None => ok = fail("scheduler absent"),
        }
    }

    remove(TID_A);
    remove(TID_B);
    ok
}
```

**Hunk 3** `self_test()` at `:499`: `& waiter_cleanup_preserves_replacement()` becomes `& waiter_cleanup_preserves_siblings()`.

Why this is correct, from the code read:
- With register A, register B, clear A: the list is `[B]` and `waiter == B` (A's CAS fails). `drain_wake_requests` returns `[B]` (list entry, then the `waiter` slot deduped by `out.contains`). The first `wake_targets` is `[B]`.
- `clear_waiter(A)` again: A is not in the list, so `false`. `clear_waiter(B)`: CAS to 0 and list removal, so `true`.
- Second append: `wake_requested` is set, the list is empty and `waiter == 0`, so `drain_wake_requests` returns nothing. `wake_targets` returns `Some(vec![])`.
- `reserve/complete/drain` take only the ring lock; `drain_wake_requests` and `clear_waiter` take only `waiters`; `deliver_pending_wakes` needs `SCHEDULER` held (it is). The `SCHEDULER` guard is the same pattern as `deferred_wake_reaches_scheduler` (`:313-364`). `fail()` while the guard is held is also done by that row.
- No ready queue is touched and no `TaskState` is changed, so nothing synthetic can become runnable. After the row both tasks are removed exactly as the old row did; the Arc queue is owned only by TID_A's removed task record.
- `Vec<usize> == [usize; 1]` and `is_empty` need no new imports (`alloc::vec::Vec` is written in full, as the file already does at `:42`). `CompletionQueue` is already imported (`:23`); `drain_wake_requests` is `pub` (`completion.rs:382`).

These snippets were written against the code read and have **not been compiled**; the first compile is part of verification.

### 4.1 Optional extras (separate decisions, each widens the closure)

- **D-2 host test** in `completion.rs` (new `#[cfg(test)] mod tests`, using the private `CompletionQueue::new(CellId(..))`, `completion.rs:128`). CI already runs `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` (`.github/workflows/ci.yml:1673`), which is the only automated gate for this logic. It would run the same four assertions (`register A,B; clear A true; wake targets == [B]; clear A false; clear B true; wake targets empty`) without a boot. The module has no `#[cfg(test)]` today (grep). Not compiled here; `WAKES_PENDING` is a process-global atomic, and the test must not assert on it.
- **D-3 comment-only truth fixes**: `completion.rs:112-119` (merge the two field docs), `:355-358` (`Returns true iff tid was registered and is now removed; another waiter's registration is untouched`), `completion_wait.rs:26-27`, `completion_selftest.rs` doc of the row (already replaced by Hunk 2). Comments only; no behaviour; but they change the pinned bytes of two more N04-owned files.
- **D-4 docs**: `CHANGELOG.md` `### Fixed` (`:434`) entry; `docs/cellbox-cli.md:12` drops `the COMPLETION-QUEUE: FAIL self-test line` from the "Known defects" list (only after the boot proof). Historic S10 records that call it "accepted non-blocking" are history and are not edited.

## 5. Exact file closure, owners, locks

### 5.1 Closure

| Path | Action | Required | Current SHA-256 |
|---|---|---|---|
| `kernel/src/task/completion_selftest.rs` | edit (section 4, three hunks) | **yes** | `abdce49cdc0117c6e6a9a0b527c9df706fa9e8b8b4fbbde1ed796b90d50addf5` |
| `kernel/src/task/completion.rs` | D-2 host test and/or D-3 comments | no | `79b07aef24170a82c40555f7ff7ac750ab08812b809c8ee80f3518c8d87d6af1` |
| `kernel/src/task/completion_wait.rs` | D-3 one doc comment | no | `32cb615aae106a9d21054973e5f29e7d284c79e5d95f598ba0a831a37527a383` |
| `CHANGELOG.md` | D-4 entry | no | `5c2022ac045b81cae884e8204994d0ffd4f2b116633d4f1181d89ccb87dceeb1` |
| `docs/cellbox-cli.md` | D-4 one list item | no | `28ac3c3ab9834e04c207a53cdcd2ce9ad94e600da9e741b490616c16a6e3057f` |

Read-only, **must stay byte-identical**: `kernel/src/main.rs` `a9636fee1f3b63b89a1058c5053d9cf446b84aede4cb10808362823539408c64` (the user's file, pinned in `phase-10-qualification.md:162`; the call site `:1012-1016` needs no edit), `kernel/src/task.rs` `a27bc5d110f03cb3db428d6013ea3c9ac3a1bf0426cc099283944a3ba06d512d`.

Not in the K-program closure either: no K2-K8 file is touched.

### 5.2 Owners

- Authors: all 5 commits of `completion_selftest.rs` and the multi-waiter change are by one author (`git log`/`shortlog`). No current owner other than the controlling P01 owner for the P04 area.
- The kernel surface is governed by CELL-CONTRACT-0024 (`.agents/cell-contract-0024-locks.md`). **No row names `kernel/src/task/completion*.rs`** (grep on the locks file finds no path entry; rows are scoped by plan, not by path). It falls in replacement area **P04** ("Correlated operations, wait/completion", locks `:99`). The locked plans whose P04-area scope could cover it: `260605-1538-milestone-2-1-vfs-complete`, `260607-1200-phase-27-protocol-hardening`, `260615-1325-vicell-app-sdk-v1`, `260727-2101-midori-lessons-cellos`, `260801-midori-phase01-completion` (`:146`), `260801-parallel-midori-closure` (`:147`, "completion/peer-death"), `260825-sdk-delivery`, `260927-1100-c2c-anywhere-tier-aware`, `260806-1026`-style history is "no new lock", and `261006-0934-httpd-single-cell-concurrency` (`:164`, "completion-capacity"). This is a lexical reading, so **[INFERENCE]: not exhaustive**.
- The only owner of the paths' current contract is the controlling plan `261011-1200-cell-contract-authority` (status complete). Its N04 exception (r4) listed `completion.rs` and `completion_wait.rs` but not the self-test, and a completed tranche unlocks nothing (locks `:310`, step 6).
- `plan-portfolio.md`: matches for completion are the P04 row (`:39`) and the note that kernel `syscall/completion/scheduler` paths need a file-owner handoff (`:352`); no row owns the self-test (grep; [INFERENCE] not exhaustive).
- **Law 1**: not touched. Law 1 files in this program are `libs/api` and `libs/types` (`plan.md:47`, `k8-cp1.md` section 0). The ABI constants `api::completion::source::*` are only read by other rows, not by this one. No confirmation text is needed.

### 5.3 Route and pinned-document consequences

- Route (locks `:310`): "Urgent in-scope baseline repairs are scheduled as named, contract-preserving slices; not blanket exceptions". This fix is contract-preserving by construction: no ABI, no queue behaviour change. It needs a short lock-handoff note (same shape as `lock-handoff-k2-cp2.md`): one slice, file-bounded exception for the section 5.1 required row, "no overlapping owner" confirmation, P04 as the area, and the explicit statement that it re-expresses the N04-approved semantics in the boot test. The accountable owner (the user) must approve it; I do not assume it.
- `scripts/verify-cell-contract-p01.py` (run read-only now) is **already red at HEAD**: 349 `PACKAGE_SOURCE_DRIFT`, 57 `CENSUS_SOURCE_DRIFT`, 3 `DOCUMENT_LINK`; none of them is for `completion*.rs`, `main.rs` is drifted (it is the user's file) and `task.rs` is drifted. `completion_selftest.rs` is currently pinned correctly in `exact-deltas/manifest.json:11176-11177` and `consumer-census-source.json` (both name the same digest). After the edit the gate gains 1 `PACKAGE_SOURCE_DRIFT` and 1 `CENSUS_SOURCE_DRIFT` for this path (+1 more of each per optional file). Pins are not refreshed by this item: refreshing would hide 400+ unrelated drifts and is a P01-owner action (`--refresh-census`). Pin file digests now: `exact-deltas/manifest.json` `b269aeb1f444211d35c1fa9ec0b25185b5e179c166df1897fab511824e482d93`, `consumer-census-source.json` `917d6cfd55c0b72375d24301a2d07eed949b980b8801978b50a38362dac76772`.
- Kernel image digests change (the self-test is linked into the kernel). Any retained pinned candidate kernel digest (S10 qualified candidate, K8 private images) stays valid only as history; new images are needed for anything that must show PASS.
- Scripts that mention the marker: `scripts/phase10_canonical_drive.py:465` only prints COMPLETION-QUEUE lines as a "known non-blocking (D3-5)" note, never gates. It keeps working (it would print the PASS line); no edit required. SHA-256 `d4fcb022dc340f6d79451fa02ae3b38e0ca90f809f0c943ba48abfbeb835fad9`.
- Other places that pin these digests (grep by SHA-256 over `.agents`, `scripts`, `docs`; [INFERENCE] other trees not searched): `completion_selftest.rs` is pinned only in `exact-deltas/manifest.json` and `consumer-census-source.json`. `completion.rs` (`79b07aef...`) and `completion_wait.rs` (`32cb615a...`) are additionally pinned in `native-development-handoff-r4.json` (and `-r3.json` for `completion.rs`) and `completion_wait.rs` in `pinned-x86-provenance.json` / `service-x86-provenance.json`: these are historical packet/provenance records, so the optional D-2/D-3 edits would make them stale by design. `resource-attribution-contract.md` already pins a different, older `completion.rs` digest (`d02521d4...`). This is one more reason the required fix is the self-test alone.

## 6. Tests and verification (main agent runs, once, after the edit)

1. Compile gates (from `validation.md` N04 section): `cargo check -p cellos-kernel --target x86_64-unknown-none` default and `--features test-hooks`; if the self-test is built for RV64 and AArch64 in the normal lanes, the existing check of those targets too (the row is arch-independent, but it is `#[cfg]`-free code, so a typo shows on all).
2. If D-2 is accepted: `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` (CI command, `ci.yml:1673`). Expected: the new tests pass, the existing kernel host tests (reported as 277 by the S9 record) unchanged.
3. **One** x86_64 boot of any existing serial lane (the same lane family that produced `qemu-x86_64.log` / the S10 L5 and P02 kernel logs) with `grep -c` on the serial log:
   - must contain `[selftest] COMPLETION-QUEUE: PASS (cap 32 slots, ` and `completion-queue self-test PASS (reserve, land, bound, defer)`;
   - must contain no `COMPLETION-QUEUE: FAIL`;
   - the following self-tests, which reuse the same scaffolding, must be unchanged: `NET-RX-RESERVATION: PASS`, `IPC-PENDING`, `IPC-GUARDRAILS`, `THREAD-QUOTA: PASS`.
4. Negative control (host or build only, throwaway, not committed): temporarily make `clear_waiter` not remove from the list; with D-2 the host test must fail, and in a boot the new row must print `a completion still named a cleared waiter` or `clearing one waiter changed who a completion wakes`. Without a control the new row's assertions are unproven to detect the regression they target. (Revert immediately; never commit.)
5. Document gate: re-run `python3 scripts/verify-cell-contract-p01.py` and confirm the error delta is exactly the +1 `PACKAGE_SOURCE_DRIFT` and +1 `CENSUS_SOURCE_DRIFT` for `completion_selftest.rs` (and no new class). Do not run `--refresh-census`.
6. `git status --short` shows only the section 5.1 required path (plus the optional rows that were accepted) and the handoff note.

## 7. Risks

| Risk | Mitigation |
|---|---|
| The new row compiles but is wrong (I did not compile it) | compile gate; it is a straight composition of the APIs the other rows already use |
| `fail()` (logger) called while `SCHEDULER` is held | `deferred_wake_reaches_scheduler` does the same (`:329-362`) |
| A tick consumes the wake request before the row reads it | the guard is held across both appends and both reads (same argument as `:309-311`); single-hart window before `smp::start_secondaries()` |
| Leaking a raised `WAKES_PENDING` into the next row | final `deliver_pending_wakes(sched)`; `withdrawal_raises_no_wake` begins and ends the same way |
| The fix hides a real multi-waiter defect (1.5 b, c) | stated in 1.5; the new row pins the current contract only, and wake-all is not asserted as desirable |
| Another hidden failing row appears once this one is green | the nine rows are ANDed without short circuit, and the logs show only this reason, so none is currently failing |
| Document gate grows by 2 drifts | disclosed (5.3); the gate is already red for unrelated reasons |
| Image digests move | only if someone needs PASS evidence; no retained candidate is rewritten |

## 8. Decision table

| ID | Decision | Options | Recommended |
|---|---|---|---|
| D-1 | How to resolve the FAIL | **A** rewrite the row (section 4); B delete the row; C add single-owner bookkeeping; D revert multi-waiter; E leave | **A** |
| D-2 | Host test in `completion.rs` for the same property | yes / no | yes (CI gate for the module; widens closure by 1 file; changes the pinned bytes of `completion.rs`) |
| D-3 | Comment-only truth fixes in `completion.rs:112-119,355-358` and `completion_wait.rs:26-27` | yes / no | yes, bundled with D-2 so `completion.rs` is touched once |
| D-4 | `CHANGELOG.md` entry and `docs/cellbox-cli.md:12` edit, after boot proof | yes / no | yes (permanent-fix rule), edit the doc line only after the PASS boot |
| D-5 | Route: named baseline-repair slice with a lock-handoff note, owner approval before any edit | yes / bundle with N04 follow-up / no | yes, handoff note first (documents only), then CP2 |
| D-6 | Refresh the P01 census/manifest pins for this path | refresh / leave drift disclosed | leave drift disclosed (refresh is the P01 owner's job and would also bless 400+ unrelated drifts) |
| D-7 | Separate follow-up for 1.5(b) capacity and 1.5(c) wake-all | open P04 item / ignore | open as an item for the P04 owner; not part of this fix |

## 9. Nothing applied

No file outside this document was created or edited. The directory `.agents/261030-1000-kms-ssh-key-custody/cleanup/` was created for this document only.
