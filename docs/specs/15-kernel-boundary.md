# Spec 15 — Kernel Boundary Law

> **Status**: Ratified 2026-06-23 — does not change without explicit architectural review.
>
> This document defines what belongs in the Cellos kernel and what does not. It is the
> authoritative reference. The summary in `CLAUDE.md` is derived from this spec.

---

## 1. Theoretical Basis

### 1.1 Liedtke's Minimality Principle (1995)

> *"A concept is tolerated inside the µ-kernel only if moving it outside the kernel,
> i.e., permitting competing implementations, would prevent the implementation of the
> system's required functionality."*
> — Jochen Liedtke, *On µ-Kernel Construction*, SOSP 1995

This is the sharpest kernel-boundary rule ever stated. We adopt it directly.

**Restated for Cellos:**

> A mechanism belongs in the kernel if and only if:
> **(a)** it requires hardware privilege level to execute (ISA mandates ring-0 / EL1), **or**
> **(b)** moving it outside would compromise the kernel's ability to enforce isolation or
>         capability integrity with bounded latency, **or**
> **(c)** it is the root of trust that other mechanisms depend on before they can load.

Everything that does not satisfy (a), (b), or (c) is kernel bloat and must be a Cell.

### 1.2 What LBI Can and Cannot Eliminate

Rust's type system (LBI — Language-Based Isolation) is Cellos's primary isolation
mechanism. But LBI has hard limits that kernel code cannot delegate away:

| LBI eliminates | LBI does NOT eliminate |
|----------------|----------------------|
| Memory unsafety between Cells | Hardware interrupt reception (ISA forces kernel-mode first receiver) |
| Pointer forgery | Physical frame allocation (SATP/TLB writes require EL1) |
| Stack/heap corruption across Cell boundaries | CPU time-multiplexing (timer IRQ arrives in kernel mode) |
| Type confusion attacks | Capability revocation (see §1.3) |
| — | Secure multiplexing of exclusive hardware resources |

**Critical: LBI does not provide capability revocation.**

The J-Kernel paper (Cornell, 1999) proves this formally: a type-safe language prevents
capability forgery but not *stale authority retention*. If Cell A is granted capability C
and that grant is later revoked, Cell A's Rust reference to the capability object does not
disappear — only the kernel can reach into Cell A's state and invalidate it. This is why
`CapSet` and manifest enforcement must live in the kernel, not in a userspace policy
server. LBI is complementary to capability enforcement; it is not a substitute.

**Consequence for Cellos:**
- "LBI isolates Cells" is true. It does NOT mean "the kernel can drop capability checks."
- The CapSet/TCB system is load-bearing security infrastructure. Do not weaken it by
  moving enforcement to userspace on the basis of "Rust already provides safety."

### 1.3 The Confused Deputy Problem

Hardy (1988) defines the *confused deputy*: a program with ambient authority (authority
derived from its identity rather than explicit grant) can be tricked into exercising that
authority on behalf of a malicious caller. LBI prevents memory corruption; it does not
track which capability granted which authority to which caller. A Cell can still be
manipulated into performing operations on behalf of an untrusted sender if it uses ambient
authority (e.g., a file descriptor it holds, a service it can reach) without checking
whether the sender was authorized to request that action.

**Consequence for Cellos:**
Capability checks at IPC boundaries (does the sender hold the right to request this
operation?) must be kernel-enforced or enforced in a trusted kernel-adjacent layer —
not left to individual Cell code. This is what the syscall gate in `syscall.rs` provides.

---

## 2. Kernel Whitelist — What BELONGS in the Kernel

### Category A: Hardware-Mandated (ISA forces EL1/ring-0)

These cannot leave the kernel on any ISA Cellos supports (RISC-V, ARM64, x86).

| Mechanism | Why kernel-only |
|-----------|----------------|
| Hardware interrupt reception + first dispatch | CPU vectors unconditionally to kernel on external IRQ; no ISA mechanism allows user-mode first receipt |
| Physical frame allocator | Writing SATP/CR3/TTBR + TLB flush = privileged instructions |
| Page table management (HHDM, cell VA mapping) | MMU hardware requires EL1 |
| Preemptive scheduler + context switch | Timer IRQ arrives at EL1; only kernel can preempt a running Cell (**note: seL4 also keeps this as "the one necessary exception"**) |
| HAL / arch (trap handlers, SBI/PSCI calls, GIC/PLIC) | Wraps privileged CPU instructions |

### Category B: Security Root-of-Trust (LBI insufficient — see §1.2, §1.3)

These cannot leave the kernel because LBI does not provide revocation or
confused-deputy protection.

| Mechanism | Why kernel-only |
|-----------|----------------|
| Capability grant / revoke / invoke (CapSet, TCB) | Kernel is the sole authority for capability integrity; userspace enforcement is self-defeating |
| Manifest verification at spawn | Signing check must complete before any Cell code runs |
| Audit log + operator policy enforcement | TCB: must be tamper-proof; a userspace audit cell can be bypassed |
| Cell lifecycle: registry, spawn gate, kill | Kernel enforces the unit of isolation itself; cannot delegate to a Cell that could be killed |

### Category C: Bootstrap Chicken-and-Egg

These are temporary kernel residents that would ideally move to Boot Cells but cannot
because they are needed to load the first Cells.

| Mechanism | Desired final home | Why still in kernel |
|-----------|-------------------|---------------------|
| ELF loader (minimal) | Boot Cell | Cannot use a Cell to load the first Cell |
| BootFS (FAT16 kernel-embedded) | Boot Cell / VFS Cell | VFS Cell needs a filesystem to load from before VFS is available |
| **Boot block device** (`virtio_blk` + its `virtio_pci` transport on x86) | Minimal boot reader in kernel + a Block Driver Cell for post-boot I/O (G2) | `loader.rs:spawn_from_path → block::read_sector` runs on EVERY spawn; the block device that holds the cell ELFs must be readable **before the first Cell exists**. A Block Driver Cell cannot serve the read that loads the Block Driver Cell. Same class as the ELF loader / BootFS above — a bootstrap root-of-trust (criterion **(c)**), **not** a §3 driver violation. |
| ACPI minimal parse (MADT + DMAR tables only) | Platform Cell | IOMMU init needs DMAR before any Cell spawns |

**Boot Cells (future):** Once a trusted Boot Cell infrastructure exists, ACPI full parsing
and PCIe ECAM enumeration move there. The kernel retains only what is needed to load the
Boot Cell itself.

> **Why NVMe/e1000 are Cells but `virtio_blk` is not** — a frequent confusion. NVMe and
> e1000 are *secondary* devices reached only after cells are running, so they migrated
> cleanly. `virtio_blk` is the *boot* device on the loader's critical path: it is read to
> load every cell, including any would-be block Driver Cell. The G2 goal is not "migrate
> virtio_blk" but "shrink the kernel to a **minimal** boot reader (ramdisk / first-sectors)
> and serve **post-boot** block I/O from a Cell" — an optimization of the bootstrap, not
> the removal of a violation.

### Category D: Hardware Exclusivity Arbiter

| Mechanism | Rationale |
|-----------|-----------|
| IOMMU init + `map_dma_for_cell` | In SAS there are no per-Cell address spaces; IOMMU is the **only** hardware boundary between Driver Cells and physical memory. Must be kernel-controlled. (Zircon model — deliberately not Genode 23.11 model, which requires per-process address spaces to be safe.) |

**IOMMU design note**: Genode 23.11 moved IOMMU programming to a userspace platform
driver. This is architecturally cleaner but requires process-level address space
isolation to be safe (if the platform driver is compromised, it can map any DMA region).
In Cellos's SAS, there is no MMU isolation between the platform driver and the kernel.
Keeping IOMMU in the kernel is the correct choice until per-Cell address spaces exist.

---

## 3. Kernel Blacklist — What MUST NOT be in the Kernel

The following categories are **forbidden** in the kernel. Any PR adding code in these
categories to `kernel/src/` must be rejected at review.

### 3.1 Device Drivers (all of them)

> **Universal consensus**: seL4, Fuchsia/Zircon, Genode, MINIX3, Redox, Singularity,
> Theseus — ALL exiled device drivers from the kernel. No exceptions in any production
> microkernel design.

| Driver category | Correct home |
|----------------|--------------|
| NVMe, MMC/SDHCI (storage) | `cells/drivers/nvme/`, `cells/drivers/mmc/` |
| e1000, RTL8168 (NIC) | `cells/drivers/e1000/`, `cells/drivers/nic-*/` |
| VirtIO block, PCI transport | **Bootstrap root-of-trust — see §2 Category C, NOT a violation.** Boot device on the loader's critical path; G2 shrinks the kernel to a minimal boot reader + a post-boot Block Cell. |
| VirtIO net, GPU, input, sound | `cells/drivers/virtio-*/` — **DONE (P06/02/03/04)** |
| PCIe ECAM enumeration | Platform Cell — **done (P01)**; kernel retains `register_bar`/`find_class` store |
| GPIO IRQ dispatch | `cells/drivers/gpio/` — **done** |
| UART (beyond early console) | Driver Cell after early boot |

**Bootstrap residents (NOT violations — §2 Category C):**

| Driver | LOC | Status |
|--------|-----|--------|
| `kernel/src/task/drivers/virtio_blk.rs` | ~217 | Boot block device, root-of-trust (criterion c). G2: shrink to minimal boot reader + post-boot Block Cell. |
| `kernel/src/task/drivers/virtio_pci.rs` | ~225 | x86 transport for the boot block device; same class as above. |

**Remaining genuine exceptions (tech debt — G2):**

| Driver | LOC | Migration target |
|--------|-----|-----------------|
| `kernel/src/task/drivers/mmc.rs` + subs | ~200 | `cells/drivers/mmc/` — G2 (QEMU lacks SDHCI; real board test required) |
| `kernel/src/task/drivers/pcie_ecam.rs` | ~100 | simplify to store-only; full removal in G2 |

**Migrated (kernel-boundary-cleanup plan, 2026-06-24):**

| Driver | Migrated to |
|--------|------------|
| `blk_nvme.rs` (856 LOC) | `cells/drivers/nvme/` ✅ |
| `nic_e1000.rs` (469 LOC) | `cells/drivers/e1000/` ✅ |
| `virtio_gpu.rs` | `cells/drivers/virtio-gpu/` ✅ |
| `virtio_input.rs`, `input_map.rs` | input service Cell ✅ |
| `virtio_net.rs` | `cells/drivers/virtio-net/` ✅ |
| `virtio_sound.rs` | deleted (no consumer, YAGNI) ✅ |
| `fb_console.rs` | GPU Cell + compositor ✅ |

**"We need it early" is not justification.** This reasoning is how Linux became a
monolith. The correct solution is: (1) design init spawn ordering so Driver Cells start
before dependent service Cells, and (2) use kernel fallbacks during the transition period.

### 3.2 Orchestration Policy

These are complex state machines that can run in a privileged Cell with access to
freeze/resume/kill primitives.

| Code | LOC | Correct home |
|------|-----|--------------|
| `kernel/src/cell/hotswap.rs` (orchestration) | ~400 | Supervisory Cell |
| `kernel/src/snapshot.rs` (state machine) | ~350 | Supervisory Cell |

The kernel retains only: `sys_freeze_cell`, `sys_resume_cell`, `sys_kill_cell` —
thin wrappers around the existing scheduler primitives.

### 3.3 Platform Enumeration

| Code | Correct home |
|------|--------------|
| Full ACPI parsing (beyond MADT+DMAR for boot) | Platform Cell (G2) |
| PCIe ECAM bus scan | Platform Cell (G2) |
| DTB full parse | Platform Cell / passed from bootloader |

### 3.4 Scheduling Policy

The kernel owns: timer ISR, context switch, ready-queue mechanics.

The kernel does NOT own: RT admission control, deadline assignment, priority boosting
policy, scheduling group management. (Midori precedent: scheduler policy in safe code;
seL4 MCS: scheduling context objects with user-delegated policy.)

**G3 target**: Trusted Scheduler Cell that receives scheduling hints from the kernel and
returns priority decisions. The kernel executes the decision atomically.

### 3.5 Test/Debug Code

All test, self-test, and debug instrumentation code must be behind
`#[cfg(feature = "test-hooks")]` or `#[cfg(test)]`. Never ships in production binary.
(Already enforced in `layer2_selftest.rs` — maintain this invariant.)

---

## 4. The Boundary Decision Test (quick reference)

When proposing to add code to `kernel/src/`, answer these questions:

```
1. Does it require EL1/ring-0 privileged instructions?
   YES → may belong in kernel. Continue.
   NO  → must be a Cell. Stop.

2. Would moving it outside give any Cell the ability to forge capabilities,
   bypass revocation, or perform silent privilege escalation?
   YES → may belong in kernel. Continue.
   NO  → must be a Cell. Stop.

3. Is it needed to load the very first Cell (chicken-and-egg)?
   YES → may be a temporary kernel resident. Document migration path.
   NO  → must be a Cell. Stop.

4. Is it a device driver, orchestration policy, or platform enumeration?
   YES → must be a Cell. Full stop. "Need it early" is not an exception.
```

---

## 5. Kernel Size Budget

| Era | Target kernel LOC | Current |
|-----|------------------|---------|
| G1 (now) | ≤ 7,000 LOC core (excl. drivers) | ~5,600 LOC |
| G1 end (after driver migration) | ≤ 5,000 LOC core | target |
| G2 | ≤ 5,000 LOC core | VirtIO cells, ACPI cell |

**Size is a proxy, not the goal.** The goal is minimizing the TCB — the code that must
be trusted for the entire system's security to hold. Smaller kernel = smaller TCB =
fewer places for a bug to compromise everything.

---

## 6. Comparison Table — Where Cellos Fits

| System | IOMMU | Device drivers | Hotswap | LOC kernel |
|--------|-------|---------------|---------|-----------|
| seL4 | Userspace (IOMMU PT object) | All userspace | N/A | ~9,300 |
| Fuchsia/Zircon | Kernel (BTI/PMT) | All userspace | Component bind/unbind | ~43,000+ |
| Genode base-hw | Userspace (23.11) | All userspace | Architectural | ~15,000 |
| Redox | None | All userspace | Limited | ~19,000 |
| MINIX3 | None | All userspace | Reincarnation | ~4,000 |
| **Cellos (target)** | **Kernel (Zircon model)** | **All → Driver Cells** | **Supervisory Cell** | **≤ 5,000** |
| Cellos (G1, 2026-06) | Kernel | NVMe+e1000+GPU+NIC+Input+Sound in Cells; VirtIO-Blk+MMC G2 pending | Kernel orchestration (hotswap/snapshot deferred) | ~7,200 |

---

## 7. References

- Liedtke, J. (1995). *On µ-Kernel Construction*. SOSP '95.
- Engler, D., Kaashoek, M.F., O'Toole, J. (1995). *Exokernel: An OS Architecture for Application-Level Resource Management*. SOSP '95.
- Klein, G. et al. (2009). *seL4: Formal Verification of an OS Kernel*. SOSP '09.
- Hunt, G.C., Larus, J.R. (2007). *Singularity: Rethinking the Software Stack*. OSR '07.
- Boos, K. et al. (2020). *Theseus: An Experiment in Operating System Structure and State Management*. OSDI '20.
- Cheriton, D., Duda, K.J. (1994). *A Caching Model of Operating System Kernel Functionality*. OSDI '94.
- Hardy, N. (1988). *The Confused Deputy: Or Why Capabilities Might Have Been Invented*.
- Wallach, D. et al. (1999). *The J-Kernel: A Capability-Based OS in Java*. USENIX '99.
- Genode release notes 23.11 — IOMMU to userspace platform_drv.
- Fuchsia — Zircon kernel objects reference, BTI/PMT DMA design.
