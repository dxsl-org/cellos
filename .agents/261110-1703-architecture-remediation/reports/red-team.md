# Red team — architecture remediation plan (self-run, 2026-11-10)

Self-run by the planning session; not an independent review. Lenses: governance conflict, ABI, sequencing, rollback, evidence honesty. Each finding lists what changed in the plan.

| # | Finding | Severity | Applied |
|---|---|---|---|
| 1 | Almost every phase edits files under `CELL-CONTRACT-0024` (`kernel/src/task/`, `cell/`, loader, `libs/ostd`). A plan that just starts would repeat the bypass the lock exists to stop. | Blocking | Phase 00 is admission-only; each slice needs its own handoff request with an exact file set; `plan.md` states nothing is executable before approval. |
| 2 | Phases 08, 11, 12, 13 (and possibly 06) change `libs/api`/`libs/types`. Law 1 needs CP1 before edits and CP2 after evidence. | Blocking | `Law-1` column in `plan.md`; each of those phases has CP1 as step 1 and CP2 in success criteria; byte-for-byte equality tests where the change is a pure move. |
| 3 | C-1's exploit chain is inferred. Fixing an unproven Critical first could spend the lane on the wrong edge. | Major | Phase 06 step 1 is a failing negative test on the current tree. If it cannot be made to fail, the finding is downgraded and logged; the identity-keyed table still lands (H-2 needs it). |
| 4 | Splitting `syscall.rs` (Phase 08) collides with every authority fix and stales every cited line. | Major | Authority slices S2/S3 run first on the monolith; Phase 08 runs only when no other kernel slice is open; it is move-only with numbers frozen. |
| 5 | Untracking `kernel/src/embedded-x86_64/kernel_fs.img` breaks lanes that rely on the committed, manifest-pinned image (`.agents/TODO.md` records it as deliberate). | Major | Phase 03 first makes CI build the image and verify the manifest hash, runs both paths for one cycle, then untracks. History size is not recoverable without a rewrite; stated as irreversible-by-design. |
| 6 | Deleting the Spec 26 skeleton crates may contradict ADR-0022 "preserve existing code" and the 2026-11-09 commit that added host-test support. | Major | Default is exclude-from-workspace, keep source; the decision is the owner's (Phase 00 item 1). Whether those crates have tests is recorded as an assumption (review grep found zero `#[test]`). |
| 7 | "Guarded copy unconditional" breaks the 32-bit targets and any lane built without `native-domains`. | Major | Phase 04 scopes it to 64-bit MMU targets with a `compile_error!` elsewhere only under release postures; legacy 32-bit builds keep the old path until decision 4. |
| 8 | Refactors of shared code can silently break RV64/AArch64, which stay supported regressions. | Major | Every kernel phase lists the RV64 baseline and an AArch64 boot lane in success criteria, not only x86. |
| 9 | The plan could reproduce C-2: twenty phases marked complete by prose. | Major | Phase 01 makes `status:` derive from checked success criteria via a guard script; all later phases are subject to it. Checkpoint phases require an evidence file with command, revision and log path. |
| 10 | Effort is large (≈110–150 days) and competes with the Intel C2C lane, iGPU and Apple lanes. | Minor | S0+S1+S2 (phases 00–06) are the minimum useful cut: truthful baseline, a regression gate and both Criticals closed. `plan.md` allows the owner to stop or reorder after any checkpoint. |

## Not challenged

- Technical correctness of the review's reviewer-cited (unverified) findings; each consuming phase re-reads its evidence first.
- Effort numbers; they are ranges from file counts, not measured velocity.
- Interaction with the open findings of the 2026-11-05 security audit.

## Validation performed

Path existence check over every repo path cited in the phase files (see the end of the planning session's output). No build, boot or test was run.
