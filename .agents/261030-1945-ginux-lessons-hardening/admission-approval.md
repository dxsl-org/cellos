# Ginux-lessons admission + Phase 04 lock handoff

Date: 2026-10-31. Approver: accountable owner (the user), answering the admission question for [plan.md](plan.md) invoked through `$hc-cook ... --auto`:

- **Admission:** "Chỉ Phase 01 + 04". Only Phase 01 (governance/docs) and Phase 04 (x86 entry safety) are admitted. Phases 02, 03, 05–09 stay `proposed`; 09A is not admitted and its Law-1 review is not requested.
- **WIP:** "Chờ CP1 xong". The Cellbox plan whose Phase02 CP1 was the single current controlling slice closed 2026-10-31 (`261020-1750-cellbox-cli/plan.md` status `closed-at-stated-ceilings-with-deferred-items`). Phase 04 starts after that closure, as the one implementation slice of this plan.
- **SMEP/SMAP/UMIP:** "Bật theo profile". Each protection is a separate build profile (cargo feature), CPUID-gated at boot, default off. A profile is called qualified only with positive and negative evidence on its own image; no Tier-1 SAS path may gain a copy or CR3 switch to make it pass.

## Phase 04 file-bounded exception

Intel C2C link: node kernel integrity on Intel x86-64 entry paths, all tiers (plan Phase 01 table). Owner: HAL x86, with the current assistant session as the single integrating owner. No `libs/api`, `libs/types`, `Cargo.lock`, manifest or signing change; no Law-1 delta (no ABI bytes or public semantics change: a syscall caller cannot observe DF/AC of the kernel).

Admitted paths:

- `hal/arch/x86/src/x86_64.rs`, `hal/arch/x86/src/x86_64/{gdt,idt,syscall}.rs`, new `hal/arch/x86/src/x86_64/ist.rs`
- `hal/arch/x86/src/x86_64/idt/{entry,policy,dispatch,probe,probe_entry,cpl3_probe,cpl3_entry}.rs`, new test-only `hal/arch/x86/src/x86_64/idt/probe_ist.rs`
- `hal/arch/x86/Cargo.toml`, `kernel/Cargo.toml` (features only)
- `kernel/src/main.rs` (one x86 call that guards IST stacks after paging, before secondaries)
- `kernel/src/task/user_copy/**` (SMAP access window only, feature-gated)
- `scripts/qemu-x86_64-idt-test.sh`, `scripts/build-x86_64-idt-test-ci.sh`, new `scripts/x86/qemu-idt-protection-test.sh`
- `CHANGELOG.md`, `docs/` x86 entry notes, this plan directory

Retained gates: fatal NMI/#DF/#MC policy unchanged; resumable NMI not admitted; real-MCE and physical Intel evidence out of reach (QEMU/TCG ceiling, KVM when available); all 38 lock relations unchanged. Every other HAL/kernel path stays under its existing owner.

## Deviation log

- 2026-10-31: `hal/arch/x86/src/x86_64/paging.rs` added: `phys_to_virt_ptr` visibility private → `pub(crate)` so `ist.rs` can walk the root to unmap guard pages. No behavior change.
- 2026-10-31: baseline IDT fixture triple-faulted before any Phase 04 edit (`ldmxcsr` #UD in `x86_64_idt_common`: probes run inside `idt::init` before `init_sse`). Fixed in-slice by calling `init_sse` first in `kernel/src/main.rs` (admitted file).
- 2026-10-31: domain (`build-x86_64-domain-test-ci.sh`) and C2C lifecycle lanes refuse to build at baseline: `ROLLOUT BLOCKED: unqualified Cellbox/terminal artifact` (Cellbox rollout containment, other owner). Not usable as regression lanes in this slice.
- 2026-10-31: `hal/core/Cargo.toml` added: forwards `x86-smep`/`x86-smap`/`x86-umip` from the kernel to `hal-x86` (features only, same pattern as `x86-idt-cpl3-test`).
- 2026-10-31: the baseline CPL3 half of the IDT fixture also fails (HEAD + `init_sse` fix only: `X86-IDT-SELFTEST: FAIL` after spawning A/B). Cause: commit `775686575` gated `yield_cpu` on `X86_BOOT_COMPLETE`, set only in `start_secondaries`, but the fixture runs inside `task::init`. Fixture-only fix (cfg `x86-idt-cpl3-test`): `kernel/src/task/smp.rs` gains `x86_idt_fixture_uniprocessor_ready()`, called from `kernel/src/task/x86_idt_cpl3.rs` before the first yield. Production builds are unchanged.

## Admission 2 (2026-10-31)

User selected all remaining phases: 09A, 02, 03, 05, 06, 07, 08, 09B, 09C. WIP stays one implementation slice; order 09A → 07 → 08 → 09B → 09C with 02/03/05/06 between kernel slices. Each slice records its exact file set here before editing. 09A changes stash syscall semantics: it needs a Law-1 CP1 approval of an exact design text before code; selecting the phase is not that approval.

## Slice 07A (2026-10-31)

User chose "07A: #3 + viết CP1 cho #1/#2" after the Phase 07 census (`agent://AuthorityCensus`). No Law-1 delta in code: kernel-internal visibility only.

File set: `kernel/src/cell/cspace.rs` (`allocate_slot` private; new `pub(crate) mint_kernel_root` with issuer Kernel and holder = the authority's own incarnation; `allocate_file_slot` loses its `holder` parameter), `kernel/src/task/syscall.rs` (OpenCap call site), `kernel/src/task/cap_file_selftest.rs`, `CHANGELOG.md`, this plan directory (new `phase-07a-cp1.md` design draft for derive destination admission / delegability / issuer preservation — awaiting approval, no code).

## Slice 07B (2026-10-31)

User: "xóa tests/unit_cspace_smoke.rs rồi tiếp tục CAP_TABLE". Census #4 re-checked: nothing allocates into the legacy `CAP_TABLE` (no `alloc`/`alloc_with_lease` caller in kernel, libs, cells, tests or tools); its only use is `revoke_all_for` at retirement (`kernel/src/task.rs:1469`), a no-op on an always-empty table. Clean cutover: delete `CapTable`/`CapEntry`/`CapResource`/`CAP_TABLE`, keep the CSpace file-backing helpers. Kernel-internal, no ABI or Law-1 delta.

File set: `tests/unit_cspace_smoke.rs` (deleted, never compiled), `kernel/src/cell/cap_registry.rs`, `kernel/src/task.rs`, `CHANGELOG.md`, this plan directory.

## Slice 07C (2026-10-31)

User: "kiểm tra hoàn thành 3 việc còn lại của phase 07". (a) Private 1/2-hart authority witness (Phase 07 step 3/4, test-hooks only); (b) signed-boot-manifest ADR, status Proposed (step 5); (c) receive-path CP1 draft (design only; Law-1, no code without approval).

File set: new `kernel/src/cell/provenance_selftest.rs` (cfg `test-hooks` + riscv64), `kernel/src/cell/mod.rs` (module line), `kernel/src/main.rs` (primary call after the TLB fixture), `kernel/src/task/smp.rs` (secondary call after the TLB fixture), new `tests/integration/tests/authority-provenance.rs`, `tests/integration/Cargo.toml` if tests need registration, new ADR under `docs/adr/`, new `phase-07c-receive-cp1.md`, `CHANGELOG.md`, this plan directory. Production (non-test-hooks) builds unchanged.

## Slice R1 (2026-10-31): RV64 kernel stack overflow diagnosis

User: "xử lý rủi ro trap entry trước". A kernel stack overflow makes every S-mode trap fault again while saving its frame, so the eventually-reported fault carries `sepc` inside the trap save sequence and misleading satp/stval. Fix: classify that signature in the Rust kernel-exception path and panic with an explicit "kernel stack overflow" message. HAL/kernel internal, no ABI or Law-1 delta.

File set: `hal/arch/riscv/src/rv64/asm/trap.S` (export a save-end label), `hal/arch/riscv/src/rv64/trap.rs`, a test-hooks overflow fixture if needed for evidence, `CHANGELOG.md`, this plan directory.

- 2026-10-31 R2: per-hart overflow stack in `hal/arch/riscv/src/rv64/asm/trap.S` + `trap.rs` (`vi_kernel_stack_overflow`); same file set as R1.

- 2026-10-31 Phase 08A (CP1 phase-08a-cp1.md approved by user: typed block outcome 0x00/0x01/0x02): libs/types (block_wire), cells/drivers/{ahci,nvme,virtio-blk}/src/{dispatch,controller,main,device}.rs, cells/services/vfs/src/{blk_router,block_stream,page_cache,disk_cellosfs,backend_fat}.rs, AHCI Cargo.toml test-hooks feature, tests/integration (08A witness), CHANGELOG.md.

- 2026-10-31 Phase 08B (CP1 phase-08b-cp1.md approved by user): kernel/src/task/drivers/{pcie_ecam,irq_wait}.rs, kernel/src/task.rs (teardown_dma_authority, reap_retired_task_resources), kernel host tests for both, CHANGELOG.md.
- 2026-10-31 Phase 08B file set amended during implementation: kernel/src/resource_registry.rs (bdfs_owned_by), kernel/src/task/thread_cap_selftest.rs (DRIVER_BDF aliased the real host bridge 00:00.0), cells/drivers/ahci/src/fault_inject.rs (F08B-DIE), tests/integration/tests/ahci-x86.rs (08B witness).
- 2026-10-31 Phase 08C (CP1 phase-08c-cp1.md approved by user with NVMe added): cells/tools/init/src/*.rs (block driver restart), cells/drivers/{ahci,nvme}/src/*.rs (reset proof logs), kernel host test (non-reuse), tests/integration/tests/{ahci-x86,nvme-x86}.rs, scripts/x86/make-ahci-fault-iso.sh, cells/drivers/nvme fault-inject, CHANGELOG.md.
- 2026-10-31 Phase 08C amended: kernel/src/task/{scheduler,syscall}.rs (masked Recv completes on peer death; user chose the kernel fix, phase-08c-ipc-cp1.md), cells/services/vfs/src/blk_router.rs (reply only while sender is the registered BLOCK_DRIVER).
