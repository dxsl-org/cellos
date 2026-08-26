# Scout Report — Tier 3b finish + G5 Lite foundations

Verification pass over hypervisor code before planning. All citations re-grepped this session (2026-07-22), not copied from prior plans.

## Ground truth (verified file:line)

### ARM64 EL2 VMM — SHIPPED and warm
- Kernel privileged half: `kernel/src/hypervisor/registry.rs` (558 LOC) + `smoke_guest.rs` (244).
  - `create_vm` / `create_vcpu` / `map_guest_memory` / `run_vcpu` / `vcpu_regs` / `write_guest_memory` / `read_guest_memory` / `inject_irq` / `reap_vms_for_task` all present.
  - `read_guest_memory` bounds path is `#[cfg(target_arch="aarch64")]` only; non-aarch64 returns `ViError::NotSupported` (`registry.rs:494-498`).
- Cell unprivileged half: `cells/services/hypervisor/src/` (15 files, ~1900 LOC).
  - Device backends already IN the cell: `pl011.rs`, `gicd.rs`, `virtio_mmio.rs`, `virtio_blk.rs`, `virtio_net.rs`, `virtio_console.rs`, `psci.rs`, `timer.rs`, `net_backend.rs`. Run loop `run_loop.rs:29-178`.
  - **Kernel Boundary status: GOOD.** Device emulation already lives in the capability-gated cell, not the kernel. G5 lever 5 ("backends as cells") is ~80% realized — but all backends share ONE cell, not per-device cells.
- ABI: `libs/api/src/abi/hypervisor.rs` — `ViVmExit` `#[repr(C,u8)]`, 8 variants (MmioRead/Write/Hvc/Wfi/SysReg/Preempted/Shutdown/Unknown), `VERSION=1` frozen (Law 1).
- Syscalls 220-227 (`libs/api/src/abi/syscall.rs:267-298`): CreateVm..ReadGuestMemory, HypervisorCap-gated (allowlist bit 44). Arch-generic.
- Guest: ONE wired guest = Alpine (`scripts/make-hypervisor-fs.sh`, 140 LOC → `kernel/src/embedded-hv/kernel_fs.img`, 54 MB present).

### Stage-2 builder — `kernel/src/memory/stage2.rs` (528 LOC, read in full)
- RO/RW descriptors both expressible: `S2_S2AP_RO` (line 38), `page_desc(pa, writable)` (line 102). **CORRECTION (red-team C1, 4/4 reviewers): the CoW isolation guard does NOT exist — only the descriptor bits do.** The `map()` SAS guard (274-279) checks ONE contiguous carved region and is SKIPPED entirely when `guest_ram_pages==0`. A clone must map borrowed-golden frames (another VM's region) + its own scattered overlay = two disjoint regions the single-region model cannot express → either `SasViolation` on the golden map, or (if the clone does not carve) the guard is fully bypassed = SAS escape. **The guard must be re-architected into a per-table multi-region HPA allowlist (P05).** This is THE CoW substrate blocker, larger than the P08 Drop fix.
- `carve_guest_ram(n)` (line 209) → `allocate_guest_ram` (`frame.rs:339`). NOTE: `carve` zeroes via `write_bytes`, but `frame.rs` alloc/dealloc (`:120,:142`) are bitmap-only — **frames leaving VM ownership are NOT zeroed** (red-team M2).
- `map(ipa,hpa,n,writable)` (line 246) with single-region SAS guard (274-279 — see C1), MMIO-hole guard (267-271), overflow guard (256-257).
- `unmap_single` (line 428): clears L3 descriptor, **does NOT free the frame and does NOT TLB-invalidate** — doc warns post-activation remap needs full S2 TLB invalidation. **No `tlbi` primitive exists anywhere in this file.**
- `Drop` (line 453): unconditionally frees `sub_frames` + root + **all `guest_ram_pages` frames**. No refcount, no "borrowed frame" concept.

### x86 VMM — DOES NOT EXIST YET
- `grep -rl "svm|Svm|VMCB|vmcb|npt|Npt" kernel/src/` → **zero matches.**
- `.agents/260711-1917-tier3b-x86-vtx/` is a 10-phase plan (~5-6K LOC) that is **0% implemented.** "Continue x86" = greenfield bring-up, not maturation of warm code.

## LIVE bugs found (not just planned)
1. **C1 IRQ-DoS still live** — `registry.rs:513` `q.push_back(intid)` has NO depth cap. Flagged as Crit in `.agents/260712-0952` (C1) but never landed. Guest masks IRQ + spams QueueNotify → unbounded kernel `Vec` growth → SAS-wide OOM. Blocks any safe multi-clone story.
2. `read_guest_memory` is aarch64-only (expected; x86 stub returns NotSupported).

## Prior-plan reconciliation
- `.agents/260712-0952` P04-08 (writable storage, glibc guest, virtqueue fuzz, x86 SVM) = the open Track-A coding items. User already chose glibc guest + writable virtio-blk.
- `.agents/260711-1917` = the x86 bring-up plan Track A phase 1 continues (it is a fresh start, not a continuation).
- `.agents/260613-2134` ARM64 = DONE; the substrate Track B builds on.

## CVE-2026-5747 note
Memory claims the bounds-check mitigation already lives at `registry.rs:311-317`; those lines in the current tree are the HAL→API `ViVmExit` conversion (`run_vcpu` path), and the guest-mem bounds guards live in `write_guest_memory`/`read_guest_memory`. Track A P3 must **verify functionally** (write a fault-injection test), not assume the line numbers.
