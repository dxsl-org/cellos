# Phase 17 census, part 17b: shared-root hardening (S8 step 1)

Measured read-only on worktree `p17-quota` at `dfbbd2054c`. Paths are relative to `kernel/src/` unless they start with `hal/`, `scripts/`, `tests/`, `cells/`, `libs/`, `docs/` or `.github/`. **[I]** marks an inference that was not executed or observed at runtime. Everything else was read in the cited lines. No build, lane or QEMU run was made for this census.

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| G1 | **The kernel-RWX free-frame invariant has exactly two writers.** The boot identity map gives every `Usable` frame RWX on RV64/AArch64, and `KernelBackend::normalise` restores RWX before a run with `KernelAlias != Intact` is freed. No other site writes a kernel-root leaf with `EXECUTE` outside test fixtures and dead code. | `memory/paging.rs:144-154`; `memory/frame_run.rs:1553-1590`; §1.1 |
| G2 | **Nothing relies on a free frame being executable.** Every reliance is on *writable*: loader zero/copy, relocation, page-table zeroing, grant/SHM zeroing, guest blobs. RW+NX is two flag edits on RV64/AArch64 plus docs. | §1.2 |
| G3 | **x86 never had a free-frame alias.** `KernelAlias::of_identity_remap` returns `Intact` on x86, so no lower-half alias is added. Kernel RAM access goes through the HHDM, and every HHDM leaf the kernel itself adds is RW+NX. Limine's own HHDM leaves were not inspected **[I]**. | `memory/frame_run.rs:92-98`; `hal/arch/x86/src/x86_64/paging.rs:563,582,698` |
| G4 | **Assumption 1 is false for grants, true for stacks.** Six Tier-1 DMA paths program a raw grant id (VA == PA today) into hardware with no translation syscall. All six are no-IOMMU MMIO or BCM builds. No cell gives a device a stack, heap or static address. | §3 |
| G5 | **The kernel's own DMA syscalls also treat the grant VA as a PA.** `GrantDma` maps its `phys` argument directly. `ExportDmaRange` stores the caller's `va` as the window's `phys`. Every IOMMU backend returns `IOVA == phys`. The ABI text says "phys" and "base_paddr". So moving grants off VA == PA changes documented `libs/abi` semantics, though no number or wire byte moves: **owner decision** (§9 Q1). | `task/syscall/grant.rs:2027,2084-2087`; `task/syscall/dma_export.rs:480,582,624`; `libs/abi/abi/syscalls.toml:252-260,869-877,994-1003` |
| G6 | **Stacks can move first without an ABI change.** No cell sees a stack PA. Four kernel sites conflate the stack's VA with its run base (PA). | §2 rows S1-S5 |
| G7 | **The x86 kernel has no SMAP window.** `arch::user_access::guarded_byte_copy` (the only user-byte mover) runs no `stac`/`clac`. The HAL has a `protect::user_access` window, but only the IDT fixture calls it. With SMAP on today, every SAS copy would `#PF` inside the guard and come back as a *recoverable* `CopyError`: syscalls would fail quietly, not crash. | `arch/x86_64/user_access.rs:20-58`; `hal/arch/x86/src/x86_64.rs:238-251`; `hal/arch/x86/src/x86_64/idt/probe_ist.rs:316`; `task.rs:448-470` |
| G8 | **Six production or test-hooks sites touch U=1 memory outside any window on x86.** Grant zeroing, the `spawn_thread` user-stack zeroing, `BlkReadAsync`, the test-hooks stack watermark, plus two sites with no caller. | §5.4 rows A7-A12 |
| G9 | **Every x86 PR lane boots a CPU model without SMEP/SMAP/UMIP, and a requested protection the CPU lacks refuses boot.** Default-on therefore breaks every x86 lane until its `-cpu` changes. It also refuses older physical CPUs. | `hal/arch/x86/src/x86_64.rs:210-229`; `arch/x86_64/boot.rs:121-126`; §5.5 |
| G10 | **`kaslr_cell_base` has no caller and cannot be wired as it is.** Its window `[0x1000_0000, 0x7FFF_0000)` is unrelated to the `va_alloc` slots, has no size reservation or collision check, and sits inside identity-mapped RAM on boards whose RAM starts below 2 GiB **[I]** (QEMU virt AArch64 at 1 GiB; x86 guests above 256 MiB). The seed is set only on x86. | `memory/kaslr.rs:29-34,61-70`; `arch/x86_64/boot.rs:97-98`; `rg kaslr_cell_base` |
| G11 | **Kernel entropy exists only on x86** (CPUID-gated RDSEED/RDRAND). RV64 and AArch64 return zero bytes: the "virtio-rng" module name is historical. "Randomise when entropy is available" therefore means x86 with `+rdrand`/`+rdseed` in the CPU model. | `dev/virtio_rng.rs:1-6,42-60,79-125` |
| G12 | **Surprise: an ELF spawn never zeroes its user stack.** `elf_prepare` zeroes only the kernel stack. `spawn_thread` zeroes both. A fresh cell's stack can show the previous owner's bytes. This is outside 17b scope, but it touches the same site SMAP must convert. | `task/elf_prepare.rs:246-256`; `sched/scheduler.rs:1084-1095,1358-1367` |
| G13 | **The SMAP-class guard is x86-only in this phase.** RV64 keeps `sstatus.SUM` set for the kernel's lifetime. AArch64 runs without PAN. | `arch/riscv64/user_access.rs:54-66`; `arch/riscv64/boot.rs:91`; `hal/arch/arm/src/aarch64/el2.rs:108-112` |
| G14 | **Stale references.** The plan cites `va_alloc.rs:140-146`; the bump path is `:137-143`. Spec 19 cites `va_alloc.rs:47` (`CELL_VA_STRIDE` is at `:56`) and "`STACK_PAGES + 1`" (two guards now). `mutable-globals.toml` places `init_kaslr` at `main.rs:224`; the only call is `arch/x86_64/boot.rs:98`. Spec 02 §1 says RV64 RAM is a high-half HHDM; RV64 is identity-mapped. | `loader/va_alloc.rs:56,137-143`; `task/stack.rs:67`; `scripts/mutable-globals.toml:1928-1931`; `docs/specs/19-hardware-isolation-layers.md:171,186`; `docs/specs/02-memory.md:14` |

## 1. Free-frame alias invariant

### 1.1 Writers of the kernel identity alias

| Site | Arch | Flags written | When |
|---|---|---|---|
| Boot identity map, `MemoryType::Usable` | RV64, AArch64 | `V|R|W|X|A|D` (`memory/paging.rs:147-154`) | `init_kernel_paging`, 4 KiB leaves (the trait default `identity_map` loops `map` per page: `hal/traits/paging/src/lib.rs:175-190`; neither arch overrides it) |
| Boot identity map, `MemoryType::Kernel` | RV64, AArch64 | `V|R|W|X|A|D` (`memory/paging.rs:155-162`) | Kernel image only (carved by `boot/dtb_memory.rs:129-130`, `arch/aarch64/boot/firmware.rs:54,262,347`). Out of 17b scope: kernel text stays RWX. |
| Boot identity map, `Bootloader`/`Framebuffer`/`MMIO` | RV64, AArch64 | `V|R|W|A|D`, no X (`memory/paging.rs:163-183`) | — |
| `KernelBackend::normalise` | RV64, AArch64 (any run whose alias is not `Intact`) | `kernel_rwx = V|R|W|X|A|D` (`memory/frame_run.rs:1557-1564`), mapped at `frame → frame` (`:1574`). A page already translating to itself is skipped by presence only, not by flags (`:1571`). | `FrameRun::free` when `alias != Intact` (`memory/frame_run.rs:458`) |
| Alias choice | all | `KernelAlias::of_identity_remap`: `Removed` when `phys_to_virt(base) == base`, else `Intact` (`memory/frame_run.rs:92-98`) | Stack (`task/stack.rs:273`), grant (`task/syscall/grant.rs:430`), SHM (`:1166`), pin-held runs (`memory/pin.rs:620`) |
| Dead code | — | `spawn_synthetic` maps RWX (`task.rs:2776,2843-2851`) | no caller (Phase 16 census §2.1) |
| Fixtures | — | `arch/riscv64/task/stack_overflow_probe.rs:86`, `arch/riscv64/task/thread_user_entry_selftest.rs:95`, `task/user_hello.rs:78`, `arch/x86_64/task/x86_idt_cpl3.rs:27` | USER code pages at their own VA, not the kernel alias |

Paths that *remove* the alias, so it must come back: user stacks and kernel stacks remap the usable pages and unmap the guards (`task/stack.rs:281-339`); `alloc_grant_pages` remaps USER (`task/syscall/grant.rs:1321-1338`); `ShmMap` remaps USER (`:1953-1968`); the AArch64 spin-table page maps and unmaps a firmware page only (`arch/aarch64/smp_bringup.rs:418-443`, RW, no X).

Docs that state the RWX contract: `memory/frame_run.rs:69-83,1546-1552`; `task/syscall/grant.rs:299-330`; `task/user_copy/mod.rs:18-24`; `task/user_copy/sas-lease.rs:3-16`; `task/stack.rs:111-120,281-283`; `docs/specs/02-memory.md:76`.

### 1.2 What relies on a free frame being kernel-writable or executable

| Reliance | Site | Needs W | Needs X |
|---|---|---|---|
| Loader zeroes a fresh segment frame through `phys_to_virt` | `loader/elf.rs:340-346` | yes | no |
| Loader copies file bytes into it | `loader/elf.rs:362-371` | yes | no |
| Relocation patches through the alias | `loader/reloc.rs:115-117` | yes | no |
| W^X pass reads data through the alias | `loader/wx.rs:165-173` | no (read) | no |
| Kernel-root and private-root page-table frames zeroed and walked | `memory/paging.rs:139` (root), `memory/address_space.rs:143-144,341-350`, `memory/paging.rs:60-62` (`table_at`) | yes | no |
| Domain grant backing zeroed | `task/syscall/mod.rs:279-286` | yes | no |
| `ShmAlloc` zeroes before publish | `task/syscall/grant.rs:1919-1930` | yes | no |
| Stage-2 tables, AArch64 guest blob | `memory/stage2.rs:202-206,256`; `arch/aarch64/vm.rs:131-144` | yes | no (guest executes through Stage-2) |
| Boot heap | `main.rs:164` | yes | no |
| In-kernel virtio DMA (heap pointer used as PA) | `dev/virtio_hal.rs:7-10,47-48` | yes | no |
| Warm snapshot (PA used as VA) | `snapshot_engine.rs:2128-2142`, compiled only with `snapshot-qualified` (`main.rs:88-92`) | yes | no |
| Sas copy-lease rationale ("re-mapped kernel-RWX… writes without faulting") | `task/user_copy/sas-lease.rs:3-7` | the hazard is W; X is irrelevant | no |

Kernel-mode execution from a `Usable` frame: none found. Searched every `Flags::EXECUTE` writer (§1.1), every kernel-root `map_page` caller (`rg 'paging::map_page\('`), and the AArch64 EL2 monitor note. The monitor fetches kernel text through this table, and kernel text is `MemoryType::Kernel` (`hal/arch/arm/src/aarch64/paging.rs:110-118`) **[I]**: the monitor's code was not traced.

### 1.3 What must change for RW+NX

| Arch | Change | Encoding effect |
|---|---|---|
| RV64 | Drop `EXECUTE` from the `Usable` boot flags (`memory/paging.rs:147-154`) and from `kernel_rwx` (`memory/frame_run.rs:1557-1564`, rename `kernel_rw`). | `X=0` |
| AArch64 | Same two edits. | A non-`EXECUTE` leaf gets `UXN|PXN` (`hal/arch/arm/src/aarch64/paging.rs:128-129`). Non-VHE EL2 reads bit 54 as XN, so the Pi monitor can no longer fetch from data pages. It must not anyway (`:110-118`). |
| x86_64 | None. No alias is added (`memory/frame_run.rs:89-97`). Kernel stacks are mapped `V|R|W` and grants and user stacks `USER|R|W`, so all are NX (`task/stack.rs:286-301`; `hal/arch/x86/src/x86_64/paging.rs:180-191`). | — |
| all | Docs: `memory/frame_run.rs:71,79,1546`; `task/syscall/grant.rs:299-316`; `task/user_copy/mod.rs:21`; `task/user_copy/sas-lease.rs:4-16`; `task/stack.rs:115,281`; Spec 02 §3.1 step 6 (`docs/specs/02-memory.md:76`). | — |

A boot-time assertion is cheap: walk one usable frame and one restored frame with `hal::paging::leaf` and require `write && !exec && !user`. `Leaf` carries those fields (`hal/traits/paging/src/lib.rs:89-102`).

### 1.4 x86 posture

- RAM is reached through the HHDM (`memory/frame.rs:13-28`; `memory/paging.rs:443-459`). The kernel copies Limine's higher-half PML4 entries (`memory/paging.rs:510-519`).
- HHDM leaves the kernel adds are `P|RW|NX` (`hal/arch/x86/src/x86_64/paging.rs:563,582,698`). Limine-created HHDM leaves were not inspected **[I]**. A boot `leaf()` probe of one HHDM page would settle it.
- Lower-half VA == PA exists only for stacks, grants, SHM, MMIO and the framebuffer (§2). Frees prune the lower-half tables instead of restoring (`task/stack.rs:83-88`; `task/syscall/grant.rs:1354-1356,1380-1382`).
- Kernel MMIO is still executable: `MMIO_KERNEL_X86` carries `EXECUTE` (`memory/paging.rs:69-73`). It is not RAM, but it is a cheap NX candidate **[I]**.

## 2. VA == PA users

| # | Kind | Site | What is identity | Arch |
|---|---|---|---|---|
| S1 | Stack allocation | `task/stack.rs:245-265` (`allocate_contiguous` `:255`, `base_addr = base_frame` `:265`) | Stack VA = run PA. The comment names the missing VA allocator. | all paged |
| S2 | Stack map | `task/stack.rs:303-305` (`map_page(addr, addr)`) | usable pages at their PA | all |
| S3 | Stack teardown | `task/stack.rs:78-103` (`retire_run` unmaps `run.base()+i`); `:124-134`; `:516-517` | the run's PA used as the VA to unpublish | all |
| S4 | Stack reservations | `task/stack.rs:16-20,345-352`; `loader/elf.rs:58` | a VA registry keyed by the same base; guards keep two frames per stack only because VA == PA (`task/stack.rs:314-339`) | all |
| S5 | `spawn_thread` / ELF spawn zeroing and watermark | `sched/scheduler.rs:1086-1095,1358-1371`; `task/elf_prepare.rs:249-256`; `task/stack.rs:427-465` | writes through the stack VA | all |
| S6 | Domain-task stacks | `memory/address_space.rs:818-842` | **not** identity: translates each VA with `virt_to_phys` | domain |
| G1 | `GrantAlloc` | `task/syscall/grant.rs:1309-1338` | contiguous run, `map_page(v, v)`, returns `paddr` | all |
| G2 | Grant zeroing | `task/syscall/grant.rs:1387-1393` | writes through `paddr as *mut u8` (USER VA) | all |
| G3 | `GrantSlice` | `task/syscall/grant.rs:2386,2445` (`Ok(access.base)`); ABI doc "same as grant_id for identity-mapped SAS" (`libs/abi/abi/syscalls.toml:896-904`) | pointer = id = PA | all |
| G4 | Grant table keys | `memory/region.rs:14-15,41-45,227-231` | the id is the run base | all |
| G5 | SAS ownership spans | `task/syscall/mod.rs:519-573` (`grant.base()` vs user `ptr`, `:559,566`) | user VA compared with the run PA | native-domains |
| G6 | Session output pin | `task/syscall/mod.rs:637` (`pin::pin(ptr, …)`) | a user VA used as a pin-registry address (`memory/pin.rs:743-755`). For segment memory the key is already a VA, not a frame **[I]**. | native-domains |
| G7 | Domain owner / receiver | `task/syscall/mod.rs:205-216`; `task/domain_grant.rs:217-222` (`map_grant_page(va, va)`); Spec 22 "Virtual identity" (`docs/specs/22-native-domain-cell-implementation-gate.md:217-222`) | the private-root VA is the grant's PA | domain |
| G8 | `ShmMap` | `task/syscall/grant.rs:1953` (`let vaddr = frame; // Identity map for SAS simplicity`), `:1964` | handle = VA = PA | all |
| G9 | `BlkReadAsync` | `task/syscall/dev.rs:1209-1214` ("Grant pages are identity-mapped (vaddr == paddr), so DMA addresses are correct") | kernel writes the grant through its PA-as-VA | all |
| G10 | `GrantDma` | `task/syscall/grant.rs:2027-2087`; ABI "→ IOVA (== phys for SAS)" (`libs/abi/abi/syscalls.toml:252-260`) | the id is taken as `phys` | all |
| G11 | `ExportDmaRange` | `task/syscall/dma_export.rs:475-484` (`base: va`), `:582,624` (`phys: base as u64`), `:829-833` | the caller's VA becomes the window's PA | x86 (remapper required, `:111-112,451`) |
| G12 | Free-path alias test | `task/syscall/grant.rs:139,1318,1495-1510` | "identity VA maps the frame" decides restore | all |
| M1 | MMIO identity (not RAM) | `memory/paging.rs:681-730` (x86 BAR), `:1254`, `:1283-1293` (`remap_range_user`); `arch/x86_64/boot.rs:745`; `task/syscall/dev.rs:1014` (framebuffer) | MMIO VA == PA, USER | x86 |
| K1 | Kernel heap DMA | `dev/virtio_hal.rs:7-10,47-48,73-75` | heap pointer = PA (RV64/AArch64 identity heap) | in-kernel virtio |
| K2 | Kernel IOMMU tables | `arch/riscv64/dev/iommu_riscv.rs:243-251,387`; `dev/iommu_pt.rs:3,77,226,606` | kernel heap VA == PA; IOVA == PA | RV64 |

**The 64 GiB limit.** Cell slots start at `CELL_VA_START = 0x10_0000_0000` (`loader/va_alloc.rs:32-49`), and the comment sizes it for identity-mapped RAM "up to 48 GiB" (`:38`). Spec 22 records the consequence for x86, where RAM starts at PA 0: a grant whose PA reaches the slots or `USER_LIMIT` (256 GiB) fails closed, and the evidence ceiling is a 256 MiB guest (`docs/specs/22-native-domain-cell-implementation-gate.md:217-222`). The same holds for stacks: a stack whose PA lands in a slot makes the loader refuse that slot (`loader/elf.rs:58`) **[I]**. `USER_LIMIT` is `1 << 38` on all three arches (`hal/arch/riscv/src/rv64/paging.rs:11`; `hal/arch/x86/src/x86_64/paging.rs:18,26`; `hal/arch/arm/src/aarch64/paging.rs:13,19`).

## 3. Assumption 1 audit: do Tier-1 cells use a stack or grant VA as a DMA address?

### 3.1 Kernel side

| Syscall | Takes | Returns | Translation |
|---|---|---|---|
| `GrantDma` 233 | `phys` must be inside a live caller-owned grant (`task/syscall/grant.rs:2057`, `reserve_caller_owned_dma_range` at `:1244`) | `iova` from `map_dma_for_cell` (`:2084-2087`) | None. The id *is* the PA. Every backend returns `IOVA == phys`: VT-d `slpt.map_range(phys, size)` (`arch/x86_64/dev/iommu_x86.rs:155-156`), no remapper (`arch/x86_64/iommu.rs:40-48`), RV64 IOMMU (`arch/riscv64/dev/iommu_riscv.rs:456`). |
| `ExportDmaRange` 437 | `va` (`libs/abi/abi/syscalls.toml:1678-1690`) | export id | `va` stored as `base`, later `phys: base` (`task/syscall/dma_export.rs:480,582,624`). Identity. |
| `MapDmaExport` 438 | export id | `ViDmaExportV1.iova` (`libs/abi/src/dma_export.rs:55-73`), a kernel-chosen window | IOVA ≠ PA by design; PA from the export (identity, above) |
| `RevokeDmaExport` 439 | export id | — | — |
| `BlkReadAsync` 212 | grant id | — | kernel writes through PA-as-VA (§2 G9) |

### 3.2 Driver cells

Classes: **a** = programs the IOVA from `GrantDma`/`MapDmaExport`; **b** = programs the raw grant id with no translation syscall; **d** = a stack/heap/static address.

| Cell | Site | Class | Arch |
|---|---|---|---|
| virtio-blk (MMIO) | `cells/drivers/virtio-blk/src/mmio.rs:42-56` (comment "`phys == virt == grant_id`") | **b** | not x86 (`cells/drivers/virtio-blk/src/main.rs:30-42`) |
| virtio-net (MMIO) | `cells/drivers/virtio-net/src/mmio.rs:47-49,70,82` | **b** | not x86 |
| virtio-gpu | `cells/drivers/virtio-gpu/src/display.rs:98-100,119,131` | **b** | MMIO |
| input service (virtio) | `cells/services/input/src/virtio_device.rs:42-50` | **b** | MMIO |
| dwc2-usb | `cells/drivers/dwc2-usb/src/usb_channel.rs:620-645` (`b.phys() + …`, then `| 0xC000_0000`) | **b** | BCM283x |
| bcm-display | `cells/drivers/bcm-display/src/mailbox.rs:59,72,88` (`dma.phys()`) | **b** | BCM283x |
| virtio-blk (PCI) | `cells/drivers/virtio-blk/src/hal_x86.rs:54-55,113,141` | a | x86 |
| virtio-net (PCI) | `cells/drivers/virtio-net/src/pci_x86/hal.rs:62-66,108,152` | a | x86 |
| virtio-scsi, nvme, ahci, e1000, e1000e, igb, igc, ixgbe, i40e, r8169, bcm57xx, vmxnet3, xhci | e.g. `cells/drivers/nvme/src/bounce.rs:42`, `cells/drivers/ahci/src/controller.rs:1013`, `cells/drivers/e1000/src/dma_layout.rs:112-121`, `cells/drivers/igb/src/dma_layout.rs:119-128`, `cells/drivers/igb/src/controller.rs:320-323` | a | PCI |
| pvscsi | `cells/drivers/pvscsi/src/dma.rs:28-31,97-101` | a, **but refuses unless `iova == buf.phys()`** | x86 |
| intel-gpu | `cells/drivers/intel-gpu/src/cell.rs:253` (`sys_grant_dma(bdf, grant.id(), …)`), `devmem.rs:153` | a | x86 |
| mesa-anv | `cells/runtimes/mesa-anv/src/memory.rs:54-59` → `libs/ostd/src/dma.rs:96` | a (export) | x86 |
| hypervisor virtqueue | guest memory accessors, no host DMA (`cells/services/hypervisor/src/virtqueue.rs:43,99,140`, per the sub-audit) | n/a | — |

Stack, heap or static addresses given to a device: **none** (class d count 0). `virtio_drivers` heap buffers are bounced through grants by the cell HALs. The cells say so themselves: "Cell image memory (heap/.bss/stack) lives at loader VAs that are NOT identity-mapped" (`cells/drivers/virtio-blk/src/mmio.rs:44-47`).

CPU dereference of the grant id (relies on id == VA, safe if the id becomes the new VA): `DmaBuf::virt()` (`libs/ostd/src/dma.rs:45-49`, used by `write_bytes`/`read_bytes` `:101-115`); the MMIO HALs above; `mesa-anv` (`memory.rs:54-55`). `GrantHandle`/`GrantSlice` users already resolve through the syscall.

ostd contracts that state VA == PA: `libs/ostd/src/dma.rs:21-28,39-46,57-61`; `libs/ostd/src/syscall.rs:870-873`.

The table rows for the 13 class-a PCI drivers, the hypervisor row and the CPU-dereference list come from a delegated read-only sub-audit. I spot-checked virtio-blk MMIO, pvscsi, dwc2-usb, the input service and every `authorize(` call site (`rg 'authorize\(|sys_grant_dma|\.phys\(\)' cells/drivers cells/services`).

### 3.3 Conclusion

- **Stack VA as a DMA address: NO.** No cell and no kernel DMA path consumes a stack PA. Moving stacks off VA == PA is internal to the kernel (§2 S1-S5).
- **Grant VA as a DMA address: YES.** Six paths program the grant id directly, all on no-IOMMU platforms. The 17 class-a cells (16 PCI drivers plus intel-gpu, and mesa-anv's export) are correct only because the kernel treats the id they pass to `GrantDma`/`ExportDmaRange` as the PA. pvscsi breaks under any VA ≠ PA because it insists on `iova == phys()`.
- Consequence: grants cannot leave VA == PA without (1) the kernel translating id → PA in `GrantDma`, `ExportDmaRange` and `BlkReadAsync`, (2) a translation path for the six class-b paths (none exists for MMIO devices: `GrantDma` requires an owned BDF), and (3) rewriting the ABI text that calls the id a physical address. Point 3 is a `libs/abi` change, so it goes to the owner (§9 Q1).

## 4. VA allocators and entropy today

### 4.1 `loader/va_alloc.rs`

| Item | Value | Site |
|---|---|---|
| Region start | `0x10_0000_0000` (64 GiB); RV32 `0x2000_0000` | `:48-51` |
| Stride | 32 MiB; 256 MiB with `large-cell-va` | `:55-58` |
| `MAX_SLOTS` | 512; 4096 with `cell-scale-experiment` | `:66-69` |
| Bound | `CELL_VA_START + MAX_SLOTS * STRIDE <= USER_LIMIT` (80, 192 or 192 GiB of 256) | `:71-72` |
| Order | free list first, lowest set bit of the lowest word (`:115-135`), then bump (`:137-143`): fully deterministic | `:114-144` |
| `reserve` | exact slot for fixed-VA images; slots skipped by a bump jump go on the free list | `:147-172` |
| `free` | rewinds the bump if this was the newest slot, else sets the free bit | `:174-190` |
| Global | `SLOTS` (`scripts/mutable-globals.toml:2486-2489`) | `:104` |
| VA hold | a slot is released only after its run's flush confirms (`memory/cell_segments.rs:5-10`) | — |

There is no arena for stacks or grants. Stacks are tracked only by `STACK_RESERVATIONS` (`task/stack.rs:12-20`), a collision registry rather than an allocator.

### 4.2 `memory/kaslr.rs`

- `kaslr_cell_base` has no caller. `rg kaslr_cell_base` finds only its definition (`memory/kaslr.rs:61`) and its doc comment. The module doc says "The ELF loader (Phase 04) calls this function" (`:8-10`); that is false.
- `init_kaslr` is called once, on x86 only (`arch/x86_64/boot.rs:97-98`). Its seed is `(hhdm ^ counter) * golden` (`memory/kaslr.rs:45-51,74-76`). On RV64/AArch64 the seed stays 0.
- The window `[0x1000_0000, 0x7FFF_0000)` with 64 KiB alignment (`:29-34`) has no slot size, no collision check and no relation to `CELL_VA_START`. Two cells can land 64 KiB apart (`:65-69`). It overlaps identity-mapped RAM where RAM starts below 2 GiB **[I]**.
- `scripts/mutable-globals.toml:1923-1931` already records "written but never consumed".

### 4.3 Kernel entropy at boot

| Source | Arch | Site | Note |
|---|---|---|---|
| RDSEED, then RDRAND | x86 | `dev/virtio_rng.rs:60-125` (CPUID `:79-84`) | bounded retries; no software fallback |
| none | RV64, AArch64 | `dev/virtio_rng.rs:52-58` | returns 0; no virtio-rng probe, no Zkr, no `RNDR` (`rg -i 'rndr|zkr'` finds nothing) |
| `GetRandom` syscall | all | `task/syscall/ipc.rs:1406-1452` | hardware first; `dev-weak-rng` xorshift fallback in default builds (`:1419-1440`; `kernel/Cargo.toml:81`) |
| AT_RANDOM for cells | all | `task/elf_prepare.rs:319-320` | `None` when no hardware entropy |
| RDTSC / `mtime` / `CNTVCT` | all | `arch/*/clock.rs:5` (`counter`) | timing only, not entropy |

CPU models with RDRAND: phase 09/10 and the Tier-3 SSH lanes add `+rdrand,+rdseed` (`scripts/qemu-phase09-ssh-test.sh:521`; `scripts/qemu-tier3-ssh-guest-x86.sh:540`). Plain `qemu64` does not **[I]**.

### 4.4 Spec 19 §3 and contiguity

- "**Contiguity binds.** Every thread's stacks need a contiguous `STACK_PAGES + 1` frame run…; the largest satisfiable run is the measurement, not free-byte totals" (`docs/specs/19-hardware-isolation-layers.md:186-189`). Today it is `pages + STACK_GUARD_PAGES` (2) (`task/stack.rs:67,197,255`), with `STACK_PAGES = 64` and `MEASURED_STACK_PAGES = 16` (`task.rs:119-121`).
- Grants are "identity-mapped, ≤ 4096 pages = 16 MiB per grant" (`docs/specs/19-hardware-isolation-layers.md:172-174`; `task/syscall/grant.rs:165`). A grant is also one contiguous run (`:1317`).
- "Deliberate absences" list a variable VA budget and demand-paged stacks (`:191-195`). A stack arena is a prerequisite for both.

## 5. SMEP, SMAP, UMIP

### 5.1 Feature chain

`kernel/Cargo.toml:181-183` (`x86-smep = ["hal/x86-smep"]`, …) → `hal/core/Cargo.toml:45-47` (`hal-x86?/x86-smep`, a weak dependency: no effect on other arches) → `hal/arch/x86/Cargo.toml:33-35` → `protect::requested()` (`hal/arch/x86/src/x86_64.rs:189-195`). The comment calls them "Off in every shipped profile" (`kernel/Cargo.toml:180`). The defaults are `dev-policy-key, dev-signing-key, dev-weak-rng, signing-required, native-domains` (`kernel/Cargo.toml:81`).

### 5.2 Where CR4 bits are set

| Site | What |
|---|---|
| `hal/arch/x86/src/x86_64.rs:157-159` | `CR4_UMIP = 1<<11`, `CR4_SMEP = 1<<20`, `CR4_SMAP = 1<<21` |
| `hal/arch/x86/src/x86_64.rs:210-229` | `init_cpu`: refuses when a requested bit is missing (`:212-215`); sets CR4 (`:222-223`); `SMAP_ACTIVE = 1` (`:224-227`) |
| `arch/x86_64/boot.rs:121-126` | BSP: `assert!(protect::init_cpu(), …)` |
| `hal/arch/x86/src/x86_64.rs:296` | AP admission: `!protect::init_cpu()` refuses the AP |
| `hal/arch/x86/src/x86_64.rs:500-503` | `ARCH.init` path (x86 `kmain` never calls it: `scripts/mutable-globals.toml:405`) |
| `hal/arch/x86/src/x86_64.rs:139-142` | the BSP/AP feature-policy word includes the three bits, so APs must match |
| `hal/arch/x86/src/x86_64/idt/entry.rs:136-141` | IDT entry: `clac` iff `ViCell_smap_active` |
| `hal/arch/x86/src/x86_64/syscall.rs:45,180` | `SYSCALL_FMASK` clears AC (bit 18) on SYSCALL |
| `hal/arch/x86/src/x86_64/idt/probe_ist.rs:375-388` | fixture-only `set_smap` toggle |
| `kernel/src/arch/x86_64/` | no CR4 writes at all: `rg -i cr4 kernel/src/arch/x86_64` finds three comments only (`boot.rs:107`, `boot/cpu_features.rs:60`, `layer2_selftest.rs:50`) |

### 5.3 Lane, script and markers

- Build: `scripts/build-x86_64-idt-test-ci.sh:12-16`. `protect` adds `,x86-smep,x86-umip` only, and the header says "SMAP has no qualifying kernel profile yet" (`:3-4`). Features `x86-idt-cpl3-test` (`:26`; `kernel/Cargo.toml:179`).
- Runner: `scripts/qemu-x86_64-idt-test.sh`. Default CPU `qemu64,+pdpe1gb,+pku` (`:12`). Markers for `on`: `X86-PROTECT: smap=probe read=pf window=ok smep=on exec=pf syscall-flags=df+ac-cleared` and `X86-PROTECT: umip=on sgdt=gp` (`:25-28`); exit status 33 (`:58`).
- Protection lane: `scripts/x86/qemu-idt-protection-test.sh` uses `qemu64,+pdpe1gb,+pku,+smep,+smap,+umip` (`:19-20`). A refusal control boots the same image on a CPU without them and requires the "unsupported by CPU" message (`:22-36`).
- Probe logic: `hal/arch/x86/src/x86_64/idt/probe_ist.rs:293-370`. With SMAP off it sets CR4.SMAP around two reads only (`:306-321`). The "`smap=on`" marker exists (`:357-358`) but no lane expects it.
- `scripts/lanes.toml:77-80,325-328,394-397`: all three are `tier = "manual"`, "current pass/fail unknown".

### 5.4 Every kernel access to user memory on x86, by window

"Window" means `task/user_copy` `GuardWindow` (`task/user_copy/guard.rs:47-73`): interrupts masked (`:57`), a recoverable-fault range published, and the copy run by `arch::user_access::guarded_byte_copy` (`:34`). It is the natural single AC setter. Today it sets nothing on x86: the only arch hook is RV64 `guard_open` (`:58-59`).

| # | Path | Touches | U=1? | Window |
|---|---|---|---|---|
| A1 | Sas commit: `task/user_copy/sas-lease.rs:91-129` → `guard.rs:16-41` → `arch/x86_64/user_access.rs:22-58` | user VA | yes | **inside** (no `stac` yet) |
| A2 | Domain commit, current root: `task/user_copy/copy.rs:122-130` | user VA in the private root | yes | **inside** |
| A3 | Domain commit, other root: `task/user_copy/copy.rs:132-184` (alias `:172`) | HHDM alias | no | inside (AC not needed) |
| A4 | Probes: `task/user_copy/page_probe.rs`, `copy_glue::validate_kernel_range` (`task/copy_glue/mod.rs:199-221`) | page-table walk via `hal::paging::leaf` | no deref | n/a |
| A5 | All syscall user I/O: `read_user_string/slice`, `write_user_slice` (`task/syscall/fs.rs:159-205`), `preflight_user_output` and its final write (`task/syscall/misc.rs:229-383`), IPC (`task/syscall/ipc.rs` `view.write_bytes`), VM, spawn, completion, gpu-scanout, ELF initial stack block (`task/elf_prepare.rs:327-329`) | via `TaskCopyView` → A1/A2 | yes | **inside** |
| A6 | `TaskCopyRepr::KernelDirect` (kernel tasks with no user stack): `task/copy_glue/mod.rs:48,138-146,171-183` | raw | no **[I]** (kernel buffers) | outside; faults closed under SMAP if ever handed a U=1 pointer |
| A7 | `alloc_grant_pages` zeroing: `task/syscall/grant.rs:1387-1393` | grant through its USER VA | **yes** | **outside** |
| A8 | `spawn_thread` user-stack zeroing: `sched/scheduler.rs:1363-1367` | user stack VA | **yes** | **outside** |
| A9 | Test-hooks watermark: `task/stack.rs:427-441,446-465` from `sched/scheduler.rs:1095,1371`, `task/elf_prepare.rs:256` | user stack VA | **yes** | **outside** (test-hooks images) |
| A10 | `BlkReadAsync`: `task/syscall/dev.rs:1209-1219` | grant through PA-as-VA | **yes** | **outside** |
| A11 | `FatFile::read_async`: `fs/fat.rs:521-535` | raw user pointer | yes | outside; no caller (`rg 'read_async\('` finds only `blk_read_async`) |
| A12 | `spawn_synthetic`: `task.rs:2776-2851` | code frame via PA | — | dead (no caller) |
| A13 | HAL `protect::user_access`: `hal/arch/x86/src/x86_64.rs:238-251` | closure | yes | a second AC setter; one caller (`hal/arch/x86/src/x86_64/idt/probe_ist.rs:316`) |
| A14 | Loader, reloc, W^X: `loader/elf.rs:344-346,368-371`; `loader/reloc.rs:115-117`; `loader/wx.rs:165-173` | HHDM | no | n/a |
| A15 | `ShmAlloc` zeroing, domain backing zeroing, framebuffer blit: `task/syscall/grant.rs:1919-1930`; `task/syscall/mod.rs:279-286`; `task/gpu-scanout.rs:215-216,240` | HHDM / kernel alias | no | n/a |
| A16 | Fault path: `vi_user_copy_guard_fault` (`task.rs:448-470`) | trap-frame registers only | no | n/a |
| A17 | Entry: SYSCALL clears AC (`hal/arch/x86/src/x86_64/syscall.rs:45`); IDT entry `clac` under SMAP, and IRETQ restores the saved AC (`hal/arch/x86/src/x86_64/idt/entry.rs:136-141`) | — | — | keeps a handler from inheriting an open window |
| A18 | RV64-only race fixture `with_test_sum`: `task/syscall/grant.rs:1573-1620` | — | — | not x86 |

The non-`native-domains` branch of `TaskCopyView` copies raw (`task/copy_glue/mod.rs:149-158,186-194`). Every x86 kernel build found carries `native-domains`: it is in `default`, `release-posture` and `production-relay-image` (`kernel/Cargo.toml:81,119,124`) **[I]** for unlisted scripts.

Landing-pad note: a guard fault inside the window enters with AC=1. The IDT entry clears it, and IRETQ restores AC=1 at the landing pad (`hal/arch/x86/src/x86_64/idt/entry.rs:136-138`). The `clac` must therefore sit in `GuardWindow::drop`, not inside the asm loop.

**Correction (review of 17b, 2026-11-16).** This table missed two U=1 stores outside the window: GrantSlice's `size_out` write (`task/user_out.rs`, a raw store to the caller's user stack) and the relocation write (`loader/reloc.rs`: on RV64 and x86 it went through the cell's USER image VA, so A14's "HHDM" was wrong for reloc). Both now go through the window or the frame's kernel alias; the full re-census is in the phase file's "Decision (review of 17b)" entry.

### 5.5 QEMU CPU models in x86 lanes

| Model | Lanes (default) | SMEP/SMAP/UMIP |
|---|---|---|
| `qemu64,+pdpe1gb` | `scripts/qemu-x86_64-test.sh:26`, `scripts/x86/qemu-domain-test.sh:56`, `scripts/run-std-smoke-qemu.sh:210`, `scripts/x86/qemu-cpp-hosted-smoke.sh:37`, phase04 TCG (`scripts/qemu-phase04-terminal-test.sh:664`) | none. This is the protection lane's own refusal control (`scripts/x86/qemu-idt-protection-test.sh:22-36`). |
| `qemu64,+pdpe1gb,+svm` | `scripts/qemu-x86-virtio-e2e.sh:115`, `scripts/qemu-x86-nginx-gate.sh:114`, `scripts/qemu-x86-python-gate.sh:80`, `scripts/qemu-tier3-hostile-runner-x86.sh:103`, `scripts/lib-qemu-ubuntu-runner.sh:54`, `scripts/qemu-hypervisor-smoke-x86.sh:74` | none **[I]** |
| `qemu64,+pdpe1gb,+pku` | `scripts/qemu-x86_64-idt-test.sh:12` | none |
| `qemu64,+…,+rdrand,+rdseed` | `scripts/qemu-phase09-ssh-test.sh:521`, `scripts/qemu-tier3-ssh-guest-x86.sh:540`, phase10 (`scripts/qemu-phase10-canonical-test.sh:33`) | none **[I]** |
| `qemu64,+pdpe1gb,+pku,+smep,+smap,+umip` | protection lane (`scripts/x86/qemu-idt-protection-test.sh:19`) | all three (TCG) |
| `max` | `scripts/qemu-cfi-test.sh:35` | all three under TCG **[I]** |
| `host` (KVM) | `scripts/qemu-x86_64-test.sh:29` (opt-in), `scripts/x86/qemu-domain-test.sh:63`, `scripts/qemu-two-node-c2c-test.sh:31`, `scripts/qemu-c2c-node-restart-test.sh:28`, `scripts/qemu-hypervisor-smoke-x86.sh:88` | the host CPU's set; UMIP needs a 2017+ Intel or a Zen 2+ AMD part **[I]** |

### 5.6 How x86 profiles pick kernel features

- `scripts/assemble-x86-image.py:60-64`: `CANONICAL_KERNEL_FEATURES = ("policy-required",)`, recorded in the image manifest (`:576`). The script builds no kernel.
- The committed manifest carries `"kernel_features": ["policy-required"]` (`kernel/src/embedded-x86_64/manifest.json:8-10`).
- Consumers: CI reads the manifest (`.github/workflows/ci.yml:1073-1079`), and so does `scripts/build-x86_64-init-death-ci.sh:53`. CI also has a hardcoded `--features policy-required` (`.github/workflows/ci.yml:1341`). The ~35 other x86 kernel builds use `default` plus their own features (§5.5; `rg -c -- --no-default-features` returns 0 for all but four scripts).
- Implication: putting the three features in `kernel/Cargo.toml` `default` reaches every x86 build at once and does nothing on other arches (weak dependency, §5.1). `CANONICAL_KERNEL_FEATURES` reaches only manifest-driven builds. Cargo features are additive, so an "off-switch" for a default-on feature needs either `--no-default-features` or a negative feature **[I]**.

## 6. Spec statements on W^X, ASLR and SMAP

| Spec | Statement | Site |
|---|---|---|
| 19 Layer A | Kernel writes into cell memory "go through the physical/HHDM alias, which is kernel-RW independently of the USER PTE". This is the W reliance in §1.2. | `docs/specs/19-hardware-isolation-layers.md:55-57` |
| 19 Layer A | Limits: heap, stack and `.data` remain USER+RW across cells in the SAS | `:62-64` |
| 19 Layer C | x86 hardening is "opportunistic"; only MPK and MTE are named; nothing about SMEP/SMAP/UMIP | `:98-110` |
| 19 §3 | Contiguity binds stacks; grants identity-mapped; the VA allocator "hands out distinct slots" | `:139-140,171-174,186-189` |
| 19 | No ASLR or KASLR statement (`rg -i aslr` finds nothing) | — |
| 02 §3.1 | Release step 6: "Normalise: RV64/AArch64 restore the kernel RWX identity alias" | `docs/specs/02-memory.md:76` |
| 02 §5 | W^X is code integrity only; stack, heap, grants and MMIO stay USER+RW across cells | `:101-133` (limit at `:119-121`) |
| 02 §1 | Layout table: RV64 "HHDM, high-half" (wrong for RV64) | `:14` |
| 02 | No ASLR or SMAP statement | — |
| 22 | `GrantAlloc` returns an identity-mapped PA; receiver VA = grant PA; the x86 64 GiB / `USER_LIMIT` limit | `docs/specs/22-native-domain-cell-implementation-gate.md:21-25,217-222` |
| HAL doc | "SMAP is only selectable for an image whose kernel never touches U=1 memory outside a `user_access` window" | `hal/arch/x86/src/x86_64.rs:146-152` |

## 7. Design proposal (part 17b)

Each step is one commit and one lane cycle, in this order. Every step reverts on its own.

### Step 4 (plan step 3): free frames RW+NX

- **Files.** `memory/paging.rs:147-154` (`Usable` loses `EXECUTE`); `memory/frame_run.rs:1557-1564` (`kernel_rw`) and the doc lines in §1.3; Spec 02 §3.1 step 6. No x86 change.
- **Fixture.** A test-hooks boot check (RV64 and AArch64): `hal::paging::leaf` on (1) a frame just allocated and (2) a frame freed after a `Removed` run (allocate a user stack, drop it, re-read the leaf) requires `valid && write && !exec && !user`. Add a host test in `memory/frame_run_tests.rs` asserting that the backend's normalise flags carry no `EXECUTE`.
- **Lanes.** `scripts/qemu-boot-test.sh`, `scripts/build-test-hooks-ci.sh` (RV64), `scripts/qemu-aarch64-test.sh`, and the native-domain `grant-pair` case (`scripts/qemu-native-domain-test.sh`, manual tier), because grants restore the alias. x86 needs `scripts/qemu-x86_64-test.sh` only as a no-change control.
- **Rollback.** Revert the two flag edits. Nothing else depends on them.

### Step 5a (plan step 4, stacks): a stack VA arena

- **Arena.** A second `SlotAllocator` in `loader/va_alloc.rs` (or a sibling `memory/va_arena.rs`), placed above the cell slots: `[CELL_VA_START + MAX_SLOTS * CELL_VA_STRIDE, USER_LIMIT)`, which leaves 176 GiB by default and 64 GiB with `large-cell-va` or `cell-scale-experiment` (§4.1). Use one stride sized for `STACK_PAGES + STACK_GUARD_PAGES` rounded to a power of two (512 KiB). Assert at compile time that it fits. Assert at boot that the arena lies above the top of identity-mapped RAM and MMIO (as `task/syscall/dma_export.rs:957` does for export windows).
- **Stack.** Keep one contiguous `FrameRun` in the first commit, so Phase 16's release contract is unchanged, but map it at `arena_va + guard`. The guard becomes an unmapped VA with no frame behind it, which saves two frames per stack. Split VA from PA in `retire_run`/`unpublish_user_frames` (`task/stack.rs:78-134`): unmap the VA range, free the run. The alias becomes `Intact` (the identity leaf is never taken). The VA returns through a `VaHold` (already used for slots). `STACK_RESERVATIONS` stays the loader's collision view of the arena. A second commit may then drop physical contiguity (one run per page or a run list). That removes Spec 19 §3's "contiguity binds" for stacks.
- **Sites.** `task/stack.rs:196-400,498-612`; `sched/scheduler.rs:1086,1358-1371`; `task/elf_prepare.rs:246-256` (zero the user stack here too, through the HHDM/identity alias: G12); `task/syscall/mod.rs:519-573,637` (pin by PA, not by user VA); `memory/address_space.rs:818-842` (already translates).
- **Fixture.** Extend `stack_probe_self_test` (`task/stack.rs:469-496`): `virt_to_phys(stack.usable_start()) != Some(stack.usable_start())`, guards unmapped, a write at `top - 8` lands in the run's frame (read back through `phys_to_virt`). Add a two-thread fixture: a thread's stack VA comes from the arena and survives spawn/exit 512 times without leaking slots (the `va_alloc::snapshot` hook, `loader/va_alloc.rs:216-226`).
- **Lanes.** RV64 boot and test-hooks, AArch64 boot, `scripts/qemu-x86_64-test.sh`, `scripts/x86/qemu-domain-test.sh` (domain stacks), and the silent-CPU quarantine fixture (stack retire under an unconfirmed flush).
- **Rollback.** Revert the commit. The arena is new code, and stacks fall back to `allocate_contiguous` + identity.

### Step 5b (plan step 4, grants): blocked on owner decision Q1

- **If approved as "x86 first".** Grants on x86 use a grant arena; the id becomes the arena VA. `GrantDma`, `ExportDmaRange` and `BlkReadAsync` translate id → run PA (§2 G9-G11). `region.rs` keeps the id as its key and the run as its frames. `sas_record_end` compares against the VA. pvscsi drops its `iova == phys()` check (`cells/drivers/pvscsi/src/dma.rs:30,99`). ostd docs follow. RV64/AArch64 keep identity grants: the six class-b paths have no translation syscall, so they would need a new one (an ABI addition) or a per-arch exemption.
- **Fixture.** An x86 driver lane must still pass end to end: `scripts/qemu-x86-virtio-e2e.sh` (virtio-blk/net PCI) and an NVMe or AHCI storage lane. A negative fixture: `GrantDma` with a PA that is not the caller's grant id is refused.
- **Rollback.** Revert the commit. Keys and ids return to the PA.

### Step 6a: SMEP on

- **Files.** `kernel/Cargo.toml:81` (add `x86-smep` to `default`, or to `CANONICAL_KERNEL_FEATURES` per §5.6). Every x86 lane CPU model in §5.5 gains `+smep`.
- **Fixture.** The existing protection probe (`hal/arch/x86/src/x86_64/idt/probe_ist.rs:335-345`, marker `smep=on exec=pf`) plus a full boot of `scripts/qemu-x86_64-test.sh` with `qemu64,+pdpe1gb,+smep`. The refusal control (`scripts/x86/qemu-idt-protection-test.sh:22-36`) stays.
- **Rollback.** Remove the feature from `default`. The feature remains as the switch.

### Step 6b: SMAP with one AC setter

- **Window.** Arm `stac` in `GuardWindow::arm` after interrupts are masked, and `clac` in its `Drop` (`task/user_copy/guard.rs:52-73`), only when `SMAP_ACTIVE` is set. Make HAL `protect::user_access` the single primitive that both the window and the IDT probe call, or restrict it to the fixture. Either way, `rg '"stac"'` must find exactly one site.
- **Convert the outside sites first.** A7: zero through `phys_to_virt` before `map_page`, as `ShmAlloc` does. A8 and A9: zero and watermark through the run's kernel alias. A10: write through `phys_to_virt(run PA)`. A11, A12: delete. Update the `run_kernel_protection` comment (`hal/arch/x86/src/x86_64/idt/probe_ist.rs:306-308`) and the build-script header (`scripts/build-x86_64-idt-test-ci.sh:3-4,14`).
- **Fixture.** The protection lane builds with `x86-smap` and expects `X86-PROTECT: smap=on read=pf window=ok` (the marker exists at `probe_ist.rs:357-358`). That proves a raw kernel read of a U=1 page faults outside the window. Add a kernel-level fixture: a `GetRandom` or `Log` syscall from a SAS cell succeeds under SMAP (the window works), and a test-hooks raw read of the same user page outside the window takes `#PF` with `PF_PRESENT` and no `PF_FETCH`. Because of G7, the success criterion must be a *successful* copy marker, not merely "no fault".
- **Lanes.** `scripts/x86/qemu-idt-protection-test.sh`, then `scripts/qemu-x86_64-test.sh`, `scripts/x86/qemu-domain-test.sh` (private-root commit A2), `scripts/qemu-x86-virtio-e2e.sh`, and the test-hooks x86 build (A9).
- **Rollback.** Drop `x86-smap` from `default`. The window code stays: it is inert while `SMAP_ACTIVE == 0` (`hal/arch/x86/src/x86_64.rs:239-241`).

### Step 6c: UMIP on

- **Files.** The feature list and CPU models gain `+umip`.
- **Fixture.** The existing `sgdt` trap (`hal/arch/x86/src/x86_64/idt/probe_ist.rs:177-184,391-408`, marker `umip=on sgdt=gp`). No cell or lib source uses `sgdt`/`sidt`/`sldt`/`smsw` (`rg -i 'sgdt|sidt|sldt|smsw' cells libs` is empty). Foreign binaries (mlibc, Tier-2 C code) are not covered by that search **[I]**.
- **Rollback.** Drop the feature.

### Step 7: slot randomisation; delete `kaslr_cell_base`

- **Delete.** `memory/kaslr.rs`, `memory.rs:22`, the call at `arch/x86_64/boot.rs:97-98`, and `scripts/mutable-globals.toml:1923-1931`. Wiring it is not viable (§4.2).
- **Randomise.** `SlotAllocator::alloc` (`loader/va_alloc.rs:114-144`) takes a random start index from `dev::virtio_rng::get_random`, read once per allocation or from a boot-seeded per-boot stream, and probes for the first free slot from there. With no entropy it keeps today's order and logs once. Apply the same to the stack (and grant) arenas. `reserve` is unchanged for fixed-VA images. Optional: a page-granular offset inside the slot for small images.
- **Fixture.** On x86 with `+rdrand,+rdseed`, boot the same image twice. A named cell's base (logged at spawn) must differ. A control run on plain `qemu64` must log "no entropy" and keep the deterministic order. `va-alloc-tests.rs` gains host tests: a seeded allocator never hands out an owned slot, and `free`/`reserve` keep the ownership invariant (`loader/va_alloc.rs:92-94`).
- **Lanes.** `scripts/qemu-x86_64-test.sh` with `X86_CPU_MODEL=qemu64,+pdpe1gb,+rdrand,+rdseed` twice, plus RV64/AArch64 boot (deterministic path).
- **Rollback.** Revert. The order returns to bump-first.

## 8. Risks

- **Silent EFAULT under SMAP (G7).** A missed `stac` does not crash; the guard turns the `#PF` into `CopyError`. Lanes that only check "no FAULT" would pass while syscalls fail. The SMAP fixture must assert a successful copy.
- **Boot refusal (G9).** Default-on with today's `init_cpu` semantics refuses every lane CPU model in §5.5 and any physical CPU lacking UMIP. All lane `-cpu` strings change in the same commit, or the semantics change (§9 Q2).
- **Hidden raw user writes.** The A-table is built from `rg` over `write_bytes|copy_nonoverlapping|from_raw_parts|read_volatile|write_volatile` and raw casts in `kernel/src` and `hal/arch/x86`. A deref through a typed `&mut T` on a user VA would not appear **[I]**. The SMAP lane run is the real census.
- **Arena collision with identity-mapped RAM.** On RV64/AArch64 the arena VA range is also where large-RAM identity maps would reach (above 80 GiB). The boot check in step 5a must refuse rather than overlap.
- **Pin registry keys.** Pins are keyed by address. Today VA == PA hides the difference for grants and stacks; segment-backed output pins already key by VA (§2 G6) **[I]**. Mixing keys after step 5a would make `withhold_pinned_frames` miss a held stack frame.
- **ELF user stack not zeroed (G12).** Fixing it changes behaviour: report it before converting A8/A9.
- **AArch64 Pi EL2 monitor.** NX on data pages sets bit 54. If the monitor ever fetches from a non-kernel-text page, it aborts once `SCTLR_EL2.M` is set (`hal/arch/arm/src/aarch64/paging.rs:110-118`) **[I]**. Step 4 must boot the Pi profile or its QEMU stand-in.
- **Entropy ceiling.** Randomisation is x86-only. RV64/AArch64 stay deterministic until a kernel entropy source exists (G11). The success criterion's "when entropy is available" must be read that way.
- **Grant ABI.** Without an owner decision, step 5b cannot land: it changes `libs/abi` documentation and pvscsi behaviour.

## 9. Open questions for the owner

1. **Grants off VA == PA (step 5b).** Moving grants changes what `libs/abi/abi/syscalls.toml` documents: `GrantAlloc` "→ base_paddr", `GrantRegister` "physical base", `GrantDma` "a1 = phys … IOVA (== phys for SAS)", `GrantSlice` "same as grant_id for identity-mapped SAS". No number or wire byte moves. Six no-IOMMU DMA paths have no translation syscall at all. Options:
   - (a) Stacks only in this phase. Grants keep VA == PA, and the residual is recorded.
   - (b) Grants move on x86 only. The ABI text is revised to "grant address"; the kernel translates in `GrantDma`/`ExportDmaRange`/`BlkReadAsync`; RV64/AArch64 keep identity grants.
   - (c) As (b) on every arch, with a new translation syscall for MMIO/platform DMA (an ABI addition).

   Recommended: (a) now and (b) as a follow-up, because the approval forbids `libs/abi` changes in this phase.
2. **CPU without a requested protection.** Today the image refuses to boot. Once SMEP/SMAP/UMIP are default-on, that semantics refuses the current lane models and older hardware. Options:
   - Keep refusal: every lane CPU model changes, and pre-UMIP hardware cannot boot the canonical image.
   - Enable what the CPU has and log the rest, keeping refusal for an explicit "required" feature.
   
   This is a boot-policy decision. The approval left it open.
3. **ELF user-stack zeroing (G12).** Zeroing the ELF spawn's user stack is a behaviour change outside §4 of the request. Approve it as part of step 6b's conversion, or record it for a later phase?
