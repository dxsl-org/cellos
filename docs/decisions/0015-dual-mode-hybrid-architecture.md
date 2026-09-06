# ADR-0015: Settle on Dual-Mode Hybrid Architecture (Real-time SAS Tier 1 + Paged Domain Tier 2 + VM Guest Tier 3)

**Date**: 2026-09-06  
**Status**: Accepted — definitive architectural baseline for Cellos  
**Decider**: Cellos maintainer, through explicit user approval in session  

---

## 1. Context

Cellos originally conceived a pure **Single Address Space (SAS)** organized around **Language-Based Isolation (LBI)** via Rust's compile-time type system rather than hardware MMU separation.

However, deep architectural root-cause analysis (`.agents/260905-1139-sas-lbi-outcome-closure/sas-lbi-architecture-root-cause-analysis.md`) revealed critical structural contradictions:
1. **LBI Erosion via Unsafe Allowlist**: Over 550 lines of allowlist exemptions (`scripts/unsafe-allowlist.toml`) permit raw C-FFI (mlibc, Doom, Lua) and driver MMIO to run uncontained in the shared SAS. A single buffer overflow or null pointer dereference in C code can corrupt kernel structures and collapse the entire OS.
2. **Missing Tier 2**: The architecture documents specified a 3-tier hierarchy, but Tier 2 (Domain Paged Cell) was never implemented in runtime code (`docs/system-architecture.md:138`). Unsigned and foreign code was either pushed into a heavyweight Linux VM (Tier 3) or admitted uncontained into Tier 1.
3. **Unsigned Code Vulnerability**: Default G1 posture left `signing-required = OFF`, allowing arbitrary unsigned ELF binaries into the SAS. At the machine-code level, CPUs execute raw instructions without knowledge of Rust safety invariants.
4. **Memory Footprint Bloat**: Fixed reservations (`HEAP_FRAMES = 8_192` = 32 MiB) and early static allocations led to a measured footprint of 79.69 MiB on QEMU, exceeding the `< 10 MiB` target for G1 Robot & Embedded.

---

## 2. Decision

We definitively settle on **Option B: Dual-Mode Hybrid Architecture** as the immutable architectural foundation of Cellos.

```text
+-----------------------------------------------------------------------------------------+
|                                     CELLOS HYBRID OS                                    |
+-----------------------------------------------------------------------------------------+
|  TIER 1: Real-time SAS (Zero-Copy)                                                      |
|  - 100% Safe Rust Cells (Robot loops, VFS core) + Audited Driver Cells (MMIO/DMA)       |
|  - Shared KERNEL_ROOT, no page table switch, zero TLB flush                             |
|  - Fastpath IPC: SPSC Lock-Free Ring Buffer (P99 <= 10µs)                                |
|  - Mandatory Ed25519 signing (signing-required = ON)                                    |
+-----------------------------------------------------------------------------------------+
|  TIER 2: Paged Domain Engine (Hardware Memory Isolation)                                |
|  - Private Page Tables (SATP on RISC-V, CR3 on x86, TTBR0 on ARM)                       |
|  - Mandatory home for: ALL C-FFI (Doom, Lua, mlibc), ALL Unsigned Binaries (even Rust)  |
|  - Hardware MMU traps page faults (SIGSEGV); Cell reaped safely without crashing SAS   |
|  - IPC Bridge: Microkernel Syscall Traps + validate_user_buf boundary copies            |
+-----------------------------------------------------------------------------------------+
|  TIER 3: Hardware VM Guest (Full OS Virtualization)                                     |
|  - Stage-2 Paging / EPT / Hardware Hypervisor                                           |
|  - Runs unmodified Linux (Alpine, Nginx, ROS2, Python)                                  |
+-----------------------------------------------------------------------------------------+
```

### 2.1 Tier 1: Real-Time SAS (Pure LBI + Audited Drivers)
- **Admission Gate**: Mandatory valid Ed25519 signature (`__ViCell_sig`) attesting compile-time `#![forbid(unsafe_code)]`. Unsigned binaries are unconditionally rejected from Tier 1.
- **Audited Kernel Driver Surface**: Audited Driver Cells (VirtIO, NVMe, e1000) that require direct volatile MMIO and DMA pointer operations remain in Tier 1 SAS under strict IOMMU translation authorization. They are distinct from user-facing C-FFI and are governed by dedicated kernel boundary laws.
- **Fastpath IPC**: Inter-cell communication between Tier 1 cells on the same Hart utilizes lock-free shared memory SPSC ring buffers, eliminating syscall trap overhead.

### 2.2 Tier 2: Paged Domain Engine (Hardware MMU Contained)
- **Admission Gate**: All unsigned binaries (including unsigned Rust developed during debug/test cycles), all C/C++ FFI applications, and all dynamic script runtimes (Lua, MicroPython).
- **Isolation Mechanism**: Dedicated hardware page tables per domain. Kernel space is mapped Supervisor-only / NX. User space is private.
- **Fault Containment**: Memory violations (null dereference, out-of-bounds write) trigger CPU Page Faults. The kernel terminates or restarts the offending Tier 2 Cell. SAS Tier 1 and the Kernel remain completely uncorrupted.
- **Programming Model**: Tier 2 cells use the exact same Native SDK and capability model (`CapId`) via Microkernel IPC traps.

### 2.3 Tier 3: Hardware Virtual Machine Guest
- Hardware hypervisor utilizing Stage-2 paging for legacy Linux distributions and cloud microservices.

---

## 3. Cell Admission Matrix

| Source & Language | Signed (`__ViCell_sig`) | Target Execution Tier | Isolation Mechanism | IPC Mechanism |
|---|:---:|:---:|---|---|
| **Safe Rust (`#forbid(unsafe)`)** | **YES** | **Tier 1 (SAS)** | LBI (Rust ownership) + Shared Space | Zero-Trap SPSC Ring Buffer / Grant |
| **Driver Cell (Audited MMIO/DMA)** | **YES** | **Tier 1 (SAS)** | IOMMU + Audited Unsafe Surface | Direct MMIO / DMA Buffers |
| **Rust (Unsigned / Debug)** | **NO** | **Tier 2 (Domain)** | Hardware Page Table (`satp`/`CR3`) | Syscall Trap + Bounded Copy |
| **C / C++ / POSIX (mlibc)** | Any | **Tier 2 (Domain)** | Hardware Page Table (`satp`/`CR3`) | Syscall Trap + Bounded Copy |
| **Linux OS Guest Image** | Any | **Tier 3 (VM)** | Stage-2 Hypervisor Paging | VirtIO Virtual Network / Block |

---

## 4. Consequences & Impact

- **Positive**:
  - Eliminates the fatal threat of unverified C code crashing the operating system.
  - Preserves the core technological identity and sub-microsecond zero-copy benefits of SAS for real-time robot control and native microservices.
  - Provides a frictionless path for developer experimentation: unsigned code runs safely in Tier 2; signing promotes it to Tier 1 without code changes.
  - Resolves the philosophical deadlock between pure LBI academic ideals and industrial C/POSIX compatibility.
- **Negative / Costs**:
  - Kernel must implement and maintain lightweight page-table allocation (`DomainPageTable`) and context switching for Tier 2.
  - Syscall handler must enforce recoverable user-pointer copying (`copy_from_user` / `copy_to_user`).

---

## 5. References
- Root-cause analysis: `.agents/260905-1139-sas-lbi-outcome-closure/sas-lbi-architecture-root-cause-analysis.md`
- Implementation plan: `.agents/260906-dual-mode-kernel-evolution/plan.md`
- Security model: `docs/security-model.md`
- Trust tiers spec: `docs/specs/18-cell-trust-tiers.md`
