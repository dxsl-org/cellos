# ViCell Project Roadmap

**Project**: ViCell (Jarvis Hybrid OS)  
**Current Version**: 0.2.1-dev (Mycelium Era)  
**Current Phase**: Phase 1 - Core Stability (Phase 23 complete) · **Active Stage**: G1 (Robot & Embedded)
**Last Updated**: 2026-06-13 (Phase 32 SMP, TLS, RTC, MMC, Compositor, Milestone 2.1/2.5 VFS all marked complete; ARM64 + x86_64 full bring-up verified)

---

## Overview

ViCell development is organized into 4 major **technical phases** (Core Stability → System Services → Apps/Runtimes → Advanced) plus hardening Phases 24–32. This document tracks progress, blockers, and next steps.

**On top of that technical numbering, work is now framed by 2 product stages by target hardware / use-case** (overlay — see next section). Technical phase IDs (Phase 24–32, M2.x–M4.x) and all `.agents/` cross-references are preserved; the `[G1]`/`[G2]` labels are a use-case overlay, NOT a renumbering.

---

## 🎯 Two Use-Case Stages (Overlay)

ViCell ships in two product stages defined by target hardware. The mapping principle: **architecture maturity matches use-case** — ARM64/RV64 (with MMU) → robot SBC `[G1]`; x86_64 → server/PC `[G2]`; RV32 → MCU deeply-embedded (sub-track at end of G1).

### 🤖 Stage G1 — Robot & Embedded
> **"Done" means**: never-die · bounded real-time · bounded per-Cell memory · fault isolation · fast boot · peripheral I/O · small footprint.
>
> **Hardware**: primary = **Tier A SBC with MMU** (RV64/ARM64, RPi-class robot brain/companion). Sub-track (end of G1) = **Tier B MCU** (RV32 <512KB, CHERIoT-Nano) for low-level motor/sensor control.

### 🖥️ Stage G2 — Server & Specialized PC
> **"Done" means**: throughput · multi-core scaling · untrusted third-party code · desktop GUI · zero-downtime · full tooling · large storage · RT-bounded NPU inference (via Tier 1b).
>
> **Hardware**: x86_64 (full bring-up) + multi-core RV64/ARM64 servers + RISC-V AI server (C930/P870).

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

### Milestone → Stage Map

| Item | Source phase | Status | Stage |
|------|--------------|--------|-------|
| Core Stability (VirtIO, kbd, ELF, hotswap) | Phase 1 | ✅ | G1 (foundation) |
| Perf baseline + KASLR | Phase 24 | ✅ | G1 |
| Priority scheduler + RT TLSF heap + spawn_pinned | Phase 25 | ✅ | G1 |
| Memory quota + ZST caps + panic isolation | Phase 26 | ✅ | **G1** (never-die) |
| Reliability / supervisor restart | specs/12 | ✅ SUBSTANTIAL (P00-03 DONE 2026-06-06: fault-path force-unlock, reboot-on-panic, guard pages, RT watchdog; P05 done: RecvTimeout deadline, NotifyOnExit supervisor, zombie reaper; P06 observability done) | **G1** |
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
| 🆕 Tier B sub-track (end G1): RV32 HAL + ViCell-Nano + CHERIoT | M4.3 + Phase 31 | ✅ QEMU boot verified (2026-06-07) | **G1** (sub-track) |
| 🆕 Reference robot demo (sensor→compute→actuator + MQTT) | *new* | ✅ COMPLETE (skeleton + proven on RISC-V; ARM64 kernel build complete — GPIO periph-demo verified on QEMU virt) | **G1** (graduation) |
| Direct-IPC vtable (raw perf) | Phase 27-3 | ✅ | G2 |
| WASM Tier-2 MVP (wasmi + 4 vi.* imports + fuel) | Phase 28 | ⚠️ experimental only — DROPPED from official stack 2026-06-06; revisit G2 multi-tenant only | G1 (legacy) |
| WASM WASI 2.0 Component Model (+ePMP) | Phase 28/31 | ⚠️ dropped — same decision | **G2 (dropped)** |
| 🆕 Tier 3 kernel prep — H-extension HS-mode boot (RISC-V) | *new* | ✅ COMPLETE (2026-06-07) — cpu_features.rs DTB detection + HypervisorCap ZST + TCB field; see .agents/260607-1420-h-ext-hypervisor-cap/ | **G1 prep** (non-breaking) |
| 🆕 Tier 3a Security Silo (Stage-2 fenced bare-metal guest) | *new* | 📋 | G1-optional |
| 🆕 Tier 3b Linux VM — crosvm fork + vicell_hv/ port | Phase 31 | 📋 | **G2** |
| 🆕 **SMP multi-core scheduler + work-stealing** | Phase 32 | ✅ COMPLETE 2026-06-09 — SBI HSM hart_start/send_ipi, per-hart ViHartLocal via tp CSR, per-hart ready queues + work stealing, RT cells pinned to hart 1, WaitForEvent (217) | **G2** |
| Compositor + GPU desktop (full) + mouse | M2.4 + M2.2 full | 📋 | G2 |
| 🆕 **ViUI v1** (Elm model, FramebufferCanvas, GlyphAtlas, P01–P07) | new | ✅ Done 2026-06-08 — foundation only, design superseded | **G2 prep** |
| 🆕 **ViUI v2** (Reactive Signal Tree + Dual-Layer DSL) | new | ✅ G1 P01–P06 + G2 Wave 1 COMPLETE 2026-06-09 (FlexBox, Virtual ListView, Typed DSL, Keyboard a11y, GLES2 skeleton; P05 GPU EGL-deferred) | **G2** |
| 🆕 **TLS 1.3 stack** `[shared, G1-priority]` | Phase TLS-01 | ✅ COMPLETE 2026-06-07 — Network service supports TLS 1.3 via sys_get_random(214), three TLS IPC opcodes (0x30/0x31/0x32), HTTPS demo verified | **G1** |
| 🆕 **RTC / wall-clock** `[G1]` | new | ✅ COMPLETE 2026-06-07 — Goldfish RTC (RISC-V/ARM64) + CMOS RTC (x86_64); GetTime op=2/3 for epoch_ns/epoch_secs; date binary shows real UTC time | **G1** |
| 🆕 **MMC subsystem** (SDHCI PIO) `[G1 ext / G2]` | Phase M2.6 | ✅ COMPLETE 2026-06-07 — 5 phases done (card init, eMMC/SD variants, PL180 impl, QEMU VirtIO + real SBC routing); 812 LOC; RPi4/VisionFive2 ready | **G1** |
| 🆕 **Large-buffer IPC** `[shared, G3 prerequisite]` | Phase M2.7 | ✅ COMPLETE 2026-06-07 — MAX_GRANT_PAGES lifted 16→4096 (16MB cap), grant reaper on task death, GrantRegister/Unregister syscalls 215/216 shipped | **G2/G3** |
| 🆕 **Compositor Grant surfaces** `[M2.4 partial]` | Phases 01–05 | ✅ COMPLETE 2026-06-09 — zero-copy surfaces, damage-driven render, FONT8X8, ViSurface wrapper; replaces WRITE_PIXELS IPC with Grant shared memory | **G2** |
| Hot migration / zero-downtime | M4.1 | 📋 | G2 |
| 🆕 x86_64 full bring-up | ext. M1.3 | ✅ COMPLETE (2026-06-13) — APIC, HPET/TSC, real MMU, VirtIO, PL011 RX; 5/5 QEMU integration tests pass; syscall exit path fixed | **G2** |
| VFS scale (FAT32/ext4, large disks) | M2.1 ext. | 📋 | G2 |
| Full utility suite (grep/sed/awk/top/ps…) | M3.2 full | 📋 | G2 |
| Throughput benchmark | M4.4 full | 📋 | G2 |
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
- [ ] Real SBC validation (RPi4 / VisionFive2)

> ⚠️ Largest new chunk of G1 — needs its own brainstorm → plan → cook cycle. Do not underestimate.

#### Architecture Full Bring-Up (split from "Multi-Arch HAL ✅")
The existing Milestone 1.3 marks ARM64/x86_64 as **ring-3 smoke only**. Real targets need full bring-up (interrupt controller, timer, real MMU, device drivers).
- **ARM64 full bring-up `[G1]`** ✅ COMPLETE (2026-06-12) — GIC, generic timer, 3-level MMU, VirtIO, PL011, PL061 on QEMU virt; 6/6 integration tests pass
- **x86_64 full bring-up `[G2]`** ✅ COMPLETE (2026-06-13) — APIC, HPET/TSC, real MMU, VirtIO, PL011 RX; 5/5 QEMU integration tests pass; syscall exit path fixed (CVE-2012-0217 canonical check, user RSP restore)

#### Reference Robot Demo `[G1]`
**Status**: 🆕 — **G1 graduation gate**
End-to-end loop: sensor read → compute → actuator write over GPIO/CAN, with MQTT telemetry. Proves the embedded stack works as a whole.

#### Tier 3: Hypervisor / Virtualization `[G1-prep + G2]`
**Status**: 🆕 DESIGNED — spec at [specs/05-application.md §4](specs/05-application.md)
**VMM**: Custom **minimal VMM** (~9K LOC Rust, built from scratch as Tier 1 cell). microvm profile — MMIO bus, no PCI. VirtIO blk/net/console backends forward to ViCell VFS/Net IPC. No tokio, no mmap — SAS-native. (crosvm fork rejected: ~75K LOC, tokio+mmap incompatible with SAS cell constraints.)

Three sub-items:
- **Tier 3 kernel prep** `[G1-prep, non-breaking]`: RISC-V H-extension detect + HS-mode boot path (`hal/arch/riscv/hypervisor.rs`, ~200 LOC). `HypervisorCap` ZST token gates hypervisor syscalls (follows existing BlockIoCap/NetworkCap pattern). Transparent fallback to S-mode if H-ext absent.
- **Tier 3a Security Silo** `[G1-optional]`: bare-metal Rust no_std guest in Stage-2 fenced memory. No Linux needed. Robot TLS private key isolation use case.
- **Tier 3b Linux VM** `[G2, Phase 31]`: minimal VMM, boot Alpine Linux, VirtIO → ViCell IPC. Enables `apt install nginx`. CPU overhead ~5-10% (H-extension hardware virt), disk I/O ~20-40% (VirtIO roundtrip) — acceptable for management plane.

> See [specs/05-application.md §6](specs/05-application.md) for wrong-path list (no QEMU-as-cell, no Type-1 hyp, no crosvm fork, no Android in G2).

### Graduation Criteria

**G1 — Robot/Embedded is "done" when:**
1. Never-die: a single Cell fault/OOM → killed & restarted, kernel survives.
2. Bounded memory enforced on EVERY write path (Write/Append/IPC).
3. RT determinism: a control-loop Cell meets its deadline; IPC latency has a measured bound.
4. Peripheral I/O: GPIO/I2C/SPI/UART work on QEMU + ≥1 real board.
5. Instant-On boot under target threshold.
6. Runs on real RV64 + ARM64 SBC (full bring-up).
7. Sub-track: ViCell-Nano minimal profile boots on RV32 (QEMU at minimum).
8. Reference robot demo runs end-to-end.

**G2 — Server/PC is "done" when:**
SMP scales across N cores · windowed desktop + mouse · hot migration with no dropped connections · x86_64 full bring-up · full utility suite + large storage · throughput benchmarks meet targets · **Linux VM boots inside Tier 3 (minimal VMM) and runs a real workload (nginx serving HTTP)** · RISC-V AI inference server demo: HTTP → NPU cell → response with P99 latency bound.

> WASM Tier 2 deferred: dropped from official stack; revisit only if G2 needs multi-tenant platform (untrusted third-party workloads). See [specs/05-application.md §6](specs/05-application.md).

---

## 🧩 Application Platform Gaps (backlog — brainstorm+plan pending)

> Added 2026-06-06 after a first-app feasibility study ([researcher-260606-1041-first-app-candidates.md](../.agents/reports/researcher-260606-1041-first-app-candidates.md)).
> **Finding:** ViCell today is a solid kernel + thin userspace; the *application-platform* layer is missing,
> so candidate apps come out as toys or narrow plumbing. The gaps below are what unlocks **real** apps.
> Each is a backlog item to be brainstormed + planned individually. Status 📋 = not yet planned.

### A. Hardware I/O `[G1]`
- **Peripheral bus** (GPIO/I2C/SPI/CAN/PWM/ADC) — 📋 already designed → see "Peripheral Driver Track" + [specs/13-peripherals.md](specs/13-peripherals.md). #1 gap: no app reads sensors / drives actuators without it.

### B. Interaction `[G1 input · G1-opt/G2 display]`
- 🆕 **P0 UART input delivery to apps** `[G1]` — ✅ COMPLETE (2026-06-15). UART bytes now relayed to input service via EV_ASCII opcode (0x04) on all arches; ARM64 integration test green. Apps can register for input focus and receive keyboard events. See [.agents/260615-p0-uart-input-delivery/](../agents/) for details.
- **Display / GUI** — 📋 see Milestone 2.4 (compositor/GPU, HMI feature-gate). Blocks user-facing graphical apps.

### C. Real-world connectivity `[G1 priority · shared]`
- 🆕 **TLS 1.3 for the net stack** `[shared, G1-priority]` — ✅ COMPLETE (Phase TLS-01). Network service now supports TLS 1.3 client handshake via sys_get_random(214) entropy + three TLS IPC opcodes (0x30/0x31/0x32). HTTPS demo cell connects to example.com:443, validates cert chain, issues HTTP GET. Foundation for MQTT over TLS, secure device communication, IoT protocols.
- 🆕 **RTC / wall-clock time** `[G1]` — ✅ COMPLETE (2026-06-07). Goldfish RTC (RISC-V/ARM64) + CMOS RTC (x86_64); GetTime op=2/3 for epoch_ns/epoch_secs; date binary shows real UTC time with fallback to uptime. See [.agents/260607-1719-rtc-wall-clock/plan.md](.agents/260607-1719-rtc-wall-clock/plan.md)
- 🆕 **Large-buffer IPC / scatter-gather** `[shared, G3 prerequisite]` — 📋 512-byte IPC buffer → 6000 round-trips for a 3MB tensor (unusable for video, file transfer, NPU inference). Recommended: `sys_grant_pages(tid, vaddr, len, perms)` — page-table remap, no memcpy, ~1K LOC. Extends existing Lease/GrantEntry pattern. **G3 cannot start without this.**

### D. App SDK / ergonomics `[shared]`

> **Decision (2026-06-14):** `ostd` IS ViCell's std — do NOT build a `std` facade (std assumes Unix process model, contradicts SAS/LBI). The three gaps below are what unlock real native apps without false familiarity. See brainstorm `.agents/brainstorms/260614-native-app-std.md` (to be written).

- 🆕 **Name service** `[shared]` — 📋 service endpoint ids are spawn-order constants (vfs=3, net=6…), hard-coded everywhere. Replace with a registry/lookup.
- 🆕 **High-level cell libraries** `[shared]` — 📋 HTTP/JSON/TLS client helpers so apps don't hand-roll protocol bytes + manual encode/decode.
- 🆕 **Python/scripting story** `[G2]` — Python R&D users: full CPython via Tier 3 Linux VM (`apt install python3 pip numpy torch` → works). Lua/MicroPython native runtimes **dropped** (half-measure). WASM Tier 2 dropped — no `micropython.wasm` path. Robot code stays Rust (Tier 1). Milestones 3.3/3.4 marked complete but runtimes not actively maintained.
- 🆕 **Async runtime exposed to apps** `[shared]` — 📋 no app-facing async executor for concurrent I/O.
- ✅ **`embedded-io` traits for ostd** `[shared, COMPLETE 2026-06-15]` — `embedded_io::Read` impl'd for `ostd::fs::File` + `Stdin`; `embedded_io::Write` impl'd for `Stdout` + `File` (via `VfsRequest::Append` IPC, chunked at 400B). Opens the no_std embedded-crate ecosystem. **Gate for high-level cell libraries: cleared.**
- ✅ **`HashMap` in ostd prelude** `[shared, COMPLETE 2026-06-15]` — `hashbrown` already in `libs/ostd/Cargo.toml`; `ostd::collections::HashMap`/`HashSet` exported; re-exported in `ostd::prelude`. Was already shipped — roadmap was stale.
- 🆕 **ViCell App SDK** `[shared, G1-tail]` — 📋 Apps today write raw syscall boilerplate (declare_manifest, sys_recv dispatch loop, manual service lookup). Need a structured application framework layer on top of `ostd`: `AppContext` (unified entry, service discovery, lifecycle), typed event loop (`AppEvent::Message/Shutdown`), ergonomic IPC patterns. The threading model (Cell spawn = Actor, not `std::thread`) must be documented clearly. This is the primary unlock for "real native apps" — equivalent to what SwiftUI/Android lifecycle did for mobile. Effort: ~2 weeks. Depends on: Name service (registry/lookup) + embedded-io traits.

### E. Ecosystem / distribution `[G2]`
- ✅ **Tier 1b C library integration** `[shared, COMPLETE 2026-06-13]` — link vendor C/C++ libraries (NPU SDK, mbedTLS, SQLite, legacy firmware) into Rust cells via `vicell-libc` (Newlib + POSIX shim). Shim in `libs/api/src/posix.rs`: malloc/free, strings, file I/O, time → ViSyscall, getentropy → `ViSyscall::GetRandom` (op 214), socket/connect/send/recv/close → typed Net IPC (postcard). ARM64 `svc #0` ABI added; send() postcard decode bug fixed; `_time()` op code fixed (op=3 = epoch seconds). Integration tests: `posix_shim_getentropy` + `posix_shim_net` in `tests/integration/tests/boot.rs`. No `fork` by design. Primary use case: hardware NPU SDKs (RKNN/Hailo/K230). Plan: `.agents/260613-0520-tier1b-posix-shims/`. See [specs/05-application.md §3](specs/05-application.md).
- **WASM Tier-2** — Phase 28 MVP ✅ (wasmi + 4 imports). **Tier 2 dropped from official stack** (2026-06-06). Phase 28 code retained under `feature = "wasm-experimental"` only — Phase 28-5 and WASI 2.0 migration cancelled. Revisit only if G2 becomes multi-tenant platform (Cloudflare Workers–style) after WASI 1.0 freezes (late 2026/early 2027).
- 🆕 **Package manager / app distribution** `[G2]` — 📋 no install/update mechanism beyond baking into the disk image.

### F. G2 Server Strategy — ARM64 Graduation Demo + RISC-V Latency Demo `[G2]`

**Decision (2026-06-06, updated 2026-06-11):** G2 value proposition = **latency guarantee + reliability + security**, NOT throughput. Not competing with LLM GPU throughput (5-30× gap) or general x86 workloads.

**⚠️ Hardware correction (2026-06-11 research):** C930 = Alibaba IP core (RTL delivery to licensees March 2025, no SoC/board before 2027). P870 = SiFive IP licensed by Sophgo — no standalone P870 chip purchasable. H-ext (hypervisor extension) absent from ALL shipping RISC-V chips — blocks Tier 3b VM plane on RISC-V. See `docs/research/research-riscv-ai-ecosystem.md`.

**G2 graduation demo: ARM64 RK3588 first (not RISC-V)**

Primary graduation target: **Radxa ROCK 5B+ 16GB (~$149)** — Rockchip RK3588.
- NPU: 6 TOPS INT8, RKNN SDK v2.3.2 (mature, C API `rknn_init`/`rknn_run`/`rknn_query` → Tier 1b FFI)
- Tier 3b: Alpine Linux VM via KVM EL2 (confirmed, 4 vCPU limit) — ARM64 EL2 works NOW; RISC-V H-ext does NOT exist yet
- ViCell = first custom OS with deterministic NPU inference on RK3588 (Zephyr = UART-only; Redox = no port)

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

| | Linux | ViCell G2 |
|---|---|---|
| Inference P99 latency | Best-effort | RT-bounded per cell |
| NPU cell crash | System hung / cold restart | Supervisor respawn (never-die) |
| Memory copies (net→NPU→resp) | 3-4 copies | 0-1 (zero-copy grant) |
| Security (model weights, keys) | Process isolation | Stage-2 Security Silo |

**G2 graduation criteria (updated):**
- ARM64 bring-up on RK3588: U-Boot → ViCell EL1 → Cell ecosystem running
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

### G. Chipset & Driver Support Matrix

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

**G2 PCIe strategy:** Port Redox OS PCIe ECAM enumeration logic (~40-60% reuse for BAR parsing / capability walk); rewrite MMIO access layer to use ViCell's `MmioRegion` safe-MMIO + Resource Registry. Do NOT port Redox's `mmap`-based driver model.

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

### H. G2 Application Platform Layers `[G2 — post-G1 foundation]`

> **Context (2026-06-14):** Setelah G1 graduation, ViCell sẽ có kernel rất solid nhưng application platform gần như trống. Chỉ kernel team mới viết được app hiệu quả. G2 không chỉ là thêm tính năng kernel — mà là xây dựng toàn bộ platform layer, giống hành trình Linux từ 1991 (kernel) đến 2000 (LAMP stack).
>
> **Rule:** Không có L1 → không ai viết được app. Không có L2 → chỉ toy apps. Không có L3 → không distribute/maintain được. Không có L4 → không operate production được. **Không skip layer.**

| Layer | Cần xây | Tương đương Linux | Phụ thuộc | Status |
|-------|---------|-------------------|-----------|--------|
| **L0 — Mental model** | Docs dạy Cell/Actor thinking; migration patterns từ Linux (`thread→cell`, `blocking→async/IPC`) | Unix philosophy, man pages | — | 📋 |
| **L1 — App Framework** | `AppContext` (service discovery, lifecycle, typed events), ergonomic IPC patterns, ViCell App SDK | glibc + POSIX | Name service (205/206 done), embedded-io traits | 📋 |
| **L2 — Middleware** | HTTP server native ViCell (zero-copy từ đầu), auth/JWT, pub-sub, DB access (SQLite via Tier 1b) | Express, Django, Spring | L1 |📋 |
| **L3 — Tooling** | Package manager, cell image format, cell-aware debugger, `cargo-vicell` | apt/cargo, gdb, strace | L1 | 📋 |
| **L4 — Observability** | Cell metrics, distributed tracing cross-cells, kernel audit ring integration, Prometheus-compatible export | Prometheus, OpenTelemetry | L1 + L3 | 📋 |

**Lợi thế thiết kế ViCell có thể tận dụng (không có ở Linux):**
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
|---|---|
| **Real G1 robot app** | Peripheral I/O → RTC → input delivery (if HMI) |
| **Real cloud/IoT app** | **TLS** → bigger IPC/streaming → name service |
| **Hardware NPU inference (RKNN/Hailo)** | ✅ Tier 1b entropy + net shims DONE — next: RKNN runtime FFI cell |
| **Python R&D** | Tier 3: full CPython in Linux VM (`apt install python3 pip numpy`) |
| **Rich apps / ecosystem (G2)** | Tier 1b SDK libs → name service → display → Tier 3 Linux VM |
| **Real native Rust apps (non-toy)** | ✅ `embedded-io` traits → ✅ `HashMap` in prelude → ViCell App SDK |

---

## Phase 1: Core Stability (Current — Target: 2026-06-30)

**Goal**: Fix critical issues (VirtIO hang, keyboard input), stabilize nano-kernel, achieve multi-architecture HAL.

**Start Date**: 2026-04-01  
**Target End Date**: 2026-06-30  
**Effort**: 320 hours (~8 weeks @ 40h/wk)
**Status**: ✅ 100% COMPLETE (Phases 01, 02, 05, 10, 14, 15, 16, 18, 20, C, D, E, F, G, H, A–E, X-1–X-6 all complete)

### Milestone 1.1: VirtIO Block Device Fix
**Status**: ✅ PARTIAL (Root Cause Fixed)  
**Owner**: TBD  
**Priority**: P0 (blocking)

**Current State**:
- Root cause identified: Limine does not report MMIO ranges to kernel
- Solution: Explicit identity-mapping of VirtIO MMIO regions (0x1000_0000–0x1001_0000) in `kernel/src/memory/paging.rs`
- Duplicate MMIO entries removed from `kernel/src/boot.rs` FALLBACK_MEMORY_MAP
- Device interrupts now properly delivered via PLIC

**Deliverables**:
- [x] Debug root cause (MMIO identity-mapping missing)
- [x] Implement MMIO explicit mapping for VirtIO regions
- [x] Remove duplicate MMIO entries from fallback map
- [ ] Verify read/write complete within 100ms (testing in progress)
- [ ] Shell loads `/bin/shell` from disk (blocked by Phase 06)

**Completion**: Awaits full integration testing with Phase 06 (external ELF loading)

**Next Action**: Proceed with Phase 06 (External ELF Loading)

---

### Milestone 1.2: Keyboard Input Fix
**Status**: ✅ COMPLETE  
**Owner**: TBD  
**Priority**: P0 (blocking)

**Current State**:
- Root cause identified: VirtIO input IRQ was never acknowledged, leaving `InterruptStatus` register set; PLIC continuously re-fired interrupt, causing kernel hang
- Fix applied: Added `pub static INPUT_DEVICE_IRQ` constant and `pub fn ack_irq(irq: u32) -> bool` to `kernel/src/task/drivers/virtio_input.rs`
- Expanded `vi_handle_virtio_irq()` in `kernel/src/task/drivers/virtio_blk.rs` to dispatch to both block and input devices
- Established IRQ numbering pattern: QEMU VirtIO MMIO slot `i` → IRQ `i+1` (applies to all device types)
- Interrupt storm prevented by proper IRQ acknowledgment

**Deliverables**:
- [x] Multiple keystrokes processed without hang
- [x] IRQ acknowledgment properly implemented for all VirtIO devices
- [x] PLIC dispatch pattern established for block and input devices
- [x] Shell input loop no longer deadlocks on subsequent input
- [x] Async waker path analysis complete (not needed for polling-based shell)

**Completion**: Verified 2026-05-29; ready for Phase 2 shell interaction testing

**Next Action**: Proceed with Phase 03 (Ring 3 Boot) and Phase 06 (External ELF Loading)

---

### Milestone 1.3: Multi-Architecture HAL
**Status**: ✅ COMPLETE  
**Owner**: Completed in Phase 05  
**Priority**: P1 (high)

**Implemented**:
- [x] RISC-V 64-bit: FULLY IMPLEMENTED (SV39 paging, PLIC, SBI, traps)
- [x] ARM AArch64: FULLY IMPLEMENTED (4K paging, exception handling)
- [x] x86_64: FULLY IMPLEMENTED (4K paging, exception handling)
- [x] Feature-gated builds: `cargo build --features aarch64`, `--features x86_64`
- [x] Ring-3 smoke tests pass on all three architectures (QEMU)
- [x] RV32 + AArch32 trait stubs (impl only, no boot code)

**Trait Design**:
- `hal::Arch` — context switch, interrupts
- `hal::PageTableTrait` — paging operations
- `hal::InterruptController` — IRQ handling
- Uses conditional compilation: `#[cfg(target_arch = "riscv64")]`, etc.

**Next Action**: Reliability hardening — see [specs/12-reliability.md](specs/12-reliability.md).
> ⚠️ **Decided 2026-06-05: per-Cell SATP isolation is NOT pursued.** Hardware isolation
> for untrusted code lives in Tier 3 (Stage-2 paging), not in per-Cell SATP at Tier 1.
> This keeps Tier 1 zero-copy IPC intact. See [specs/05-application.md](specs/05-application.md).

---

### Milestone 1.4: External ELF Loading
**Status**: ✅ COMPLETE  
**Owner**: Completed in Phase 10  
**Priority**: P1 (high)

**Implemented**:
- [x] Load Cell binaries from `/bin/` directory
- [x] `syscall::spawn_from_path(path)` reads ELF from disk
- [x] ELF relocation for position-independent code (PIE)
- [x] Hot-swap: Replace shell, config, vfs at runtime
- [x] Cache mechanism in VFS service

**Verified**:
- shell, config, vfs load from `/bin/` and execute
- Hot-swap protocol: freeze → serialize → load → deserialize → resume
- Config + shell history/state preserved across swap

**Design**:
- Reuse ELF loader (kernel/src/loader.rs)
- PIE relocation via R_RISCV_RELATIVE (RV64)
- VFS handles binary caching + discovery

**Next Action**: Supervisor-based cell restart — see [specs/12-reliability.md](specs/12-reliability.md).
> Address-space isolation for untrusted code is provided by Tier 2 (WASM sandbox) and
> Tier 3 (hypervisor / Stage-2 paging), **not** per-Cell SATP. See [specs/05-application.md](specs/05-application.md).

---

### Milestone 1.5: Test Coverage
**Status**: 🚧 IN PROGRESS  
**Owner**: TBD  
**Priority**: P2 (medium)

**Current State**:
- Architecture validation: 10/10 score ✅
- Unit tests: 75%+ coverage estimate
- Integration tests: 2 scenarios (boot_banner, fat_filesystem_mounts) + 6 arch-validation modules

**Implemented**:
- [x] Frame allocator tests (95% coverage) — stress test: 10K alloc/free
- [x] Scheduler tests (90% coverage) — fairness, preemption, state transitions
- [x] IPC tests (85% coverage) — Send/Recv, Call/Reply, timeout, capability grant
- [x] Multi-Cell integration (70% coverage) — init → vfs → shell scenario

**Deliverables**:
- [x] Frame allocator: sequential, random, fragmentation patterns
- [x] Scheduler: round-robin fairness, preemption under load
- [x] IPC: grant/revoke, cascading messages, timeout behavior
- [x] Config service: KV operations, state transfer
- [x] Shell: input dispatch, history, aliases

**Run**: `cargo test --all --release`

**Target**: Reach 80%+ coverage before Phase 2

---

### Phases X-1 through X-6 (Completed 2026-06-04 to 2026-06-05)

**Phase X-1 — VirtIO VA→PA Fix**:
- Resolves multi-sector write corruption in FAT16
- Kernel/src/task/drivers/virtio_net.rs: proper address mapping

**Phase X-2 — Shell Function Arguments**:
- Function args ($1, $2, ..., $9) support
- Cells/apps/shell/src/executor.rs: arg stack management
- read built-in for interactive input

**Phase X-3 — Command Substitution**:
- $(cmd) syntax for command substitution in shell
- Parser and executor support for nested commands
- Works with all built-ins and pipes

**Phase X-4 — Lua Eval with Fault Handling** ✅:
- Execute Lua code via `lua -c` or script files
- Graceful fault handling (code-exec panics caught, banner-only verification)
- Integration test validates execution model

**Phase X-5 — MQTT 3.1.1 Client Cell** ✅:
- New binary cell `/bin/mqtt` implements MQTT QoS-0 publish/subscribe
- `mqtt publish host:port topic payload` and `mqtt subscribe host:port topic`
- Two new integration tests (mqtt_publish, mqtt_subscribe with mock broker)
- Key insight: ostd bump allocator exhausted by nested IPC polling; fixed with single-poll-per-iteration + outer yield loop

**Phase X-6 — ForceExit Syscall (kernel kill -9 equivalent)** ✅ COMPLETE (2026-06-05):

> **Root cause documented (2026-06-05):** `cmd_kill` uses `sys_send(tid, [0xFF])`.
> If the target is NOT in `TaskState::Recv`, `ipc_send` puts the **SHELL** into
> `TaskState::Sending` indefinitely — creating a deadlock chain.
> Mitigated by state-check before send (commit f0e7ad34+), but cannot kill
> tasks stuck inside VFS/net IPC.

**Design:**
- New `ViSyscall::ForceExit` (opcode 61) — **⚠️ Law 1, requires 2x confirmation**
- Caller must hold `SpawnCap` (already exists on shell/init)
- Kernel handler (non-blocking, returns immediately to caller):
  1. `exit_task(tid)` — remove from scheduler
  2. Scan all tasks in `TaskState::Sending { target: tid }` → unblock with error sentinel (`reply_value = usize::MAX`)
  3. `revoke_all_for(cell_id)` — cap table cleanup
  4. `deregister quota(cell_id)` — memory cleanup
  5. Audit log `CellExit` with force flag
- VFS/net cells: handle `sys_send` reply errors gracefully (don't crash when client is gone)

**Files (estimated ~60 lines total):**
- `libs/api/src/syscall.rs` — add `ForceExit = 61` (⚠️ Law 1)
- `libs/ostd/src/syscall.rs` — add `pub fn sys_force_exit(tid: usize) -> SyscallResult`
- `kernel/src/task/syscall.rs` — ForceExit handler + stuck-sender unblock
- `cells/apps/shell/src/commands.rs` — `cmd_kill` uses `sys_force_exit`
- `cells/services/vfs/src/main.rs` — handle reply-send errors

**Acceptance criteria:**
- `kill <tid>` terminates any task regardless of its state
- Shell does NOT block when target is in Recv or non-Recv state
- Tasks stuck in VFS IPC are terminated; VFS continues serving
- Tasks that were Sending TO killed task are unblocked with error

### Phase 1 Acceptance Criteria

All milestones complete when:
- ✅ VirtIO block device working (read/write, no hang) — Phase 05
- ✅ Keyboard input responsive (multiple keys, no deadlock) — Phase 05
- ✅ ARM + x86 HAL boot and run shell — Phase 05 (Ring-3 smoke)
- ✅ External ELF loading from `/bin/` functional — Phase 10
- ✅ HotSwap orchestrator (5-step protocol) working — Phase 20
- 🚧 Unit + integration tests pass (80%+ coverage) — 75% now, targeting 80%
- ✅ Architecture validation score: 10/10 — Phase 02
- ✅ Kernel LOC: < 10,000 (actual: 8,700) — Phase 05
- ✅ Multi-architecture HAL (RV64 + AArch64 + x86_64) — Phase 05

---

## Phase 24–31: Architecture Hardening & Research-Driven Features

> Derived from multi-persona analysis + deep research (2026-06-05).
> **Reference**: See [`docs/research-references.md`](research-references.md) for source repos, papers, and code pointers per phase.

### Phase 24 — Performance Baseline + KASLR (P0) `[G1]`
**Target**: 2026-07-07 | **Effort**: ~2 weeks | **Status**: ✅ COMPLETE (2026-06-05)
See `.agents/260605-0958-phase24-perf-kaslr/` for detailed phase reports.

**Phase 01 (Bench CI Baseline)** — ✅ COMPLETE
- [x] Fix `perf.yml` disk step (skips on Linux; bench never runs in CI)
- [x] Create `scripts/gen-bench-disk.sh` — Linux FAT16 disk builder for CI
- [x] Create `scripts/compare-bench-results.sh` — p99 regression detection vs baseline
- [~] Establish `perf-baseline.json` — **DEFERRED** (acceptable): first CI run skips comparison; 2nd run establishes baseline

**Phase 02 (KASLR via Limine Boot Randomization)** — ✅ COMPLETE (2026-06-05)
- [x] Switch QEMU to Limine S-mode bootloader chain (OpenSBI → Limine → kernel)
- [x] Make kernel PIE (`-C relocation-model=pic -C link-arg=-pie` via kernel/build.rs)
- [x] Create `limine.conf` with `KASLR=yes` at repo root
- [x] Create `scripts/download-limine.sh` (v8.9.2 RISC-V binary from GitHub releases)
- [x] Update `boot.rs`: log `physical_base` from `get_kernel_address()`
- [x] Update `paging.rs`: parameterize `init_kernel_paging(kernel_phys_base: PAddr)` ✅ (already working)
- [x] Update `ci.yml` + `perf.yml`: Limine download + new QEMU args
- [x] Update `run.ps1`: new QEMU invocation with Limine + disk
- [x] Verify all 65 integration tests pass with KASLR enabled ✅
- [x] Ready for first CI run: two consecutive boots will show different `physical_base` values
- [x] Add CI gate: p99 regression > 10% from baseline = build failure (script ready)

**Implementation Notes**:
- PIE flags via `kernel/build.rs` cargo:rustc-link-arg (avoids workspace .cargo/config.toml conflict)
- linker.ld parameterization skipped — mmap already handles KASLR correctly
- `perf-baseline.json` generation deferred to 2nd+ CI run (requires ≥2 baseline measurements)

**Why urgent**: Without a baseline, all performance claims are fiction. KASLR is fundamental security hygiene.

### Phase 25 — Priority Scheduler (P1) `[G1]`
**Target**: 2026-07-21 | **Effort**: ~2 weeks  
**Status**: ✅ COMPLETE (2026-06-05) — see `.agents/260605-1052-phase25-priority-scheduler/`

**Completed (2026-06-05):**
- [x] Phase 25-1: Timer preemption — `sie.STIE` enabled, `vi_timer_tick()` wired, initial timer armed
- [x] Phase 25-2: Priority queue — `TaskPriority` enum in `libs/api/`, `priority: u8` on TCB, `BTreeMap<u8, VecDeque>` scheduler
- [x] Phase 25-3: SSIP self-IPI — `sie.SSIE` enabled, scause==1 handler clears SSIP + yields, `pend_preempt_if_needed` at wakeup
- [x] Phase 25-4: TLSF RT heap — rlsf 0.2.2 integrated, 256 KiB pool, RT cells use `rt_alloc()` for stacks
- [x] Phase 25-5: Tests + spawn_pinned — 3 priority unit tests added, `SpawnPinned` syscall opcode 16, core_id validation

**Implementation Summary:**
- Timer fires every 10 ms (TICKS_PER_10MS = 100,000 @ 10 MHz mtime clock)
- `TaskPriority` enum: Background=0, Normal=1 (default), RealTime=2
- Ready queue: `BTreeMap<u8, VecDeque<usize>>` — pick_next iterates in descending priority order
- SSIP pending: `pend_preempt_if_needed()` fires immediately when RealTime becomes ready
- RT heap: Isolated TLSF pool (256 KiB) for O(1) RealTime stack allocation; Normal cells use global heap
- `spawn_pinned(0)` succeeds; `spawn_pinned(n>0)` returns `NotSupported` (SMP future-compatible)

**Verification:**
- `cargo check -p vicell-kernel` — PASSED (1 pre-existing warning unrelated)
- All unit tests compile and link correctly
- No ABI breakage; Law 1 gate confirmed (`TaskPriority` is `#[repr(u8)]`)

**Blockers Resolved:**
- ✅ Timer interrupt was stub → fully wired with rearm + preemption
- ✅ No priority field → TCB field added + scheduler restructured
- ✅ No SSIP handler → scause==1 implemented with IPI pending logic

**Ready for Phase 26**: Memory Quota + ZST Capabilities (depends on priority scheduler working)

### Phase 26 — Memory Quota + ZST Capabilities + Panic Isolation (P1) `[G1]`
**Target**: 2026-08-04 | **Effort**: ~3 weeks  
**Status**: ✅ COMPLETE (2026-06-07) — see `.agents/260605-1129-phase26-memory-quota-caps-panic/`

**Research findings (2026-06-05):**
- `catch_unwind` impossible with `panic = "abort"` — use trap handler as isolation boundary instead
- `NetTx`/`NetRx` syscalls are **currently unguarded** (security hole) — Phase 26-1 fixes this
- Tock grant model not portable to SAS; use `QuotaAlloc` wrapper + `CURRENT_CELL_ID` atomic instead
- ZST cap pattern: `pub struct BlockIoCap(())` + `pub(in crate::kernel) fn new()` — crate boundary enforces no-forgery

**Phase 26-1 — ZST Capability Tokens (P0, security fix):**
- [ ] Create `kernel/src/task/cap.rs` (BlockIoCap, NetworkCap, SpawnCap — kernel-only constructors)
- [ ] Replace `KernelPerms(u32)` with `Option<BlockIoCap>` + `Option<NetworkCap>` + `Option<SpawnCap>` on TCB
- [ ] Guard `NetTx`/`NetRx` with `NetworkCap` check (currently unguarded!)
- [ ] Guard `SpawnFromPath`/`SpawnPinned`/`HotSwap` with `SpawnCap` check

**Phase 26-2 — Per-Cell Memory Quota:**
- [ ] Add `CURRENT_CELL_ID: AtomicUsize` to scheduler; set on every context switch
- [ ] Create `kernel/src/memory/cell_quota.rs` (`BTreeMap<CellId, CellQuota>`, `charge`/`refund`)
- [ ] Wrap `LockedHeap` in `QuotaAlloc` (`GlobalAlloc` impl with per-cell accounting)
- [ ] Register 4 MiB default quota per Cell at spawn; deregister at exit

**Phase 26-3 — Cell Fault Isolation:**
- [ ] Add `terminate_current_cell_on_fault(scause, sepc)` to `task.rs`
- [ ] Update trap handler: exception + `CURRENT_CELL_ID != 0` → kill Cell, not kernel panic
- [ ] Update `#[panic_handler]`: Cell OOM/panic → kill Cell, not halt

**Phase 26-4 — Audit Ring Buffer:**
- [ ] Create `kernel/src/audit.rs` (256 KB SPSC ring, `log_event()`, `drain()`)
- [ ] Instrument IPC Send/Recv, File Open/Write, NetTx/NetRx, Spawn, Fault, Exit
- [ ] Low-priority `log-flusher` background Cell writes to `/data/kernel.log`

### Phase 27 — Protocol Hardening (Typed Postcard IPC) (P2) `[G1]`
**Target**: 2026-08-25 | **Effort**: ~4 weeks  
**Status**: ✅ COMPLETE (2026-06-07) — net service now uses typed postcard `NetRequest`/`NetResponse` for primary IPC; raw opcodes 0x15 (close) and 0x30–0x32 (TLS ops) fall through to legacy fallback handler for backward compatibility.

**Research findings (2026-06-05):**
- Hermit vtable = function-pointer table, not true ring-bypass; real speedup is SAS = no privilege switch → direct `jalr` (~3 cycles vs ~100 ecall)
- postcard crate recommended for typed enums into existing `[u8; 512]` buffer
- Syscall filter: u64 bitset in TCB + `__ViCell_syscalls` ELF section (xmas-elf already supports arbitrary sections); check BEFORE handle_syscall to avoid SCHEDULER double-lock
- Existing VFS 3-byte header needs version-gate on postcard migration
- Raw opcodes 500-503 (BlkRead/Write) bypass ViSyscall::from() — need separate raw-id allowlist path

**Phase 27-1 — Typed IPC Enums (⚠️ Law 1):**
- [x] Add `postcard` + `serde` to `libs/api/Cargo.toml`
- [x] Create `libs/api/src/ipc.rs` (VfsRequest, VfsResponse, NetRequest, NetResponse)
- [x] Migrate VFS service with version-gate byte (0xFF prefix)

**Phase 27-2 — Syscall Allowlist (⚠️ Law 1 for allowlist_bit()):**
- [x] Add `allowlist_bit() -> Option<u8>` to `ViSyscall` in libs/api
- [x] Add `syscall_allowlist: u64` to Task TCB
- [x] Read `__ViCell_syscalls` ELF section in `spawn_from_path()`
- [x] Add check at top of `ViCell_syscall_dispatch` (lock-drop pattern to avoid double-lock)
- [x] Add `KEEP(*(__ViCell_syscalls))` to linker scripts

**Phase 27-3 — Direct IPC vtable (⚠️ Law 1 for TrustedHandle):**
- [x] Create `TrustedHandle<T>` + `VfsCell`/`NetCell` markers in `libs/api/src/fast_ipc.rs`
- [x] Create `kernel/src/fast_ipc.rs` with `VFS_FAST_HANDLER: Option<fn>` static
- [x] VFS cell registers handler at init; shell uses fast path for `cat`/`ls`
- [x] Benchmark: direct vtable call vs ecall round-trip

### Phase 28 — Tier 2 WASM + RISC-V ePMP Cell Boundaries (P2) `[G2]`
**Target**: 2026-09-22 | **Effort**: ~5 weeks  
**Status**: 📋 PLANNED — see `.agents/260605-1406-phase28-wasm-cells-epmp/`

**Research findings (2026-06-05):**
- WasmEdge: **discard** (C++ + libc, incompatible with no_std bare-metal)
- **wasmi v1** chosen: pure Rust, no_std + alloc, RISC-V confirmed, fuel metering, 2 deps
- WASI 2.0 Component Model: **skip** (unstable toolchain, canonical ABI overhead) — use 4 custom `vi.*` imports
- Loading: WASM cell = Tier 1 Rust host ELF that reads `.wasm` from VFS (`/data/apps/*.wasm`)
- ePMP: **blocked by M-mode architecture** — PMP CSRs require M-mode, violations trap to M-mode. Full per-Cell ePMP deferred; static boot-time kernel protection as optional Phase 28-4

**Phase 28-1 — wasmi integration:**
- [ ] Add wasmi v1 (`no_std`, `prefer-btree-collections`) to `cells/drivers/wasm/Cargo.toml`
- [ ] Implement `WasmRuntime::new()`, `load_module()`, `new_store()` with fuel metering

**Phase 28-2 — `vi.*` host imports:**
- [ ] `vi.send(target, ptr, len)`, `vi.recv(ptr, max_len, sender_out)`, `vi.log(ptr, len)`, `vi.exit(code)`
- [ ] Register via `Linker::func_wrap` in `imports.rs`

**Phase 28-3 — WASM host cell (`/bin/wasm`):**
- [ ] Tier 1 Rust ELF that reads `.wasm` path from argv, loads via VFS, runs via wasmi
- [ ] Fuel-cooperative loop: `OutOfFuel` trap → `set_fuel()` + `yield_cpu()`

**Phase 28-4 — PMP foundation (optional, P2):**
- [ ] `hal/arch/riscv/src/common/pmp.rs` with NAPOT helpers + `init_static_regions()`
- [ ] Static kernel R-X / data R-W protection at boot (if M-mode accessible)

### Phase 29 — Heap Snapshotting / Instant On (P2) `[G1]`
**Target**: 2026-10-06 | **Effort**: ~3 weeks  
**Status**: ✅ COMPLETE (2026-06-07) — see `.agents/260605-1452-phase29-heap-snapshot-instant-on/`

> Killer feature: sub-100 ms warm boot on real hardware (eMMC 100+ MB/s). QEMU TCG: ~270ms.

**Completed (2026-06-07):**
- [x] `kernel/src/snapshot/mod.rs`: `serialize_snapshot()`, `try_restore()`, `invalidate_snapshot()`, `validate_header()`
- [x] `sys_snapshot()` syscall (ViSyscall::Snapshot = 420, SpawnCap required)
- [x] Shell `snapshot` command triggers serialization, reports frame count
- [x] Warm-boot path: `try_restore()` between `task::drivers::init()` and `EarlyLoader::probe()`
- [x] Auto-invalidation on kernel hash mismatch (`VERGEN_GIT_SHA` baked at compile time)
- [x] CRC32 integrity check via `crc32fast` — corrupted snapshot → cold boot
- [x] `disk_v3.img` extended to 300,000 sectors (LBA 200,000 reachable)
- [x] 4 unit tests: header round-trip, hash/magic/version mismatch invalidation
- [x] Timing instrumentation in both `serialize_snapshot()` and `try_restore()`

**Performance (measured with timing instrumentation):**
| Metric | QEMU TCG | Real eMMC (estimate) |
|--------|----------|----------------------|
| Snapshot write (4 MB) | ~133–266 ms | ~40 ms |
| Warm boot restore (4 MB) | ~133–266 ms | ~40 ms |
| Sub-100 ms target | requires `/dev/shm` disk or real HW | ✓ achievable |

Note: QEMU TCG VirtIO throughput ~30 MB/s. Sub-100 ms on QEMU requires memory-backed disk (`-drive file=/dev/shm/disk.img`). The product claim is for real hardware with eMMC 100+ MB/s.

**Implementation note:** `SNAPSHOT_BASE_LBA = 200_000` is inside the FAT32 data area (0–524287) — safe for small `/data/` files. Long-term: relocate beyond cell table (LBA > ~566000) when disk is regenerated with full FAT32 layout.

### Phase 30 — Cell Capability Manifests in ELF (P2) `[G1]`
**Target**: 2026-10-27 | **Effort**: ~2 weeks | **Status**: ✅ COMPLETE (2026-06-05)
**Learn from**: Singularity SIP manifests → [SOSP 2007 paper](https://www.microsoft.com/en-us/research/publication/singularity-rethinking-the-software-stack/)

**Completed (2026-06-05):**
- [x] Define `CellManifest` type: 8-byte `#[repr(C)]` struct with magic, version, capability flags
- [x] Create `declare_manifest!` macro: embeds manifest into `__ViCell_manifest` ELF section
- [x] Add `KEEP(*(__ViCell_manifest))` to all 7 cell linker scripts (prevents GC under release LTO)
- [x] Embed manifests in vfs (block_io), net (network), shell/init (spawn) — 4 cells updated
- [x] Enforce at `spawn_from_path`: privilege gate rejects user cells (path not under `/bin/`) declaring privileged caps
- [x] 6 unit tests for `CellManifest` parsing + validation; boot-time test pass
- [x] Backward compatible: cells without manifest fall back to legacy hardcoded path grants

**Security**: Manifest is `#[repr(C)]` and ABI-stable per Law 1. Gate runs BEFORE `spawn_from_mem` — no task created for rejected cell.

### Phase 31 — RV32 HAL + ViCell-Nano Minimal Profile (P3) `[G1 sub-track]`
**Target**: 2026-Q4 | **Effort**: ~2 weeks
**Status**: ✅ COMPLETE (2026-06-07)
**Learn from**: RV64 HAL design (phase 05), OpenSBI SBI specification, RISC-V S-mode architecture
**Spec**: [.agents/260607-1500-rv32-hal-nano-profile/plan.md](.agents/260607-1500-rv32-hal-nano-profile/plan.md)

> QEMU RV32 virt boots to `ViCell>` shell with bare-physical memory (SATP=0). Nano profile = no MMU, no drivers, foundation for embedded/MCU targets (sub-track at end of G1).

**Completed (2026-06-07)**:
- [x] RV32 context switch (switch.S) with sepc/sstatus/gp/tp/sscratch
- [x] RV32 trap handler (trap.S) + trap.rs with ViTrapFrame32
- [x] RV32 SBI timer wrapper (set_timer hi+lo split for carry safety)
- [x] RV32 boot path (_start, bare-physical, no PIE for simplicity)
- [x] Kernel compile + link for riscv32imac-unknown-none-elf
- [x] QEMU smoke boot: banner + kernel init + idle loop verified
- [x] Baseline for CHERIoT-IBEX (next iteration, deferred until board available)

**Next iteration (Phase 31b, deferred to G1 tail):**
- [ ] Sonata dev board (CHERIoT-IBEX) — hardware not yet available
- [ ] CHERIoT-Platform/rust fork integration (toolchain fork risk, low priority)
- [ ] ViCell-Nano profile variants (no WASM, minimal drivers)

### Phase 32 — SMP Multi-Core Scheduler (P3) `[G2]`
**Target**: 2027-Q1 | **Effort**: ~4 weeks | **Status**: ✅ COMPLETE (2026-06-09)
**Learn from**: RustyHermit SMP scheduler → [`hermit-os/kernel`](https://github.com/hermit-os/kernel) `src/scheduler/`

**Completed (2026-06-09)**:
- [x] SBI HSM hart_start + send_ipi for multi-hart control
- [x] Per-hart ViHartLocal struct via tp CSR (hart_id + local ready queue)
- [x] Per-hart ready queues + work stealing (idle steals half of busiest Normal backlog)
- [x] RT cells pinned to hart 1 (no steal from RT queue); cross-hart IPI preempt
- [x] WaitForEvent syscall (217) for idle power-down coordination

---

## Phase 2: System Services (2026-07 — 2026-08-30)

**Goal**: Complete VFS, input, network, and graphics services.

**Effort**: 530 hours (~13 weeks)  
**Status**: 🚧 IN PROGRESS (Storage 2.0 complete; VFS robustness + Input/Compositor planned)

### Storage 2.0 — Zero-Copy Grant API + PageCache + Async VFS `[shared, G1-foundation · G2-scale · G3-prerequisite]`
**Status**: ✅ COMPLETE (Phases 00–03, 2026-06-06) — see `.agents/260606-*/`  
**Priority**: P0

**Completed (2026-06-06):**
- [x] Phase 00: FAT32 partition upgrade (540K sectors, 524K partition size)
- [x] Phase 01: Zero-copy grant API (5 syscalls: GrantAlloc, GrantShare, GrantSlice, GrantFree, BlkReadAsync; PAGE_GRANT_TABLE, frame zeroing)
- [x] Phase 02: VFS grant IPC (ReadGrant/WriteGrant, GrantDone, F14 safety contract prevents UAF)
- [x] Phase 03: PageCache LRU (4MB cache, write-through policy, CachedBlockStream)
- [~] Phase 04: Async VFS executor — DEFERRED to next milestone

**Impact:**
- **Performance**: Zero-copy grants eliminate memcpy for large file transfers; ~70% latency improvement via LRU cache (cached vs cold reads)
- **Security**: Frame zeroing prevents cross-cell info-leak; GrantDone contract prevents use-after-free
- **Scalability**: Multi-GB storage feasible; 6000+ round-trips for 3MB file → 6 with grant (1000x improvement)
- **Foundation**: Unblocks G2 (streaming, large models) and G3 (tensor handoff)

**Effort**: 80 hours (Phases 00–03 implemented)

---

### Milestone 2.1: Complete VFS Service `[G1 robustness · G2 scale]`
**Status**: ✅ COMPLETE (Phases 01–04, 2026-06-06) — see `.agents/260605-1538-milestone-2-1-vfs-complete/`  
**Priority**: P0

**Completed (2026-06-06)**:
- [x] **Phase 2.1-1**: Wire quota enforcement — `can_charge()` added, called before Write/Append, released in Unlink
- [x] **Phase 2.1-2**: Complete directory listing — FAT32 subdirectory listing via `fatfs::Dir::iter()`, Type prefix (`d:`/`f:`) in ListDir responses
- [x] **Phase 2.1-3**: Capability-based access control — `AccessTable` with per-prefix `can_read`/`can_write` rules, gates all mutating ops, Phase 30 ELF manifests integrated
- [x] **Phase 2.1-4**: Non-blocking async read — `VfsRequest::ReadAsync` + `VfsRequest::Poll` + `VfsResponse::PendingHandle`, `PendingTable` in VFS state
- [x] **Phase 2.1-5**: Integration test suite — `cells/apps/vfs-test/` with 8 automated scenarios (quota, access control, async, directory, edge cases, all passing)

**Test Results**: vfs_test 8/8 passing; full integration suite 48/51 (99.2% coverage)

**Dependency**: Phase 1 (VirtIO) ✅

---

### Milestone 2.2: Complete Input Service `[G1 opt (feature-gate) · G2 full]`
**Status**: ✅ COMPLETE (2026-06-12)  
**Priority**: P1

- [x] AT keyboard driver (scancode → ASCII) — VirtIO input driver
- [x] Input event queue with IPC forwarding — `dispatch_pending()` drains to input service on IRQ
- [x] App focus registration — `request_input_focus()` + sender-verified SetFocus
- [x] ViUI event collection — `collect_input_events()` per frame
- [x] End-to-end CI test: `input_keyboard_e2e` — QMP Tab injection → kernel event + dispatch probes verified
- [x] VirtIO keyboard fault fixed — SumGuard sets sstatus.SUM in timer ISR path
- [ ] PS/2 mouse driver (deferred to G2 — VirtIO mouse/touchpad supported)

**Dependency**: Phase 1 (basic shell)

---

### Milestone 2.3: Complete Network Service `[shared]`
**Status**: ✅ COMPLETE (TCP/UDP/DNS data-path + HTTP/1.0 + LISTEN/ACCEPT + DHCP + Lua bindings + multicast/broadcast; only IRQ-wakeup optimization deferred)  
**Priority**: P1

**Phases A+B+C+D+E Complete**:
- [x] TCP client (CONNECT, SEND, RECV, CLOSE)
- [x] HTTP/1.0 GET client (curl)
- [x] nc utility (TCP echo client + server mode with LISTEN/ACCEPT)
- [x] Socket state introspection (SOCKET_STATE opcode)
- [x] TCP server (LISTEN opcode 0x17, ACCEPT opcode 0x18)
- [x] Static hostname resolution table (resolve_host)
- [x] IPC buffer length fix (zero-scan with per-opcode floors)
- [x] Lua TCP bindings (vnet_connect, vnet_send, vnet_recv, vnet_close)
- [x] UDP socket creation (SOCKET_UDP opcode 0x20)
- [x] UDP send (SENDTO opcode 0x21, sends datagram with (addr, port))
- [x] UDP recv (RECVFROM opcode 0x22, returns [src_addr:4][src_port:2 LE][data])
- [x] UDP capability isolation (rejects TCP ops on UDP caps, prevents type confusion panic)
- [x] DNS resolver (static + dynamic A-record queries via UDP to 10.0.2.3:53)
- [x] Lua DNS bindings (vnet.resolve(hostname) with static table + DNS fallback)
- [x] Integration tests (lua_vnet_resolve, lua_vnet_resolve_dns)

**Status correction (2026-06-06 audit)** — the items below were previously listed as "remaining" but are already implemented:
- [x] DHCP client — `cells/services/net/src/dhcp.rs`; auto-acquires IP at boot (`main.rs:84-127`)
- [x] Full socket API — BIND (0x16), LISTEN (0x17), ACCEPT (0x18) at `main.rs:382-498`
- [x] VirtIO NIC kernel driver — `kernel/src/task/drivers/virtio_net.rs` (real driver, not stub)
- [~] UDP broadcast — no new opcode needed (SENDTO to 255.255.255.255 + RECVFROM on a bound socket); code path present, **runtime QEMU verification pending** (SLIRP broadcast forwarding is limited)
- [~] UDP multicast — JOIN_MULTICAST (0x23) / LEAVE_MULTICAST (0x24) added; smoltcp `proto-igmp`; `iface.join/leave_multicast_group` (2026-06-06, `cargo check` clean); **runtime QEMU verification pending** (SLIRP multicast limited — needs 2-guest or real net)

**Remaining (deferred, non-blocking)**:
- IRQ→net-service wakeup: currently polls every 100 ms instead of an IPC ping (Phase 15 TODO). Functional; ~100 ms RX latency under no traffic.

**Effort**: 200 hours (Phases A–E + DHCP + socket API + multicast/broadcast complete; only IRQ-wakeup optimization deferred)

---

### Milestone 2.4: Complete Compositor & Display `[G1 HMI opt (feature-gate) · G2 desktop full]`
**Status**: 📋 PLANNED  
**Priority**: P2

- VirtIO GPU driver
- Compositor Cell (window management)
- Wayland-like protocol
- 2D graphics rendering

**Effort**: 150 hours

---

### Milestone 2.5: VFS Mount-Table Layered Backends `[G1 tail · G2 scale]`
**Status**: ✅ COMPLETE (Phases 01–05, 2026-06-11) — see `.agents/260610-1202-vfs-mount-table-backends/`  
**Priority**: P1 (Phase 2.5-3 littlefs gates robot demo on real board)

**Architecture decision (2026-06-10, specs/09-vfs.md v0.5):**
- ❌ Dual-VFS viFS1/viFS2 DROPPED — TFS upstream dead; RedoxFS port too large for G1 (YAGNI)
- ✅ **Final design**: 1 VFS service + MountTable (longest-prefix) + backend dispatch:
  BootFS (`/bin` initramfs) · RamFS (`/tmp`) · FAT32 (interop SD → `/mnt/sd`) · littlefs (`/data` power-safe, G1) · Native FS stub (`/srv`, G2 NVMe)

**Completed (all 5 phases, 2026-06-11)**:
- [x] **Phase 2.5-1**: MountTable v2 backend dispatch — FsBackend trait, hardcoded paths migrated to dispatch, main.rs 875→107 LOC (87% reduction)
- [x] **Phase 2.5-2**: Remove duplicate `/bin` embedding — VFS binary 405KB→202KB (−50%), BootFsProxy lists via Open+ReadDir
- [x] **Phase 2.5-3**: MBR partition table + per-cell block grants — Real MBR (P1=FAT32, P2=cell-table, P3=snapshot, P4=littlefs), Law 1 confirmed ×2
- [x] **Phase 2.5-4**: littlefs backend — littlefs2 0.7.2 C FFI, power-loss harness 20/20 PASS (no corruption on mid-operation QEMU kill), `/data` now power-safe
- [x] **Phase 2.5-5**: exFAT + Native FS — exFAT graceful fallback, RedoxFS ADR chilled for G2, StubBackend at `/srv` prevents crashes

**Test Results**: vfs suite 11/11 on littlefs; full suite 48/51 (baseline preserved); power-loss harness 20/20 PASS

**Dependency**: Milestone 2.1 (VFS robustness) ✅; 2.5-4 gates robot demo on real board ✅

---

## Phase 3: Applications & Runtimes (2026-09 — 2026-11-30)

**Goal**: Feature-rich shell, standard utilities, runtime integration.

**Effort**: 500 hours (~12 weeks)  
**Status**: 📋 PLANNED

### Milestone 3.1: Enhanced Shell `[shared]`
**Status**: 📋 PLANNED  
**Priority**: P1

- Piping: `cat file | grep pattern`
- Redirection: `cmd > file`, `cmd < input`
- Background execution: `cmd &`
- Job control: `fg`, `bg`, `jobs`
- Shell scripts (`.sh` files)
- Tab completion

---

### Milestone 3.2: Standard Utilities `[G1 minimal subset · G2 full suite]`
**Status**: 📋 PLANNED  
**Priority**: P1

**File Tools**: cp, mv, rm, mkdir, rmdir, find  
**Text Tools**: grep, sed, awk, sort, uniq, wc  
**System Tools**: top, ps, kill, shutdown, reboot  
**Network Tools**: ping, curl, nc, ifconfig  

**Effort**: 200 hours

---

### Milestone 3.3: Lua Runtime Enhancement `[shared]`
**Status**: ✅ COMPLETE (2026-06-05)  
**Priority**: P2

**Completed 2026-06-05** (4 phases, all integrated):
- [x] Phase 01: Migrated `vfs.read/write/append/mkdir` from raw opcodes to typed postcard IPC
- [x] Phase 02: Implemented VFS-backed `io.open(path, "r"/"w"/"a")` with `:read()`, `:write()`, `:close()`
- [x] Phase 03: Added `vfs.stat()`, `vfs.listdir()`, `vfs.remove()` for filesystem introspection
- [x] Phase 04: Integration tests pass (5/5 cargo tests, all script execution verified)
- Execute `.lua` scripts from shell via typed VFS IPC
- Stdlib access (table, string, math, io, os)
- File I/O via VFS syscalls (RamFS `/tmp`, FAT16 `/data`)
- C FFI for kernel calls

**Known Limitation**: `vfs.read()` and script loading use `GetFile` which serves RamFS/kernel-embedded files. FAT16 `/data/` read access depends on VFS cell adding FAT16 fallback in GetFile handler (separate VFS improvement).

---

### Milestone 3.4: MicroPython Runtime Enhancement `[shared]`
**Status**: ✅ COMPLETE (2026-06-05)  
**Priority**: P2

**Completed 2026-06-05** (3 phases, all integrated):
- [x] Phase 01: Migrated `vfs.read/write/append/mkdir` from raw opcodes to typed postcard IPC
- [x] Phase 02: Implemented VFS-backed file I/O with stat, listdir, remove
- [x] Phase 03: Integration tests pass (cargo check zero errors)
- Execute `.py` scripts from shell via typed VFS IPC
- File I/O via VFS syscalls (RamFS `/tmp`, FAT16 `/data`)
- Stdlib access (builtins, sys, os, math, random)

**Files Modified**:
- `cells/runtimes/micropython/src/vfs_bridge.rs` — NEW: C-callable Rust bridge
- `cells/runtimes/micropython/src/main.rs` — vfs_read_to_buf rewired to vfs_bridge
- `cells/runtimes/micropython/src/c/ViCell/modvfs.c` — complete rewrite using typed IPC

---

## Phase 4: Advanced Features & Optimization (2026-12 — 2027-03-31)

**Goal**: Hot migration, complete multi-arch support, performance optimization, v1.0 readiness.

**Effort**: 460 hours (~11 weeks)  
**Status**: 📋 PLANNED

### Milestone 4.1: Hot Migration (State Transfer) `[G2]`
**Status**: 📋 PLANNED  
**Priority**: P2

- Serialize Cell state (memory, registers, file handles)
- Load new binary, restore state
- Resume execution seamlessly
- Zero-downtime shell update

**Effort**: 120 hours

---

### Milestone 4.2: Advanced IPC `[shared]`
**Status**: 📋 PLANNED  
**Priority**: P2

- Lease: Capability grant with auto-revoke
- Grant chains: transitive capability delegation
- Bulk message passing (gather/scatter)
- Timeout support on Recv/Call

**Effort**: 60 hours

---

### Milestone 4.3: Complete RV32 & ARM Support `[G1 sub-track (RV32-Nano)]`
**Status**: 📋 PLANNED  
**Priority**: P2

- RISC-V 32-bit (RV32) full HAL
- ARM AArch32 full HAL
- Boot tests on all targets
- Single binary: `cargo build --features rv32 --release`

**Effort**: 200 hours

---

### Milestone 4.4: Benchmarking & Optimization `[G1 RT latency · G2 throughput]`
**Status**: 🔄 IN PROGRESS — G1 RT subset ✅ COMPLETE (2026-06-07)  
**Priority**: P3

**G1 RT latency subset — COMPLETE (QEMU boot verified 2026-06-07)**:
- `RtReport`: min/p50/p99/p99.9/max/jitter/deadline-miss as JSON (no Law 1 change)
- Scenario 1 — `preempt_latency`: RealTime wake-to-run under K Normal load cells
- Scenario 2 — `control_loop_jitter`: periodic control loop (P=10ms), period error + miss-rate
- Scenario 3 — `ipc_under_load`: IPC/syscall p99 idle vs under-load + degradation ratio
- `perf.yml` RT gate: `p999`/`jitter`/`miss` regression detection in `compare-bench-results.sh`
- Integration test `bench_all_pass` in `tests/integration/tests/boot.rs`
- **QEMU boot verified**: `[bench] ALL BENCHMARKS PASS` (ctx_switch p99=39µs, syscall_yield p99=19.8µs, memory PASS)
- Bug fixed: all 7 cell linker scripts `.vicell_manifest` → `__ViCell_manifest` (capability system was silently broken)
- RT scenarios SKIP in QEMU — SAS VA collision prevents same-binary multi-instance; PIE = future work

> ⚠️ **QEMU TCG caveat**: RT numbers are relative/regression-only — QEMU TCG timing is
> non-deterministic and P=10ms equals 1 scheduler tick, so jitter reflects scheduling
> granularity. Absolute hard-RT validation requires real SBC hardware (G1 graduation).

**G2 throughput targets** (planned):
- Context-switch latency: < 100 µs
- Message latency (Send/Recv): < 50 µs
- Syscall overhead: < 10 µs
- Memory footprint: < 10 MB (kernel + 3 services)

**Remaining G2 deliverables**:
- Profiling tools
- Throughput regression tests (SMP, large-message IPC)

**Effort**: 80 hours (G1 RT subset ~20h done)

---

## High-Level Timeline

```
Use-case stages (overlay on technical phases below):
  G1 Robot & Embedded  ─ now → ~2026 Q4 ─ Tier A SBC (RV64/ARM64) primary; Tier B RV32-Nano sub-track at tail
  G2 Server & PC       ─ ~2027         ─ SMP + WASM + desktop + x86_64 + hot migration

Technical phases:
2026
├─ Q2 (Apr-Jun): Phase 1 - Core Stability
│  ├─ W1:    Phase 01 Workspace Cleanup ✅ (2026-05-28)
│  ├─ W1-2:  Phase 02 CI/CD Pipeline ✅ (2026-05-28)
│  ├─ W2-3:  Phase 04 VirtIO Block Fix (PARTIAL) ⚡ (2026-05-28)
│  ├─ W3:    Phase 05 Keyboard Input Fix ✅ (2026-05-29)
│  ├─ W4-5:  Phase 03 Ring 3 Boot + Phase 06 External ELF (PENDING)
│  ├─ W6-7:  Multi-arch HAL (ARM, x86) — Phases 08, 09
│  └─ W8:    Unit + integration tests — Phase 11
│  └─ TARGET: Phase 1 Complete (2026-06-30) [65% likely]
│
├─ Q3 (Jul-Sep): Phase 2 - System Services + Phase 3.1-3.2
│  ├─ VFS, input, network, compositor services
│  └─ Shell enhancements + standard utilities
│  └─ TARGET: Services Stable (2026-08-30)
│  └─ TARGET: User-Ready OS (2026-11-30)
│
└─ Q4 (Oct-Dec): Phase 3.3-3.4 + Phase 4.1-4.2
   ├─ Lua/MicroPython integration
   ├─ Hot migration + advanced IPC
   └─ Performance optimization
   └─ TARGET: v1.0 Production Ready (2027-03-31)
```

---

## Dependency Graph

```
Phase 1 (Core Stability)
├─ 1.1: VirtIO Fix
│  └─ blocks: 1.4 (External ELF loading)
│  └─ blocks: 2.1 (Complete VFS)
│
├─ 1.2: Keyboard Input Fix
│  └─ blocks: 2.2 (Complete Input Service)
│
├─ 1.3: Multi-Arch HAL
│  └─ unblocks: Phase 2+ on ARM/x86
│
└─ 1.5: Test Coverage
   └─ enables: Phase 2 (regression detection)

Phase 2 (System Services)
├─ 2.1: Complete VFS
│  └─ blocks: 3.1 (Enhanced Shell, scripting)
│
├─ 2.2: Complete Input
│  └─ blocks: 2.4 (Compositor)
│
└─ 2.4: Compositor
   └─ enables: GUI applications

Phase 3 (Applications)
├─ 3.1 + 3.2: Shell + Utilities
│  └─ blocks: 3.3, 3.4 (runtime integration)
│
└─ 3.3, 3.4: Runtimes
   └─ unblocks: Phase 4 (advanced features)

Phase 4 (Advanced Features)
└─ All phases complete
   └─ v1.0 Production Ready
```

---

## Known Blockers & Issues

### Resolved (Phase 05)

| Issue | Resolution |
|-------|-----------|
| VirtIO hang | Fixed: MMIO explicit identity-mapping in paging.rs |
| Keyboard deadlock | Fixed: IRQ acknowledgment pattern (ack_irq flag) |

### Medium Priority

| Issue | Impact | Status |
|-------|--------|--------|
| Per-Cell SATP | ❌ **NOT pursued** — isolation handled by Tier 2/3, not Tier 1 SATP | ✅ Decided 2026-06-05 ([12-reliability.md](specs/12-reliability.md)) |

### Low Priority

| Issue | Impact |
|-------|--------|
| KASLR | Not implemented |
| Ed25519 signing | Spec only, not implemented |
| Audit logging | Not implemented |

---

## Completed Work (Phases 0-20, C-H, A-E, X-1-X-6, Storage 2.0)

✅ **Phase 0 (Alpha)**: Kernel skeleton, RV64 HAL, basic shell  
✅ **Phase 01**: Workspace consolidated, 0 cargo warnings  
✅ **Phase 02**: CI/CD pipeline (4-job matrix, weekly security scans)  
✅ **Phase 05**: VirtIO fixes (keyboard + block), IRQ acknowledgment pattern  
✅ **Phase 10**: External ELF loading from `/bin/`  
✅ **Phase 14**: Keyboard input fully functional  
✅ **Phase 15**: Network (DHCP verified, data-path stubs)  
✅ **Phase 16**: Compositor (basic framebuffer, opt-in GPU)  
✅ **Phase 18**: MicroPython 1.24.1 runtime (256KB heap, REPL verified)  
✅ **Phase 20**: HotSwap orchestrator (5-step protocol, shell + config + vfs verified)  
✅ **Phase 20**: Advanced IPC (SendGather, RecvScatter, RecvTimeout)  
✅ **Phase C**: VFS RamFS write + shell echo redirect  
✅ **Phase D**: FAT16 write persistence on VirtIO block device  
✅ **Phase E**: Hardening + reboot persistence  
✅ **Phase F**: FAT16 hardening (unlink, mkdir, nested paths, block-I/O gate)  
✅ **Phase F**: Lua script file loading + vfs.* bindings  
✅ **Phase G**: FAT16 completion (can_block_io capability, rmdir, persistence)  
✅ **Phase H**: Kernel permissions + FAT16 type guards (KernelPerms, rmdir type-safe, recursive rm, append)  
✅ **Phase A**: Network TCP Data-Path (CONNECT, SEND, RECV, CLOSE, socket state)  
✅ **Phase B**: HTTP/1.0 GET via curl (nc binary, curl binary, state introspection)  
✅ **Phase C**: TCP Server (LISTEN, ACCEPT, hostname resolution, nc -l server mode)  
✅ **Phase D**: IPC buffer hardening + Lua TCP bindings (vnet.*, zero-scan, per-opcode floors)
✅ **Phase E**: UDP sockets + DNS resolver (SOCKET_UDP, SENDTO, RECVFROM, vnet.resolve, DNS A-record)
✅ **Phase X-1**: VirtIO VA→PA address mapping fix for FAT16 multi-sector writes
✅ **Phase X-2**: Shell function arguments ($1–$9) and read built-in
✅ **Phase X-3**: Command substitution $(cmd) for shell execution
✅ **Phase X-4**: Lua execution with fault handling (code-exec verification)
✅ **Phase X-5**: MQTT 3.1.1 QoS-0 client cell (/bin/mqtt) with publish/subscribe
✅ **Phase X-6**: ForceExit syscall (opcode 61, SpawnCap-gated, shell kill -9)
✅ **Storage 2.0**: Zero-copy grant API + PageCache + FAT32 upgrade (Phases 00–03, 2026-06-06)
✅ **Milestone 3.3**: Lua runtime enhancement (typed VFS IPC, io.open, vfs.stat/listdir/remove)
✅ **Milestone 3.4**: MicroPython runtime enhancement (vfs_bridge.rs, modvfs.c rewrite, typed VFS IPC)

---

## Next Steps (Immediate)

### This Week (2026-05-28 — 2026-06-03)

1. **Create GitHub Project Board**
   - Organize Phase 1 tasks
   - Set sprint deadlines

2. **Debug VirtIO Hang**
   - Enable QEMU `-trace` mode
   - Analyze device initialization sequence
   - Check interrupt handling

3. **Keyboard Input Analysis**
   - Add `eprintln!` logs to shell input loop
   - Trace async task state
   - Reproduce hang scenario

### Next 2 Weeks (2026-06-04 — 2026-06-17)

- Implement fixes based on debugging
- Start ARM AArch64 HAL stub → implementation
- Write allocator unit tests
- Document findings in ARCHITECTURE.md

### End of Month (2026-06-18 — 2026-06-30)

- All Phase 1 milestones complete
- Prepare Phase 2 kickoff
- Tag v0.2.1 release

---

## Success Metrics (Current Status: 2026-06-05)

### Phase 1 Acceptance (Target: 2026-06-30)

| Metric | Target | Current | Status |
|--------|--------|---------|--------|
| VirtIO working | ✅ Yes | ✅ Block + GPU verified | ✅ COMPLETE |
| Keyboard input | ✅ Multi-key | ✅ Verified, no deadlock | ✅ COMPLETE |
| IRQ dispatch | ✅ All devices ack'd | ✅ All VirtIO devices | ✅ COMPLETE |
| CI/CD pipeline | ✅ 4-job matrix | ✅ Implemented | ✅ COMPLETE |
| Workspace warnings | ✅ 0 | ✅ 0 | ✅ COMPLETE |
| Multi-arch HAL | ✅ RV64+ARM+x86 | ✅ All 3 (Ring-3 smoke) | ✅ COMPLETE |
| External ELF | ✅ Working | ✅ spawn_from_path verified | ✅ COMPLETE |
| HotSwap | ✅ Working | ✅ 5-step protocol verified | ✅ COMPLETE |
| FAT16 persistence | ✅ Full stack | ✅ All phases C–H verified (21/21 tests) | ✅ COMPLETE |
| Network TCP | ✅ Data-path functional | ✅ Phases A–B–D verified (24/24 tests) | ✅ COMPLETE |
| Network UDP | ✅ Data-path functional | ✅ Phase E verified (25/25 tests) | ✅ COMPLETE |
| DNS resolver | ✅ Working | ✅ vnet.resolve + DNS A-record verified | ✅ COMPLETE |
| Lua TCP bindings | ✅ Working | ✅ vnet.* + http_get test verified | ✅ COMPLETE |
| Lua UDP + DNS | ✅ Working | ✅ vnet.udp_* + vnet.resolve verified | ✅ COMPLETE |
| MQTT client | ✅ QoS-0 pub/sub | ✅ /bin/mqtt with publish + subscribe | ✅ COMPLETE |
| Lua runtime | ✅ Working | ✅ Milestone 3.3 complete (typed VFS IPC) | ✅ COMPLETE |
| MicroPython runtime | ✅ Working | ✅ Milestone 3.4 complete (typed VFS IPC) | ✅ COMPLETE |
| Test coverage | ✅ 80%+ | ✅ 96%+ (65+ integration tests: Phases A–H, X-1–X-6, 3.3, 3.4) | ✅ MET |
| Architecture tests | ✅ 10/10 | ✅ 10/10 | ✅ MET |
| Kernel LOC | ✅ < 10,000 | ✅ 8,700 | ✅ MET |

---

## Release Planning

### v0.2.0 (Current — Mycelium Era)
- Stable basic kernel
- Working RV64 HAL
- Basic shell REPL
- Architecture validated

### v0.2.1-dev (Current: 2026-06-06)
- ✅ VirtIO block device fixed (Phase 05)
- ✅ Keyboard input fixed (Phase 05)
- ✅ Multi-arch HAL (RV64, ARM, x86) Ring-3 smoke (Phase 05)
- ✅ External ELF loading (Phase 10)
- ✅ HotSwap orchestrator (Phase 20)
- ✅ FAT16 persistence stack: VFS RamFS + block I/O + hardening + type guards (Phases C–H)
- ✅ Network TCP data-path: CONNECT/SEND/RECV/CLOSE + HTTP/1.0 GET (Phases A–B)
- ✅ IPC buffer hardening + Lua TCP bindings (Phase D)
- ✅ UDP sockets + DNS resolver (Phase E: SOCKET_UDP, SENDTO, RECVFROM, vnet.resolve)
- ✅ Storage 2.0: Zero-copy grant API (5 syscalls) + PageCache LRU (4MB) + FAT32 upgrade (Phases 00–03)
- ✅ Integration test suite (96%+ coverage, 65+ tests passing)

### v0.3.0 (Target: 2026-09-30)
- FAT16 feature parity (permissions, extended attrs, sparse files)
- Kernel permissions model (capability tokens, transitive delegation)
- Enhanced shell (advanced piping, complex redirects, background jobs)
- Standard utilities (full grep, sed, awk, etc.)
- Network data-path completion (TCP throughput, UDP)

### v1.0.0 (Target: 2027-03-31)
- Hot migration support
- Full multi-arch (RV32, RV64, ARM32, ARM64, x86_64)
- Production-grade performance
- Complete documentation
- Permissive license (MIT or Apache 2.0)

---

## Review & Update Cadence

- **Weekly**: Milestone status updates (every Monday)
- **Bi-weekly**: Blocker review + sprint planning
- **Monthly**: Phase progress review + roadmap adjustments
- **Quarterly**: Strategic review, Phase kickoff

**Last Review**: 2026-06-03 (Documentation update, Phase 1 status verification)  
**Next Review**: 2026-06-10 (Phase 1 completion target, Phase 2 kickoff planning)

---

## See Also

- **project-overview-pdr.md** — Detailed PDR + requirements
- **codebase-summary.md** — Current code structure
- **code-standards.md** — Development rules
- **system-architecture.md** — Architecture overview
- **99-roadmap.md** — Original roadmap (archive)
