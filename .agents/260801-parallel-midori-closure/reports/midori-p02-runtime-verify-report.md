## Phase Implementation Report
- Phase: phase-01-p02-runtime-verify | Plan: .agents/260801-parallel-midori-closure | Status: partial
### Files Modified — cells/tests/vfs-test/src/main.rs, cells/tests/vfs-test/src/grant_io.rs, cells/tests/vfs-test/src/dircap.rs, tests/integration/tests/vfs-quota.rs, .agents/260801-parallel-midori-closure/phase-01-p02-runtime-verify.md, .agents/260801-parallel-midori-closure/reports/midori-p02-runtime-verify-report.md, docs/project-roadmap.md, docs/project-changelog.md
### Tasks Completed — [x] Read docs/code-standards.md and the revised phase files; [x] Captured the mandated baseline commands and corrected them to WSL execution; [x] Added honest `ReadFileGrant` QEMU markers in `vfs-test`; [x] Tightened `vfs-quota` to require both new grant PASS markers and reject `[FAIL] grant:` output; [x] Verified `ReadGrant` source coverage is blocked because `HandleTable::insert_ro` has no real caller beyond the unit fixture; [x] Updated the original Phase 01 status/report plus living docs to keep the phase partial
### Tests — typecheck: pass (`cargo build --release --target riscv64gc-unknown-none-elf -Z build-std=core,alloc -p app-vfs-test --features test-hooks`) | unit: pass (`bash scripts/build-test-hooks-ci.sh`) | integration: pass (`cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`)
### Issues — `ReadGrant` runtime coverage blocked: `cells/services/vfs/src/handle_table.rs:136` is still the only `insert_ro` caller, so no real handle source exists. Actual fast-IPC `GetFile` runtime proof remains blocked by D1 because `cells/tools/shell/src/cmd_fs.rs` still falls back to ordinary IPC for separately linked Cells. The test-hooks build dirties `kernel/src/embedded-test-hooks/init` in the worktree; that artifact is build output, not a source change.
### Next — Separate approved work is still required for real `ReadGrant` handle seeding and for fast-IPC `GetFile` Tier-1 runtime proof or a formal success-criteria rescope.

## Runtime Evidence
- Baseline build command: `bash scripts/build-test-hooks-ci.sh`
  Result: initially failed from the PowerShell host path with `cargo: command not found`; passed from WSL and produced `target/riscv64gc-unknown-none-elf/release/vicell-kernel-test-hooks`
- Baseline integration command: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`
  Result: before the rebuild, this timed out waiting for `[PASS] grant: ReadFileGrant copies nonzero bytes` because QEMU still booted the stale pre-edit artifact
- Final build command: `bash scripts/build-test-hooks-ci.sh`
  Result: pass in WSL after the `vfs-test` edits
- Final integration command: `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test vfs-quota riscv64_vfs_quota_all_pass -- --nocapture`
  Result: pass in 3.20s
- Decisive runtime markers:
  - `tests/integration/tests/vfs-quota.rs` now waits for `[PASS] grant: ReadFileGrant copies nonzero bytes`
  - `tests/integration/tests/vfs-quota.rs` now waits for `[PASS] grant: ReadFileGrant is refused after sealing`
  - `tests/integration/tests/vfs-quota.rs` now rejects any `[FAIL] grant:` substring
  - Because the final gate passed, those markers were observed by the fresh QEMU run
- Source verification command: `rg -n "insert_ro\\(|ReadGrant|sys_open_cap|OpenCap" cells/services/vfs libs/ostd kernel/src`
  Result: `cells/services/vfs/src/handle_table.rs:136` is still the only `insert_ro(` caller; `ReadGrant` remains blocked by missing real handle seeding
- Contract guard: `git diff -- libs/api libs/types kernel/src/loader/reloc.rs`
  Result: no diff
- D1 guard: `grep -R -n -E 'resolve_export|R_RISCV_JUMP_SLOT' kernel/src docs/specs/17-ipc-wire-contract.md`
  Result: no matches
