# 2026-11-22 — Phase 03 untrack, security audit batches A–E

## Done
- rust-std approval re-pinned twice under condition 6 (`e0f1c35659`, `b01498db83`).
- CI run 44921343700 on `124d4279d5` green (30 jobs, KVM skipped, 0 fallback warnings) → H-14/M-B5a untrack `f830f377c5` (1,501 paths; `.gitignore`); records `745fd3c00d`.
- Audit re-triage persisted at `.agents/reports/security-261121-audit-retriage.md`. Owner picked A–E; landed `97b86702a9`, `583e4303b3`, `0ea431240f`, `047a06510c`, `6248e7f318`, docs `64da592e79`. Owner approved Tier-1 unsafe total 586 → 587 for the aarch64 FP test cell.

## Decisions
- Untrack by moving files out, committing deletions with an exact `only()` selection, moving back.
- 08 S-18b skipped (breaks g2-cow slot recovery contract).
- httpd reads via its cwd handle; `/www` seeded in the ramfs root.

## Lessons
- `commit.draft()` preselects every in-scope change; `take()` is async and only adds. Narrow with `only(...)` before committing; check `git show --stat` before pushing.
- Embedded images are now untracked: never restore them from HEAD after image builds.

## Open
- RV64 lanes failing at HEAD too: pipe-test (LBA 0 unreadable early), phase03 (HLT moved to `arch::power`), `&` launches refused (httpd/quickjs lanes), http-infer/concurrency (no AI service in image).
- httpd `/api/files` listing still path-based (empty under a cwd handle).
- ABI text for NET_RX `PermissionDenied` and x86 `SetTlsBase` range; `gpe_blocks` host test; runtime SetTlsBase fixture; aarch64 httpd lane.
- Audit batches F/G/H and the rest listed in current-focus item 6.
- Renew B-AARCH64-SEMHOSTING before 2026-12-21T03:53:07Z.
