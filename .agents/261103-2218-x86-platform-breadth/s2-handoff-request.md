# Slice S2 (phases 06, 07a, 07b) — lock-handoff request

Status: **draft for owner approval. Not effective.** It takes effect only after slice RE releases the Intel WIP lane (`.agents/cell-contract-0024-locks.md:174`) and the owner answers §6. Written 2026-11-04 against HEAD `28dee8b3d`. Line numbers are HEAD lines. The working tree differs from HEAD only in slice-RE files (`kernel/src/task{.rs,/elf_prepare.rs}`, `kernel/src/loader/{boot_ceiling.rs,launch_profile/targets.rs}`, `kernel/Cargo.toml`, `libs/ostd/src/startup.rs`). `[INFERENCE]` marks claims that were not read in code.

## 1. Phase 06 step 1 — recon result

- **Shell command table.** The shell is `cellbox`:
  - builtin name list: `cells/tools/cellbox/src/executor.rs:1100`;
  - dispatch: `executor.rs:2518` → `cmd_sys::cmd_shutdown` (`cmd_sys.rs:108-115`), which refuses and prints "use /bin/shutdown";
  - ledger rows: `ledger.rs:107,136,152`, `oneshot.rs:82`, `typed.rs:113`.
- **Existing power path:**
  - the retained worker `cells/tools/sys-tools/src/bin/shutdown.rs:41` issues raw opcode 502 (`libs/ostd/src/syscall.rs:217-236`);
  - the kernel accepts 502 only from task name `shutdown` (`kernel/src/task/syscall.rs:2055-2073`);
  - **on x86 the 502 arm only loops on `hlt`** (`syscall.rs:9118-9123`). The comment at `tests/integration/tests/ahci-x86.rs:270-272` relies on this;
  - launch edge: shell, Path route only (`kernel/src/loader/launch_profile/profiles.rs:66-69`, test `tests.rs:500-526`).
- **Supervisor cell:** `cells/services/supervisor/src/{main.rs,protocol.rs}`.
  - Opcodes are hotswap/snapshot/status only (`protocol.rs:12-20`).
  - `AppEvent::Shutdown` makes the supervisor itself exit (`main.rs:119-121`).
  - **There is no stop-all and no VFS-flush path**, so phase 06 assumption 1 (low confidence) is false.
  - `SupervisorCap` is held by `/bin/supervisor` (`kernel/src/loader/boot_ceiling.rs:136-139`) and by init (`:73`). Gate: `caller_has_supervisor` (`syscall.rs:2194-2196`). CP1 cites `:2174-2176`, which has drifted.
- **VFS:** no sync-all message exists.
  - Per-cap fsync exists (`libs/ostd/src/fs.rs:185-188`).
  - FAT `sync` flushes one device (`cells/services/vfs/src/backend_fat.rs:443-445`).
  - CellosFS flushes at every mutation (`backend_cellosfs.rs:687-717`).
  - VFS does not handle `AppEvent::Shutdown` (no match in `cells/services/vfs/src`).
- **Power cell:** `cells/services/power/src/lib.rs:1-35` (traits only). Its only references are the workspace member (`Cargo.toml:121`) and the rpi3 disk scripts (`scripts/make-rpi3-disk.sh:66`, `gen_disk_rpi3.ps1:183`). No Rust consumer.
- **ACPI inputs exist** from phase 01: `FadtInfo` fields `sci_int`, `smi_cmd`, `acpi_enable`, `pm1a/b_cnt`, `reset_reg`, `dsdt`, `sleep_control` (`kernel/src/acpi/fadt.rs:12-32`), and `reset_supported()`/`hw_reduced()` (`:50-56`). SCI routing can use `apic::ioapic_redirect` (`hal/arch/x86/src/x86_64/apic.rs:325-334`, honours MADT overrides). MSI vectors are `0x30..=0xDF` (`idt/policy.rs:59`).
- **Delivery precedent:** `ipc_post_nonblock` takes `SCHEDULER` (`kernel/src/task.rs:2633`), so the SCI ISR cannot post. Existing precedent: the x86 timer tick relays console bytes on hart 0 (`task.rs:1083-1086`).
- **Test harness:** every x86 runner passes `-no-reboot` (`tests/integration/src/lib.rs:767,832,905,978,1055,1157,1238,1408,1472,1612`; `scripts/x86/qemu-domain-test.sh:107`), so `reboot` makes QEMU exit. QMP is available in `boot_x86_bios_with_accel` (`lib.rs:950-1000`). The CI job is `qemu-x86_64-boot` (`.github/workflows/ci.yml:909-1087`). Test targets are auto-discovered (`virtio-x86` is not listed in `tests/integration/Cargo.toml` but runs at `ci.yml:1033`).

## 2. Exact file set (C = Create; everything else exists in HEAD)

**Phase 06: reset, S5, power button, `SystemPower = 436`**

| Path | Scope |
|---|---|
| `kernel/src/acpi/power.rs` (C) | reset chain (FADT reg → `0xCF9` → 8042 → triple fault); static `\_S5_` scan of DSDT/SSDTs; PM1 / `SLEEP_CONTROL_REG` write |
| `kernel/src/acpi/sci.rs` (C) | enable ACPI mode, `PWRBTN_EN`; ISR acks PM1 status and sets an atomic; `relay_pending()` |
| `kernel/src/acpi/mod.rs`, `kernel/src/acpi/tests.rs` | module declarations, table access for the scan; `\_S5_` unit tests |
| `tests/fixtures/acpi/` (C) | captured q35 DSDT; a DSDT with `\_S5` inside a method |
| `kernel/src/main.rs` | x86 ACPI init block `:435-450` only (one `sci::init` call) |
| `kernel/src/task.rs` | one x86 hart-0 call `acpi::sci::relay_pending()` after `:1086` (inside `vi_timer_tick`); nothing else |
| `kernel/src/task/syscall.rs` | append-only 436 arms: enum after `:4314`, allowlist variant map after `:4879`, dispatch after `:8885`, decode after `:10429`; unit test "436 refused without `SupervisorCap`". The raw-502 code (`:2055-2079`, `:9088-9134`) stays unchanged |
| `hal/traits/arch/src/kernel_abi.rs` | declare `vi_handle_sci` beside `:155` (HAL→kernel hook, not a cell ABI) |
| `hal/arch/x86/src/x86_64/idt/{policy,dispatch}.rs` | SCI vector class (outside `0x30..=0xDF`) and its arm |
| `libs/api/src/abi/syscall.rs` | `SystemPower = 436` after `:698`; add to the bit-49 group `:1199-1203`; `436 =>` decode after `:1453` |
| `libs/api/src/abi/syscall_tests.rs`, `libs/ostd/src/syscall.rs` (append after `:3039`) | number/bit tests; `sys_system_power` |
| `cells/services/supervisor/src/{main.rs,protocol.rs}`, `cells/services/supervisor/src/power.rs` (C) | `OP_POWER` handler and bounded orderly shutdown. `snapshot.rs` and `hotswap.rs` are not touched |
| `cells/tools/sys-tools/src/bin/shutdown.rs` | x86: send `OP_POWER` (poweroff, `-r` reboot) to the supervisor; riscv64/aarch64 path unchanged |
| `tests/integration/tests/power-x86.rs` (C), `tests/integration/src/lib.rs` | power lanes; runner variant without `-no-reboot`, plus QMP `system_powerdown` |
| `.github/workflows/ci.yml` | job `qemu-x86_64-boot` (`:909-1087`) only |
| `docs/abi-boundary-records.md`, `CHANGELOG.md`, this plan's `phase-06-*.md`, `cp2-06-system-power.md` (C), `reports/phase-06-evidence.md` (C) | records |

Not in the set: `cells/services/power/**` (H-S2-9); `cells/tools/cellbox/**`, `launch_profile/**`, `boot_ceiling.rs` and image/signing scripts (H-S2-6 option a needs no new binary or launch row); `cells/services/vfs/**` (H-S2-8 option i).

**Phase 07a: per-CPU structures for N CPUs (behaviour-preserving at 2)**

| Path | Scope |
|---|---|
| `kernel/src/task/smp.rs` | per-arch `MAX_HARTS` (`:16`); array literals `:24,34-35,154-156,185`; mapping `:281-300,314-317` → topology table; `x86_secondary_main` `assert!(logical == 1)` `:977` |
| `kernel/src/task/smp/topology.rs` (C) | `CpuTopology` (logical ↔ physical), written once before APs start |
| `kernel/src/task/hart_local.rs` | `:42-43,67-68` (aarch64 map), `:331-332` literal, `:341-344` |
| `kernel/src/task/hart_local/ready.rs` | `:19-23`, `CLAIMS` `:116` (size audit) |
| `hal/arch/x86/src/x86_64.rs` (`:264`), `x86_64/{syscall,apic,gdt,ist}.rs` | `MAX_CPUS` (`syscall.rs:58,78,96`) becomes the HAL source of `MAX_HARTS`; `apic.rs:25-26` packs 2×u32 into one `AtomicU64`, which becomes an array; `apic.rs:74-104`, `gdt.rs:92-99`, `ist.rs:19-78` |
| `hal/arch/riscv/src/critical_section.rs:14` | doc only |
| `.agents/…/phase-07a-*.md`, `reports/phase-07a-evidence.md` (C) | records |

Already `[…; MAX_HARTS]` repeats, so no edit is expected:
- `task.rs:570-659,1340`;
- `scheduler.rs:265,1533,3010`;
- `telemetry.rs:76,85,279,475-478`;
- `deferred_release.rs:70,297-307`;
- `p01_owner.rs:16`;
- `domain_switch_tests.rs:417-418`.

`kernel/src/task/cpu_accounting.rs:8,19` (G1 lane: `M = 2` is asserted equal to `MAX_HARTS`) stays byte-identical through a `cfg(cellos_g1_cpu)` value of 2 in `smp.rs` (H-S2-5). `kernel/src/task/quiesce.rs` stays unchanged: it fails closed at more than 8 online harts (`:55,73-75`), and `PARK_REQUEST.get(hart)` is bounds-checked (`:517-519`).

**Phase 07b: N-CPU scheduling on x86**

| Path | Scope |
|---|---|
| `kernel/src/task/smp.rs` | x86 `start_secondaries` `:825-973` (all MP CPUs ≤ `MAX_HARTS`, `cpu N ignored` log); AP entry `:976+`; set-based retirement/preempt waits; fixtures `:1150-1251` (`harts=2` markers) |
| `kernel/src/memory/tlb_shootdown.rs` | `await_remote_invalidation` `:231-260` (serial per-hart wait → broadcast, then wait over the set, fail-stop kept); `:296-322`. **Missing from the phase 07b file list; added here** |
| `kernel/src/task/hart_local/ready.rs` | `steal_from_busiest_with` `:710-810` (bounded victim scan, RT excluded, lock order by hart id). The signature is kept, so `scheduler.rs:2586-2609`, including the G1 arm, stays byte-identical |
| `hal/arch/x86/src/x86_64/clock/{kvmclock,mod}.rs` | per-AP pvclock area (today one `init(virt, phys)`, `kvmclock.rs:51`) |
| `scripts/x86/qemu-domain-test.sh` (`:102,277`), `tests/integration/src/lib.rs`, `tests/integration/tests/x86_64-boot.rs`, `.github/workflows/ci.yml` (job `qemu-x86_64-boot`) | `online=N` gate; `-smp 4` lane |
| `cells/apps/c2c-render/src/main.rs:462-485`, `scripts/x86/run-c2c-render-smoke.py`, `scripts/x86/run-c2c-concurrent-input-smoke.py` | `ProcessorIds` bound; smoke runs at `-smp 4` |
| `docs/specs/04-hardware.md` §5, `CHANGELOG.md`, phase-07b file, `reports/phase-07b-evidence.md` (C) | records |

## 3. Overlap analysis

| # | Against | File | Lines / symbols | Resolution |
|---|---|---|---|---|
| O1 | Slice RE (holds the lane) | `kernel/src/task.rs` | RE: `prime_user_mode_entry` (working-tree hunks at HEAD `:170-180`); S2: one call after `:1086` | Disjoint. S2 starts only after RE closes (single-WIP rule), rebased on RE's commit |
| O2 | Slice RE | `launch_profile/{profiles,targets,tests}.rs`, `boot_ceiling.rs`, signing/image rows | none in S2 under H-S2-6 (a) | Options (b) and (c) would collide. Recommend (a) |
| O3 | 04b §6 (approved, not effective) | `kernel/src/task/scheduler.rs` | 04b: `:2223-2226`, `:2303-2318`; S2: none (07b keeps the steal signature; 07a needs no edit at `MAX_HARTS ≤ 64`) | Disjoint. If H-S2-5 picks 256, `:265-267,2028,3011` change and remain disjoint from the 04b lines |
| O4 | 04b §6 | `kernel/src/task/syscall.rs` | 04b: `WaitCompletion` decode `:10616-10619`; S2: 436 arms (§2) | Disjoint hunks; the second lander rebases |
| O5 | 04b §6 | `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs` | 04b: 242 doc `:751-783`; S2: after `:698`, `:1199-1203`, after `:1453`; ostd append | Disjoint. 04b's Law-1 digest gate (`scripts/check-multi-source-wait-law1-digests.sh:101-103,133-136`) must stay green: S2 touches none of its items (`task.rs:346`, `scheduler.rs:2308`) |
| O6 | 04b §6 | `tests/integration/tests/ahci-x86.rs`, `CHANGELOG.md` | 04b converts ahci; the 06 comment `:270-272` goes stale | S2 does not edit `ahci-x86.rs`; the stale comment is logged in phase 06's Deviation Log. CHANGELOG is append-only |
| O7 | 04b, specs and digest script | `docs/specs/{17,03b,20}*.md`, digest script, `completion*.rs`, `libs/ostd/src/ipc.rs` | none | No overlap |
| O8 | Kernel-repair Phase 07 (in progress, locked; `260927-0739-…/phase-07-warm-snapshot.md:17`: `syscall.rs`, `main.rs`, `scheduler.rs`/`smp.rs` quiescence, `vi_timer_tick` hook, `supervisor/src/snapshot.rs`) | `syscall.rs`, `main.rs`, `task.rs`, `smp.rs` | 06: `syscall.rs` 436 arms, `main.rs:435-450`, `task.rs` after `:1086`; 07a/07b: `smp.rs` (whole file; no snapshot-quiescence code: `git grep park\|quiesce` matches only the boot park loop and the retirement-epoch doc `:26`; the park protocol lives in `quiesce.rs`), `quiesce.rs` pinned, `snapshot.rs` pinned | Line-bounded grant as for slices d/e (`cell-contract-0024-locks.md:138,147`), H-S2-3 |
| O9 | Locked row `260808-1544-wx-cross-hart-tlb-shootdown` (`locks.md:200`, "cross-hart permission retirement and reuse gates") | `tlb_shootdown.rs`, `smp.rs`, `deferred_release.rs` | 07b set-based shootdown / retirement waits | Narrow the row for 07a/07b only. Semantics stay: fail-stop on an unconfirmed tag (`tlb_shootdown.rs:132`), no new reuse rule (H-S2-4). Phase 07b already demands this handoff (`phase-07b-*.md:82`) |
| O10 | iGPU condition 2 (`locks.md:167`) | `kernel/src/task/{syscall,task}.rs`, `libs/api/src/abi/syscall.rs` (`iommu*` untouched by S2) | 06 holds these three | S2 releases them at S2 close (or at CP2-06 if H-S2-1 (c)). Condition 3 (04a, 04b, 12) is the binding gate in any case (H-S2-10) |
| O11 | iGPU CP1 §8 and large-memory CP1 (approved, not effective) | `libs/api/src/abi/syscall.rs`, `libs/ostd/src/syscall.rs`, `tests/integration/src/lib.rs`, `scripts/x86/qemu-domain-test.sh` | 437-440 append next to 436; domain-test cases | 436 lands first by number order. The large-memory A5 SMP lane builds on 07b's shootdown, so 07b goes before large memory [INFERENCE] |
| O12 | Runtime R5 XSAVE (`hal/arch/x86/**`, needs the lane; `locks.md:172`) | `hal/arch/x86/src/x86_64{.rs,/{syscall,gdt,ist,apic}.rs}` | 07a per-CPU tables | Sequential under single-WIP. Whichever lands second rebases. The owner orders R5 against 07a |
| O13 | Apple 02c (`261104-0711-…/phase-02c-*.md:32,43,67`) | `smp.rs`, `hart_local.rs` | Apple waits for 07a | No conflict: 07a keeps aarch64 at 2, and Apple raises it afterwards |
| O14 | Rows `260624-0630` ("kernel-versus-Driver-Cell ownership and MMIO/DMA/IRQ/registry authority migration", `locks.md:187`), `260712-0800` ("snapshot, restart and replacement authority followups", `locks.md:190`), `261004-1957` (device authority, `locks.md:215`) | `acpi/sci.rs`, supervisor `OP_POWER` | SCI stays kernel-owned (no cell gains IRQ/MMIO); the supervisor gains a power request, not snapshot/restart/replacement authority | Rule compatible (H-S2-2); owner confirms |

## 4. Law-1 items

**4.1 Proposed handoff text for 436 (inventory entry):** "Bounded adopted slice: x86 platform breadth, slice S2 (phases 06, 07a, 07b). File-bounded exception for exactly the §2 paths of `s2-handoff-request.md`. Planned Law-1 delta: `SystemPower = 436` as approved at design level in CP1 §5 (D-4): `a0 = 1` reboot, `a0 = 2` poweroff, `a1..a5 = 0`; gate `SupervisorCap`; allowlist bit 49 shared; no return on success; errors `InvalidInput`, `PermissionDenied`, `NotSupported`, with sentinel values fixed at CP2. CP2-06 is required after phase 06 and before 07a starts. 07a/07b carry no Law-1 delta. Kernel-repair Phase 07 overlap granted for the lines named in O8 only. Row `260808-1544` narrowed for 07a/07b shootdown and retirement waits. Pinned byte-identical: `quiesce.rs`, `cpu_accounting.rs`, `snapshot.rs`, `scheduler.rs` (unless H-S2-5 = 256), raw opcode 502, `cells/services/vfs/**`. The G1 private lane is untouched. All 38 relations keep their whole-plan scope."

**4.2 Points CP2-06 must settle:**
- Phase 06 says `a0 = 0 reboot | 1 poweroff` (`phase-06-*.md:42`). The approved CP1 says 1/2 (`cp1-pcie-irq-power-abi.md:96`). The CP1 governs, and the phase text is corrected under S2.
- Supervisor-private `OP_POWER` (`protocol.rs`, not `libs/api`) and the kernel→supervisor power-button message (sender 0): recorded as non-Law-1 cell-local wire unless the owner rules otherwise (H-S2-7).
- Raw 502 is unchanged on every architecture.

**4.3 07a/07b change no ABI.** Checked:
- neither phase lists a `libs/api`/`libs/types` path;
- `TelemetryHeaderV1.online_cpus: u32` (`libs/api/src/abi/telemetry.rs:118`) and `ostd::system_info::processor_id()` (`c2c-render/src/main.rs:688`) are unaffected.

**One semantic edge:** `ViRetirementV1.remote_hart_mask: u64` documents "bit h set" (`libs/api/src/abi/retirement.rs:68-69`). `scheduler.rs:2028` drops harts ≥ 64 through `checked_shl(..).unwrap_or(0)`. With x86 `MAX_HARTS = 256`, that public record would under-report. Capping x86 at 64 (H-S2-5) keeps the bytes and the documented meaning exact.

## 5. Sequencing options

| Option | Order | Unblocks first | Cost |
|---|---|---|---|
| (a) 04b before S2 | 04b → S2 (06 → CP2-06 → 07a → 07b) | 08, 11, 14-19, iGPU condition 3 (with 12), via 08 → 09 | Power and SMP later; KVM nodes stay at 2 CPUs |
| (b) S2 before 04b | S2 → 04b | 06 → 09's shutdown step (09 still needs 08 ← 04b); 07a → Apple 02c; 20 (still needs 09-12) | Delays the largest fan-out; nothing S2 unblocks can finish without 04b |
| (c) split | 06 → CP2-06 → 04b → 07a → 07b | ACPI shutdown for `virsh`/VMware early; 436 frozen before 437-440 | Two lane switches |

**Recommendation: (a).**
- 04b's handoff is already approved, and its CP1 is confirmed.
- It unblocks nine phases and iGPU condition 3.
- Every S2 downstream (09, 20) also waits on 04b.
- 04b and S2 are file-disjoint at the line level (O3-O7), so order costs only a rebase.
- Choose (c) if early ACPI shutdown on KVM/VMware matters more than NIC breadth.

## 6. Owner decision checklist

| ID | Question | Recommendation |
|---|---|---|
| H-S2-1 | Order (a), (b) or (c), §5 | (a) |
| H-S2-2 | Approve the S2 file-bounded exception for exactly §2, effective when RE closes (and after 04b under (a)) | Yes |
| H-S2-3 | Kernel-repair Phase 07 overlap: `syscall.rs` 436 arms, `main.rs:435-450`, `task.rs` one call after `:1086`, `smp.rs` whole file; `quiesce.rs`/`snapshot.rs` pinned | Grant, lines only |
| H-S2-4 | Narrow row `260808-1544` for 07a/07b (set-based shootdown and retirement waits; fail-stop kept) | Narrow |
| H-S2-5 | x86 `MAX_HARTS`: 64 (u64 masks and `ViRetirementV1` exact; amends `phase-07a-*.md:37`), or 256; G1 lane keeps 2 by `cfg` | 64 |
| H-S2-6 | Operator surface: (a) `/bin/shutdown [-r]` via supervisor; (b) new cellbox builtins; (c) new `/bin/reboot`,`/bin/poweroff` workers (launch rows, RE overlap) | (a) |
| H-S2-7 | Power-button delivery: SCI ISR sets an atomic → `vi_timer_tick` hart 0 relays `OP_POWER` from sender 0 to the supervisor; vs reuse `[0xAC,0xFF]` `ShutdownWith`, which today makes the supervisor exit (`main.rs:119-121`) | `OP_POWER`, cell-local |
| H-S2-8 | Orderly shutdown: (i) `FreezeCell` (bit 49) all non-core cells, bounded settle window, no VFS change, marker states only what is proven; (ii) new VFS sync/quiesce opcode (widens to `cells/services/vfs/**`, own CP1). Risk: init restarts dead services (`cells/tools/init/src/supervisor.rs:203-232`), so killing is unsafe [INFERENCE] | (i) |
| H-S2-9 | `cells/services/power`: leave untouched and drop it from phase 06 (no consumer) | Leave |
| H-S2-10 | Read iGPU condition 2 as "no x86-breadth slice holds those files"; S2 releases them at close | Yes |
| H-S2-11 | CP2-06 (436 frozen) before 07a; 07b checkpoint = S2 close with the independent race review | Yes |

## 7. Per-phase verification, rollback, irreversibility

| Phase | Test matrix | Undo | Cannot be undone |
|---|---|---|---|
| 06 | Host: `\_S5_` scan on 2 fixtures; 436 decode/bit-49; 436 refused without `SupervisorCap`. Guest TCG and KVM: `/bin/shutdown -r` → second boot banner; `/bin/shutdown` → QEMU exit 0; QMP `system_powerdown` → supervisor marker → exit. Regression: `qemu-x86_64-boot` job; RV64/AArch64 `/bin/shutdown` unchanged | `git revert` (callers first, then the 436 arms) | After CP2-06, number 436 and its envelope are frozen |
| 07a | `cargo test -p cellos-kernel`; x86 suites at `-smp 2` (TCG, KVM); riscv64/aarch64 boot lanes, identical markers; domain test `online=2`; `.bss` delta recorded; G1 lane build | Revert; no persistent state | none |
| 07b | Epoch-set and victim-selection unit tests (4 simulated harts); KVM `-smp 8` `online=8`; TCG `-smp 4`; 10× stress with zero `[tlb] … invalidation unconfirmed` lines (the phase's "TLB flush timeout" string does not exist; actual log at `tlb_shootdown.rs:132`); riscv64/aarch64 still 2 harts; race review | Revert; c2c smokes return to `-smp 2` | none |

**Plan-text drift to fix under S2:**
- 07a context cites `x86_64.rs:255`, now `:264`; and `apic.rs:17`, now `:26`.
- 06 assumption 1 is false (§1).
- 07b omits `tlb_shootdown.rs`.
- CP1 E7 ("no reset or poweroff path on x86") holds: the 502 arm only executes `hlt`.
