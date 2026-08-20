# Cellos Project Roadmap

**Project**: Cellos (Jarvis Hybrid OS)
**Current version**: 0.2.1-dev (Mycelium Era)
**Current phase**: Phase 1 - Core Stability; active product stage G1 Robot & Embedded
**Last updated**: 2026-08-19

This file is now the roadmap entrypoint. The previous all-in-one roadmap is
preserved as a read-only content snapshot at
[project-roadmap-legacy.md](project-roadmap-legacy.md). Use it
only when a historical decision is not represented by the current topic pages.

> **2026-08-20 G1 common-driver update:** Phase 03 is complete for the BCM/RPi3 lane. Real hardware passed GPIO17-to-GPIO27 edge detection, BCM BSC1 explicit data NACK, GPIO actuator readback, and BCM SPI0 `AA55` loopback; AArch64 QEMU separately passed PL061, PL011, and bounded pinned-worker regression gates. DesignWare I2C/SPI remains conditional on verified board compatible, MMIO, IRQ, and pinmux evidence.

## How to Read the Roadmap

<<<<<<< HEAD
| Need | File |
=======
> **2026-08-09 phase-note:** SAS/LBI VFS Phases 02 and 03 are complete. The approved bridge is an exact per-request VFS grant-copy lease plus current-caller-cell-only death watch; shell is the sole bounded grant-read pioneer with sender-masked replies, exact bounds, typed failure, and no raw-pointer/fast fallback. Phase 04 file handles are now complete as an append-only ABI delta: `OpenFileAt` / `ReadFileHandle { max: u32 }` / `CloseFile` are appended, `ViVfsFileHandle` is service-local, the file-handle path stays attested-message-only, and `Data` replies are bounded inline at 4000 bytes. RV64 shell, quota, and lifetime QEMU lanes pass; production kernels compile on RV64/AArch64/x86_64; API tests and the existing test-hooks QEMU markers cover the change; no hardware claim is made and global coverage debt remains.

> **D13 correction (2026-08-01):** the earlier "cryptographic origin proof" wording
> describes the verification hook, not a completed fleet trust chain. Default G1 admits
> absent signatures, the dev seed is public, no production public-key provisioning path
> exists, and secure boot remains open. See Spec 18 and the fleet-admission item below.

> **Plan-portfolio WIP limit (D34-D39, 2026-08-01):**
> [`.agents/plan-portfolio.md`](../.agents/plan-portfolio.md) is the scheduling source of
> truth. Midori is the sole active feature program until phases 07/08 close; package
> distribution, Trust & Identity remainder, and remote integration are queued. P0 security,
> broken CI/build repair, and verification-only closures are the only side-work exceptions.
> Phase 02 is runtime-closed under amended criteria, and phase 03 is complete as the
> closure-amendment package. Supervisory migration is complete.

> **2026-08-08 supervisory migration update:** Phase 03 snapshot-trigger authority is complete; shell snapshot routes through Supervisor IPC, `Snapshot=420` is SupervisorCap-gated, QEMU proof remains NullBlock/unavailable on tested targets, and Phase 04 kernel cleanup is complete. Accepted followups are host-gated x86/AArch64 fresh boot lanes and host API coverage.

> **2026-08-17 board-split update:** root `boards/` landed as a no_std descriptor crate and the RV64 QEMU boot path now consumes it for audited fallback data. Shared drivers remain in `cells/drivers/`; `hal/soc/riscv` now owns the SoC profile slice, while AArch64/RPi3 board extraction, SDHCI, and feature-collapse remain deferred. Verified gates for this slice were `cargo fmt --all --check`, `cargo test -p cellos-boards --target x86_64-unknown-linux-gnu`, RV64/AArch64 `cargo check`, `cargo check --features board-vf2`, `cargo check --features board-rpi3`, `cargo build --release -p cellos-kernel --target riscv64gc-unknown-none-elf`, and `scripts/qemu-boot-test.sh target/riscv64gc-unknown-none-elf/release/cellos-kernel`.

> **2026-08-17 RISC-V SoC-profile update:** `hal/soc/riscv` is now the data-only
> owner for RV64 SoC compatible lists and access policies. QEMU virt and
> JH7110/VF2 keep MMIO discovery; SG2042/Pioneer stays fail-closed with SBI
> DBCN-only console, no RTC MMIO, and no VirtIO-MMIO slots. Shared drivers remain
> in `cells/drivers/`, board descriptors remain in root `boards/`, and
> AArch64/RPi3/SDHCI extraction plus feature-collapse remain deferred. Verification
> passed the hal-soc and board unit tests, RV64/AArch64 feature checks, RV64
> release build, and QEMU release-kernel boot.

> **2026-08-18 board-descriptor update:** root `boards/` now includes QEMU RV64
> and Raspberry Pi 3 Model B descriptors. `BoardDescriptor` keeps UART mandatory
> and makes PLIC/CLINT/RTC optional, the RPi3 fallback map ends exactly at
> `0x3F000000`, and the kernel consumes board data through
> `kernel/src/{board.rs,boot.rs,platform.rs}`. `hal/soc/bcm27xx` still owns the
> BCM2837 facts; shared UART/SDHCI/GIC/PLIC/PCIe drivers remain single-copy.
> RPi3 physical boot stays hardware-gated; this slice is compile-only.

> **2026-08-18 BCM27xx MMIO-policy update:** `hal/soc/bcm27xx` now owns the
> exact peripheral/local-controller spans and GPIO/AUX grant widths consumed by
> RPi3 paging, the resource registry, and GPIO IRQ owner lookup. Existing page
> permissions and allowlist widths remain unchanged; no IRQ/timer mechanism or
> new MMIO authority moved. The 9-gate matrix passed through RV64 release and
> QEMU FAT16 boot; this slice adds no physical-RPi3 runtime claim.

> **2026-08-18 BCM27xx arch-base update:** RPi3-specific ARM HAL modules now
> source mini-UART, GPIO, system-timer, legacy-IRQ, and local-controller bases
> from `hal/soc/bcm27xx`. The optional dependency is activated only by
> `board-rpi3`; register offsets, IRQ numbers, timer periods, and mechanisms
> remain in `hal/arch/arm`. The 11-gate matrix passed through RV64 release and
> QEMU FAT16 boot; RPi3 remains compile-only for this slice.

> **2026-08-18 BCM27xx IRQ-topology update:** `hal/soc/bcm27xx` now owns
> BCM2837 legacy IRQ numbers and BCM2836 Core0 source masks. ARM HAL public
> constants remain compatible aliases; register offsets, C1 status/ack, the
> 10 ms timer period, and interrupt mechanisms remain in `hal/arch/arm`. The
> 11-gate matrix and review passed; RPi3 remains compile-only for this slice.

> **2026-08-18 BCM27xx IRQ-consumer update:** GPIO pending-bank masks, RPi3
> CNTP routing, and the kernel IRQ diagnostic now consume the centralized
> BCM2837 topology instead of repeating controller addresses or source bits.
> Register offsets, public constants, C1 status/ack, the 10 ms policy, and
> diagnostic output are unchanged. Baseline and final 11-gate matrices plus
> review passed; RPi3 remains compile-only.

> **2026-08-18 RPi3 UART-debug reuse update:** the kernel TrapFrame diagnostic
> now uses ARM HAL's existing FIFO-safe mini-UART byte writer instead of
> duplicating LSR/IO addresses, TX readiness polling, and MMIO writes. Byte
> formatting and task setup remain unchanged. Baseline/final 11-gate matrices,
> the scoped guard, and review passed; RPi3 remains compile-only.

> **2026-08-18 HAL-split catalog/RV64 update:** root `boards/` now has typed
> descriptors for all seven current selections. RV64 chooses one descriptor/SoC
> pair, boot fallback memory and platform policy consume it, and VF2/Pioneer
> require a valid firmware DTB. Board/SoC tests, three RV64 compile lanes, the
> release build, and QEMU FAT16 boot passed; physical boards remain compile-only.

> **2026-08-18 ARM/SDHCI HAL-split update:** `hal/soc/arm-virt` now owns QEMU
> AArch64 platform facts, `hal/soc/bcm27xx` covers BCM2711, and the shared SDHCI
> implementation consumes runtime BCM2837/BCM2711/JH7110 access policy. RPi4
> cell mappings are limited to disjoint GPIO/UART/SDHCI pages, with GIC
> kernel-only and PCIe unadvertised. The 15-lane compile matrix and RV64 QEMU
> witness passed. AArch64 QEMU reached `ViCell >`; the final closure slice later
> corrected the stale test marker. Physical boards remain compile-only.

> **2026-08-18 HAL-split closure:** the seven-board catalog now satisfies the
> board-only completion contract: no SoC MMIO/IRQ fields remain in descriptors,
> RV64 fallback MMIO lives in validated SoC profiles, enabled-driver data gates
> shared initialization, required-DTB nodes fail closed, and incompatible board
> features fail at compile time. CI owns the boundary and six-board matrix.
> Host/compile/review gates, RV64 QEMU FAT16 boot, and AArch64 QEMU `ViCell >`
> boot passed. Only physical VF2/Pioneer/RPi3/RPi4 runtime evidence remains
> hardware-gated; it is not unfinished code separation.

> **2026-08-18 RISC-V PLIC runtime-data update:** `hal/arch/riscv` now consumes
> the active physical-hart context from the selected SoC profile and the device
> IRQ list from kernel `PlatformInfo`. `hal/soc/riscv` owns checked PLIC context
> policy data, and the shared PLIC
> driver no longer hardcodes QEMU `context 1` or fixed IRQ enable ranges.
> Verification passed `cargo fmt --all -- --check`, `cargo test -p hal-soc-riscv
> --target x86_64-unknown-linux-gnu`, `cargo test -p cellos-boards --target
> x86_64-unknown-linux-gnu`, RV64/AArch64 `cargo check`, `cargo check --features
> board-vf2`, `cargo check --features board-pioneer`, `cargo check --features
> board-vf2,board-pioneer`, `cargo check --features board-rpi3`, `cargo build
> --release -p cellos-kernel --target riscv64gc-unknown-none-elf -Z
> build-std=core,alloc`, and `bash scripts/qemu-boot-test.sh
> target/riscv64gc-unknown-none-elf/release/cellos-kernel` (`PASS: FAT16 mounted
> — kernel booted (no disk)`).
> VF2, Pioneer, and RPi3 remain compile-only for this slice.

> **2026-08-18 BCM27xx SoC-facts update:** `hal/soc/bcm27xx` now owns immutable
> BCM2837 controller layout and SDHCI access-policy facts. Existing RPi3
> platform/MMC code consumes those facts while the shared SDHCI mechanism stays
> single-copy. Board identity, boot/fallback memory, SD pinmux selection, PHY
> wiring, enabled-driver lists, IRQ/timer extraction, and feature collapse remain
> outside this slice. The final matrix passed 12/12 gates, including AArch64
> `board-rpi3` compile and RV64 QEMU boot; no new physical-RPi3 claim is made.

> **Midori Phase 02 runtime-closure update (2026-08-05):** the test-hooks QEMU lane now
> proves metadata-only governed message-path `GetFile` positive before `SealPaths`, preserves
> the existing post-seal denial marker, and still proves `ReadFileGrant` clamp/nonzero/post-seal
> denial.
> Phase 02 is now runtime-closed under the user-approved amended criteria only. Real
> `ReadGrant` production is explicitly deferred to a future Law 1 `OpenAt`/file-handle/close
> design, direct fast-IPC `GetFile` proof is deferred to a future Tier-1 transport rewrite,
> and `DataPtr` remains same-SAS only, not Tier-2-safe.

> **Midori reactor-stack Phase 02 guardrails (2026-08-06):** a raw first-command shell burst,
> caller-visible dead-peer error + sender requeue, stale-result reset, mailbox-only
> `RecvScatter` isolation, heartbeat-watchdog blocked-send wake, and ForceExit notification
> drain now pass. The VFS grant audit records the two unsafe service-side copy sites whose
> safety still depends on blocking caller lifetime. Generic reactor work, `RecvScatter`
> repair, and async VFS/DMA remain deferred behind Law 1; parked executor work is closed,
> and stack resizing is now the open Phase07 follow-on with no blocker.

> **Midori Phase 07 post-shim stack sizing closure (2026-08-06):** six measured paths
> (`init`, `shell`, `vfs`, `vfs-test`, `net`, `virtio-net`) are fixed at 16 usable pages plus
> the two Phase 06 guards; unmeasured names stay on the 64-page default. The RV64
> test-hooks sizing lane, RV64 shell/DHCP/TCP/VFS production lane, and production boots on
> RV64/AArch64/x86_64 all passed; x86 VirtIO-MMIO enumeration was fixed; no manifest/public
> ABI field changed.

> **Midori Phase 05 parked-executor closure (2026-08-06):** per-executor `Arc` RawWaker,
> bounded TIMER park, independent monotonic-ms sleep deadlines, and fail-loud authority checks
> are verified; shell `Recv` stays unchanged, the broad shell/input/DHCP/TCP/VFS and
> peer-death lanes were run before the final fallback-only change, the exact parked marker
> `[executor] dummy-waker=absent executor=parked source=TIMER PASS` was rerun after that
> tweak, and reviewer APPROVE is recorded.

> **Midori Phase 01 partial-closure update (2026-08-01):** the test-hooks QEMU lane now
> proves `ReadFileGrant` allow/deny markers, but Phase 01 stays partial. `ReadGrant`
> runtime coverage is blocked because `cells/services/vfs/src/handle_table.rs:136` remains
> the only `HandleTable::insert_ro` caller, and actual fast-IPC `GetFile` runtime proof is
> still blocked by D1 pending a separate approved Tier-1 rewrite/rescope.

---

## Overview

Cellos development is organized into 4 major **technical phases** (Core Stability → System Services → Apps/Runtimes → Advanced) plus hardening Phases 24–32. This document tracks progress, blockers, and next steps.

**On top of that technical numbering, work is now framed by product stages (G1–G4) by target hardware / use-case** (overlay — see next section). Technical phase IDs (Phase 24–32, M2.x–M4.x) and all `.agents/` cross-references are preserved; the `[G1]`/`[G2]`/`[G3]`/`[G4]`/`[G5]` labels are a use-case overlay, NOT a renumbering.

---

## 🎯 Use-Case Stages (Overlay)

Cellos ships in product stages defined by target hardware. The mapping principle: **architecture maturity matches use-case** — ARM64/RV64 (with MMU) → robot SBC `[G1]`; x86_64 → server/PC `[G2]`; RV32 → MCU deeply-embedded (sub-track at end of G1).

### 🤖 Stage G1 — Robot & Embedded
> **"Done" means**: never-die · bounded real-time · bounded per-Cell memory · fault isolation · fast boot · peripheral I/O · small footprint.
>
> **Hardware**: primary = **Tier A SBC with MMU** (RV64/ARM64, RPi-class robot brain/companion). Sub-track (end of G1) = **Tier B MCU** (RV32 <512KB, CHERIoT-Nano) for low-level motor/sensor control.

### 🖥️ Stage G2 — Server & Specialized PC
> **"Done" means**: throughput · multi-core scaling · untrusted third-party code · desktop GUI · zero-downtime · full tooling · large storage · RT-bounded NPU inference (via Tier 1b).
>
> **Hardware**: x86_64 (full bring-up) + multi-core RV64/ARM64 servers + RISC-V AI server (C930/P870).
>
> **Queued scale profile (D5):** 1000 simultaneous per-request isolated cells is a G2
> qualification goal, not current capacity. Promotion requires 64/128/256/512 measurements,
> shared immutable image frames, demand-paged stacks, profile quotas, and dynamic tables; the
> current 64-cell large-app default stays in force.

### 🧠 Stage G3 — NPU-native Compute OS _(placeholder — starts after G2 ships)_
> **"Done" means**: kernel schedules NPU as first-class compute resource · zero-copy tensor pipeline cross cells · per-cell NPU quota · NPU fault isolation (driver cell restart, app cells survive) · model weight shared across inference cells.
>
> **Conditions to start G3** (ALL required):
> 1. G2 graduation criteria met (inference demo via Tier 1b with P99 bound)
> 2. Real NPU hardware acquired (RK3588 ~$150 available now, OR SiFive P870+X390)
> 3. Large-buffer IPC (sys_grant_pages) done — G2 extension, prerequisite for tensor handoff
> 4. ≥2 months hands-on with real NPU vendor API (RKNN/X390) to validate `ViAccelerator` contract
>
> **Hardware**: same as G2 server targets, with dedicated NPU (RK3588 ARM64 OR SiFive P870+X390 RISC-V).
>
> ⚠️ **Do NOT spec G3 in detail before hardware** — API contract (ViAccelerator trait, TensorBuffer, dual-domain memory) must be hardware-informed. Exploratory draft: [.agents/reports/brainstorm-260606-2032-g3-npu-native-os.md](.agents/reports/brainstorm-260606-2032-g3-npu-native-os.md)

### 🦀 Stage G4 — App Platform: full Rust std for Tier 1 _(placeholder — research after G3 ships; decided 2026-07-22)_
> **"Done" means**: custom rustc target `x86_64-unknown-cellos` (then `aarch64-`) with full `std` · unmodified crates.io crates (serde/regex/clap…) compile & run in Tier 1 cells · tokio ecosystem runs via `mio`-cellos backend (features `process`/`signal` off) · `std::os::cellos` extension traits (capability-based `SpawnExt` instead of `pre_exec`/uid) · **zero C in the Tier 1 TCB**.
>
> **Locked decisions (brainstorm 2026-07-22)**:
> 1. **Route A** — pure-Rust PAL (`sys/pal/cellos`) in a small rustc fork, apps via `-Zbuild-std`; precedent = Hermit (`x86_64-unknown-hermit`: SAS, no fork, tier-3 upstream). **Route B (std over mlibc) REJECTED** — pulls C into every Tier 1 cell's TCB; std's unix PAL assumes fork/signals (thin-shim creep).
> 2. **No tokio reimplementation** — write bottom-of-stack backends: `polling` crate first (smol; validates protocol), then `mio` (tokio). Readiness = IPC message from net cell; reactor = recv-mask + RecvTimeout loop. **No kernel epoll** — multiplexing is policy (Kernel Boundary Law). Planner research revised "zero new syscalls" → **2 small kernel-surface additions needed** (futex_wait has no timeout — `Condvar::wait_timeout` requires it; no self-wake primitive for reactor `notify()` — IPC mask is binary 0/N, not OR-able). Both Boundary-Law-legal (scheduler/IPC mechanism); P2.5 settles the notify() design before P3 code.
> 3. **Fallback ladder** per std module: native syscall → userspace emulation over IPC/Grant → `ErrorKind::Unsupported` stub (WASI-style fallback model). `std::os::unix` deliberately ABSENT — POSIX-needing crates fail at compile → routed to Tier 3 VM (Scope Doctrine firewall enforced by rustc itself).
> 4. `panic=abort` first (never-die supervisor = recovery story); unwinding late. `Command` → spawn-model on `sys_spawn_from_elf` (no fork). ostd survives as Cellos-native ext layer (Grant/IPC/Silo/ViUI) beside std, like hermit-abi beside Hermit std.
>
> **Already in place** (why this is cheaper than it looks): FutexWait/FutexWake syscalls 9/10 (std's lock impls are futex-based) · `spawn_thread` in-cell threads · GetRandom=214 · RTC wall-clock · spawn-args stash · `sys_spawn_from_elf` · VFS IPC + Grant large-buffer path.
>
> **Kernel prerequisites (G4-P0)** _(revised by planner research + red-team 2026-07-22 — a real thread-runtime build, not a register swap)_: (A) per-thread TLS (tp/fs-base swap) + **per-thread USER stacks — do not exist today** (`Task.user_stack` declared but never populated; only kernel stacks, scheduler.rs:271; `spawn_thread` makes S-mode kernel-stack threads, task.rs:471-484, and must be rebuilt to user-mode) + guard-page wiring (deliberately-unmapped guard ≠ freed frame — no SAS frame-identity conflict, document it); (B) **thread lifecycle — `sys_exit` runs CELL-WIDE teardown** (syscall.rs:1477-1537) so a worker thread exiting self-destructs the whole cell → add thread-scoped exit + per-cell thread refcount + make `panic=abort` abort the WHOLE cell (recovery unit = the cell); (C) TLS-image source decision (loader ignores PT_TLS, elf.rs:49-50); (D) **futex hardening — REVERSAL, NOT "verify-only": `futex_wait` derefs a raw addr with no ownership check + `futex_wake` scans all tasks with no cell_id filter (task.rs:1495-1527) = cross-cell read oracle defeating LBI + kernel-deref DoS** → add addr-ownership validation + cell-scoped wake + timeout arg (MTIME ticks). Separately, the CellId(0) *quota* escape (Mythos #7) is already fixed (verify-only, syscall.rs:1306-1334); std threads inherit the parent cell's syscall allowlist, so `app_entry!` must auto-include futex/thread/random.
>
> **Phasing (8 phases)**: P0 kernel thread-runtime prereqs → P1 "compute std" (alloc/thread/futex-sync/time/stdio/env/random; fs/net/process=Unsupported; **+ `ostd-ext` split + std entry shim** — REVERSAL: ostd's singleton lang items (allocator/panic/`_start`, heap.rs:68/76 + startup.rs:120/24) collide with std's, and a std cell must still emit `__ViCell_manifest`/`__ViCell_syscalls` or signing rejects it; **+ x86_64 build/sign/boot pipeline** — gen_disk is riscv-only; milestone = serde/regex/clap **signed + booted**, not just compiled) → P2 "OS std" (fs via VFS+Grant, `fs::rename`=copy+delete; net **owner-scoped SocketTable** (fixes cross-cell socket hijack) + WouldBlock/EOF/error discriminants + DNS resolver-or-numeric-only + path canonicalization) → P2.5 readiness protocol + reactor recv rules + **`AsCellHandle` ABI freeze** (spec before code, highest design risk) → **P2.6 net-cell readiness engine** (the net cell emits NO readiness today — a spec alone can't unblock P3; new phase) → P3 `polling` then `mio` backends (milestone: tokio+axum hello-world — highest-uncertainty milestone) → P4 `std::os::cellos` + process-lite → P5 unwinding + upstream tier-3 + rebase CI gate (~6-week fork rebase cadence). **Re-baselined effort: ~120-175 engineer-days / ~10-17K LOC.**
>
> **Plan**: [.agents/260722-0917-g4-full-std-tier1/](../.agents/260722-0917-g4-full-std-tier1/) (8 phase files + `## Red Team Review`: 6 Critical / 7 Major / 3 Minor, all accepted, 1 DEFER = pre-existing global VFS read) · **Conditions to start coding (revised 2026-07-23, user-approved): G2 shipped** (was G3 — G3 is NPU-hardware-gated, unrelated to std/PAL work). The design deliverables are complete, but D8 corrected their status: P0 kernel prerequisites, P2.6 net-engine design, and the P1 PAL mapping remain reviewed inputs; the P2.5 readiness protocol in Spec 17 §10 is **Draft/reserved-but-unbuilt**, with `0x11`/`0x12` held against collision until implementation and fresh Law-1 confirmation.

### 🖥️ Stage G5 — Virtualization Platform: SAS/LBI-accelerated dual-profile VM host _(placeholder — research/design only, post-G4; direction set 2026-07-22)_
> **Thesis**: evolve the Tier 3 VMM into **one VMM core with two build/feature profiles** (industry precedent: `rust-vmm` core → Firecracker "lite" + Cloud Hypervisor "wide"; NOT two separate codebases — that violates DRY). Leverage the fact that the Cellos kernel **owns the frame allocator + `Stage2Table` directly** (no host-Linux `mm` layer between VMM and physical frames) to make VM load/reset faster and DMA-safer than a generic hypervisor.
>
> **Axis clarification (a profile is a VMM/host config, NOT a guest OS)**: "profile" selects the *hypervisor's* device model, boot path, and features; the *guest image* (Alpine, glibc/Ubuntu, a minimal custom rootfs) is a separate, orthogonal axis loaded INTO a VM. Re-architecting to "one core, two profiles" is host-side VMM work — it does **not** require authoring a new Linux. Today there is one VMM + one wired guest (**Alpine**, `vmlinuz-virt` + `initramfs-virt`, direct-kernel boot — scripts/make-hypervisor-fs.sh); a glibc/Ubuntu-class guest is the planned *compat* direction. Alpine (musl, ~5MB, already direct-kernel+initramfs = Firecracker's own pattern) is ~80% of the Lite guest already — you *strip/assemble* a minimal rootfs (buildroot/Yocto/Alpine-minirootfs), you do not write a distro.
>
> **Two profiles = two curated presets over a feature matrix, NOT a fixed count** — model as composable flags `{device-model: minimal|full} × {boot: direct-kernel|firmware} × {snapshot/CoW: on|off} × {confidential: none|TDX/SEV/CCA}`; add a preset only when a real workload needs a combination the presets don't cover (YAGNI):
> - **Lite** preset (pairs with a minimal guest, e.g. stripped Alpine) — minimal device model + direct kernel boot (PVH / Linux boot protocol, already in the x86 Tier 3b plan) + CoW-golden clone + snapshot-restore. Target = low cold-start, fast reset, agent-sandbox / FaaS-shaped workloads. Note: a Lite VMM booting a *full distro* is still slow — the guest userspace, not the VMM, is then the bottleneck.
> - **Wide** preset (pairs with a stock glibc/Ubuntu guest) — full device model + bootloader + broad guest compatibility = today's Tier 3b Linux VM. Target = "run existing Linux software".
> - **Confidential** (candidate 3rd preset, gated) — a different *security posture* (TDX/SEV/CCA), not "more compat"; roadmap already keeps the `VmHandle` ABI CC-neutral. Do NOT build until hardware + a paying customer exist.
>
> **Speed reality (do not oversell — two different numbers)**: Firecracker's headline "sub-5ms" is *snapshot resume*, not cold boot (~125ms). Cellos Tier 3b's current 2-10s is mostly booting a **full distro**, not a VMM defect (the VMM is already minimal — custom ~9K LOC). **Cold-boot ~150ms is an UNMEASURED TARGET, not "parity plausible"** (red-team 2026-07-22): there are zero measurements and a contrary data point (the FAT guest-image loader re-seeks the cluster chain from the start on every read → per-file load is quadratic in call count, [loader_image.rs:130-134](../cells/services/hypervisor/src/loader_image.rs)). The number must be gated behind a measured ARM64 cold-boot baseline on a KVM-accel/real-HW lane before it is quoted. **Headline sub-10ms parity REQUIRES building guest snapshot/restore** — a real, bounded deliverable that does not exist yet.
>
> **SAS/LBI leverages (grounded in code — the genuinely differentiated part)**:
> 1. **CoW-golden clone (load) + reset-to-golden (reset) — the strongest lever.** Boot a VM once to ready state; keep its guest-RAM frames as a read-only "golden set" G. New clone = fresh `Stage2Table` mapping guest-IPA → G with `writable=false` (the RO/RW descriptor bits exist — [stage2.rs:38](../kernel/src/memory/stage2.rs) `S2_S2AP_RO` — **BUT the CoW substrate does NOT exist**: the `map()` SAS isolation guard is single-region and skipped when `guest_ram_pages==0`; it must be re-architected into a per-table multi-region HPA allowlist (golden-RO ∪ overlay-RW) before a clone can be expressed safely — red-team 2026-07-22, 4/4 reviewers); guest write → stage-2 permission fault → hypervisor copies the page to a fresh frame and remaps writable (classic CoW). **Reset = drop the dirty overlay + re-point IPAs back at golden RO → cost O(dirtied pages), not O(guest RAM); no re-boot** (but overlay frames MUST be zeroed on free — frame layer is bitmap-only, so "no re-zero" applies only to the RO-golden re-point). Needs: a new EL2 stage-2 permission-fault handler; a VMID free-list + `tlbi` primitive (both absent today — monotonic VMID wraps → cross-VM leak); guest RAM CoW is the easy 80%, **vCPU + device state save/restore is the understated hard part** (`vcpu_regs` captures ≈1/10 of the register surface — GPRs + ELR only, no SCTLR/TTBR/timer/vGIC). **CoW is arch-specific** — ARM64 (stage-2 + VMID + tlbi) and x86 (EPT/NPT + VPID + INVEPT/INVVPID) are both in scope, NOT a shared-core feature. Design plan: [.agents/260722-2330-tier3b-finish-g5-lite/](../.agents/260722-2330-tier3b-finish-g5-lite/).
> 2. **Zero-copy kernel-image load** via Grant remap ([Grant API syscalls 208-211]) instead of `write_guest_memory` copy — Cellos's existing zero-copy primitive.
> 3. **Frame-identity + single-address-space → cheap/safe reclaim**: free = return frames to the allocator (`Drop` already does this, [stage2.rs:453](../kernel/src/memory/stage2.rs)); no cross-address-space unmap or TLB-shootdown gymnastics.
> 4. **Per-cell IOMMU applied to the guest (safety)**: existing 3-level DDT + VT-d SLPT + `sys_grant_dma` confines guest DMA to granted frames **even if the guest kernel is fully compromised**; the stage-2 SAS-isolation guard ([stage2.rs:274-279](../kernel/src/memory/stage2.rs), HPA must stay in the carved region) is the CPU-side twin. A dedicated per-cell path vs relying on the host kernel.
> 5. **Device backends as capability-scoped cells under LBI (safety)**: MMIO holes already trap out of the EL2 core to a hypervisor cell ([stage2.rs:70](../kernel/src/memory/stage2.rs)); pushing virtio backends (net/blk) into separate capability-scoped cells means a bug in one backend cannot corrupt other cells/guests — type + capability isolation, stronger than a monolithic-VMM-process + seccomp (seL4/Genode model). (EL2 core stays kernel-priv; only backends are cells.)
> 6. **never-die + `reap_vms_for_task`** ([registry.rs:531](../kernel/src/hypervisor/registry.rs)) → crashed guest/backend restarts from golden (ties lever 1).
>
> **⚠️ NEW security risk this design introduces (must be solved, not deferred)**: the golden frame set is a **shared trust anchor across all clones**. Stage-2 RO blocks the *guest* from writing it, but the SAS frame-identity invariant means the kernel identity-maps those frames **writable** → a kernel/EL2 bug can poison the golden image and contaminate every tenant/clone. A traditional host keeps the golden image as a file/mmap, not writable-by-default in the VMM's own map. **Mitigation required**: mark golden frames read-only in the kernel identity map too (or checksum-verify before each clone). Also: SAS gives one software boundary (LBI) + hardware (stage-2/IOMMU); it cannot stack a second host-process boundary the way KVM-on-Linux can.
>
> **Positioning (honest, from market research 2026-07-22 — see [.agents/reports/research-260722-1209-g4-market-positioning.md](../.agents/reports/research-260722-1209-g4-market-positioning.md))**: even at Firecracker speed, inside a VM Cellos's LBI/SAS differentiator does not participate (a Linux guest is just a Linux guest), so this is NOT a moat against KVM+Firecracker for untrusted multi-tenant hosting. **Justify G5 by the dual-purpose ROI**: CoW-golden + reset-to-golden + IOMMU-confined guest serve **first-party fleets** (instant VM restart, crash recovery, appliance instant-on) *and* the agent-sandbox latency requirement — so the investment pays off on Cellos's real turf regardless of whether the untrusted-hosting market is ever pursued. Speed removes the *latency* deficit only; the *maturity/operational* deficit (years of production hardening, x86 SVM still MVP) remains.
>
> **Conditions to start**: G4 shipped (or at least Tier 3b x86 matured); real-hardware VMX/SVM/EL2 testbed (TCG cannot validate x86 virt — and it is now unconfirmed whether TCG can even faithfully `VMRUN` a nested-SVM guest; an early spike gates the x86 estimate). Design/spec (golden-frame CoW mechanism, multi-region SAS-guard rework, VMID/VPID lifecycle, TLB/INVEPT invalidation, poisoning + all-teardown-path lifecycle mitigation, RAM-CoW-vs-state-snapshot split) is now-able as a Tier 3b design note — and now spans **both ARM64 and x86** (scope expanded 2026-07-22). **Scope also expanded**: the Wide-preset reference guest is a **full Ubuntu + systemd + apt-persist** image (not a minimal glibc rootfs) — a categorically larger build/integration deliverable. Full design plan + red-team: [.agents/260722-2330-tier3b-finish-g5-lite/](../.agents/260722-2330-tier3b-finish-g5-lite/).

### Milestone → Stage Map

| Item | Source phase | Status | Stage |
|------|--------------|--------|-------|
| Core Stability (VirtIO, kbd, ELF, hotswap) | Phase 1 | ✅ | G1 (foundation) |
| Perf baseline + KASLR | Phase 24 | ✅ | G1 |
| Priority scheduler + RT heap init + spawn_pinned | Phase 25 | ✅ | G1 |
| Memory quota + ZST caps + panic isolation | Phase 26 | ✅ | **G1** (never-die) |
| Capacity observability: typed spawn OOM + opt-in MemInfo | A2/A3 | ✅ DONE 2026-08-01 — real allocator metric is 129.49 MiB; `<10 MiB` optimization remains open | G1 |
| Reliability / supervisor restart | specs/12 | ✅ SUBSTANTIAL (P00-03 DONE 2026-06-06: fault-path force-unlock, reboot-on-panic, guard pages, RT watchdog; P05 done: RecvTimeout deadline, NotifyOnExit supervisor, zombie reaper; P06 observability done) | **G1** |
| Generic completion contract | kernel/task completion + wait plumbing | ✅ Law 1 double confirmation honored; `WaitCompletion` stays additive with `NET_RX` plus finite `TIMER` only, source masks fail closed, the v1 source field uses bytes 12..16 inside the 24-byte record, task-death cleanup runs outside the scheduler lock, `Recv*`/`WaitForEvent` remain intact, and no peer/VFS/DMA/grant source was added; TIMER userspace proof deferred to Phase05 | **G1** |
| Phase 08 stack-sizing gate | per-path stack sizing baseline | ✅ baseline-only QEMU markers PASS for init/shell/vfs/vfs-test; Phase06 closed with two guards + U-mode `cause=0xf` probe; Phase07 closed with six measured paths at 16 usable pages + two guards while unmeasured paths remain 64; x86 VirtIO-MMIO branch fixed; no public ABI change | **G1** |
| Typed IPC + syscall filter (reliability part) | Phase 27-1/2 | ✅ | G1 (next) |
| ELF capability manifests | Phase 30 | ✅ | G1 |
| Heap snapshot / Instant-On | Phase 29 | ✅ | G1 |
| 🆕 Storage 2.0 (zero-copy grant + PageCache + FAT32) | Phases 00–03 | ✅ | **G1/G2/G3** |
| 🆕 Peripheral Driver track (GPIO/I2C/SPI/UART; CAN/PWM/ADC) | *new* | ✅ v2 COMPLETE (GPIO+UART+I2C+SiFive GPIO; SHT3x sensor demo; real SBC pending) | **G1** |
| VFS robustness (quota enforce, access control) | M2.1 | ✅ | G1 |
| 🆕 ARM64 full bring-up (beyond ring-3 smoke) | ext. M1.3 | ✅ COMPLETE (2026-06-12) — 6/6 QEMU integration tests pass (GIC, timer, MMU, VirtIO, PL011 RX, GPIO periph-demo); fatfs LFN fix | **G1** |
| HMI feature-gate (compositor/input, optional) | M2.2/M2.4 subset | 📋 | G1 (opt) |
| Minimal utilities (embedded debug) | M3.2 subset | ✅ DONE 2026-06-16 — standalone /bin/{ls,cat,echo,ps,kill} in sys-tools; embedded in kernel_fs.img + disk | G1 |
| RT latency benchmark | M4.4 subset | ✅ QEMU verified "ALL BENCHMARKS PASS" (2026-06-07) | G1 |
| 🆕 Tier B sub-track (end G1): RV32 HAL + Cellos-Nano + CHERIoT | M4.3 + Phase 31 | ✅ QEMU boot verified (2026-06-07) | **G1** (sub-track) |
| 🆕 Reference robot demo (sensor→compute→actuator + MQTT) | *new* | ✅ COMPLETE (2026-06-16) — full SHT3x I2C + GPIO actuator + MQTT pipeline; `robot-demo-e2e` integration test passes on QEMU ARM64 in 9.83s | **G1** (graduation) |
| 🆕 Distributed Cells L.0+L.1 — robot swarm (net-broker + merge/split + gossip) | *new* | ✅ FOUNDATION CODE COMPLETE (2026-06-23) — net-broker Cell, Noise KKpsk0 p2p, XChaCha20 gossip, remote service proxy, task-claiming lease, enrollment/merge/split; the forwarder/runtime path is still a stub, so do not read this as a shipped two-node runtime. See §L.0 | **G1** |
| 🆕 **Cell-to-Cell Anywhere** — L.2 Internet layer (flagship feature) | *new* | ✅ G1 FOUNDATION CODE COMPLETE (2026-06-24) — P00 Remote-Call API Contract (approved), P01 CellNetId+Ticket+NodeId binding (Law 1 approved), P02 STUN reflexive, P03 DERP relay client; modules compile cleanly (0 errors), but the remote forwarder is still a stub and two-node runtime verification remains pending. G2 plan forthcoming (P04 HyParView+PlumTree, P05 UDP hole-punch, P06 Pkarr/DNS, P07 K2 per-node, P08 K3 DICE). See `.agents/260624-cell-to-cell-anywhere/plan.md` | **✅ G1 (P01-P03) · 📋 G2 (P04-P08)** |
| Distributed Cells L.2 — server cluster control plane | *new* | 📋 PARKED (2026-06-23) — separate problem; reuses L.0 foundation; lean on external k8s/LB. See §L.2 | **G2/G3** |
| Direct-IPC vtable (raw perf) | Phase 27-3 | ✅ | G2 |
| 🆕 Tier 3 kernel prep — H-extension HS-mode boot (RISC-V) | *new* | ✅ COMPLETE (2026-06-07) — cpu_features.rs DTB detection + HypervisorCap ZST + TCB field; see .agents/260607-1420-h-ext-hypervisor-cap/ | **G1 prep** (non-breaking) |
| 🆕 Hardware Key Isolation (Silo — Tier 1 ext., G2 ARM64/x86) | *new* | ✅ COMPLETE 2026-06-16 — SiloHandle API shipped; reclassified from Tier 3a → Tier 1 capability (not a VM tier) | G2 |
| 🆕 Tier 3b Linux VM / x86 VMM | Phase 31 + x86 follow-up | ARM64 EL2 boots Alpine with CI smoke; AMD SVM is an implemented MVP; Intel VMX has root-operation plumbing only; RISC-V H-extension remains pending | **G2** |
| 🆕 **Tier 3b VirtIO-GPU Backend** (Linux VM Graphics / Browser Support) | M2.4 ext. | 📋 | **G2** |
| 🆕 **Enterprise App Isolation** — Wine/Proton-in-Linux-VM Cell + bare Windows VM Cell | new | 📋 G3 on-demand (gated on paying customer + virtio-gpu) | **G3** |
| 🆕 **SMP multi-core scheduler + work-stealing** | Phase 32 | ✅ COMPLETE 2026-06-09 — SBI HSM hart_start/send_ipi, per-hart ViHartLocal via tp CSR, per-hart ready queues + work stealing, RT cells pinned to hart 1, WaitForEvent (217) | **G2** |
| Compositor + GPU desktop (full) + mouse | M2.4 + M2.2 full | 📋 | G2 |
| 🆕 **ViUI v1** (Elm model, FramebufferCanvas, GlyphAtlas, P01–P07) | new | ✅ Done 2026-06-08 — foundation only, design superseded | **G2 prep** |
| 🆕 **ViUI v2** (Reactive Signal Tree + Dual-Layer DSL) | new | 🚧 Implemented library surface — overlays, navigation, charts, `.vi` build integration, virtual lists, FlexBox, and signal bindings are present; signed-App, input/render, compositor-damage, and measured target qualification remain open | **G2** |
| 🆕 **TLS 1.3 stack** `[shared, G1-priority]` | Phase TLS-01 | ✅ COMPLETE 2026-06-07 — Network service supports TLS 1.3 via sys_get_random(214), three TLS IPC opcodes (0x30/0x31/0x32), HTTPS demo verified | **G1** |
| 📋 **TLS server-side accept** `[G2, optional]` | .agents/260623-1500-tls-server-accept | PARKED — plan complete, implement G2 when httpd needs to serve external HTTPS (curl/browser). Swarm uses Noise_KKpsk/NNpsk (separate plan). `tls-server` optional Cargo feature. | **G2** |
| 🆕 **RTC / wall-clock** `[G1]` | new | ✅ COMPLETE 2026-06-07 — Goldfish RTC (RISC-V/ARM64) + CMOS RTC (x86_64); GetTime op=2/3 for epoch_ns/epoch_secs; date binary shows real UTC time | **G1** |
| 🆕 **MMC subsystem** (SDHCI PIO) `[G1 ext / G2]` | Phase M2.6 | ✅ COMPLETE 2026-08-17 — 5 phases done (card init, eMMC/SD variants, PL180 impl, QEMU VirtIO + real SBC routing); real RPi3 Model B v1.2 lane now validated end to end with external SD boot to `Cellos >`, FAT16/FAT32 mounts, and `/mnt/sd` + `/bin` available; RPi4/VisionFive2 ready | **G1** |
| 🆕 **Root board descriptors** (`boards/`) | board contract slice | ✅ COMPLETE 2026-08-18 — seven integration-only descriptors now include QEMU q35 x86_64; `hal/soc/x86` owns COM1/ISA and legacy firmware-window facts while ACPI-discovered MMIO stays fail-closed; shared drivers remain single-copy and CI enforces every board build lane; `boards/qemu/q35-x86_32`, `boards/qemu/virt-riscv32`, and `boards/qemu/virt-aarch32` are placeholder-only; QEMU is integration evidence only and physical boards remain hardware-gated | shared |
| 🆕 **Large-buffer IPC** `[shared, G3 prerequisite]` | Phase M2.7 | ✅ COMPLETE 2026-06-07 — MAX_GRANT_PAGES lifted 16→4096 (16MB cap), grant reaper on task death, GrantRegister/Unregister syscalls 215/216 shipped | **G2/G3** |
| 🆕 **Compositor Grant surfaces** `[M2.4 partial]` | Phases 01–05 | ✅ COMPLETE 2026-06-09 — zero-copy surfaces, damage-driven render, FONT8X8, ViSurface wrapper; replaces WRITE_PIXELS IPC with Grant shared memory | **G2** |
| Hot migration / zero-downtime + cell-signing mechanism | M4.1 + G.2 P2 | ✅ MECHANISMS COMPLETE 2026-06-23; Phase 00 public syscall landing is complete 2026-08-07 — `PauseService` 422 is SupervisorCap-gated with bit 49 and drains pre-pause ingress before Snapshot; `HotSwap` 400 is retired/reserved in Phase 04, `SpawnReplacement` 421 is additive with bit 57, the exact launch-profile intersection is enforced; Phase 01 supervisory atomic cutover is complete 2026-08-07 — paused+Frozen bounded FIFO, source→replacement binding, compare-and-commit rollback, plain resume invalidation of stale bindings, barrier-then-kill-old, and final QEMU hotswap-smoke 13/13 with reviews PASS/CLEAR; Phase 04 kernel cleanup is complete 2026-08-08 — legacy kernel orchestrator and direct wrapper are retired, API tests passed 75/75, release-kernel builds passed for riscv64/aarch64/x86_64, `gen_disk.ps1` rebuilt fresh images and refreshed `kernel/src/embedded/init`, hotswap-smoke passed 15/15, launch-profile passed 1/1, and accepted followups remain x86/AArch64 fresh boot packaging plus host API coverage 33.26 percent line / 0 percent branch. | **G2** |
| 🆕 x86_64 full bring-up | ext. M1.3 | ✅ COMPLETE (2026-07-11) — APIC/HPET/MMU + PCIe Driver-Cell stack (platform/nvme/e1000 in VIFS1); FAT32-on-NVMe end-to-end incl. under VT-d per-Cell DMA isolation; sysretq preserve-all ABI fixed; 4 QEMU suites 12/12 (the 2026-06-13 "5/5" claim ran on a stale pre-G2 ISO) | **G2** |
| VFS scale (FAT32/ext4, large disks) | M2.1 ext. | 📋 | G2 |
| Full utility suite (grep/sed/awk/top/ps…) | M3.2 full | ✅ COMPLETE 2026-07-28 — grep `-F/-E/-e/-f/-i/-v/-n/-c/-q/-x/-r` with 0/1/2 statuses, one-command sed (alt delimiter, `&`, numeric/regex addresses), mini-AWK (`-F`, NR/NF, `$0..$9`, comparisons, arithmetic, print), `top` batch/interactive on the new `GetProcs2` telemetry ABI; ERE-lite is bounded + linear-time via regex-automata, explicitly not POSIX-complete. Pure stages live in `libs/text-engine` (38 host tests); 33/33 guest scenarios green under QEMU rv64 | **G2** |
| Throughput benchmark (SMP) | M4.4 subset | ✅ DONE 2026-06-16 — 3 SMP scenarios in bench cell: spawn_rate(≥20/s), ipc_throughput(≥5000/s), work_distribution(scale≥1.4×); QEMU-TCG caveat logged | G2 |
| Lua / MicroPython runtimes | M3.3/M3.4 | ✅ | shared |
| Advanced IPC (SendGather/RecvScatter/Timeout) | M4.2 | ✅ | shared |
| Network TCP/UDP/DNS/MQTT | Phases A–E | ✅ | shared |
| Enhanced shell (pipes/redirects/tab) | M3.1 | ✅ | shared |

### 🆕 New Work Items (not in original numbering)

#### Peripheral Driver Track `[G1]`
**Status**: ✅ v2 COMPLETE (2026-06-13) — GPIO+UART+I2C+SPI bit-bang all done on QEMU ARM virt
**Priority**: P1 (defining requirement for "complete for robots")

HAL bus traits + driver Cells for sensor/actuator control. Capability-gated via ELF manifests (Phase 30).
- [x] HAL traits `ViGpio` (`hal/traits/gpio/`) + `ViUart` extension (`hal/traits/uart/`)
- [x] `ostd::mmio::MmioRegion` — safe MMIO accessor (`#![forbid(unsafe_code)]` compatible)
- [x] Kernel Resource Registry — exclusive MMIO ownership + allowlist + release-on-exit
- [x] `sys_request_mmio` (opcode 213) + `MANIFEST_FLAG_GPIO/UART` (Law 1 confirmed)
- [x] `driver-gpio` (PL061 impl) + `driver-serial` (PL011 impl)
- [x] `periph-demo`, `periph-test` (4 scenarios), `robot-demo` skeleton
- [x] `run-arm-virt.ps1` — QEMU ARM virt boot script
- [x] **Done (2026-06-12)**: aarch64 kernel build — 6/6 integration tests pass on QEMU virt; periph-demo GPIO verified
- [x] **Track C (2026-06-13)**: `ViI2c` + `BitBangI2c<G>` + `sensor-demo` (SHT3x) + linker scripts
- [x] **Track C (2026-06-13)**: `ViSpi` (`hal/traits/spi`) + `BitBangSpi<G>` (pins 2-5, Mode 0) + `spi-demo` + integration test `periph-i2c-spi`
- [ ] Extension: `ViCan`, `ViPwm`, `ViAdc` (G1 ext / G2)
- [x] **VisionFive2 JH7110 bring-up** (2026-06-29) — `board-vf2` feature, Limine UEFI image, flash scripts, PLIC/UART addresses match QEMU virt (zero HAL changes). `docs/vf2-bringup.md`. Pending: physical hardware run.
- [x] **Pioneer SG2042 bring-up** (2026-06-29) — `board-pioneer` feature, SBI DBCN console (UART sv39-inaccessible), T-Head PLIC/CLINT compat strings, flash scripts. `docs/pioneer-bringup.md`. Pending: physical hardware run.

> ⚠️ Largest new chunk of G1 — needs its own brainstorm → plan → cook cycle. Do not underestimate.

#### Architecture Full Bring-Up (split from "Multi-Arch HAL ✅")
The existing Milestone 1.3 marks ARM64/x86_64 as **ring-3 smoke only**. Real targets need full bring-up (interrupt controller, timer, real MMU, device drivers).
- **RV64 firmware memory discovery `[G1]`** ✅ COMPLETE (2026-07-31) — direct OpenSBI boots
  consume all enabled DTB RAM ranges, subtract firmware/kernel/FDT reservations, and fail closed
  to audited board maps on malformed or unsupported input. A QEMU 2 GiB capacity gate manages
  more than 1.95 GiB after reservations; focused runtime gates pass. Fresh full-suite rerun is
  still pending after the serial harness timed out near test 28.
- **ARM64 full bring-up `[G1]`** ✅ COMPLETE (2026-06-12) — GIC, generic timer, 3-level MMU, VirtIO, PL011, PL061 on QEMU virt; 6/6 integration tests pass
- **x86_64 full bring-up `[G2]`** ✅ COMPLETE (2026-07-11) — APIC, HPET/TSC, real MMU, UART RX; PCIe Driver-Cell stack (platform ECAM + NVMe + e1000 cells, user-mapped MMIO); FAT32-on-NVMe end-to-end incl. under VT-d per-Cell DMA isolation; sysretq preserve-all ABI + CVE-2012-0217 check; 4 QEMU suites 12/12 (supersedes the 2026-06-13 claim, which ran on a stale pre-G2 ISO with kernel drivers)

#### Reference Robot Demo `[G1]`
**Status**: 🆕 — **G1 graduation gate**
End-to-end loop: sensor read → compute → actuator write over GPIO/CAN, with MQTT telemetry. Proves the embedded stack works as a whole.

#### Tier 3: Hypervisor / Virtualization `[G1-prep + G2]`
**Status**: 🆕 DESIGNED — spec at [specs/05-application.md §4](specs/05-application.md)
**VMM**: Custom **minimal VMM** (~9K LOC Rust, built from scratch as Tier 1 cell). microvm profile — MMIO bus, no PCI. VirtIO blk/net/console backends forward to Cellos VFS/Net IPC. No tokio, no mmap — SAS-native. (crosvm fork rejected: ~75K LOC, tokio+mmap incompatible with SAS cell constraints.)

Two sub-items (Silo reclassified — see Hardware Key Isolation entry above):
- **Tier 3 kernel prep** `[G1-prep, non-breaking]`: RISC-V H-extension detect + HS-mode boot path (`hal/arch/riscv/hypervisor.rs`, ~200 LOC). `HypervisorCap` ZST token gates hypervisor syscalls (follows existing BlockIoCap/NetworkCap pattern). Transparent fallback to S-mode if H-ext absent.
- **Tier 3b Linux VM** `[G2, Phase 31]`: minimal VMM, boot Alpine Linux, VirtIO → Cellos IPC. Enables `apt install nginx`. CPU overhead ~5-10% (H-extension hardware virt), disk I/O ~20-40% (VirtIO roundtrip) — acceptable for management plane.

> See [specs/05-application.md §6](specs/05-application.md) for wrong-path list (no QEMU-as-cell, no Type-1 hyp, no crosvm fork, no Android in G2).

### Graduation Criteria

**G1 — Robot/Embedded is "done" when:**
1. ✅ Never-die: a single Cell fault/OOM → killed & restarted, kernel survives.
2. ✅ Bounded memory enforced on EVERY write path (Write/Append/IPC).
3. ✅ RT determinism: a control-loop Cell meets its deadline; IPC latency has a measured bound.
4. ⚠️ Peripheral I/O: GPIO/I2C/SPI/UART work on QEMU ✅ + ≥1 real board (pending hardware acquisition).
5. ✅ Instant-On boot under target threshold.
6. ⚠️ Runs on real RV64 + ARM64 SBC: QEMU full bring-up ✅, SBC bring-up code complete (VisionFive2 + Pioneer SG2042, 2026-06-29) — pending physical hardware run.
7. ✅ Sub-track: Cellos-Nano minimal profile boots on RV32 (QEMU verified).
8. ✅ Reference robot demo runs end-to-end (`robot-demo-e2e` passes on QEMU ARM64, 2026-06-16).

**G2 — Server/PC is "done" when:**
SMP scales across N cores · windowed desktop + mouse · hot migration with no dropped connections · x86_64 full bring-up · full utility suite + large storage · throughput benchmarks meet targets · **Linux VM boots inside Tier 3 (minimal VMM) and runs a real workload (nginx serving HTTP)** · RISC-V AI inference server demo: HTTP → NPU cell → response with P99 latency bound.

---

## 🧩 Application Platform Gaps (backlog — brainstorm+plan pending)

> Added 2026-06-06 after a first-app feasibility study ([researcher-260606-1041-first-app-candidates.md](../.agents/reports/researcher-260606-1041-first-app-candidates.md)).
> **Finding:** Cellos today is a solid kernel + thin userspace; the *application-platform* layer is missing,
> so candidate apps come out as toys or narrow plumbing. The gaps below are what unlocks **real** apps.
> Each is a backlog item to be brainstormed + planned individually. Status 📋 = not yet planned.

### 🌱 First Real App: **Hypha** — the gap-closure driver `[G1]`

> Decided 2026-06-21. Plan: [.agents/260621-1433-hypha-ai-agent/](../.agents/260621-1433-hypha-ai-agent/) (plan.md · architecture.md · **os-gaps.md** · phase-00). App home: `cells/apps/hypha/`.

**Hypha** (sợi nấm — one thread of the *Mycelium*) is Cellos's first **real** application: a native
Tier-1 Rust **AI agent Cell**. Unlike the demos (which each prove one primitive), Hypha is useful
*and* showcases what is unique to Cellos:
- **Capability-isolated tools** — each tool is a separate Cell; manifest declares its exact authority,
  kernel-enforced. The agent core holds no dangerous capability and delegates all side-effects.
- **Never-die** — kill the LLM gateway mid-conversation → supervisor respawns → agent reconnects via
  service lookup → conversation continues.
- **Zero-copy IPC at scale** — multi-KB prompts/responses move via Grant, not message-copy.
- **Natural-language robot control** — ties into the G1 robot demo (sensor → reason → actuator).

**Strategic role:** Hypha is the **forcing function** for this whole section. Building it surfaces
the missing modules below and prioritizes them by real need; gaps are tracked in the plan's
`os-gaps.md` and filled incrementally. **Closed 2026-06-21:** no HTTP lib ✅, no_std JSON ✅.
**Remaining top gaps:** public-DNS-over-NAT unverified, fixed-only service IDs. Design inversion
vs a Unix agent: it orchestrates **Cells via IPC + spawn**, not processes via fork/exec.

**Repo layout:** a cluster of normal workspace-member crates (`llm-gateway`/`core`/`tools/*` +
shared `libs/agent-proto`) — **not** a git submodule (each os-gap fill is an atomic commit spanning
Hypha + `ostd`/`api`/kernel). Revisit extraction only after P4.

**Phases:** P0 `llm-gateway` (HTTPS LLM client, extends `https-demo`) → P1 core chat → P2 tool
protocol + `tool-fs` → P3 `tool-sys`/`tool-spawn` → **P4 `tool-peripheral` = robot NL-control (G1
showcase)** → P5 persistence/memory → P6 ViUI chat → P7 G3 NPU backend.

**Status:** 🔨 P0 in progress (started 2026-06-21).

### A. Hardware I/O `[G1]`
- **Peripheral bus** (GPIO/I2C/SPI/CAN/PWM/ADC) — 📋 already designed → see "Peripheral Driver Track" + [specs/13-peripherals.md](specs/13-peripherals.md). #1 gap: no app reads sensors / drives actuators without it.

### B. Interaction `[G1 input · G1-opt/G2 display]`
- 🆕 **P0 UART input delivery to apps** `[G1]` — ✅ COMPLETE (2026-06-15). UART bytes now relayed to input service via EV_ASCII opcode (0x04) on all arches; ARM64 integration test green. Apps can register for input focus and receive keyboard events. See [.agents/260615-p0-uart-input-delivery/](../agents/) for details.
- **Display / GUI** — 📋 see Milestone 2.4 (compositor/GPU, HMI feature-gate). Blocks user-facing graphical apps.

#### Shell-on-screen: 3 tiers (hiện tại shell chỉ trên UART serial — cần build thêm để hiện trên màn hình HDMI)

> **Tại sao cần**: trên board thật cắm màn hình, shell tương tác hiện tại yêu cầu USB-UART adapter. Các tier dưới đây giải phóng board khỏi serial cable.

- 📋 **Mức A — fb_console keyboard relay** `[G1-ext]` — Kernel `fb_console` đọc key events từ input service → relay sang UART shell. Màn hình hiện output shell (font cố định, không scroll). Nhanh: ~1 tuần, không cần Terminal Cell. Dùng cho kiosk/panel không cần cable.
  - Phụ thuộc: input service ✅, fb_console ✅ (chỉ cần nối keyboard relay)
  - Giới hạn: font cố định, không scroll, không ANSI color — "shell trên TV" cơ bản.

- 📋 **Mức B — Terminal Emulator Cell (VT100)** `[G2 Desktop]` — App cell VT100 emulator: render text lên compositor surface (ViUI font rendering + scrollback), nhận keyboard từ input service, IPC pipe output shell qua relay syscall. Tương đương `xterm` trên Linux — full ANSI color, resize, scrollback.
  - Phụ thuộc: Mức A + compositor grant surfaces ✅ + ViUI text rendering ✅
  - Effort: ~3-4 tuần
  - Mở khóa: shell tương tác đầy đủ trên HDMI không cần cable, đúng nghĩa "shell như Linux trên màn hình".

- 📋 **Mức C — SSH remote access** `[G2 Server]` — Tier 3b Alpine Linux VM cài `dropbear`/`tinyssh`; forward cổng SSH qua VirtIO net. Remote shell từ PC khác qua mạng.
  - Phụ thuộc: Tier 3b Linux VM ✅ + VirtIO net ✅
  - Effort: ~1 tuần (cấu hình, không code kernel)
  - Không cần nếu đã có Mức B (Mức C chỉ thêm remote access).

### C. Real-world connectivity `[G1 priority · shared]`
- 🆕 **TLS 1.3 for the net stack** `[shared, G1-priority]` — ✅ COMPLETE (Phase TLS-01). Network service now supports TLS 1.3 client handshake via sys_get_random(214) entropy + three TLS IPC opcodes (0x30/0x31/0x32). HTTPS demo cell connects to example.com:443, validates cert chain, issues HTTP GET. Foundation for MQTT over TLS, secure device communication, IoT protocols.
- 📋 **TLS server-side accept** `[G2, optional]` — PARKED. Full plan at [.agents/260623-1500-tls-server-accept/](.agents/260623-1500-tls-server-accept/). Needed when httpd must serve external HTTP clients (curl/browser) over HTTPS. Robot swarm uses Noise_KKpsk/NNpsk instead (separate plan). Dual-stack design: `tls-client` default (embedded-tls, keeps nano-robot working) + `tls-server` optional Cargo feature (rustls 0.23).
- 🆕 **RTC / wall-clock time** `[G1]` — ✅ COMPLETE (2026-06-07). Goldfish RTC (RISC-V/ARM64) + CMOS RTC (x86_64); GetTime op=2/3 for epoch_ns/epoch_secs; date binary shows real UTC time with fallback to uptime. See [.agents/260607-1719-rtc-wall-clock/plan.md](.agents/260607-1719-rtc-wall-clock/plan.md)
- 🆕 **Large-buffer IPC / scatter-gather** `[shared, G3 prerequisite]` — ✅ COMPLETE (2026-06-07). Grant API shipped: `GrantAlloc/GrantShare/GrantSlice/GrantFree` (syscalls 208–211), `BlkReadAsync` (212), `GrantRegister/GrantUnregister` (215–216); MAX_GRANT_PAGES=4096 (16MB cap); grant reaper on cell death. Zero-copy page-table remap, no memcpy. See [.agents/260606-2021-storage-2-zero-copy-grant/](.agents/260606-2021-storage-2-zero-copy-grant/) and [.agents/260607-1747-large-buffer-ipc-grant-pages/](.agents/260607-1747-large-buffer-ipc-grant-pages/).

### D. App SDK / ergonomics `[shared]`

> **Decision (2026-06-14):** `ostd` IS Cellos's std — do NOT build a `std` facade (std assumes Unix process model, contradicts SAS/LBI). The three gaps below are what unlock real native apps without false familiarity. See brainstorm `.agents/brainstorms/260614-native-app-std.md` (to be written).

- 🆕 **Name service** `[shared]` — ✅ REGISTRY SHIPPED (2026-06-06). `sys_register_service(id, tid)` (syscall 205) + `sys_lookup_service(id)` → tid (206); fixed service-ID registry (`service::VFS`, `service::NET`, etc.); kernel init auto-registers bootstrap services; lookup-based clients replace hardcoded TIDs. See [project-service-id-registry.md](../MEMORY.md). 📋 Residual gap: string-name / dynamic registration still fixed-ID-only (Hypha os-gaps).
- ✅ **High-level cell libraries** `[shared, COMPLETE 2026-06-21]` — HTTP/1.1 + no_std JSON shipped. `libs/http-core` (pure, host-testable protocol) + `ostd::http`/`ostd::json` (feature-gated). `HttpClient<T>` generic over `embedded_io::Read+Write` (TcpStream/TlsStream); serde_json optional (zero link cost if unused). 51 host tests, `cells/demos/http-smoke` reference Cell. Known: HTTPS binary bodies unreliable (net-cell frame-length gap); cert verification deferred. Hypha P0 unblocked.
- 🆕 **Python/scripting story** `[G2]` — Python R&D users: full CPython via Tier 3 Linux VM (`apt install python3 pip numpy torch` → works). Lua/MicroPython native runtimes **dropped** (half-measure). Robot code stays Rust (Tier 1). Milestones 3.3/3.4 marked complete but runtimes not actively maintained.
- 🆕 **Async runtime exposed to apps** `[shared]` — 📋 no app-facing async executor for concurrent I/O.
- ✅ **`embedded-io` traits for ostd** `[shared, COMPLETE 2026-06-15]` — `embedded_io::Read` impl'd for `ostd::fs::File` + `Stdin`; `embedded_io::Write` impl'd for `Stdout` + `File` (via `VfsRequest::Append` IPC, chunked at 400B). Opens the no_std embedded-crate ecosystem. **Gate for high-level cell libraries: cleared.**
- ✅ **`HashMap` in ostd prelude** `[shared, COMPLETE 2026-06-15]` — `hashbrown` already in `libs/ostd/Cargo.toml`; `ostd::collections::HashMap`/`HashSet` exported; re-exported in `ostd::prelude`. Was already shipped — roadmap was stale.
- 🆕 **Cellos App SDK** `[shared, G1-tail]` — ✅ COMPLETE (2026-06-16). L1 SDK shipped in [.agents/260616-0705-app-sdk-l1/](.agents/260616-0705-app-sdk-l1/): `ostd::app_entry!` macro (manifest + syscall allowlist auto-generated), `AppContext` (service discovery via `ctx.vfs()`, `ctx.net()`, etc.), typed `AppEvent` loop (`Init/Message/Shutdown`), canonical app pattern in `cells/apps/sdk-demo`. Documented in CLAUDE.md § "App Entry Patterns — Tier 1 Native Rust". Unlocks real native apps without boilerplate.
- 🆕 **Cell `--help` / help UI** `[shared, G1-tail]` — 📋 No cell currently documents itself at runtime. Standard: CLI cells parse `--help` as the first spawn arg and print usage/description to stdout then exit; GUI cells (robot-dashboard, compositor) show a Help overlay or menu. Prerequisite: `ostd::args()` helper that reads the spawn-args buffer set by `sys_set_spawn_args` — currently a raw `[u8; 64]` with no typed accessor. Service cells (vfs, net, input) are not user-facing and do not need `--help`. Effort: ~1 day (ostd helper ~30 LOC; each CLI cell adds a `match args[0] { "--help" => { ... } }` guard).

### E. Ecosystem / distribution `[G2]`
- ✅ **Tier 1b C library integration** `[shared, COMPLETE 2026-06-13]` — link vendor C/C++ libraries (NPU SDK, mbedTLS, SQLite, legacy firmware) into Rust cells via `Cellos-libc` (Newlib + POSIX shim). Shim in `libs/api/src/posix.rs`: malloc/free, strings, file I/O, time → ViSyscall, getentropy → `ViSyscall::GetRandom` (op 214), socket/connect/send/recv/close → typed Net IPC (postcard). ARM64 `svc #0` ABI added; send() postcard decode bug fixed; `_time()` op code fixed (op=3 = epoch seconds). Integration tests: `posix_shim_getentropy` + `posix_shim_net` in `tests/integration/tests/boot.rs`. No `fork` by design. Primary use case: hardware NPU SDKs (RKNN/Hailo/K230). Plan: `.agents/260613-0520-tier1b-posix-shims/`. See [specs/05-application.md §3](specs/05-application.md).
- 🆕 **Tier 1b Zig Support** `[G1/G2]` — ✅ COMPLETE (2026-06-23). Level A (raw syscalls via `libs/zig-syscall`) + Level B (mlibc). Plan: [.agents/260623-0834-tier1b-zig/](.agents/260623-0834-tier1b-zig/); cells: `tests/zig-hello` (L.A) + `tests/zig-mlibc-smoke` (L.B). Validates SAS with Zig natively alongside C/Rust. 📋 Residual demo: Tetris.zig port (not critical for graduation).
- ✅ **C Runtime: picolibc libm cherry-pick** `[G1, COMPLETE 2026-06-17]` — 9-module split of posix.rs (alloc/strings/sysio/entropy/net/math/stdio_fmt/stdio/setjmp), 96+ C99 math symbols via libm crate, full stdio family (FILE/fopen/fclose/fread/fwrite), naked-asm setjmp/longjmp for RV64/ARM64. Zero picolibc dependency. Enables: DOOM, codec libs (zlib/libpng), MicroPython/Lua math. c-math-smoke cell (12 scenarios) verifies all three stacks end-to-end.
- 🆕 **C Runtime: mlibc migration** `[G2]` — ✅ COMPLETE (2026-06-17). [mlibc](https://github.com/managarm/mlibc) (MIT) integrated as `third_party/mlibc/`, sysdeps mapping Cellos primitives (`open/read/write` → VFS IPC, `clock_get` → sys_get_time, `socket` → Net IPC). Shipped in [.agents/260617-1000-mlibc-integration/](.agents/260617-1000-mlibc-integration/). Cell precedent: `mlibc-smoke` on aarch64 via WSL2 clang. **Does NOT unlock fork-based software** — nginx/PostgreSQL → always Tier 3 VM (fork incompatible with SAS). **Does unlock:** broader single-process C apps, vendor C/C++ SDKs (RKNN/Hailo/codec libs). posix.rs remains Tier A default (simpler cells).
- 🆕 **Package manager / app distribution** `[G2]` — 📋 no install/update mechanism beyond baking into the disk image. Plan drafted 2026-07-12: [.agents/260712-1000-cell-package-distribution/](.agents/260712-1000-cell-package-distribution/).

### F. G2 Server Strategy — ARM64 Graduation Demo + RISC-V Latency Demo `[G2]`

**Decision (2026-06-06, updated 2026-06-11):** G2 value proposition = **latency guarantee + reliability + security**, NOT throughput. Not competing with LLM GPU throughput (5-30× gap) or general x86 workloads.

**⚠️ Hardware correction (2026-06-11 research):** C930 = Alibaba IP core (RTL delivery to licensees March 2025, no SoC/board before 2027). P870 = SiFive IP licensed by Sophgo — no standalone P870 chip purchasable. H-ext (hypervisor extension) absent from ALL shipping RISC-V chips — blocks Tier 3b VM plane on RISC-V. See `docs/research/research-riscv-ai-ecosystem.md`.

**G2 graduation demo: ARM64 RK3588 first (not RISC-V)**

Primary graduation target: **Radxa ROCK 5B+ 16GB (~$149)** — Rockchip RK3588.
- NPU: 6 TOPS INT8, RKNN SDK v2.3.2 (mature, C API `rknn_init`/`rknn_run`/`rknn_query` → Tier 1b FFI)
- Tier 3b: Alpine Linux VM via KVM EL2 (confirmed, 4 vCPU limit) — ARM64 EL2 works NOW; RISC-V H-ext does NOT exist yet
- Cellos = first custom OS with deterministic NPU inference on RK3588 (Zephyr = UART-only; Redox = no port)

Parallel track: Milk-V Pioneer (SG2042, ~$600) for RISC-V P99 latency story — no NPU needed there.

**Two-plane architecture:**
```
DATA PLANE (performance-critical, Tier 1 + 1b):
  HTTP → Net Cell → Inference Cell (Tier 1b + RKNN/nncase SDK) → response
  Zero-copy grant, RT-bounded, <10ms P99

MANAGEMENT PLANE (ecosystem, Tier 3b):
  Alpine Linux VM — Prometheus, SSH, admin tools, PostgreSQL
  ARM64: KVM EL2 (works today) | RISC-V: H-ext absent → separate mgmt node or deferred
  overhead: ~5-10% CPU, ~20-40% disk I/O, 1-5s boot (one-time)
```

**Value vs Linux + nginx:**

| | Linux | Cellos G2 |
|---|---|---|
| Inference P99 latency | Best-effort | RT-bounded per cell |
| NPU cell crash | System hung / cold restart | Supervisor respawn (never-die) |
| Memory copies (net→NPU→resp) | 3-4 copies | 0-1 (zero-copy grant) |
| Security (model weights, keys) | Process isolation | Stage-2 Security Silo |

**G2 graduation criteria (updated):**
- ARM64 bring-up on RK3588: U-Boot → Cellos EL1 → Cell ecosystem running
- RKNN inference Cell: HTTP request → NPU → response, P99 latency bounded
- Tier 3b Alpine VM: KVM, boots, runs real workload (Prometheus/SSH)
- Never-die: NPU cell crash → supervisor auto-restart, inference continues
- RISC-V parallel: P99 latency demo on Pioneer (SG2042, no NPU required)

**Real RISC-V hardware path (no vaporware):**

| Phase | Board | Price | Purpose |
|---|---|---|---|
| Now (RISC-V dev) | Milk-V Pioneer (SG2042) | ~$600 | 64-core RISC-V, mature Linux BSP |
| Now (RISC-V RVV bench) | BPI-F3 (SpacemiT K1) | ~$100 | RVV 1.0 measured, llama.cpp 8.6 t/s |
| G2 demo | Radxa ROCK 5B+ (RK3588) | ~$149 | ARM64 NPU graduation demo |
| G2 future | SG2044 SRA3-40 | TBD | RVV 1.0 + DDR5, IF H-ext ships |
| Long-term | C930 SoC (unknown) | TBD | 2027+ IF H-ext confirmed |

See also: [.agents/reports/brainstorm-260606-2016-g2-riscv-server-strategy.md](.agents/reports/brainstorm-260606-2016-g2-riscv-server-strategy.md) · [docs/research/research-arm64-g2-hardware.md](research/research-arm64-g2-hardware.md) · [docs/research/research-riscv-ai-ecosystem.md](research/research-riscv-ai-ecosystem.md)

### G. Security Platform `[G2]`

> Added 2026-06-19 after Security Model design session. Expanded 2026-06-21 with two deep dives.
> **Full menu + status + citations:**
> [research-hardware-isolation.md](research/research-hardware-isolation.md) — *memory* isolation (Cell can't read another Cell's memory; rated vs the SAS "no-TLB-flush-per-Cell-switch" criterion), and
> [research-cell-security-permissions.md](research/research-cell-security-permissions.md) — *permission* model + hardware attestation (Cell can only do what it's granted + can prove its identity).
> The two are orthogonal axes.

**Hardware-isolation delivery model (owned by Spec 19):**
```
Layer A — W^X after relocation       → code/constant integrity         [DONE]
Layer B — Per-domain page tables     → untrusted native-cell wall      [PLANNED]
Layer C — Per-arch hardening         → opportunistic MTE/MPK bonuses   [HW-GATED]
```

> LBI, CFI, DMA isolation, and Tier-3 Silo/VM protection remain separate security-stack
> mechanisms; MTE/MPK/PMP are not a substitute for Layer B and do not mitigate Spectre.

**🟢 IOMMU DMA isolation (previously 🔴 CRITICAL gap — NOW FIXED):**
- ✅ **Per-Cell DMA isolation (2026-06-22)** `[G1-hw / G2]` — IOMMU upgraded from bare passthrough (`DDTP MODE=1`, IOVA==PA, zero DMA isolation) to per-Cell translate mode. **RISC-V**: 3-level DDT (MODE=3LVL), per-Cell Sv39 domains, unique PSCIDs, PSCID free-list, IOTINVAL.VMA/IOFENCE.C/IODIR.INVAL_DDT. **x86**: per-Cell VtdSlpt + DID, ECAP.IRO-computed IOTLB offsets, PSI/DSI IOTLB flush, context-cache DSI invalidation. **Cell exit**: `cleanup_cell()` in Exit/ForceExit/watchdog paths, IOFENCE/IVT flush before frame reclaim. **Capability**: new `sys_grant_dma(233)` syscall (BDF ownership, DMA quota = 1× memory quota, page alignment). Kernel enforces DMA quota via `can_map_dma()` + `record_dma_mapped/unmapped()`. Zero DMA attack surface — peripherals pinned to kernel domain; user Driver Cells request authorization via syscall. See docs/research/research-hardware-isolation.md for closure of the Thunderclap gap. NIC/NVMe still kernel-local; userspace Driver Cells (future) use syscall. Both arches boots clean; syscall ABI tests pass. **Hardware isolation research gap CLOSED.**

**Hardware-supplement implementation plan — 5 phases delivered (2026-06-23; enforcement varies)**
- **P01 ARM64 BTI+PAC-RET** ✅ — SCTLR_EL1.BT0/BT1/APIAKEY_EL1 init, compiler flags `+bti,+paca,+pacg`, runtime detection via ID_AA64PFR1_EL1/ID_AA64ISAR1_EL1
- **P02 ARM64 MTE** ✅ implementation — ViMte trait, AArch64Mte impl (SCTLR_EL1.ATA/ATA0/TCF/TCF0), STGP tag writes, sync/async fault modes; runs only where FEAT_MTE exists (QEMU or future Armv8.5+ hardware), not RK3588
- **P03 x86_64 CET-IBT** ✅ — CR4.CET + MSR_IA32_S_CET ENDBR_EN, ENDBR64 landing pads on all ring-3 stubs, #CP (IDT vec 21) handler
- **P04 x86_64 PKU plumbing** ⚠️ — CR4.PKE, task PKRU values, WRPKRU guards on iretq+sysretq, and CET-IBT prerequisite are wired; PTE key tagging is absent, all pages remain key 0, and isolation is not enforced
- **P05 Testing** ⚠️ — CFI/MTE tests and feature gates exist; the PKU self-test checks constants + kernel RDPKRU only and does not attempt a denied keyed-page access

**Backlog items:**

- 📋 **rustc TCB documentation** `[immediate]` — Document that rustc IS the Trusted Computing Base. Add to `docs/specs/00-context.md`. A compromised compiler bypasses all LBI guarantees — this must be explicit in threat model.
- 📋 **PKU PTE key tagging (G2 follow-up)** `[G2]` — Loader fills PTE bits [62:59] with cell-assigned key during load; WRPKRU enforcement becomes active (currently PKU is wired but keys are all-zero, so enforcement is bypassed). Prerequisite: CET-IBT already enforced (P03 complete, addresses JOP gadget threat).
- 📋 **RISC-V PMP / Smepmp firmware study** `[G1-ext / G2]` — PMP CSRs and violation handling are M-mode concerns; Cellos S-mode cannot write or switch them. Any dynamic C-tier design therefore requires a custom, separately approved M-mode firmware owner and must not replace Spec 19 Layer B. Static boot-time guards remain the nearer option.
- 📋 **RISC-V WorldGuard / Smmtt** `[G2 future, watch]` — Beyond PMP, both isolate domains in one address space **without TLB flush**. **WorldGuard** (SiFive→RISC-V Int'l, QEMU 4/2025): 1 WID CSR write/switch, ≤32 worlds, propagates to bus fabric (covers DMA too). **Smmtt/Smsdid** (draft): per-SDID physical-page access control, SDID switch + MTT-fence (lighter than SATP). Design Cell scheduling + grant API to be SDID/WID-aware now. Available when SiFive P/E-series silicon ships.
- 📋 **Confidential computing for Tier 3** `[G2/G3]` — TDX/SEV-SNP (x86), **ARM CCA/RME/GPT** (ARMv9.3, Fujitsu Monaka ~FY2027) protect against a *compromised kernel/hypervisor* — a threat LBI does NOT cover. Make the Tier 3 `VmHandle` ABI CC-neutral now so attested multi-tenant slots in without protocol redesign (extends the Silo "safe even if kernel compromised" principle).
- ✅ **Cell-signing mechanism** `[G1 dev/test]` — The common loader gate verifies a present Ed25519 signature and rejects an invalid one. Default builds still admit absent signatures, and the public dev seed is a reproducible test fixture rather than a provenance root.
- 📋 **Fleet-secure Tier-1 admission** `[G2]` — Provision an immutable fleet public key; enable `signing-required` and `policy-required` in a named production profile; exclude dev-key and weak-RNG features; bind reviewed source to the signed artifact in controlled CI/KMS; add negative tests for unsigned, stripped, wrong-key, dev-key, tampered, and unchecked-dev-signed ELFs; anchor the kernel and embedded key in secure boot. Signature status does not select Tier-1/Tier-2 memory mapping today.
- 📋 **Key Management Service (KMS Cell)** `[G2]` — Tier 1 service cell wrapping `SiloHandle`. Exposes `sys_lookup_service(service::KMS)` + typed IPC for Wrap/Unwrap/Derive keys. First client: TLS stack (replace hardcoded keys).

#### G.2 Permission model + attestation `[G1/G2 — needs its own plan]`

> Added 2026-06-21 from the per-Cell security deep dive ([research-cell-security-permissions.md](research/research-cell-security-permissions.md)).
> Current state: the manifest is one `flags: u8` (FULL), coarse, granted all-at-spawn, no scoping/delegation/revocation/consent — i.e. **Android pre-6.0 install-time model**. The four capability-OS invariants (no ambient authority · explicit delegation · monotonic downgrade · revocable) are all violated today. Reference: Fuchsia `.cml` routing, seL4 badges, Genode session-args, Capsicum one-way ratchet.
> ⚠️ **Headless-robot reframe:** consent dialogs are a UX primitive, not a security primitive. G1 (headless) → signed **operator/fleet policy** (ROS 2 SROS2-style), NOT dialogs. G2 HMI → optional TCC-style consent for *sensitive caps only*, with anti-fatigue rules.
> Hard invariant: manifest = **ceiling not floor** (iOS entitlement lesson); **only the kernel enforces** (consent feeds the syscall-boundary check — where TCC repeatedly failed); LBI already closes the TCC "permission-laundering via injection" hole.

- 🟡 **Parameterized capabilities** `[G1, no Law 1]` — Attach scope params so a cap carries WHICH resource, not just yes/no (= Genode session-args / Capsicum CAP_IOCTL whitelist).
  - ✅ **Device-scoped MMIO (2026-06-21)** — `mmio_cap: bool` → `mmio_devices: u8` (`DEV_GPIO`/`DEV_UART` in `resource_registry`); `request_mmio` now requires the range's device class ∈ the cell's declared devices. Closes the gap where a GPIO-only cell could claim the UART window. Kernel-only, no ABI change (manifest already separates gpio/uart). Compiles clean on riscv64 + aarch64. Files: `resource_registry.rs`, `task/tcb.rs`, `loader.rs`, `task/syscall.rs`.
  - 📋 BLOCK_IO `lba_range` — partly present (`block_regions` partition bitmask + `check_block_access`); extend to arbitrary LBA ranges if needed.
  - 📋 NETWORK `proto_mask + host/port allowlist` — enforced in the net **service** cell (not kernel — net is a service), so it ships with net-cell work, not here.
  - ⚠️ **GPIO per-pin is NOT kernel-enforceable** — cells own the GPIO MMIO directly (app-owns-MMIO, no broker), so the kernel cannot gate individual pins without a GPIO broker cell (deliberately rejected). Device-class is the enforceable granularity.
  - 📋 General `__Cellos_cap_args` ELF section — only needed for params the kernel can't derive from existing flags; deferred until a concrete case appears.
- ✅ **Spawn-time cap intersection (delegation) (2026-06-21)** `[G1]` — `spawn_from_path(path, Spawner)` grants `manifest ∩ spawner_caps`; a Cell cannot hand a child a cap it lacks (Fuchsia/Genode monotonic downgrade; kills confused-deputy). New `CapSet`/`Spawner` in `kernel/src/task/cap.rs` (intersect unit-tested). **init = root authority `CapSet::ALL`** via direct main.rs TCB write (NOT manifest — init spawns via `spawn_from_mem`, manifest never read); HotSwap passes the replaced cell's caps as ceiling (not the Root exemption). Red-teamed + validated (plan `.agents/260621-0830-cell-perms-p2-p5/`). riscv64 boots to `Cellos >`, "init granted root authority" logged, vfs/net/shell receive full caps, no faults/denials. (Phases P5 — Ed25519 + operator policy — deferred pending the Phase 02 crypto spike.)
- 📋 **Runtime revocation** `[G1/G2]` — `CapHandle` kernel object; `sys_cap_revoke(handle)` clears `task.cap`; next syscall → `ViError::CapRevoked`; Cell gets `AppEvent::CapRevoked`. Simpler than seL4 CDT (no cap-to-cap derivation yet).
- 🟡 **Operator-policy consent (G1)** `[G1]` — Operator signs a policy blob (Ed25519) at fleet provision; kernel verifies vs fleet root key + spawns with `manifest ∩ spawner ∩ policy`. SROS2 semantics at the kernel level; no dialog.
  - `/bin/vfs` keeps the cell-store block-region bit end-to-end: request ∩ ceiling ∩ signed policy preserve `block_regions=0b1111`, and the loader fails closed instead of backfilling a raw grant if the bit is missing.
  - ✅ **Crypto (P5a, 2026-06-21)** — in-kernel `ed25519::verify` (`ed25519-compact`, no_std, PIC-clean both arches); RFC 8032 + tamper self-test at boot.
  - ✅ **Load/verify/parse (P5b)** — `kernel/src/policy.rs`: `VPOL` blob, verify-then-parse (panic-free, domain-validated), `lookup`; host signer `scripts/sign-policy.py`; absent + signed/invalid paths verified at boot.
  - ✅ **Intersection + recovery (P5c/Phase 04)** — `policy::apply` folds `∩ policy` into the spawn grant; trusted-core (`vfs`/`shell`/`net`) recovery hatch + `maintenance-mode`; `init` exempt; fail-safe (dev-permissive G1 / `policy-required` fleet). Narrowing self-test green both arches.
  - ✅ **Deployment (2026-06-21)** — `tools/fat16_insert.py` bakes a dev-signed `/POLICY.BIN` into the VIFS1 images; kernel loads + verifies from disk at boot (`PolicyLoaded`, 4 entries) on both arches; integration boot tests green. Reproducible deploy step (signer is deterministic; images are gitignored generated artifacts so the blob is not committed). Without the bake → `PolicyAbsent` → dev-permissive (safe). `dev-policy-key` in `default` (G1 dev posture — prod provisions the real key).
  - 📋 **Revoke** — "push new policy + reboot" (+ snapshot-invalidate on policy change); runtime hot-revoke deferred (separate §G.2 item).
- 📋 **Consent-broker Cell (G2 HMI)** `[G2]` — Trusted Cell renders TCC-style dialog for *sensitive caps only* (camera/mic/storage), purpose-string required, signed consent-db; anti-fatigue (first-use only, one-time option, auto-revoke after N days). After ViUI HMI stable.
- ✅ **Per-Cell measurement (2026-06-21)** `[G1]` — `spawn_from_path()` now hashes the ELF (`SHA256`) before the cell is scheduled and records it in an append-only measurement log + rolling aggregate (`agg = SHA256(agg‖hash)`, the value a future DICE/EAT token signs). Linux IMA model. New files: `kernel/src/sha256.rs` (self-contained, NIST-vector-verified), `kernel/src/measurement_log.rs`; audit event `CellMeasure = 15`. Evidence only (orthogonal to Cell-signing enforcement). Compiles clean riscv64 + aarch64.
- 📋 **DICE/RIoT attestation chain** `[G1/G2]` — TPM-free layered attestation (`CDI_n = HKDF(CDI_{n-1}, HASH(layer_n))`), AliasKey signs an EAT (RFC 9711) per RATS (RFC 9334). No Rust no_std DICE crate yet → build from `hkdf`+`ed25519-dalek`+`coset`. Fleet verifier = ARM **Veraison** (open-source). Sealed storage: AEAD key from `CDI_final` held in **Silo** (closes the CDI-in-RAM hole).
- 📋 **Hardware RoT — OpenTitan backing for Silo** `[G2/G3]` — `ostd::silo::SiloHandle` API stays; backend evolves from Stage-2 mailbox → **OpenTitan** (Earl Grey discrete over SPI, or Darjeeling IP in a custom SoC). OpenTitan (Apache 2.0, RISC-V Ibex, production silicon) is the open-source hardware realization of what Silo approximates in software. Caliptra (DICE measurement) complements it for custom SoCs.

> **Sequencing:** P1 parameterized caps → P2 delegation → P3 per-Cell measurement → P4 DICE+sealed storage → P5 operator policy → P6 consent-broker (G2) → P7 remote attestation. Hardware secure-boot (eFuse) is G2 (untestable on QEMU — do not block G1). **Needs a dedicated `/hc-plan`** (touches kernel + ABI + multi-phase).

### H. Enterprise App Isolation `[G3 — on-demand]`

> Added 2026-06-21. Chỉ triển khai khi có khách hàng doanh nghiệp/chính phủ cam kết với contract. Đây là compliance bridge, không phải product feature. Cả hai track đều gated trên `virtio-gpu` (G2) và G2 graduation.

**Nguyên lý cốt lõi:** App nguy hiểm/không tin tưởng chạy trong VM Cell. Nếu app crash hoặc bị exploit → chỉ VM Cell đó chết, Cellos kernel và các Cell khác hoàn toàn không bị ảnh hưởng. Hardware EPT/Stage-2 MMU bảo vệ — đây là hardware isolation thực sự, không phải LBI.

```
[Cellos kernel]
  └── [VM Cell — hardware EPT boundary]
        └── [Linux guest + Wine/Proton]   (Track H1)
              └── [Windows app]
        └── [Windows guest]               (Track H2)
              └── [Windows app + USB token passthrough]
```

#### H1. Wine/Proton in Linux VM Cell
- **Status:** 📋 G3 on-demand
- **Isolation:** hardware EPT/Stage-2 — identical to existing Tier 3b Linux VM guarantee
- **App compatibility:** ~70% Windows apps (Wine regression list applies)
- **Hard blockers:** USB token (chữ ký số) PKCS#11 fatal; HTKK .NET crypto không chạy được qua Wine
- **Use case:** Sandbox Windows apps thông thường không cần token signing

#### H2. Bare Windows VM Cell
- **Status:** 📋 G3 on-demand
- **Isolation:** hardware EPT/VT-x hoặc EL2 Stage-2 — cùng level với H1
- **App compatibility:** ~100% (native Windows guest, không qua Wine)
- **USB token:** ✅ passthrough qua IOMMU (đã complete Track B 2026-06-16)
- **Use case:** HTKK + chữ ký số USB + toàn bộ enterprise/compliance app Windows
- **VMM additions:** ~14-16K LOC (ACPI table gen, UEFI/OVMF pflash, VirtIO-PCI transport, Hyper-V enlightenments)
- **Feasibility ref:** Cloud Hypervisor (Intel, ~106K LOC Rust) đã boot Windows 10/11 thành công
- **License:** VDA E3 ≈ $10/user/tháng (hypervisor-neutral) hoặc Windows Server Datacenter

**Điều kiện để build (ALL required):**
1. `virtio-gpu` shipped (G2) — không có display thì không có GUI app
2. Khách hàng ký contract và cam kết thanh toán trước
3. G2 graduation criteria met
4. Thỏa thuận rõ về licensing model (VDA vs Server DC)

**Không phải:**
- ❌ Giải pháp né bản quyền Windows — license vẫn cần
- ✅ Hardware-isolated sandbox: app bị compromised → chỉ VM Cell chết

---

### I. Chipset & Driver Support Matrix

> Decided 2026-06-06. Full analysis: `.agents/reports/brainstorm-260606-2205-chipset-driver-strategy.md`

#### Hardware targets per stage

| Stage | CPU arch | Dev/test platform | Real board (when ready) |
|-------|----------|-------------------|------------------------|
| G1 | ARM64 + RV64 | **QEMU ARM virt** (primary, QEMU-first policy) | RPi 4 (BCM2711) → VisionFive2 (JH7110) |
| G1 sub-track | RV32 | QEMU RV32 virt | SiFive E21 / CHERIoT-Nano |
| G2 graduation demo | ARM64 | **Radxa ROCK 5B+ 16GB (~$149, RK3588)** | — (this IS the graduation board) |
| G2 parallel | RV64 | **Milk-V Pioneer (SG2042, now)** | SG2044 SRA3-40 (IF H-ext ships, 2026+) |
| G2 | x86_64 | QEMU x86_64 virt | x86 PC (when G2 starts) |
| G3 | ARM64 | Same as G2 demo board (RK3588) | — |
| G3 | RV64 | — | C930 SoC (2027+, IF H-ext confirmed) |

#### Extended Hardware Testing (Post-Primary Boards)

After validation on the primary boards, Cellos will expand testing to the following hardware to ensure maximum portability and community adoption:

| Stage | CPU arch | Target Board | Purpose |
|-------|----------|--------------|---------|
| G1 sub-track | RV64/RV32 | **Milk-V Duo / LicheeRV (Cvitek CV1800B)** | Ultra-low cost embedded testing, dual-core asymmetrical RV64/RV32. |
| G1 | ARM64 | **Raspberry Pi 4 / 5** | Widespread community adoption, rich I/O driver validation. |
| G1 | ARM64 | **Pine64 / Quartz64** | Open-source friendly, alternative ARM64 driver validation. |
| G1 sub-track | RV32 | **ESP32-C3 / ESP32-C6** | Deeply-embedded IoT integration, RTOS determinism on Wi-Fi/MCU boards. |

**QEMU-first policy (G1):** Develop and validate peripheral Driver Cells on QEMU ARM virt (PL061 GPIO, PL011 UART, VirtIO) before buying real SBCs. HAL traits (`ViGpio`, `ViUart`) must be **board-agnostic** from v1 so real-board support adds only a new impl, zero kernel changes.

#### G1 peripheral driver priority

```
GPIO (PL061 QEMU → BCM/JH7110 real)
UART configure baud (extend existing cell)
I2C → IMU / ToF / temperature sensors
SPI → fast ADC / display / high-speed IMU
PWM → servo / ESC motor control
ADC → analog sensors / battery monitoring
CAN → industrial robot bus (ROS2 CAN bridge)  [low priority, defer]
```

#### G2 driver priority (strict order — each is prerequisite for the next)

```
1. PCIe ECAM host controller   ✅ DONE 2026-06-13 (Track A)
2. RISC-V IOMMU                ✅ DONE 2026-06-16 (Track B — bare passthrough)
3. NVMe (~3-5K LOC)            ✅ DONE 2026-06-13 (Track A — polled PRP I/O)
4. RTL8125 / Intel i225 2.5G   ✅ DONE 2026-06-16 (Track B — e1000/QEMU; RTL8125/i225 ID table)
5. Intel i40e 10G              ← only when inference server needs bandwidth
```

> ⚠️ RISC-V IOMMU (ratified 2023) is **non-optional** before NIC: in SAS, an unguarded NIC DMA can write to kernel memory. Implement before step 4.

**G2 PCIe strategy:** Port Redox OS PCIe ECAM enumeration logic (~40-60% reuse for BAR parsing / capability walk); rewrite MMIO access layer to use Cellos's `MmioRegion` safe-MMIO + Resource Registry. Do NOT port Redox's `mmap`-based driver model.

> **2026-08-20 q35 lane close:** q35 PCIe storage/network is now closed in QEMU evidence only: NIC 2/2, NVMe 3/3 with VFS FAT32 roundtrip, `X86_NIC_MODEL=e1000e` fail-closed, and VT-d active. Physical x86 stays hardware-gated/deferred; Pioneer stays blocked; RTL8125/i225 stay research-only; the BAR no_std unit-harness gap is deferred low risk.

#### G3 NPU path

```
G2 Level A  →  RKNN Runtime FFI cell (Tier 1b)    — validate ViAccelerator API on real HW
              + Tier 1b net/entropy shims (see §E)
G3 Level B  →  ViAccelerator HAL trait              — informed by ≥2 months RKNN experience
               Kernel NPU scheduler + AcceleratorCap ZST
G3 Level B+ →  SiFive X390 VCIX driver cell         — 2nd impl validates trait generality
G3 Level C  →  sys_grant_tensor + TensorBuffer       — needs sys_grant_pages (G2 prerequisite)
               ModelHandle shared weight (4GB cross-cell)
```

**RK3588 first:** buy Radxa ROCK 5 / Orange Pi 5+ (~$150) during G2 development. Hands-on with RKNN API ≥2 months BEFORE designing `ViAccelerator` trait.

#### Scope killers — NOT planned

| Excluded | Reason |
|----------|--------|
| Mellanox mlx5 (ConnectX) | 100K+ LOC, not needed for G2 demo; i225/RTL8125 sufficient |
| Bluetooth / WiFi | Stack complexity out of proportion with use case |
| USB host (xHCI) before G2 | Not blocking G1/G2 graduation |
| Full ACPI power management | Only ACPI MADT for SMP CPU topology needed |
| Audio / sound | Not a G1/G2 use case |
| Multiple boards simultaneously G1 | 1 QEMU + 1 real SBC at graduation; HAL abstraction handles more later |

---

### J. G2 Application Platform Layers `[G2 — post-G1 foundation]`

> **Context (2026-06-14):** Setelah G1 graduation, Cellos sẽ có kernel rất solid nhưng application platform gần như trống. Chỉ kernel team mới viết được app hiệu quả. G2 không chỉ là thêm tính năng kernel — mà là xây dựng toàn bộ platform layer, giống hành trình Linux từ 1991 (kernel) đến 2000 (LAMP stack).
>
> **Rule:** Không có L1 → không ai viết được app. Không có L2 → chỉ toy apps. Không có L3 → không distribute/maintain được. Không có L4 → không operate production được. **Không skip layer.**

| Layer | Cần xây | Tương đương Linux | Phụ thuộc | Status |
|-------|---------|-------------------|-----------|--------|
| **L0 — Mental model** | Docs dạy Cell/Actor thinking; migration patterns từ Linux (`thread→cell`, `blocking→async/IPC`) | Unix philosophy, man pages | — | 📋 |
| **L1 — App Framework** | `CellRuntime` (builder), `app_entry!`/`service_entry!` macros, typed clients (VfsClient/NetClient/InputClient), lifecycle hooks | glibc + POSIX | Name service (205/206 done), embedded-io traits (✅ both done) | ✅ COMPLETE (2026-06-16) |
| **L2 — Middleware** | HTTP server native Cellos (zero-copy từ đầu), auth/JWT, pub-sub, DB access (SQLite via Tier 1b) | Express, Django, Spring | L1 |📋 |
| **L3 — Tooling** | Package manager, cell image format, cell-aware debugger, `cargo-Cellos` | apt/cargo, gdb, strace | L1 | 📋 |
| **L4 — Observability** | Cell metrics, distributed tracing cross-cells, kernel audit ring integration, Prometheus-compatible export | Prometheus, OpenTelemetry | L1 + L3 | 📋 |

**Lợi thế thiết kế Cellos có thể tận dụng (không có ở Linux):**
- HTTP server zero-copy ngay từ đầu — Grant API đã có; không phải patch sau như nginx
- Service discovery type-safe qua cap system — không cần consul/etcd bolt-on
- Observability baked-in — audit ring buffer đã có trong kernel; không retrofit như eBPF
- Security by default — capability manifests; không phải patch lên Unix DAC sau 30 năm

**Dependency chain cho G2 native app development:**
```
✅ embedded-io traits → ✅ HashMap in prelude → App SDK (L1) → Middleware libs (L2) → real G2 apps
```

---

### Minimal unlock sets (by use-case)
| To write… | Needs (leverage order) |
>>>>>>> 47fc639b (fix(x86): close phase05 q35 driver gates)
|---|---|
| What is active now | [roadmap/current-focus.md](roadmap/current-focus.md) |
| Hardware qualification lanes | [roadmap/hardware-tracks.md](roadmap/hardware-tracks.md) |
| Product-stage overlay G1-G5 | [roadmap/product-stages.md](roadmap/product-stages.md) |
| Runtime and platform overlays | [roadmap/runtime-and-platform-tracks.md](roadmap/runtime-and-platform-tracks.md) |
| Technical milestones and historical status | [roadmap/technical-milestones.md](roadmap/technical-milestones.md) |
| Completed history ledger | [roadmap/completed-history.md](roadmap/completed-history.md) |
| Known open risks and deferred gates | [roadmap/open-risk-register.md](roadmap/open-risk-register.md) |
| Immutable pre-split snapshot | [project-roadmap-legacy.md](project-roadmap-legacy.md) |

## Current Direction

Cellos is being shaped around product stages, not only phase numbers:

- G1 Robot & Embedded: RV64/ARM64 SBC-class robot/embedded system with bounded
  memory, hardware I/O, fast boot, and never-die supervision.
- G2 Server & Specialized PC: x86_64/server qualification, SMP throughput,
  large storage, zero-downtime service upgrades, desktop/tooling depth.
- G3 NPU-native Compute OS: parked until real NPU hardware and vendor API
  experience inform the contract.
- G4 Full Rust std for Tier 1 Cells: planned as a `rust-std` runtime profile
  using pure-Rust PAL/rustc target work, not `std` over mlibc.
- G5 Virtualization Platform: research/design overlay after G4.

## Current Codebase Facts

- Cargo workspace members: 111, verified with `cargo metadata --no-deps`.
- HAL shape: `hal/core`, four `hal/soc/*` crates, fifteen `hal/traits/*`
  crates, and three `hal/arch/*` crates.
- HAL to kernel Rust ABI hook signatures are single-sourced in
  `hal/traits/arch/src/kernel_abi.rs`; `scripts/check-hal-boundaries.sh`
  rejects new local `extern "Rust"` declarations under `hal/arch`.
- Board descriptors live in root `boards/`; seven descriptors are active
  integration targets, while `q35-x86_32`, `virt-riscv32`, and `virt-aarch32`
  remain placeholder-only documentation entries.
- Active native scripting runtime: Lua. MicroPython is historical roadmap text
  and is not a current Cargo workspace member.
- Application execution uses Tier 1/2/3 terminology. `Tier 1b` and `Tier 3b`
  are legacy guide aliases for Tier 1 runtime profiles and Tier 3 Linux guests;
  SDK packaging uses named modules, not numbered tiers.

## Immediate Open Gates

- Production signing is not fleet-enforced by default: `signing-required` is
  non-default and the non-dev public key path is still a `[0u8; 32]` placeholder.
- Physical hardware evidence must remain separate from QEMU/compile evidence.
  RPi3 smoke has been merged, but VF2/Pioneer/RPi4 physical lanes are still
  hardware-gated unless logs say otherwise.
- AArch64 test-hooks runtime evidence remains host-gated where the existing
  `qemu_exit::AArch64Semihosting` issue blocks the lane.
- Net-broker has implemented pieces for Noise/identity/routing, but `main.rs`
  still marks K1 loading, beacon sockets, relay dispatch, leases, and enrollment
  as TODO wiring.

## Update Rule

Keep this file short. Put maintained detail in the matching
`docs/roadmap/*.md` topic file. Do not edit the legacy snapshot; historical
delivery evidence belongs in `project-changelog.md`.
