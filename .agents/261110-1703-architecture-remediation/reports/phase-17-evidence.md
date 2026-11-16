# Phase 17 evidence: quota ownership and shared-root hardening

**Date:** 2026-11-16 · **Phase:** [phase-17-quota-shared-root.md](../phase-17-quota-shared-root.md) · **Handoff:** [S8 approval](../s8-handoff-approval.md) · **Census:** [17a charging](../research/p17-census-charge.md), [17b shared root](../research/p17-census-shared-root.md)

## 1. Success criteria

| Criterion | Result |
|---|---|
| An allocation made in interrupt context while Cell A runs is not charged to A | `[selftest] IRQ-ALLOC-CHARGE: PASS` on RV64 `--harts 2` (every case boot), AArch64 test-hooks and the x86 domain lane (TCG 1/2/4, KVM 4); host test in `memory/heap.rs`. |
| A VM whose guest RAM exceeds its owner's budget is refused before any mapping | `[selftest] VM-GUEST-BUDGET: PASS refused 65 pages over a 64-page budget with nothing taken; 32 pages charged 172032 bytes; reaped to 0` on AArch64 virt-vhe and the x86 KVM domain lane (nested VMX). |
| x86 boots with SMEP+SMAP+UMIP; a kernel read of a user address outside the window faults | Every x86 lane boots with `[x86] protection: smep=on smap=on umip=on` and no `#PF kernel`. `[selftest] SMAP-WINDOW: PASS (syscall copy ok, raw read faulted)` and `[selftest] SMAP-GRANT-SLICE: PASS` appear on the domain lane (TCG 1/2/4, KVM 4). The IDT protection lane passes `smap=on read=pf window=ok`, `smep=on exec=pf` and `umip=on sgdt=gp`, and its refusal control still refuses a required build on a CPU without the bits. |
| Two boots of the same image place a named cell at different bases when entropy is available | `qemu64,+rdrand,+rdseed`: `init` image base `0x13b2000000` then `0x1176000000`, both logging `[va] slot order: randomised`. Without RDRAND, both boots log `deterministic (no entropy)` and put the base at `0x1002000000`. |
| Kernel host tests; x86/RV64/AArch64 boot and domain lanes | 731 kernel host tests, 41 `hal-x86`, 23 `hal-arm`; lanes in §4. |

## 2. What changed

### 17a: charging

- **One charge record.** `Charge { cell, generation }`, `Account { Memory, Dma, Guest }` and `Reservation` (`memory/cell_quota.rs`). Every refund is checked against the generation, so a refund for a retired incarnation of a reused CellId does nothing. The limits table is lock-free. Each counter word packs 32 generation bits over 32 byte bits.
- **Explicit heap charge.** The allocator charges the hart's `alloc_charge`, not the dispatched cell.
  - Syscall entry opens the caller's scope.
  - `yield_cpu` runs housekeeping and selection as the kernel and restores the caller's scope on resume.
  - Each of the 8 Rust interrupt entries opens a kernel scope first.
  - The census counted 31 identity-swap scopes. Seven were drop-only and are deleted; the rest became explicit owner or kernel scopes. `KernelAllocationContext` and `PipeAllocationContext` are gone.
- **Frames carry their charge.** `FrameRun` holds a `RunCharge` and refunds it once, whoever frees the run. That includes the hart-0 reaper, where the refund is lock-free and allocation-free.
  - Thread stacks and SHM moved their charge into the run.
  - Domain page tables and private pages are now charged.
  - Kernel tables, root stacks, cell segments and grant pages record an owner but no account (owner decision; residual).
- **Guest-memory budget.** `GUEST_IN_USE` sits beside `DMA_IN_USE`. The kernel-only catalog field sets 1544 MiB for `/bin/hypervisor`, 1 MiB for `/bin/silo` and 0 for every other cell; `catalog.generated.json` is unchanged.
  - `create_vm` reserves the whole guest charge before any root, carve or tag is taken. Only a budget refusal returns `OutOfMemory`; every other error keeps `NotSupported`.
  - VMs are keyed by `(incarnation, vm_id)` and reaped at root retirement.
- **One tag pool.** `TagPool` (`memory/tag_pool.rs`) replaces the private-root ASIDs, the AArch64 VMIDs and the x86 SVM ASIDs. A tag is reused only with an invalidation proof for that value. Per-owner limits are 1 private root, 4 VMIDs and 4 SVM ASIDs; the kernel is exempt. This closes the Phase 16 residual where one owner could hold all 255 VMIDs.

### 17b: shared root

- **Free frames are RW+NX.** On RV64 and AArch64 the identity alias of free RAM is no longer executable (`paging::FREE_FRAME_ALIAS`). x86 has no identity alias.
- **User stacks in a VA arena.** User stacks live at 80–96 GiB with unmapped guard pages, so a user stack's VA no longer equals its PA. The slot is returned only with the frames. Kernel stacks stay at VA = PA, because an RV64 secondary hart sets `sp` before paging is on.
- **Identity maps refuse the reserved range.** Identity MMIO maps that reach the cell slots or the stack arena are refused, and `map_mmio_x86` returns a `Result`. RAM is managed only below `CELL_VA_START` (64 GiB) on every arch, and any RAM dropped is logged.
- **SMEP, SMAP and UMIP.** Each bit is enabled whenever the CPU has it. `x86-<bit>` refuses a CPU without it; `x86-no-<bit>` keeps it off. There is one `stac` site, the user-copy window.
  - Every other kernel access to user memory now goes through the kernel alias or was deleted. The 17b review found two the census had missed: `GrantSlice`'s `size_out` and ELF relocation.
- **Slot randomisation.** Cell image and user-stack slots start at a random index when RDSEED or RDRAND is available, using one atomic ownership bitmap. The lowest 8 cell slots stay for fixed-VA images. `kaslr_cell_base` is deleted.
- **ELF user stacks are zeroed** at spawn (a cross-cell information leak before).

## 3. Defects found and fixed

| Severity | Defect | Fix |
|---|---|---|
| HIGH (pre-existing) | ELF spawn handed out an unzeroed user stack | `fbc17690ef` |
| HIGH (17a) | A receive's wire snapshot was charged to the sender, so a server could never receive a message whose sender was gone or full | `0d92da9c9a` |
| HIGH (17a) | An infallible completion-queue `Arc` was charged to a cell under `SCHEDULER`, which could deadlock the kernel | `0d92da9c9a` |
| HIGH (17b) | `GrantSlice` wrote `size_out` outside the SMAP window, panicking every SMAP CPU on the first VFS grant read | `07dced9eb6` |
| HIGH (17b) | ELF relocations were written through the cell's user mapping, panicking every PIE spawn on a SMAP CPU | `260885e9fe` |
| MED (17a) | A client's async submit charged the server's quota | `0d92da9c9a` |
| MED (17b) | The boot check let RAM at 64–80 GiB alias the cell slots, and machines with RAM above 80 GiB stopped booting | `260885e9fe` (RAM clamp) |
| MED (17b) | A refused x86 MMIO map returned silently, and its IOMMU/MSI-X caller panicked later | `260885e9fe` |
| MED (step 2) | Charging mailbox growth to the receiver made kernel relays to a full receiver fail | `5789a14325` |
| HIGH (17b lanes) | The kernel read ECAM (and could read MSI-X tables and the virtio common config) through identity leaves that a cell's `RequestMmio` had made USER, so every x86 boot panicked at `va=0xb0000000` once SMAP was on | `8257c7ea0c`: a kernel-only uncached MMIO alias window |
| LOW ×6 (alias review) | Fixed alias cap; window check; ECAM alias failure panicked; refusal log flooding; MSI-X partial programming failure; VT-d IOTLB registers past 4 KiB unmapped | `4e1628ce84` |
| LOW ×9 | Generation width, fixture gating, fixture CellId, p01 hot-swap ledger, test probe, `WaitCompletion` error, unheld free, fixed-VA slots, the `native-domains` guard and the STACK-ARENA fixture | `0d92da9c9a`, `7216cdfd0d`, `260885e9fe` |

## 4. Lanes

The runner is `~/worktrees/p16-cp/pr.sh`. Each run is a snapshot worktree at the commit named. Logs are under `~/worktrees/p16-cp/<run>/`.

| Run (commit) | Lanes | Result |
|---|---|---|
| `p17-c12` (`3a7532f50b`) | x86 boot 1/4 CPU, x86 domain TCG 1/2/4 + KVM 4, RV64 boot, AArch64 test-hooks, RV64 `--harts 2` | PASS |
| `p17-c3`, `p17-fx2` (`a0bf00b07b`, `1babeaf159`) | The same, plus C2C, srv, native-workload | PASS after the `IRQ-ALLOC-CHARGE` ordering fix (AArch64 test-hooks first failed on its missing marker) |
| `p17-s3`, `p17-s3b` (`9622bfffed`, `753be088f7`) | x86 boot, four AArch64 boots, both hypervisor smokes, x86 domain, Tier-3 Chrome (nested VMX), C2C, RV64, AArch64 test-hooks | PASS. Chrome passes 13/13 with its 1536 MiB guest inside the 1544 MiB budget. |
| `p17-a-final` (`7216cdfd0d`, end of 17a) | Every x86 device lane, C2C, x86 domain, the AArch64 boots, test-hooks, both hypervisor smokes, Chrome, RV64 boot/srv/native/`--harts 2` | PASS. `virtio-x86` first failed because the runner built the PC ISO after the test; it passes 4/4 once the ISO exists, and the runner now builds it first. |
| `p17-b1` (`443bf40026`) | x86/RV64/AArch64 boot, AArch64 test-hooks, vfs-quota, x86 domain, RV64 `--harts 2` | PASS (`STACK-ARENA`, the user-stack probe and `FREE-FRAME-NX` all pass). `vfs-quota` fails as on `main`. |
| `p17-b-final` (`260885e9fe`) | Full set, with the x86 CPU models carrying `+smap` | FAIL on x86: `#PF kernel va=0xb0000000` (ECAM). AArch64 and RV64 pass, the two-boot randomisation passes, and the IDT lanes pass. Fixed in `8257c7ea0c`. |
| `p17-b-final2` (`8257c7ea0c`) | IDT base and protection lanes, every x86 device lane, x86 domain TCG 1/2/4 + KVM 4, C2C, Tier-3 virtio-e2e (VT-d), hostile, python, nginx, Chrome, x86 hypervisor smoke, rust-std on three arches, phase09 ssh, four AArch64 boots, AArch64 test-hooks, AArch64 hypervisor smoke, RV64 boot | PASS. No `#PF kernel` in any log. |
| `p17-last` (`4e1628ce84`, final) | 13 kernel build configurations, every host test suite, IDT base and protection lanes, every x86 device lane, x86 domain TCG 1/2/4 + KVM 4, C2C, Tier-3 virtio-e2e (VT-d), Chrome, x86 hypervisor smoke, rust-std on three arches, four AArch64 boots, AArch64 test-hooks, RV64 boot | PASS. `smep=on smap=on umip=on` and no `#PF kernel` in any log. `pvscsi-x86` failed once under load: init stopped VFS bootstrap after `[nvme] irq wait expired after 10 ticks`, and the run took 154 s against 107 s. Rerun alone it passed 3/3, so this is a timing flake. |

These fail the same way on `main`, so they are not regressions: `vfs-quota` (`shell: authenticated serial session failed: NotFound`, checked at `dfbbd2054c`, `7216cdfd0d` and `443bf40026`), 2 `rust-std-promotion` tests, the `x86hv-iso` rollout guard, and the EL2 test-hooks lane (recorded broken).

## 5. Reviews

All reviews were independent, read-only `haily-reviewer` runs.

| Range | Verdict | Fixed in |
|---|---|---|
| `dfbbd2054c..753be088f7` (17a) | FAIL: 2 HIGH, 1 MED, 5 LOW | `0d92da9c9a` |
| `753be088f7..0d92da9c9a` (re-review) | PASS_WITH_RISK: 3 new LOW | `7216cdfd0d` |
| `7216cdfd0d..ed21726730` (17b) | FAIL: 1 HIGH, 2 MED, 4 LOW | `07dced9eb6`, `260885e9fe` |
| `ed21726730..260885e9fe` (re-review) | PASS_WITH_RISK: 3 new LOW | `45cdf44c97` |
| `260885e9fe..8257c7ea0c` (MMIO alias) | PASS_WITH_RISK: 1 MED (lane coverage, closed by `t3-virtio-e2e` PASS), 6 LOW | `4e1628ce84` |

## 6. Measurement (handoff §4)

`top -b -n 1` on the x86 headless image, three boots per image; the samples agree. Total memory is 81.3M throughout.

| Cell | Before (`fbc17690ef`) | Explicit heap charge (`a0bf00b07b`) | After 17a (`0d92da9c9a`) |
|---|---|---|---|
| init | 3.1M | 3.1M | 3.1M |
| cellbox | 18.4K | 19.5K | 19.5K |
| e1000e | 11.8K | 12.7K | 12.3K |
| net | 0B | 928B | 960B |
| vfs | 1.1K | 1.1K | 1.1K |
| input, kms, machine-pin, serial, supervisor | 0B | 0B | 0B |

The headless image runs no domain cells or VMs. The guest charge is measured by `VM-GUEST-BUDGET` and the Chrome lane (a 1536 MiB guest inside the 1544 MiB budget, 13/13). No limit is retuned.

## 7. Residuals

- **SMAP scope.** SMAP stops the kernel touching user pages outside its copy window; it does nothing between cells. Kernel MMIO now goes through a supervisor-only alias. `RequestMmio` refuses any window that reaches LAPIC, IOAPIC, HPET or VT-d registers; no in-tree cell asks for one.
- **OVMF 64-bit aperture.** A q35+OVMF guest with 33–64 GiB of RAM may place its 64-bit PCI aperture at 64 GiB. Its 64-bit BARs are then refused, so those devices are absent, but nothing panics [INFERENCE from OVMF's default placement; not run].
- **Boot assert on RV64/AArch64.** If kernel or bootloader memory sits at or above 64 GiB, boot still stops at the assert. No known firmware places it there.
- **RV64 `fence.i`.** No `fence.i` is issued after loader writes. This predates the phase and was noted by review.
- **Grants stay at VA = PA** (owner decision). Six no-IOMMU DMA paths program the grant id as a DMA address, and `libs/abi` calls it a physical address. Moving them needs ABI text or a translation syscall.
- **Uncharged frames** (owner decision): grant pages, root stacks and cell segments.
- **Kernel stacks stay at VA = PA.** Moving them needs an RV64 secondary-hart entry that enables paging before setting `sp`.
- **Not exercised on this host:**
  - The x86 SVM paths of `TAG-POOL-OWNER-LIMIT` (the host is Intel VMX; the fixture skips under VMX).
  - An x86 boot with RAM at or above 64 GiB (the host has 39 GiB). A host test covers the clamp.
- **Pre-existing, same on `main`:** `vfs-quota` fails with `shell: authenticated serial session failed: NotFound` before its `[stack-baseline] name=shell` marker (`dfbbd2054c`, `7216cdfd0d`, `443bf40026`).
- **x86 demand-paging path:** it maps a frame described as "zeroed" without clearing it. The path is dormant.
- **UMIP:** foreign binaries (mlibc, Tier-2 C) were not audited for `sgdt`/`sidt`/`sldt`/`smsw`.
- Evidence ceiling: host and QEMU (TCG, KVM with nested VMX). No physical hardware.
