---
phase: 1
title: "Contain unsafe paths and establish behavioral baselines"
status: completed
priority: P1
effort: "1 day"
dependencies: []
tier: thinking
---

# Phase 01: Fail-closed safety gate

## Requirements / architecture
- Gate domain-class zero-copy `GrantShare`/`GrantSlice`/`GrantAlloc` and `GrantRegister` operations that cannot complete owner/grantee mapping and revoke, **at the common syscall gate**, without disabling copied IPC or valid Tier-1 SAS grant behavior. Deny rather than return a plausible success/raw address or fall back to SAS. On-path gate must use the actual caller/target domain identity and generation, not an `is_domain` hint or a target TID without liveness check (`kernel/src/task/syscall.rs:130-181,1997-2105,6315-6657`). Deny all relevant domain-backed grant entry points consistently; preflight grants already live at update time and prohibit domain admission with them until drained.
- On AArch64/x86_64, temporarily refuse Tier-2 admission itself until phase 02 proves context/root switch ordering and incoming completion. `task.rs:1358-1365` currently activates TTBR0/CR3 before `Context::switch` saves the outgoing context; a grant-only gate does not contain this risk. Keep RV64 Tier-2 copied IPC available. Non-RV64 domain requests fail without SAS fallback.
- Disable live snapshot capture/restore before disk mutation on every block-capable board; `Snapshot` keeps the existing SupervisorCap/error contract. Warm boot still cold-boots; preserve `snapshot` command's unavailable response and audit denials (`kernel/src/task/syscall.rs:6299-6313`, `kernel/src/main.rs:681-694`). Do not leave a stale header that an older image could replay; record required cold-boot/operator procedure.
- Record failing-before tests **where safe** using throwaway dev images: domain grant RO-write, revoke/reuse and snapshot CRC/layout on a modeled block device; never intentionally run corrupt restore against production data. Freeze existing Tier-1 baseline and Tier-2 copied IPC, non-grant tests.

## Related files
`kernel/src/task/syscall.rs`, `kernel/src/main.rs`, `kernel/src/snapshot.rs`, `kernel/src/loader/domain_admission.rs`, `kernel/src/task.rs`, `tests/integration/tests/tier2_fault_isolation.rs`, `scripts/qemu-native-domain-test.sh`, `docs/system-architecture.md`.

## Implementation steps
1. Inventory live `GrantAlloc`, `GrantRegister`, `GrantShare`, `GrantSlice`, `GrantFree`, `GrantUnregister` callers and source-target tier combinations; preserve SAS semantics. Capture the dev/fleet feature tuples and test binary digests.
2. Add deny-before-mutation guards with **per-op ABI-safe sentinel**: `GrantAlloc`/`GrantRegister` must return `Ok(0)`, `GrantSlice` returns `Ok(usize::MAX)`, share/free/unregister return their established nonzero failure. Generic `Err(PermissionDenied)` encodes `usize::MAX`, which `libs/ostd/src/syscall.rs:1766-1775,1960-1967` wrongly treats as `Some(grant_id)` for allocation; test raw syscall **and wrapper**. Block capability grant even when sender is SAS and receiver is a domain, without publishing frame/map/task.
3. Restrict AArch64/x86_64 native-domain admission on the publication path until the switch/invalidation proofs land, preserving fleet-off posture and Tier-1/Tier-3. Audit pre-existing domain processes: require cold reboot before deploying this gate.
4. Make snapshot entry/restore refuse unless one internal build+runtime qualification gate is enabled; initially disabled in all shipping images. Test stale-header cold boot and the existing shell/Supervisor contract.
5. Capture the regression suite and exact baseline logs with `scripts/qemu-native-domain-test.sh --harts 1 --case admission,ipc-copy,grant-revoke` and `--harts 2 --case migration,user-copy-race,ipc-copy-race` after rebuilding; grant-revoke is a fixture, **not** production-syscall proof.

## Success criteria
- [x] RV64 Tier 2 native launch + copied IPC still work; domain↔SAS grant attempts fail closed before PTE/frame publication; AArch64/x86_64 Tier-2 launch denies until phase 02 qualifies switch; Tier 1 SAS grants remain functional.
- [x] `Snapshot` reports unavailable, no disk write or restore; cold boot and shell/`tests/integration/tests/launch-profile.rs` behavior unchanged.
- [x] Baseline logs identify exact feature/architecture image; unsafe test cases fail before patch and are not re-labeled as a pass.

## Assumptions / risk / rollback
- [UNVERIFIED] A single call-site gate covers every grant alias and thread-sharing path; map all syscall callers before editing. Rollback is to build with native admission off and snapshot restore disabled; reboot. A previously exposed frame or restored disk write cannot be reverted by code rollback: retire/reimage affected dev system and treat data as compromised. No ABI change authorized.

## Evidence

Host: WSL2 x86_64, `nightly-2026-05-01`, `qemu-system-riscv64` TCG. Every RV64 lane rebuilds the
image under `scripts/build-native-domain-test-ci.sh` (`test-hooks,native-domains`,
`RUSTFLAGS="-D warnings -C relocation-model=pic"`), and each case directory carries a `run.env`
with the feature tuple, firmware, QEMU version and the built ELF's sha256.

| Item | Command | Result |
|---|---|---|
| Pre-patch baseline, 1 hart | `scripts/qemu-native-domain-test.sh --harts 1 --case admission,ipc-copy,grant-revoke` | PASS 3/3, exit 0 — `.logs/native-domain-qemu/h1-admission-9WwwG1`, `h1-ipc-copy-m3PQ6T`, `h1-grant-revoke-coCSJ4` |
| Pre-patch baseline, 2 harts | `... --harts 2 --case migration,user-copy-race,ipc-copy-race` | PASS 3/3 — `h2-migration-sge2dT`, `h2-user-copy-race-JY2AXJ`, `h2-ipc-copy-race-2MQoFZ` (the trailing suite line was lost because this session edited the runner script while the lane was still reading it; the per-case `PASS:` lines are the record) |
| **Failing-before witness** | `... --harts 1 --case grant-gate` with the fixture present and the gate absent | exit 1: `S22-RV64-GRANT-GATE-{ALLOC,REGISTER,SHARE,SLICE,FRAMES}: FAIL`, `-SAS: PASS` — `h1-grant-gate-6j4WCL`. The ungated kernel publishes a domain-owned grant, a receiver PTE with `shared_to`'s rights ignored, and consumes frames on the refused-then-freed path. |
| Post-patch witness | same command with the gate | PASS 6/6 + terminal — `h1-grant-gate-rol41M`, re-confirmed on the pre-relocation build and on the frozen code |
| Post-patch regression, 1 hart | `... --harts 1 --case admission,ipc-copy,grant-revoke,grant-gate` | PASS 4/4, exit 0 — `h1-admission-llOmt4`, `h1-ipc-copy-ckkl4o`, `h1-grant-revoke-rh9RNV`, `h1-grant-gate-R1makd`; every case's boot log also shows `S22-RV64-SAS-FASTPATH: PASS roots=0 flushes=0` (Tier-1 untouched) |
| Post-patch regression, 2 harts | `... --harts 2 --case migration,user-copy-race,ipc-copy-race` | PASS 3/3, exit 0 — `h2-migration-xQlSZg`, `h2-user-copy-race-9n6WfT`, `h2-ipc-copy-race-AjwneD` |
| Off-feature build | `RUSTFLAGS="-D warnings" cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --no-default-features -Z build-std=core,alloc` | exit 0 (two build errors fixed — see Deviation Log) |
| AArch64 Tier-2 denial | `cd tests/integration && CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo test --test aarch64-boot tier2 -- --test-threads=1` | 2 passed: neither `tier2-smoke` nor `tier2-exploit` is published, neither runs, the shell answers after each refusal |
| Snapshot / Supervisor contract | `cargo test --test launch-profile -- --test-threads=1` (canonical `disk_v3.img` + patched RV64 kernel) | 1 passed (9.56s): `[snapshot] unavailable`, shell `snapshot: unavailable on this platform`, caller without `SupervisorCap` still denied, no frame written |
| RV64 Tier-2 integration | `cd tests/integration && cargo test --test tier2-fault-isolation -- --test-threads=1` on a `gen_disk.ps1`-rebuilt image | 5/5 passed (49.95s): fail-closed grant denial in `tier2-smoke`, NULL/peer/kernel fault containment, and `posix-shim-test` still positively admitted as a Tier-2 FFI domain |

Denial sentinels actually asserted by the new fixture, through the production `handle_syscall`
path with a real `TaskAddressSpace::Domain` task:

| Operation | Denial | Why that sentinel |
|---|---|---|
| `GrantAlloc`, `GrantRegister` | `Ok(0)` | any nonzero return is decoded as a grant id by `libs/ostd/src/syscall.rs` |
| `GrantSlice` | `Ok(usize::MAX)` | the established "not authorized" result for the same call |
| `GrantShare`, `GrantFree`, `GrantUnregister`, `GrantDma` | `Err(PermissionDenied)` | the established nonzero failure |

Not proven here, and deliberately left to its phase: the domain grant lifecycle itself (phase 03),
the ordered non-RV64 switch (phase 02), the snapshot format/restore (phase 07). Physical MMC and
remote-TLB witnesses remain hardware-gated, so those profiles stay disabled.

## Deviation Log

- **Subagent delegation unavailable.** Every agent type routes to the Codex provider, which
  returned `Payment Required` / `usage_limit_reached` for `scout`, `sonic` and `reviewer` on
  2026-09-27. Recon, Build, Verify and the adversarial review pass were therefore executed inline
  by the session model instead of the skill's specialist agents; there is no independent reviewer
  signature on this phase. The review pass is recorded in
  `reports/harness/adversarial-validation.json` with the finding it produced and the fix.
- **Test contract changes were required, not optional.** The plan's cutover disables domain-class
  grants, so three suites that asserted the retired positive behaviour now assert the refuse
  contract instead of being relabelled: `cells/tests/tier2-smoke/src/main.rs`,
  `tests/integration/tests/tier2_fault_isolation.rs`, `tests/integration/tests/aarch64-boot.rs`,
  `tests/integration/tests/x86_64-boot.rs`. RV64 Tier-2 launch, copied IPC, fault containment and
  the `posix-shim-test` FFI domain leg stay positive tests.
- **Snapshot gate shape.** Implemented as a build feature (`snapshot-qualified`, never in a default
  set) plus a runtime constant, so both the build and the running image have to opt in; the plan
  asked for one internal build+runtime gate.
- **Adjacent pre-existing fix.** The off-feature build (`--no-default-features`, i.e. no private
  root) did not compile before this phase: `task::futex::key_for` called the domain-gated
  `hart_local::current_domain()`. Added the SAS fallback (`(0, 0)`, the documented `space = 0`
  identity) so the phase's off-feature check can run at all.
- **Review finding fixed before delivery.** The first version of the live-domain-grant preflight
  ran inside `evaluate_domain_admission`, which is called while `task::launch::publish_prepared`
  holds `SCHEDULER`; the scan takes the grant tables and re-reads `SCHEDULER`, a self-deadlock on
  any domain-class launch. Moved to `refuse_while_domain_grant_live`, called first, before the
  scheduler lock, preserving the documented `*_GRANT_TABLE → SCHEDULER` order.
- **Not executed in this environment** (named, not silently skipped): the x86_64 QEMU lanes need a
  Limine ISO built by PowerShell tooling not available here; the physical-board MMC snapshot lane
  and remote-TLB witnesses need hardware. Their claim gates stay closed.
- **Local image rebuild side effect.** `gen_disk.ps1` rebuilt `disk_v3.img` (gitignored) so the
  updated `tier2-smoke` cell is packaged for the integration lane. In this WSL environment two
  *optional* cells fail to build and were omitted (`doom`, `tetris-lua`); the previous image is
  backed up at `/tmp/disk_v3.img.pre-phase01`. A CI rebuild produces the canonical set.
- **Pre-existing break found while verifying.** The AArch64 test-hooks kernel lane
  (`scripts/build-aarch64-test-hooks-ci.sh`, `test-hooks` without narrowing the RV64-only
  scaffolding) did not compile: 8 errors in `task/scheduler.rs`, `task/user_copy/mod.rs` and
  `memory/paging.rs` (riscv64-gated items used unconditionally, unused imports under
  `-D warnings`, two `u64 & usize` mismatches), and 14 more once the imports were narrowed.
  Reproduced with this phase's kernel changes stashed — identical errors — so it is not caused
  by the containment work. **Repaired in the phase-02 session** (each helper now carries the
  cfg of the fixture that uses it; the lane builds and boots green — see
  `phase-02-domain-root-lifetime.md` § Progress). The AArch64 *admission* evidence above still
  comes from the production image.
