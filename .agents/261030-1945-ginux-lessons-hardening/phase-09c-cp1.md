# Phase 09C CP1 — device-backed replacement: verdict-gated restart for every supervised service, per-device block routes (Law-1: syscalls 416 and 432, `ViBlockRouteV1`), cache-cold readback

Status: **CP1 APPROVED by the user 2026-11-01 (v3).** Exact Shape 2 delta §4.4 approved ("Duyệt": 416, 432, 40-byte `ViBlockRouteV1`, `BLOCK_DRIVER` retired, new ostd API, no shim). §10: Q1 names by PCI location `/mnt/disk/pci-BB-DD-F` / `mmio-<base>` (40-byte record kept); Q2–Q6 defaults approved (system disk = unique Cellos-MBR disk else lowest key; secondary disks FAT32 only; `/mnt/disk/` has `/mnt/sd` policy; no hot-add; 8 routes); Q7 **add** a driver-side check that the request sender is the VFS (behaviour change, part of 09C). **CP2 APPROVED by the user 2026-11-01 ("Duyệt CP2 + commit") on the §11 evidence and its listed deviations; re-checked: kernel host 345/345, api 169/169, mutable-globals 0 problems.**

## Decisions so far (relayed by Main)

**On v1:**
- **D1 approved:** Option B.
- **D2 approved:** verdict policy (§4.1), 200-tick bound.
- **D3 approved:** VFS evicts a cached sector whose write did not complete (§4.2).
- **D5 approved:** test-only seams (§4.3).

**On v2:**
- **D4:** fold the two-disk fix into 09C.
- **D6:** gate every init-supervised service.

**v3:** CP1 v2 approved with **Shape 2** for D4: per-device block routes, both disks
usable.
- **Approved as written:** R3, R6 (D6 details), R7.
- **Adapted to Shape 2:**
  - **R4 is moot.** Per-device routes have no "route bound to another device"
    outcome, so the `RouteBound` sentinel and the idle exit are dropped (§4.4.2).
  - **R5 becomes a per-device rule** (§4.4.8).

Admission: 09C admitted 2026-11-01 (`admission-approval.md:82`); that line requires this
separate CP1 before code. Dependencies met: 08 done (08A 6ee14c3cb, 08B f54615c85,
08C f0edc9338; `phase-08-device-quiescence.md:4`), 09B done (9d9e1e10a; CP2 approved,
`phase-09b-cp1.md:3`). Evidence ceiling: x86 q35, exact QEMU device model
(`phase-01-admission.md:38`).

## 0. Verdict

08C already restarts a dead AHCI/NVMe driver and reads a file back
(`tests/integration/tests/ahci-x86.rs:583-649`, `nvme-x86.rs:215-284`). It does not yet
meet 09C, for five reasons found in source:

1. **Init restarts without the 09B K1 verdict.** It respawns at once on the death
   notice, which arrives before quiescence. This applies to every supervised service,
   not only drivers.
2. **A BDF that retirement still holds looks like "no controller".** The replacement
   then exits 0, and init treats that as a clean exit.
3. **The readback can come from VFS memory.** The 08C readback can be served from the
   VFS page cache and may never use the new lease.
4. **Death with a command in flight is never exercised.**
5. **There is one last-wins block route.** With two disks only one is usable, and a
   restart can move the mounted volume's I/O to the other disk (§1.6, §1.8).

**Design: B, with D4 = Shape 2 and D6.**
- Init consults `QueryRetirement` (431) before every restart (§4.1).
- VFS evicts a cached sector whose write did not complete (§4.2).
- Each storage device gets its own kernel route, bound to the device it owns.
  - A driver (re)binds only its own device's route (416).
  - VFS enumerates routes (new 432), mounts the system disk as today and every other
    FAT32 disk under `/mnt/disk/<name>`.
  - VFS addresses I/O per device (§4.4).
- Witnesses: cache-cold record, death after issue, 1/2 vCPU, AHCI and NVMe, sibling
  owner, VT-d NVMe, both disks mounted with independent kill/restart, and the generic
  gate (§7).

**Law-1 public deltas (§4.4):**
- 416 gets new arguments and checks, typed errors, and per-device route publication.
- New syscall 432 `LookupBlockRoute` and its record `ViBlockRouteV1` (40 bytes).
- Service `BLOCK_DRIVER` is retired:
  - 416 no longer publishes it;
  - `RegisterService(BLOCK_DRIVER)` is refused;
  - `LookupService(BLOCK_DRIVER)` always returns 0.
- The ostd wrappers are replaced or added (clean cutover).

**Internal semantic changes:**
- init restart gate;
- init per-driver block wait and route readiness;
- VFS cache eviction;
- VFS device discovery, choice of the primary disk, secondary mounts, the per-device
  router and the per-device sticky rule.

Durability is **not** claimed (§6).

## 1. Facts (observed in source)

### 1.1 How a restarted driver gets its BDF and DMA lease today
- **Authority comes from the launch edge, not from lineage.** `/bin/ahci` and
  `/bin/nvme` are PcieDriverCap launch targets
  (`kernel/src/loader/launch_profile/profiles.rs:25-27`; manifest note
  `cells/drivers/ahci/src/main.rs:229-231`). Each init spawn is a fresh task:
  `sys_spawn_from_elf` / `sys_spawn_from_path_raw`
  (`cells/tools/init/src/service_table.rs:56-79`).
- **The lease is the BDF owner table, keyed by bare tid with no generation.**
  - `BDF_OWNERS: BTreeMap<bdf, tid>` (`kernel/src/resource_registry.rs:780-781`).
  - `claim_bdf_owner` inserts only when the BDF is unowned
    (`resource_registry.rs:787-799`).
  - The claim happens inside 418/424 `FindPcieDevice[ByVendor]`, which needs
    PcieDriverCap (`kernel/src/task/syscall.rs:8057-8089`, `offer_pcie_device`
    `:2278-2331`).
  - Every DMA grant re-checks `owner_of_bdf(bdf) == caller`
    (`syscall.rs:780-782`) and needs a live, non-retiring caller
    (`syscall.rs:802-807`).
  - Tids are never reissued (`kernel/src/task/task_id_selftest.rs:93,156`). So a new
    tid in `BDF_OWNERS` is a **fresh grant with no lineage**. It is the 08C "fresh
    lease" (`phase-08c-cp1.md:26-27`). No lease generation is exported to user space.
- **The BDF is released only after quiesce and fence.** `teardown_dma_authority`
  runs in this order (`kernel/src/task.rs:1870-1911`), and a failure at any step
  returns `false`:
  1. quiesces each owned function (BME off, INTx disable, MSI/MSI-X off);
  2. `revoke_dma_for_cell`;
  3. `release_bdfs_for`;
  4. `release_acked_frames`.

  On `false` the work is queued and retried on each scheduler pass
  (`task.rs:1913-1945`, `:1959-1962`).
- **Without VT-d, "fenced" means only that BME was cleared.**
  `unmap_cell_domain` returns `true` when the tid has no VT-d domain
  (`kernel/src/task/drivers/iommu_x86.rs:830-833`, via `iommu.rs:222-235`). On a
  q35 lane without `intel-iommu`, frames are therefore released after the
  BME-clear readback alone. That is the open 08B residual
  (`phase-08b-cp1.md:84-87`).
- **Death of the old owner before the claim is not distinguished from absence.**
  - `offer_pcie_device` returns `Ok(0)` when another tid owns the BDF
    (`syscall.rs:2314-2318`), and `Ok(0)` also when no device matches
    (`:8067`, `:8086`).
  - The AHCI replacement tries 200 yields, then logs `no AHCI controller present;
    driver cell idle` and exits **0** (`cells/drivers/ahci/src/main.rs:77-99`).
    NVMe does the same (`cells/drivers/nvme/src/main.rs:63-73`).
- **The device reset is done by the replacement, not by the kernel.**
  - AHCI: `GHC.HR`, then logs `HBA reset complete`
    (`cells/drivers/ahci/src/controller.rs:209-222`).
  - NVMe: `CC.EN=0`, waits for `CSTS.RDY=0`, then logs `controller reset complete`
    (`cells/drivers/nvme/src/controller.rs:138-157`).
  - Both reset before `sys_register_block_driver`
    (`ahci/src/main.rs:128-168`, `nvme/src/main.rs:126-134`).
  - Neither driver uses `WaitIrq` or SHM (grep of `cells/drivers/{ahci,nvme}` finds no
    match). The IRQ waiter path (`task.rs:1963`) is therefore not exercised by these
    drivers.

### 1.2 Whether init consults the retirement verdict
- **It does not.** The only `sys_query_retirement` callers are in
  `cells/demos/recovery-demo` (`src/bin/authority/lifecycle.rs:148`,
  `authority/main.rs:47`). There are none in `cells/tools/init`.
- **Init's restart path:**
  - On a death message it applies the policy: `OnFailure` restarts only when
    `reason != 0` (`cells/tools/init/src/supervisor.rs:44-52`).
  - It charges the budget of 5 restarts per 1000 ticks (`:8-9,69-85`) and sets
    `restart_pending`.
  - `restart_pending` then spawns immediately at the top of the loop (`:14,92-121`).
  - Block drivers are `required: false`, `OnFailure`, `SelfReady`, with a 1000-tick
    ready timeout. They are watched with `NotifyOnExit`
    (`cells/tools/init/src/boot.rs:437-458`); replacements are watched again at
    `service_table.rs:87`.
  - Init's loop blocks in `Message::receive(0, true)`
    (`cells/tools/init/src/activation.rs:71-72`).
- **Init could query the verdict with no new authority.**
  - The verdict record opens at retirement start with the root's current death
    subscribers as readers (`kernel/src/task/scheduler.rs:1397-1402`). That happens
    before `exit_task` drains them (`scheduler.rs:1702`).
  - Init subscribed at spawn, so it is a recorded watcher. The cap is 8 watchers and
    the table holds 64 records (`phase-09b-cp1.md:419-420`).
  - 431 has no allowlist bit and is gated at dispatch
    (`kernel/src/task/syscall.rs:4391-4395,6181-6184`). The recovery authority calls
    it without declaring it (`authority/main.rs:30-33`).
- **The death notice comes before quiescence.** `NotifyOnExit` timing is unchanged by
  09B (`phase-09b-cp1.md:263-264`). The verdict closes only after every member is
  reaped (`task.rs:1444-1502`). So for a closed non-`DMA_DEFERRED` verdict, the BDF has
  already been released (`task.rs:1454` → `:1904`, before `:1502`).
- **Code drift.** `boot.rs:431-436` says a retained BDF makes "the replacement
  exit, and the crash budget gives up". In code, the replacement exits **0** (§1.1),
  and `OnFailure` does not restart a 0 exit at all (`supervisor.rs:44-51`).

### 1.3 VFS and the block router across driver death
- **The route is keyed by tid.**
  - `DRIVER_TID` caches the tid (`cells/services/vfs/src/blk_router.rs:26-45`).
  - A reply counts only while `tid` is still the registered `BLOCK_DRIVER`
    (`:67-74`); otherwise `forget_driver` runs (`:54-56`).
  - A send refused by the kernel is `NotIssued`. A missing reply after the send is
    `Indeterminate` (`:155-170`).
- **Indeterminate writes are not replayed.** `write_raw_sector` retries only
  `NotIssued` (`cells/services/vfs/src/block_stream.rs:147-156`). The router logs
  `write lba=… indeterminate; not replayed` (`blk_router.rs:130-135`).
- **While no driver is registered**, I/O falls to the kernel path
  (`blk_router.rs:105-107,125-129`). On QEMU that path is the null device
  (`kernel/src/task/drivers/block.rs:38-46`):
  - reads fail;
  - writes report `Indeterminate`, which fails closed.
- **Finding: the cache is not coherent after a write that did not complete.**
  - `PageCache::write_sector` returns before touching the cache when the device write
    is not `Completed` (`cells/services/vfs/src/page_cache.rs:77-83`). A resident
    entry keeps the **pre-write** bytes while the medium may hold the new or a torn
    sector.
  - Later reads are served from that entry (`page_cache.rs:56-63`).
  - Partial-sector read-modify-write builds on it too (`block_stream.rs:217-226`).
  - So "reconcile against the committed record" can read VFS memory instead of the
    medium.
- **There is no VFS write journal.** 08C left that out of scope
  (`phase-08c-cp1.md:49-50`).

### 1.4 What "committed record readback" means today
- **Committed** means every sector write returned `Completed` (0x00, 08A wire,
  `phase-08a-cp1.md:38-46`) and the flush completed
  (`block_stream.rs:235-241`). For FAT this is write-through, device first
  (`page_cache.rs:32-37`).
- **The 08C witness:** `vwrite keep.txt F08C-KEEP`, then death, then
  `vcat keep.txt` (`ahci-x86.rs:608-625`, `nvme-x86.rs:242-259`). The sectors of
  `keep.txt` are inserted into the page cache on write (`page_cache.rs:81`). So the
  readback **may never reach the device through the new lease** [INFERENCE: not
  observed; no per-read driver marker exists]. The same holds for `after.txt`
  (`:626-627`).
- **A two-boot readback already exists.** `ahci_fat32_persistence_reboot_x86` writes
  in one QEMU instance and re-reads in a fresh one (`ahci-x86.rs:298-390`).
- **Durability:** no CellosFS durable-state owner is admitted
  (`phase-09-post-fault-replacement.md:33,54`; `phase-01-admission.md:17,40-43`). A
  `Completed` reply on QEMU is not proof of power-cut durability.

### 1.5 Whether the retirement verdict accounts for driver DMA, IRQ and BDF state
- **What the tally holds.** Fields at `kernel/src/cell/retirement_verdict.rs:27-39`:
  - `dma_deferred` counts members whose `teardown_dma_authority` failed
    (`task.rs:1455,1959`). That covers both quiesce failure and IOMMU
    non-acknowledgement.
  - `DMA_DEFERRED` classifies as **Quarantined** (`retirement_verdict.rs:92-101`).
- **The record is final at close.** The deferred retry (`task.rs:1926-1945`) never
  updates the verdict: a later successful release leaves `Quarantined/DMA_DEFERRED`
  in place. No query reports when the BDF finally becomes claimable.
- **The record cannot name these:** the BDF identity, BDF release, IRQ waiter release
  (unconditional, `task.rs:1963`), whether an IOMMU fence existed, or the device reset
  state.
  - Extending it is not additive. V1 decoders refuse unknown reason bits
    (`KNOWN = 0x1F`) and non-zero reserved fields
    (`libs/api/src/abi/retirement.rs:33-45,173-181`).
  - The rules of `docs/abi-boundary-records.md:28-36` would require a new version (V2)
    under Law-1.
- **Doc drift.** `docs/abi-boundary-records.md:69` still says "CP2 pending,
  uncommitted" for `ViRetirementV1`.

### 1.6 Sibling device ownership
- `RegisterBlockDriver` (416) needs PcieDriverCap. It binds the caller's live
  `(cell, generation)` (`syscall.rs:2163-2201`).
- The service registry is **last-wins**: `register` "replacing any prior entry"
  (`kernel/src/cell/service_registry.rs:84-114`).
- With NVMe and SATA both present, the winner depends on scheduling. This is a
  recorded plan risk; every other lane attaches exactly one storage device
  (`ahci-x86.rs:392-404`).
- [INFERENCE, not observed] On such a machine, a restarted AHCI driver re-registers
  and displaces a live NVMe route:
  1. The pending NVMe reply then fails the `lookup == tid` check
     (`blk_router.rs:67-71`) and is reported `Indeterminate`, although it completed.
  2. The next request goes to the other disk at the same LBA.

  This hazard predates 09C (Phase 02a/08C) and makes the dual-storage lane unsafe to
  qualify.

### 1.7 Fault seams and test lanes
- **Both death hooks fire before the issue point.**
  - AHCI `F08B-DIE` exits in `before_write` (`cells/drivers/ahci/src/fault_inject.rs:44-47`).
    The issue point is the `PxCI` write (`controller.rs:485-511`).
  - NVMe exits before `write_sector` (`cells/drivers/nvme/src/dispatch.rs:96-106`). Its
    issue point is the SQ tail doorbell (`controller.rs:427-430`).
- **The fault ISO** builds both drivers with `fault-inject` and carries init, vfs,
  input, cellbox, platform, serial, ahci and nvme only. There is **no e1000**
  (`scripts/x86/make-ahci-fault-iso.sh:51-55,77-85`).
- **x86 QEMU helpers pass no `-smp`, so they run on 1 vCPU.** Examples:
  - `boot_x86_bios_with_sata` (`tests/integration/src/lib.rs:1027-1053`);
  - the NIC/NVMe lane used by the 08C NVMe witness (`nvme-x86.rs:229`, `lib.rs:1270`).

  Only RV64/aarch64 helpers take a CPU count (`lib.rs:1887-1907`, `:652`).
- **x86 SMP support:** the kernel brings up at most one AP, logging
  `[x86-smp] topology online=2` (`kernel/src/task/smp.rs:810-876`).
- **A VT-d lane exists for NVMe:** `boot_x86_bios_with_vtd(iso, nvme_disk)`
  (`lib.rs:1482-1484`). There is no VT-d helper for SATA.
- **Pattern to reuse:** the recovery authority waits for a non-`Pending` verdict and
  treats `Unproven` or no verdict as node-recovery-required, with no restart
  (`cells/demos/recovery-demo/src/bin/authority/lifecycle.rs:77-97,146-158`).

### 1.8 Census for D4: block-route writers, readers and device identity
- **Every 416 caller.** Grep of `cells`, `libs`, `kernel/src` and `tests`:
  - `cells/drivers/ahci/src/main.rs:43,157` (declared at `:265`);
  - `cells/drivers/nvme/src/main.rs:36,127`;
  - `cells/drivers/virtio-blk/src/main.rs:32,51`.
- **SDK:**
  - `ostd::syscall::sys_register_block_driver` passes four zero arguments and maps any
    non-zero return to `PermissionDenied` (`libs/ostd/src/syscall.rs:2513-2526`);
  - the re-export `ostd::dma::register_block_driver` (`libs/ostd/src/dma.rs:82-85`)
    has no users.
- **ABI text:** "416 … ABI: (no args) → 0" (`libs/api/src/abi/syscall.rs:456-459`).
  The allowlist shares bit 50 (`:495-499,1131-1134`).
- **Kernel path.**
  - Decode is at `kernel/src/task/syscall.rs:9673`, the enum at `:4029-4030`, the arm
    at `:8024-8032`.
  - The arm calls `register_driver_service` (`:2163-2208`). That function checks the
    driver cap and liveness, binds `(cell, generation)` (`:2188-2199`), and returns
    `InvalidInput` (the generic error) on rollback.
  - The role static is `driver_cell::BLOCK_DRIVER_CELL`
    (`kernel/src/task/drivers/driver_cell.rs:20,103-108`), cleared on exit
    (`:125-129,213`).
  - Inventory entry: `scripts/mutable-globals.toml:799-808` ("seal = none: overwritten
    by each RegisterBlockDriver") and `docs/generated/mutable-globals.md:183`.
  - The registry is last-wins (`kernel/src/cell/service_registry.rs:84-114`).
- **A second writer exists.** `RegisterService` lets any SpawnCap holder register any id
  with any live tid (`syscall.rs:6266-6288`), BLOCK_DRIVER included. No caller does so
  today:
  - block drivers are `SelfReady` (`cells/tools/init/src/boot.rs:448`);
  - boot config refuses BLOCK_DRIVER for configured cells
    (`libs/boot-config/src/lib.rs:240-244`; tests `tests.rs:142-144`).
- **Route readers:**
  - the VFS router: one cached tid serves every backend (FAT, littlefs, CellosFS)
    (`cells/services/vfs/src/blk_router.rs:1-13,27,32-45,67-74`);
  - the init block wait (`boot.rs:480`).
- **Tests and docs that assert last-wins:**
  - `ahci_and_nvme_both_register_x86` asserts at least 2 registrations
    (`tests/integration/tests/ahci-x86.rs:392-443`);
  - the helper doc at `tests/integration/src/lib.rs:1192-1200`.

  The NVMe marker test (`nvme-x86.rs:7-11`) only needs one registration.
- **Device identity the kernel can already check.**
  - **PCI:** `BDF_OWNERS` maps BDF to tid (`kernel/src/resource_registry.rs:780-819`).
    The BDF is encoded `bus<<8 | dev<<3 | fn` (`syscall.rs:2283`) and returned to the
    driver in `PcieDeviceInfo` bytes 0..4 (`:2321`).
  - **virtio-blk (MMIO, RV64/aarch64):** the driver claims its slot with
    `request_region(base, MMIO_SLOT_SIZE)`
    (`cells/drivers/virtio-blk/src/device.rs:183-213`). The MMIO owner is a CellId keyed
    by the exact base (`resource_registry.rs:583-605`) and is released at root
    retirement (`kernel/src/task.rs:1493`).
- **Error channel.**
  - Typed sentinels `usize::MAX-1..-4` exist only for `has_typed_errors` syscalls
    (`syscall.rs:1687-1705`); every other error is the generic `usize::MAX`.
  - ostd's `SyscallError` has no `InvalidInput` (`libs/ostd/src/syscall.rs:31-40`).
    The 431 precedent maps `MAX-2` → `FileNotFound` and `MAX-3` → `Unknown` (`:1662-1669`).
- **Kernel fallback after a driver death.** With no route, the VFS uses `sys_blk_*`
  (`blk_router.rs:105-107,125-129,141-147`). That reaches kernel MMC on real boards,
  otherwise the null device (`kernel/src/task/drivers/block.rs:38-46`).
  [INFERENCE] On a board with MMC plus a block Driver Cell, a dead driver would send
  the mounted volume's I/O to the MMC. No such configuration exists today: rpi3 skips
  `/bin/block` (`boot.rs:464`).
- **No lease generation exists or is exported.** The lease identity is the
  never-reissued tid (`kernel/src/task/task_id_selftest.rs:93`) and the registry's
  `(cell, generation)`.

### 1.9 Census for D6: init-supervised services, restart users and tests
- **Restartable services (policy ≠ `never`):**
  - bootstrap VFS, `Always` (`cells/tools/init/src/service_table.rs:21-35`);
  - `config`, `ai`, `input`, `compositor`, `kms`, `net-broker`, `supervisor`: `Always`;
  - `net` and `silo`: `Always`, `required`; `silo` is also self-ready;

    (all in `config/services.toml:38-105`)
  - `hypervisor`: `Always` (`config/autoload.toml:16-20`);
  - `cellbox`: `on-failure` (`autoload.toml:41-45`);
  - `ahci` and `nvme`: `OnFailure` (`boot.rs:437-458`).
- **`never` services:**
  - `fb-console`, `desktop`, `hotswap-demo`, `recovery-authority`
    (`autoload.toml:4-14,25-39`);
  - `virtio-net`, `e1000`, `igb`, `serial`, `dwc2-usb`, `xhci`, `virtio-gpu`,
    `bcm-display` (`services.toml:8-36,64-72`).
- **Device state among the restartable ones:**
  - Only `ahci` and `nvme` claim a BDF. All other PCI/USB/GPU drivers are `never`.
  - `hypervisor` owns VM state, reaped by `reap_vms_for_task` (`task.rs:1965`). The
    verdict does not count it.
  - The rest own no device, so their verdict can only carry grant, pin, VFS-lease,
    SHM or leak reasons. SHM has no Cell consumer (`phase-09b-cp1.md:99`).
  - [INFERENCE] `Contained` is therefore the common case. 09B observed 10/10 for a
    worker (`phase-09b-cp1.md:479`).
- **Init watches every spawn it makes:** `service_table.rs:87` and `boot.rs:454`. The
  verdict captures the root's watchers when it opens (`scheduler.rs:1397-1402`).
- **Restart consumers and tests:**
  - `init_gives_up_after_a_crash_storm` (`tests/integration/tests/boot.rs:2419-2462`)
    runs `cells/tests/bench/src/scenarios/init_giveup.rs` against `/bin/config` with
    `RESTART_WAIT_TICKS = 300` (`:33-40`);
  - shell restart marker check `scripts/phase10_canonical_drive.py:563`;
  - `tests/integration/src/bench_milestones.rs:102-104` parses
    `Init: service restarted.`;
  - the 08C witnesses (`ahci-x86.rs:615`, `nvme-x86.rs:249`).
- **Unaffected by the gate:**
  - `hotswap-smoke` and `provider-recovery` use `never` providers
    (`autoload.toml:25-39`);
  - the recovery authority restarts its own worker behind its own K1 gate
    (`lifecycle.rs:77-107`);
  - demand-activated engines are never restarted (`activation.rs:128`).
- **Wait mechanics.**
  - Init's main receive blocks (`activation.rs:71-72`).
  - A bounded poll that yields between attempts already exists
    (`wait_for_exact_registration`, `service_table.rs:117-129`).
  - A `RecvTimeout`-based sleep would consume other services' death notices.

### 1.10 Census for Shape 2: VFS mount model and route consumers
- **VFS mounts one disk's fixed partitions** (`VfsManager::new`,
  `cells/services/vfs/src/manager.rs:66-100`; layout `libs/api/src/abi/disk.rs:11-56`):
  - `/mnt/sd`: FAT32 P1 at LBA 2048;
  - `/data`: CellosFS P4;
  - `/srv`: CellosFS P5;
  - `/bin`: overlay over the FAT cell store P6.

  Every backend uses the global router:
  - `block_stream.rs:15`;
  - `disk_cellosfs.rs:6`;
  - the FAT sync flush (`backend_fat.rs:418`);
  - the bin overlay, through `FatBackend::mount("/bin", …)` (`backend_bin_overlay.rs:31`).
- **Per-volume caches.** Each FAT mount owns its own `CachedBlockStream` and
  `PageCache` (`block_stream.rs:166-177`).
- **Mount prefixes are `&'static str`** (`mount.rs:13-46`; `backend_fat.rs:32,61`;
  `backend_cellosfs.rs:24,28`).
- **Access policy** is a static prefix table (`access/rules.rs:66-116`). `/mnt/sd/` is
  read-all and write-all (`:84-88`). Guest-disk special cases name `/mnt/sd` only
  (`access/guest_disk.rs:11,63`).
- **The MBR at LBA 0 is the runtime source of truth** for the Cellos layout, with P2
  type `0x7F` (`disk.rs:6-17`).
  - The x86 test fixtures are FAT32 at LBA 2048 written by `mkfat32_inplace.py`
    (`nvme-x86.rs:114,166-194`; `ahci-x86.rs:144-172`).
  - [INFERENCE] They carry no Cellos MBR: a grep of the script for MBR or `0x7F` finds
    nothing.
- **Ordering.** The VFS is spawned after init's block wait (`main.rs:84-87` →
  `boot.rs:462-503`). Init discards the `/bin/block` tid (`boot.rs:465`). `SelfReady`
  with no service id counts as "not ready" (`service_table.rs:93-97`).
- **Capabilities.**
  - `block_io` is declared by the VFS (`vfs/src/main.rs:57`), and also by chrome and
    vm-runner (`cells/apps/chrome/src/main.rs:27`, `cells/apps/vm-runner/src/main.rs:24`).
  - It is granted by the boot ceiling (`kernel/src/loader/boot_ceiling.rs:248`,
    `kernel/src/task/cap.rs:717`) and gates the raw block syscalls
    (`syscall.rs:8266`).
- **Exposure today.** `LookupService` is open to every cell (`syscall.rs:6290-6293`), so
  the `BLOCK_DRIVER` tid is visible to all. Drivers reply to any sender
  (`ahci/src/main.rs:194-203`). This is a pre-existing exposure.
- **Next free syscall number: 432.** `QueryRetirement = 431` is the highest number in
  the 4xx range (`libs/api/src/abi/syscall.rs:630,1364`).

## 2. Problem

09C must:
- replace a driver through the proven controller-specific retirement and reset
  strategy;
- give the replacement a **fresh authorized lease**;
- **read the committed record** through it;
- report device unavailability as a real result, not as success
  (`phase-09-post-fault-replacement.md:40-42,63`);
- never turn a fault outside admitted isolation into a successful restart (`:37`).

Today:
- No lifecycle owner looks at the verdict before republishing a driver (§1.2).
- "Device retained" is reported as "no device" and ends silently (§1.1).
- The readback may never touch the device (§1.4), and the VFS cache can disagree with
  the medium after an indeterminate write (§1.3).
- No witness kills a driver with a command issued (§1.7).
- Every x86 witness is uniprocessor (§1.7).
- With two storage devices, the last-wins route can move the mounted volume's I/O to
  the other disk after a restart. A SpawnCap holder can also overwrite the route
  (§1.6, §1.8).
- Every other supervised service also restarts without consulting the verdict (§1.9).

## 3. Options

### A. Witness only, using existing mechanisms (no production change)
- **Tests only:**
  - a cache-cold record;
  - a death-after-issue fault marker;
  - 2-vCPU helpers;
  - the VT-d NVMe variant;
  - e1000 in the fault ISO.
- **Pros:** zero semantic change; smallest diff.
- **Cons:**
  - The K1 verdict for a driver stays unread: only watchers or supervisors may query
    it (`retirement_verdict.rs:176-194`), and no test Cell is one.
  - The restart still races retirement.
  - An `Unproven` retirement can still be followed by a published driver.
  - "Device retained" is still reported as a clean "no controller" exit.
  - A readback of an indeterminate sector can still return cached pre-write bytes.
- Fails 09C's "device unavailability is a real result" and the plan's
  node-recovery rule (`phase-09-post-fault-replacement.md:37,42`).
- **Rejected.**

### B. Init gates block-driver restart on the K1 verdict, VFS stays cache-coherent, full witness set (recommended)
- **B1, init (private policy):**
  - A supervised block driver is restarted only after
    `sys_query_retirement(dead_tid)` returns a final verdict that the policy table
    (§4.1) admits.
  - Because the verdict closes after the BDF release (§1.2), a `Contained` verdict
    guarantees the claim succeeds. That closes the race and the "retained = absent"
    misreport without touching 418/424.
  - Every other outcome is logged as an explicit result (`device unavailable` or
    `node recovery required`) and is never retried silently.
- **B2, VFS (internal):** a write that did not return `Completed` evicts that sector
  from the page cache. The next read then goes to the medium through whichever lease
  is current, and the value read back is the medium's truth, not VFS memory. There is
  no replay, no journal and no wire change.
- **B3, witnesses:** §7 (cache-cold record, death after issue, 1/2 vCPU, AHCI and
  NVMe, sibling owner, VT-d NVMe, verdict-gate negatives).
- **Law-1:** no public delta. B1 and B2 are internal semantic changes, approved here
  (§4.2).
- **Pros:**
  - Reuses the approved K1 and the 09B authority pattern.
  - As in v1, B itself needs no kernel change. The D4 fold adds the 416 kernel delta
    (§4.4).
  - Deterministic claim ordering.
  - Honest unavailable results.
  - The readback provably reaches the device.
- **Cons:**
  - `DMA_DEFERRED` leaves the device unavailable for the rest of the boot, because
    the verdict is never updated (§1.5).
  - Init blocks for a bounded verdict wait.
  - v1 left dual-storage machines unqualified. D4 now folds in the 416 device
    binding (§4.4).

### C. Kernel lease generation, route binding and verdict V2 (Law-1, non-additive in part)
- **Lease field:** export a lease generation in the 48-byte `PcieDeviceInfo` (bytes
  44..48 are padding today, `syscall.rs:2270-2272,2320-2329`).
- **416 change:** bind `RegisterBlockDriver` to an owned BDF and refuse displacing a
  live provider of a different BDF.
- **VFS:** the route checks device identity across a restart.
- **Verdict V2:** add BDF, IOMMU-fenced and reset-pending fields, plus an update when
  a deferred teardown later succeeds.
- **Pros:**
  - Closes the dual-storage hazard.
  - Closes the `DMA_DEFERRED` availability gap.
  - Gives a verdict that describes device state.
- **Cons:**
  - Changes 416 semantics (last-wins is the observed contract asserted by
    `ahci_and_nvme_both_register_x86`, `ahci-x86.rs:392-443`).
  - Changes the meaning of the padding bytes in a class-K record.
  - A new `ViRetirementV2` and decoder.
  - A kernel lease table.
  - Larger than 09C's literal scope ("use the proven controller-specific
    retirement/reset strategy", `phase-09-post-fault-replacement.md:42`).
- **Not adopted as a whole.** D4 is resolved by Shape 2 (§4.4): per-device routes,
  with no lease generation and no verdict V2. Those stay out of 09C.

## 4. Recommended design (Option B): exact deltas

### 4.1 B1: init restart gate for every supervised service (D6; init-private; no Law-1 byte)
- **Scope.** Every death that `supervisor::run` handles where the policy says restart
  and the budget allows (`supervisor.rs:44-85`). That covers:
  - configured services;
  - the bootstrap VFS;
  - `hypervisor`;
  - `cellbox`;
  - the block drivers.

  The gate is universal, so `Service` needs no flag.
- **Not gated:**
  - `restart = "never"`;
  - a clean exit under `OnFailure`, which is not restarted anyway;
  - demand-activated engines (§1.9).
- **Order on a death:**
  1. **Policy, unchanged.** `never`, or a clean exit under `OnFailure`, ends here with
     no query.
  2. **Crash budget, unchanged.** It is charged once per observed death, before the
     gate (`supervisor.rs:81-85`). When exhausted, init prints the existing
     `restart storm — giving up` and makes no query.
  3. **Bounded verdict wait.**
     - Poll `sys_query_retirement(dead)` while it reports `Pending`.
     - Call `yield_now` between polls.
     - Stop at `VERDICT_WAIT_TICKS = 200` scheduler ticks (`now_ticks`,
       `service_table.rs:113-115`).
     - Same pattern as `wait_for_exact_registration`; no `RecvTimeout` (§1.9).
  4. **Decide** with a pure, host-tested `restart_decision(result) -> Decision`.
  5. **Restart** goes through the existing `restart_pending` path. The existing lines
     `Init: service died — restarting...` and `Init: service restarted.` stay
     byte-identical; a verdict line is added before them.
- **Policy table** (approved D2, identical for every service):

  | Verdict result | Decision | Log (one line, consumer-visible) |
  |---|---|---|
  | `Contained` | restart (`restart_pending = true`) | `Init: <name> retirement Contained (reasons=… dma=… qpages=…) — restarting` |
  | `Quarantined`, no `DMA_DEFERRED` (tracked pins, VFS leases) | restart; the quarantine stays explicit and charged (09B E8 rule) | `… Quarantined reasons=… qpages=… — restarting` |
  | `Quarantined` with `DMA_DEFERRED` (only a BDF owner can produce it) | **no restart this boot**: the BDF is retained and the record will not change | `Init: <name> device unavailable: DMA teardown unacknowledged (BDF retained)` |
  | `Unproven`, `FileNotFound`, `PermissionDenied`, or `Pending` after the bound | **no restart** | `Init: <name> node recovery required: containment unproven, no restart` |

  `WRITABLE_BORROW` is logged in `reasons` but never changes the decision (09B §4.1).
- **Non-driver services.**
  - `Contained` is expected (§1.9) and gives today's behaviour plus one log line.
  - A no-restart decision on a `required` service (`net`, `silo`, the bootstrap VFS)
    also prints the existing `Init: required service unavailable — recovery failed.`
    (`supervisor.rs:117-119`). The node stays up, degraded. No automatic node reboot
    or recovery action is added.
- **Interaction with the crash budget.**
  - A no-restart outcome keeps the count already charged and never re-arms
    `restart_pending`. Only a new watched death can (`supervisor.rs:101-103`).
  - Budget numbers are unchanged (5 per 1000 ticks).
  - The storm test still sees the give-up: the budget check comes before the gate.
- **Interaction with `restart = "never"`.** Unchanged: init does not query, because
  the verdict has no consumer there. So `hotswap-demo`, `recovery-authority`,
  `hotswap-smoke` and `provider-recovery` are unaffected (§1.9).
- **Restart latency.**
  - Each restart waits for the verdict to close, which happens after quiescence.
    [INFERENCE] That is about one scheduler pass on an idle node; the hard bound is
    200 ticks (2 s).
  - Init handles no other message during the wait, so N simultaneous deaths serialize
    to at most N × 200 ticks.
  - Readiness budgets (`ready_timeout_ticks`, 1000 for block drivers) start after the
    spawn and are unaffected.
  - `init_giveup`'s `RESTART_WAIT_TICKS = 300` keeps 100 ticks of margin over the
    bound (§8).
- **Unchanged:**
  - policies;
  - budget numbers;
  - `SelfReady` registration wait;
  - `NotifyOnExit` subscription;
  - every existing init log string;
  - syscalls 418, 424 and 431.
- **Cleanup:** fix the `boot.rs:431-436` comment to the real behaviour, and make the
  driver comments say that exit 0 means "nothing to claim".

### 4.2 B2: VFS cache coherence after a write that did not complete (internal semantic)
- `PageCache::write_sector`: when `write_raw_sector` returns `false`, remove `sector`
  from `entries`/`lru_order` (adjusting `total_bytes`), then return `false`.
- Observable change: after an `Indeterminate` write, a later read of that sector
  returns the medium's bytes (old, new or torn) instead of the pre-write cache image.
- The write still fails to its caller (EIO path unchanged). It is **never replayed**,
  and the retry policy of 08A is untouched (`block_stream.rs:147-156`).
- The block wire, the FAT/CellosFS adapters and the NotIssued retry are unchanged.
  `blk_router` changes only for the D4 sticky route (§4.4).

### 4.3 Witness-only seams (test images only; `fault-inject` features already exist and are not in production profiles)
- **AHCI `F09C-DIE-ISSUED`:** exit (70) right after the `PxCI` write
  (`controller.rs:511`), before polling completion. Needs a new `Inject` variant
  applied in `issue`.
- **NVMe `F09C-DIE-ISSUED`:** exit (70) right after the SQ tail doorbell (`submit_io`,
  `controller.rs:430-445`).
- **Replay detector:** a later write carrying the same marker logs
  `REPLAY of indeterminate write`, following the existing `F08A-IND` pattern
  (`fault_inject.rs:48-55`).
- **Optional `F09C-DIE-SHM` (AHCI only):** `ShmAlloc` + `ShmMap`, then exit. This
  produces a runtime `Unproven(SHM_UNTRACKED)` verdict for the negative gate.
  [INFERENCE] The driver launch profile may refuse SHM. If so, this row falls back to
  the host test of `restart_decision` and is reported **runtime-unqualified**.
- **`scripts/x86/make-ahci-fault-iso.sh`:** add `driver-e1000` as `/bin/e1000`, giving
  a sibling PCI BDF owner on the NVMe+NIC lane.

### 4.4 D4: per-device block routes (Shape 2, chosen by the user; Law-1)

**Goal.** Every storage device has its own route, bound to that device for the boot.
Both disks are usable. A driver can (re)bind only the route of the device it owns. So a
restarted driver regains exactly its own route and never touches a sibling's, and a
sibling's I/O continues while it is down.

#### 4.4.1 Kernel route table
- **Storage.** New `driver_cell::BLOCK_ROUTES`: an IRQ-masking spinlock over a fixed
  array of `MAX_BLOCK_ROUTES = 8` slots.
  - No allocation: the exit path may run in ISR context
    (`scripts/mutable-globals.toml:807`).
  - Slot = `{ kind: u16, id: u64, provider: Option<(tid, cell_id, generation)> }`.
- **Slots are append-only.**
  - A device's slot index is fixed at its first bind and never reused within the boot.
  - Its key is never cleared; only the provider is.
- **Lock order:** SCHEDULER → ROLE_PUBLICATION → BLOCK_ROUTES (leaf).
- **On exit**, `deregister_all_for(tid)` clears the provider of the slot naming exactly
  `tid` and keeps the key. This replaces `deregister_block_driver`
  (`driver_cell.rs:125-129,213`).
- **Removed:** `BLOCK_DRIVER_CELL` (`driver_cell.rs:20,103-108`; "no non-test reader",
  `scripts/mutable-globals.toml:803`).
- **Moved:** the `block-ready` bench milestone (`driver_cell.rs:106-107`) moves to the
  route publish.

#### 4.4.2 Syscall 416 `RegisterBlockDriver` (changed)

| Reg | Before | After |
|---|---|---|
| a0 | ignored (wrapper passes 0) | `kind`: 1 = PCI function, 2 = platform MMIO window; other values refused |
| a1 | ignored | `id`. Kind 1: kernel BDF encoding (bus·256 + dev·8 + fn, ≤ 0xFFFF; the value of `PcieDeviceInfo` bytes 0..4). Kind 2: exact physical base of an MMIO region |
| a2, a3 | ignored | reserved, must be 0 |
| return | 0 or the generic error | 0 or a typed sentinel |

**Validation before effect:**
1. **Decode.**
   - `kind ∈ {1, 2}`.
   - `a2 = a3 = 0`.
   - Kind 1: `id ≤ 0xFFFF`. Kind 2: `id ≠ 0`.

   Otherwise → **Malformed**.
2. **Caller.** Live, not `Retiring`/`Terminated`, and holding PcieDriverCap or
   UsbDriverCap (`syscall.rs:2169-2182`). Otherwise → **Denied**.
3. **Ownership.**
   - Kind 1: `owner_of_bdf(id) == Some(caller_tid)`.
   - Kind 2: `lookup_mmio_owner(id) == Some(caller_cell)`.

   Otherwise → **NotOwner**. This is the lease check: a BDF is released only after
   quiesce and fence (§1.1), and tids are never reissued.
4. **Route, under ROLE_PUBLICATION and BLOCK_ROUTES:**
   - **A slot with this key exists:**
     - provider empty or equal to the caller → set the provider to
       `(caller tid, cell, generation)` and publish. This is how a restarted driver
       regains its own route, and a repeat call is idempotent.
     - live provider ≠ caller → **Denied**, with a kernel warning. This is
       unreachable while ownership is exclusive; the check is defensive.
   - **No slot:** append into a free slot. If the table is full → **Denied**, logged
     `[driver_cell] block route table full`.
   - A driver cannot write another device's slot: the slot is chosen by the key, and
     step 3 proves the caller owns the key.
   - No service-registry write at all: `service::BLOCK_DRIVER` is not published.
   - Log: `[driver_cell] block driver registered: tid=T device=pci-00:1f.2 route=R`.
     The existing prefix is kept for the test markers (`nvme-x86.rs:7-11`).

**Errors** (416 joins `has_typed_errors`, `syscall.rs:1700-1705`; encoded by
`encode_ipc_result`, `:1687-1696`):

| Wire return | Kernel result | Meaning | ostd `BlockRegisterError` |
|---|---|---|---|
| 0 | `Ok(0)` | route published | `Ok(())` |
| `usize::MAX - 2` | `FileNotFound` | **NotOwner**: the caller does not own the named device | `NotOwner` |
| `usize::MAX - 3` | `InvalidInput` | **Malformed**: unknown kind, reserved ≠ 0, or id out of range | `Malformed` |
| `usize::MAX` | `PermissionDenied` and every other error | **Denied**: no driver cap, caller retiring, table full, or the defensive live-provider case | `Denied` |

v2's `RouteBound` (`MAX-5`) is dropped: no new sentinel and no new kernel error
variant.

#### 4.4.3 Syscall 432 `LookupBlockRoute` (new)
- **Number:** `ViSyscall::LookupBlockRoute = 432`.
- **Arguments:** `a0 = index` (0..=7), `a1 = out_ptr`, `a2 = out_len`, `a3 = 0`. Returns
  40, the record length, on success.
- **Gating:** no allowlist bit, because the u64 allowlist is full
  (`phase-09b-cp1.md:130-131`). It is gated at dispatch in the same group as 431
  (`syscall.rs:4391-4395`).
- **Who may call:**
  - holders of **BlockIoCap** (the VFS; the same gate as the raw block syscalls,
    `syscall.rs:8266`);
  - holders of **SpawnCap** (init, for its per-driver wait and restart readiness).
  - Everyone else → `PermissionDenied`.

  This narrows today's exposure, where `LookupService(BLOCK_DRIVER)` is open to all
  cells (§1.10).
- **Typed errors**, through `has_typed_errors` / `encode_ipc_result`:

  | Return | Meaning |
  |---|---|
  | `MAX-2` (`FileNotFound`) | no route at that index: end of enumeration, or index ≥ 8 |
  | `MAX-3` (`InvalidInput`) | `out_len ≠ 40`, `a3 ≠ 0`, or a bad out pointer |
  | `MAX` (`PermissionDenied`) | the caller holds neither cap |
- **No side effects.** The record is a snapshot of one slot, copied under
  BLOCK_ROUTES.

#### 4.4.4 `ViBlockRouteV1` (new class K record, `libs/api/src/abi/block_route.rs`)
Little-endian, `#[repr(C)]`, exactly 40 bytes, fields in declared order:

| Offset | Size | Field | Type | Rule |
|---|---|---|---|---|
| 0 | 2 | `version` | u16 | `1`; accepted set {1}; other → `UnsupportedVersion` |
| 2 | 2 | `kind` | u16 | `1` PCI, `2` MMIO; other values refused |
| 4 | 4 | `flags` | u32 | bit 0 `PROVIDER_LIVE`; bits 1–31 reserved, written 0, refused if set |
| 8 | 8 | `id` | u64 | kind 1: ≤ 0xFFFF (BDF encoding); kind 2: ≠ 0 (MMIO base) |
| 16 | 8 | `provider_tid` | u64 | ≠ 0 when `PROVIDER_LIVE`; 0 otherwise |
| 24 | 8 | `cell_id` | u64 | provider Cell; 0 when not live |
| 32 | 8 | `generation` | u64 | provider generation; 0 when not live |

- **Constants:**

  | Name | Value |
  |---|---|
  | `BLOCK_ROUTE_RECORD_LEN` | 40 |
  | `BLOCK_ROUTE_VERSION` | 1 |
  | `BLOCK_DEVICE_PCI` | 1 |
  | `BLOCK_DEVICE_MMIO` | 2 |
  | `FLAG_PROVIDER_LIVE` | 1 |
  | `FLAGS_KNOWN` | 1 |
  | `MAX_BLOCK_ROUTES` | 8 |
- **Decoder (rules of `docs/abi-boundary-records.md` §1).** Refuse each of the
  following:
  - length ≠ 40;
  - version outside {1};
  - kind outside {1, 2};
  - unknown flag bits;
  - an id out of range for its kind;
  - inconsistent liveness: `PROVIDER_LIVE` set with `provider_tid = 0`, or clear with
    any of `provider_tid`, `cell_id`, `generation` ≠ 0.

  The full record is decoded before any use.
- **Ownership:** producer is the kernel; consumers are the VFS and init.
- **Not in the record:** the route index is a syscall argument. Slot order is bind
  order: stable within a boot, not across boots.

#### 4.4.5 Service `BLOCK_DRIVER` retired (R3)
- 416 no longer publishes it.
- `RegisterService(BLOCK_DRIVER, …)` → `PermissionDenied` for every caller, SpawnCap
  holders included. So `LookupService(BLOCK_DRIVER)` always returns 0.
- The constant stays reserved: boot config still refuses it
  (`libs/boot-config/src/lib.rs:240-244`).
- The kernel owner-watch test list drops it (`syscall.rs:11096-11119`).

#### 4.4.6 SDK (ostd, clean cutover)
- **New `sys_register_block_device(device: BlockDevice) -> Result<(), BlockRegisterError>`.**
  - `BlockDevice` = `Pci { bdf: u16 }` or `Mmio { base: u64 }`.
  - `BlockRegisterError` = `NotOwner`, `Malformed` or `Denied`.
- **New `sys_lookup_block_route(index: u32) -> Result<Option<ViBlockRouteV1>, SyscallError>`:**

  | Kernel return | Result |
  |---|---|
  | 40 and a valid decode | `Ok(Some(record))` |
  | `MAX-2` | `Ok(None)` |
  | `MAX-3`, or a decode failure | `Err(Unknown)` |
  | anything else | `Err(PermissionDenied)` |

  This follows the 431 mapping (`libs/ostd/src/syscall.rs:1662-1669`).
- **Removed:**
  - `sys_register_block_driver` (`libs/ostd/src/syscall.rs:2513-2526`);
  - `ostd::dma::register_block_driver` (`libs/ostd/src/dma.rs:82-85`; no users).

#### 4.4.7 Every consumer to migrate (census §1.8, §1.10)

| Site | Change |
|---|---|
| `cells/drivers/ahci/src/main.rs:43,157` | `sys_register_block_device(Pci { bdf: info.bdf as u16 })`; any error → `exit(1)` |
| `cells/drivers/nvme/src/main.rs:36,127` | `Pci { bdf }`; any error → `exit(1)` |
| `cells/drivers/virtio-blk/src/main.rs:32,51`, `device.rs:183-213` | `Mmio { base }`; `BlkDevice` keeps its claimed slot base |
| `cells/tools/init/src/boot.rs:437-458` | block drivers get `service_id: None` and an init-private readiness "a route whose `provider_tid == tid`", read via 432 (a `Service` field, not `CellSpec`) |
| `cells/tools/init/src/boot.rs:462-503` (`:465`) | keep the `/bin/block` tid; wait until every spawned block driver has a live route or has terminated, bounded by `BLOCK_DRIVER_WAIT_MS` (60 s, `:401`); `block_wait_step` host tests updated |
| `cells/tools/init/src/service_table.rs:88-98` | readiness branch for the private route readiness |
| `cells/services/vfs/src/blk_router.rs` (whole file) | per-device router (§4.4.8) |
| `cells/services/vfs/src/block_stream.rs:15,44-100,122-177,235-241` | `BlockStream` and `CachedBlockStream` carry a `DeviceRef` |
| `cells/services/vfs/src/disk_cellosfs.rs:6-52` | `CellosPartitionDisk` carries a `DeviceRef` |
| `cells/services/vfs/src/backend_fat.rs:35-75,417-419` | `mount(prefix, dev, base)`; `sync` flushes its own device |
| `cells/services/vfs/src/backend_cellosfs.rs:24-32` | `DeviceRef`, owned prefix |
| `cells/services/vfs/src/backend_bin_overlay.rs:31` | `DeviceRef` |
| `cells/services/vfs/src/manager.rs:66-100` | discovery, choice of primary, secondary mounts |
| `cells/services/vfs/src/mount.rs:13-46` and backend `prefix` fields | owned prefix (`Box<str>`) for names found at runtime |
| `cells/services/vfs/src/access/rules.rs:66-116` | static prefix rule `/mnt/disk/`, read-all and write-all like `/mnt/sd/` |
| `kernel/src/task/syscall.rs` | 416 enum, decode and arm (`:4029-4030`, `:9673`, `:8024-8032`); 432 enum, decode and arm; `has_typed_errors` (`:1700-1705`); `RegisterService` refusal (`:6197-6289`); test list (`:11096-11119`); host tests |
| `kernel/src/task/drivers/driver_cell.rs:20,103-129,213,220-261` | route table, exit clearing, tests |
| `kernel/src/task/drivers.rs:65-69` | comment |
| `libs/api/src/abi/syscall.rs:456-459` and the `from_usize` table (`:1349-1364`) | 416 doc; 432 variant |
| `libs/api/src/abi.rs` + new `libs/api/src/abi/block_route.rs` (+ tests) | record and constants |
| `libs/ostd/src/syscall.rs:2513-2526`, `libs/ostd/src/dma.rs:82-85` | §4.4.6 |
| `scripts/mutable-globals.toml:799-808`, `docs/generated/mutable-globals.md:183` | remove `BLOCK_DRIVER_CELL`, add `BLOCK_ROUTES` |
| `tests/integration/tests/ahci-x86.rs:392-443`, `tests/integration/src/lib.rs:1192-1200` | migrate the dual-register test and its doc |
| `docs/abi-boundary-records.md` | 416 row, new 432 + `ViBlockRouteV1` row, `BLOCK_DRIVER` retired, 431 status drift (`:69`) |

#### 4.4.8 VFS: discovery, mounts, addressing, outcomes
- **Discovery**, when the VFS starts (`VfsManager::new`).
  - Enumerate 432 from index 0 until `Ok(None)` (at most 8).
  - Each route becomes a device entry `{index, kind, id, name}`.
  - Name: kind 1 → `pci-BB-DD-F` (hex, e.g. `pci-00-1f-2`); kind 2 → `mmio-<base hex>`
    (e.g. `mmio-10001000`).
- **Primary (system) device.**
  - Read LBA 0 (the MBR) of each device through its route.
  - The primary is the **one** device whose MBR has a partition entry of type `0x7F`,
    the Cellos P2 marker (`disk.rs:11-17`).
  - If none has it, or more than one does, the lowest `(kind, id)` is primary, and the
    reason is logged.
  - With zero routes, the primary is the kernel block path (today's fallback for MMC
    boards) and there are no secondaries.
- **Primary mounts are exactly today's set:** `/mnt/sd` (P1), `/data` (P4), `/srv` (P5)
  and the `/bin` overlay (P6) (`manager.rs:66-100`). Paths are unchanged, so single-disk
  lanes and the 08C witnesses are unaffected.
- **Secondary mounts.**
  - FAT32 at `PART_FAT32_BASE_LBA`, probed as today (`backend_fat.rs:35-75`), mounted at
    `/mnt/disk/<name>`.
  - A secondary without FAT32 is not mounted; it logs
    `[vfs] /mnt/disk/<name>: no FAT32 volume`.
  - Secondaries get no CellosFS and no `/bin`.
  - One static prefix rule `/mnt/disk/` carries the `/mnt/sd/` policy. Guest-disk
    special cases stay `/mnt/sd` only.
- **Late devices.** A route that appears after discovery is not mounted until the next
  boot. Init's per-driver wait (§4.4.7) makes this rare (Q5).
- **Addressing.**
  - `DeviceRef` is either a device-table index or `Kernel`.
  - `blk_read`, `blk_write`, `blk_flush`, `blk_read_sectors` and `blk_write_sectors`
    take a `DeviceRef`.
  - Each device caches its own provider tid. Resolve: `432(index)` with
    `PROVIDER_LIVE` set and a matching key gives the tid.
  - A reply counts only if a fresh `432(index)` still names that tid after the receive.
    This replaces `lookup_service == tid` (`blk_router.rs:67-74`).
- **Outcomes per device.** The per-request semantics are unchanged:
  - a send the kernel refuses → `NotIssued`;
  - a missing reply after the send → `Indeterminate`;
  - a reply byte is decoded per 08A.

  A dead provider affects only requests addressed to its own device.
- **Sticky rule per device (R5, adapted).** A device found through 432 never uses the
  kernel `sys_blk_*` path. While its route has no live provider, reads fail at once and
  writes or flushes return `NotIssued`, since nothing was sent. The kernel path serves
  only the `Kernel` pseudo-device, used when discovery found no routes.
- **Per-device scope.**
  - Flush is per device: `FatBackend::sync`, `CachedBlockStream::flush` and
    `CellosPartitionDisk::flush` flush only their own device.
  - The page cache is already per FAT volume, so B2 eviction is per volume.
- **Restart regains only its own route.** The same key maps to the same slot index, so
  the VFS device entry re-resolves to the new tid. Every other device entry is
  untouched.

#### 4.4.9 Not needed
- No lease generation. Identity is the never-reissued tid plus the record's
  `(cell_id, generation)`.
- `PcieDeviceInfo` and `ViRetirementV1` are unchanged.

#### 4.4.10 Shape 1 (not chosen)
One route, bound to the first device for the boot. A second disk stays unused (v2 text,
superseded).

#### 4.4.11 D4 tests
- **Kernel host:**
  - 416 decode and typed errors.
  - Route matrix:
    - a first bind appends a slot;
    - the same device under a new owner after release keeps its slot with a new
      provider;
    - a second device gets a second slot;
    - `NotOwner` when another tid owns the BDF, or another cell owns the MMIO base;
    - each `Malformed` row;
    - a full table → `Denied`;
    - exit clears the provider and keeps the key;
    - the defensive live-provider path;
    - every refusal leaves the table untouched.
  - 432: gate (BlockIoCap, SpawnCap, neither), typed errors, record bytes.
  - `RegisterService(BLOCK_DRIVER)` refused for a SpawnCap holder, and
    `LookupService(BLOCK_DRIVER) == 0` after a 416.
- **api:**
  - `size_of::<ViBlockRouteV1>() == 40` and every field offset;
  - encode/decode round trip;
  - each decoder refusal: length, version, kind, flags, id range, liveness.
- **ostd:** sentinel → result mapping.
- **VFS host:**
  - name derivation;
  - choice of primary: exactly one `0x7F` MBR, none, two;
  - per-device router: a provider death on one device leaves the other device's
    requests untouched;
  - sticky rule: no kernel call;
  - flush per device;
  - mount set for the primary and for a secondary.
- **init host:** multi-driver block wait; route readiness.
- **Integration:** E9 (§7). `ahci_and_nvme_both_register_x86` migrates to
  `ahci_and_nvme_both_mounted_x86`.

### 4.5 Explicit non-goals
- No lease generation, no `PcieDeviceInfo` change, no verdict V2.
- No hot-add of block devices after VFS start.
- No CellosFS, `/data`, `/srv` or `/bin` on secondary disks.
- No sender check in drivers (pre-existing exposure, §1.10, Q7).
- No VFS write journal, no automatic reconciliation write, no replay.
- No virtio-blk restart: it only gets the 416 migration.
- No automatic node reboot or recovery action for a refused required service.
- No physical hardware, no KVM claim.
- No durable, reboot or power-cut claim (§6).

## 5. File set (record in `admission-approval.md` before editing)

- **ABI / SDK (Law-1):**
  - `libs/api/src/abi/syscall.rs` (416 doc, 432 variant);
  - `libs/api/src/abi.rs`;
  - new `libs/api/src/abi/block_route.rs` and its tests;
  - `libs/api/src/abi/syscall_tests.rs`;
  - `libs/ostd/src/syscall.rs`;
  - `libs/ostd/src/dma.rs`.
- **Kernel:**
  - `kernel/src/task/syscall.rs` (416, 432, `RegisterService` refusal, typed errors,
    host tests);
  - `kernel/src/task/drivers/driver_cell.rs` (route table, exit clearing, tests);
  - `kernel/src/task/drivers.rs` (comment);
  - `scripts/mutable-globals.toml`, `docs/generated/mutable-globals.md`.
- **Drivers:**
  - `cells/drivers/{ahci,nvme}/src/main.rs`;
  - `cells/drivers/virtio-blk/src/{main.rs,device.rs}`;
  - fault-inject only: `cells/drivers/ahci/src/{fault_inject.rs,controller.rs}` and
    `cells/drivers/nvme/src/{dispatch.rs,controller.rs}`.
- **Init:**
  - `cells/tools/init/src/supervisor.rs` (universal gate, bounded wait,
    `restart_decision`);
  - `cells/tools/init/src/service_table.rs` (bounded-poll helper, route readiness);
  - `cells/tools/init/src/boot.rs` (per-driver block wait, `/bin/block` tid, comment
    fix);
  - host tests.
- **VFS:**
  - `cells/services/vfs/src/{blk_router,block_stream,page_cache,disk_cellosfs,backend_fat,backend_cellosfs,backend_bin_overlay,manager,mount}.rs`;
  - `cells/services/vfs/src/access/rules.rs`;
  - host tests in those modules.
- **Image:** `scripts/x86/make-ahci-fault-iso.sh` (e1000).
- **Tests:**
  - `tests/integration/src/lib.rs`: `-smp` variants of the SATA, NIC, VT-d and
    SATA+NVMe helpers, and the doc at `:1192-1200`;
  - `tests/integration/tests/{ahci-x86,nvme-x86}.rs`: 09C witnesses, E9, and the
    migrated dual test;
  - `tests/integration/tests/boot.rs`: verdict-line assertion;
  - `cells/tests/bench/src/scenarios/init_giveup.rs` and
    `tests/integration/src/bench_milestones.rs`: only if observed runs need it.
- **Docs:**
  - `docs/abi-boundary-records.md` (416, 432/`ViBlockRouteV1`, `BLOCK_DRIVER` retired,
    431 drift);
  - `CHANGELOG.md`;
  - this plan directory (`phase-09-post-fault-replacement.md` status and deviation;
    §11 CP2 of this file).

**Size estimate** [INFERENCE, from the census, not measured]: about 40–45 files and
about 2.5–3.5k changed lines including tests.

| Area | Lines (approx.) |
|---|---|
| kernel | 400–500 |
| api + ostd | 300–350 |
| drivers | about 60, plus fault seams about 80 |
| init | 300–400 |
| VFS | 700–1000; the largest part is threading `DeviceRef` through every backend plus discovery and mounting |
| integration tests and helpers | 500–700 |
| docs and inventory | 150 |

Suggested internal order, one commit per green step and still one CP2:
1. api/ostd and the kernel (416, 432);
2. the drivers;
3. init;
4. VFS;
5. witnesses.

## 6. RAM, medium and durable

| Evidence | Shows | Does not show |
|---|---|---|
| Same-boot cache-cold read after replacement | Bytes on the QEMU medium, read through the new lease | Survival of power loss |
| Second QEMU boot on the same image (clean exit) | Bytes reached the image file | Torn-write or power-cut behaviour; QEMU write cache is flushed on clean exit |
| Indeterminate sector read back after replacement | The medium value (old, new or torn) without replay | Which value hardware would produce |

No durable claim. A reboot or power-cut profile needs the admitted CellosFS
durable-state owner and its named failure class
(`phase-09-post-fault-replacement.md:33,54`, `phase-01-admission.md:40-43`). That
profile stays **unqualified**.

## 7. Evidence plan (x86 q35 QEMU/TCG; fault ISO; `-smp 1` and `-smp 2`)

Results are observed by consumers: shell `vcat`/`vwrite` output, init's verdict line,
kernel ownership/registration logs, and a second boot's reads. No result rests only on
diagnostic markers. A lane that cannot boot (e.g. no AP online, VT-d lane failure) is
reported **unqualified**, never as a pass.

| # | 09C criterion | Evidence (both AHCI on `boot_x86_bios_with_sata[_smp]` and NVMe on `boot_x86_bios_with_nic[_smp]`, unless noted) |
|---|---|---|
| E1 | fresh authorized lease | `F08B-DIE`. Expected sequence: `[pcie] quiesced <bdf> for tid T0`; init `retirement Contained … — restarting`; `Init: service restarted.`; the reset line (`HBA reset complete` / `controller reset complete`); `[iommu] Cell T1 granted DMA BDF=<bdf>` (`syscall.rs:7216-7217`); `[driver_cell] block driver registered: tid=T1 device=<bdf> route=R`, where R is the **same route index** as before the death; VFS logs the re-resolved provider `(cell C1, gen G1)` read from 432. Assert `T1 ≠ T0`, `(C1, G1) ≠ (C0, G0)`, and that the verdict line precedes T1's grant. No `not quiescent`, no `restart storm`. |
| E2 | committed record read through the new lease | Two boots. **Boot 1:** `vwrite R0`, then a clean shutdown. **Boot 2:** never read R0; `F08B-DIE`; replacement (E1); `vcat R0` = R0. R0's data sectors are absent from the fresh VFS page cache, so the read is served by T1. Then `vwrite R1`, `vcat R1`. **Boot 3** (fresh QEMU, same image): `vcat R0`, `vcat R1` = expected, i.e. medium readback, not durability (§6). |
| E3 | uncertain write across death not replayed; reconciliation reads the medium | `vwrite D F09C-DIE-ISSUED`: driver exits after `PxCI` / doorbell. VFS logs `driver died before replying; outcome indeterminate` and `… not replayed`; the shell write fails. After the replacement, a `vcat D` result is recorded as observed, whether old, new or error; no value is required. The replacement logs no `REPLAY` for the marker. Boot 3 reads the same medium value as Boot 2 did after B2 (proves VFS did not serve a cached image). Verdict `Contained` or `Quarantined`; never a restart on `Unproven`. |
| E4 | device unavailable = real result | (a) Host tests: `restart_decision` for all six rows of §4.1, plus kernel `teardown` retention (`resource_registry_tests.rs:91-98`). (b) Runtime `F09C-DIE-SHM` if admitted (§4.3): init `node recovery required`, no `Init: service restarted.`; `vcat R0` fails (error, not data). (c) Runtime `DMA_DEFERRED` is not reachable on q35 without VT-d (`phase-08c-cp1.md:42-45`), so it is reported as host-only. (d) Crash budget: 6 rapid `F08B-DIE` → `restart storm — giving up`, and later I/O fails. |
| E5 | 1 and 2 vCPU | E1–E3 on `-smp 1` and `-smp 2`. On 2 vCPUs, record `[x86-smp] topology online=2` and the verdict's `remote_hart_mask`. A remote-hart wait is reported as observed, not forced (09B §11.5 precedent). |
| E6 | independent owner survives | NVMe+NIC lane with `/bin/e1000` in the fault ISO: the e1000 tid and its BDF grant are unchanged across the NVMe replacement, and no `[pcie] quiesced` line names the NIC BDF. AHCI lane: VFS, serial and shell tids unchanged. The two-storage-device case is E9. |
| E7 | IOMMU-fenced variant | NVMe on `boot_x86_bios_with_vtd[_smp]` with the fault ISO: E1+E2 with a real IOTLB invalidation. AHCI under VT-d: **unqualified** unless a SATA VT-d helper boots; it is not added by default. Without VT-d, E1–E3 are labelled "BME-clear only" (§1.1). |
| E8 | repeated cycles | 10 `F08B-DIE` cycles per controller, spaced past the 1000-tick budget window. Each cycle's verdict line shows `dma=0 qpages=0 leaked=0 shm=0`, and E2's read succeeds. Kernel heap is outside any no-leak claim (09B E12 rule). |
| E9 | both disks mounted; independent failure (D4, Shape 2) | `boot_x86_bios_with_sata_and_nvme[_smp]` with the fault ISO and two FAT32 fixtures. (a) **Discovery:** two `block driver registered … route=R` lines; VFS logs both routes, the primary and its reason (`lowest key`: the fixtures have no Cellos MBR), and mounts `/mnt/sd` on the primary and `/mnt/disk/<name>` on the secondary. (b) **Independent I/O:** `vwrite`/`vcat` a file on each mount. (c) **Kill the secondary** (`F08B-DIE` on a write to `/mnt/disk/<name>`): during the gap and after it, primary `vwrite`/`vcat` succeed, and the primary's provider tid is unchanged (kernel and VFS logs). A secondary write during the gap fails with `NotIssued` and nothing is sent. After the verdict-gated restart, the secondary's route has the **same index and key** with a new tid, and a cache-cold read of a file committed in an earlier boot (E2 pattern) succeeds through the new lease. (d) The same with the **primary** killed: secondary I/O continues. (e) **Isolation:** a final fresh boot reads each disk's files only from that disk, and a file written to one disk is absent on the other. No `block driver registered` line for a different device appears in any restart. |
| E10 | generic gate (D6) | RV64 default image (`boot.rs` lane) and x86. `bench init-giveup` gives `PASS` or `SKIP`, never `FAIL`. Every restart of `/bin/config` is preceded by `Init: … retirement Contained … — restarting`. The shell restart in `scripts/phase10_canonical_drive.py` still passes. The negative rows (Unproven, no record, Pending timeout) for non-driver services are host tests of `restart_decision` only: no non-driver fault fixture exists, so they are reported runtime-unqualified. |
| R | regressions | Existing `ahci-x86` and `nvme-x86` suites (08A/08B/08C witnesses, `ahci_fat32_persistence_reboot_x86`, the migrated `ahci_and_nvme_both_mounted_x86`); every RV64 and aarch64 virtio-blk lane boots and mounts as before (kind 2, single primary); `hotswap-smoke` 13/13, `provider-recovery` 2/2; `boot.rs` suites; init, VFS, api and kernel host tests; x86 kernel `-D warnings` build; the default x86 image carries no `fault-inject`/`F09C` strings; `LookupService(BLOCK_DRIVER)` returns 0 everywhere. |

## 8. Risks

- **Init stalls during the verdict wait, now for every service.** The wait is bounded
  at 200 ticks per death. Other deaths queue in IPC and serialize. A `Pending` record
  past the bound fails closed: no restart, an availability cost, not a safety one.
- **The verdict wait may push `init_giveup` restarts past `RESTART_WAIT_TICKS = 300`**
  if retirement closes slowly on TCG, giving `FAIL`. Change the constant only on
  observed numbers.
- **A refused required service (`net`, `silo`, VFS) leaves the node degraded**, with a
  log line only and no automatic node recovery.
- **Loss of availability on `DMA_DEFERRED`.** On a VT-d machine with a slow IOTLB
  acknowledgement, the device stays unavailable until reboot, because the verdict is
  never updated (§1.5). This is accepted as fail-closed; Option C is the fix.
- **Verdict eviction or watcher cap.** A full table or more than 8 watchers gives
  `FileNotFound`/`PermissionDenied`, which is treated as Unproven. Init is the only
  driver watcher today.
- **B2 exposes medium truth after an indeterminate write.** FAT may read a torn or new
  sector it previously never saw. That is correct (FAT has no journal), but a later
  read-modify-write can now build on it.
- **QEMU completes commands quickly.** `F09C-DIE-ISSUED` may find the DMA already
  finished, so E3 proves non-replay and medium reconciliation, **not** that a transfer
  was in flight during quiesce.
- **The 2-vCPU x86 lane is new.** SMP defects unrelated to 09C get their own report and
  are not absorbed. If the AP never runs the driver, the verdict's `remote_hart_mask`
  stays 0 and the claim is limited to "booted 2 vCPUs".
- **The VFS rework touches every disk backend.** Threading `DeviceRef` through FAT,
  CellosFS, the bin overlay and flush is a regression risk on every lane. The full boot
  regression set (R) is mandatory.
- **Choice of primary.** The fixtures carry no Cellos MBR, so the primary is the lowest
  key. On a machine with two Cellos-layout disks, the lowest key wins and a warning is
  logged; the system disk is then chosen by PCI location.
- **Mount names follow the PCI or MMIO location.** Moving a disk to another slot renames
  its `/mnt/disk/<name>`.
- **The route table caps at 8.** A ninth storage device gets `Denied` and stays
  unusable.
- **The 416 cutover is not compatible with old binaries.** An old driver calling 416
  with zero arguments gets `Malformed`, exits 1, and storms. The census found three
  in-tree callers, rebuilt and signed in the same image.
- **The MMIO binding relies on exclusive CellId ownership of the exact base.**
- **432 is visible to every BlockIoCap holder** (chrome and vm-runner declare it). This
  is still narrower than today's open `LookupService(BLOCK_DRIVER)`.
- **Drivers serve any sender.** This pre-existing exposure (§1.10) is not changed
  (Q7).
- **Init now waits for every spawned block driver.** A driver that neither registers
  nor exits holds boot for up to 60 s, the same bound NVMe already has.
- **SHM for drivers.** If the launch profile refuses SHM, E4(b) is host-only.
- **TCG only.** Nothing qualifies physical hardware or KVM
  (`phase-08-device-quiescence.md:52`).

## 9. Rollback

- Revert the 09C commit(s) and cold boot a known-good image.
- Init returns to an immediate, ungated restart. VFS returns to keeping the cache entry
  after a write that did not complete.
- Reverting restores 416 with no arguments and last-wins, removes 432 and
  `ViBlockRouteV1`, and restores the `BLOCK_DRIVER` service. Kernel, api, ostd, the three
  drivers, init and VFS must revert together, as one unit: a driver/kernel mix fails
  registration in one direction.
- Init returns to an ungated restart. The VFS returns to a single route and single-disk
  mounts.
- Files written to a secondary disk stay on that disk but are not mounted after a
  revert.
- No persistent format changes.
- Effects already issued to the device and writes already completed are not undone by a
  binary revert (`phase-09-post-fault-replacement.md:71`,
  `phase-08-device-quiescence.md:56`).

## 10. Decisions

**Recorded:**
- D1, D2, D3 and D5 approved (v1).
- D4: fold into 09C; **Shape 2** (v3).
- D6: every service (v2).
- R3, R6 and R7 approved as written.
- R4 moot under Shape 2; R5 adapted per device (§4.4.8).

**Confirm before code:**
1. **Exact Law-1 delta** (§4.4.2–4.4.6):
   - 416 arguments, validation and the three typed errors (no `RouteBound`);
   - 432 arguments, gate (BlockIoCap or SpawnCap) and typed errors;
   - the `ViBlockRouteV1` layout and decoder;
   - retiring the `BLOCK_DRIVER` service;
   - the ostd API.
2. **Q1, names of secondary mounts:** `/mnt/disk/pci-BB-DD-F` and
   `/mnt/disk/mmio-<base>`, derived from the location. The alternative is class-based
   names (`/mnt/sata0`, `/mnt/nvme0`), which would need a class field and a 48-byte
   record.
3. **Q2, choice of primary:** the unique device with a Cellos MBR (P2 type `0x7F`),
   else the lowest `(kind, id)`.
4. **Q3:** secondaries mount only FAT32 P1.
5. **Q4:** `/mnt/disk/` gets the `/mnt/sd/` access policy (read-all, write-all).
6. **Q5:** no hot-add. A device registering after VFS discovery waits for the next
   boot. The alternative is re-enumeration on demand, which is more VFS work.
7. **Q6:** route table cap of 8.
8. **Q7:** leave the pre-existing "drivers serve any sender" exposure out of 09C, or add
   a driver-side sender check against the attested VFS identity (a further semantic
   change).

## 11. CP2 — implementation and evidence (2026-11-01)

Status: implemented per CP1 v3; **CP2 awaiting user approval**. Nothing committed.

### 11.1 Public delta as implemented (matches §4.4)
- **416** `RegisterBlockDriver(kind, id, 0, 0)`: typed `usize::MAX-2` NotOwner,
  `usize::MAX-3` Malformed, `usize::MAX` Denied. It publishes only the caller-owned
  device's slot in `driver_cell::BLOCK_ROUTES` (8 append-only slots).
- **Ownership key, an implementation fact:** `RequestMmio` records the claiming
  **task id** as the window owner (`kernel/src/task/syscall.rs` `request_mmio(CellId(caller_id), …)`).
  So the kind-2 check is `lookup_mmio_owner(base) == caller tid`, the same per-task key
  as `owner_of_bdf`. The public semantics "the caller owns the window it claimed" are
  unchanged.
- **Lock order:** ownership is read without SCHEDULER held, because `request_mmio` takes
  the MMIO registry before SCHEDULER. The caller is re-checked live under SCHEDULER, and
  the slot is published before SCHEDULER is dropped.
- **432** `LookupBlockRoute(index, out, 40, 0)` returns the 40-byte `ViBlockRouteV1`
  (`libs/api/src/abi/block_route.rs`). It is gated on BlockIoCap or SpawnCap.
- **`RegisterService(BLOCK_DRIVER)`** is refused for every caller, and nothing publishes
  `BLOCK_DRIVER` any more.
- **ostd:** `sys_register_block_device(BlockDevice)` and `sys_lookup_block_route(index)`.
  `sys_register_block_driver` and the `ostd::dma` re-export are removed.
- **Q7:** each block driver (ahci, nvme, virtio-blk) answers NotIssued to any sender other
  than the registered VFS (`sys_lookup_service(VFS)`, tid cached). The VFS waits, for at
  most 500 ticks, until it is registered before its first block I/O. No new ABI.

### 11.2 Deviations
1. **Init block wait.** Init waits for every spawned block driver to bind a route or
   exit, then **waits for each exited driver's retirement verdict** before it starts
   the VFS.
   - The first part makes a q35 machine without a SATA disk wait for AHCI's idle exit.
     The VFS spawn then landed on the other CPU while AHCI's retirement shot down TLBs.
   - That raced a **pre-existing x86 SMP defect**: `panicked at
     kernel/src/memory/tlb_shootdown.rs:389: [x86-smp] shared mapping invalidation
     unconfirmed: Timeout { hart: 1 }`.
   - On the NVMe+NIC lane with `-smp 2`, 2 of 3 boots panicked, against 0 of 3 for a
     HEAD-built fault ISO. After the verdict wait: 5 of 5 boots clean.
   - The kernel defect is **not fixed here**. A retirement that overlaps a long spawn on
     another CPU can still hit it, so it needs its own report and fix.
2. **VFS host stand-in.** VFS host tests use a no-kernel stand-in for the block
   syscalls. On a Linux host, ostd's raw `Log` syscall number 11 is `munmap`.
3. **Witness adjustments:**
   - E1 does not order `Init: service restarted.` before the reset line, because init
     prints it after registration.
   - E9's "write during the gap" raced the restart in every run. Instead the sticky
     refusal is taken from the dead device's own follow-up writes between death and
     re-registration.
4. **Pre-existing blockers fixed or worked around:**
   - The recovery-demo F1/F5 unsafe gate failure (09B) is fixed with the approver's
     allowlist entries.
   - The canonical assembler route signs from a `.assemble-*` subdirectory and is
     refused (`scripts/cellos_sign/signing.py:181-182`). The production ISO was
     assembled through the candidate route instead: out-dir `/tmp`, kernel built with
     `EMBEDDED_OVERRIDE`.

### 11.3 Commands and results (final tree)
| # | Command | Result |
|---|---|---|
| 1 | `cargo test -p api --target x86_64-unknown-linux-gnu` | 169 passed |
| 2 | `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | 345 passed |
| 3 | `cargo test -p service-vfs` / `-p app-init` / `-p ostd` / `-p driver-nvme` (host) | 125 / 28 / 84+7+19 / 11 passed |
| 4 | x86 kernel `-D warnings` release build (`-C code-model=kernel … -Z cf-protection=full`) | finished, no warnings |
| 5 | `cargo run … tools/mutable-globals -- --check` | 596 declarations, 0 problems |
| 6 | `python3 scripts/check-spec-anchors.py --check` | OK (2 pre-existing spec-19 constant violations reported, not failing) |
| 7 | integration `--test driver-registration-contract` | 4/4 |
| 8 | `scripts/assemble-x86-image.py --profile headless --out-dir /tmp/p09c-x86img` + kernel + `make-iso-ci.sh`; `make-ahci-fault-iso.sh` | ISO `b6a37332…`; fault ISO `0b5f58e6…` |
| 9 | `--test ahci-x86` (production + fault ISO) | 14/14; plus E4(b) `ahci_09c_untracked_export_blocks_restart_x86` 1/1 |
| 10 | `--test nvme-x86` (production + fault ISO, incl. VT-d lane) | 11/11, 0 kernel panics |
| 11 | `bash scripts/build-x86_64-c2c-lifecycle-ci.sh`; `--test local-service-lifecycle-x86` | 2/2 |
| 12 | RV64 `CELLOS_INIT_FEATURES=input,hotswap-demo gen-disk-ci.sh`; `--test hotswap-smoke` | 13/13 |
| 13 | RV64 `…input,recovery-demo`; `--test provider-recovery` | 2/2 (two_harts first failed: virtio-blk 416 `NotOwner` until the tid ownership key fix in §11.1) |
| 14 | RV64 `…input,ai`; `--test boot init_gives_up_after_a_crash_storm` | 1/1 PASS (E10) |
| 15 | RV64 default `gen-disk-ci.sh`; `--test boot` | 51/65. The 14 failures are all in HEAD's failing set: HEAD measured 50/65, the same 14 plus a flaky `vfs_lifetime_selftest_passes` |
| 16 | `bash scripts/gen-disk-aarch64-ci.sh` | **pre-existing failure** at HEAD and here (`kernel_fs.img contains 16 /bin cells; expected 15`); the aarch64 image carries no block driver, so it is not a virtio-blk lane |
| 17 | Default RV64 image restored at the end (`bash scripts/gen-disk-ci.sh`) | `disk_v3.img` sha256 `ba914edbfd0d6bcd…` |

### 11.4 Evidence map (x86 q35 QEMU/TCG; serial lines quoted)
| # | Criterion | Observed |
|---|---|---|
| E1 | fresh authorized lease | AHCI `T0=4 T1=10 device=pci-00-1f-2 route=0`; NVMe `T0=3 T1=10 device=pci-00-02-0 route=0`. Each sequence: `[pcie] quiesced …`, `Init: <drv> retirement Contained (reasons=0x0 dma=0 qpages=0 leaked=0) — restarting`, reset line, `[iommu] Cell 10 granted DMA BDF=…`, `[driver_cell] block driver registered: tid=10 device=… route=0`, `[vfs] block device … provider tid=10 cell=… gen=115` |
| E2 | committed record via the new lease | boot 2 read R0 cache-cold through T1; boot 3 `read R0=F09C-E2-R0-COMMITTED and R1=F09C-E2-R1-NEW-LEASE from the medium` (AHCI, NVMe, NVMe+VT-d; 1 and 2 vCPUs) |
| E3 | uncertain write not replayed; medium read | `[ahci] fault-inject: driver exiting after PxCI at lba=4136` / NVMe after doorbell, exactly once per serial; `driver died before replying; outcome indeterminate`, `write lba=… indeterminate; not replayed`; `vcat d.txt` after the restart `""` = boot 3 `""` |
| E4 | device unavailable = real result | (a) host tests of `restart_decision` (every row); (b) `[ahci] fault-inject: driver exiting holding SHM` → `Init: ahci node recovery required: containment unproven, no restart`, no registration, later write fails unsent; (c) `DMA_DEFERRED` host-only (q35 without VT-d); (d) `Init: restart storm — giving up on this service (escalate).` for AHCI and NVMe, then `no live provider; not issued` |
| E5 | 1 and 2 vCPU | `[x86-smp] topology online=2 processor_ids=0;1 qualified=yes`; E1–E3 pass at `-smp 1` and `-smp 2` for AHCI, NVMe and NVMe+VT-d. The verdict line does not carry `remote_hart_mask` (not recorded) |
| E6 | independent owner survives | `e1000 tid=6 BDF=00:03.0 unchanged; NVMe pci-00-02-0 T3->T10` |
| E7 | IOMMU-fenced variant | NVMe on the VT-d helper: E1+E2 pass at 1 and 2 vCPUs; the lane's serial shows `[vtd] Cell 3 BDF 00:02.0 DID=1 SLPT=…` and `[vtd] Cell 4 DID=2 contexts cleared + caches drained`. AHCI under VT-d not run (unqualified) |
| E8 | 10 cycles | AHCI and NVMe: cycles 1–10 `T4->T10 … T18->T19 route=0 Init: … retirement Contained (reasons=0x0 dma=0 qpages=0 leaked=0)`; spaced 12 s |
| E9 | both disks, independent failure | discovery `block route 0 pci-00-03-0`, `block route 1 pci-00-1f-2`, `primary disk pci-00-03-0 (lowest key)`, `FAT32 /mnt/disk/pci-00-1f-2 volume mounted`; secondary killed: `pci-00-1f-2 route=1 T4->T10 …; survivor pci-00-03-0 tid=3 unchanged`; primary killed: `pci-00-03-0 route=0 T3->T11 …; survivor pci-00-1f-2 tid=10 unchanged`; sticky `[blk-router] <dev>: no live provider; not issued` during each gap; `boot 3 and the raw media hold each disk's files only on that disk` (1 and 2 vCPUs) |
| E10 | generic gate | RV64 `input,ai` image: `Init: config retirement Contained (reasons=0x0 dma=0 qpages=0 leaked=0) — restarting` before each of 6 config restarts; `[init-giveup] PASS` |
| R | regressions | rows 7, 9–15 above |

### 11.5 Not claimed
- Durable, reboot or power-cut retention (no CellosFS owner); boot-3 reads follow a
  clean QEMU exit.
- Physical hardware, KVM, AHCI under VT-d, a runtime `DMA_DEFERRED`.
- The pre-existing x86 SMP TLB-shootdown timeout (§11.2.1). The MMIO-release key
  mismatch is also not fixed: the funnel calls `release_for(CellId(cell_id))` while
  windows are owned by `CellId(tid)`. `request_mmio` skips dead owners, so restarts
  still claim.
