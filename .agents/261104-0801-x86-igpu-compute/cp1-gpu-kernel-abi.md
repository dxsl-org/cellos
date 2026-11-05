# CP1 — GPU driver kernel authority: client-memory DMA export, revocation, completions, service entry

Status: **Law-1 confirmation 1 APPROVED by the owner on 2026-11-04 ("duyệt theo khuyến nghị"), exactly as the §10 recommendations.** The lock handoff is approved but **not yet effective** ([approval record](cp1-approval.md)). CP2 is required after implementation. Before approval this was a draft, written 2026-11-04 against HEAD `7fdc088af`. The working tree is dirty with x86-breadth S1 and doc edits by other sessions (`kernel/src/task.rs`, `kernel/src/task/drivers/{pcie_ecam,irq_wait}.rs`, `kernel/src/main.rs`, …); every Cellos `file:line` below was read in this session from that working tree and may shift when S1 commits. Upstream `file:line` refer to `torvalds/linux` master, Mesa main and QEMU `v8.2.2` as fetched on 2026-11-04; every upstream file cited carries an MIT header (checked before reading). `[inference]` marks claims not read. `D-Gn` marks owner decisions (§4, summary §10).

Serves: [ADR-0033](../../docs/decisions/0033-x86-intel-igpu-native-compute.md) §3-4, [phase G1](phase-01-xe-uapi-service-and-kernel-abi.md) "Kernel ABI inventory", [phase G2](phase-02-igpu-driver-cell.md) requirement "mappings only of client-exported buffers; revocation on client death before frame release".

## 1. Scope

**In:**
- three append-only syscalls `ExportDmaRange = 437`, `MapDmaExport = 438`, `RevokeDmaExport = 439` and the record `ViDmaExportV1` (48 bytes);
- one service-registry constant `service::XE_GPU = 21`;
- kernel-internal (no wire change) rules: export holder table, owner-watch grant, death ordering, IOVA windows, read-only VT-d leaves;
- the reuse decision for completions (no new wait primitive or completion source);
- prerequisites and lock relations.

**Out:**
- any GPU request, structure or semantics: those are the existing `xe` uAPI served over IPC (G1 subset table, `xe-uapi-subset.md`);
- Tier-2 large anonymous memory on x86 (prerequisite D-G0, its own CP1);
- PASID / scalable-mode VT-d, SR-IOV VF assignment, dma-buf sharing between clients, discrete-GPU VRAM.

## 2. Existing primitives (survey)

| # | Fact | Evidence |
|---|---|---|
| E1 | Highest syscall in code is `AllocPcieIrq = 435`; `SystemPower = 436` is approved at design level only and not in the enum. No 436-439 use anywhere in `libs/api`, `libs/ostd`, `kernel/src/task/syscall.rs` (grep). Next free: **437** | `libs/api/src/abi/syscall.rs:684,698`; `cp1-approval.md:21` |
| E2 | Allowlist full: bits 0-62 assigned, 63 = VFS-mutate. Precedents: share a bit (434/435 on bit 50) or no bit with a dispatch gate (431/432/433, 245/248, `CellSessionV1`) | `libs/api/src/abi/syscall.rs:1204-1220,1258-1290` |
| E3 | `GrantDma = 233` (bit 48) maps a range only if it lies inside a **caller-owned** live Grant (page or registered), the caller owns the BDF, DMA quota allows, and a pin is taken. `shared_to` does not authorize. Domain (private-root) callers are refused | `kernel/src/task/syscall.rs:762-784,786-867,7861-7959`; `syscall.rs:89,1196` (api) |
| E4 | `ostd::dma::DmaBuf` = `GrantAlloc` (physically contiguous) + `GrantDma`; IOVA returned verbatim; no unmap call exists — DMA mappings end only at death or `CapRevoke(PCIE_DRIVER)` | `libs/ostd/src/dma.rs:3-5,33-64`; `cells/drivers/nvme/src/dma.rs:3-18`; callers of `unmap_dma`: none outside `iommu.rs` (grep) |
| E5 | Grants: ≤ 4096 pages (16 MiB) per `GrantAlloc`/`GrantRegister`, physically contiguous; one grantee at a time | `kernel/src/task/syscall.rs:257-259,900-911,873-885` |
| E6 | x86 has **no domain grant lifecycle** ("no root-switch witness"); a record naming a private-root endpoint is refused; SAS-owned record to a domain receiver refused | `kernel/src/task/syscall.rs:327-348,662-682,3666-3672` |
| E7 | VT-d domains are **per driver task** (`VTD_DOMAINS: tid → {SLPT, DID, BDFs}`); identity IOVA (`IOVA == phys`); one requester ID names exactly one domain (stale owners removed); legacy context entries, `TT = 00` (untranslated only, no ATS/device-TLB); 39-bit AW; no PASID/scalable mode (grep) | `kernel/src/task/drivers/iommu_x86.rs:1-9,44-55,92-98,592-730,493-503`; `iommu_pt.rs:1-7` |
| E8 | Every SLPT leaf is `R|W` — no device-read-only mapping | `kernel/src/task/drivers/iommu_pt.rs:199-200,231` |
| E9 | IOTLB invalidations carry drain-read/drain-write (`DRD|DWD`); a timed-out invalidation is reported, never assumed (`PublishedUnconfirmed`, `DmaUnmapResult`) | `iommu_x86.rs:36-42,252-264,312-341`; `iommu.rs:61-99` |
| E10 | Driver death / `PCIE_DRIVER` revoke: quiesce every owned function (BME off, MSI off, MSI-X masked) → release vectors → revoke and fence the whole domain → release BDFs → release acknowledged frames; unacknowledged → retried each scheduler pass, BDF and frames stay fail-closed | `kernel/src/task.rs:1898-1959,1961-2004,2013-2033` |
| E11 | Pin registry: pinned frames are never freed by the owner and are **quarantined** on owner death until the pin holder acknowledges; fixed tables `MAX_PINNED_REGIONS = 128`, `MAX_PINS_PER_TASK = 48`, quarantine 64 | `kernel/src/memory/pin.rs:1-26,50-65,483-520,686-706,764-843` |
| E12 | VFS lease = closest precedent for "service holds a client's memory": holder/owner/generation key, `pending_revoke` on owner death, frames quarantined under the lease key until the holder releases | `kernel/src/memory/pin.rs:845-961`; `kernel/src/task/syscall.rs:1674-1695` |
| E13 | Death reap order: `teardown_dma_authority` → IRQ waiters → `reap_grants_for_task` (quarantine pins, clear grantee refs, reclaim owned grants) | `kernel/src/task.rs:2013-2033`; `kernel/src/task/syscall.rs:1228-1251,1278-1322,1343` |
| E14 | `WaitCompletion = 242` sources `NET_RX | TIMER | IPC`; `IPC` reports one terminal per owned `IpcSubmit` operation bound to `(cell, generation)`; surface FROZEN by slice-d CP2 (item-level check) | `libs/api/src/abi/completion.rs:48-80`; `libs/api/src/abi/syscall.rs:751-783,823-845`; `scripts/check-multi-source-wait-law1-digests.sh:1-38` |
| E15 | Async IPC: services defer with `IpcCurrent` + `IpcReply`; ≤ 64 outstanding operations per caller; kernel-enforced terminal deadline 3000 ticks (30 s) | `kernel/src/task/syscall.rs:5198-5235`; `kernel/src/task/async_ipc.rs:14-18,376` |
| E16 | `WaitIrq = 234` (bit 51) parks one task on one IRQ; `MAX_IRQ = 256` (S1 working tree) | `libs/api/src/abi/syscall.rs:90-95`; `kernel/src/task/drivers/irq_wait.rs:1-18,27` |
| E17 | `NotifyOnExit = 204` needs `SpawnCap`. `WatchCellOwner = 245/248` allowed to the VFS cell and the live registry-bound provider of a service in `OWNER_WATCH_SERVICES = [NET, COMPOSITOR]`, only for the principal it is currently serving | `libs/api/src/abi/syscall.rs:287-292,790-807`; `kernel/src/task/scheduler.rs:289-318,765-790`; `kernel/src/task/syscall.rs:7353-7373` |
| E18 | Service IDs 1-20 assigned, next free 21. `GPU_DRIVER = 12` is the VirtIO-GPU role: `PcieDriverCap` self-registration also makes the kernel forward `GpuFlush`/`GpuCursor` to it. `RegisterService` otherwise needs `SpawnCap`; `LookupService(Bound)` open to all | `libs/api/src/abi/syscall.rs:1483-1548`; `kernel/src/task/syscall.rs:6864-6965`; `kernel/src/task/drivers/driver_cell.rs:80,279` |
| E19 | `PcieDriverCap` check and BDF ownership helpers | `kernel/src/task/syscall.rs:2198`; `kernel/src/resource_registry.rs:894-911` |
| E20 | Tier-2 x86 cells: private-root domains (`switch_ordering_qualified` true on x86); mlibc anonymous memory is a static 4 MiB bump arena; `mmap` only `MAP_ANONYMOUS` | `kernel/src/loader/domain_admission.rs:1-14,154-158`; `third_party/mlibc/sysdeps/vicell/generic/generic.cpp:32,156-183`; `docs/guides/posix-shim-contract.generated.md:141,286` |
| E21 | Error sentinels in use for 434/435: `MAX` denied, `MAX-2` not found, `MAX-3` invalid, `MAX-4` buffer too small, `MAX-1` try again, `MAX-5` not supported, `MAX-6` exhausted | `kernel/src/task/syscall.rs:1725-1746`; `libs/api/src/abi/pcie_function.rs:68-76` |

Upstream facts used:

| # | Fact | Source (MIT) |
|---|---|---|
| U1 | `xe` request numbers `DEVICE_QUERY 0x00` … `VM_GET_PROPERTY 0x0f`; `VM_BIND` ops `MAP`, `UNMAP`, `MAP_USERPTR`, `UNMAP_ALL`, `PREFETCH` | `include/uapi/drm/xe_drm.h:97-112,1169-1173` |
| U2 | A user fence passed through `VM_BIND` is a CPU pointer written by the KMD; through `EXEC` it is a GPU VA | `xe_drm.h:1582-1590` |
| U3 | `xe` DMA-maps every BO through the device's DMA API (one device domain), not per process | `drivers/gpu/drm/xe/xe_bo.c:398-425` (`xe_tt_map_sg`) |
| U4 | `xe` userptr: an MMU-notifier invalidation zaps the GPU PTEs and waits for GPU TLB invalidation before the core MM may free the pages | `drivers/gpu/drm/xe/xe_userptr.c:102-148,220-242` |
| U5 | Panther Lake / Wildcat Lake: `dma_mask_size = 46`, `va_bits = 48`, `vm_max_level = 4`, `has_sriov` | `drivers/gpu/drm/xe/xe_pci.c:441-454,537-538` |
| U6 | No `iommu`/`vtd` workaround in `xe_ggtt.c`, `xe_bo.c`, `xe_vm.c`, `xe_device.c`, `xe_pci.c` (grep) | same files |
| U7 | anv creates every BO with `GEM_CREATE`, maps with `GEM_MMAP_OFFSET` + `mmap(fd, MAP_SHARED[, MAP_FIXED])`, and uses `MAP_USERPTR` only for `from_host_ptr` BOs (`VK_EXT_external_memory_host`); `gem_create_userptr` returns the workaround BO handle | Mesa `src/intel/vulkan/xe/anv_kmd_backend.c:128,148-190,205-245,426-433` |
| U8 | QEMU `edu` (1234:11e8): DMA registers 0x80 src, 0x88 dst, 0x90 count, 0x98 cmd (bit0 run, bit1 direction, bit2 IRQ); device buffer 0x40000+4096 (out-of-range aborts QEMU); host address clamped to `dma_mask` (default 28 bits, property `dma_mask`); MSI | `hw/misc/edu.c:44-45,65-68,111-167,220-229,374,411-413,424`; local `qemu-system-x86_64 -device edu,help` lists `dma_mask` |

## 3. What is missing

| Need (task) | Today | Gap |
|---|---|---|
| (a) GPU maps pages of a client buffer the client explicitly exported | `GrantDma` maps only caller-owned grants (E3); x86 domains cannot share grants at all (E6); identity IOVA (E7) | **No cross-Cell device-DMA authority.** New authority: client export → driver map |
| (b) revoke + device quiesced before client frames are freed | Only whole-domain revoke at driver death (E10); owner death quarantines only the owner's *own* pins (E11) | **No per-range revoke tied to another Cell's death or unexport** |
| (c) completions the client can wait on | `IpcSubmit`/`IpcReply` + `WaitCompletion(IPC)` (E14-E15) | None (reuse; D-G6) |
| (d) clients find the driver | Registry + `LookupServiceBound` (E18) | One constant (D-G8) |
| Client big memory (prereq) | 4 MiB arena on x86 Tier-2 (E20) | **D-G0**, outside this package |

## 4. Decisions

### D-G0 — Prerequisite: large memory for x86 Tier-2 clients (not decided here)

anv + llama.cpp run as a Tier-2 private-root Cell (phase G3). That Cell can own at most a 4 MiB arena today (E20), so neither the client's CPU-side model data nor any exported buffer can exist. Every option in D-G1 needs some new x86 Tier-2 memory primitive.

| Option | Effect | Assessment |
|---|---|---|
| (i) General Tier-2 anonymous mapping (`mmap(MAP_ANONYMOUS)` backed by kernel page allocation in the private root, scattered frames, contiguous VA) with its own CP1 in the Tier-2/POSIX lane | llama.cpp heap works; D-G1 option B exports those pages | **Recommended.** Not GPU-specific; must add one invariant for this package: a VA range under a live export cannot be unmapped or re-protected (§6) |
| (ii) Allocate-and-export in one GPU-motivated syscall | Narrower | Rejected: a memory API hidden inside a device API |
| (iii) Run anv + llama.cpp as Tier-1b SAS (`PROTECTION_CLASS_FFI`) | Grants work, 16 MiB cap remains | Contradicts phase G3 "untrusted Tier-2"; owner-only choice |

### D-G1 — Who owns buffer-object frames

| Option | Kernel delta | For | Against |
|---|---|---|---|
| **A. Driver-owned BOs lent to the client** (xe GEM semantics: `GEM_CREATE` allocates in the driver, `mmap` maps driver frames into the client) | Cross-root **CPU** lending SAS → x86 domain at a contiguous VA for scattered frames, revocable; >16 MiB BOs need multi-grant BOs (raise pin limits, E11) | No new device-DMA authority: the GPU reaches only driver-owned memory; client death only drains client PTEs | Needs the x86 domain receiver lifecycle that is closed for lack of a root-switch witness (E6); all client GPU memory charged to the driver's quota; mmap needs a new cross-root mapping call anyway |
| **B. Client-owned memory exported to the driver's VT-d domain** (xe userptr-like at kernel level; the shim serves `GEM_CREATE` by allocating locally, exporting, and sending the unchanged `drm_xe_gem_create` with the export id as a transport attachment, like fd passing; `GEM_MMAP_OFFSET`/`mmap` resolve to the client's own pages) | 437-439 (§5); kernel-only revoke | No cross-root CPU mapping (sidesteps E6); memory charged to the client; revocation fully kernel-enforced; same primitive also implements `MAP_USERPTR` (U7) | New authority: a device writes into another Cell's frames; shim carries an attachment beside the xe struct |
| C. Existing grants only | none | — | Fails: 16 MiB contiguous per grant (E5), 48 pins per task (E11) → ≤ 768 MiB device-visible; domain clients cannot share (E6) |

**Recommendation: B.** It is the shape ADR-0033 §3 already names, needs no x86 cross-root CPU shootdown, and keeps memory accounting with the client. A remains a valid alternative if the owner prefers "no device DMA into another Cell's frames" over "no cross-root CPU mapping"; it would need its own lending syscall and the x86 domain-receiver lifecycle first. XeUapiSubset notes that anv's normal path is A-shaped (U7): under B the transport shim, not anv, absorbs the difference.

### D-G2 — VT-d domain topology

| Option | Assessment |
|---|---|
| One VT-d domain per client | **Not possible** with legacy context entries: the iGPU is one requester ID (00:02.0), and one RID names exactly one domain (E7). Needs scalable mode + PASID-tagged GPU DMA, which Cellos does not implement and `xe` does not use for PPGTT (U3) `[inference for the GPU side]` |
| One VT-d domain per client via SR-IOV VFs (`has_sriov`, U5) | Each VF has its own RID, but VF provisioning needs a PF driver and GuC VF scheduling; out of scope |
| **Shared holder domain + per-client GPU PPGTT** | Matches `xe` (U3). Kernel bound: the GPU reaches only (driver-owned grants) ∪ (live exports to this driver) ∪ (RMRR ranges). Isolation *between* clients is the driver's PPGTT discipline (Tier-1 driver is trusted for its device, as today for NVMe) |

**Recommendation:** shared holder domain. The kernel guarantees "never a frame the client did not export, never after revoke"; the driver guarantees "client X's exports appear only in client X's VMs". Recorded as an explicit trust statement.

### D-G3 — IOVA assignment for exports

| Option | Assessment |
|---|---|
| Identity (`IOVA == phys`, as today, E7) | Needs a page-list record per export (1 GiB = 262,144 entries) and a paged query; discloses scattered physical addresses; GPU cannot use 64 KiB/2 MiB PTEs |
| **Kernel-allocated contiguous IOVA window per export** in the holder's domain, from a region disjoint from identity ranges | One `{iova, len}` per export; no physical addresses to the driver; contiguous IOVA lets the PPGTT use large pages when frames allow `[inference]`. Needs a non-identity `map(iova, phys, rights)` in `iommu_pt.rs` and a per-domain window allocator (kernel-internal) |

**Recommendation:** window. Region `[2^38, 2^39)` of the 39-bit AW (E7), inside PTL's 46-bit DMA mask (U5). The kernel refuses to enable exports if the highest identity-mapped frame reaches `2^38` (logged; a machine above 256 GiB RAM needs a wider AW first).

### D-G4 — How the export names its holder

| Option | Assessment |
|---|---|
| Holder tid | TOCTOU against restart; tid reuse |
| **Registry service id + kernel table `DMA_EXPORT_SERVICES`** (kernel-internal, initially `[XE_GPU]`; test-hooks builds add a test id) | Kernel binds the lease to the provider's `(cell, generation)` at export time (`lookup_bound`); a restarted provider is a different holder. Mirrors `OWNER_WATCH_SERVICES` (E17): each entry is an owner decision |
| CSpace capability object transferred over IPC | Long-term shape under CELL-CONTRACT-0024 P05 typed handles; not available now |

**Recommendation:** service id + table, provider must hold `PcieDriverCap` at export time. A later P05 typed handle can replace the id without changing the lease semantics.

### D-G5 — Revocation model

| Option | Assessment |
|---|---|
| Driver-acknowledged (Linux MMU-notifier model, U4): kernel asks, driver zaps PPGTT + GPU TLB, then kernel frees | Death would wait on a possibly-wedged GPU driver; breaks the "death is never delayed" rule (E11) |
| **Kernel-only**: kernel clears the window's SLPT leaves and issues a domain IOTLB invalidation with drain (E9); frames are released only after acknowledgement; otherwise quarantined and retried (E10/E11 pattern). The driver learns of client death through D-G7 and tears down GPU state on its own schedule | Never waits on the driver; GPU accesses after revoke fault at VT-d instead of reaching freed frames. The driver must treat a VT-d/GPU page fault on a dead client's VM as expected (ban/reset that context) |

**Recommendation:** kernel-only. "Device quiesced" for the revoked range means "remapper drained and translation removed", not "GPU idle"; whole-device quiesce stays the existing driver-death path (E10). TT = 00 means no device-TLB to invalidate (E7).

### D-G6 — Completions

| Option | Assessment |
|---|---|
| **Reuse**: client shim submits each blocking `xe`/DRM wait (`WAIT_USER_FENCE`, syncobj waits) with `IpcSubmit`; the driver defers with `IpcCurrent` and answers with `IpcReply` from its MSI-X path; the client parks in `WaitCompletion(IPC | TIMER)` or `IpcWait` and takes the result with `IpcTake` | Zero ABI delta; frozen slice-d surface untouched (E14). Constraints: 30 s kernel deadline per operation and ≤ 64 outstanding operations per caller (E15) → the shim re-arms waits longer than 30 s (DRM waits are restartable with an absolute timeout) and bounds in-flight waits |
| New `completion::source::GPU` written by the kernel on GPU interrupt | The kernel does not know GPU fences; would put GPU semantics in the kernel (violates "no new GPU API") |
| `WaitCompletion` source for IRQs (driver side) | Would let one driver thread wait on IRQ + IPC; not needed if the driver uses a dedicated IRQ task parked in `WaitIrq` that triggers `IpcReply` `[inference: a sibling thread of the same Cell may reply for the service generation; confirm in G2]` |

**Recommendation:** reuse. User fences written by the GPU through `EXEC` land in exported client memory and can be polled; fences the KMD writes through `VM_BIND` (U2) are written by the shim locally when the reply arrives.

### D-G7 — Driver learns that a client died

| Option | Assessment |
|---|---|
| **Add `XE_GPU` to `OWNER_WATCH_SERVICES`** (kernel-internal const), driver calls `WatchCellOwner(Record)` (245/248) while serving the client's first request | Existing gate: live registry-bound provider, only for the principal it is serving (E17); precedent NET/COMPOSITOR |
| Implicit holder notification for every export | Covers only clients with exports; contexts/VMs without exports would leak |
| `NotifyOnExit` | Needs `SpawnCap`; rejected |

**Recommendation:** owner-watch grant. Equivalent to DRM `postclose` for per-client objects.

### D-G8 — Service registry entry

| Option | Assessment |
|---|---|
| Reuse `GPU_DRIVER = 12` | Wrong: that role also redirects kernel `GpuFlush`/`GpuCursor` (E18) |
| **New `service::XE_GPU = 21`, registered by init (`SpawnCap`)** after the driver reports ready | No new kernel exception; generation-bound lookup via `LookupServiceBound` |
| New id with a `PcieDriverCap` self-registration exception (like 12) | Adds a kernel exception; not needed |

**Recommendation:** 21, init-registered. Doc text: "Intel GPU driver Cell serving the Linux `xe` uAPI over IPC." It is a registration, not an API.

### D-G9 — Allowlist

**Recommendation:** 437 and 439 take **no bit** (dispatch gate: own memory / lease party; allowlist full, E2, precedents 431-433). 438 **shares bit 48** with `GrantDma` (same "PCIe DMA authorization" authority; `PcieDriverCap` and BDF ownership checked at dispatch).

### D-G10 — Device-read-only exports

**Recommendation:** support them. `EXPORT_DEVICE_WRITE` clear → SLPT leaves `R` only (kernel-internal change to `VTD_RW`, E8). Model weights are read-only to the GPU; the driver's `xe` `READONLY` bind flag is then backed by the remapper. Alternative: v1 RW only (simpler, weaker).

### D-G11 — Graphics DRHD must translate (plan amendment, no ABI)

Breadth phase 12 says the iGPU unit "can be left with translation off" (`.agents/261103-2218-x86-platform-breadth/phase-12-vtd-multi-unit-rmrr-ir.md:29`). That contradicts ADR-0033 §4 and phase G2 "refuse to start if its graphics DRHD is not translating". **Recommendation:** amend phase 12: when a GPU driver Cell is admitted, its unit translates, with RMRR (stolen memory) identity-mapped; under `Required`, `MapDmaExport`/`GrantDma` for that BDF refuse with `NotSupported` if the unit is not translating.

### D-G12 — `MAP_USERPTR`

Under B the same export implements it: the shim exports the host range on demand and sends the unchanged `VM_BIND MAP_USERPTR` with the export attachment. **Recommendation:** no extra kernel delta; whether G3 enables it is a subset decision (XeUapiSubset). Under A it would need option-B machinery anyway.

## 5. Wire layouts (option B)

All three: class K, register envelope; the cell stub carries four arguments (`docs/abi-boundary-records.md:202-203`). Errors use the E21 sentinels through a 434/435-style encoder; new constants in `api::dma_export::ERR_*`.

### 5.1 `ExportDmaRange = 437` (client / owner)

| Reg | Meaning | Rule |
|---|---|---|
| a0 | `va` | page-aligned, in the caller's own address space |
| a1 | `len` | multiple of 4096, `1 ..= EXPORT_MAX_BYTES` (proposed 2^32) |
| a2 | `holder_service` | u16 in `DMA_EXPORT_SERVICES`; upper bits 0 |
| a3 | `flags` | bit0 `EXPORT_DEVICE_WRITE`; other bits 0 |
| ret | `export_id` | `1 ..= isize::MAX`, from a boot-wide counter, never reused within a boot |

- **Gate (dispatch):** caller live; every page of `[va, va+len)` is present, user-accessible and **owned** by the caller (SAS: inside a live caller-owned page/registered grant; domain: anonymous pages of its own root per D-G0), and writable by the caller if `EXPORT_DEVICE_WRITE`; never MMIO, framebuffer, received-grant, VFS-lease or ELF-segment pages. Holder = live bound provider of `holder_service`, holds `PcieDriverCap`, is not the caller.
- **Effect (after full validation):** lease `{id, owner (tid, cell, gen), holder (cell, gen), va, len, rights, state Live, iova None}`; the owner range is locked (no unmap / protection change / grant free while a lease is Live or Revoking). No frame list is stored: the locked owner mapping is the source of truth.
- **Errors:** `InvalidInput` (alignment, length, unknown flags/service bits); `PermissionDenied` (range not owned, holder not eligible, holder == caller); `FileNotFound` (no live provider); `NotSupported` (no translating remapper — exports never fall back to identity DMA); `OutOfMemory` (lease table: proposed 4096 global, 1024 per owner).

### 5.2 `MapDmaExport = 438` (driver / holder)

| Reg | Meaning | Rule |
|---|---|---|
| a0 | `export_id` | |
| a1 | `bdf` | ≤ 0xFFFF, owned by caller (418/424) |
| a2 | `out_ptr` | writable, checked before any effect |
| a3 | `out_len` | exactly `DMA_EXPORT_RECORD_LEN = 48` |
| ret | 0 | record written |

- **Gate:** allowlist bit 48; `PcieDriverCap`; `owner_of_bdf(bdf) == caller`; caller `(cell, gen)` == lease holder; lease Live; the BDF's remapping unit is translating.
- **Effect:** allocate a window in the caller's domain, map each owner frame (`R` or `RW`), invalidate (context + domain IOTLB as `GrantDma` does), charge the SLPT pages to the holder's DMA quota (`cell_quota::try_reserve_dma`, E3), enable bus mastering, write the record. A repeat call (same export, any BDF of the same holder domain) returns the same record without remapping.
- **Errors:** `InvalidInput`; `PermissionDenied`; `FileNotFound` (unknown, revoked, or owner dead); `NotSupported`; `OutOfMemory` (window space, tables, quota); `Unknown` when the invalidation is unacknowledged (`PublishedUnconfirmed`, E9): the lease stays mapped-unconfirmed and is cleaned only by revoke/death, exactly like `GrantDma` (`syscall.rs:7932-7942`).

**Record `ViDmaExportV1`** (`repr(C)`, little-endian, 48 bytes; producer kernel, consumer driver Cell):

| Off | Size | Field | Rule |
|---|---|---|---|
| 0 | 2 | `version` | exactly 1 |
| 2 | 2 | `flags` | bit0 `EXPORT_DEVICE_WRITE` (echo); others 0 |
| 4 | 4 | `reserved0` | 0 |
| 8 | 8 | `export_id` | echo |
| 16 | 8 | `owner_cell_id` | the exporting Cell; the driver must match it against the `CallerIdentity` of the request that carried the id (confused-deputy check) |
| 24 | 8 | `owner_generation` | as above |
| 32 | 8 | `iova` | window base, 4 KiB aligned |
| 40 | 8 | `len` | bytes, echo |

Boundary record (`docs/abi-boundary-records.md` §1): exact length (`!= 48` refused), version set {1}, decoders refuse non-zero reserved bytes and unknown flag bits, full record built before copy-out. Do not repeat gap 5 (`<` length checks).

### 5.3 `RevokeDmaExport = 439` (owner or holder)

- `a0 = export_id`, `a1..a3 = 0` → `0`.
- **Gate:** caller is the lease owner or holder (by `(cell, gen)`); anyone else gets `FileNotFound` (no existence oracle).
- **Effect:** Live → Revoking; if mapped: clear window leaves, domain IOTLB invalidation with drain; on acknowledgement release the window and quota, unlock the owner range, drop the lease. Idempotent.
- **Errors:** `InvalidInput` (non-zero a1..a3); `FileNotFound`; `TryAgain` (invalidation unacknowledged: lease stays Revoking, owner range stays locked; a deferred retry also runs each scheduler pass).

### 5.4 Constants (`libs/api`)

- `service::XE_GPU: u16 = 21`; the "next free" comment moves to 22.
- `api::dma_export`: `DMA_EXPORT_RECORD_LEN = 48`, `EXPORT_DEVICE_WRITE = 1`, `ViDmaExportV1` with `encode`/`decode`, `ERR_*`.

### 5.5 Kernel-internal (no wire change, still listed for review)

`DMA_EXPORT_SERVICES = [XE_GPU]`; `OWNER_WATCH_SERVICES += XE_GPU` (E17); IOVA window allocator and non-identity/`R`-only SLPT leaves; export holds participate in `withhold_pinned_frames` with a release key `DmaExport { holder, id }` (E11-E12); exports refused while the DMAR unit is inactive.

## 6. Failure and death semantics

| Event | Ordering | If invalidation is not acknowledged |
|---|---|---|
| Owner unexports (439) | clear leaves → IOTLB drain → unlock range → drop lease | lease Revoking, range locked, `TryAgain`; deferred retry |
| **Owner dies** | new first step in `reap_retired_task_resources` (E13), before `reap_grants_for_task` and before the private root's frames are released `[inference: root frame release follows reap; confirm in implementation]`: revoke every lease the owner holds as in 439 | frames go to quarantine under `DmaExport{holder,id}`; released by the retry, never by a timer; death is not delayed |
| Holder dies / loses `PCIE_DRIVER` | existing `teardown_dma_authority` (quiesce BDFs, revoke whole domain, E10); **after** it succeeds, every lease held by that incarnation → dropped, owner ranges unlocked | domain teardown is retried (E10); leases stay, owner ranges stay locked; if the owner dies meanwhile its frames are quarantined |
| Holder restarts | new `(cell, gen)`; old leases already dropped; clients re-export on reconnect (shim reports `VK_ERROR_DEVICE_LOST`, as after an `xe` unbind) | — |
| GPU touches a revoked window | VT-d fault; no memory reached. Driver bans/resets the context of the dead or unexporting client | — |
| Driver lies about ownership | 438 checks holder identity; record exposes owner identity for the driver's own cross-check | — |

Invariant (host- and QEMU-tested): **no frame of an export is returned to the allocator while any SLPT leaf of its window may be cached by hardware.**

## 7. Test plan

**Host (kernel unit tests beside the modules, `api` tests):**
- `ViDmaExportV1`: `size_of == 48`, field offsets, decode refuses version ≠ 1, reserved ≠ 0, unknown flags, wrong length.
- Lease state machine with a fake remapper whose acknowledgement can fail: export → map → revoke; owner death with ack (frames free) and without ack (frames withheld under `DmaExport` key, released by retry); holder death (leases dropped only after domain teardown ack); stale holder generation refused; repeat 438 idempotent; ids never reused.
- Gates: every refusal happens before any lease, pin, window or SLPT change (`InvalidInput`, `PermissionDenied`, `FileNotFound`, `NotSupported`, `OutOfMemory` cases); caller-as-holder; service not in table; grant-received pages refused.
- `iommu_pt`: non-identity mapping, `R`-only leaf encoding, windows disjoint from identity ranges, exhaustion → `OutOfMemory`, window reuse only after ack.
- G1 fake `xe` backend (phase G1 success criteria): client death revokes GPU mappings before frame release; cancelled exec signals its fence with an error.

**QEMU 8.2.2 q35 (TCG, plus local KVM), `-device intel-iommu` before `-device edu,dma_mask=0x7fffffffff`** (39-bit, so window IOVAs are not clamped, U8). Test-hooks image with two test Cells: `edu-holder` (Tier-1 driver: `FindPcieDeviceByVendor(0x1234, 0x11e8)`, MSI through 435, registered under a test-only service id in `DMA_EXPORT_SERVICES`) and `export-client` (SAS first; Tier-2 domain after D-G0).

| Lane | Steps | Pass |
|---|---|---|
| L1 happy path | client exports 2 pages (pattern), sends id; holder 438, edu DMA window→device buffer→window+4096 (≤ 4096 B per transfer, device offsets inside 0x40000..+4096), MSI, `IpcReply`; client `WaitCompletion(IPC)` | bytes copied; log `[export] map … iova=0x40…` |
| L2 unexport then DMA | client 439; holder DMA to old IOVA | QEMU VT-d translation-fault report, client memory unchanged |
| L3 owner death mid-lease | `ForceExit` client; kernel log shows revoke+ack before frame release; holder DMA to stale IOVA; a new Cell receiving the frames checks its pattern | fault, no corruption |
| L4 holder death | kill holder | edu BME off (existing log), leases dropped, client range unlocked and reusable |
| L5 refusals | non-holder 438, wrong `out_len`, unknown flags, no `intel-iommu` on `x86_64-pc` (`Required`) | exact sentinels, no `[export] map` line |
| L6 unacknowledged invalidation | test-hooks fault injection in the invalidation path | frames quarantined, retried, then released |

Ceiling: QEMU/TCG and local KVM. QEMU's VT-d completes invalidations synchronously, so drain timing is not exercised `[inference]`. No GPU evidence: GPU first light and the "client killed mid-submit" capture are phase G2 (S2, physical).

## 8. Files a later implementation would touch

- **ABI (Law 1, CP2 after implementation):** `libs/api/src/abi/syscall.rs` (437-439, allowlist arms, `service::XE_GPU`), new `libs/api/src/abi/dma_export.rs`, `libs/api/src/abi.rs` (module, next to `pcie_function` at `:35`), `libs/ostd/src/syscall.rs` (`sys_export_dma_range`, `sys_map_dma_export`, `sys_revoke_dma_export`), `libs/ostd/src/dma.rs` (holder helper, optional).
- **Kernel:** `kernel/src/task/syscall.rs` (decode, dispatch gates, encoder); new `kernel/src/task/dma_export.rs` (lease table, state machine, tests); `kernel/src/memory/pin.rs` (export release key in `withhold_pinned_frames`); `kernel/src/task/drivers/iommu.rs` (window map/unmap API); `kernel/src/task/drivers/iommu_x86.rs` — or its phase-12 split `iommu_x86/{domain,invalidate}.rs` (window allocator, non-identity map); `kernel/src/task/drivers/iommu_pt.rs` (`map(iova, phys, rights)`, `R` leaf); `kernel/src/task.rs` (death ordering in `reap_retired_task_resources`, holder lease drop after `teardown_dma_authority`); `kernel/src/task/scheduler.rs` (`OWNER_WATCH_SERVICES`); `kernel/src/memory/address_space.rs` and the D-G0 mapping code (range lock, frame withholding) `[inference on exact files]`; `kernel/src/memory/cell_quota.rs` (SLPT charge).
- **Tests / CI:** new `tests/integration/tests/dma-export-x86.rs`; `tests/integration/src/lib.rs` (`boot_x86_q35` variant with `intel-iommu` + `edu`); two test Cells (paths per the test-cell convention at implementation time); `scripts/x86/make-iso-ci.sh` (test image only).
- **Docs:** `docs/abi-boundary-records.md` (new §6-style rows for 437-439 and `ViDmaExportV1`), `CHANGELOG.md`, breadth phase 12 text (D-G11), G1/G2/G3 phase files.

## 9. Lock relations and scheduling

- **WIP:** x86 breadth slice S1 holds the Intel lane (`.agents/plan-portfolio.md:183`) and edits `kernel/src/task/{syscall,task}.rs`, `iommu{,_x86}.rs`, `libs/api/src/abi/syscall.rs` under its own handoff (`cp1-pcie-irq-power-abi.md` §7). This package needs **its own** file-bounded exception, effective only after this CP1 is approved and the owner gives the GPU slice the lane; implementation lands after breadth 04a, 04b and 12 (phase-12 module split changes the `iommu_x86` paths).
- **CELL-CONTRACT-0024 controlling areas** (`.agents/cell-contract-0024-locks.md:95-105`): P05 (DMA/device authority, revocation) owns the export lease; P06 (lifecycle) the death ordering; P02 (domain roots) the owner range lock; P04 completion is reused unchanged.
- **Rows to rule compatible or narrow at handoff** (`cell-contract-0024-locks.md:171,186,197,199,185,195,198`): `260624-0630-kernel-boundary-cleanup` (MMIO/DMA/IRQ/registry authority), `260819-1416-port-common-drivers-g1-g2-g3` (shared device/DMA authority), `260927-0739-kernel-architecture-repair` (grant/domain slices; `pin.rs`, grant reap), `261004-1957-x86-pc-lane` (device authority), `260809-0922-sas-lbi-revocable-vfs-access` and `260827-1015-sas-caller-owned-range-ledger` (leases/owner-scoped ranges; the export key sits beside the VFS lease key), `260927-1100-c2c-anywhere-tier-aware` (completion).
- **Frozen surface:** D-G6 changes nothing checked by `scripts/check-multi-source-wait-law1-digests.sh` (no change to `WaitCompletion`, `completion::source`, `async_ipc`); the 437-439 enum additions in `libs/api/src/abi/syscall.rs` must leave that script green.
- **Not authorized by this draft:** any source edit, number reservation beyond this proposal (numbers are provisional until CP2; recheck for collisions then), hardware purchase, D-G0 work.

## 10. Decisions for the owner

| ID | Question | Recommendation |
|---|---|---|
| D-G0 | Tier-2 x86 large anonymous memory as a separate CP1 (prerequisite) | Yes, option (i) |
| D-G1 | Buffer ownership: client-owned exports (B) vs driver-owned lent BOs (A) | B |
| D-G2 | Shared holder VT-d domain + per-client PPGTT (no per-client VT-d domain) | Yes |
| D-G3 | Kernel-allocated IOVA window per export, region `[2^38, 2^39)` | Yes |
| D-G4 | Holder named by service id in kernel table `DMA_EXPORT_SERVICES`; provider needs `PcieDriverCap` | Yes |
| D-G5 | Kernel-only revoke with drain; quarantine on unacknowledged invalidation; never wait on the driver | Yes |
| D-G6 | Completions reuse `IpcSubmit`/`IpcReply` + `WaitCompletion(IPC)`; no new source | Yes |
| D-G7 | `XE_GPU` added to `OWNER_WATCH_SERVICES` | Yes |
| D-G8 | `service::XE_GPU = 21`, init-registered | Yes |
| D-G9 | 437/439 no allowlist bit; 438 shares bit 48 | Yes |
| D-G10 | Device-read-only exports (`R`-only SLPT leaves) | Yes |
| D-G11 | Amend breadth phase 12: graphics DRHD translates when a GPU driver is admitted | Yes |
| D-G12 | `MAP_USERPTR` via the same export, enabled or not by the G1 subset | No extra kernel delta |
| D-G13 | Wire: 437/438/439 as §5, `ViDmaExportV1` 48 bytes, sentinels per E21 | Yes |

## 11. Not checked / open questions

- Where a private root's frames are released relative to `reap_retired_task_resources` on x86 (owner-death ordering hook point); depends on D-G0's mapping code.
- Whether a sibling thread can `IpcReply` for a token its service thread received (driver IRQ-thread pattern, D-G6).
- Intel graphics VT-d behaviour on translation faults (engine hang vs. aborted transaction) and whether Xe3 needs any gfx-specific invalidation quirk; only the MIT `xe` files of U6 were checked, Linux `drivers/iommu/intel/*` (GPL) was not read.
- anv behaviour when `MAP_USERPTR` is refused: XeUapiSubset reports device loss (`anv_kmd_backend.c:341-347`); not read here.
- Per-export SLPT overhead (8 bytes per 4 KiB page, about 8 MiB of tables for 4 GiB) and whether the window allocator should prefer 2 MiB leaves when frames are contiguous.
- Proposed limits (`EXPORT_MAX_BYTES = 2^32`, 4096 leases global, 1024 per owner) are placeholders to size from G4 workloads.
