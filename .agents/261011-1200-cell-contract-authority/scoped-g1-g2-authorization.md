# Scoped G1/G2 development implementation authorization

Recorded 2026-10-14. Decision source: user instruction **“review và cấp quyền triển khai scoped G1/G2 để tạo số đo thực”**, following the selected dependency-checkpoint approach. This records the requested **development source-edit authority**, not assistant-issued fleet/operator authority. [Independent review and dispositions](g1-g2-scoped-review.md) corrected the candidates. Exact candidate identities and source baseline are pinned in [authorization manifest](scoped-g1-g2-authorization.json).

## Decision and owner handoff

Authorize **G1 Intel private accounting** and **G2-R-core/B development correction/provisioning** within the paths and restrictions below. Do **not** authorize G2-J: journal/Unknown ownership/crash-preserving allocation requires a separate complete candidate and review. This is an explicit partial grant based on review, not a claim that all G2 evidence is reachable today.

Integrating owner is the controlling P01 execution owner; conceptual P03 CPU and P05/P06 VFS/lifecycle owners hand off only the named prerequisite work to this owner. No legacy plan may resume. No grant from this document may be delegated into a broader file-owner or phase-completion permission. **WIP is one implementation slice: G1 first, then G2-R-core/B.** Read-only research/review may be independent; each actual edit/build/capture sequence has one integration owner.

The dependency amendment is adopted **only to enable this named source-edit sequence**. Historical proposed texts remain unchanged. This authorization supersedes their blanket documentation-only statement for these slices alone; the ten-phase DAG and38legacy lock relations remain pending/effective. It does not authorize P02–P10 general execution.

## G1 allowed work and paths

Implement the reviewed G1 r2 design on the Intel/x86_64 private development lane: checked shared HPET, aggregate occupancy integration, common period splitting, burst/debt, bounded eligibility/throttling, concrete-switch-only staging/no-switch service, non-returning incoming-hook failure, exact-generation retirement and non-evicting terminal summaries. Fixed payload41304bytes plus separately accounted lock/code/stack. Preserve existing authority, thread/Cell ceilings, watchdog and public tick telemetry; no public CPU-policy request or default production entitlement.

Permitted files:

- `kernel/src/task/cpu_accounting.rs` (new private module), `kernel/src/task.rs`, `kernel/src/task/scheduler.rs`, `kernel/src/task/launch.rs`, `kernel/src/task/hart_local/ready.rs`.
- `kernel/build.rs`, `kernel/src/main.rs`: private checked development selection/init only; normal-image exclusion, no production admission.
- `hal/arch/x86/src/x86_64/hpet.rs`, `hal/arch/x86/src/x86_64.rs`, `hal/arch/x86/src/x86_64/context.rs`: checked clock/calibration outcome and existing incoming callback integration only; preserve assembly/Context public layout and unrelated HAL behavior.
- Existing named kernel fixture files `retirement_selftest.rs`, `stack_overflow_probe.rs`, `x86_idt_cpl3.rs`, `domain_switch_tests.rs` under `kernel/src/task/`: selected-image accounting compatibility only.
- `cells/tests/bench/src/main.rs`, `cells/tests/bench/src/bench-probe.rs`; one new private `cells/tests/bench/src/scenarios/cpu_accounting.rs` may supply the finite scenario. `cells/tests/bench/Cargo.toml` may register the private scenario/build selection without public dependencies or normal-profile widening.
- `scripts/x86/p01_capture.py`, `scripts/x86/p01_trace.py`, `scripts/x86/test_p01_trace.py`: private source-bound producer/decoder/consumer and regression integration; no bypass of existing oneCPU/twoCPU or invalid-observation gates.

A wrong/nonexistent existing path is not permission to create a guessed alternate. Additional scheduler/SMP/TCB/syscall/HAL source changes require explicit scoped amendment before edits; do not use directory scope as a fallback. Internal exported Rust accessor/trait changes require actual reference/caller closure before their edit; failed LSP must be recorded and resolved sufficiently for the affected build/provider matrix, not called exhaustive.

## G2-R-core/B allowed work and paths

R-core: open-existing-only native mounts; format-preserving V1 geometry/range/counter validation; exact functional create-or-truncate/positional/read results; all mutation/read/barrier errors propagated; strict NotFound target checks, duplicate rejection and allocator error propagation; original-error poison; authorization-first pre/post health gates including ReadFileGrant/empty/pending/synthetic/rename paths; unsupported recursive native deletion. Existing quota behavior remains unchanged. Possibly accepted failure permanently ends mutation on that disposable/staged image; reopen is read-only fault observation, then discard. No restarted mutation or quota/journal/durability recovery claim.

Permitted existing VFS files under `cells/services/vfs/src/`: `backend_cellosfs.rs`, `backend.rs`, `manager.rs`, `mount.rs`, `paths.rs`, `dispatch_paths.rs`, `dispatch_grants.rs`, `dispatch_file_handles.rs`, `dispatch_dirs.rs`, `dispatch.rs`, `grant_write.rs`, `grant_read.rs`; actual remaining FsBackend implementors `access/stub.rs`, `backend_ramfs.rs`, `backend_fat.rs`, `backend_bootfs.rs`, `backend_bin_overlay.rs` solely for internal health-contract migration without a durability promise.

Permitted library paths: `libs/cellos-fs/src/engine.rs`, `allocator.rs`, `format.rs`, `disk.rs` and `libs/cellos-fs/tests/fs_tests.rs`, `power_cut_fuzz.rs` for existing V1 functional/error checks and software fault scenarios. Existing `FsError` variants, V1 bytes/reservations/version and public BlockDevice/VFS wire remain frozen; any incompatible Rust/public/persistent API change requires separate exact approval.

B: one bounded host formatter using existing V1 format may be added under `tools/cellos-fs-provision/` with workspace registration in `Cargo.toml`/`Cargo.lock` if required. Tool validates geometry and blank windows **before** format writes, stages/exclusively creates outputs and refuses nonblank/incompatible existing data. No new dependency is assumed necessary. Allowed producer migration: `scripts/mksrv-img.sh`, `scripts/format-disk-arm.sh`, `scripts/gen-disk-ci.sh`, `scripts/gen-disk-aarch64-ci.sh`, `scripts/format-disk-hv-arm.sh`, `gen_disk.ps1`; `.github/workflows/ci.yml` and `tests/integration/tests/srv-cellosfs.rs` fixture compatibility. Non-Intel producer edits preserve compatibility only, not non-Intel execution/qualification. Existing output must be classified/staged before destructive truncation; never format a retained/nonblank volume under this grant.

Documentation may update exact affected semantics/comments and `CHANGELOG.md`, including `docs/decisions/0016-cellosfs-native-replaces-redoxfs.md`. `libs/api/src/abi/disk.rs` comments only: no constants/layout/wire edits. Existing candidate/manifest/source pins must be explicitly revised when legitimate implementation changes invalidate them, never regenerated to conceal drift.

## Explicitly forbidden

No D01–D12/C01–C11 CP1/CP2 promotion, frozen VFS/KMS/syscall/manifest/public encoding change, OGJ1 implementation, quota Unknown/owner-exit persistence/state transfer, allocator reachability reconstruction/deferred crash-preserving reclamation, genesis/reservation/live claim/key closure, real issuer roster/public-pin admission, backend/device/cache/floor admission, protected store/time/provider/TLS, remote route enablement, production entitlement, physical/VMX qualification, procurement or provisioning of credentials/hardware. No test signer becomes authority; no source or emulator observation becomes hardware proof.

## Editing, capture and interpretation gates

1. Before editing, verify pinned candidate and source baseline, actual path/implementor/reference closure and one-owner/WIP selection. Candidate grant authorizes writing the implementation; it does not require an already-written diff.
2. Keep changed measurement behavior behind checked private development selection; ordinary images cannot accidentally enable G1. G2-R/B source can be developed/build-checked but executed only on explicitly disposable/staged private images; do not deploy normal retained-volume images from this grant.
3. Before an accepted capture, review actual diff/lock/unsafe/cfg/caller closure and observe normal-image exclusion/compatibility. Specify finite event kinds/emission/coalescing/action schedule and static capacity proof. Failure/overflow closes measurement, never truncates to success.
4. G1 capture measures actual checked clock/device tuple and full exhaustion-to-final-transfer D; H/S component bounds and sample maxima do not prove universal D/W. Counterexamples remain raw evidence.
5. G2 software fault runs exercise normal plus error/short-read/short-write/both flush cuts, nonformat corruption/reopen, blank/existing provisioning and every affected mutation/read/dispatch branch. They cannot qualify physical durability, acknowledged checkpoint preservation or Unknown quota recovery.
6. Retain commands/exits/source/image/config/compiler/device/decoder/public-verifier pins as applicable, all successful and refused runs. Update affected caller/tests/docs in the same source cutover. Actual implemented-delta review and relevant CP2 remain required before freeze/promotion.

## Rollback and expiry

Disable private G1 selection and revert only slice-created source/config changes; keep failure receipts. Fresh disposable images may be discarded. Never use binary rollback/old autoformat runtime to recover retained data: close admission and preserve bytes/obligations for forward recovery. This authority expires on candidate invariant/path/representation changes outside this scope; submit an exact amendment, do not stretch it. No irreversible real ledger/key/effect/floor operation is authorized.
