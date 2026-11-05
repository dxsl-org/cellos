# Cellos Security Model

**Updated:** 2026-10-11 — architecture precedence clarified; dated threat rows below are historical assessments, not fresh security verification.

## Design Philosophy

The current authority/trust contract is [ADR-0024](decisions/0024-cell-kernel-authority-contract.md)
and [Spec 25](specs/25-cell-kernel-authority-contract.md), with workload routing
and scaling constraints in [Spec 26](specs/26-workload-placement-scaling-contract.md).
The [architecture guide](system-architecture.md) distinguishes existing mechanisms
from pending implementation. These contracts supersede incompatible claims below,
but do not certify those historical mitigations or close an open risk.

- Tier 1 SAS is for admitted trusted Rust and audited unsafe substrate, not a
  hardware sandbox for hostile native code. Untrusted/FFI workloads require
  the qualified isolation tier; unavailable isolation means refusal, not downgrade.
- Typed, generation-bound capabilities, shared Cell CSpace and scoped delegation
  are the target authority model; a name, signature, tier or authenticated node
  does not itself grant a method/resource right. Shared CSpace migration is pending.
- Close releases a reference; revoke cuts descendant authority and quiescence
  must precede reuse. These are distinct, not interchangeable security operations.
- Issuers validate their own resources. Routers preserve original-subject
  authority; controllers cannot bypass destination admission or multiply budget.
- Signing/provenance, isolation, scoped authority and resource accounting are
  separate requirements. Rust memory safety alone does not establish all of them.

The STRIDE tables retain earlier observations and code references. Their status
labels must be revalidated at the applicable source/profile/runtime ceiling before
release; they are not a current all-tier or production security PASS.

## STRIDE Threat Model

### Spoofing
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell forges another Cell's CellId in IPC | Kernel verifies sender ID from TCB on every message; user cannot inject arbitrary sender values | ✅ Mitigated |
| Cell guesses a service-local socket CapId | The net service currently allocates predictable CapIds without binding lookup to the caller, so another network-capable Cell may address a live socket owned by a peer | ❌ Open — see [roadmap/open-risk-register.md](roadmap/open-risk-register.md) |
| Malformed ELF binary spawns as a different Cell | ELF header validated before execution; Cell registry assigns IDs monotonically | ✅ Mitigated |
| Network peer logs in to `/bin/sshd` without a key, or impersonates its host key | `publickey` (`ssh-ed25519`) only, verified over the RFC 4252 §7 input against `/srv/sshd/authorized_keys`; phases enforced per message (anything out of phase disconnects); 5 failures disconnect; 60 s login grace. Host key: a seed persisted at `/srv/sshd/host_ed25519.seed`, generated from kernel entropy on first start; x25519 scalar and KEXINIT cookie fresh per connection; no entropy → no start / no handshake; non-contributory DH refused | 🔶 Partial — key files bound to the measured sshd image (below); spoofable under the dev posture (see Known Limitations) |
| Another Cell reads the sshd host seed or adds itself to `authorized_keys` | `/srv/sshd/**` is admitted only to a Cell whose **kernel-measured image** is signature-verified and named `sshd` (`cells/services/vfs/src/access/srv-owner.rs`). The kernel records per incarnation the SHA-256 of the admitted bytes, the verifying anchor and the program name from the signed `__ViCell_image` section, which the signer assigns from the artifact name (`service-sshd` → `sshd`); an unsigned image carries no name. VFS asks the kernel (`ResolveCellImage`, syscall 441, registered VFS Cell only) for the exact `(cell, generation)` of each request — never a cached tid, so a stale or respawned generation does not match. Accepted anchors: release, and dev (only a dev-anchor kernel can report it; a release kernel reporting dev does not build). Every other caller is refused read, stat, list, write, append, mkdir, rename (either end), unlink, D03 save and handle traversal; nobody removes the namespace or an ancestor; the interrupt-disabled fast path refuses it outright | ✅ Mitigated under the release anchor; 🔶 dev posture: the dev seed is public, so a Cell that can spawn from bytes can sign an image named `sshd` |
| Cell drives a NIC driver directly to inject or read raw frames; off-path peer predicts TCP ISNs, DNS ids/source ports or IPv4 ids | The virtio-net, e1000, igb and DWC2 NIC Driver Cells serve Tx/Rx/GetMac only to the tid the kernel's registry names as the live `service::NET` provider, asked per request (`ostd::service::sender_is_live_provider`), and answer anyone else with the failure byte; the LAN9514 front-end attaches only to the registered `NIC_DRIVER` host. service-net seeds smoltcp's `Config::random_seed` and a random-increment ephemeral-port cursor from `GetRandom` at start and refuses to start without entropy (audit H-20/H-21, 2026-11-05) | ✅ Mitigated — a `dev-weak-rng` kernel without hardware entropy serves predictable xorshift bytes |
| Cell keylogs the GUI session, shows another Cell's window buffer, or spoofs the desktop shell | Input: once a live `COMPOSITOR` binding (tid + attested cell/generation) is seen, only that compositor moves the physical keyboard; other `SetFocus` senders claim the serial console only. Compositor: client create/raise/select never takes focus from another client (pointer press does; one new window may after a left press on the desktop `Panel`, until the next press or key), keys stop for a focused surface that is off-screen; a Grant another owner's surface presents is refused and every `ATTACH_GRANT` refusal replies alike; Grants are re-resolved on every blit and detached once released; frames are single-owner; `Panel` is single-owner; geometry is bounded (audit H-27–H-29, 11 S-5/S-7/S-8, 2026-11-05) | 🔶 Partial — a Grant shared to the compositor but not yet attached is attachable by any client (no kernel owner query); the owner can free a Grant between resolve and copy (no pin); a window created right after a panel press races the launched app; the serial console route and headless images stay self-service |

### Tampering
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell writes to another Cell's memory | Historical SAS ownership assessment applies only within admitted safe-Rust/audited trust constraints; hostile/unsafe code requires qualified hardware isolation | Historical scope — not universal mitigation |
| Attacker uses a closed/revoked handle | Target distinguishes close, transitive authority revoke and resource quiescence; exact current issuer enforcement must be verified before promotion | Contract selected; shared-authority migration pending |
| Attacker modifies disk image to inject malicious ELF | Every spawn source reaches the same SHA-256 measurement + Ed25519 verification hook. A present invalid signature is denied, but default G1 leaves `signing-required` off: stripping `__ViCell_sig` turns the image into an admitted unsigned SAS cell. The reproducible dev seed is forgeable and no production public-key provisioning path exists yet. | ⚠️ Partial — mechanism shipped; fleet enforcement open |
| Hostile NVMe controller steers the driver's MMIO or DMA | `driver-nvme` reaches registers and doorbells only through bounds-checked `MmioRegion`; init refuses a `CAP.DSTRD` whose doorbells overrun the claimed 16 KiB BAR0 window and any namespace format other than 512-byte blocks without metadata (one-page PRP1 transfers only); a page with a timed-out command is retired, never reused or freed (audit 08 S-3/S-4/S-11) | ✅ Mitigated |
| Kernel user copy writes a frame freed mid-copy (`GrantFree`/`GrantUnregister`, SHM release, sibling stack drop) | Every Sas commit re-checks VALID\|USER\|R/W for each page inside an interrupt-masked per-hart window (`user_copy/sas-lease.rs`); every path that retires a shared-root user frame unmaps it, flushes every hart and waits for open windows (`stack::unpublish_user_frames`) before re-mapping it kernel-RWX and freeing it, so a racing copy gets a recoverable error instead of writing the reused frame (audit H-2). Builds without `native-domains` keep the unchecked legacy copy | ✅ Mitigated (`native-domains`) |
| Crafted FAT volume (`/mnt/sd`, `/mnt/disk/*`, `/bin` cell-store) steers I/O into a neighbouring partition; one Cell disables `/srv` or `/data` | Every FAT sector access is bounded by its partition length from the VFS mount plan (`BlockStream::abs_lba`); CellosFS requests are all-or-nothing (`CellosFs::atomically`), so a refused request (missing parent, full directory, long name, no space) leaves the mount serving and only a storage fault poisons it; unlinked inode numbers are reused with their generation continued past a tombstone (audit H-22/H-23) | ✅ Mitigated |
| Domain-grant receiver keeps a PTE to frames freed by a concurrent revoke | `DomainGrant::publish` runs its `Live` check, receiver map and receiver record inside one hold of the record's lifecycle gate, and `revoke` makes its only `Live → Revoking` transition under the same gate (`task/domain-grant-gate.rs`), so a racing `GrantSlice`/`GrantShare` is either drained by the revoke or maps nothing (audit H-5) | ✅ Mitigated |
| Cell image maps the null page, a stack guard page or another cell's VA slot | Every image owns one `va_alloc` slot (PIE: any free slot; fixed-VA ET_EXEC: the slot it is linked into, refused when another spawn holds it) and every PT_LOAD is admitted before any frame is allocated only if it avoids the null page, lies in `[CELL_VA_START, USER_VADDR_MAX)` and inside that slot, and overlaps no live kernel/user stack run, guard pages included (`loader/image-va.rs`, `task/stack-reservations.rs`; audit H-6) | ✅ Mitigated |

### Repudiation
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell claims it did not send an IPC message | Sender ID in TCB is set by kernel on message delivery; cannot be forged | ✅ Mitigated |
| Audit log missing | Kernel audit ring buffer shipped (Phase 26, `kernel/src/audit.rs`) — records `IpcSend`, `CellFault`, `CellExit`, `CellMeasure`, etc. events | ✅ Mitigated |

### Information Disclosure
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Kernel leaks pointers to user-mode | Kernel zeroes new frames before mapping; TrapFrame zeroes scratch regs on EL0 return | ✅ Mitigated |
| Spectre / Meltdown side-channel | Known limitation; not mitigated in v1.0.  See `known_limitations` below | ⚠️ Known |
| File content readable without capability | VFS returns data only to cells holding a valid `CapId` with READ permission | ✅ Mitigated |

### Denial of Service
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell allocates unbounded memory | Frame allocator has a hard cap (total usable RAM); OOM kills the cell | ✅ Mitigated |
| Cell floods IPC queue | Message queue is bounded; sender blocks when full (future Phase 20) | 🔶 Partial |
| Lua script infinite loop | Cell exit triggered by kernel timeout (future scheduler enhancement); MicroPython is an archived runtime experiment, not a current workspace member | ❌ Deferred |

### Elevation of Privilege
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell executes privileged instruction (e.g., `wfi`) | Cells run in EL0/Ring3; trap dispatched to kernel | ✅ Mitigated |
| AArch64 EL0 Cell runs untranslated under an EL2 host (`HCR_EL2.{E2H,TGE} = {0,1}` makes EL0 ignore `SCTLR_EL1.M`) | The QEMU `virt` EL2 host is VHE-only (`{1,1}`, EL2&0 regime with AP/UXN enforced); boot writes `E2H` and reads it back before setting `TGE`, a CPU without FEAT_VHE drops the host to EL1 (EL2 keeps only an SMCCC stub, no hypervisor), and `el2_mmu_init` re-reads `HCR_EL2` before the first Cell (2026-11-05) | ✅ Mitigated — QEMU TCG never showed the flaw (it translates EL0 under `{0,1}`); it applies on silicon/KVM |
| `#[allow(unsafe_code)]` in a Cell | `scripts/cellos-sign --check --strict` fails CI when a Cell file contains `unsafe` and is not in the reviewed allowlist, and refuses to sign; allowlist entries carry reason/approver/date and are reported once past `review_by` | ✅ Mitigated, with a bounded hole — see `docs/specs/18-cell-trust-tiers.md` §2.1 |
| Malformed syscall arguments overflow kernel buffers | All syscall arg lengths validated via `validate_user_buf` before dereference | ✅ Mitigated |
| Driver/service cell reaches kernel memory or a foreign service id through a narrow cap (`PlatformCap` `RequestMmio`, `SpawnCap` `RegisterService`, `GpuFlush`/`GpuCursor` source pointers) | `RequestMmio`'s PlatformCap path claims only inside the kernel-admitted ECAM window and never over managed RAM; well-known service ids are bound only by trusted init or SupervisorCap holders, a spawn-only cell publishes only a private id it owns; scanout is compositor-only, the source range is probed readable through the caller's copy view before the GPU Cell reads it, and the framebuffer fallback stages through the view (2026-11-05) | ✅ Mitigated — the GPU Cell still reads the compositor buffer asynchronously in SAS (no pin) |

## Known Architecture Risks

The maintained severity-ranked inventory is
[roadmap/open-risk-register.md](roadmap/open-risk-register.md). In particular,
production signing still requires an immutable provisioned key and mandatory
release policy, while the net service must owner-bind socket handles before it
can be treated as a capability boundary.

### Spectre v1/v2 — SAS Worst-Case Scenario
**Severity: Critical (research/trusted-environment only)**

SAS is the worst-case environment for Spectre attacks. In a traditional OS, Spectre leaks within a single process boundary. In Cellos SAS, a compromised Tier 1 cell can speculatively read any memory in the entire system — including kernel heap, crypto keys, and other cells.

**Current status**: No mitigation. Cellos v1.0 requires all Tier 1 cells to be trusted (signed, first-party code).

**Mitigations planned**:
- Short-term: Document "trusted cells only" constraint explicitly (done here)
- Medium-term: Tier 3 VM isolation for untrusted code (hardware page tables per VM)
- Long-term: CHERIoT RISC-V hardware capabilities — see "Hardware Isolation Roadmap" section below

**Settled Architecture Baseline (ADR-0015)**: [ADR-0015](decisions/0015-dual-mode-hybrid-architecture.md)
formalizes Cellos as a **Dual-Mode Hybrid Operating System**:

1. **Tier 1 (Real-Time SAS)**:
   - **Strict Safe Rust**: Compile-time `#![forbid(unsafe_code)]` verified via mandatory Ed25519 signatures (`signing-required = ON`).
   - **Audited Driver Cells**: Essential hardware drivers (VirtIO, NVMe, e1000) that require raw MMIO registers and DMA pointers operate in Tier 1 under IOMMU translation authorization. They form a strictly bounded, audited kernel-driver surface distinct from user-facing applications.
   - **Fastpath IPC**: Lock-free SPSC shared memory ring buffer between same-hart cells eliminates kernel trap overhead.

2. **Tier 2 (Paged Domain Engine)**:
   - **Hardware Memory Containment**: Dedicated per-domain page tables (`satp` on RISC-V, `CR3` on x86, `TTBR0` on ARM).
   - **Mandatory destination**: All unsigned binaries (including unsigned Rust developed during debug cycles), all C/C++ FFI applications (Doom, Tetris-C, mlibc), and dynamic runtimes (Lua).
   - **Safe Reaping**: Memory violations (null pointer dereferences, buffer overflows) trigger CPU Page Faults caught by the kernel; the offending cell is terminated without compromising the SAS.
   - **IPC Bridge**: Microkernel syscall traps with `validate_user_buf` boundary copying.
   - **SMP handoff and shootdown**: on every SMP target (RV64, x86_64, AArch64) a task's Context is dispatchable on another hart only after the raw switch that left it has saved it — the switch stub calls `vi_context_switch_complete` on the incoming stack, which releases the outgoing-save guard and the selection pin armed under `SCHEDULER`; AArch64 also masks IRQs from selection to the switch. The boot hart is published in `HART_ONLINE` by `task::init` on every architecture, so remote TLB/ASID shootdowns and flush epochs issued from any hart include hart 0. A hart spinning with interrupts masked — in a shootdown wait or a `Spinlock` acquisition — answers its own pending flush (`smp::answer_tlb_flush_in_spin`, lock-free and log-free), so two harts waiting on each other cannot stall into a failed probe.

3. **Tier 3 (Hardware Virtual Machine Guest)**:
   - Full hardware virtualization via Stage-2 paging running unmodified Linux guests.
   - **x86 world-switch boundary**: the guest never reaches host-owned CPU state. SVM intercepts every SVM instruction (CLGI/STGI, VMLOAD/VMSAVE, SKINIT, VMMCALL → guest #UD), XSETBV, MONITOR/MWAIT, INVLPGA and SHUTDOWN; VMX exits on CR8 access and emulates a virtual TPR (a VMCS without external-interrupt, unconditional-I/O, CR8 or EPT exiting is refused). Host PKRU and XCR0 — on VMX also CR8 and `KERNEL_GS_BASE` — are restored right after every exit. VMX host state (CR3, GS/FS/TR/GDTR/IDTR bases) is rewritten from the running CPU on each run and the VMCS is `VMCLEAR`ed between calls, so a vCPU may run on any hart. Guest CPUID hides XSAVE/AVX, MONITOR, VMX/SVM and x2APIC.
   - **Guest-memory boundary**: Stage-2/EPT/NPT maps compute their span with checked arithmetic, refuse ranges larger than the guest carve, and build every table before writing a leaf, so a failed map leaves no partial mapping; `MapGuestMemory` also refuses `size == 0` and sizes above the guest RAM. `RunVcpu`, `VcpuRegs`, `Read/WriteGuestMemory` move bytes only through the caller's copy view into kernel staging buffers (≤ 256 KiB chunks), so a cell-supplied address is never dereferenced by the kernel. EPT changes are made visible with `INVEPT` (single-context, else all-context; an EPT guest is refused on a CPU without it): after every map/unmap, before freeing the tables and guest RAM, and on the entering CPU before every VMX entry, which covers translations cached on other harts. NPT relies on the per-VMRUN ASID flush.

#### Cell Admission Matrix (ADR-0015)

| Source & Language | Signed (`__ViCell_sig`) | Target Execution Tier | Isolation Mechanism | IPC Mechanism |
|---|:---:|:---:|---|---|
| **Safe Rust (`#forbid(unsafe)`)** | **YES** | **Tier 1 (SAS)** | LBI (Rust ownership) + Shared Space | Zero-Trap SPSC Ring Buffer / Grant |
| **Driver Cell (Audited MMIO/DMA)** | **YES** | **Tier 1 (SAS)** | IOMMU + Audited Unsafe Surface | Direct MMIO / DMA Buffers |
| **Rust (Unsigned / Debug)** | **NO** | **Tier 2 (Domain)** | Hardware Page Table (`satp`/`CR3`) | Syscall Trap + Bounded Copy |
| **C / C++ / POSIX (mlibc)** | Any | **Tier 2 (Domain)** | Hardware Page Table (`satp`/`CR3`) | Syscall Trap + Bounded Copy |
| **Linux OS Guest Image** | Any | **Tier 3 (VM)** | Stage-2 Hypervisor Paging | VirtIO Virtual Network / Block |

### KASLR — Shipped (Phase 24)
**Severity: Resolved**

**Shipped (2026-06-05, Phase 24)**: KASLR via Limine boot randomization. The boot chain is
OpenSBI → Limine → kernel, the kernel is built PIE (`-C relocation-model=pic -C link-arg=-pie`
via `kernel/build.rs`), and `limine.conf` sets `KASLR=yes` so consecutive boots load the kernel
at different physical bases. Verified across the integration test suite.

### x86-64 Privileged Entry — Hardened (2026-10-31)
**Severity: Resolved for the entry paths listed; SMAP open**

Applies to every tier: LBI does not make hardware exception delivery safe.

- **IST stacks.** NMI, #DF and #MC use per-CPU IST1/IST2/IST3 stacks (`hal/arch/x86/src/x86_64/ist.rs`),
  8 KiB each, with an unmapped guard page on both ends. An NMI in the SYSCALL window (after `swapgs`,
  before the kernel stack load) or after SYSRET's user-RSP restore no longer writes through an
  untrusted RSP, and a stack overflow ends in a diagnosed #DF instead of a triple fault. These vectors
  stay fatal; their path does not read GS, so it is safe in every `swapgs` window.
- **SYSCALL flags.** `IA32_FMASK` clears IF, TF, DF, AC, NT and IOPL. Previously it masked only TF/IF,
  so a caller's DF reached Rust code. SYSRET restores the caller's flags.
- **Frame decode.** 64-bit delivery always pushes SS:RSP; the saved RSP is now read for kernel-origin
  entries too.
- **Opt-in SMEP/UMIP** (`x86-smep`, `x86-umip` features): off by default; a requested protection the
  CPU lacks refuses boot or AP admission. **SMAP** (`x86-smap`) has the mechanism (CLAC on IDT entry,
  AC cleared on SYSCALL, `protect::user_access` windows) but no qualifying kernel profile: the SAS
  kernel writes U=1 pages directly (first site: user-stack zeroing in
  `Scheduler::spawn_with_stacks_configured`). Enabling it requires migrating those accesses into
  bounded windows without adding copies.
- Evidence: `scripts/qemu-x86_64-idt-test.sh` and `scripts/x86/qemu-idt-protection-test.sh`
  (QEMU/TCG only; no physical Intel or real machine-check evidence).

### Per-Cell DMA Isolation — Shipped (2026-06-22)
**Severity: Resolved (residual gap for untrusted FFI/native code — see below)**

The IOMMU was upgraded from bare passthrough (`DDTP.MODE=1` / VT-d single shared passthrough domain,
IOVA==PA, no permission table) to **per-Cell translate mode** on both architectures:

- **RISC-V** (`kernel/src/task/drivers/iommu_riscv.rs`): 3-level DDT (`DDTP.MODE=3LVL`), a per-Cell Sv39
  translation domain, a unique **PSCID** per Cell (with a free-list to survive Cell restarts), and
  `IOTINVAL.VMA` / `IOFENCE.C` / `IODIR.INVAL_DDT` invalidation.
- **x86** (`kernel/src/task/drivers/iommu_x86.rs`): a per-Cell VT-d second-level page table (`VtdSlpt`) with
  its own DID, one lazily initialized context table per PCI bus, exact canonical BDF tracking, ECAP-computed
  IOTLB offsets, and context-cache plus PSI/DSI IOTLB invalidation. Publishing a new bus context while
  translation is active requires acknowledged global context-cache invalidation followed by acknowledged
  global IOTLB invalidation with read/write drains before `GrantDma` succeeds. If invalidation is not
  acknowledged, authorization fails and the newly pinned frames remain quarantined rather than reclaimed.

DMA authorization is now a first-class capability: the new **`sys_grant_dma` (233)** syscall grants a Cell a
BDF plus a DMA-mapping quota (1× its memory quota, page-aligned), enforced by `can_map_dma()` +
`record_dma_mapped/unmapped()`. On Cell exit, `cleanup_cell()` (Exit / ForceExit / watchdog paths) clears every
exact BDF context, then invalidates the context cache, invalidates and drains the IOTLB, and only then permits
frame reclaim. Peripherals default to the kernel domain; a userspace Driver Cell must explicitly request
authorization. This closes the Thunderclap (NDSS 2019) class
of attack for PCIe DMA-capable devices — the blast radius of a compromised Driver Cell is now confined to its
own granted pages, not all of physical memory.

**Key distinction preserved**: MMIO ownership ≠ DMA authorization. The Resource Registry enforces exclusive
MMIO ownership; DMA capability is tracked **separately** via `sys_grant_dma` and per-device, per-Cell IOMMU
translation entries.

> ⚠️ **Residual gap (honest):** the IOMMU fronts only the **PCIe root complex**. **virtio-mmio** devices are
> **not** behind the IOMMU (see [specs/15-kernel-boundary.md](specs/15-kernel-boundary.md) §1.4). A native
> C/Zig Cell that can issue raw MMIO writes to a virtio-mmio device can still program its virtqueue with
> arbitrary physical addresses, and the device will DMA there. This remains open for **untrusted FFI/native code**;
> the load-bearing roadmapped closure is a Spec 19 Layer-B CPU page-table domain plus
> IOMMU/WorldGuard coverage of virtio-mmio DMA. PMP is only a future alternative if a custom
> M-mode firmware owner is approved; Cellos S-mode cannot program it. See
> [research/research-hardware-isolation.md](research/research-hardware-isolation.md) §3.

### Forward-Edge CFI (BTI / CET-IBT) — Shipped (2026-06-23)
**Severity: Resolved**

Forward-edge CFI closes the gap where a corrupted indirect branch jumps anywhere in the SAS. The five-phase
hardware-supplement implementation plan delivered the following components, but their enforcement status differs:

- **ARM64 BTI + PAC-RET** — `SCTLR_EL1.BT0/BT1` + `APIAKEY_EL1` init, compiled with `+bti,+paca,+pacg`,
  runtime-detected via `ID_AA64PFR1_EL1` / `ID_AA64ISAR1_EL1`. Covers both forward edge (BTI) and backward
  edge (PAC-RET return addresses).
- **ARM64 MTE** — `ViMte` trait, `SCTLR_EL1.ATA/TCF` config, `STGP` tag writes, sync/async fault modes; hardware-gated and unavailable on RK3588's Armv8.2-A Cortex-A76/A55 cores.
- **x86 CET-IBT** — `CR4.CET` + `MSR_IA32_S_CET` ENDBR_EN, `ENDBR64` landing pads on all ring-3 stubs, `#CP`
  (IDT vector 21) handler.
- **x86 PKU plumbing** — `CR4.PKE`, task PKRU values, and `WRPKRU` guards on `iretq`/`sysretq`,
  with CET-IBT enforced as a hard prerequisite. This is not yet page isolation.

> ⚠️ **One G2 follow-up:** PKU is wired but PTE key tagging is deferred — the loader does not yet stamp
> per-Cell keys into PTE bits [62:59], so keys are all-zero and PKU **enforcement is bypassed** until then.
> The current self-test checks PKRU constants and kernel `RDPKRU`; it does not attempt a
> denied keyed-page access. CET-IBT (already enforced) covers the JOP-gadget threat, not
> the absent page-key enforcement. See
> [research/research-hardware-isolation.md](research/research-hardware-isolation.md) §2 and roadmap §G.

### Audit Log — Shipped (Phase 26)
**Severity: Resolved**

**Shipped**: a kernel audit ring buffer (`kernel/src/audit.rs`) records security-relevant events —
`IpcSend`, `CellFault`, `CellExit`, `CellMeasure`, and others — providing an in-kernel trail for forensic
analysis after an incident.

### Capability Token System — Implementation Gap
**Severity: Medium**

Spec (01-core.md) describes unforgeable Zero-Sized Type capability tokens. Current implementation uses an ELF
manifest of one `flags: u8` (8 boolean flags, **all used**) plus a `can_block_io` TCB flag — coarse, granted
all-at-spawn, with **no scoping, no delegation, no revocation, no user/operator consent**. This is effectively
the **Android pre-6.0 install-time permission model**, and violates all four capability-OS invariants (no
ambient authority · explicit delegation · monotonic downgrade · revocable).

**Planned** (see [research/research-cell-security-permissions.md](research/research-cell-security-permissions.md)
for the full design + capability-OS / mobile-OS references): evolve through (1) **parameterized capabilities**
(`__Cellos_cap_args` ELF section — e.g. "GPIO pins 14-17" not "all GPIO"; additive, no Law 1 bump), (2)
**spawn-time intersection** (a Cell can only delegate caps it holds — kills confused-deputy), (3) **runtime
revocation** (`CapHandle` + `sys_cap_revoke`), (4) **operator-signed policy** for headless G1 fleets (consent =
signed policy, NOT a dialog — see the headless-robot caveat) and an optional **TCC-style consent-broker Cell**
for G2 HMI (sensitive caps only). Hard invariant: the manifest is a **ceiling, not a floor**, and **only the
kernel enforces** (consent feeds the syscall-boundary check). LBI already closes the TCC "permission-laundering
via code injection" hole that produced repeated macOS/iOS TCC CVEs.

### Boot Trust Chain + Attestation — Partial (measurement + signing shipped)
**Severity: Medium (High for fleet deployment)**

**Shipped**: two of the building blocks are now in place.
- **Per-Cell SHA-256 measurement (2026-06-21)** — every ELF is hashed at `spawn_from_path()` and extended
  into an append-only kernel measurement log (Linux IMA model) *before* the Cell is scheduled
  (`kernel/src/sha256.rs`, `kernel/src/measurement_log.rs`; emitted as a `CellMeasure` audit event).
- **Ed25519 cell-signing verify-at-spawn (2026-06-23; key separation 2026-10-31; postures 2026-11-05)** —
  the loader extracts the `__ViCell_sig` section and verifies it against the kernel's single trust anchor.

  | Posture (kernel features) | Cell trust anchor | Unsigned cell | Bad signature | Absent policy |
  |---|---|---|---|---|
  | `default` (dev/test: `dev-signing-key`, `dev-policy-key`, `dev-weak-rng`) | public dev seed `0x43*32` | Tier 2 | denied | dev-permissive |
  | `release-posture` = `release-signing-key` + `signing-required` + `policy-required` | `CELLOS_RELEASE_CELL_PUBLIC_KEY` (private key a CI secret) | **denied** | denied | fail-closed |
  | `production-relay-image` = `release-signing-key` + `signing-required` + `policy-required` | same release key | **denied** | denied | fail-closed |

  `release-signing-key` cannot be combined with `dev-signing-key` (build error), so a dev-signed artifact
  is refused by a release kernel; both release postures build with `--no-default-features`.
  `signing-required` without any anchor feature (`dev-signing-key`, `release-signing-key`,
  `phase02-terminal-profile`) is a build error: the zero placeholder anchor would refuse every signed
  cell while still admitting unsigned ones. `.github/workflows/release.yml` builds `release-posture` and
  fails when the release-key secret is empty; `scripts/check-production-relay-image.py` requires
  `release-signing-key` for relay candidates.

  Images from caller memory (`SpawnFromElf`, `CellSessionV1` route 2, `SpawnFromMem`) are copied into a
  kernel-owned buffer before hashing, verification, manifest classification and loading (2026-11-05).
  The grant routes copy through the caller's `TaskCopyView` under the grant-table lock: later writes to the
  grant never reach verification or loading, and the grant cannot be freed mid-copy.
- **TLS trust anchor (service-net `tls-ca-private`, 2026-11-05)** — `cells/services/net/roots/private.der`
  is a dev CA whose key exists only on the machine that generated it (`tools/hypha-mock-llm/gen-dev-ca.py`,
  key written 0600 and gitignored); fleet builds must replace it. The previous dev CA's key was committed
  and remains in git history, so its certificate (sha256 `36335e76…775925d1`) is refused by
  `cells/services/net/build.rs` and by `scripts/check-production-relay-image.py` (root file and any
  artifact that embeds it).
- **What the normal signing workflow means (2026-07-30, phase 11)** — `scripts/cellos-sign` checks the
  repository's F1/F5 policy before its normal signing call: it verifies that every Cell crate root carries
  `#![forbid(unsafe_code)]`, that no tracked `.rs` file under `cells/` contains `unsafe` outside
  `scripts/unsafe-allowlist.toml`, and that the running rustc is the one `rust-toolchain.toml` pins (F5).
  Every image lane signs through it — `scripts/lib-sign-cells.sh` for the bash scripts, `gen_disk.ps1` for
  the Windows lane — and CI runs the same `cellos-sign --check`. F5 is mandatory on the signing path: a host
  that cannot verify the toolchain refuses to sign rather than signing with a `SKIP`. The low-level
  `scripts/sign-cell.py` refuses to sign at all unless `cellos_sign.signing` has marked the check as passed,
  or the caller passes the explicit `--unchecked-dev-signature` opt-in. That route and the known dev seed
  are test fixtures, not security boundaries: the kernel cannot distinguish an unchecked dev signature
  from a checked one. The CLI also accepts target ELF paths after checking the source tree; a controlled
  CI/KMS pipeline must separately bind those artifacts to the checked build.

  **Do not oversell this.** It defends against unintentional mistakes in the trusted image workflow. A
  fleet claim additionally requires an immutable provisioned public key, mandatory signing/policy features,
  no dev-key/weak-RNG features, controlled build provenance, negative admission tests, and secure boot.
  Tier 2 remains the future wall for native code that does not receive that approval.

**Still open** (see [research/research-cell-security-permissions.md](research/research-cell-security-permissions.md)
§3): full **secure/measured boot** and a **device attestation** story — a TPM-free **DICE/RIoT** layered chain
(`CDI_n = HKDF(CDI_{n-1}, HASH(layer_n))`), **remote attestation** via EAT tokens (RFC 9711) + RATS (RFC 9334)
verified by ARM Veraison fleet-side, and **sealed storage** with the AEAD key held in the **Silo** (closes the
CDI-in-RAM exposure). Hardware root of trust: **OpenTitan** (open-source RISC-V) is the natural backing for the
existing Silo abstraction. A robot fleet still cannot yet cryptographically prove end-to-end that a device runs
unmodified Cellos, nor bind secrets to a measured boot state.

## Hardware Isolation Roadmap

> **Full menu + status:** [research/research-hardware-isolation.md](research/research-hardware-isolation.md).
> This section keeps only the CHERI sub-roadmap; CFI / MPK-PKS / MPU-PMP / WorldGuard-Smmtt / IOMMU-IOPMP /
> confidential-computing supplements live in the research doc and the project roadmap §G.

### CHERI sub-roadmap

**CHERIoT** (Capability Hardware Extension RISC-V for IoT) là extension RISC-V cung cấp **hardware-enforced pointer bounds** — kết hợp hoàn hảo với Rust LBI của Cellos.

### Tại sao CHERI quan trọng với Cellos

| Cơ chế | Rust LBI (hiện tại) | CHERI + Rust LBI |
|--------|---------------------|-----------------|
| Bounds checking | Compile-time only | Compile-time **+** hardware runtime |
| `unsafe` blocks | Không được kiểm soát bounds | Hardware trap nếu pointer ra ngoài bounds |
| Spectre gadgets | Không mitigate | Capability bounds giới hạn speculative access |
| Pointer forgery | Compiler ngăn trong safe Rust | Hardware ngăn kể cả trong `unsafe` |
| Use-after-free trong HAL | Phụ thuộc code review | Hardware trap ngay lập tức |

**Kết luận**: Rust LBI + CHERI = defense-in-depth thực sự. Rust bắt lỗi lúc compile; CHERI bắt lỗi còn sót lại lúc runtime — kể cả trong kernel `unsafe` blocks.

### Silicon availability (2026)

| Platform | Status | CHERI Type |
|----------|--------|-----------|
| **CHERIoT-IBEX** (lowRISC/Microsoft) | ✅ Sonata FPGA (~$412); SCI ICENI silicon Early-Access 2025; Rust no_std fork active (cập nhật hằng tuần từ 2/2026) | RV32E (embedded) |
| **Morello** (ARM) | ❌ **ARM tuyên bố KHAI TỬ** — không sản phẩm, không kế thừa có tên (eval ~20-35% overhead) | AArch64 CHERI (EoL) |
| **RISC-V "Zcheri" extension** | ❌ **Chưa ratify** (target đầu 2026, đã trượt) | RV32CH/RV64CH |
| **CHERI-RISC-V RV64** (Cambridge / COSMIC) | 🔶 FPGA only; COSMIC nhắm secure-enclave 3/2028, chưa tape-out | RV64 full CHERI |

> **Thực tế (2026)**: CHERI cho RV64 (target chính của Cellos) **chưa có silicon, chưa có Rust target, ISA chưa ratify**
> — KHÔNG khả thi cho 2026-Q4; realistic 2028-2030. ARM Morello đã bị khai tử. **CHERIoT-IBEX là RV32E** và là path
> duy nhất chín muồi — phù hợp **Cellos-Nano** profile (embedded robots). Compartment switch đo được 209-452 cycle
> (nhanh hơn null syscall, SOSP 2025).

### Integration Path với Cellos (Phase 31)

```
Bước 1: HAL arch mới
  cells/hal/arch/cheriot32/      # CHERIoT-IBEX RV32 target
  - Capability registers thay thế VAddr/PAddr
  - Memory tagging qua hardware capability table

Bước 2: libs/types thay đổi
  #[cfg(feature = "cheri")]
  pub type VAddr = CheriCapability;  // hardware capability
  pub type PAddr = CheriCapability;

Bước 3: Rust toolchain
  - Dùng CHERIoT-Platform/rust fork (rustc CHERI support)
  - Target: riscv32cheriot-unknown-unknown
  - Không cần thay đổi Tier 1 cell code (Rust LBI vẫn hoạt động)

Bước 4: Kernel unsafe blocks
  - Mỗi unsafe block tự động được hardware bounds-check
  - SAS attack surface giảm từ "toàn bộ address space"
    xuống "chỉ các capabilities được cấp phép"
```

### Prerequisites (Phase 31)

- [ ] Mua Sonata development board (CHERIoT-IBEX, ~$50)
- [ ] Xác nhận CHERIoT-Platform/rust build cho no_std Cellos target
- [ ] Thiết kế `feature = "cheri"` flag trong libs/types không breaking existing RV64 code
- [ ] Benchmark: overhead của CHERI bounds check vs. phần mềm Rust LBI

**Target**: Phase 31 (2026-Q4) cho Cellos-Nano profile trên Sonata board.

---

## Known Limitations

1. **Spectre v1/v2:** The SAS model means kernel and all Cells share a
   virtual address space.  Spectre-class microarchitectural leakage is
   inherent.  Mitigation (retpoline, IBRS, CSR flushing) is deferred to
   Phase 12 hardening.

2. **KASLR:** *Shipped (Phase 24).* Limine randomizes the kernel load base
   (kernel built PIE, `KASLR=yes`).

3. **Trusted Cells:** All installed Tier 1 Cells are fully trusted (now enforced by
   Ed25519 verify-at-spawn + SHA-256 measurement). Untrusted third-party code belongs
   in **Tier 3 (Linux VM / hypervisor)** today. Tier 2 is the accepted native private-
   MMU-domain class but is not implemented; an unsigned or unverified artifact is an
   admission input, not the definition of Tier 2. See
   [specs/18-cell-trust-tiers.md](specs/18-cell-trust-tiers.md). See Phase 23 for
   community submission review gates.

4. **Audit log:** *Shipped (Phase 26).* Cell actions are recorded in the kernel
   audit ring buffer (`kernel/src/audit.rs`).

5. **sshd key files under the dev posture:** `/srv/sshd/**` is bound to a signed image
   named `sshd` (see Spoofing). With the release anchor only the release pipeline can
   produce such an image. With the dev anchor the signing seed (`[0x43]*32`) is public,
   so any Cell able to spawn from caller-supplied bytes can sign its own image as `sshd`
   and read the host seed or add a key. Raw block access (VFS, block driver) and offline
   disk access are outside the rule. The private profile anchor
   (`phase02-terminal-profile`) is not admitted. `/bin/sshd` is excluded from x86 images.

## Defense in Depth

| Layer | Mechanism |
|-------|-----------|
| Language | `#![forbid(unsafe_code)]` on all Cell crates |
| Compile-time | Rust ownership, borrow checker, lifetimes |
| Kernel | Capability table, syscall argument validation, frame zeroing |
| CI | `cellos-sign --check --strict` (F1/F5 admission), `cargo-audit`, `cargo-deny` on every PR |
| Fuzzing | Weekly libFuzzer harnesses on ELF parser + VFS path validator |
| HW — spatial | MTE hardening is available on Armv8.5+/QEMU, **not RK3588**; x86 PKU register/return-path plumbing exists but keys remain all-zero and enforcement is absent; PMP needs a future M-mode owner |
| HW — control-flow | ✅ **Shipped**: BTI+PAC-RET (ARM), CET-IBT (x86); Zicfilp/Zicfiss (RISC-V) _(roadmap)_ |
| HW — DMA | ✅ **Shipped**: IOMMU translate mode (RISC-V 3LVL DDT / x86 VT-d per-Cell with per-bus context tables) + per-Cell `sys_grant_dma` (**not** MMIO ownership). virtio-mmio DMA + IOPMP coverage _(roadmap)_ |
| HW — VM-grade _(roadmap)_ | Stage-2/EPT (Tier 3); TDX/SEV-SNP/ARM CCA for attested multi-tenant |

> Spec 19 owns the Layer A/B/C hardware-isolation taxonomy. The research document rates
> candidate mechanisms against the SAS "no-TLB-flush-per-switch" criterion:
> [research/research-hardware-isolation.md](research/research-hardware-isolation.md).

## Security Contacts

For vulnerability reports, open a GitHub Issue with label `security`.
Critical issues (RCE, privilege escalation): email directly (see SECURITY.md).
