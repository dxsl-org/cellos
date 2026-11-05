# Slice S3 (phases 08, 09) — lock-handoff request

Status: **approved by the owner 2026-11-06 with every §6 recommendation ([record](s3-handoff-approval.md)); effective — S3 holds the Intel WIP lane.** Written 2026-11-06 against HEAD `78dd8b0b9`. Line numbers are HEAD lines and are re-located by symbol on the current tree. `[INFERENCE]` marks claims that were not read in code or observed in this session. The QEMU and VMware host facts in §1.4 and §1.5 come from read-only probes run in this session (no VM was created, started or modified).

## 1. Recon

### 1.1 Phase 08: what the e1000 family has today

- **e1000 cell (`/bin/e1000`) is an 82540EM-only cell that finds its device by class triple.**
  - `sys_find_pcie_device(0x02, 0x00, 0x00)` (`cells/drivers/e1000/src/main.rs:33-36,53-65`); BAR0 128 KiB (`:38-39`).
  - No identity check before programming registers. No SKU log line.
  - No `declare_syscalls!` (`main.rs:149-158` declares only the manifest). By the igb note, such a cell keeps its TCB's `u64::MAX` allowlist (`cells/drivers/igb/src/main.rs:225-231`).
  - EERD read uses the 8254x layout: `addr << 8 | START`, `DONE = 1 << 4` (`controller.rs:36,56-57,118-133`). The MAC comes from EEPROM words 0-2 and is written back to RAL0/RAH0 (`:206-219`).
  - Polled TX/RX. RX is a request (`dispatch.rs:3-16`, `controller.rs:264-354`).
- **Kernel exact-ID gate.**
  - The family table is `ETHERNET_DRIVER_IDS: [_; 5]` = `100E`, `10C9`, `1533`, `1AF4:1041`, `1AF4:1000` (`kernel/src/task/drivers/pcie_ecam.rs:36-42`).
  - The legacy class query (418) serves only `ETHERNET_LEGACY_CLASS_IDS = [(8086,100E)]` (`:62`, `:1216-1219`).
  - The vendor query (424) applies the family gate to class `02:00:00` (`:1248-1251`).
  - The scan prints `[e1000] unsupported Ethernet vvvv:dddd; driver gate closed` for every other Ethernet ID (`:670-680`).
  - Tests: `:1338-1374`. Today they assert that `10D3` is refused (`:1348`).
- **The igb cell is the pattern to copy.**
  - It discovers by exact ID through 424 over `QUERY_DEVICE_IDS` (`igb/src/main.rs:75-87`, `identity.rs:54-61`).
  - It runs `classify` as defence in depth (`main.rs:98-105`), logs a bind line with the SKU (`:142-154`) and declares an exact syscall list (`:232-255`).
- **NIC role.**
  - `RegisterNicDriver = 417` makes the caller the single `service::NIC_DRIVER`; a later registration overwrites the earlier one (`kernel/src/task/drivers/driver_cell.rs:249-261`; `syscall.rs:8814-8820`).
  - The net service reaches the driver with the cell-local opcodes `OP_TX = 0`, `OP_RX = 1` (`cells/services/net/src/interface.rs:52-61`). Its idle loop waits on `NET_RX` with a maintenance budget (`service-runtime.rs:901-911`), so x86 NIC RX is **request-driven**.
  - The bridge witness lines say `first e1000 TX/RX` whichever NIC cell is bound (`interface.rs:557,604`). Both `nic-x86` and `igb-x86` already assert them.
- **Harness.**
  - `scripts/qemu-x86_64-test.sh` accepts `X86_NIC_MODEL=e1000|e1000e|igb` (`:60-71`). **`e1000e` is a fail-closed check: it requires the refusal line for `8086:10d3` (`:123-127`).**
  - `nic-x86.rs` runs e1000 DHCP with and without VT-d and checks the order registration → TX → RX → DHCP (`tests/integration/tests/nic-x86.rs:72-104,124-165`). **`nic-x86` is not in CI.** The `qemu-x86_64-boot` job (`.github/workflows/ci.yml:933-1139`) runs `igb-x86` (`:1051-1059`), but no `nic-x86` step exists.
  - Runners: `boot_x86_bios_with_nic` uses a hard-coded `e1000` (`lib.rs:1434-1454`). The VT-d body already takes a model string (`:1630-1648`).

### 1.2 Phase 09: what vmxnet3/PVSCSI can reuse

- **ABI already in HEAD:**
  - `QueryPcieFunction = 434`, `AllocPcieIrq = 435`, `SystemPower = 436` (`libs/api/src/abi/syscall.rs:691,705,723`);
  - `RegisterBlockDriver = 416`, `LookupBlockRoute = 432` (`:477,663`);
  - `WaitCompletion(IRQ = 8)`, frozen (`cp2-04b-waitcompletion-irq.md:18-31`).
  - 434 returns all six BARs and the capability bytes (`cp1-pcie-irq-power-abi.md:44-59`). 435 refuses MSI-X if the table shares a page with registers (`:89-92`).
- **Storage template (virtio-scsi + `libs/scsi-block`).**
  - virtio-scsi discovers by exact ID through 424 (`cells/drivers/virtio-scsi/src/init.rs:16-17,61-73`). It probes the first disk through `scsi_block::ScsiDisk` (`scan.rs:23-77`), serves `types::block_wire` to VFS only (`dispatch.rs:1-90`, `main.rs:154-185`), registers `BlockDevice::Pci { bdf }` (`main.rs:137-146`) and logs `irq=polled reason=04b-deferred` (`:148`).
  - `scsi-block` has no unsafe code and is transport-independent. It names PVSCSI as its next consumer (`libs/scsi-block/src/lib.rs:1-10`). The seam is one trait method, `ScsiExecutor::execute(target, lun, cdb, data)` (`exec.rs:67-87`). **PVSCSI needs no `scsi-block` change.**
  - Route registration is generic: any live `PcieDriverCap`/`UsbDriverCap` task that owns the BDF can register (`kernel/src/task/syscall.rs:2416-2454`). There is no per-driver kernel edit and no VFS edit.
  - **Block cells must be spawned by init before VFS mounts:**
    - `start_block_drivers` spawns `/bin/nvme`, `/bin/ahci` and `/bin/virtio-scsi` (x86) and waits for routes (`cells/tools/init/src/boot.rs:514-546`);
    - so `/bin/pvscsi` needs one spawn there and no `services.toml` row.
- **IRQ template.**
  - nvme: allocate with 435, park with `wait_any(IRQ, 10)`, re-read the queue after every return, fall back with `irq=polled reason=…` (`cells/drivers/nvme/src/irq.rs:1-16`).
  - **NIC cells stay `request-driven` until a `MAILBOX` source exists:**
    - CP1-04b §3.6 (`cp1-04b-waitcompletion-irq.md:80-87`);
    - CP1-04b §7 (`:166-168`);
    - CP2-04b §4 (`cp2-04b-waitcompletion-irq.md:58`).
  - No NIC cell actually prints `irq=polled reason=request-driven` (git grep, this session).

### 1.3 Rows and gates every new cell needs

Pattern (`/bin/igb`, `/bin/virtio-scsi`):
- `kernel/src/task/cap.rs:465-479` (`with_path_caps`, `pcie_driver`);
- `kernel/src/loader/boot_ceiling.rs:159-164`; `boot_ceiling/selftest.rs:94-106` (`[PrivCapCase; 11]`);
- `kernel/src/loader/launch_profile/profiles.rs:25-29`; `launch_profile/tests.rs:1252-1264`;
- `scripts/sign-policy.py:150-165`;
- `config/services.toml:13-21` (NIC cells only);
- `scripts/assemble-x86-image.py:116-125`;
- `scripts/cellos_sign/signing.py:64-72`: assembler pin `X86_CANDIDATE_RECIPE_SHA256` = `7f32d28a…` (matches `sha256sum` of the assembler at HEAD) and `_X86_HEADLESS_SIGNED`;
- `scripts/test_assemble_x86_image.py:62-64`;
- `scripts/unsafe-allowlist.toml`: `[[crate]]` at `:665-670,693-698,721-726`, `[[file]]` at `:230-241,279,286,349,356`;
- `Cargo.toml:85-86,103` (members), `Cargo.lock`;
- `boards/src/descriptor.rs:52-68`; `boards/{pc/x86_64-pc,vm/x86_64-vm}/board.rs` (`:16-27`, `:11-22`). `DriverId` NIC/storage variants have **no kernel consumer** (git grep of `kernel/src`, `hal`).

Not needed:
- `launch_profile/targets.rs`: no driver rows there;
- `generate-boot-config.py`: selects every path by default (`:43-71`);
- ISO scripts: `make-iso-ci.sh` already builds BIOS+UEFI (`:2,54-55,73-80`);
- `docs/abi-boundary-records.md`: no ABI delta.

**Gates already red at HEAD** (`.agents/TODO.md:43,45,123-124`):
- `tools/mutable-globals --check`: 48 problems. Its scope is kernel and HAL crates only (`scripts/mutable-globals.toml` crate roots; zero `cells/` roots). S3's kernel edits are `const` tables, so **S3 adds no static**.
- The unsafe allowlist reports stale entries as `NOTE` only. The e1000 entries are dated 2026-07-14 (`unsafe-allowlist.toml:234`), past `max_age_days = 90` (`:28`).

**Tracked canonical image.** `kernel/src/embedded-x86_64/{kernel_fs.img,init,manifest.json,boot-config/*}` is tracked. Its last change, `1a182947d`, predates S1. S1 and S2 edited the assembler and the pin without republishing it.

### 1.4 QEMU 8.2.2 facts (probed this session with `-S` and `info pci`)

- **Device IDs:** `e1000-82545em` = `8086:100f`; `e1000e` = `8086:10d3`; `vmxnet3` = `15ad:07b0` (Ethernet class); `pvscsi` = `15ad:07c0` (SCSI class).
- **BARs:**
  - 82545EM: BAR0 128 KiB, BAR1 I/O;
  - e1000e: BAR0 128 KiB, BAR1 128 KiB, BAR2 I/O, BAR3 16 KiB (the MSI-X BAR, `[INFERENCE]` from size);
  - vmxnet3: BAR0 4 KiB, BAR1 4 KiB, BAR2 8 KiB (MSI-X `[INFERENCE]`);
  - pvscsi: BAR0 32 KiB only.
  - Capability lists were not probed. Whether pvscsi offers MSI is `[INFERENCE]`; 434 output in the lane settles it.
- **q35 adds a default NIC, `8086:10d3` (82574L), whenever no `-netdev`/`-nic` is given.** It is still added next to a bare `-device e1000` (two Ethernet functions). A `-netdev` suppresses it.

Consequences:
- Every runner that names no NIC carries an e1000e today: `boot_x86_bios` (`lib.rs:901-958`), `boot_x86_q35_lane` with no NIC devices (`:1212-1280`), and `qemu-x86_64-test.sh` with no model (`:61,101-112`). Today that device is refused.
- After phase 08, `/bin/e1000e` binds it.
- `virtio_pci_devices` and the machine-pin runner already pass `-nic none` (`lib.rs:194-197,1653`).
- See H-S3-7.

### 1.5 VMware host (read-only probes this session)

| Fact | Status |
|---|---|
| `/mnt/c/Program Files (x86)/VMware/VMware Workstation/vmrun.exe`: `vmrun version 1.17.0.24995812`; `-T ws list` → `Total running VMs: 0` | verified |
| Product: `vmware.exe` and `x64/vmware-vmx.exe` report `25.0.0 build-24995812` | verified |
| `vmware-vdiskmanager.exe`, `vmplayer.exe`, `vnetlib*.exe` present | verified |
| Host adapters: VMnet8 (NAT) `192.168.232.1`, VMnet1 (host-only) `192.168.174.1`; `C:\ProgramData\VMware\vmnetdhcp.conf` exists | verified |
| Windows `HypervisorPresent = True` (WSL2 / Hyper-V platform active) | verified |
| Windows `py -3` = Python 3.14.3, `powershell.exe` present; WSL has `wslpath`, `qemu-img` 8.2.2 | verified |
| Existing VMware evidence used Workstation Pro 17, BIOS, 1 vCPU, serial **to file only**, NIC/SATA/SCSI disabled (`docs/evidence/vmware-domain-test.txt:4-6`; `scripts/x86/vmware-boot-test.py:95-117`, NIC/SATA/SCSI off at `:109-113`). The host has since been upgraded to 25.0.0 | verified (scripts and evidence read); behaviour on 25.0.0 `[INFERENCE]` |
| With Hyper-V present, Workstation runs on the Windows hypervisor platform, and "Virtualize IOMMU" / nested VT-x are unavailable | `[INFERENCE]` — this is what makes plan A3 likely false on this host (§7.1) |

## 2. Exact file set (C = Create; everything else exists in HEAD)

**Phase 08: 82545EM in `/bin/e1000`, new `/bin/e1000e` (82574L)**

| Path | Scope |
|---|---|
| `cells/drivers/e1000/src/identity.rs` (C) | `{100E: 82540EM, 100F: 82545EM}`, `classify`, `sku_name`, query list (igb pattern) |
| `cells/drivers/e1000/src/main.rs` | discovery via 424 over the identity list instead of 418 (`:53-65`, H-S3-6); identity check; bind line `[e1000] controller bound 8086:dddd <SKU> … irq=polled reason=request-driven`; exact `declare_syscalls!` (the igb set) |
| `cells/drivers/e1000/src/controller.rs` | header comment only (`:1`); the register model is unchanged (plan A2) |
| `cells/drivers/e1000e/**` (C) | `Cargo.toml`, `build.rs`, `src/{main,identity,regs,controller,phy,rings,irq,dispatch,dma_layout}.rs`, each under 200 lines. `identity.rs` returns `MacKind::{I82574, PchLan(gen)}`, with only `10D3` Driveable and I219 IDs listed as Target (`reference-nic-cell-contract.md:8`). 82574 EERD layout, MDIC PHY autoneg, legacy rings through `GrantDma`, `CTRL_EXT`/IVAR, MSI-X through 435 under H-S3-4. `dispatch.rs` serves the unchanged NIC wire (`e1000/src/dispatch.rs:3-16`) |
| `kernel/src/task/drivers/pcie_ecam.rs` | `ETHERNET_DRIVER_IDS` `+ (8086,100F) /bin/e1000`, `+ (8086,10D3) /bin/e1000e` (`:36-42` and docs `:23-35`); `ETHERNET_LEGACY_CLASS_IDS` **unchanged** (`:62`); tests `:1338-1374` (flip `:1348`; add 100F → e1000, 10D3 → e1000e, an unknown Intel ID refused by name, the class query still serves only 100E) |
| `kernel/src/task/cap.rs` | `with_path_caps` `+ "/bin/e1000e"` (`:465-476`); net-rights census `+` the new path (`:1054-1066`) |
| `kernel/src/loader/boot_ceiling.rs`, `boot_ceiling/selftest.rs` | `:159-160` `+ "/bin/e1000e"`; `:94-106` array 11 → 12 |
| `kernel/src/loader/launch_profile/{profiles,tests}.rs` | `profiles.rs:25-29` `+ "/bin/e1000e"`; `tests.rs:1252-1262` `+` the path |
| `scripts/sign-policy.py` | one row after `:164`, the same shape as `/bin/igb` |
| `config/services.toml` | an `e1000e` row after `:21` (`restart = "never"`) |
| `scripts/assemble-x86-image.py`, `scripts/cellos_sign/signing.py`, `scripts/test_assemble_x86_image.py` | the cell row after `:118`; re-pin `X86_CANDIDATE_RECIPE_SHA256` and add `driver-e1000e` to `_X86_HEADLESS_SIGNED` (`signing.py:65-72`); the package list (`test_assemble…:62-64`) |
| `scripts/unsafe-allowlist.toml` | `[[crate]] driver-e1000e` and `[[file]]` rows for its unsafe files; refresh the date and approver of the touched e1000 entries (`:230-241,665-670`) |
| `Cargo.toml`, `Cargo.lock` | member line after `:85`; lock entry (append-only) |
| `boards/src/descriptor.rs`, `boards/pc/x86_64-pc/board.rs`, `boards/vm/x86_64-vm/board.rs`, `boards/src/catalog_tests.rs` | `DriverId::EthernetE1000e` after `:63`; listed in pc and vm (arrays 10 → 11); a catalog assertion |
| `tests/integration/src/lib.rs` | public `boot_x86_bios_with_nic_model(iso, disk, model, vtd)` over the existing bodies (`:1434-1454,1630-1648`) |
| `tests/integration/tests/nic-x86.rs` | models `e1000`, `e1000-82545em`, `e1000e` × {plain, VT-d}; per-model bind line; no sibling NIC cell touches the device; `irq=msix` for e1000e under H-S3-4 |
| `scripts/qemu-x86_64-test.sh` | models `e1000-82545em` and `vmxnet3` (the latter in 09); `-nic none` plus a restricted `-netdev` for every explicit model (`:60-71`); the `e1000e` check becomes positive (`:123-127`); with no model, assert that `/bin/e1000e` bound the default NIC (H-S3-7) |
| `.github/workflows/ci.yml` | job `qemu-x86_64-boot` only: one new step `nic-x86` (6 lanes, TCG), next to `igb-x86` (`:1047-1059`) |
| `docs/hardware-compatibility-list.md` | R6 wording (`:79`, `:125-128`): 82545EM/82574L are admitted IDs at QEMU/VMware evidence; I219 stays unimplemented |
| `CHANGELOG.md`, `phase-08-*.md`, `reports/phase-08-evidence.md` (C), `plan.md` row | records |

**Phase 09: `/bin/vmxnet3`, `/bin/pvscsi`, VMware matrix**

| Path | Scope |
|---|---|
| `cells/drivers/vmxnet3/**` (C) | `src/{main,identity,regs,shared,rings,irq,dispatch}.rs`. BAR0 (PT) and BAR1 (VD) are mapped from the 434 record. `VRRS`/`UVRS` = 1. `DriverShared` plus 1 Tx and 1 Rx queue through `GrantDma`. `ACTIVATE_DEV`, `GET_LINK`, `UPDATE_MAC_FILTERS`. MSI-X through 435 under H-S3-4. Unchanged NIC wire |
| `cells/drivers/pvscsi/**` (C) | `src/{main,regs,rings,irq,dispatch,scan}.rs`. Ring state and request/completion rings through `GrantDma`, `SETUP_RINGS`, kicks, `impl ScsiExecutor`, first disk through `scsi-block`, `BlockDevice::Pci` route, `block_wire` dispatch to VFS only (the virtio-scsi pattern), MSI-X/MSI completion waits (H-S3-5) |
| `kernel/src/task/drivers/pcie_ecam.rs` | `ETHERNET_DRIVER_IDS` `+ (15AD,07B0) /bin/vmxnet3`, test row. PVSCSI is class `01:00:00` and is not gated by the Ethernet table (`:1248-1251`) |
| `kernel/src/task/cap.rs`, `boot_ceiling.rs`, `boot_ceiling/selftest.rs`, `launch_profile/{profiles,tests}.rs`, `scripts/sign-policy.py` | `/bin/vmxnet3` and `/bin/pvscsi` rows exactly as for `/bin/e1000e` (selftest array 12 → 14) |
| `config/services.toml` | a `vmxnet3` row (NIC). No `pvscsi` row: init spawns block cells |
| `cells/tools/init/src/boot.rs` | `start_block_drivers`: one x86 spawn of `/bin/pvscsi`, supervised, after virtio-scsi (`:536-546`); doc `:514-516` |
| `scripts/assemble-x86-image.py`, `signing.py`, `test_assemble_x86_image.py`, `unsafe-allowlist.toml`, `Cargo.toml`, `Cargo.lock` | two cells; re-pin; signed set; allowlist rows; members |
| `boards/src/descriptor.rs`, `boards/{pc/x86_64-pc,vm/x86_64-vm}/board.rs`, `catalog_tests.rs` | `DriverId::{EthernetVmxnet3, StoragePvscsi}`; listed in pc and vm (vIOMMU guests run `x86_64-pc`, ADR-0031 §4) |
| `tests/integration/src/lib.rs` | `vmxnet3` through `boot_x86_bios_with_nic_model`; `pvscsi_devices(nvme_disk, scsi_disk, vtd)` (`-nic none`, `pvscsi` + `scsi-hd`; shape of `virtio_pci_devices`, `:190-218`) |
| `tests/integration/tests/vmxnet3-x86.rs` (C), `tests/integration/tests/pvscsi-x86.rs` (C) | vmxnet3 DHCP ± VT-d; PVSCSI two-boot persistence on the `virtio-x86.rs:59-83` pattern (NVMe primary + PVSCSI secondary), PVSCSI on the `x86_64-pc` ISO with `intel-iommu`, `irq=msix\|msi` asserted |
| `scripts/qemu-x86_64-test.sh`, `.github/workflows/ci.yml` (`qemu-x86_64-boot` only) | `X86_NIC_MODEL=vmxnet3`; steps `vmxnet3-x86` and `pvscsi-x86` |
| `scripts/x86/vmware-boot-test.py` | `make_vmx` (`:95-117`) takes NIC, storage, vCPU (`--cpus` choices `:179-180` → 1, 2, 4), firmware, HW version, pipe serial and vIOMMU parameters; stale HPET comment `:102` |
| `scripts/x86/test-native-vmware-matrix.py` | new `nic × storage × vcpu × firmware` scenarios with two-boot persistence and JSON summary; existing AHCI fault scenarios kept |
| `scripts/x86/test_native_vmware_matrix.py` | VMX-generation and verdict unit tests (synthetic, not runtime evidence, `:1`) |
| `scripts/x86/vmware_serial_relay.py` (C) | runs under Windows `py -3`; bridges `\\.\pipe\cellos-<run>` ↔ stdio for the WSL harness |
| `docs/hardware-compatibility-list.md`, `CHANGELOG.md`, `phase-09-*.md`, `reports/phase-09-evidence.md` (C), `reports/phase-09-vmware-matrix.json` (C), `plan.md` row | records |

**Not in the set (pinned byte-identical in S3):**
- `libs/api/**`, `libs/types/**`, `libs/ostd/**`, `libs/scsi-block/**`, `libs/virtio-pci-transport/**`;
- `cells/services/net/**`, `cells/services/vfs/**`;
- `cells/drivers/{igb,virtio-*,nvme,ahci,xhci}/**`;
- `kernel/src/task/{syscall,task,scheduler,smp,completion*}.rs`, `kernel/src/task/drivers/{driver_cell,irq_wait,pcie_msi,iommu*,block}.rs`, `kernel/src/main.rs`, `hal/**`;
- `scripts/check-multi-source-wait-law1-digests.sh`;
- the tracked canonical `kernel/src/embedded-x86_64/*` (H-S3-12);
- `scripts/x86/make-iso-ci.sh`, `launch_profile/targets.rs`;
- the G1 lane.

## 3. Overlap analysis

| # | Against | File | Lines / symbols | Resolution |
|---|---|---|---|---|
| O1 | iGPU DMA-export, approved not effective (`cell-contract-0024-locks.md:169-174`; file set `cp1-gpu-kernel-abi.md:276-279`) | `tests/integration/src/lib.rs`, `CHANGELOG.md` | iGPU: `boot_x86_q35` variant with `edu`; S3: NIC/PVSCSI runners | Append-only; the second lander rebases. S3 holds none of condition 2's files (`syscall.rs`, `task.rs`, `iommu{,_x86}.rs`, `libs/api/src/abi/syscall.rs`), so condition 2 stays met |
| O2 | iGPU condition 3: phases 04a, 04b, **12** (`locks.md:173`) | `phase-12-*.md`, `iommu_x86.rs`, ACPI DMAR | none | **Unaffected.** S3 only reads the `[acpi] gates: … dmar=` line (`kernel/src/main.rs:437`) for A3 |
| O3 | Large memory, approved not effective (`locks.md:175`; `cp1-tier2-large-memory.md:255-258`) | `scripts/sign-policy.py` (v9 `anon_limit_mib`) | S3: three rows after `:164` | Sequential under single-WIP. If v9 lands first, the S3 rows take the column default |
| O4 | Runtime R5 XSAVE, needs the lane (`locks.md:177`; `cp1-x86-xsave-user-state.md:142-152`) | `hal/arch/x86/**`, `main.rs`, launch row | S3: none of these | Disjoint |
| O5 | Runtime H-1 rows (`locks.md:178-179`) | `profiles.rs`, `tests.rs`, `boot_ceiling.rs` | `/bin/cpp-hosted-smoke` and `/bin/drm-lite-x86` already landed in `6c61ba1ec` (`targets.rs:77-79`, `boot_ceiling.rs:221-223`). Only `/bin/x86-fpu-preserve` remains, and its cell does not exist | Append-only; S3 does not carry it. `locks.md:179` ("three rows still need the lane") has drifted |
| O6 | Kernel-repair Phase 07, locked (`260927-0739-…/phase-07-warm-snapshot.md:16-17`: `syscall.rs`, `main.rs`, `drivers/block.rs`, `scheduler.rs`/`smp.rs`, `tests/integration/tests/launch-profile.rs`) | none | Block routes use unchanged 416 (`syscall.rs:2416-2454`) | **No grant needed** |
| O7 | Frozen Law-1 surfaces: multi-source wait (`locks.md:142`), `IRQ = 8`, 434/435 (CP2-04a), 436 (CP2-06) | `completion.rs`, digest script, `syscall.rs` | used, not edited | The digest script stays green trivially |
| O8 | Row `261004-1957-x86-pc-lane` ("device-authority … affecting Cell contracts", `locks.md:225`) | `pcie_ecam.rs` family gate | +3 exact IDs; 418 class semantics unchanged | Narrow for S3, as S1 did for 04a's widening |
| O9 | Rows `260624-0630` (MMIO/DMA/IRQ/registry authority migration, `:197`), `260819-1416` (shared device/DMA authority, `:212`), `260712-0952` (guest device authority; "platform breadth stays parked", `:201`) | new driver cells | existing 424/434/435/233/416/417 authority only; no migration; `260712-0952` concerns Cellos-as-hypervisor guest devices | Compatible (H-S3-10) |
| O10 | NIC singleton and two-NIC machines | `driver_cell.rs:249-261` (last registration wins) | `qemu-x86_64-test.sh` puts a bare `-device e1000` next to q35's default e1000e (§1.4) | Explicit models get `-nic none` (§2). In-tree lanes otherwise have at most one admitted NIC. A multi-NIC policy is out of scope |
| O11 | Default q35 NIC in every unnamed-NIC lane (§1.4) | `lib.rs:901-958,1212-1280`, `qemu-x86_64-test.sh` | after 08 these lanes bind e1000e and run DHCP | H-S3-7 |
| O12 | Gates already red (`TODO.md:43,45`) | mutable-globals 48, IDT fixture | S3 adds no kernel/HAL static (`const` tables only) and does not touch IDT | S3 acceptance: count not increased. Fixing the red gates is out of scope |
| O13 | Apple A2 (needs own handoff, `locks.md:185`), N02-A (proposed, `:82-86`) | aarch64, `cspace.rs` | none | No overlap |
| O14 | Shared append-only files: `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `config/services.toml`, `assemble-x86-image.py` + pin | members, rows | every slice | The pin is re-computed by whichever slice lands last |

## 4. Law-1 items

**4.1 No Law-1 delta.**
- No syscall, allowlist bit, service ID, completion source or ABI record changes.
- The cells use only HEAD surfaces: 424, 434, 435, 233/`GrantAlloc`, 416, 417 (`service::NIC_DRIVER = 10`, `libs/api/src/abi/syscall.rs:1581`), and 242 with `IRQ`.
- **Cell-local wire, unchanged:**
  - the NIC protocol `OP_TX/OP_RX/OP_GETMAC` (`e1000/src/dispatch.rs:3-16` ↔ `net/src/interface.rs:52-61`);
  - `types::block_wire` to VFS.
- **Kernel policy, not wire:** the Ethernet family gate gains `100F`, `10D3` and `07B0`. 418 class semantics stay byte-identical because `ETHERNET_LEGACY_CLASS_IDS` is unchanged. Same class of change as phase 04a's widening (`pcie_ecam.rs:28-31`).
- `boards::DriverId` variants live in `boards/`, not `libs/api`/`libs/types`.
- **Stop rule:** if implementation needs a `libs/api`, `libs/types`, `libs/ostd` or `cells/services/net` edit, the slice stops and a CP1 is drafted. Therefore S3 has no CP1/CP2; it closes at the phase-09 checkpoint (H-S3-13).

**4.2 `MAILBOX` source: not needed for S3; scoped as its own CP1.**
- S3 needs no `MAILBOX` source, because the net service drives RX by request (§1.1).
- If a measured need appears (RX latency or throughput on C2C KVM/VMware nodes), a separate "CP1 — `MAILBOX` source and NIC RX protocol" would contain:
  - `completion::source::MAILBOX = 1 << 4` on 242: level-triggered on `pending_msgs`, `result = 0`, woken through `wake_after_ipc_publish` (`cp1-04b-waitcompletion-irq.md:84-87`);
  - `SELECTION_ORDER` 4 → 5 and an `ALL` update, with the digest script, Specs 17/03b/20 and ostd `Wake` in the same commit;
  - a decision on the `task.rs` NET_RX-only interruption (digest-pinned, `cp1-04b…:28,87`);
  - a net-service RX push or long-poll protocol (`cells/services/net/**`);
  - conversion of all NIC cells;
  - rulings on rows `260927-1100` (completion) and `261006-0934` (completion capacity).
- It needs its own handoff and CP2. **Recommendation: do not draft it now** (H-S3-4).

**4.3 Proposed handoff text (inventory entry):** "Bounded adopted slice: x86 platform breadth, slice S3 (phases 08, 09). File-bounded exception for exactly the §2 paths of `s3-handoff-request.md`. No Law-1 delta: no syscall, flag, allowlist bit, service ID, completion source or `libs/api`/`libs/types` byte changes. The cells use 424/434/435/233/416/417 and `WaitCompletion(IRQ)` as frozen. Kernel policy: the Ethernet family gate admits `8086:100F` (`/bin/e1000`), `8086:10D3` (`/bin/e1000e`) and `15AD:07B0` (`/bin/vmxnet3`). The class-triple table is unchanged. Row `261004-1957` is narrowed for this widening. Rows `260624-0630`, `260819-1416` and `260712-0952` are ruled compatible. No kernel-repair Phase 07 overlap. Pinned byte-identical: `libs/{api,types,ostd,scsi-block,virtio-pci-transport}/**`, `cells/services/{net,vfs}/**`, `kernel/src/task/{syscall,task,scheduler}.rs`, `drivers/{driver_cell,pcie_msi,irq_wait,iommu*,block}.rs`, `hal/**`, the digest script, and the G1 lane. Evidence ceiling: QEMU TCG (CI), QEMU/KVM (local), VMware Workstation (local). S3 closes at the phase-09 checkpoint with an independent DMA/ring review. All 38 relations keep their whole-plan scope."

**Proposed portfolio row:**

| Field | Value |
|---|---|
| Slice | S3 VMware, phases 08, 09 |
| C2C link | Repeatable C2C nodes as default VMware guests. The 82574L cell is the base for I219 (phase 13), and I219 is the NIC of the approved iGPU machine class (`plan-portfolio.md:201`) |
| Acceptance | `vmware-default-vm`: the `x86_64-vm` ISO on Workstation with each of e1000/e1000e/vmxnet3 and AHCI/NVMe/PVSCSI reaches `Cellos >`, completes DHCP and persists across two boots. Each model also passes its QEMU lane in CI |
| Ceiling | QEMU TCG + QEMU/KVM (local) + VMware local |

## 5. Sequencing options

| Option | Order | Unblocks first | Cost |
|---|---|---|---|
| (a) S3 as one slice | V0 VMware tooling smoke → 08.1 82545EM → 08.2-3 e1000e → 09.1 vmxnet3 → 09.2 PVSCSI → 09.3-4 VMware matrix → checkpoint | VMware nodes; e1000e base for 13 | Phase 12 (iGPU condition 3) waits for the whole of S3 |
| (b) S3 code, VMware matrix as a side lane | 08 → 09.1-2 → release the lane; 09.3-4 runs beside S4 on the VMware script and report files only; the S3 checkpoint waits for the matrix | Phase 12 sooner | Two activities at once; needs an explicit single-WIP exception like D-R2 |
| (c) S4 phase 12 first | 12 → S3 | iGPU condition 3 | Reorders the plan (S3 before S4, `plan.md:65`); VMware nodes later |

**Recommendation: (a).**
- The VMware matrix is the S3 deliverable, and the plan's checkpoint sits on phase 09.
- (a) needs no new exception to the single-WIP rule.
- Running V0 first retires the most uncertain item (pipe input under Workstation 25 on a Hyper-V host) before any driver work.
- Choose (b) if phase 12 / iGPU has priority over VMware throughput. The files are disjoint, so (b) costs only the exception.
- Choose (c) only if iGPU outranks VMware nodes outright.

## 6. Owner decision checklist

| ID | Question | Recommendation |
|---|---|---|
| H-S3-1 | Order (a), (b) or (c), §5 | (a) |
| H-S3-2 | Approve the S3 file-bounded exception for exactly §2, effective now (lane free since `78dd8b0b9`) | Yes |
| H-S3-3 | No Law-1 delta (§4.1): the NIC and block wires stay cell-local and unchanged; the family-gate widening is kernel policy; the stop rule applies; no CP1/CP2 | Yes |
| H-S3-4 | NIC interrupts: (a) all NIC cells polled, `irq=polled reason=request-driven`, no 435; (b) e1000e and vmxnet3 allocate MSI-X with 435 and park on `wait_any(IRQ, ≤10)` only for bounded bring-up, link and TX-completion waits, re-reading the device after every return; RX stays request-driven; log `irq=msix rx=request-driven`; 82540/82545 stay polled; (c) the `MAILBOX` CP1 (§4.2) inside S3 | (b). `MAILBOX` stays its own CP1, drafted only on measured need |
| H-S3-5 | PVSCSI completion: IRQ-driven (435 MSI-X/MSI + `WaitCompletion(IRQ)`, deadline-bounded, nvme/ahci pattern) or polled `04b-deferred` like virtio-scsi | IRQ-driven, with fallback `irq=polled reason=…` |
| H-S3-6 | 82545EM admission: (a) `/bin/e1000` moves to 424 with its own identity table and an exact `declare_syscalls!`; `ETHERNET_LEGACY_CLASS_IDS` stays `[100E]`; (b) add `100F` to the legacy class table and leave the cell on 418 | (a): follows `reference-nic-cell-contract.md:9`; 418 unchanged; also narrows e1000's allowlist from `u64::MAX` |
| H-S3-7 | q35's default NIC (`8086:10d3`, §1.4): (a) keep it, so every unnamed-NIC lane binds `/bin/e1000e`; assert this in the CI default boot; explicit models get `-nic none`; (b) add `-nic none` to every runner that names no NIC | (a): free e1000e coverage that matches what a real KVM/VMware default guest has. (b) is the rollback lever if a lane flakes |
| H-S3-8 | VMware lane as in §7.1: named-pipe serial plus a Windows `py -3` relay; the `x86_64-vm` ISO as baseline; matrix {e1000, e1000e, vmxnet3} × {sata, nvme, pvscsi} × {2, 4} × {bios, efi}, two boots each; JSON evidence. If the V0 relay smoke fails, persistence is reported `NOT-RUN`, never `PASS` | Yes |
| H-S3-9 | A3: when "Virtualize IOMMU" is unavailable on this Hyper-V host, record A3 false, run the matrix under `x86_64-vm` only, and claim no VMware production-guest result (ADR-0031 §4, `0031-…md:42`) | Yes |
| H-S3-10 | Lock rulings: narrow `261004-1957` for the gate widening; rule `260624-0630`, `260819-1416` and `260712-0952` compatible; no kernel-repair Phase 07 grant | Yes |
| H-S3-11 | iGPU: condition 2 stays met (S3 holds none of its files); condition 3 still waits for phase 12. Large memory, R5 and the `/bin/x86-fpu-preserve` row stay queued; append-only overlaps rebase (O1-O5) | Yes |
| H-S3-12 | Do not republish the tracked canonical `kernel/src/embedded-x86_64/*`; change only the assembler and its pin; CI assembles fresh (S1/S2 precedent) | Yes |
| H-S3-13 | S3 closes at the phase-09 checkpoint, with an independent review of the e1000e, vmxnet3 and PVSCSI DMA/ring code (`phase-09-*.md:84`) | Yes |
| H-S3-14 | Register constants for vmxnet3/PVSCSI: VMware's public behaviour and BSD-licensed FreeBSD headers, with `NOTICE` attribution for any copied table. Linux and QEMU (GPL) are references only (`reference-nic-cell-contract.md:24`, `phase-09-*.md:30`) | Yes |

## 7. Per-phase verification, rollback, irreversibility

| Phase | Host | QEMU TCG (CI, `qemu-x86_64-boot`) | QEMU/KVM (local, `CELLOS_X86_ACCEL=kvm`, `lib.rs:154-170`) | VMware local | Undo | Cannot be undone |
|---|---|---|---|---|---|---|
| 08 | `cargo test -p cellos-kernel` (gate: 100F → e1000, 10D3 → e1000e, 10C9 → igb, an unknown Intel ID refused by name, the class query still serves only 100E). e1000e unit tests on a fake register file: identity, 82574 EERD, MDIC/autoneg, descriptor layout, IVAR. Boards catalog, boot-ceiling selftest, launch-profile tests. `test_assemble_x86_image.py`. `cellos-sign --check`. mutable-globals count not above 48 | `nic-x86` × {e1000, e1000-82545em, e1000e} × {plain, VT-d}: registration → TX → RX → DHCP order; bind line names the SKU; no sibling NIC cell touches the device. Default boot asserts e1000e bound. Whole-job regression (igb, virtio, ahci, xhci, power, machine-pin, serial, `-smp 4`) | same `nic-x86` set; `-smp 4` | rows e1000/e1000e of the 09 matrix (A2) | `git revert` per step (cells and rows first, then the kernel table) | none (kernel policy table; no ABI, no on-disk format) |
| 09 | vmxnet3: `DriverShared`/queue-descriptor size and offset asserts, gen-bit ring logic, command encoding, version negotiation. PVSCSI: ring-state and request-descriptor layout, LUN encoding, malformed completion → error, not panic. `scsi-block` tests unchanged. `test_native_vmware_matrix.py`: VMX generation per matrix cell, verdicts, relay framing | `vmxnet3-x86` DHCP ± VT-d; `pvscsi-x86` two-boot persistence plus the `x86_64-pc` + `intel-iommu` variant; `irq=msix\|msi` for PVSCSI | same | V0 smoke; full matrix (§7.1); A2/A3 records | revert; the matrix JSON stays as a historical record | committed VMware evidence (append-only); no ABI |

**7.1 VMware local lane (design; evidence ceiling "VMware local")**

- **Driver.** WSL2 → `vmrun.exe -T ws` through the existing `find_vmrun`/`to_host_path` (`vmware-boot-test.py:32-35,53-92`):
  - workspace under `/mnt/c/Temp/cellos-vmware` (`:195-196`);
  - `start <vmx> nogui` (`:258`); hard stop plus a `list` check (`:146-162`).
  - Boot 1 ends with `/bin/shutdown` (S2 S5, flushed) and falls back to a hard stop.
- **VMX.** Built from `make_vmx`/`make_native_vmx` (`vmware-boot-test.py:95-117`, `test-native-vmware-matrix.py:63-84`), extended with:
  - `firmware = "bios"|"efi"` (plus Secure Boot off for EFI);
  - `numvcpus` 2 or 4;
  - `ethernet0.present = "TRUE"`, `ethernet0.virtualDev = "e1000"|"e1000e"|"vmxnet3"`, `ethernet0.connectionType = "nat"`;
  - `sata0` / `nvme0` / `scsi0.virtualDev = "pvscsi"` with a `:0.fileName` disk;
  - `serial0.fileType = "pipe"` (server end);
  - for A3, `vvtd.enable = "TRUE"`;
  - one pinned `virtualHW.version`, recorded from `vmware.log`. Today the scripts disagree: 10 at `vmware-boot-test.py:98`, 14 at `test-native-vmware-matrix.py:65`.
  - Every new key is `[INFERENCE]` (not yet in the repo). V0 proves each one before the matrix runs.
- **Disks.** FAT32 fixtures → VMDK through `qemu-img convert` (pattern at `test-native-vmware-matrix.py:203`). After power-off, the host converts back to raw and checks the boot-1 nonce independently of the guest.
- **Serial.**
  - Output: a file (pattern proven by the 2026-10-09 evidence).
  - Input: a named pipe bridged by `vmware_serial_relay.py` under Windows Python 3.14.3 `[INFERENCE: not exercised]`.
- **Images.**
  - Baseline: `make-iso-ci.sh` with `limine-vm.conf` (`cellos.profile=x86_64-vm`; the same command as `ci.yml:1067-1069`).
  - vIOMMU variant: `limine-pc.conf` (`x86_64-pc`).
  - The default ISO binds `qemu-q35-x86_64` under any hypervisor (`kernel/src/board.rs:280-281`), the wrong descriptor for VMware, so it is not used.
- **Verdict per matrix cell:**
  - `Cellos >`;
  - the NIC bind line and `[net] DHCP acquired` (VMnet8 192.168.232.0/24; `cells/services/net/src/dhcp.rs:69`);
  - the storage ready line;
  - the boot-2 guest read of the boot-1 nonce, plus the host read-back;
  - no panic or fault;
  - cleanup confirmed.
  - 36 cells × 2 boots, plus the A3 boots.
- **A2** (QEMU `e1000-82545em` ≈ VMware 82545EM) holds when the same `/bin/e1000` binary passes `nic-x86` and the VMware e1000 rows. Any register-level difference becomes a phase-09 Surprise.
- **A3** holds when `vvtd.enable` with EFI and the `x86_64-pc` ISO shows `dmar=` present in `[acpi] gates:` (`main.rs:437`), `board=x86_64-pc` and no `Init: SAFE MODE`. Expected false on this host `[INFERENCE]` (§1.5), which leads to H-S3-9.
- **Ceiling.** Workstation 25.0.0 build 24995812 on a Windows host with Hyper-V, driven from WSL2. Not CI, not ESXi, not physical; never promoted (`plan.md:67`).

**Plan-text drift to fix under S3:**
- Phase 08 cites `pcie_ecam.rs:27-60,1174-1291`; now `:23-80` (tables), `:1196-1257` (queries), `:1338-1374` (tests).
- Phase 08 omits three facts:
  - e1000 discovers through the class query (`main.rs:56`), served only for `100E` (`pcie_ecam.rs:62`);
  - `qemu-x86_64-test.sh:123-127` asserts the e1000e refusal;
  - q35's default NIC is 82574L (§1.4).
- Phase 08 lists "ISO scripts" and `e1000e-x86.rs`. No ISO-script change is needed, and its success criterion uses `nic-x86`, so S3 parameterizes `nic-x86` and creates no `e1000e-x86.rs`.
- Phase 08 asks for "MSI-X (3 vectors)" while its assumption is "QEMU e1000e 5 vectors". 435 grants ≤ `want`, so the 434 log settles it.
- `nic-x86` is not in the CI job (`ci.yml:933-1139`).
- Phase 09: the HPET assumption is moot after phase 02. `vmware-boot-test.py:102` ("Cellos requires … HPET") is stale.
- Phase 09: the "Workstation Pro installed" assumption holds. The installed version is 25.0.0, not the 17 of `vmware-domain-test.txt:6`.
- Phase 09: two-boot persistence needs serial input, which no VMware script has (file-only serial at `vmware-boot-test.py:106-107`).
- Phase 09 context cites `vmware-boot-test.py:109-113` and `test-native-vmware-matrix.py:73-83`. Both still hold.
- CP1-04b `:166-168` and CP2-04b `:58` say the NIC cells log `irq=polled reason=request-driven`. None does. S3 adds the line to `/bin/e1000` only; igb and virtio-net stay out of the set.
- `cell-contract-0024-locks.md:179`: two of the three H-1 rows have landed (`6c61ba1ec`).
- `ci.yml:956-957` calls `kernel_fs.img` gitignored. It is tracked.
