# S6b handoff request — init reduction, hot-swap decision, `api` split (phase 13)

**Date:** 2026-11-13 · **Plan:** [plan.md](plan.md) · **Prior slice:** S6 Phase 12 closed 2026-11-13 (CP2 confirmed, merged to `main` at `e99137cd8`).

## 1. Scope (measured on `dc675b8c2`)

| Surface | Today |
|---|---|
| init | 3 087 lines and 21 feature flags. It hosts the activation endpoint (`OCEL_ACTIVATOR`, `main.rs:149`) and the machine-pin gate (`machine_pin::gate`, `main.rs:137`). Test-profile launch code sits in `boot.rs` (e.g. the AArch64 `tier2-grant-pair` launches its pair at `/bin/silo` and `/bin/net-broker`). |
| Kernel and init | `TRUSTED_INIT_TID` is first-wins (`task/syscall/spawn.rs:62`). The kernel has no explicit policy for init's death. |
| Hot-swap | The supervisor's target map names vfs, net, compositor, input and hotswap-demo (`supervisor/src/main.rs`). No production service handles Snapshot/Restore. Two traits are both named `ViStateTransfer` (`libs/api/src/services/hotswap.rs:31`, `libs/ostd/src/hotswap.rs:201`). |
| `api` | 676 files import it, from 50 crates. It bundles the frozen ABI, the service wire (now `services::proto`) and a 64 + 11-file posix shim (4 importers). The kernel links all of it, but uses only `VfsRequest::Rename` from the service wire (`task/syscall/fs.rs:543,571`). |
| `types` | 6 353 lines, of which 5 130 (81 %) are KMS codecs. |

## 2. Staging: proposed

**Step 1 (now)** is read-only plus a document:
- the import census of `api`;
- the crate design: `abi` (no dependencies), `wire` (Phase 12 proto modules and postcard service enums), opt-in `posix-shim`, and `kms-codec` taken out of `types`;
- what the kernel still needs from the service wire (`Rename`);
- the init and kernel contract changes: endpoints move out, and an init-death policy is added;
- the hot-swap decision;
- `cp1-api-split.md`.

**Nothing changes until you confirm CP1.**

**Steps 2–7** follow, one commit per step, with the x86 boot lane run after every init step:
2. Split the crates behind an `api` facade, keeping import paths. The kernel depends on `abi` (plus the minimum it encodes).
3. Move the activator and machine-pin endpoints into supervised cells. Fixtures leave init (catalog fixture rows).
4. Kernel init-death policy: a bounded fault record, then a platform reset. Add an x86 fixture that kills init.
5. Hot-swap: config completes a real v1→v2 swap through `serve` hooks, with a key preserved. The other map entries and dead impls are removed. Spec 03 §3 and Spec 12 axis 5 are corrected.
6. Migrate imports off the facade and delete `libs/api`.
7. CP2. This is a checkpoint phase: three arches, `x86-c2c`, release-posture boot, and an independent review.

## 3. File set (after CP1)

- **Create:** `libs/abi/`, `libs/wire/`, `libs/posix-shim/`, `libs/kms-codec/`, `cells/services/activator/`, `cells/services/machine-pin/`, `cp1-api-split.md`, `cp2-api-split.md`, the init-death x86 fixture.
- **Modify:**
  - `cells/tools/init/**`;
  - `kernel/src/task/syscall/**` (init-death policy), plus kernel imports of `api`;
  - `cells/services/supervisor/src/{main,hotswap}.rs`, `cells/services/config/src/main.rs`;
  - `libs/api/**` (facade, then deleted), `libs/types/**`, `libs/ostd/src/hotswap.rs`;
  - root `Cargo.toml`, `catalog/cells.toml`, `config/services.toml`, every crate's `api` dependency line and imports (mechanical);
  - `docs/specs/{03-runtime,12-reliability}.md`;
  - the Law-1 digest scripts, re-pinned under CP2.
- **Delete:** `cells/services/vfs/src/manager/state_transfer.rs`, one `ViStateTransfer` trait, init's fixture modules.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `.agents/cell-contract-0024-locks.md`, `docs/project-changelog.md`.

## 4. Behaviour changes to expect

- **PID 1 answers no service IPC.** Activation and machine-pin become their own restartable cells.
- **Init's death resets the platform** after a bounded fault record, instead of leaving an orphaned system.
- **Hot-swap is claimed only for config,** which is proven in QEMU. The swap entries for vfs, net, compositor and input go away.
- **Import paths move** from `api::…` to `abi::…`/`wire::…`. No frame byte or syscall moves.

## 5. Conflicts

- **The other session** committed `fix(supervisor): authorise hostile backend kills by attested program` (`3968428f2`) and is working on the hostile VirtIO lanes. Step 5 edits `supervisor/src/{main,hotswap}.rs`; each commit is rebased on what that session has committed, and none of its uncommitted files is touched.
- **Step 6 touches about 50 crates' imports mechanically.** It lands as its own commit, rebased just before merge.

## 6. Questions to the owner

1. Grant S6b staged as in §2: step 1 (census and CP1) now, steps 2–7 only after your CP1 confirmation.
2. Hot-swap: the plan default is to wire config and remove the other four map entries. The alternative is to remove hot-swap claims entirely.
3. Init death: should it reset the platform (plan default), or only log and halt?
