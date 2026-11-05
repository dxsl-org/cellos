# Phase 07a evidence — per-CPU structures for N CPUs (behaviour-preserving at 2)

Branch `feat/x86-s2-phase07a-percpu` from main `26d8f3f3c`; code commit `5e6301264`. No Law-1 delta.
Ceilings: host, QEMU 8.2.2 TCG (`qemu64,+pdpe1gb`), local KVM (`-cpu host`). Not physical evidence. x86 still brings up at most two CPUs.
Base comparisons use a `git archive` copy of `26d8f3f3c` built with the same embedded inputs. Regenerated `kernel/src/embedded*/` restored before committing.

## What changed

- `smp.rs`: `MAX_HARTS` is `hal::x86_64::syscall::MAX_CPUS` (64) on x86_64 and 2 under `cellos_g1_cpu`, on riscv64, aarch64 and 32-bit targets; `const _: () = assert!(MAX_HARTS <= MAX_CPUS)` on x86. Per-hart array literals are `[const { … }; MAX_HARTS]`.
- `smp/topology.rs` (new): `CpuTopology<N, P>`, a write-once logical ↔ physical bijection (CAS from `UNMAPPED`, never cleared, out-of-range refused). The static is `CPU_TOPOLOGY`.
  - RV64: `set_boot_physical_hart` publishes the boot-hart swap (`0 ↔ boot`, the former `boot ^ 1`). `BOOT_PHYSICAL_HART` is gone. `logical_to_physical`/`physical_to_logical` read the table.
  - AArch64: `CPU_TO_LOGICAL` is gone; `publish_physical_cpu`/`physical_cpu_for`/`owned_slot_index` use the table.
  - x86: `start_secondaries` publishes BSP = 0 and the first non-BSP MP record = next free index (1) before the HAL slot. `x86_secondary_main` accepts any `0 < logical < MAX_HARTS` whose published LAPIC ID matches (was `assert!(logical == 1)`).
- HAL: `MAX_CPUS = 64` (single source). `apic::CPU_APIC_IDS` is `[AtomicU32; MAX_CPUS]` (was one packed `AtomicU64` + `assert!(MAX_CPUS == 2)`). Uniqueness: `PENDING|id` claim, SeqCst scan, withdraw on a duplicate. GDT/TSS, GS records and IST stacks are sized by `MAX_CPUS` (static: Deviation Log D3).
- `ready.rs`: `CLAIMS` size documented; `steal_from_busiest_with` returns for `thief > 1` (D8).
- `scripts/mutable-globals.toml` + regenerated `docs/generated/mutable-globals.md` (file-set deviation #1, D7).

## Size audit (`cargo build --release -p cellos-kernel`, default features, `-Z build-std=core,alloc`)

| Kernel | `.bss` before | `.bss` after | Δ | `.data` before → after |
|---|---|---|---|---|
| x86_64-unknown-none | 668,688 | 3,625,552 | +2,956,864 | 32,928 → 42,192 |
| riscv64gc-unknown-none-elf | 606,528 | 606,528 | 0 | 45,360 → 45,416 |
| aarch64-unknown-none-softfloat | 573,712 (+ `.bss.stack` 65,536) | 573,696 (+ 65,536) | −16 | 40,408 → 40,336 |

x86_64 per-symbol (`nm -S`), base → 07a:

| Symbol | Section | Before | After |
|---|---|---|---|
| `hal_x86::x86_64::ist::REGION` | bss | 0x14000 (80 KiB) | 0x280000 (2.5 MiB) |
| `hart_local::ready::CLAIMS` | bss | 0x3000 | 0x60000 (384 KiB) |
| `hart_local::HART_LOCALS` | bss | 0x210 | 0x4200 |
| `hal_x86::x86_64::gdt::TABLES` | bss | 0x180 | 0x3000 |
| `hal_x86::x86_64::syscall::CPU_LOCALS` | bss | 0x80 | 0x1000 |
| `smp::{TLB_FLUSH_*, RETIRE_SWITCH_*}`, `HART_LOCAL_TP_ADDRS` | bss | 0x10 each | 0x200 each |
| `smp::{HART_ONLINE, X86_PREEMPT}` | bss | 0x2 each | 0x40 each |
| `task::BOOT_CONTEXTS` | data | 0xc0 | 0x1800 |
| `hal_x86::x86_64::apic::CPU_APIC_IDS` | data | 0x8 | 0x100 |
| `smp::topology::CPU_TOPOLOGY` (new) | data | — | 0xa08 |

The x86 growth is static capacity for 64 CPUs: 2.5 MiB IST region, 384 KiB `CLAIMS`, the rest under 64 KiB. All 257 IST/boot guard pages (`GUARD_PAGES = 4 × 64 + 1`) are unmapped at boot; `kmain` panics otherwise, and every boot below passed.

## Host

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `test result: ok. 443 passed; 0 failed`. New: `task::smp::topology::tests::{logical_physical_round_trip, out_of_range_ids_are_refused, slots_are_write_once_and_the_map_stays_a_bijection, rv64_boot_hart_swap_matches_the_former_xor_mapping, the_kernel_map_is_sized_by_max_harts} ... ok`.
- `cargo test -p hal-x86 --target x86_64-unknown-linux-gnu`: `test result: ok. 26 passed`. New: `x86_64::apic::tests::cpu_registration_is_write_once_and_unique ... ok`.
- clippy `-p cellos-kernel -- -D warnings` (default features): x86_64-unknown-none, riscv64gc-unknown-none-elf and aarch64-unknown-none-softfloat each `Finished` with no warning. `-p hal-x86` (x86_64-unknown-none): `Finished`.
- `cargo fmt --all --check`: no diff in a touched file. The repo has fmt drift elsewhere at base (supervisor, acpi, sys-tools); not touched.
- G1 lane: built as in `scripts/x86/p01_capture.py` (`CELLOS_P01_TRACE=1 CELLOS_G1_CPU=1`, `EMBEDDED_OVERRIDE` with a 96-byte placeholder `p01_manifest.bin`, `--features test-hooks`, x86_64-unknown-none, clippy `-D warnings`). Base and 07a give the **identical** error list, so it does not compile on either tree:

  ```
  error: empty line after doc comment  --> kernel/src/task/domain_switch_tests.rs:27:1
  error[E0614]: type `task::tcb::Task` cannot be dereferenced  --> kernel/src/task/scheduler.rs:2131:23
  error[E0614]: type `task::tcb::Task` cannot be dereferenced  --> kernel/src/task/scheduler.rs:2622:60
  ```

  `cpu_accounting.rs:19` (`M == MAX_HARTS`) is therefore not reached by the compiler on either tree. Under `cellos_g1_cpu`, `MAX_HARTS = 2` holds by the `cfg` in `smp.rs`.
- `python3 scripts/cellos-sign --check --strict`: `OK: F5 — rustc f53b654a8882 is the pinned nightly-2026-05-01`, `OK: F1 — 139 crates and 999 files scanned; unsafe confined to 77 allowlisted files` (the extra file is `topology.rs`, which has no `unsafe`).
- Law-1 digests: `OK: the frozen AI interface matches its Law 1 record`; `PASS: LookupServiceBound Law-1 items intact`; `PASS: multi-source WaitCompletion Law-1 items intact`.
- `tools/mutable-globals --check`: **red before and after**. Base `26d8f3f3c`: `636 declarations, 251 need review, 37 problems`. 07a: `635 declarations, 252 need review, 37 problems`.
  - The 35 unclassified statics and the stale md pre-date 07a. 07a adds no unclassified static.
  - Declarations −1: `CPU_TOPOLOGY` is new; `BOOT_PHYSICAL_HART` (riscv) and `CPU_TO_LOGICAL` (aarch64) are gone. Both are inactive in the x86 profile.
  - The regenerated md renders the 35 pre-existing rows as UNCLASSIFIED. Main tracks them as a separate repair item.

## x86 guest (images built as in CI job `qemu-x86_64-boot`)

The images are the headless image (`kernel_features=policy-required`, kernel sha256 `d7f932f6…f875`, `make-iso-ci.sh`), `build-x86_64-prod-refusal-ci.sh`, `build-x86_64-domain-test-ci.sh` and `build-x86_64-c2c-render-ci.sh`.

| Lane | TCG | KVM |
|---|---|---|
| `x86_64-boot` (`CI=true`, prod-refusal ISO pinned; this suite boots `-smp 1`) | `9 passed` | `9 passed` |
| canonical ISO, `-smp 2` (temporary copy of `scripts/qemu-x86_64-test.sh` with `-smp 2`) | `PASS: x86_64 shell prompt reached` | `PASS: x86_64 shell prompt reached` |
| domain lane `X86_CPUS=2 scripts/x86/qemu-domain-test.sh` | `PASS … (x86-domain-test-cpu2) — every marker present` | `PASS … (x86-domain-test-kvm-cpu2) — every marker present` |
| c2c-render smoke `--cpus 2` | FAIL, pre-existing (below) | FAIL, pre-existing (below) |

Decisive lines; topology and dispatch are identical on every 2-CPU run (canonical ISO, domain lane, c2c), TCG and KVM:

```
[ INFO] [x86-smp] transport IPI-invalidate bidirectional=pass
[ INFO] [x86-smp] topology online=2 processor_ids=0;1 qualified=yes
[ INFO] [sched] hart 1 dispatched a task
[ INFO] [sched] hart 0 dispatched a task
```

Domain lane (test-hooks kernel), TCG and KVM:

```
[ INFO] S22-X86-CONTEXT-HANDOFF: PASS raw_switch=true save_guard=true steal_after_save=true late_retirement_deferred=true
[ INFO] S22-X86-REMOTE-SHOOTDOWN: PASS harts=2 global=true irq_masked_lock=true before=4951 after=9320
[ INFO] S22-X86-MASKED-SHOOTDOWN: PASS harts=2 irq_masked_uncontended_lock=true after=4951
TCG: [ INFO] S22-X86-DOMAIN-LIVE: PASS cr3=0x4bca000 root=0x4bca000 pcid=0 pcid_usable=false … hart=1 harts=2
KVM: [ INFO] S22-X86-DOMAIN-LIVE: PASS cr3=0x4bca001 root=0x4bca000 pcid=1 pcid_usable=true … hart=1 harts=2
```

**c2c-render: pre-existing failure, not 07a.**
- Every run ends with `USER: c2c-render: ERROR VFS write /tmp/c2c-render.ppm failed: Err(3); partial image may remain`, after a correct topology line. The script then raises `RuntimeError: c2c-render compare …`.
- This holds for base `26d8f3f3c` (TCG and KVM, `--cpus 2`), for 07a (TCG and KVM, `--cpus 2`) and for 07a KVM `--cpus 1` (`topology online=1 processor_ids=0 qualified=yes`).
- The cause lies in cells/VFS, which is outside the 07a set.

**IDT/IST fixture** (`build-x86_64-idt-test-ci.sh` + `scripts/qemu-x86_64-idt-test.sh`; not a CI lane; it exercises `gdt.rs`/`ist.rs`):
- 07a prints `X86-IDT-SELFTEST: PASS …`, `X86-IST: PASS gates=2/8/18->ist1/2/3 tss=own-cpu guards=9 …`, `X86-IST-HW: nmi=2d8 df=2d8 mc=2d8 limit=2000` and `X86-IDT-CPL3: PASS …`.
- The script still ends in `FAIL: forbidden failure/skip/reset marker present despite success status`, **identical at base**. The trigger is the phase-06 lines `[acpi] FADT: … reset=true …` / `[acpi] power: reset_reg=true …`.
- `guards=9` is a literal at `probe_ist.rs:271`. The check before it (`guards_installed() == GUARD_PAGES`) passed against 257 (Deviation Log S5).

## riscv64 / aarch64

Embedded inputs for both trees come from `scripts/build-boot-ramdisk-ci.sh` (rv64) and the CI aarch64 cell/`mkfat32.py` recipe. Each aarch64 boot uses a fresh copy of one `format-disk-arm.sh` image.

| Lane | base `26d8f3f3c` | 07a |
|---|---|---|
| rv64 CI lane `scripts/qemu-boot-test.sh` (no `-smp`) | `PASS: FAT16 mounted — kernel booted (no disk)` | same |
| rv64 same command + `-smp 2` | FAT16 mounted, no panic | same |
| aarch64 CI default lane `BOOT_WINDOW=90 scripts/qemu-aarch64-test.sh` (`smp=1`) | `PASS: aarch64 shell prompt reached — full boot successful` | same |
| aarch64 default lane `QEMU_SMP=2` | same PASS | same PASS |

The SMP markers match line for line between base and 07a.
- rv64, boot on physical 0: `[smp] physical 0 -> logical 0 boot`, `[smp] physical 1 -> logical 1 HSM state = 1`, `[smp] hart 1 start requested (entry=0x802000ac)`, `[smp] physical 1 -> logical 1 trap-ready, interrupts-enabled`, `[smp] hart 1 online, parked`.
- rv64, boot on physical 1 (OpenSBI's boot-hart lottery; this case is the swap through the table). Seen on base run 2 and on 07a runs 2 and the first `-smp 2` run; `diff` of the `[smp]` lines between base and 07a is empty:

  ```
  [ INFO] [smp] physical 1 -> logical 0 boot
  [ INFO] [smp] physical 0 -> logical 1 HSM state = 1
  [ INFO] [smp] hart 0 start requested (entry=0x802000ac)
  [ INFO] [smp] physical 0 -> logical 1 trap-ready, interrupts-enabled
  [ INFO] [smp] hart 1 online, parked
  ```

- rv64 `-smp 1` (both trees): `[smp] physical 0 -> logical 0 boot`, `[smp] physical hart 1 HSM status unavailable`.
- aarch64 `QEMU_SMP=2` (both trees): `[smp] hart 1 start requested (cpu=1 entry=0x40080164)`, `[smp] hart 1 trap-ready, interrupts-enabled`, `[smp] hart 1 online, parked`, `[selftest] SMP-IPI: PASS hart=1 epoch=1`. The IPI goes through `physical_cpu_for` → `CPU_TOPOLOGY`.
- aarch64 `smp=1` (both trees): `[smp] hart 1: firmware reports no such core — single-hart`, `[selftest] SMP-IPI: skipped, no remote hart online`.

## Not run

- The other `qemu-x86_64-boot` CI suites (ahci, nvme, xhci, igb, virtio, iommu-dmar, machine-pin, serial, power, local-service-lifecycle).
- The aarch64 `-kernel Image` and `virt-vhe` lanes.
- Any run above two CPUs; that is phase 07b.
