# Scout report — ginux lessons for Cellos

Source repo: `https://github.com/gmkurtzer/ginux`, shallow clone at `/tmp/ginux`, commit `1852455` (2026-10-29), Apache-2.0. Ideas are adopted, not code; any code port needs a `hc-cop` license pass.

Full scout transcripts: `agent://ScoutX86Entry`, `agent://ScoutDeviceRecovery`, `agent://ScoutAuthorityAbi`, `agent://ScoutDisciplineStorage`. This is historical reconnaissance: citations are PRIOR until rechecked against current source, not fresh runtime evidence. The reviewed `plan.md` and phase files control implementation; Ginux's fresh-address-space and sole-root assumptions are not Tier-1 SAS requirements. Live stash authority enforcement must cover the syscall-used unversioned table, not only the versioned helper.

## Per-item state

| # | Ginux idea | Cellos today | Gap | Owner area |
|---|---|---|---|---|
| 1 | Guarded IST stacks for NMI/#DF/#MC; stack-safe SYSCALL; recoverable user copy; SMAP balanced | `idt.rs:51-60,80-87` sets `ist: 0` for every gate; TSS only `rsp0` + opaque rest (`gdt.rs:23-38`); SYSCALL does `swapgs; mov %rsp,%gs:8; mov %gs:0,%rsp` (`syscall.rs:229-234`); vectors 2/8/18 go to the fatal halt (`idt/policy.rs:45-60`, `idt/fatal.rs:16-29`); no SMEP/SMAP/UMIP/`stac`/`clac` anywhere under `hal/arch/x86/src/x86_64`. User copy already has range checks, per-page PTE walk, and a per-hart #PF landing pad (`kernel/src/task/user_copy/{range,copy,guard}.rs`, `idt/dispatch.rs:19-43`). Non-native-domain fallback still builds raw slices (`copy_glue/mod.rs:229-245,280-290`) | IST + guards, NMI handling that is safe in the entry window, SMEP/SMAP/UMIP. F2 is mostly present | HAL x86 (no direct lock); `kernel/src/task/**` is P02–P06 serialized |
| 2 | Prepared vs notified device timeout; ordered quiescence: MSI mask → bus master off → reset → IOMMU revoke → scrub/release → fresh generation | IPC already splits `PRE_DISPATCH_TIMEOUT` and `INDETERMINATE` (`async_ipc.rs:524-542`, `ostd/src/ipc.rs:41-47`). Retirement funnel is IOMMU revoke → BDF release → acknowledged-frame release (`task.rs:1805-1824`). PCI has only `enable_bus_master` (`pcie_ecam.rs:688-728`). No MSI mask or reset on teardown | Device-level state and the first three quiescence steps | P05 (device/buffer revoke), P04 (operations) |
| 3 | Replacement = fresh generation/CSpace/AS/DMA lease; post-fault test recovers exact committed record | Identity `(CellId, generation)` (Spec 25:18-29); hotswap is generation-fenced routing (`hotswap.rs:380-438`); `state_stash::restore_versioned` does not authenticate caller identity/generation (`state_stash.rs:204-212`); `native_stateful` kills VFS, not the faulted provider, and is RV64 (`native_stateful.rs:128-260`) | Faulted-provider replacement test on x86; stash caller authentication | P06 |
| 4 | Signed boot manifest is the only authority root; runtime caps ⊆ manifest; observer/service split | `boot_ceiling` is a hard-coded kernel table (`loader/boot_ceiling.rs:1-97`); spawn intersects root/ceiling/spawner/policy (`governed_spawn.rs:114-150`); CSpace has lineage/attenuation/revoke (`cell/cspace.rs:400-770`); legacy `CapTable::alloc` has no provenance (`cell/cap_registry.rs:65-104`); no envelope test exists | Provenance invariant + test; legacy CapTable provenance; signed boot manifest is a Law-1 decision | P02/P05; lock blocks new manifest-authority interpretation (`cell-contract-0024-locks.md:158-162`) |
| 5 | ABI records: magic/version/size/features/reserved=0; injected bad record fails before activation | `CellManifest` has magic/version/flags/reserved, strict parse (`manifest_parse.rs:19-85`); validation precedes signature, caps and task creation (`governed_spawn.rs:43-59,103-126`); unit tests cover version/reserved mutations (`manifest_compat_tests.rs:124-165`). Other records are inconsistent (`completion.rs`, `telemetry.rs`, `ViFstatV1`) | Written convention for new records; an end-to-end x86 pre-activation refusal test | P01 ABI census; Law-1 for any layout change |
| 6 | `require_text`/`reject_text` doc checker; syscall ledger cross-checked against dispatcher | `check-spec-anchors.py --check` in CI (`ci.yml:62-66`), anchors only; ADR-0018 wants a published POSIX symbol table + divergence test (`0018-*.md:120-139`), not built | Obsolete-claim checker; POSIX ledger + cross-check | P07 for the ledger; tooling unowned |
| 7 | Two-slot record, prepare/flush/commit/flush, SHA-256, newest unambiguous generation, never reformat | CellosFS A/B superblock + CRC32C (`format.rs:17-137`, `engine.rs:79-119`); runtime never formats (`vfs/src/manager.rs:68-70`); `sync_durable` always `DurabilityUnavailable` (`engine.rs:844-859`); plan `261025-cellosfs-crash-safety` is `planned` and owns this | Already owned elsewhere: feed inputs only | 261025 plan / P06 |
| 8 | Generated inventory of writable globals by owner class; CI rejects unclassified | Policy `static mut` only via `Spinlock<Option<T>>` (`specs/00-context.md:223`) is already contradicted: 24 `static mut` in `kernel/`, 15 in `hal/`; no inventory or CI gate | Inventory tool, classification, drift gate | P02/P03 (SMP prerequisite; `MAX_CPUS=2`) |
| 9 | Perf budget enforced only on KVM; TCG report-only | Bench is RV64 TCG only (`bench_results.py:23-26,780-817`, `perf.yml:92-164`); std-parity evidence is a synthetic fixture (`run-std-parity-benchmark.sh:29-41`); no accelerator detection | x86 q35 profile with accelerator field; KVM-only budget | P10 |

## Reusable gates (found, not run)

- x86 entry: `bash scripts/build-x86_64-idt-test-ci.sh && bash scripts/qemu-x86_64-idt-test.sh`
- x86 domain/copy: `bash scripts/build-x86_64-domain-test-ci.sh && bash scripts/x86/qemu-domain-test.sh`
- Lifecycle: `bash scripts/build-x86_64-c2c-lifecycle-ci.sh` + `cargo test --manifest-path tests/integration/Cargo.toml --target x86_64-unknown-linux-gnu --test local-service-lifecycle-x86`
- Hostile devices: `bash scripts/qemu-tier3-hostile-runner-x86.sh build/vicell-x86-virtio-hostile.iso`
- Manifest: `python3 scripts/validate-manifest-abi-predesign.py`, `bash scripts/qemu-manifest-continuity.sh`
- Docs: `python3 scripts/check-spec-anchors.py --check`; bench: `scripts/compare-bench-results.sh`, `python3 scripts/test_compare_bench_results.py`
- Baseline: `./scripts/check-baseline.sh`

## Governance constraints

- Portfolio admission: every task must name an Intel C2C deliverable, prerequisite, or regression and its evidence ceiling (`plan-portfolio.md`). Plans not admitted are parked.
- `CELL-CONTRACT-0024`: 38 locks; `kernel/src/task/`, `kernel/src/cell/`, and the loader have one serialized owner (controlling plan §Ownership). WIP is one implementation slice.
- Law-1: any `libs/api` / `libs/types` change needs two maintainer checkpoints (`docs/code-standards.md:28-52`).

## Observed inconsistency (not fixed here)

`README.md:11,69-76` says phases P02–P10 are "Selected; pending", while the controlling plan frontmatter says `status: complete` / `completed: 2026-10-16` and the portfolio says all 10 phases executed. Phase 06's own success criteria and Spec 25 AC-01..AC-05 are still marked pending. This is exactly the drift Phase 05 targets; the maintainer must decide which side is true.

## Precedents

- `.agents/260607-1543-x86-hal-bringup/phase-05-sysret-idt-hardening.md:46-67,108-161`: proposed #DF IST and deferred SMEP/SMAP; still `TODO`, historical/parked (lock register `:178`). Evidence only.
- `.agents/260902-x86-per-vector-idt/plan.md:18,47-48`: completed; explicitly kept IST=0.
