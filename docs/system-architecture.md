# Cellos System Architecture

**Audience**: Developers new to Cellos
**Level**: High-level (conceptual + key components)
**Version**: 0.2.1-dev (Mycelium Era)
**Last Updated**: 2026-08-29 (opaque KMS-backed C2C identity consumer wired; remote/provider gates remain closed.)

> **Status refresh 2026-08-21**: [Spec 23 Native SDK contract](specs/23-native-sdk-contract.md)
> is ratified as the normative contract for the single Native SDK family. It
> uses named modules and keeps execution tier, runtime profile, stability, and
> availability separate. The Phase 02 acceptance ledger is recorded through
> `LEDGER_RECORDED` at ratified revision `798e8b04`, with implemented, verified,
> and attested lifecycle commits `92340d05`, `635600c8`, and `c538df84`. Phase
> 03 remains `PLANNED`; its production-admission work remains blocked. Its
> current C9 result is `NOT_COMPLETE`; compile, test/runtime, delivery, hardware,
> admission, and hostile-test witnesses remain mandatory before any applicable
> cell can be called `USABLE`. FFI, `rust-std`, and Tier-2 scopes without
> ratified applicability remain non-qualifying. This changes no ABI, source API,
> loader, signing policy, or runtime profile and does not implement Tier 2 native
> domains.
>
> **Status refresh 2026-08-21**: `kernel/src/admission` now contains a private,
> backend-neutral Tier 1 admission seam: a fixed authenticated floor tuple,
> explicit slot/backend outcomes, and a pure borrowed decision function. It
> admits only an exact committed floor binding with an authenticated,
> same-backend stale committed partner; ambiguity, conflicting or forward
> state, and backend failure fail closed to denial or recovery, never floor
> advancement. A private fake plus 17 state and 14 transaction checks exercise
> the model only under `test-hooks`; production builds exclude that harness.
> Independent code and security reviews passed this Core+harness slice, but
> they are not human production approvals. No loader, signing, boot, task, or
> audit path consumes the decision. The existing RPi3 is not a qualified
> independent external floor, so production admission stays disabled and Phase
> 03 stays blocked pending a qualified authenticated rollback-resistant floor,
> persistent production slot/evidence recovery, physical hostile evidence,
> provisioned owner/publisher anchors, production integration and
> no-task-on-denial evidence, required security-owner and independent
> production approvals, and governed ledger/release closure.
>
> **Status refresh 2026-08-21 — PREQUALIFICATION INFRASTRUCTURE COMPLETE /
> ADMISSIBLE EVIDENCE BLOCKED:** the machine-readable 18-row catalog pins all
> 33 stable `C3-ADM-*` `test-hooks` IDs, and its strict parser validates ordered
> runtime output. The public CLI validates only that canonical catalog; it has
> no capture, context, or evidence-writing path. Focused verification recorded
> Python 13/13, RV64 33/33 plus the aggregate test PASS marker, QEMU integration
> 1/1, production-marker exclusion PASS, and an unchanged host baseline of 101
> passed, 0 failed, and 4 ignored. The rejected local capture/writer and its
> generated bundle were removed rather than relabeled. Local runs remain
> non-admissible and retain no Phase 04 evidence. Production parsing/task
> creation, a qualified floor and persistent recovery, physical hostile
> evidence, the production profile, provisioned anchors, both human approvals,
> and ledger/release closure remain external gates; production stays disabled
> and Phase 04 stays `BLOCKED`.
>
> **Status refresh 2026-08-21 — RUST `STD` FEASIBILITY PACKAGE VERIFIED /
> SECURITY BACKING AND HUMAN APPROVAL BLOCKED:** the pinned inventory reconciles
> 27/27 sys modules, 36/36 hooks (8 Supported, 10 Unsupported, 18 Deferred), 46
> Rust sources, and an exact six-path kernel security inventory. Verification
> passed 33/33 feasibility cases, 57/57 validator adversarial attacks, 36/36
> security-manifest tamper attacks, and the unchanged host baseline of 105
> passed, 0 failed, and 4 ignored; all 106 approval inputs, including the
> governed GetRandom hostile-evidence report, and their links and digests match.
> This verifies only the conditional compiler/runtime contract and fixture-only
> validator. There is no PAL, target, sysroot, runtime, live capture, or promotion. All six human approval rows remain `NOT GRANTED`, the
> implementation checkpoint is `BLOCKED`, and Phase 06 remains pending and
> dependency-blocked on Phase 03.
>
> **Status refresh 2026-08-21**: [Spec 18c Publisher Provenance Envelope](specs/18c-publisher-provenance-envelope.md)
> is a **proposed** Claim-A contract, pending security-owner and
> independent-reviewer approval. It introduces no producer, kernel parser,
> production build profile, or admission path, and does not change the Phase 03
> ledger state (`PLANNED`) or unblock production work. Production admission
> remains disabled until the external-floor and owner-record gates are qualified
> and approved.
>

> **Status refresh 2026-08-22 — MANIFEST-V2 TOOLING COMPLETE / PHASE 07
> ATOMIC PREREQUISITE VERIFIED / PHASE 08 PREDESIGN BLOCKED**: Manifest-v2
> tooling remains complete, not production-loader readiness. It retains the
> completed unique-section classification—`Absent`, `Valid` (v1/v2), or
> `Malformed`—before task creation; only genuine absence follows the explicit
> legacy policy. Malformed ELF metadata, duplicate or `SHT_NOBITS` manifest
> sections, invalid lengths/version/class/flags/reserved bytes, and unsupported
> extended numbering fail closed. Rust v2 remains the exact 16-byte
> little-endian record, and Zig continues to emit its exact 8-byte v1 record
> with compatible legacy upcast behavior.
>
> The Phase 07 atomic-publication prerequisite passed after a fresh `test-hooks`
> build/sign:
> a populated-fixture one-hart VFS run (1/1; AP-00–11 and AP-15, with AP-13
> explicitly `SKIP`) and an SMP atomic run (1/1; AP-00–15). The SMP proof
> includes AP-02 PTE/TLB restoration evidence, an AP-13 remote-hart scheduler
> witness, and terminal/aggregate completion markers. Its state remains
> `ATOMIC_PUBLICATION_PREREQUISITE_COMPLETE / PHASE07_BLOCKED`.
>
> Phase 08's Manifest-v3 ABI predesign validator passes 20/20 with pinned
> consumer-inventory/content-digest artifacts, but its state is
> `PREDESIGN_COMPLETE / PHASE08_BLOCKED`; it depends directly on Phases 03, 05,
> and 07 and introduces no Manifest-v3 code, readiness decision, or approval.
> Full Phase 07 and Phase 08 remain blocked by `CELLOS-LOADER-SIG-001`'s Phase
> 03 provenance/signature boundary, the Phase 04 production-admission gate,
> and the Tier 2 native-domain gate.
>
> `CELLOS-VFS-SMP-006` is closed after the owner-lifetime lifecycle
> implementation passed API90, an RV32 release compile, fresh `test-hooks`,
> one-hart VFS 2/2, and two-hart VFS 7/7, followed by final quality and
> security closure PASS. RV32 runtime remains unavailable on this host because
> OpenSBI firmware is missing; it is a non-blocking compile-only evidence gap,
> not a runtime claim.

> **Status refresh 2026-08-21**: Spec 22 is now the mandatory design and
> negative-test gate before Tier 2 native domains can be implemented or offered.
> It covers private page-table ownership and switching, recoverable
> domain-aware syscall copies, fault/teardown recovery, revocable IPC grants,
> DMA fencing, adversarial tests, and separate build-capability/runtime-
> admission rollback. No Tier 2 runtime mechanism exists; current unsigned
> native cells remain in the shared SAS and are not contained.

> **Status refresh 2026-08-20**: the HAL split covers all seven current
> board selections. Root `boards/` descriptors contain integration data only;
> `hal/soc/{riscv,arm-virt,bcm27xx,x86}` owns immutable platform facts; `hal/arch`
> and shared Driver Cells own mechanisms. Required-DTB boards fail closed on
> missing enabled hardware, typed driver lists gate initialization, and CI runs
> the ownership plus seven-board build matrix. RV64 and AArch64 QEMU runtime gates
> pass; QEMU q35 x86_64 is the current x86 integration board, and the Phase 05
> PCIe lane passes in QEMU only with bus 0 ECAM, bounded BAR registration, and
> q35-gated VT-d before DMA-capable cells. RPi3 has merged physical smoke,
> external-SD, and BCM GPIO/I2C/SPI evidence; VF2, Pioneer, RPi4, and physical
> x86 remain hardware-gated.

> **Status refresh 2026-08-19**: HAL↔kernel Rust ABI hooks are now single-sourced
> in `hal/traits/arch/src/kernel_abi.rs`; HAL arch crates import the shared
> declarations instead of hand-writing local `extern "Rust"` blocks, and the
> x86 page-fault hook is declared with a matching compile-time assertion. The
> boundary script now rejects new local HAL ABI blocks. This is a structural
> cleanup only: no public ABI changed and no physical-board claim was added.

---

## Development Execution and Production Admission Boundary

[ADR-0007](decisions/0007-development-first-hardware-constrained-execution.md)
separates the development platform from production trust anchors. The current
platform inventory is QEMU, two owner-reported Raspberry Pi 3 Model B+ boards,
and incoming sensors. The architecture directs work to those assets now and
authorizes no additional hardware procurement at this time.

| Platform | Permitted evidence | Ceiling and prohibition |
|---|---|---|
| Host and QEMU | Software contracts, integration behavior, emulated-device behavior, and bounded hostile software scenarios | `host` or `qemu`; never physical, service, protected-root, secure/measured-boot, or production qualification |
| Two owner-reported Raspberry Pi 3 Model B+ boards | G1 development boot, peripheral, sensor, and exact-board hardware-integration behavior after exact serial/revision/condition reconciliation | `physical` development evidence for the exact board only; never a production-security target, independent external floor, or production qualification |
| Incoming sensors | Protocol/fixture work before receipt and exact-device behavior after physical exercise | Host/QEMU before receipt; `physical` development evidence after receipt; never broader device-family or production qualification |

Lanes advance independently to these ceilings. QEMU, RPi3, sensor, and local
Cell-to-Cell runtime work is current executable work. Defects in supported
paths are current-scope technical debt; remote/public operation and later
product-stage expansion are future capabilities; unavailable exact assets and
vendor evidence are external-gated prerequisites; production admission and
governed release closure are production release gates. These classifications
must not be collapsed into a single technical-debt queue.

The production architecture is unchanged and remains fail-closed. Remote C2C
identity where applicable, protected relay identity, a production KMS/root,
secure/measured boot, a qualified authenticated rollback-resistant external
floor with persistent recovery, physical hostile evidence, an authenticated
runner, required human approvals, and governed release-ledger closure remain
mandatory before the applicable production-admission or production-release
claim. No stock TPM or generic secure-element counter is selected as the floor.
[ADR-0005](decisions/0005-mutual-tls-relay-identity.md) and
[ADR-0006](decisions/0006-block-production-root-pending-exact-product-evidence.md)
continue to govern protected remote identity and root selection. Their gates
block only their production milestones and never promote development evidence;
production admission stays disabled.

### Authenticated Software Evidence Boundary

`cellos.authenticated-evidence/v1` binds a GitHub-hosted workflow identity,
revision, run-id/attempt, runner class, hashed inputs, raw logs, and producer
metadata into a content-addressed manifest attested by GitHub Actions Native
Attestations. Offline admission verifies the exact signer and attested manifest
digest before revalidating every member.

Replay control is external to the submitted bundle. An explicitly provisioned,
operator-owned state file is opened through a pinned directory, locked across
verification and consumption, and atomically advanced only for a strictly newer
run-id/attempt. Failed verification cannot advance state; exact replay,
regression, concurrent double-consumption, unsafe ownership or modes, and path
substitution fail closed.

This boundary authenticates software evidence carriage, not the evidence class
inside it. Host and QEMU results retain their existing ceilings. It cannot
produce physical, secure-root, cloud/service, human-approval, admissibility, or
production qualification.

---

## Core Philosophy

Cellos is **NOT** a traditional Linux-style OS. It uses:

- **Cellular Architecture**: Software organized as **Cells** (not processes), all sharing one address space
- **Language-Based Isolation**: Rust's type system (not hardware MMU) provides isolation
- **Single Address Space (SAS)**: Kernel and all Cells live in one virtual memory space, with no process boundaries
- **Zero-Copy IPC**: Capability-based message passing using owned buffers

**Impact**: No expensive context switches, no TLB flushes, minimal privilege escalation overhead.

### Scope Doctrine — SAS/LBI-first (decided 2026-06-23)

The architecture above is the product. **New capability is built natively only when it leverages SAS/LBI** (zero-copy IPC, type-isolation, never-die, capability model). The wider software ecosystem is **not** ported into the native/kernel layer — it runs in a **Tier 3 Linux VM** (`apk add` today on Alpine; broader package-manager coverage remains platform work), except a narrow set of trusted libraries linked into Tier 1 runtime profiles (crypto, codec, libm, sensor protocols). This keeps Cellos's identity intact and avoids re-implementing what Linux already does well.

Routing any new idea: (1) uses SAS/LBI → **Tier 1 native**; (2) trusted library a native Cell needs → **Tier 1 `ffi-posix` profile — port the library, not the feature**; (3) untrusted native code without source disclosure → **Tier 2 native domain once implemented**; (4) general Linux app / fork-based / POSIX stack → **Tier 3 VM**; (5) replicates Linux into native or erodes SAS/LBI → **reject**. Validated repeatedly: server cluster ("don't clone CNCF, Cellos is a great *node*"), nginx/postgres/CPython (Tier 3), mTLS/X.509 (Tier-3/interop, never PKI in kernel), Noise kept SAS-native, MicroPython dropped.

---

## System Layers

```
┌─────────────────────────────────────────┐
│  Cells (Applications, Drivers, Services) │  Apps: hello, shell, robot-dashboard, doom
├──────────────────────────────────────────┤  Drivers: disk, gpu, input, net, e1000, nvme, serial (Driver Cells)
│  Kernel (responsibility-bounded TCB)    │  Services: vfs, config, compositor, net, hypervisor, silo, power
├──────────────────────────────────────────┤
│  HAL (Hardware Abstraction Layer)        │  RV64 ✅, AArch64 ✅ (Ring-3), x86_64 ✅ (Ring-3)
├──────────────────────────────────────────┤
│  Hardware (QEMU, Bare-metal)             │  Memory, CPU, Devices
└─────────────────────────────────────────┘
```

### RV64 Desktop Input, Decoration, and Scanout

The input service owns device translation and sends pointer frames only to the
compositor. The compositor owns cursor state, surface hit-testing, z-order,
keyboard focus, and mode-tagged left-button capture. Interactive content clicks
select, raise, and forward surface-local input; compositor-owned titlebars,
frame edges/corners, and controls consume their own input. A titlebar drag
relocates its content rectangle, while an edge/corner drag proposes a bounded
`WindowConfigure`.

Each interactive `ViSurface` can set a bounded title and polls typed
`SurfaceEvent::{Configure,CloseRequest,StateChanged}` frames alongside normal
forwarded input. `apply_configure` allocates and attaches a replacement Grant,
acknowledges the matching serial, swaps local dimensions only on success, and
lets the compositor commit the staged content rectangle atomically. The
compositor owns all frame/title/control pixels outside client coordinates and
reblends old and new decoration bounds.

Minimized surfaces are excluded from paint and hit testing until restored.
Maximize and restore use the same configure transaction. A close control first
requests an owner decision; rejection restores visibility, while acceptance
removes the surface when the owner destroys it. `SurfaceRole::Background`
surfaces remain visible but cannot hit-test, raise, or acquire decoration
controls. There is intentionally no taskbar, snapping, or live stretched
resize preview.

---

## Board Descriptors (`boards/`)

`boards/` is a root workspace crate, not a HAL subdirectory. It holds immutable board descriptors for product-specific integration data and keeps hardware mechanism code elsewhere.

- `cellos-boards` is `#![no_std]` and carries descriptor data only.
- Each descriptor owns board identity, compatible strings, boot/firmware contract,
  fallback memory map/DT asset, pinmux/PHY wiring, typed SoC identity, and the
  list of shared drivers to enable. It contains no SoC MMIO or IRQ layout.
- The catalog covers QEMU virt RV64/AArch64, VisionFive 2, Milk-V Pioneer,
  Raspberry Pi 3/4, and QEMU q35 x86_64. Checked DTS assets are
  audit/fallback data; x86 instead consumes Limine's memory map and validated
  ACPI. `boards/qemu/q35-x86_32`, `boards/qemu/virt-riscv32`, and
  `boards/qemu/virt-aarch32` are documentation-only and not part of the
  catalog. Physical-board descriptors remain compile-only without matching runs.
- Kernel consumers are `kernel/src/board.rs`, `kernel/src/boot.rs`, and `kernel/src/platform.rs`.
- Shared drivers remain in `cells/drivers/`; no UART/SDHCI/DW I2C/SPI/GIC/PLIC/PCIe driver is duplicated per board.
- `hal/soc/riscv` owns validated RISC-V fallback MMIO and discovery policy.
  Generic QEMU virt enables its audited UART/PLIC/CLINT/RTC/VirtIO fallback;
  JH7110 requires firmware DTB nodes for enabled drivers and exposes no RTC or
  VirtIO driver; SG2042 preserves SBI DBCN-only console and no RTC/VirtIO MMIO.
- The RV64 PLIC split now keeps policy data in `hal/soc/riscv` and runtime
  mechanism in `hal/arch/riscv`: the kernel resolves the physical-hart context
  from the selected SoC profile and device IRQs from `PlatformInfo`, and the
  shared PLIC driver no longer hardcodes QEMU context or enable-range assumptions.
- `hal/soc/bcm27xx` owns immutable BCM2837 peripheral/GPIO/AUX/Arasan layout and
  SDHCI word-access/write-spacing policy. It also owns exact peripheral and
  local-controller spans, system-timer and legacy-IRQ bases, plus GPIO/AUX grant
  widths and the immutable legacy/local IRQ topology. RPi3 paging, resource
  allowlisting, platform, GPIO IRQ, MMC, ARM HAL, and kernel IRQ diagnostics use
  those facts without changing page permissions, register offsets, public IRQ
  contracts, timer policy, pinmux selection, diagnostic output, or shared
  mechanisms.
- RPi3 kernel diagnostics reuse ARM HAL's FIFO-safe mini-UART byte writer; the
  kernel does not maintain a second LSR/IO polling and write implementation.
- `hal/soc/arm-virt` owns QEMU AArch64 platform layout and IRQ facts. PL011,
  GIC, paging, VirtIO, PCIe, RTC, GPIO, and resource consumers share those facts
  without gaining board-specific mechanism copies.
- `hal/soc/bcm27xx` also owns BCM2711 layout and grant widths. RPi4 exposes only
  disjoint GPIO/UART/SDHCI pages to cells and keeps GIC mappings kernel-only.
  PCIe is intentionally absent from its enabled-driver list until a BCM2711
  host-controller path is implemented.
- BCM2837 BSC1 and SPI0 use shared Driver Cell crates with distinct manifest,
  policy, and MMIO device classes. The RPi3 descriptor selects their pinmux and
  exact 4-KiB controller windows; GPIO authority cannot claim either window.
  The current RPi3 head now passes the wired physical I2C/SPI run, while the
  separate Phase 04 RPi3 boot/storage/input baseline passes TFTP, SDHCI/mount,
  shell, interactive help, and a lossless 100-command UART burst. DesignWare
  controllers stay conditional on verified compatible/controller evidence.
- Phase 04 boot evidence uses freshly packaged RV64/AArch64 images and asserts
  separate block, input, GPU, and shell markers. Both QEMU architectures also
  reach the shell with optional GPU/NIC devices omitted. This proves VirtIO
  fallback behavior separately; the matching RPi3 SDHCI and UART/input physical
  gates now pass on the current head.
- `hal/soc/x86` owns static PC-compatible COM1/ISA wiring and bounded legacy
  BIOS/RSDP windows. The kernel selects that profile before early serial output;
  validated ACPI remains the only source for LAPIC, IOAPIC, HPET, and PCIe ECAM
  addresses, so every downstream timer/interrupt/PCIe gate still fails closed.
  The Phase 05 q35 flow scans PCIe ECAM on bus 0, registers BAR windows through
  the resource registry, and keeps VT-d board-gated at the q35 fixed base before
  any DMA-capable Driver Cell starts. That evidence is QEMU-only; physical x86
  remains gated, bus > 0 is not yet validated, and real NIC Tx/Rx/DHCP is not
  proven.
- The shared SDHCI controller receives an immutable runtime access policy;
  BCM2837 word-only/spaced writes, BCM2711 native access, and JH7110 native
  access do not create per-board driver implementations.
- `scripts/check-hal-boundaries.sh` rejects SoC imports/MMIO fields or shared
  driver copies in board packages. `scripts/check-board-configs.sh` validates
  all seven assets/build commands, compiles every selection, and proves conflicting
  real-board features fail. CI runs this matrix on every change.

### RPi3 VideoCore HDMI Boundary

RPi3 selects the BCM display Driver Cell; generic AArch64 continues to select
the unchanged VirtIO GPU path. The BCM mailbox owns one persistent grant-backed
DMA page for its cell lifetime. `GrantCacheSyncBegin` validates owner and exact
bounds, pins the range, performs Point-of-Coherency maintenance, and returns an
operation token. `GrantCacheSyncComplete` accepts only that owner/token pair,
invalidates before CPU parsing, and releases only the matching pin. Any
post-submit uncertainty poisons the transport; task teardown quarantines a
still-pinned page until reboot. No generic cache-maintenance interface is
exposed.

`RegisterDisplayFramebuffer` requires both `DEV_DISPLAY` and ownership of the
BCM mailbox window. The kernel validates the converted ARM base, alignment,
reserved range, size, geometry, pitch, and byte coverage before installing the
bounded USER|DEVICE mapping and publishing resolution. This is a trusted
shared-SAS/LBI boundary, not owner-private page-table isolation; the firmware
framebuffer allocation therefore remains boot-lifetime. Scanout flush IPC is
accepted only from kernel sender TID 0, then clipped to the registered
pitch/range and written with device-visible volatile stores.

The completed run on revision `a22082` / Raspberry Pi 3 Model B / serial
`000000003d042795` establishes exact-device development evidence only. Its
mapping to the two owner-reported current Model B+ boards is unresolved. The
TFTP transfer record, later UART boot block, and user visual observation are
retained as separate evidence sources. They do not establish production
qualification or isolate the earlier late-connect `No Signal` root cause.

---

## Kernel (nano-kernel)

> **Size evidence:** [code-metrics.generated.md](code-metrics.generated.md) owns the moving
> `kernel/src` nLOC total and the narrower boundary-migration lens. The historical frozen totals
> and `<10K`/`≤5K` targets are withdrawn; the kernel is bounded by responsibility under Spec 15,
> with generated trend evidence preventing size claims from drifting.

The kernel is **tiny** by design, handling only:

### 1. **Boot & Initialization** (`kernel/src/boot.rs`)
- Limine bootloader integration (fallback: SimpleBootInfo)
- Resolve the effective firmware DTB once for CPU, platform, and fallback boot discovery
- On RV64 direct OpenSBI boot, derive a non-overlapping memory map from enabled DTB memory
  nodes while excluding firmware, the live kernel, `/memreserve/`, and static
  `/reserved-memory`; malformed or unsupported maps fail closed to audited board defaults
- Initialize UART for logging
- Initialize HAL (interrupts, paging)
- Set up frame allocator
- Initialize memory (paging, heap)
- Initialize scheduler
- Spawn init Cell
- Enable interrupts and enter idle loop

### 2. **Memory Management** (`kernel/src/memory/`)

**Frame Allocator**:
- Bitmap-based allocation (O(1) free, O(n) scan for allocate)
- RV64 capacity follows the firmware DTB rather than a fixed 190 MiB usable window
- Selects the largest page-aligned usable interval; multi-region allocation is not implemented
- Tracks allocated vs. free pages (4KB each) with an exact counter updated only on bitmap
  transitions, so repeated reservation and double-free paths do not skew the snapshot
- Exposes aggregate telemetry through opt-in `MemInfo=243` (allowlist bit 56), returning the
  fixed 32-byte `ViMemInfoV1`; it reveals no addresses or per-cell ownership

**Virtual Memory (SV39 on RV64)**:
- **Trap Zone**: Low 4KB, unmapped → catches NULL deref
- **User VA**: < 0x8000_0000 (per-task isolation via page tables)
- **Guard Hole**: 0x8000_0000–0x8020_0000 (unmapped, prevents overflow)
- **Kernel VA**: 0x8020_0000+ (identity-mapped)
- **Heap**: 64 MB kernel heap (linked-list allocator)

**Paging Structure** (RV39):
```
User Space: 1 GB (virt addr < 0x8000_0000)
├─ Stack: top of user VA (grows down)
├─ Heap: dynamic (grows up)
└─ Code/Data: loaded from ELF

Kernel Space: (virt addr 0x8020_0000+)
├─ Code: kernel binary
├─ Data: statics, globals
├─ Heap: kernel allocations
└─ Page Tables: per-task
```

**Stack Contract**:
- `Stack` records `base`, `pages`, `guard_pages`, and `top`.
- Every kernel and user stack reserves two verified-unmapped bottom guards; usable pages stay 64 for the current stack policy.
- Scheduler zeroing, watermarking, and quota charging derive from the `Stack` fields (`usable_start()`, `usable_bytes()`, `allocated_bytes()`), not from recomputed page constants.
- Allocation is fail-closed: if a guard cannot be mapped, unmapped, or verified absent, the allocator releases the full contiguous run and rejects the spawn.
- RV64 test-hooks deliberately spawn a U-mode probe whose first instruction stores `zero` at `usable_start() - 8`; the resulting store-page fault kills only that probe while the VFS boot path continues.

### 3. **Task Scheduler** (`kernel/src/task/scheduler.rs`)

**Fixed-Priority Scheduler**:
- Three public tiers: `Background < Normal < RealTime`
- FIFO within each tier
- RT-hart routing when the dedicated RV64 hart is online
- RV64 software-interrupt preemption path for higher-priority wakeups

> **Roadmap**: Fixed-priority scheduling is shipped. The consolidated latency baseline is still pending, and the immediate-preemption path remains architecture-scoped (RV64 only).

**Task Control Block (TCB)**:
```rust
struct Task {
    id: TaskId,
    state: TaskState,          // Running, Ready, Blocked, Dead
    cpu_context: TrapFrame,    // Registers, PC, SP
    page_table: PageTable,     // Task's virtual memory
    parent: TaskId,            // Parent Cell for tracking
    ipc_queue: Queue<Message>, // Incoming IPC messages
    grants: Vec<Grant>,        // Capability objects
}
```

**States**:
- `Running` — executing on CPU
- `Ready` — waiting for CPU
- `Blocked` — waiting for IPC message or I/O
- `Dead` — finished, pending cleanup

### 4. **IPC System** (`kernel/src/task/ipc.rs`)

11 core syscalls (vs. Linux's 300+):

> **Implementation Note**: The current implementation uses kernel-mediated syscall
> message passing. A trusted-cell direct-vtable fast path remains planned, but it must
> use explicit typed ownership/lifetime authorities; there is no monolithic Metadata
> Registry or generic linker-time pointer scanner.
>
> **Normative wire contract**: the actual kernel-mediated framing is now ratified in **[`docs/specs/17-ipc-wire-contract.md`](specs/17-ipc-wire-contract.md)** (Ratified 2026-07-07, normative for all cells). It fixes the recurring silent-failure class from ad-hoc per-service framing: `sys_recv(mask, buf)` returns the **sender tid** not a byte count (`mask == 0` wildcard, `mask == tid` filters one sender); the **byte-0 discriminant registry** and message framing/buffer-size rules are defined there. Any new IPC path MUST comply.
>
> **Suspended-receiver ownership**: a matched send never writes through another task's retained
> `Recv` pointer. The kernel queues owned bytes in the target mailbox, wakes the task, and the
> resumed receiver performs the validated copy. IRQ-sized payloads remain inline; larger payloads
> are allocated fallibly and charged/refunded to the receiver Cell.
>
> **Dead-peer wake contract**: when a target dies, blocked senders are marked Ready with a
> caller-visible error, their trap return is set to the error value, and the scheduler requeues
> them; the next send clears stale reply state before it parks again. Boot guardrails keep
> `RecvScatter` mailbox-only rather than CQ-backed until the broader reactor work is actually
> built.

> **Hot-swap cutover contract**: `PauseService` is the soft quiesce barrier and hides the
> current provider from new lookups while leaving it runnable for Snapshot IPC. The `hotswap`
> CLI reaches the Supervisor over IPC; the legacy syscall 400 path is retired/reserved and
> decodes `Unknown`. `SpawnReplacement` binds the new task to the frozen source tid, and
> `ResumeCell` becomes the atomic compare-and-commit barrier when `source_tid != 0`: it
> publishes the replacement, closes the old ingress, and preserves rollback if the compare or
> capacity preflight fails. Plain `ResumeCell` (`source_tid == 0`) is just an unfreeze and must
> not publish a replacement.

> **Snapshot trigger contract**: `Snapshot` remains syscall `420` with allowlist bit `32`;
> `HotSwapReady` keeps the same bit-32 compatibility bucket after retiring opcode 400.
> The shell snapshot client sends an opcode-only request inside a full zero-filled App IPC
> buffer to the Supervisor; the Supervisor authenticates the exact `shell` sender before
> parsing, accepts only opcode-plus-zero padding, and returns a bounded 3-byte status reply.
> Kernel dispatch still requires `SupervisorCap` before `serialize_snapshot()` runs, so an
> allowlisted but non-supervisor caller is denied. QEMU proves the two failure modes honestly:
> `NullBlock` reports snapshot unavailability on the emulated path, and real MMC save/restore
> remains host-gated. The snapshot format, kernel serializer, and warm-boot restore code are
> otherwise unchanged.

> **Grant lookup/lease linearization**: `GrantSlice` resolves a PAGE or REG
> grant and publishes the exact VFS lease while the matching grant-table lock is
> still held. This is the linearization point against `GrantFree` and
> `GrantUnregister`: teardown checks the pin registry and removes the entry in
> the same table-lock critical section, so lookup cannot win and then expose
> recycled frames before its lease exists. Owner death quarantines leased frames
> until the matching holder/request release. VFS copies only through the safe
> bounded OSTD adapter and requires the exact requested count before backend
> mutation. This ABI-stable correction closes the former High CWE-416
> lookup-to-pin race without a wire or syscall change.

| Syscall | Purpose |
|---------|---------|
| `Send(to, msg, cap)` | Send message to Cell, optionally grant capability |
| `Recv(from_filter, timeout)` | Receive message (blocks if none) |
| `Call(to, msg, cap)` | Send + wait for reply (RPC) |
| `Reply(to, msg)` | Reply to caller |
| `Spawn(binary, argv)` | Create new Cell |
| `Exec(binary, argv)` | Replace self with new Cell |
| `SpawnFromMem(ptr, size)` | Load Cell from memory buffer; no active launch-profile route |
| `MemInfo(out, len)` | Opt-in aggregate frame totals (`ViMemInfoV1`, 32 bytes) |
| `Snapshot()` | Serialize allocated physical frames to the P3 snapshot region; `SupervisorCap`-gated |
| `Exit(code)` | Terminate self |
| `Yield()` | Voluntarily yield CPU |
| `Log(msg)` | Print to kernel log |

Cell-spawn allocation exhaustion is encoded additively as `-2` for the four cell-spawn paths and
decoded as `SyscallError::OutOfMemory`. Generic syscall errors retain the legacy `-1` sentinel.
Source-stage and bounded caller/path logs make exhaustion diagnosable without panicking the kernel.

The current RV64 benchmark measures allocator commitment directly: **135,782,400 bytes
(129.49 MiB)** on 2026-08-01. This exceeds the unchanged `<10 MiB` performance objective; the
measurement mechanism is complete, while memory reduction remains separate work. The destructive
capacity probe is excluded from default images and enabled only with
`CELLOS_INCLUDE_CAPACITY_PROBE=1` for test-mode builds.

**Capability-Based Access Control**:
```rust
pub struct Capability {
    rights: u32,  // Read, Write, Execute, etc.
    target: CellId,
}

pub struct Grant {
    cap: Capability,
    from_cell: CellId,
    to_cell: CellId,
    // Revoked on drop
}
```

### 5. **ELF Loader** (`kernel/src/loader.rs`)

- Parse ELF header
- Load segments (allocate frames, map to vaddr)
- Apply relocations (position-independent code)
- Set up stack, heap pointers
- Enter user-space at `_start`
- `/bin/vfs` is only admitted if the manifest request, boot ceiling, and signed
  operator policy all preserve the cell-store bit (`block_regions=0b1111`);
  the loader fails closed and tears down the spawn instead of adding authority
  after policy.
- `SpawnFromPath`, `SpawnFromElf`, and `SpawnPinned` are authorized by exact
  launch-profile rows in `kernel/src/loader/launch_profile`; `SpawnFromMem`
  has no active profile and remains fail-closed for shell/user cells.
- Manifest classification is a pre-task tri-state: `Absent` selects only the
  explicit legacy path policy, `Valid` carries an exact v1/v2 record, and
  `Malformed` is audited and denied before scheduler state changes.
- `tools/check_elf.py` is a strict, read-only structural inspector. Its
  `Execution tier`, `Runtime profile`, `Protection class`, `Capabilities`, and
  `Evidence` lines deliberately keep product/runtime policy separate from
  manifest assertions; they are not signature or runtime-measurement proof.
- The loader is **not production-ready** despite completed Manifest-v2 tooling:
  `CELLOS-LOADER-SIG-001`, `CELLOS-LOADER-RACE-002`, and
  `CELLOS-LOADER-CLEANUP-003` remain open under the Phase 03/07 owners recorded
  in the [open-risk register](roadmap/open-risk-register.md).

### 6. **Filesystem (FAT32)** (`kernel/src/fs/`)

- Read-only FAT32 parser for boot
- Contains: `/bin/shell`, `/bin/hello`, `/bin/lua`, `/bin/cat`, `/bin/ls`
- Kernel uses this to spawn init Cell

---

## Hardware Abstraction Layer (HAL)

### Traits (Pure Interfaces)

```rust
// hal/traits/arch/lib.rs
pub trait Arch {
    fn init();
    fn switch_context(old: &TrapFrame, new: &TrapFrame);
    fn enable_interrupts();
    fn disable_interrupts();
}

// hal/traits/paging/lib.rs
pub trait PageTableTrait {
    fn map(&mut self, va: VAddr, pa: PAddr, flags: u32);
    fn unmap(&mut self, va: VAddr);
    fn translate(&self, va: VAddr) -> Option<PAddr>;
}

// hal/traits/interrupt/lib.rs
pub trait InterruptController {
    fn init();
    fn enable_irq(irq: u32);
    fn disable_irq(irq: u32);
    fn ack_irq(irq: u32);
}
```

### Implementations

Board policy stays out of HAL mechanism code. `boards/` carries product
descriptors, `hal/soc/riscv` carries immutable SoC profile facts, and the kernel
composes both before boot/platform fallback decisions. RV64 features select one
descriptor/SoC pair; required-DTB boards stop on absent or invalid firmware data.
Shared drivers remain in `cells/drivers/`; AArch64 SoC facts and SDHCI policy
remain for later migration slices.

**RISC-V 64-bit (RV64) — Production boot PASS** ✅
- `hal/soc/riscv` — Data-only SoC profiles for compatible strings and access
  policies (`GENERIC_VIRT`, `JH7110`, `SG2042`)
- `hal/arch/riscv/src/rv64/context.rs` — Trap frame, context switch
- `hal/arch/riscv/src/rv64/paging.rs` — SV39 page table walker
- `hal/arch/riscv/src/rv64/trap.rs` — Exception/interrupt handler
- `hal/arch/riscv/src/rv64/boot.rs` — Assembly entry (_start, trap setup)
- `hal/arch/riscv/src/common/uart_ns16550a.rs` — Serial UART
- `hal/arch/riscv/src/common/sbi.rs` — SBI calls (shutdown, time)
- `hal/arch/riscv/src/common/timer.rs` — SBI timer (scheduling)

**ARM AArch64 — Production boot PASS** ✅
**x86_64 — Production boot PASS** ✅
- PCIe-only x86 platforms enumerate no VirtIO-MMIO slots; the boot path skips that discovery to avoid a platform panic.
**RV32, AArch32 — TRAIT STUBS** (trait impls only, no boot code)

### Multi-Architecture Strategy

Use `#[cfg(target_arch = "riscv64")]` to conditionally compile:

```rust
#[cfg(target_arch = "riscv64")]
mod riscv;

#[cfg(target_arch = "arm")]
mod arm;

pub use crate::riscv::*;  // Or arm::* depending on build
```

---

## VirtIO Device Integration

### MMIO Memory Mapping

**Problem**: Limine bootloader does not report MMIO ranges in its memory map, causing device registers to become inaccessible after kernel paging is activated.

**Solution**: Explicit identity-mapping in `kernel/src/memory/paging.rs::init_kernel_paging()`:

```rust
// QEMU virt machine MMIO layout (RV64)
// CLINT (Core Local INTerrupt)
map(VAddr(0x0200_0000), PAddr(0x0200_0000), 0x10000, READABLE | WRITABLE | VALID);

// PLIC (Platform Level Interrupt Controller)
map(VAddr(0x0C00_0000), PAddr(0x0C00_0000), 0x0400_0000, READABLE | WRITABLE | VALID);

// UART0 + VirtIO MMIO devices (slot 0–7)
map(VAddr(0x1000_0000), PAddr(0x1000_0000), 0x0001_0000, READABLE | WRITABLE | VALID);
```

All MMIO regions are identity-mapped (VA = PA) for simplicity and to preserve bootloader-assigned addresses.

### VirtIO IRQ Dispatch Pattern

VirtIO devices on QEMU `virt` machine use PLIC IRQs with slot-based numbering:

| Device | MMIO Slot | Base Address | IRQ |
|--------|-----------|--------------|-----|
| UART0  | —         | 0x1000_0000  | 10  |
| VirtIO Block | 0 | 0x1000_1000 | 1 |
| VirtIO Input | 1 | 0x1000_2000 | 2 |
| VirtIO Net | 2 | 0x1000_3000 | 3 |
| ... | i | 0x1000_(i+1)000 | i+1 |

**IRQ Dispatch**: `kernel/src/task/drivers/virtio_common.rs::vi_handle_virtio_irq(irq: u32)` — a single generic router (no per-device arms). It signals whichever Driver Cell registered for that IRQ via `sys_wait_irq`; the kernel drives no VirtIO device logic itself.

```rust
#[no_mangle]
pub extern "Rust" fn vi_handle_virtio_irq(irq: u32) {
    // A Driver Cell registered for this IRQ via sys_wait_irq — signal it
    // (sets IRQ_PENDING + writes VirtIO InterruptACK) and return.
    if irq_wait::has_waiter(irq as u8) { irq_wait::signal_irq(irq as u8); return; }
    // Input slot: ACK to prevent an interrupt storm before the input Cell is up.
    if input_irq_ack::ack_if_input(irq) { return; }
    log::warn!("[virtio] unhandled IRQ {} — no registered device for this slot", irq);
}
```

**Kernel IRQ Handler Responsibilities** (Phase 05 established):
1. Receive IRQ from PLIC
2. Invoke Device Handler's `ack_irq(irq)` to clear device `InterruptStatus` register
3. Wake the corresponding Driver Cell task if blocked on I/O
4. Driver Cell (userspace) handles the rest: drain rings, process data, refill available ring

**Note**: As of Phase 01 (2026-06-24, Kernel Boundary Law enforcement), device logic lives in Driver Cells (`cells/drivers/`), not kernel code. The kernel only handles interrupt dispatch and wakeup.

### x86 VMM Backend Lifecycle

The x86 VirtIO block and network models resolve supervised VFS/Net generations
through the service registry. Backend IPC uses bounded send admission and
receive deadlines. A receive timeout poisons that service generation until the
registry publishes a different TID, preventing an uncorrelated late reply from
being consumed by a later request. A dead generation therefore produces block
IOERR or network unavailability rather than blocking the VMM. Block recovery
reopens `/mnt/sd/guest_disk.img` and accepts the new handle only when its size
matches the retained device capacity. Network recovery remains pending until the new
generation acknowledges a transmitted frame with `NetResponse::Ok`.

The `hostile-backend-recovery` feature packages a permanent supervisor and
exposes a test-only MMIO control register that terminates one backend generation.
Production builds omit this control surface. The x86 hostile runner requires
independent disconnect markers, new-generation recovery markers, persisted
block readback, acknowledged network TX, and the corresponding ARP RX.

**Driver residency (2026-07-07, post G2 loader redesign)** — migrated to Driver Cells: **virtio_blk, virtio_net, virtio_gpu, virtio_input, virtio_sound, e1000, nvme**. The kernel drives **no block device** — `virtio_blk.rs` + `virtio_pci.rs` were deleted; `cells/drivers/virtio-blk/` owns the disk and serves `service::BLOCK_DRIVER` (bootstrap cells load from the VIFS1 RAM ramdisk, so no block device is needed before the first Cell exists; see the changelog entry + `docs/specs/15-kernel-boundary.md`). The kernel retains only:
- **`mmc`** — descoped G2 (no SDHCI on QEMU to validate against; genuine tech debt).
- **IOMMU init + `map_dma_for_cell`** — whitelisted: the only hardware boundary between Driver Cells and physical memory in a Single Address Space.
- **`NullBlock` fallback** — a stub block device so boot-time reads (snapshot restore, `verify_mbr`) degrade gracefully on QEMU when no block Cell has claimed the device yet; real MMC save/restore remains host-gated on boards.

**Interrupt Flow (Correct Pattern)**:
```
Device generates interrupt
  ↓
PLIC sets bit in Pending register
  ↓
PLIC delivers IRQ to CPU
  ↓
RV64 trap handler calls vi_handle_riscv_external_irq(irq)
  ↓
Kernel runtime router selects UART, VirtIO, or unknown handling from PlatformInfo
  ↓
Device handler:
  - Process available data/requests
  - Call ack_irq(irq) to clear InterruptStatus
  - Refill available ring
  ↓
PLIC acknowledges via plic_complete()
  ↓
Device can fire next interrupt (if new data arrives)
```

**Critical Fix (Phase 05)**: Input device was not calling `ack_irq()`, leaving `InterruptStatus` register set. PLIC would immediately re-fire the same interrupt after `plic_complete()`, creating an infinite interrupt storm. This caused kernel to hang on first keystroke. Fix: Ensured input Driver Cell calls `ack_irq()` on every interrupt; kernel dispatcher invokes the handler. *(Later refactored to Driver Cell architecture in Phase 01, 2026-06-24.)*

### Tier 3 VirtIO-GPU Host Stack (legacy: Tier 3b)

- `cells/services/hypervisor/src/virtio_gpu.rs` implements the VirtIO-GPU 2D device model (DeviceID 16, MMIO slot 3, SPI 19) and wires the control/cursor queues into the VMM IRQ path.
- `cells/services/hypervisor/src/virtio_gpu/resource/{control,render}.rs` owns resource creation, scanout binding, cursor redraw, and the VMM-owned scanout Grant.
- `cells/services/hypervisor/src/virtio_gpu/scanout.rs` performs Grant sharing, compositor attach/damage, reconnect, and deferred teardown when the compositor owner disappears.
- `cells/tools/init/src/main.rs` and the supervisor start the physical GPU Driver Cell, compositor, and hypervisor in dependency order; `scripts/fetch-alpine-artifacts.sh` and `scripts/make-hypervisor-fs.sh` build the pinned ARM64 Alpine guest image.
- `tests/integration/tests/tier3b-virtio-gpu.rs` is the strict, ignored-by-default guest lane; it requires both `TIER3B_GPU_E2E=1` and an explicit `--ignored` run on ARM64 KVM or real hardware, not nested Windows QEMU-TCG.

### FAT16 Persistence & Graceful Shutdown (Phase E)

**Hardening** (safety fixes, no behavior change):
- `cells/services/vfs/src/block_stream.rs` — SeekFrom::Current now validates result ≥ 0 before u64 cast (prevents underflow→seek to arbitrary LBA)
- `kernel/src/task/syscall.rs` — BlkRead/BlkWrite now reject sectors ≥ CELL_TABLE_BASE_LBA (82,000) to prevent cell from corrupting kernel bootstrap table

**Clean Shutdown Path**:
- Syscall 502 (raw, no `ViSyscall` enum entry) — kernel SBI SRST handler calls OpenSBI to power off
- `cells/apps/shell/src/cmd_sys.rs` — `shutdown` built-in command triggers graceful QEMU exit
- Test harness `wait_for_natural_exit()` allows disk image to flush before reboot

**Integration Test** (`vfs_fat16_reboot_persistence`):
- Writes marker to FAT16 `/data/`, issues shutdown, waits for QEMU clean exit
- Reboots against same disk image, reads marker back to prove write durability across power cycle
- **Critical bug fixed during this phase**: `shell.rs` had pre-parser echo handler that split by whitespace, completely bypassing redirect parser. Removed handler; echo now correctly goes through parser and supports OP_WRITE redirects.

---

## Public API (Kernel-Cell Boundary)

Located in `libs/api/`, these traits define the stable ABI:

### Filesystem (`ViFileSystem`, `ViFile`)
```rust
pub trait ViFileSystem {
    async fn open(&self, path: &str, flags: u32) -> ViResult<Box<dyn ViFile>>;
    async fn read_dir(&self, path: &str) -> ViResult<Vec<DirEntry>>;
}

pub trait ViFile {
    async fn read(&mut self, buf: Box<[u8]>) -> ViResult<Box<[u8]>>;
    async fn write(&mut self, data: &[u8]) -> ViResult<usize>;
    async fn seek(&mut self, pos: u64) -> ViResult<u64>;
}

// IPC Opcodes (Phase F: FAT16 Hardening)
// OP_WRITE (0x04): [opcode][path_len:u8][content_len:u16 LE][path][content]
//   - Effective message cap: min(512, 4 + path_len + content_len) bytes
//   - /data/* → FAT16, /tmp/* → RamFS
// OP_UNLINK (0x07): [opcode][path_len:u8][path]
//   - /data/* → FAT16, /tmp/* → RamFS (nested paths supported)
// OP_MKDIR (0x05): [opcode][path_len:u8][path]
//   - /data/* → FAT16 mkdir -p, /tmp/* → RamFS (nested paths supported)
```

### Block Devices (`ViBlockDevice`)
```rust
pub trait ViBlockDevice {
    async fn read(&self, sector: u64, count: u32) -> ViResult<Box<[u8]>>;
    async fn write(&self, sector: u64, data: &[u8]) -> ViResult<u32>;
}
```

### Networking (`ViTcpStack`, `ViTcpStream`, Typed IPC, TLS)
```rust
pub trait ViTcpStack {
    async fn listen(&self, addr: &str, port: u16) -> ViResult<Box<dyn ViTcpListener>>;
    async fn connect(&self, addr: &str, port: u16) -> ViResult<Box<dyn ViTcpStream>>;
}

// Primary IPC Format (Phase 27 — Protocol Hardening)
// Net service now uses typed postcard IPC as primary wire format:
// - NetRequest enum: CreateSocket, Connect, Bind, Send, Recv, Close, Listen, Accept, TlsConnect, TlsSend, TlsRecv, GetSocketState, etc. (15 variants)
// - NetResponse enum: SocketCreated, Connected, Bound, DataSent, DataReceived, SocketClosed, etc.
// - All variants type-checked at kernel dispatch; prevents serialization bugs and type confusion

// TLS 1.3 Client (Phase TLS-01) — typed + raw-opcode fallback
// Typed path (primary):
//   - NetRequest::TlsConnect { host, port, hostname } → NetResponse::TlsConnected { cap_id }
//   - NetRequest::TlsSend { cap_id, data } → NetResponse::TlsDataSent { bytes_written }
//   - NetRequest::TlsRecv { cap_id, max_len } → NetResponse::TlsDataReceived { data }
//
// Raw fallback (legacy, for backward compatibility with ostd::tls helpers):
//   - TLS_CONNECT (0x30): [addr:4 LE][port:2 LE][hostname:*] → [cap_id:8 LE]
//   - TLS_SEND (0x31): [data:*] → [bytes_written:4 LE]
//   - TLS_RECV (0x32): [max_len:4 LE] → [decrypted_data:*]
```

### Drivers (`ViDriver`)
```rust
pub trait ViDriver {
    fn name(&self) -> &str;
    fn probe(&mut self) -> ViResult<()>;
    fn capabilities(&self) -> u32;
}
```

### Runtime (`ViVmRuntime`)
```rust
pub trait ViVmRuntime {
    fn load(&mut self, bytecode: &[u8]) -> ViResult<()>;
    fn execute(&mut self, function: &str, args: &[Value]) -> ViResult<Value>;
}
```

---

## Cells (User-Space Software)

### What is a Cell?

A **Cell** is an isolated execution context (like a process) but:
- Shares kernel's address space (no context-switch overhead)
- Cannot use `unsafe` code (Rust enforces this)
- Communicates via syscalls (IPC, filesystem, logging)
- Has its own task control block, page table, and message queue

### Cellos App SDK (L1 Platform Layer)

**Purpose**: Eliminate boilerplate and unlock real native applications without kernel expertise.

**Components** (`libs/ostd/`):
- **`CellRuntime` builder**: Unified app initialization — handles manifest generation, permission sets, lifecycle
- **`app_entry!` / `service_entry!` macros**: Declarative entry points (10–30 lines replaces 200+ lines of manual boilerplate)
- **Typed client facades**:
  - `VfsClient` — read_file, write_file, append_file, stat, list_dir, mkdir, unlink
  - `NetClient` — tcp_connect, tcp_send, tcp_recv, tcp_close, dns_lookup, local_ip
  - `InputClient` — request_focus, get_focus, clear_focus
- **Lifecycle support**: `ShutdownReason` enum, `ShutdownWith` event, `arm_heartbeat()`, `run_with_lifecycle()` for graceful shutdown
- **Lazy service accessors**: `app.vfs()`, `app.net()` resolve on first use

**Reference app** (`cells/apps/hello-cell/`):
```rust
use api::{app_entry, CellRuntime};

app_entry!(handler = run);

async fn run() {
    println!("Hello from Cellos App SDK!");
}
```

**Impact**: Apps no longer need to understand manifests, syscall allowlists, or raw IPC — all abstracted by the SDK. Foundation for L2 middleware (HTTP servers, databases, pub-sub), unblocking G2 real application development.

### Planned Tier 1 Rust `std` Profile

The feasibility decision conditionally selects an exact, no-fuzz,
content-addressed source overlay against a private checkout matching the pinned
Rust compiler. A later authorized implementation would add a real internal
Cellos PAL under `library/std`, select it through matching rustc target metadata,
and produce a private, provenance-bound sysroot. An external PAL plug-in,
target-OS impersonation, `std` over mlibc/POSIX, unsupported/fake `std`, and a
renamed `core` + `alloc` are rejected. Upstreaming is only a later exit path;
the decision does not authorize publishing a target or triple.

The current package contains contracts, inventories, and a fixture-only
benchmark validator, not a PAL implementation. The support map classifies all
36 hooks as 8 Supported, 10 Unsupported, and 18 Deferred. Blocking Deferred
rows include entropy (`PAL-019`) and the raw output-buffer boundary
(`PAL-031`). A later PAL/target/runtime child remains barred until those and
every other blocking row are implemented and evidenced, all six named human
approvals are granted, the implementation checkpoint passes, and umbrella
Phase 03 production gates are approved.

### Cell Types

**Tools**: System utilities & CLI applications
```
cells/tools/shell/     — Interactive REPL (parser, executor, aliases, jobs, history)
cells/tools/init/      — Bootstrap (spawns vfs, config, input, net, compositor, shell, robot-demo; games/demos run on-demand from shell)
cells/tools/sys-tools/ — Standalone binaries: ls, cat, echo, ps, kill (0x2A000000 VA base)
cells/tools/net-tools/ — Network utilities: ping, curl, wget, nc, httpd, mqtt (0x26000000 VA base)
```

**Applications**: User-facing applications
```
cells/apps/robot-dashboard/ — Reference G1 HMI dashboard (ViUI v2, 800×480, 0x0D000000 VA)
```

**Demos**: Hardware/feature demonstrations and graphical showcases
```
cells/demos/hello/           — Minimal test app
cells/demos/hello-cell/      — SDK reference (17-line zero-boilerplate app)
cells/demos/periph-demo/     — GPIO pin blink demo (QEMU ARM virt)
cells/demos/sensor-demo/     — I2C SHT3x temperature sensor (0x2E000000 VA)
cells/demos/spi-demo/        — SPI peripheral test (0x30000000 VA)
cells/demos/pwm-demo/        — PWM servo control
cells/demos/adc-demo/        — ADC analog input
cells/demos/can-demo/        — CAN bus messaging
cells/demos/robot-demo/      — End-to-end sensor→compute→actuator (GPIO ownership cycling, MQTT)
cells/demos/sdk-demo/        — Cellos App SDK patterns
cells/demos/https-demo/      — TLS 1.3 HTTPS client to example.com
cells/demos/viui-demo/       — ViUI v2 DSL → Rust codegen pipeline (Counter.vi)
cells/demos/audio-demo/      — VirtIO sound test tone (A4-C#5-E5 arpeggio, S16LE/2ch/44100)
cells/demos/doom/            — doomgeneric DOOM port (1024×768, 16MB quota, 0x42000000 VA); run: `doom`
cells/demos/tetris/          — Tetris in Rust-native Cell (ViUI)
cells/demos/tetris-c/        — Tetris via C platform hooks (demonstrates Tier 1 ffi-posix profile)
cells/demos/tetris-lua/      — Tetris scripted in Lua (demonstrates Tier 1 lua profile)
```

**Drivers**: Hardware device drivers
```
cells/drivers/disk/      — VirtIO block passthrough (✅ working)
cells/drivers/gpu/       — VirtIO GPU (opt-in framebuffer)
cells/drivers/input/     — VirtIO input passthrough (deprecated; kernel poll used)
cells/drivers/net/       — VirtIO NIC wrapper (deprecated; kernel poll used)
cells/drivers/gpio/      — PL061 GPIO driver (ARM64 QEMU virt)
cells/drivers/gpio-sifive/ — SiFive GPIO extension
cells/drivers/serial/    — PL011 UART driver (ARM64)
cells/drivers/i2c-gpio/  — BitBangI2c<G> generic over ViGpio
cells/drivers/spi-gpio/  — BitBangSpi<G> generic over ViGpio
cells/drivers/pwm-gpio/  — BitBangPwm<G> generic over ViGpio
cells/drivers/adc-sim/   — Simulated ADC (no MMIO)
cells/drivers/can-loopback/ — Loopback CAN (no MMIO)
```

**Services**: System services with long-lived state
```
cells/services/vfs/       — RamFS + FAT32 + littlefs + BootFS (✅ MountTable dispatch complete)
cells/services/config/    — Key-value store (✅ ViStateTransfer impl)
cells/services/compositor/  — Software blending + z-order + Grant surfaces
cells/services/input/     — Input event routing + focus system
cells/services/net/       — smoltcp TCP/IP + DHCP + TLS 1.3 (✅ typed postcard IPC)
cells/services/hypervisor/ — ARM64 EL2 VMM (Alpine Linux) (✅ minimal VMM)
cells/services/silo/      — KMS-internal development Silo (`test-hooks`, AArch64 QEMU `DEV_REFERENCE`; not production/hardware custody)
cells/services/httpd/     — HTTP web server (shell builtin)
cells/services/power/     — Power management (stub)
```

**Runtimes**: VMs/interpreters for scripting
```
cells/runtimes/lua/       — Lua 5.4 via FFI (⚠️ milestone marked complete but native runtime NOT actively maintained — roadmap §D; scripting/Python story is Tier 3 Linux VM, not a native runtime)
```

**Tests**: Integration & stress test cells
```
cells/tests/bench/           — RT + SMP latency benchmark (3 scenarios)
cells/tests/vfs-test/        — VFS service test suite (8 scenarios)
cells/tests/srv-test/        — Spawn + state transfer tests
cells/tests/hypervisor-test/ — Tier 3 VM lifecycle tests
cells/tests/gpio-test-rv/    — RISC-V GPIO integration
cells/tests/periph-test/     — Peripheral driver unit tests
cells/tests/posix-shim-test/ — POSIX stdio/math/setjmp tests
cells/tests/c-math-smoke/    — C runtime verification (12 scenarios, 3 arches)
cells/tests/mlibc-smoke/     — mlibc profile integration (Rust + libc.a)
cells/tests/zig-hello/       — Zig raw-syscall smoke test (no mlibc)
cells/tests/zig-mlibc-smoke/ — Zig mlibc profile smoke test (links mlibc libc.a)
cells/tests/input-test/      — Input service focus & event tests
cells/tests/silo-test/       — development containment probe (KMS denials/readiness; AArch64 QEMU `test-hooks`)
cells/tests/test-isolation/  — Cell fault isolation tests
```

**Guests**: Hypervisor guests (Tier 3)
```
cells/guests/silo-guest/  — locked AArch64 development P-256 guest (KMS purpose-bound `DEV_REFERENCE`, not a secure enclave)
```

**UI Library** (`libs/viui/`): no_std UI toolkit for GUI app Cells
```
libs/viui/             — ViUI toolkit (no_std + alloc, MIT)
  v1 (done):           Elm model, FramebufferCanvas, GlyphAtlas — foundation
  v2 (✅ shipped 2026-06-16, all 7 phases): Reactive Signal Tree + Dual-Layer DSL (see below)
```

---

<a id="viui-architecture"></a>
<a id="viui-architecture-g2-target"></a>

## ViUI Architecture (✅ shipped 2026-06-16)

ViUI v2 targets the constraints of Cellos's no_std Cell environment while matching the ergonomics of modern native UI toolkits. **All 7 phases are complete** (P01 Overlay Widgets · P02 Navigation · P03 Charts · P04 DSL build.rs · P05 Virtual ListView · P06 FlexBox · P07 Advanced Bindings). **Slint was rejected** for ViUI (GPL-3 viral / per-device commercial license unfit for an OS platform — `docs/specs/14-viui.md`, `06-graphics.md §39`); the `.vi` DSL is Slint-*compatible syntax* only, not a Slint dependency.

### Dual-Layer Design

```
┌────────────────────────────────────────────────────────┐
│  Layer 1 — .vi DSL  (Slint-compatible syntax)          │
│                                                        │
│  component Counter {                                   │
│      in-out property <int> count: 0;                   │
│      VerticalLayout {                                  │
│          Text { text: "Count: \{count}"; }             │
│          Button { text: "+1"; clicked => {count+=1;} } │
│      }                                                 │
│  }                                                     │
│                                                        │
│  vi-compiler (build.rs) → generates Layer 2 Rust code  │
│  Hot-reload: watcher daemon, no recompile needed       │
└────────────────────────────────────────────────────────┘
                         ↓ compiles to
┌────────────────────────────────────────────────────────┐
│  Layer 2 — Rust Signal API  (also direct public API)   │
│                                                        │
│  #[vi_component]                                       │
│  struct Counter { count: Signal<i32> }                 │
│                                                        │
│  impl ViComponent for Counter {                        │
│      fn view(&self) -> impl ViNode {                   │
│          vstack!(                                      │
│              label!(text: self.count                   │
│                  .map(|n| format!("Count: {n}"))),     │
│              button!(text: "Increment",                │
│                  on_click: || self.count               │
│                      .update(|n| n+1)),                │
│          )                                             │
│      }                                                 │
│  }                                                     │
└────────────────────────────────────────────────────────┘
```

**Key properties**:
- Layer 1 uses Slint expression language → zero migration cost from Slint
- Layer 2 uses Rust expressions → familiar to Rust devs, no DSL required
- Signal<T> reactive engine: only affected widgets repaint → no full-screen repaints
- ViRenderer trait: FramebufferCanvas (CPU, no GPU needed) or GPU backend (G2+)
- no_std + alloc throughout; no std dependency in runtime crates

`FramebufferRenderer` submits finite damage only after outward rounding and
clipping it to the current `ViSurface`; `None` remains the full-repaint path,
while empty or offscreen rectangles submit no damage. `ManagedSurfaceApp`
provides the compositor boundary for one `ViApp`: configure events apply the
replacement Grant and trigger relayout, minimized surfaces stop ticking until
restore, close requests follow an explicit accept/reject policy, and
`shutdown()` performs the normal surface-destruction sequence after an
accepted close. `cells/demos/viui-demo` exercises this boundary with the
generated Counter component and compositor-forwarded input. Pointer presses
also establish local ViUI widget focus, so Enter activation survives a
compositor maximize/restore cycle. This adds no `libs/api` or wire-protocol
change.

The dedicated RV64 QEMU oracle passes generated-label repaint, pointer input,
maximize/restore geometry, accepted close, and post-restore Enter activation.
The independent `window-policy` QEMU regression remains green. This is QEMU
software evidence only, not physical-board or production qualification.

### Reactive Update Model

```
Signal<count>.set(42)
    ↓
Notify subscriber widgets (only label in this example)
    ↓
Mark label's dirty_rect
    ↓
Repaint only label region (~80×16 px)
    ↓
surf.damage_rect(dirty)    ← NOT damage_all()
```

Contrast with ViUI v1 (Elm): every button click → rebuild all 20 widgets → layout all → repaint 307,200 px.

### Crate Layout

```
tools/vi-compiler/     (std, build tool)     — .vi parser, Slint expr evaluator, codegen
tools/viui-build/      (std, build-dep) ✅   — build.rs integration wrapper (P05 complete)
libs/viui-macros/      (proc_macro) ✅       — vi_design!{} for inline prototype use (P06 complete)
libs/viui-core/        (no_std + alloc)      — Signal<T>, LayoutNode, DirtyRect, ViRenderer trait
libs/viui-widgets/     (no_std + alloc)      — typed widget structs (Layer 2 API)
libs/viui/             (no_std, umbrella) ✅ — re-exports all above + viui_macros (P06 complete)
```

**P05 Build Integration** (2026-06-08): `tools/viui-build/` wraps vi-compiler; cells use `build.rs` → `viui_build::compile(glob)` → `include!()` generated Rust. Demo Cell (`cells/apps/viui-demo/`) validated end-to-end. Workspace `exclude` separates compiler from kernel/cells for independent versioning.

**P06 Proc Macro** (2026-06-08): `libs/viui-macros/` ships with `vi_design!` macro for inline component prototyping. `libs/viui` re-exports both paths (build.rs + macro); users import once, use both. Codegen redesigned to wrap each component in `mod __vi_generated_<Name>` to prevent symbol collisions.

The original ViUI design brief is retained in the local, gitignored `.agents/`
workspace; this section is the repository-owned architectural summary.

---

### Cell Lifecycle

```
1. Boot kernel
   ↓
2. Kernel spawns "init" Cell from embedded binary
   ↓
3. Init spawns "config" service (KV store)
   ↓
4. Init spawns "vfs" service (filesystem server)
   ↓
5. Init spawns "shell" application (interactive REPL)
   ↓
6. User types commands → shell sends IPC to vfs/config
   ↓
7. Shell displays output from services
   ↓
8. Ctrl+A X to shutdown
```

---

## Boot Sequence (Visual)

```
┌─────────────────────────────────────────────────┐
│ Bootloader (Limine or OpenSBI)                  │
│ Sets up: memory, DTB, argc/argv                 │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/boot.rs: kmain(hartid, dtb)          │
│ 1. Initialize UART for logging                  │
│ 2. Resolve DTB and build reservation-safe map   │
│ 3. Initialize HAL (traps, interrupt handler)    │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/main.rs: _km_start()                 │
│ 4. Frame allocator (bitmap)                     │
│ 5. Virtual memory (SV39 paging)                 │
│ 6. Heap allocator (64 MB)                       │
│ 7. PLIC (interrupt controller)                  │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ kernel/src/task.rs: init_scheduler()            │
│ 8. Task allocator (TCB pool)                    │
│ 9. Load "init" Cell from embedded FAT32         │
│ 10. Enter scheduler loop                        │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ cells/apps/init/src/main.rs: main()             │
│ 11. Spawn "config" service via syscall::spawn() │
│ 12. Spawn "vfs" service                         │
│ 13. Spawn "shell" application                   │
│ 14. Idle (let scheduler handle)                 │
└──────────────┬──────────────────────────────────┘
               ↓
┌─────────────────────────────────────────────────┐
│ cells/apps/shell/src/main.rs: main()            │
│ 15. Print prompt: "Cellosh> "                     │
│ 16. Read user input (async)                     │
│ 17. Parse command (echo, cat, ls, etc.)         │
│ 18. Send IPC to vfs/config services             │
│ 19. Display response                            │
│ 20. Loop to step 15                             │
└─────────────────────────────────────────────────┘
```

---

## Memory Layout (SV39 RV64)

```
Virtual Address Space (64-bit, SV39 = 39-bit VA)
┌───────────────────────────────────┐
│  User Space (< 0x8000_0000)       │  Per-task, isolated via page table
│  - Stack (top, grows down)        │
│  - Heap (dynamic, grows up)       │
│  - Code/Data (ELF loaded here)    │
└─────────────────────────────────────┘  0x7fff_ffff

┌───────────────────────────────────┐
│  Guard Hole (unmapped)            │  0x8020_0000 - 0x7fff_ffff
│  Prevents user/kernel overflow    │
└───────────────────────────────────┘  0x8020_0000

┌───────────────────────────────────┐
│  Kernel Space (≥ 0x8020_0000)     │  Identity-mapped, shared
│  - Code: kernel binary            │
│  - Data: statics, globals         │
│  - Heap: kernel allocator         │
│  - Page tables (per-task)         │
│  - Task pool (TCBs)               │
└───────────────────────────────────┘  0xffff_ffff_ffff_ffff

Physical RAM: firmware-described on RV64 direct OpenSBI boots; protected intervals are removed
before usable pages reach the frame allocator.
```

---

## IPC & Message Passing

### Send Message (Async)

```
┌────────────────────────────────────┐
│ Cell A (shell)                     │
│ syscall::send(vfs_id, msg, grant) │
│ (doesn't block, returns immediately)
└────────────────────┬───────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Validates msg │
            │ - Queues in VFS │
            │ - Wakes VFS     │
            └────────┬────────┘
                     ↓
            ┌─────────────────┐
            │ Cell B (vfs)    │
            │ woken by kernel │
            │ syscall::recv() │
            └─────────────────┘
```

### Call & Reply (RPC)

```
┌────────────────────────────────────┐
│ Cell A (shell)                     │
│ syscall::call(vfs_id, req, cap)   │
│ BLOCKS, waiting for reply          │
└────────────────────┬───────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Queues msg    │
            │ - Blocks Cell A │
            └────────┬────────┘
                     ↓
            ┌──────────────────────┐
            │ Cell B (vfs)         │
            │ syscall::recv()      │
            │ → gets request       │
            │ process...           │
            │ syscall::reply(A, rsp)
            └────────┬─────────────┘
                     ↓
            ┌─────────────────┐
            │ Kernel          │
            │ - Unblocks A    │
            │ - Delivers rsp  │
            └────────┬────────┘
                     ↓
            ┌──────────────────────┐
            │ Cell A resumes       │
            │ receives reply       │
            │ continues...         │
            └──────────────────────┘
```

---

## Security Model Implementation Status (2026-06-23)

### Hardware-Isolation Delivery Model (Spec 19)

```
Layer A — W^X after relocation      → code/constant integrity        [DONE]
Layer B — Per-domain page tables    → untrusted native-cell wall     [PLANNED]
Layer C — Per-arch hardening        → opportunistic MTE/MPK bonuses  [HW-GATED]
```

Layer A keeps the load -> relocate -> lower -> register ordering on every paged arch.
Its TLB closure is still arch-scoped: RV64 now orders PTE updates, local `sfence.vma`, and
SBI RFENCE before W^X return or reuse; QEMU 8.2/OpenSBI passed the two-hart physical-byte
oracle and negative control in five repeated iterations, while real RV64 hardware remains host-gated;
x86_64 is local `invlpg` pending SMP/IPI shootdown, and AArch64 already emits
inner-shareable `tlbi vaae1is` (plus `vae2is` when EL2 is active) bracketed by `dsb ishst` /
`dsb ish` / `isb`, but multi-PE runtime proof remains gated. The repo therefore does not
claim D7 complete.

LBI, CFI, DMA isolation, the KMS-internal development Silo containment lane,
and Tier 3 VM protection complement this delivery model but do not change its
Layer A/B/C ownership or turn MTE/MPK into a side-channel guarantee.

**Hardware Security Implementations by Architecture:**

| Component | ARM64 | x86_64 | RISC-V |
|-----------|-------|--------|--------|
| **CFI (Forward-edge)** | BTI+PAC ✅ | CET-IBT ✅ | Zicfilp (ratified, await silicon) |
| **Memory Tagging** | MTE implementation ✅; unavailable on RK3588 (A76/A55, Armv8.2-A) | N/A | Zimt (draft, await silicon) |
| **Domain Isolation** | Layer B planned | PKU plumbing only; PTE enforcement pending | PMP descriptors only; M-mode owner absent |
| **DMA Enforcement** | Per-Cell DDT ✅ | Per-Cell VT-d ✅ | Per-Cell IOMMU ✅ |

**Deployment Details:**
- **ARM64**: Compiler flags `-C target-feature=+bti,+paca,+pacg`; MTE is runtime-gated through `ID_AA64PFR1_EL1` and requires Armv8.5-A or later. RK3588's Cortex-A76/A55 cores expose the MTE field as zero.
- **x86_64**: CR4.CET + MSR_IA32_S_CET for CET-IBT; CR4.PKE and WRPKRU paths are feature-gated, but user PTEs remain key 0
- **RISC-V**: DMA isolation (3-level DDT, Sv39 domains) complete; PMP is inaccessible from the S-mode runtime without a custom M-mode firmware owner

**Known Limitations:**
- **MTE**: Probabilistic (1/16 tag collision), hardening only, and unavailable on the RK3588 deployment target; use only on QEMU or future Armv8.5+ hardware
- **PKU**: PTE key tagging (bits [62:59]) is absent; current PKRU switching cannot deny access while all pages remain key 0, and the self-test does not prove a keyed-page fault
- **RISC-V**: Zicfilp/Zimt/Smepmp extensions ratified 2024–2025 but no shipping silicon yet

---

## Cross-Machine Communication & Clustering (proposed contract — see Spec 20)

> Designed 2026-06-23. Stable summary: cross-machine IPC belongs in the userspace
> `net-broker`; the proposed contract is owned by [Spec 20](specs/20-unified-ipc-contract.md).
> Until Layer-3 generation exists, Spec 20 carries an explicitly transitional snapshot:
> broker boot and NodeId generation are wired, transport/relay modules compile, typed
> forwarding remains a stub, and no two-node runtime is proven. Once generated,
> `docs/spec-status.generated.md` is the sole owner of volatile implementation status.
> Research and historical design input remains in the local, gitignored
> `.agents/260623-remote-cell-ipc-research/` workspace.

### Foundational principle: LBI stops at the machine boundary

Language-Based Isolation is the Rust type system within **one** compiler's address space (SAS). It proves nothing about a remote machine. Therefore **every remote machine is untrusted**, cross-machine messages must be explicitly authenticated, and the kernel only ever sees *local* IPC. All cross-machine logic lives in a **userspace `net-broker` Cell** — zero kernel changes to the transport/auth substrate. Intra-machine zero-copy IPC (Grant) degrades to **one-copy** across machines; every other Cell guarantee (supervisor restart, capability gating, owned buffers) survives.

Phase 02A adds a boot-provisioned, non-secret export registry at `/etc/cellos/c2c-exports.cfg` inside `net-broker`. The registry is policy input, not a secret store: the broker can validate and count exported endpoints, but it still keeps remote/public delivery disabled unless the protected KMS identity is ready and every later transport/governance gate passes. Readable `/etc/cellos` state is never authorization.

Phase 02B uses the existing append-only KMS ABI for node identity. The live
broker registers with KMS and accepts an identity only when status and
acquisition agree on ready state, provider, binding epoch, blob revision, and
public key. `KmsBackedX25519` gives Clatter an opaque handle/epoch representation
and delegates static DH to KMS; the private scalar never enters broker or VFS
state. Plaintext VFS `machine-id` is not an identity root. KMS absence or any
mixed/non-ready snapshot selects an ephemeral local-only NodeId and keeps remote
disabled. This consumer wiring does not qualify a provider or prove two-node
transport.

Node-identity recovery is supervisor-only and compare-and-swap guarded. The
live attested supervisor must read an exact nonzero blob revision and submit an
auditable clone-recovery, lost-key-recovery, or operator-rekey reason; zero is
never a wildcard. Recovery first keeps remote disabled. A qualified provider
must atomically seal the replacement, advance the revision, revoke old handles,
and clear the broker binding before success. The broker then re-registers and
re-acquires, while changed peer/relay enrollment remains an independent gate.
Unavailable revision/root state stops at physical authority re-provisioning;
plaintext restoration is forbidden.

Phase 03 implements Candidate B without a kernel ABI change. The broker's main
task blocks in `sys_recv_attested`; fixed-capacity request, reply, in-flight,
and stale-history state binds work to the kernel-attested sender TID, Cell id,
and generation. Broker-owned monotonic request ids correlate replies, per-caller
windows bound fairness, full queues return `Busy`, and bounded reply retries
retain explicit ownership. Separate worker, reply-pump, and network-poller
roles prevent blocking local ingress from stopping network cadence. The
restart-enabled single-guest RV64 oracle also proves clean role drain,
supervisor replacement, stale old-TID failure, fresh volatile state, and retry.
Kernel heartbeat/watchdog termination logs are fail-hard. This is local-process
evidence only; remote session cleanup and two-node transport remain open.

Phase 04 local protocol construction may proceed while remote dispatch remains
disabled. The canonical V1 envelope has a 112-byte header and a 3,712-byte
payload cap: the minimum bound after local ingress, Noise AEAD, and net-cell
`TcpSend` IPC costs. Streaming and fragmentation are absent. A fixed 16-entry,
30-second dedup cache keys authenticated source NodeId, source boot epoch,
request id, and destination server epoch. In-flight entries are never evicted
or redispatched. Sixteen authenticated source/boot windows retain the highest
ordered request id. A newer authenticated boot advances the boot floor before
response-capacity admission, so saturation cannot revive an old boot; stale
boots and evicted old ids return `Indeterminate`. Expired completed entries may
therefore release response capacity without turning eviction into redispatch.
A nonzero `ServerEpoch` identifies one exported-server incarnation within a
broker lifetime. The boot-local issuer is intentionally volatile. `ReceiveGate`
accepts only a strictly increasing replacement epoch, then retires
dead-incarnation response entries while preserving authenticated source replay
floors. It compares the current epoch before dedup or local delivery.
Authenticated session incarnation state must still invalidate endpoints learned
from an older broker before remote enablement. A shared nonzero
`RelativeDeadline` is mandatory in both the envelope and remote-call API;
absolute monotonic conversion fails on overflow and distinguishes definite
pre-dispatch `Timeout` from post-dispatch `Indeterminate`. `LocalEndpoint<M>`
performs direct sender-masked IPC. `RemoteEndpoint<M>` retains authenticated
route metadata and the frozen typed error taxonomy, but its Phase 04 `call`
requires the deadline and returns `NotSupported` without broker contact.
`CellEndpoint<M>` requires an explicit locality branch. Provider qualification
still gates remote dispatch, relay, and direct LAN.

Phase 05 local contract work begins without opening a remote route. The
four-slot Noise session pool now admits only into empty slots; exhaustion
returns `WouldBlock` for mapping to remote `Busy` and never displaces an
occupied session. `ConnectionManager` checks capacity before `TcpConnect`, so
full-pool pressure cannot leak a newly opened socket or fall through to
`NotSupported`. The Noise prologue binds
`cluster_id || initiator_node_id || responder_node_id` on both peers; it never
uses local endpoint order, which would reverse the responder transcript. Exact
byte-layout coverage pins little-endian cluster ID, initiator NodeId, then
responder NodeId for both local roles; a paired KKpsk0 transcript regression
covers peer agreement. Relay authentication, receive wiring, and two-node
evidence remain absent.

### Cluster membership: 3 modes

A Cell declares its mode via a new additive `__ViCell_cluster` ELF section (follows the `__ViCell_syscalls` pattern; not a manifest/Law-1 change):

- **`Isolated`** (default) — intra-machine IPC only; no cross-machine visibility.
- **`Public`** — reachable by any Public cell on any machine, no auth.
- **`Private(id)`** — cross-machine IPC only within the same named cluster (`ClusterId = FNV-1a-64(name)`, routing-only, **never** authentication).

Routing (cross-machine): Private→Public ✓ · Public→Private ✗ · Private(A)→Private(B) ✗ · any→Isolated ✗.

### Transport security — by tier (decision 2026-06-23, after Noise red-team)

**"Noise vs mTLS" is not a crypto-strength axis** — they are cryptographic peers (AEAD + ephemeral DH + mutual auth). The real axis is the **identity model**. What breaks at G2 is the shared PSK (K1), not the cipher.

| Layer | Transport | Identity |
|-------|-----------|----------|
| **Native Cell↔Cell, G1** | **Noise KKpsk0** (p2p) + **XChaCha20-Poly1305** (gossip) | **K1** PSK (baked, fleet-shared) |
| **Native Cell↔Cell, G2** | **same Noise core** (identity upgrade, not a transport swap) | **K3** per-node static key + DICE attestation; revocation via KMS Cell |
| **Interop / external relay / HTTPS-serving** | **TLS 1.3 mTLS (X.509)** at the external boundary; relayed Cell payload remains opaque Noise ciphertext | CA-rooted PKI; relay NodeId is `SHA-256(SPKI DER)` |

**Hard rules (architectural invariants):**
- Native Cell-to-Cell traffic **never replaces Noise with mTLS**. Noise is the
  lingua franca at every stage; G1→G2 is an identity upgrade (K1→K3), not a
  transport swap.
- mTLS terminates only at an external/interop boundary. A native `net-broker`
  may act as the mTLS client for an external relay, but it must preserve Noise
  end-to-end, validate the relay CA and hostname, and sign through an attested,
  service-net-authorized KMS relay key without exposing private-key bytes.
  X.509 PKI remains outside the Cellos kernel. See
  [ADR-0005](decisions/0005-mutual-tls-relay-identity.md).

- **Profile-specific entropy and output-buffer gate**: the default
  development/QEMU tuple enables `dev-weak-rng` and remains non-qualifying.
  The governed production release tuple omits default features; its
  source-equivalent no-default QEMU companion proves unavailable entropy
  returns zero without synthetic success. GetRandom validates the original
  descriptor, authorizes only its bounded caller-owned writable span, and
  retains final authorization through the write. Isolated RV64 QEMU evidence
  covers direct hostile descriptors and races against root retirement, grant
  revocation, and exact backing-frame reuse. PAL-019 and PAL-031 technical
  backing/evidence are complete but both remain Deferred pending named
  approval. This evidence supplies no real entropy, fleet credentials, release
  signatures, production Noise keys, PAL approval, or promotion evidence.
- `ClusterId` is routing-only; the PSK/Noise handshake is the sole authenticator. Multicast gossip is ~G1-only (cloud VPCs block multicast → G2 discovery shifts to a registry).

### KMS TLS signer boundary (Phase 1, 2026-08-25)

KMS v1 remains a fixed-frame, append-only ABI. Phase 1 adds distinct operations
to bind the live service-net instance, read Relay P-256 status, and sign only a
TLS 1.3 client `CertificateVerify` transcript. Existing broker and C2C
operations are unchanged. The request carries typed protocol state rather than
a generic digest, raw message, caller-selected key, or private-key material, so
the interface cannot be repurposed as a general signing oracle.

Authorization is bound to the service registry's live service-net TID and to
the caller cell ID and generation; a stale or restarted instance is denied
before provider access. One protected-root provider boundary owns two
independent typed leaves, C2C X25519 and Relay P-256. Each leaf has its own
algorithm, generation, policy epoch, provider assessment, and readiness, and
one leaf's state cannot authorize or stand in for the other.

For Relay P-256 signing, the nonzero request ID must advance monotonically.
KMS checks relay generation and the active profile digest, reconstructs the
exact TLS 1.3 client `CertificateVerify` input, validates provider scalars,
normalizes the signature to low-S, and self-verifies it before advancing replay
state or returning the signature. Authorization, replay rejection, profile
binding, and self-verification therefore remain inside KMS rather than the TLS
client.

The implemented Relay P-256 providers are the Phase 1 fixture and the optional
Phase 2 development Silo; neither is protected production hardware. Unsafe
Cargo feature combinations fail at compile time, and the artifact checker
rejects development, fixture, insecure, raw-relay, and K1-fallback paths from
production.

[ADR-0006](decisions/0006-block-production-root-pending-exact-product-evidence.md)
closed Phase 6 NO-GO and selected no production root product. No exact product,
procurement path, OTP/provisioning plan, or board/AP integration is approved,
and `hardware-relay-provider` remains compile-blocked. Production is
`BLOCKED_BY_ADR_0006`.

Phase 4 is product-independent and remains blocked only on real protected
persistence, authenticated time, and a distinct reviewed pending-key binding
under the frozen KMS ABI. Phase 5 is `DEV_REFERENCE`. Phases 7–8 remain blocked:
Phase 7 may implement one exact product and trust chain only after a superseding
GO ADR, and Phase 8 still requires physical qualification and authenticated
build provenance.

The decision may be reopened only after one vendor-signed evidence package
contractually binds all eight ADR-0006 criteria to the same proposed deployment.
Receipt permits new architecture, security, procurement, and board review; it
is not approval. Every item must pass without inference and the selected product
must be recorded by a superseding ADR before production implementation resumes.
Current Phase 1–3 and Phase 6 evidence does not provide client-certificate
issuance, production TLS-client integration, a production artifact, or
hardware-backed signing. The external-boundary identity and key non-exposure
constraints remain governed by
[ADR-0005](decisions/0005-mutual-tls-relay-identity.md).

### Development Silo provider boundary (Phase 2, 2026-08-26)

Phase 2 cleanly removes the public/general Silo boundary. There is no supported
application handle, direct initialization, generic sign/digest operation, ECDH,
raw opcode, or private-key export. The only implemented Silo signing purpose is
the private KMS-provider operation corresponding to the existing TLS 1.3 client
`CertificateVerify` contract. The live KMS instance is authenticated before
private-protocol decode; direct, unbound, forged, stale, and post-fault callers
are denied without guest mutation.

Readiness is exact-instance rather than timing-based. The Silo service admits
and loads the guest, performs one-time development initialization, observes guest
readiness, and validates public metadata before it registers itself. A
`DevelopmentSiloRegistrationCap` exists only with `test-hooks`, is minted only
for the governed exact `/bin/silo` root task, cannot be requested through a
manifest or delegated through `CapSet`, and authorizes only
`RegisterService(SILO, tid=0)`. Init and the supervisor require the registry to
contain the exact spawned TID before KMS starts or restarts; `HypervisorCap`
alone carries no readiness authority.

The standalone guest is built through its locked package and admitted before VM
creation by non-empty, maximum-size, and exact SHA-256 checks. The verified image
is 33,888 of 61,440 available bytes with digest
`fea5cd2b9c36bb158e1e74b9e2c60209c133e0057292f0b9b4bc5f3e830838e4`.
Guest protocol/crypto faults, VMM faults, malformed or stale responses, and
reset permanently latch the current instance unavailable. There is no retry or
in-process fallback; a governed permanent-service restart creates and admits a
new exact instance.

This lane is explicitly `DEV_REFERENCE` and AArch64 virtualized-QEMU-only.
Stage-2 supplies useful address-space and fault containment, but the Cellos EL2
host still creates the VM, loads the guest, and supplies its disposable
development seed. It is therefore software-custody evidence, not an independent
hardware root, hardware-backed Silo, kernel-compromise-resistant custody, or
production qualification.

The exact signed 12-cell QEMU image passed registered readiness, KMS
self-verification, direct/unbound denials, VFS PAGE+REG lifecycle checks, and
`vfs-test` 96/0. This does not alter the production gate:
`BLOCKED_BY_ADR_0006`. Phase 6 closed NO-GO with no product selected; only a
superseding GO ADR may unblock Phase 7, and Phase 8 still requires physical
qualification and authenticated build provenance.

### Relay certificate activation and provisioning boundary (Phase 3, 2026-08-26)

Phase 3 extends the append-only KMS v1 contract with purpose-specific opcodes
9–14 for enrollment begin, ordered CSR reads, atomic generation commit, abort,
service-net profile staging, and active public-key inspection. Enrollment begin
is restricted to the live supervisor identity; profile staging and active-key
inspection are restricted to the live service-net identity. Neither surface
accepts caller-supplied private-key material, CSR bodies, or arbitrary digests.

KMS permits one volatile pending generation and enforces
`Prepared -> CsrIssued -> Staged`. The CSR handle is bound to the generation,
policy epoch, restart epoch, request, and exact supervisor identity, and its
bounded 104-byte chunks must be consumed once in strict order. Any stale,
foreign, repeated, or out-of-order access invalidates the pending flow. Commit
requires the exact staged generation, policy epoch, and profile digest; abort
and invalidation retain a cleanup tombstone until the provider confirms that
the pending key is deleted or already absent.

The development provider creates the fresh P-256 enrollment key inside Silo
from a nonce and pending generation. The scalar is never exported. Silo and KMS
independently reconstruct the frozen RFC 2986
`CertificationRequestInfo`; KMS validates the returned point and signature,
normalizes it to low-S, verifies it, and only then assembles and publishes the
bounded canonical CSR. Commit promotes the pending key inside the provider
between lifecycle validation and activation. If provider promotion cannot be
matched by lifecycle activation or protected persistence, KMS seals serving
rather than expose a mixed active tuple.

Only the committed generation/profile and monotonic restart and authenticated
time floors are recoverable; pending enrollment material is intentionally not
persisted. Missing, torn, unavailable, or regressed protected state seals both
enrollment and serving. The default runtime therefore remains sealed until
protected persistence and authenticated time are supplied; process-local
counters or volatile entropy are not substitutes.

Service-net's mounted profile boundary is schema-allowlisted and rejects
unknown/duplicate fields, non-canonical or oversized paths, and any client
private-key field. Certificate chains are bounded to three certificates and
12 KiB, use strict DER framing, require a clientAuth leaf without serverAuth,
and, for an active chain, require the leaf SPKI, opcode-14 SPKI digest, KMS
NodeId, and manifest NodeId to agree.

Opcode 14 deliberately returns only the active generation's public key. It
cannot authenticate the pending enrollment key before commit, so the
active-chain validator must not authorize opcode-13 staging. Initial enrollment
and renewal therefore remain fail-closed until an authenticated pending-key
binding is available without reinterpreting the frozen opcode. Phase 3 does not
claim that binding, protected production persistence, authenticated production
time, hardware custody, or a production relay artifact. Production remains
`BLOCKED_BY_ADR_0006`.

### Robot swarm (G1) vs server cluster (G2/G3)

Same foundation, **opposite coordination semantics** → two separate problems:
- **G1 robot swarm** — leaderless, small N, fixed hardware. "Merge" = federation (shared control surface + one primary), NOT literal SAS unification. Adds: task-claiming gossip, runtime enrollment, **degrade-to-standalone** (lose peers > X s → drop shared tasks, release leases; lease is an *optimistic hint* — physical actuator safety must use a local interlock independent of it). k8s's hard problems (scheduler, Raft consensus, autoscaling) **do not apply**.
- **G2/G3 server cluster** — hierarchical control plane; deferred. Lean on external k8s/LB; **do not reimplement CNCF**. Cellos is a great *node*.

---

## Current Status (2026-07-25)

> **Status refresh 2026-07-25**: Every item the 2026-06-05 snapshot listed as In-Progress / Planned — KASLR, ARM64 full bring-up, ViUI v2, reliability/supervisor restart, Tier 3 Linux VM, cell signing — has since **shipped** (cross-checked against `docs/project-roadmap.md` milestone table). The legacy Tier 3b VirtIO-GPU host stack is code-complete and documented below, but the strict Linux guest lane remains hardware-gated on ARM64 KVM or real hardware. See the "Recently shipped" block below.

### ✅ Implemented (Phases 01, 02, 05, 10, 14, 15, 16, 18, 20, 24, 26, 31, C–H, A–E, X-1–X-3, Peripheral Driver Track v1, Robot Demo, ViUI v2, Reliability P00–P06, Tier 3 VM, Cell Signing)
- **RV64, AArch64, x86_64** HAL with paging (SV39/4K/4K respectively)
- **Responsibility-bounded kernel** ([generated nLOC](code-metrics.generated.md); see Spec 15) with fixed-priority scheduler and RT-hart routing
- **Exact launch-edge profiles** — kernel-authorized `(caller, route, target)` rows gate shell/init/hypha/tool-spawn/supervisor/pinned launches; shell carries no ambient SpawnCap/gpio/uart, `SpawnFromMem` remains fail-closed, the shell-only `/bin/hotswap` edge stays capability-free, and supervisor replacements are bounded by reviewed target rows, the frozen-task ceiling, and manifest/policy checks
- **48 syscall variants** (IPC, memory, task, FS, GPU, network, state) + **Block I/O capability gate**
- **Block I/O syscalls** (raw 500/501/503 for FAT16 persistence, gated to VFS task 3)
- Frame allocator (bitmap) and virtual memory
- **RV64 DTB memory discovery** — enabled RAM nodes minus firmware, live-kernel, FDT
  `/memreserve/`, and static `/reserved-memory`, with audited static-map fallback; a 2 GiB QEMU
  capacity gate verifies more than 1 GiB is managed
- ELF loader with PIE relocation support
- **VFS service** (RamFS read/write, FAT32 write/read/delete via block device, zero-copy grants)
  - **10 IPC opcodes** (0x01–0x0A): OP_GET_FILE, OP_LIST_DIR, OP_STAT, OP_WRITE, OP_MKDIR, OP_RMDIR, OP_UNLINK, **OP_READ, OP_RMDIR_RECURSIVE, OP_APPEND**
  - **Zero-copy grants** (syscalls 208–212): GrantAlloc, GrantShare, GrantSlice, GrantFree, BlkReadAsync
  - **4-byte OP_WRITE header** (u16 content length, up to 65KB writes per message)
  - **OP_READ (0x08)** — read file bytes (up to 480, path → bytes)
  - **OP_APPEND (0x0A)** — seek-to-end append write
  - **OP_RMDIR_RECURSIVE (0x09)** — recursive directory delete (restricted to /data/ path prefix)
  - **OP_UNLINK** for /data/ flat files and nested paths
  - **/data/ subdirectories** with mkdir -p semantics and full path traversal
  - **OP_MKDIR** for /data/ nested directory creation
- **FAT32 filesystem** (LBA 0–524,287 on VirtIO disk, 540K sectors, /data/* paths persistent with subdir support)
- **Config service** (KV store with ViStateTransfer)
- **Interactive shell** (parser+executor) with:
  - Pipes, redirection (>, >>), background jobs (&), history, aliases
  - for/in/do/done, while/do/done, if/then/else/fi loops
  - case/esac conditional, shell functions (name() {}), **command substitution $(cmd)**
  - **Function arguments** ($1, $2, ..., $9)
  - **read built-in** for input
  - 45+ built-in commands
- **Lua 5.4** / **MicroPython 1.24.1** native runtimes — ⚠️ milestones (3.3/3.4) marked complete, but **the native runtimes are NOT actively maintained** (roadmap §D, decision 2026-06-06: half-measure dropped). The Python/scripting story is **Tier 3 Linux VM** (`apt install python3`), not a native Cellos runtime. Robot code stays Rust (Tier 1).
- **Keyboard input** (VirtIO, multi-key support, no deadlock)
- **Network** (smoltcp TCP/UDP/DNS, DHCP verified, full data-path TCP client+server)
  - **TCP client**: SOCKET_TCP, CONNECT, SEND, RECV, CLOSE
  - **TCP server**: LISTEN (0x17), ACCEPT (0x18) opcodes
  - **UDP**: SOCKET_UDP, SENDTO (0x21), RECVFROM (0x22), BIND
  - **DNS resolver**: static table → IPv4 literal → UDP A-record query
  - **net-tools binaries** (6 total): ping, curl (HTTP/1.0), wget, nc (multi-conn relay), httpd, mqtt (skeleton)
- **GPU framebuffer** (opt-in, basic compositor)
- **Tier 3 VirtIO-GPU host stack** (legacy: Tier 3b) — host device model, resource/scanout Grant lifecycle, and compositor bridge are implemented; strict guest verification stays hardware-gated (`TIER3B_GPU_E2E=1` on ARM64 KVM / real hardware).
- **HotSwap orchestrator** (5-step live Cell replacement, kernel + shell + config + vfs + robot-demo verified)
- **Peripheral Driver Track v1** (GPIO/UART HAL traits + driver Cells + safe MMIO + Resource Registry)
  - `cells/drivers/driver-gpio/` — PL061 GPIO implementation (QEMU ARM virt)
  - `cells/drivers/driver-serial/` — PL011 UART extension
  - `ostd::mmio::MmioRegion` — safe memory-mapped I/O (forbids unsafe in Cells)
  - Manifest-based capability gating via `declare_manifest!(gpio=true, uart=true)` (Phase 30)
- **Robot Demo (`cells/apps/robot-demo/`)** — Reference G1 closed-loop application
  - Sensor read (GPIO input) → control compute → actuator write (GPIO output)
  - MQTT 3.1.1 client: TcpConnect → handshake → publish telemetry → close
  - 7-cell boot sequence: vfs, config, input, net, compositor, shell, robot-demo
  - Graceful fallback to simulation when GPIO unavailable
  - Policy: Temporary (run once, no restart)
- **Workspace consolidated** with 0 cargo warnings
- **CI/CD pipeline** with architecture validation (10/10 score)
- **VirtIO VA→PA mapping fix** (Phase X-1) — resolves multi-sector write issues

### ✅ Recently shipped (were In-Progress/Planned in the 2026-06-05 snapshot)
- **KASLR** — ✅ COMPLETE (Phase 24, 2026-06-05) via Limine boot randomization (`limine.conf` `KASLR=yes`); 65 integration tests pass with KASLR enabled.
- **ARM64 full kernel bring-up** (beyond ring-3 smoke) — ✅ COMPLETE 2026-06-12: GIC, generic timer, 3-level MMU, VirtIO, PL011 RX, PL061 GPIO on QEMU virt; 6/6 integration tests pass.
- **ViUI v2 — Reactive Signal Tree + Dual-Layer DSL** — implemented library surface
  includes overlays, navigation, charts, `.vi` build integration, virtual lists, flex
  layout, and signal bindings. "Production-ready" remains gated on signed App Cell,
  input/render integration, compositor-damage validation, and measured target evidence.
  See [Spec 14](specs/14-viui.md).
- **Reliability / never-die / supervisor restart** — ✅ SUBSTANTIAL (P00–P03 done 2026-06-06: fault-path force-unlock, reboot-on-panic, stack guard pages, RT watchdog; P05: RecvTimeout deadline, NotifyOnExit supervisor, zombie reaper; P06 observability) — see [specs/12-reliability.md](specs/12-reliability.md).
- **Generic completion contract** — QEMU markers pass for completion-queue reserve/land/bound/defer, net-rx-reservation fill/remember/release, and ipc-pending deferred delivery/bounds/quota; RV64 now enables S-mode external IRQ delivery, VirtIO ACK uses scoped SUM + exact `InterruptStatus`, NIC owner/device-type binding points the NET_RX source and is the only production caller of `signal_net_rx()`, the `[net-rx-producer] irq->completion PASS` witness requires a real RX drain, and shared death/hotswap clears driver roles; the completion path now covers finite `TIMER` as well as `NET_RX`, uses fail-closed source masks, keeps `Recv*`/`WaitForEvent` intact, and `libs/ostd/src/executor.rs` now parks through an `Arc`-backed `RawWaker` on a one-tick TIMER wait with fail-loud authority checks; the exact QEMU parked marker is `[executor] dummy-waker=absent executor=parked source=TIMER PASS`. Peer-death CQ target-generation ABI, `RecvScatter`, and async VFS/DMA remain deferred.
- **Phase 08 stack-sizing gate baseline** — the measured static table now covers `init`, `shell`, `vfs`, `vfs-test`, `net`, and `virtio-net`; each path lands at 16 usable pages plus 2 guards, using `max(16, ceil(2 * peak / 4096))` from the captured kernel/user watermarks. Unknown or risky paths stay on the 64-page default until they are measured.
- **Memory quota + ZST caps + panic isolation** — ✅ Phase 26 (per-cell OOM no longer takes down the system).
- **Tier 3 Linux VM** (legacy: Tier 3b) — ARM64 EL2 boots Alpine 3.21.3 aarch64 and has its CI smoke
  lane. x86 is backend-specific: AMD SVM has an implemented MVP registry/vCPU/run-loop
  path, while Intel VMX currently enters root operation but lacks VMCS/EPT guest
  execution. Neither x86 path is production hardware-qualified. RISC-V H-extension
  remains unsupported on the current board set.
- **Cell-signing mechanism + hot migration** — ✅ MECHANISMS COMPLETE 2026-06-23; Phase 00 public syscall landing is complete 2026-08-07: `PauseService` 422 is SupervisorCap-gated with bit 49, rejects cached-TID ingress and waits for accepted sender/mailbox work to drain before Snapshot; legacy `HotSwap` 400 is retired/reserved, `HotSwapReady`/`Snapshot` keep bit 32, `SpawnReplacement` 421 is additive and allowlist bit 57, `SupervisorCap` gates Freeze/Resume/Kill/QueryHotswapReady/SpawnReplacement, the kernel consumes one live frozen-task ceiling under `SCHEDULER -> SWAP_CEILINGS`, and resume / all scheduler exits clear the ceiling. Phase 01 supervisory atomic cutover is complete: compare-and-commit barrier, cached sender FIFO proof, old-TID rejection, and `supervisor_hotswap_preserves_demo_state` passed with `[hotswap-cached-sender] PASS` and `[hotswap-demo-v2] SpawnCap retained`. Phase 02 supervisory hotswap closure is complete: the `hotswap` CLI uses Supervisor IPC, the exact shell-only `/bin/hotswap` edge stays capability-free, unauthorized senders receive `0xFD`, and the final QEMU hotswap-smoke suite passed 15/15 zero skip; fleet signed-only admission remains planned.
- **Development Silo provider** — ✅ PHASE 2 `DEV_REFERENCE` COMPLETE 2026-08-26: the former public/general Silo API is removed; the signed AArch64 virtualized-QEMU lane is KMS-mediated and test-hooks-only. Stage-2 is software containment, not hardware custody; the Phase 6 NO-GO leaves production `BLOCKED_BY_ADR_0006`.

### 🚧 In Progress / Partial
- **MQTT binary** (skeleton added; full implementation deferred)

### ⏳ Genuinely planned (later phases)
- Peripheral Driver extensions: I2C shipped; SPI/CAN/PWM/ADC via sim/loopback + generic bit-bang (G1 ext / G2 real MMIO)
- Real SBC validation (RPi 4 / VisionFive2 / Radxa ROCK 5)
- DICE/RIoT attestation chain, KMS Cell for G2 key management
- Additional architecture ports (RV32 nano, full x86_64 beyond ring-3)

> ⚠️ **Per-Cell SATP isolation at Tier 1 is explicitly NOT pursued** (decided 2026-06-05).
> Hardware isolation belongs to Tier 2 native domains and Tier 3 VM guests, not to
> every Tier-1 Cell. Tier 2 is the future native private-MMU-domain class, not just
> "unsigned Tier 1" — see
> `docs/specs/18-cell-trust-tiers.md`. See *Key Design Decisions* below
> and [specs/05-application.md §6](specs/05-application.md).

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Single Address Space | Reduce context-switch overhead, simplify memory management |
| Language-Based Isolation | Rust's type system enforces isolation better than hardware |
| **No per-Cell SATP (Tier 1)** | Per-cell page tables would break Tier 1 zero-copy IPC and add `sfence.vma` cost on every switch (ASID broken on most RV silicon). Untrusted code is confined to the **Tier 3 Linux VM** (Stage-2/EPT). Decided 2026-06-05. |
| Tiered isolation (1 / 2 / 3) | Tier 1 trusted SAS cells and runtime profiles · Tier 2 native domains in a private MMU protection domain once implemented — see `docs/specs/18-cell-trust-tiers.md` · Tier 3 VM guests (legacy: Tier 3b Linux VM). |
| Native SDK contract | One named-module SDK family shared by Tier 1 and the future Tier 2; availability is evidence-gated by Spec 23 and the Phase 02 acceptance ledger. |
| Fixed-Priority Scheduler | Three tiers, FIFO within tier, RT-hart routing on RV64 |
| Capability-Based Access | Fine-grained control, no global permissions |
| Owned Buffers in Async | Deterministic cleanup in SAS (no process teardown) |
| Nano Kernel (nano by responsibility, not a frozen line count) | Keep TCB minimal by Spec 15 scope; [generated metrics](code-metrics.generated.md) own both total and core nLOC trends |
| Trait-Based HAL | Multi-architecture support without code duplication |
| No mod.rs | Clearer module boundaries, IDE-friendly |

---

## Architecture Gap Summary

Areas where the current implementation diverges from the specification or modern OS best practices.

| Gap | Impact | Status / Target |
|-----|--------|-----------------|
| IPC is syscall-based, not direct vtable call | Direct-vtable fast-path remains unimplemented; use measured IPC results rather than an estimated multiplier | **Open** — wire contract ratified ([specs/17](specs/17-ipc-wire-contract.md)); direct vtable fast-path still Phase 27 |
| Fixed-priority scheduler shipped; RV64 immediate preemption only | Consolidated latency baseline still pending | **Closed / verify** — architecture-scoped limit |
| TLSF pool initialised but unused; no runtime caller or WCET qualification | RT allocation guarantee not yet established | **Open** — follow-up qualification |
| Per-path stack sizing | Measured table now covers init/shell/vfs/vfs-test/net/virtio-net with 16 usable pages + 2 guards; unknown/risky paths remain 64 | **Closed** — shrink is now evidence-backed; no ABI/public manifest field was added |
| Spectre v1/v2 unmitigated in SAS | Critical for untrusted code | **Mitigated by design** — untrusted code confined to Tier 3 Linux VM (Layer-2 HW mitigations for native, see Security Model) |
| No KASLR | Kernel address predictable | ✅ **DONE** (Phase 24, 2026-06-05 — Limine boot randomization) |
| No per-cell memory quota enforcement | Single cell can OOM system | ✅ **DONE** (Phase 26 — quota + ZST caps + panic isolation) |
| Performance baseline unmeasured | Can't validate PDR targets | ✅ **DONE** (Phase 24 — bench cell, RT + SMP latency) |
| Audit ring buffer | Forensics | Partial — reliability P06 observability shipped; full audit log G2 |

---

## See Also

- **CLAUDE.md** — 8 Coding Laws & quick reference
- **api-reference.md** — Full trait & syscall reference
- **patterns.md** — Common code patterns
- **codebase-summary.md** — File structure & LOC counts
- **code-standards.md** — Code style & naming
- **Specs**: `docs/specs/0X-*.md` — Detailed subsystem specifications
