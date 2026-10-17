# ADR-0025 — Cellos guest image execution platforms and generic linear framebuffer

**Date:** 2026-10-17
**Status:** Accepted
**Decider:** Cellos maintainer
**Related ADRs:**
- [ADR-0015 — Dual-mode hybrid architecture](0015-dual-mode-hybrid-architecture.md)
- [ADR-0018 — Cell native portability and runtime profiles](0018-cell-native-portability-and-runtime-profiles.md)
- [ADR-0022 — Intel x86-64 C2C-only direction](0022-intel-x86-64-c2c-only-direction.md)
- [ADR-0024 — Cell, Kernel Cell and object-capability authority](0024-cell-kernel-authority-contract.md)

---

## 1. Context and Problem Statement

Cellos is designed as an isolation-first operating system and microkernel. A key deployment requirement is enabling Cellos images to execute reliably as a **guest workload / virtual machine image** across mainstream virtualization and cloud-native orchestration platforms:
1. **QEMU / KVM** (Linux KVM hosts, Proxmox, OpenStack)
2. **VMware** (ESXi, Workstation Pro, Fusion)
3. **Kubernetes (K8s)** (via KubeVirt containerdisks)

A prior architectural ambiguity arose regarding whether Cellos itself should embed a KVM hypervisor subsystem. This decision formally establishes that **Cellos runs as a guest OS on these platforms**; no KVM host subsystem is to be integrated into the Cellos kernel (the pre-existing minimal Tier 3b VM VMM remains the sole confined mechanism for hosting auxiliary guest workloads).

Furthermore, on VMware and generic x86_64 UEFI/BIOS targets, Cellos historically lacked display output outside QEMU's `virtio-gpu`, restricting VMware instances to headless serial console operation. A strategy was required to enable graphical display (Compositor / ViUI) on VMware without incurring excessive driver complexity.

---

## 2. Decision Drivers

- **Role Clarity:** Cellos is an operating system and cluster node, not a general-purpose Type-1/Type-2 hypervisor.
- **Portability:** Image artifacts and device drivers must enable zero-friction deployment on standard virtualization platforms without proprietary out-of-tree guest tools.
- **Minimal TCB (Law 4 & Microkernel Doctrine):** Avoid kernel bloat; prefer generic firmware abstractions over hypervisor-specific hardware emulations.
- **Software Rendering Parity:** Cellos's UI compositor is software-rasterized (CPU 2D); hardware 3D acceleration is not present in the base compositor.
- **Packaging Simplicity:** Single build pipeline producing deployable formats (`.qcow2`, `.raw`, `.vmdk`, and KubeVirt OCI containerdisks).

---

## 3. Decision

### 3.1. Guest Deployment Platforms and Driver Strategy

Cellos images targeting x86_64 virtualization platforms utilize existing and native driver cells:

| Target Platform | Image Format | Storage Driver | Network Driver | Console / Display |
| :--- | :--- | :--- | :--- | :--- |
| **QEMU / KVM** | `.qcow2` / `.raw` / `.iso` | `virtio-blk` / `nvme` | `virtio-net` / `e1000` | `serial-x86` (COM1) + `virtio-gpu` |
| **VMware (ESXi / Workstation)** | `.vmdk` / `.iso` | `ahci` (SATA) / `nvme` | `e1000` (VMware default vNIC) | `serial-x86` (COM1) + Generic Linear FB |
| **Kubernetes (KubeVirt)** | OCI Containerdisk | `virtio-blk` | `virtio-net` | `serial-x86` (virtctl) + VNC |

### 3.2. Boundary on Hypervisor Integration Inside Cellos

- **No KVM subsystem in Cellos:** Cellos will NOT integrate a Linux KVM-compatible hypervisor system or nested virtualization manager into its kernel.
- **Tier 3b Confined VMM:** The existing minimal Tier 3 VMM (Stage-2 / EPT guest-hosting for isolated companion micro-VMs) remains strictly scoped to its approved workload sandbox role and is not expanded into a general-purpose hypervisor.

### 3.3. Generic Linear Framebuffer for x86_64 Display

- Enable `map_display_framebuffer_user` on x86_64 in `kernel/src/memory/paging.rs` to map physical linear framebuffers provided by Limine (via UEFI GOP or VESA VBE).
- Use **Write-Combining (WC)** caching flags via the x86 Page Attribute Table (PAT) on leaf page entries.
- Expose the mapped linear framebuffer to the display/compositor subsystem, enabling graphical shell and ViUI applications to run identically across VMware, QEMU, VirtualBox, Hyper-V, and bare-metal PC firmware without needing hypervisor-specific graphics drivers.

---

## 4. Considered Options and Rejected Alternatives

### Option 1: Integrate a KVM Hypervisor Layer Inside Cellos Kernel
- *Description:* Implement full KVM ioctl interfaces (`/dev/kvm`) and nested virtualization within the Cellos kernel.
- *Status:* **REJECTED.**
- *Reason:* Fundamentally confuses the host and guest roles. Cellos is intended to run *as a workload* on cloud/virtualization hosts. Duplicating KVM inside the kernel adds tens of thousands of lines of high-risk code, breaches SAS/LBI isolation invariants, and duplicates capabilities already provided by the underlying cloud hypervisor.

### Option 2: Write a Dedicated VMware SVGA II Driver (`cells/drivers/svga2`)
- *Description:* Implement a full VMware SVGA II PCI driver supporting SVGA FIFO ring-buffer commands and register handshakes.
- *Status:* **REJECTED.**
- *Reason:* SVGA II requires ~1,000 LOC of driver plumbing, works exclusively on VMware, and provides no 2D rendering speedup over a direct linear framebuffer because Cellos uses CPU software rasterization. The only advantages (hardware cursor, host window auto-resize) do not justify the development and maintenance cost over a universal firmware framebuffer.

### Option 3: Restrict VMware to Headless-Only (Serial Console)
- *Description:* Only support server/headless workloads on VMware without graphical display support.
- *Status:* **REJECTED.**
- *Reason:* Prevents visual debugging, interactive UI evaluation, and developer testing on VMware Workstation / Fusion workstations.

### Option 4: Generic Limine Linear Framebuffer with Write-Combining (Chosen)
- *Description:* Implement `map_display_framebuffer_user` for x86_64, mapping the physical GOP/VESA framebuffer handed over by Limine into userspace with Write-Combining PAT flags.
- *Status:* **ACCEPTED.**
- *Reason:* Universal compatibility across all hypervisors and bare-metal UEFI systems with minimal kernel changes (~50 lines). Write-Combining delivers multi-gigabyte/second CPU write throughput, ensuring smooth 60 FPS UI rendering for Cellos compositor and ViUI applications.

---

## 5. Consequences

### Positive
- **Universal Portability:** Cellos images can boot and display graphical interfaces out-of-the-box on QEMU/KVM, VMware ESXi/Workstation, KubeVirt, VirtualBox, and physical x86 PCs.
- **Architectural Clarity:** Strict line drawn between Cellos as a guest workload and the underlying host infrastructure.
- **Cloud-Native Packaging:** Clear build path for generating KubeVirt OCI containerdisks and VMware VMDK disks from the canonical build system.
- **Minimal Maintenance:** No proprietary graphics driver stack required for VMware.

### Negative / Trade-offs
- **Static Display Resolution:** Screen resolution is fixed at boot time by Limine firmware mode; dynamic window auto-resize on VMware window stretch is not supported without re-negotiating video modes.
- **Software Cursor:** The mouse cursor is composited by the Cellos software compositor rather than host hardware cursor acceleration.
