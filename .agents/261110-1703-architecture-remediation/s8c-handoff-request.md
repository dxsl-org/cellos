# S8c handoff request: spec reconciliation, hygiene and closure (Phase 19)

**Date:** 2026-11-16 · **Plan:** [plan.md](plan.md) · **Prior:** Phase 18 closed 2026-11-16 ([evidence](reports/phase-18-evidence.md)); `main` is at `0e16f59f32`.

Phase 19 is the last phase of the plan and a checkpoint phase: it ends with your written closure.

## 1. Scope (measured on `0e16f59f32`)

| Surface | Today |
|---|---|
| Findings | The [architecture review](../reports/architecture-review-261110.md) lists 2 Critical, 14 High, 22 Medium and 8 Low findings. [scout-report.md](scout-report.md) maps them to phases. No closure matrix exists yet. |
| Spec numbering | Two numbers are used twice: `00-context.md`/`00-fork.md` and `14-distributed.md`/`14-viui.md`. The phase file names only 14. |
| Spec contradictions | The phase file lists Spec 03, 07, 09, 10, 18, 19 and 22. Phases 13–18 already edited 13, 15, 16 and 18. Per-arch status is restated in Specs 02, 18, 19 and 22. |
| Root residue | All eight named files are tracked: `invalid.json`, `dummy.jsonl`, `__build.bat`, `__build_check.ps1`, `do_build.bat`, `doom_build_debug.txt`, `test_subprocess.py`, `test_python_payload.py`. So are `sha256:4970…jsonl` and `cells/services/compositor/kernel-graphics-legacy.rs`. |
| `.agents/` | 7,442 tracked files, 140 MB tracked, 204,227 tracked Markdown lines, 5.1 GB on disk. `.agents/archive/` holds 160 KB. |
| Lanes | `scripts/lanes.toml`: 23 `pr`, 1 `nightly`, 51 `manual`. These fail on `main` today: `vfs-quota`, 2 `rust-std-promotion` tests, the `x86hv-iso` rollout guard, the EL2 test-hooks lane, and `check-lanes.py` (`qemu-tier3-{chrome,display}-x86.sh` have no row). |
| Carried residuals | Phases 16–18 residuals in their evidence reports. Most urgent: from 2027-02-04 stale allowlist entries start failing `--sign`, so every image build stops signing (15 entries that day, 67 by 02-09, then 109 plus the three tier exceptions on 02-15). |

## 2. Staging (proposed)

1. **Closure matrix.** `reports/closure-matrix.md`, with one row per review finding: phase, evidence file, the review's original check re-run (grep/count), and a status of `closed`, `residual` or `open`. No row is `closed` on prose alone.
2. **Spec sweep.** Each contradicted section is fixed to match the code, or carries a dated "historical" banner when an earlier phase deliberately changed the behaviour. `scripts/generate-arch-status.py` writes `docs/generated/arch-status.md` from cfg/feature probes and `lanes.toml`. Specs 02, 18, 19 and 22 reference that table instead of restating per-arch status.
3. **Spec numbering.** Rename one file of each duplicate pair, then fix links, `llms.txt` and anchors.
4. **Residue.** Delete each file in §1 after a reference grep across `scripts/`, `.github/`, `tests/` and the build files. Also delete the `kernel/src/memory.rs` dead items, the duplicate `PAGE_SIZE` definitions and the rest of the review's Low list.
5. **Contributor docs.** Fix `CONTRIBUTING.md`, `Dockerfile`, `llms.txt`, the target names in `rust-toolchain.toml`/`deny.toml`, and `tests/integration/.cargo/config.toml`.
6. **Allowlist re-review wave.** Re-read every entry with `review_by` before 2027-03-01, site by site as Phase 18 did, so signing does not stop in February.
7. **`.agents/` retention.** Report sizes per plan and apply only what you approve (question 3).
8. **Living docs.** `system-architecture.md`, `security-model.md` (re-read against Phases 04, 06, 07 and 18), `codebase-summary.md`, `current-focus.md`, `project-roadmap.md` and the changelog.
9. **Checkpoint.** Every `pr` and `nightly` lane runs on the closing commit, followed by an independent end-to-end review. The closure request then goes to you.

The working mode is the same as S8 and S8b: a worktree, one commit per step, nothing pushed. An independent review runs after steps 1–5 and again at the checkpoint.

## 3. File set

- **Create:** `scripts/generate-arch-status.py`, `docs/generated/arch-status.md`, `reports/closure-matrix.md`, `reports/phase-19-evidence.md`.
- **Modify:**
  - `docs/specs/**`;
  - `docs/{system-architecture,security-model,codebase-summary,project-changelog,project-roadmap,code-standards}.md`, `docs/roadmap/current-focus.md`, `docs/spec-status.generated.md`;
  - `CONTRIBUTING.md`, `Dockerfile`, `llms.txt`, `rust-toolchain.toml`, `deny.toml`, `tests/integration/.cargo/config.toml`;
  - `scripts/unsafe-allowlist.toml` (reasons and `review_by` only), `scripts/lanes.toml` (rows for the two tier3 scripts);
  - `kernel/src/memory.rs` and the `PAGE_SIZE` sites (dead items only);
  - this plan directory and `.agents/plan-portfolio.md`.
- **Delete:** the root residue in §1 and `cells/services/compositor/kernel-graphics-legacy.rs`, each after a reference grep.
- **Move (only with your approval):** completed plans' `reports/` to `.agents/archive/YYYYMM/<plan>/` by `git mv`.

`libs/abi` and `libs/types` do not change. No kernel behaviour changes. A code change other than deleting dead items is a new finding, and comes back to you.

## 4. Behaviour to expect

- No runtime behaviour changes: specs, docs, generated tables, dead files and allowlist reasons only.
- Spec file renames break external links to the old names; in-tree links are fixed.
- After step 6 no allowlist entry goes stale before 2027-03-01.

## 5. Risks and conflicts

- **Checkpoint lanes.** "All `pr` and `nightly` lanes green" cannot hold while the failures in §1 stand, and none of them is caused by this plan (question 2).
- **A banner can hide a still-valid requirement.** Only sections contradicted by code that an earlier phase deliberately produced get a banner. Anything else is logged as a new finding.
- **Archive moves** rewrite paths that other plans may link to. Moves use `git mv`, links are re-checked by `check-spec-anchors.py` and `check-plan-status.py`, and nothing is deleted.
- **Other session in `/home/dmin/cellos`:** about 43 untracked paths, including `.agents/261112-1035-multi-sector-block/`, which fails `check-plan-status` while untracked. This file set does not touch them.

## 6. Questions to the owner

1. Grant S8c (Phase 19) staged as in §2, continuing without stops. It stops only for a code change beyond dead-item deletion, or a finding that needs a decision.
2. **Lanes failing on `main`** (`vfs-quota`, `rust-std-promotion`, `x86hv-iso`, EL2 test-hooks). Proposed: each is diagnosed. A failure that a small, local fix resolves is fixed in this phase. The rest go into the closure matrix as `residual` for your written acceptance. Alternative: accept all four as residuals without diagnosis.
3. **`.agents/` retention.** Proposed: report sizes only and move nothing; retention becomes a separate decision. Alternatives: archive the `reports/` of completed plans by `git mv` (nothing deleted), or archive and delete plans stale for more than 90 days.
4. **Allowlist re-review wave (step 6).** Proposed: in this phase, before 2027-03-01, as Phase 18 did. Alternative: a separate follow-up, with the 2027-02-04 date recorded as a deadline.
