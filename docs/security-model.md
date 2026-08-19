# Cellos Security Model

**Version:** v0.2.3-dev | **Updated:** 2026-07-07

## Design Philosophy

Cellos uses a **Cellular Single Address Space (SAS)** model with
Language-Based Isolation (LBI) via Rust's type system.  Traditional OS
security relies on hardware MMU separation between processes; Cellos instead
relies on:

1. **Rust ownership + borrow checker** — prevents spatial/temporal memory bugs
2. **Capability tokens (`CapId`)** — unforgeable, kernel-managed access rights
3. **`#![forbid(unsafe_code)]` on Cells** — enforced at the signing gate by
   `scripts/cellos-sign --check` (both CI workflows), absolute outside a reviewed allowlist
   (`scripts/unsafe-allowlist.toml`); each allowlisted file is an approved hole in the LBI
   wall, not an exempt no-op
4. **VFS access through capabilities** — no direct file-descriptor integers

## STRIDE Threat Model

### Spoofing
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell forges another Cell's CellId in IPC | Kernel verifies sender ID from TCB on every message; user cannot inject arbitrary sender values | ✅ Mitigated |
| Cell guesses a service-local socket CapId | The net service currently allocates predictable CapIds without binding lookup to the caller, so another network-capable Cell may address a live socket owned by a peer | ❌ Open — see [roadmap/open-risk-register.md](roadmap/open-risk-register.md) |
| Malformed ELF binary spawns as a different Cell | ELF header validated before execution; Cell registry assigns IDs monotonically | ✅ Mitigated |

### Tampering
| Threat | Mitigation | Status |
|--------|-----------|--------|
| Cell writes to another Cell's memory | SAS + Rust ownership; no `unsafe` in cells/ | ✅ Mitigated |
| Cell modifies a revoked capability | Kernel removes CapId from table on Close; subsequent ops return PermissionDenied | ✅ Mitigated |
| Attacker modifies disk image to inject malicious ELF | Every spawn source reaches the same SHA-256 measurement + Ed25519 verification hook. A present invalid signature is denied, but default G1 leaves `signing-required` off: stripping `__ViCell_sig` turns the image into an admitted unsigned SAS cell. The reproducible dev seed is forgeable and no production public-key provisioning path exists yet. | ⚠️ Partial — mechanism shipped; fleet enforcement open |

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
| `#[allow(unsafe_code)]` in a Cell | `scripts/cellos-sign --check --strict` fails CI when a Cell file contains `unsafe` and is not in the reviewed allowlist, and refuses to sign; allowlist entries carry reason/approver/date and are reported once past `review_by` | ✅ Mitigated, with a bounded hole — see `docs/specs/18-cell-trust-tiers.md` §2.1 |
| Malformed syscall arguments overflow kernel buffers | All syscall arg lengths validated via `validate_user_buf` before dereference | ✅ Mitigated |

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

**Do NOT use Cellos to run untrusted third-party code until either containment tier is
implemented: Tier 2 (per-domain page tables for unsigned native cells) or Tier 3 (VM).**
Neither exists today — the kernel holds a single root page table (`memory/paging.rs:38`
`KERNEL_ROOT`) and no context switch writes `satp`/`TTBR0`/`CR3`, so an unsigned cell in the
shared address space is contained by nothing but the Rust type system. Tier 2 is the
designed answer (`specs/18-cell-trust-tiers.md` §2.2) and does not reverse this warning; it
narrows what the warning will say once the mechanism ships.

> **Full analysis:** [research/research-hardware-isolation.md](research/research-hardware-isolation.md) — covers the
> full menu of hardware supplements (CFI, MPK/PKS, MPU/PMP, RISC-V WorldGuard/Smmtt, IOMMU/IOPMP, confidential
> computing, CHERI), each rated against the SAS "no-TLB-flush-per-Cell-switch" criterion, plus peer-OS prior art
> (Tock, Hubris, RedLeaf, Theseus, Singularity, CheriOS) and a severity-ranked gap list.

> **Isolation strategy decision (2026-06-05):** per-Cell **SATP** isolation at Tier 1 is
> **explicitly NOT pursued**. PMP is M-mode-only (unreachable from Cellos's S-mode without
> custom firmware) and sPMP is unratified; per-cell SATP would break Tier 1 zero-copy IPC.
> Hardware isolation available **today** is **Tier 3 Stage-2 paging (per-VM)**, so untrusted
> third-party code is confined to **Tier 3 (Linux VM / hypervisor)** for now. **Tier 2**
> (accepted, **not yet implemented**) will run an unsigned native cell in its own page-table
> domain — same cell shape and SDK as Tier 1, contained by the MMU instead of by trust — see
> [specs/18-cell-trust-tiers.md](specs/18-cell-trust-tiers.md) §2.2. Note that the decision
> not to pursue per-Cell SATP applies to **Tier 1**, where it would break zero-copy IPC; it
> is not an argument against Tier 2, which pays exactly that cost deliberately, in exchange
> for needing no trust in the code it contains. Ed25519 verification runs at the common
> loader spawn gate (`kernel/src/signing.rs` + `loader.rs`), but the default G1 posture is
> **not signed-only**: an absent signature is admitted to the SAS because
> `signing-required` is off. `/bin/` remains an authorization label, and signature status
> does not select a memory tier. See Specs 12 and 18.

### KASLR — Shipped (Phase 24)
**Severity: Resolved**

**Shipped (2026-06-05, Phase 24)**: KASLR via Limine boot randomization. The boot chain is
OpenSBI → Limine → kernel, the kernel is built PIE (`-C relocation-model=pic -C link-arg=-pie`
via `kernel/build.rs`), and `limine.conf` sets `KASLR=yes` so consecutive boots load the kernel
at different physical bases. Verified across the integration test suite.

### Per-Cell DMA Isolation — Shipped (2026-06-22)
**Severity: Resolved (residual gap for untrusted Tier-1b — see below)**

The IOMMU was upgraded from bare passthrough (`DDTP.MODE=1` / VT-d single shared passthrough domain,
IOVA==PA, no permission table) to **per-Cell translate mode** on both architectures:

- **RISC-V** (`kernel/src/task/drivers/iommu_riscv.rs`): 3-level DDT (`DDTP.MODE=3LVL`), a per-Cell Sv39
  translation domain, a unique **PSCID** per Cell (with a free-list to survive Cell restarts), and
  `IOTINVAL.VMA` / `IOFENCE.C` / `IODIR.INVAL_DDT` invalidation.
- **x86** (`kernel/src/task/drivers/iommu_x86.rs`): a per-Cell VT-d second-level page table (`VtdSlpt`) with
  its own DID, ECAP-computed IOTLB offsets, and PSI/DSI IOTLB + context-cache invalidation.

DMA authorization is now a first-class capability: the new **`sys_grant_dma` (233)** syscall grants a Cell a
BDF plus a DMA-mapping quota (1× its memory quota, page-aligned), enforced by `can_map_dma()` +
`record_dma_mapped/unmapped()`. On Cell exit, `cleanup_cell()` (Exit / ForceExit / watchdog paths) tears down
the domain and issues `IOFENCE`/IVT flush **before** frame reclaim. Peripherals default to the kernel domain;
a userspace Driver Cell must explicitly request authorization. This closes the Thunderclap (NDSS 2019) class
of attack for PCIe DMA-capable devices — the blast radius of a compromised Driver Cell is now confined to its
own granted pages, not all of physical memory.

**Key distinction preserved**: MMIO ownership ≠ DMA authorization. The Resource Registry enforces exclusive
MMIO ownership; DMA capability is tracked **separately** via `sys_grant_dma` and per-device, per-Cell IOMMU
translation entries.

> ⚠️ **Residual gap (honest):** the IOMMU fronts only the **PCIe root complex**. **virtio-mmio** devices are
> **not** behind the IOMMU (see [specs/15-kernel-boundary.md](specs/15-kernel-boundary.md) §1.4). A Tier-1b
> C/Zig Cell that can issue raw MMIO writes to a virtio-mmio device can still program its virtqueue with
> arbitrary physical addresses, and the device will DMA there. This remains open for **untrusted Tier-1b**;
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
- **Ed25519 cell-signing verify-at-spawn (2026-06-23)** — the loader extracts the `__ViCell_sig` section and
  verifies a present signature before spawning. Invalid signatures fail closed; an absent signature fails
  only under the non-default `signing-required` feature. The default dev seed is public/reproducible, while
  disabling it selects a `[0u8; 32]` placeholder; no production public-key provisioning path exists yet.
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
   in **Tier 3 (Linux VM / hypervisor)**. Tier 2 runs unsigned native cells in a
   private MMU protection domain — see
   [specs/18-cell-trust-tiers.md](specs/18-cell-trust-tiers.md). See Phase 23 for
   community submission review gates.

4. **Audit log:** *Shipped (Phase 26).* Cell actions are recorded in the kernel
   audit ring buffer (`kernel/src/audit.rs`).

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
| HW — DMA | ✅ **Shipped**: IOMMU translate mode (RISC-V 3LVL DDT / x86 VT-d per-Cell) + per-Cell `sys_grant_dma` (**not** MMIO ownership). virtio-mmio DMA + IOPMP coverage _(roadmap)_ |
| HW — VM-grade _(roadmap)_ | Stage-2/EPT (Tier 3); TDX/SEV-SNP/ARM CCA for attested multi-tenant |

> Spec 19 owns the Layer A/B/C hardware-isolation taxonomy. The research document rates
> candidate mechanisms against the SAS "no-TLB-flush-per-switch" criterion:
> [research/research-hardware-isolation.md](research/research-hardware-isolation.md).

## Security Contacts

For vulnerability reports, open a GitHub Issue with label `security`.
Critical issues (RCE, privilege escalation): email directly (see SECURITY.md).
