# SAS/LBI Architecture → Approved Plan

Date: 2026-09-05
Status: Planning complete; implementation not started.
Plan: `.agents/260905-1139-sas-lbi-outcome-closure/plan.md`

## Decisions
- User approved Approach A: retain trusted SAS, close evidence/ownership/recovery gaps, then measure a real native workload.
- Five phases: evidence validity; grant/quota integrity; state-required hotswap; native baseline; counter + RedoxFS + upgrade/restart.
- Three reviewers reported 12 findings, consolidated into 10; accepted as plan corrections or explicit design gates.
- Generic IPC actually returns one zero byte. Native restart needs a private authorized fixture. Cached increment is included in the 1,000 total.
- Phase03 requires unforgeable requester identity, transaction-bound stash and late-completion/quiescence fences before Build; no implied ABI approval.
- Independent baseline rows proceed without unrelated recovery gates. Target misses stay open.

## Evidence and Limits
- Revised-plan structural validation PASS: 8 documents, 9 relative links, 47 exact file references; 2 source paths explicitly proposed new files.
- Source review only in this planning turn; no runtime fixes, new QEMU result, hardware qualification, ledger promotion or commit.
- Preserve existing source-bound approvals; LSP reference lookup lacked rust-analyzer in the pinned toolchain.

## Next
Run `/hc-cook /home/dmin/cellos/.agents/260905-1139-sas-lbi-outcome-closure/plan.md`, starting Phase01 and respecting phase-local design gates.
