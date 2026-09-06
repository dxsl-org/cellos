# Scout Report

> Historical planning input. ADR-0013 supersedes the actor-separation and
> cross-lane serial recommendations below; factual source findings remain usable.

## Scope and Inputs

Read and reconciled `agent://ResearchArmAcceptance`, `agent://ResearchPosixSequence`, `agent://ResearchX86Qemu`, `agent://RiskFirstPlanJudge`, `agent://SimplicityFirstPlanJudge`, and `agent://ScoutNextCellosWork`. Source spot-checks below are against the current worktree; no implementation, build, or test was performed.

## Architecture Map

- Acceptance governance: `docs/app-tier-acceptance-ledger.json` → `scripts/app_tier_acceptance/{events,ledger,validator}.py` → `tests/app-tier-acceptance/`.
- POSIX path: frozen `libs/api` ABI → `libs/ostd` wrappers → `kernel/src/task/syscall.rs` → caller-scoped task/FD state → `ViFileSystem` → C shim/cells.
- x86 qualification: checksum-pinned installer → explicit `QEMU_X86_BIN` → smoke/e2e/hostile runners → existing CI artifacts.

## Decisive Findings

1. `scripts/app_tier_acceptance/ledger.py:159-211` permits one lifecycle event, requires one adjacent lifecycle transition, and keeps blocker `id/subject/scope/evidence` immutable. The stale `qemu-rv64` AArch64 blocker therefore cannot be redirected by an ordinary event.
2. `docs/app-tier-acceptance-ledger.json:6-15` binds `B-AARCH64-SEMHOSTING` to RV64 and a stale compile claim. `scripts/qemu-aarch64-test-hooks.sh:43-72,88-124` already provides a semihosting runtime oracle, but a passing independent raw run is not checked in as lawful closure.
3. `libs/api/src/abi.rs:2-12` requires two explicit confirmations even for additive ABI changes. `libs/api/src/abi/syscall.rs:766-780` proves bits 55–59 are occupied; searches found no bit 60–63 assignments and no IDs 252–255.
4. `kernel/src/task.rs:1488-1500,1613-1680` already provides caller-scoped lexical CWD, failure-atomic chdir, and exact non-NUL getcwd. Shell `pwd` and `$(pwd)` still return root literals (`cells/tools/shell/src/cmd_sys.rs:6-12`; `cells/tools/shell/src/executor.rs:79-110`).
5. `_fstat` fabricates success into target C layout (`libs/api/src/services/posix/sysio.rs:83-97,227-239`), while available backend facts are only kind/size/existence (`libs/api/src/services/fs.rs:23-35`). The restrictive smoke manifest currently lacks `Open/Fstat/Close`, so truthful wire/copyout and explicit open/close coverage are both required.
6. `ViFileSystem` has no rename and VIFS1 is read-only. Task FDs, `OpenCap`→`CAP_TABLE` (`kernel/src/task/syscall.rs:4249-4273`), and `read_file_from_vifs1` transient handles (`kernel/src/fs.rs:25-61`) all hold pathname-backed files. Writable activation also exposes WriteCap/TruncateCap. A canonical-key lease/reservation ledger and all-mutator authority are required; bit 62 alone is insufficient.
7. The installer pins archive/digest/build/version but accepts an exact-version cached binary early (`scripts/install-qemu-x86-ci.sh:6-83`). Qualification must use one exported initially absent `.../qemu-10.2.0` prefix and hashed installer transcript. Smoke reports any version; e2e/hostile regexes accept suffixed 10.2.0 builds.
8. Live POSIX navigation still cites deleted `libs/api/src/posix.rs`, while the real split root is `libs/api/src/services/posix.rs:1-43`. Historical records must not be retroactively rewritten.
9. The safest serial rule is strict: a failed phase halts every successor. Passing evidence must name the exact clean-tested source commit/tree in the normal verification report/current changelog.

## Existing Conventions to Reuse

- Exhaustive syscall ID/decode/allowlist tests in `libs/api/src/abi/syscall_tests.rs`.
- Caller-aware user-copy and string bounds in `kernel/src/task/syscall.rs`; no per-hart/current-task attribution.
- Shell integration harness in `cells/tools/shell/src/shell_test.rs` and `tests/integration/tests/shell-utils.rs`.
- POSIX smoke cell plus existing boot test in `tests/integration/tests/boot.rs:2157-2180`.
- Content-addressed regular evidence files and trusted-baseline validation in the acceptance-ledger tooling.
- Existing exact QEMU installer and explicit-binary CI path; no new downloader or runner abstraction.

## Ownership Boundaries

- Phase 01: ledger steward owns schema/events; independent runner owns raw execution; independent reviewer ratifies correction. These identities must be distinct.
- Phase 02: docs owner only; no ABI/kernel/test ownership.
- Phase 03: ABI owner approves, kernel syscall owner wires existing CWD, shell owner consumes, integration owner accepts.
- Phase 04: ABI owner freezes wire, kernel/FS owner supplies facts, POSIX owner translates, integration owner observes the guest marker.
- Phase 05: storage owner proves backend; security owner approves all-mutator authority; kernel owner covers Task FD, CapResource, and transient lease lifetimes; ABI work begins last.
- Phase 06: x86 virtualization owner changes existing preflights; CI/toolchain owner retains installer provenance; evidence reviewer guards non-claims.
- Cross-phase handoff: each successor starts only after the prior phase's clean-tested commit/tree and normal verification binding pass every criterion.

## Precedent and History

The bounded CWD precedent is `.agents/260902-posix-cwd-path-slice/phase-01-canonical-cwd-paths.md`; it deliberately leaves public ABI, shell, fstat, rename, and broad POSIX out. The x86 differential report `.agents/debug/debug-260824-2322-x86-tcg-svm-version.md` supports exact 10.2.0 selection, not a qualified backport. Current roadmap/risk text classifies AArch64 evidence and pinned x86 QEMU as technical debt while retaining production, physical, and broad POSIX exclusions.
