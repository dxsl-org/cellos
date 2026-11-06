# Phase 09 evidence — vmxnet3 and PVSCSI cells (09.1-09.2) and the VMware matrix (09.3-09.4)

Recorded 2026-11-06 on branch `feat/x86-s3-phase09-vmxnet3-pvscsi` (worktree `/home/dmin/worktrees/cellos-s3-phase09`, base `8d0a4d982`). Ceiling for this section: **QEMU TCG + QEMU/KVM (local)**. Host: WSL2 `6.18.40.1-microsoft-standard-WSL2`, QEMU 8.2.2. Nothing here is VMware or physical evidence.

Images were assembled outside the repository on the dev-key candidate route (`/home/dmin/worktrees/s3p09-out/rebuild.sh`: `assemble-x86-image.py --profile headless --out-dir …/embedded`, kernel with `EMBEDDED_OVERRIDE` and the manifest's `kernel_features`, ISOs with `make-iso-ci.sh` for the default, `limine-pc.conf` and `limine-vm.conf`). The tracked `kernel/src/embedded-x86_64/*` was not touched (H-S3-12). Read-back: `published …/kernel_fs.img (12582912 B, sha256 55e146714a3365e157fceb964391752a0f391028c2d934c70e30a3792e98462a), 32 files` (phase 08: 30; + `/bin/vmxnet3`, `/bin/pvscsi`). Assembler pin re-computed: `X86_CANDIDATE_RECIPE_SHA256 = 8c9b1ed0…07928f`.

Register constants come from FreeBSD `if_vmxreg.h` (ISC) and `pvscsi.h` (BSD-2-Clause), attributed in `NOTICE` (H-S3-14); QEMU `hw/net/vmxnet3.c` and `hw/scsi/vmw_pvscsi.c` v8.2.2 were read as behaviour references only.

## Host gates

| Gate | Result |
|---|---|
| `cargo test -p driver-vmxnet3` | `test result: ok. 13 passed` — identity (only `15AD:07B0` driveable; `0720`, `07C0`, foreign vendor refused), revision/UPT negotiation and refusal by value, permanent-MAC queries, register map, `DriverShared` (720 B) and queue descriptor (2 × 256 B) offsets pinned to the packed device layout, encoders write exactly those offsets, interrupt plan (3/2/1 vectors, polled), generation-bit wrap, TX/RX descriptor encoding (generation word last), completion decoding by generation, DMA layout |
| `cargo test -p driver-pvscsi` | `test result: ok. 8 passed` — command handshake order and status, RW vs non-RW kick, ring-state/request (128 B)/completion (32 B)/`SETUP_RINGS` (132 words) layouts, PPN words, LUN re-encoding (peripheral/flat < 256 accepted, others refused), request encoding (direction flags, no-data case, CDB > 16 refused), completion decoding (residual, QEMU `dataLen = 0` quirk, sense clamp), malformed completions → error (foreign context, overrun, status > 0xFF, selection timeout, host status), all-byte-pattern decode without panic |
| `cargo test -p cellos-kernel` | `test result: ok. 500 passed` (incl. `ethernet_family_gate_covers_driver_cells_only`: `15AD:07B0 → /bin/vmxnet3`, `15AD:07C0` and `15AD:0720` refused as Ethernet; `class_triple_serves_only_the_legacy_family`; boot-ceiling selftest array 14; launch-profile rows) |
| `cargo test -p cellos-boards` | `test result: ok. 15 passed` (pc and vm declare `EthernetVmxnet3`, `StoragePvscsi`) |
| `cargo test -p app-init` | `test result: ok. 41 passed` |
| `python3 -m unittest scripts/test_assemble_x86_image.py` | `Ran 40 tests … OK` |
| `python3 scripts/cellos-sign --check --strict` | `OK: F1 — 140 crates and 1034 files scanned; unsafe confined to 77 allowlisted files` |
| `bash scripts/check-multi-source-wait-law1-digests.sh` | `PASS: multi-source WaitCompletion Law-1 items intact` |
| `mutable-globals --check` | `640 declarations, 255 need review, 48 problems` (= base 48; the kernel edits are `const` tables). It also prints `docs/generated/mutable-globals.md is not current`; the generated doc is outside the §2 set and was not regenerated |
| clippy `-D warnings` | `driver-vmxnet3`, `driver-pvscsi` on `x86_64-unknown-none` and `riscv64gc-unknown-none-elf`, `--tests` on host; `cellos-kernel` and `app-init` on `x86_64-unknown-none` — clean |
| `libs/api`, `libs/types`, `libs/scsi-block`, `cells/services/**`, `kernel/src/task/syscall.rs`, tracked embedded image | untouched (`git status`) |

## QEMU lanes (phase 09)

`VICELL_NIC_ISO`/`VICELL_X86_ISO` = default ISO, `VICELL_X86_PC_ISO` = `limine-pc.conf` ISO; `cargo test --test vmxnet3-x86 --test pvscsi-x86`.

| Run | Result |
|---|---|
| TCG | `vmxnet3-x86`: `ok. 2 passed`; `pvscsi-x86`: `ok. 3 passed` |
| KVM (`CELLOS_X86_ACCEL=kvm`) | `pvscsi-x86`: `ok. 3 passed … finished in 133.32s`; `vmxnet3-x86`: `ok. 2 passed` |
| `scripts/qemu-x86_64-test.sh`, `X86_NIC_MODEL=vmxnet3`, TCG | `PASS: x86_64 shell prompt reached — full boot successful` |

vmxnet3 (both lanes assert VT-d active first when set, bind < registration < TX < RX < DHCP, exactly one NIC cell and one registration, no gate refusal):

```
[ INFO] [msi] 00:02.0 tid=13 kind=msix irq=0x30+3
[vmxnet3-x86 vtd=true] [vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit=0xfed90000
[vmxnet3-x86 vtd=true] [vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=1000Mb/s mac=52:54:00:12:34:56 irq=msix rx=request-driven vectors=3 irq_wakes=0 park_expiries=0
[vmxnet3-x86 vtd=false] [driver_cell] NIC driver registered: tid=13
[vmxnet3-x86 vtd=false] [net-bridge] first e1000 TX accepted=true
[vmxnet3-x86 vtd=false] [net-bridge] first e1000 RX len=590
[vmxnet3-x86 vtd=false] [net] DHCP acquired — IP configured
USER: [net] IP address: 10.0.2.15
```

"Passes traffic" here is the DHCP exchange through the cell (DISCOVER/REQUEST out, OFFER/ACK in); no other traffic generator exists in the image. `irq_wakes=0`: QEMU completes TX synchronously and the link is up at activation, so every bounded wait ended inside its spin.

PVSCSI (NVMe primary + PVSCSI secondary; boot 1 writes `/mnt/disk/pci-00-03-0/pvscsi.txt`, boot 2 reads it back; bind line must carry `irq=msix|msi`):

```
P09 q35 boot 1:            [pvscsi] pci 00:03.0 15ad:07c0 target=0 lun=0 blocks=528384 irq=msi vectors=1 irq_wakes=0 park_expiries=0
                           [vfs] FAT32 /mnt/disk/pci-00-03-0 volume mounted
P09 q35 + intel-iommu boot 2:  same bind line, same mount
P09 x86_64-pc + intel-iommu boot 1/2:  same bind line, same mount
KVM: [pvscsi] pci 00:03.0 15ad:07c0 target=0 lun=0 blocks=528384 irq=msi vectors=1 irq_wakes=7 park_expiries=0
```

See the phase file's deviation log for D4 (QEMU never fills `dataLen`), D5 (QEMU's PVSCSI ring/sense accesses bypass the IOMMU, so the VT-d lanes do not prove confinement of those pages) and D6 (`[pvscsi] irq wait expired after 10 ticks` after bind: TCG 10, KVM 2 lines; bounded, cause open).

### Bus ≥ 1 probe (integrator request; not a CI lane)

TCG, `vmxnet3` on `pcie-root-port` rp1 and `pvscsi` + blank 64 MiB `scsi-hd` on rp2:

```
USER: [platform] unsupported Ethernet identity — registration skipped
USER: [platform] registered bus 2 device 0 function 0 class 1:0:0
USER: [pvscsi] pci 02:00.0 15ad:07c0 target=0 lun=0 blocks=131072 irq=msi vectors=1 irq_wakes=0 park_expiries=0
USER: [vmxnet3] no vmxnet3 controller present; driver cell idle
```

vmxnet3 behind a root port is dropped by the platform cell's Ethernet filter (`scan.rs`, not edited, owner decision pending); PVSCSI at bus 2 binds. (The blank disk then logs `Read sector=… failed: OutOfRange` while VFS probes it; expected for an unformatted disk.)

## Regression (same ISOs)

| Suite | Result |
|---|---|
| `nic-x86` TCG / KVM | `ok. 6 passed` / `ok. 6 passed` |
| `x86_64-boot` TCG | `ok. 10 passed` (the two tier-2 refusal tests first stopped in `ci_guard` until `bash scripts/build-x86_64-prod-refusal-ci.sh` built `build/vicell-x86-prod-refusal.iso`, as in phase 08) |
| `-smp 4` (`CELLOS_X86_SMP=4 … x86_smp_brings_every_cpu_online`) | `ok. 1 passed` |
| `virtio-x86` | `ok. 4 passed` |
| `ahci-x86` | `ok. 15 passed` |
| `nvme-x86` | `ok. 11 passed` |
| `igb-x86` | `ok. 2 passed` (with `VICELL_IGB_VTD_ISO` set; without it the VT-d test stops in `ci_guard`) |

Not re-run this phase: `xhci-x86`, `iommu-dmar-x86`, `machine-pin-x86`, `serial-x86`, `power-x86` (no file of theirs changed; CI runs them).

## VMware matrix (09.3-09.4)

Recorded 2026-11-06 on `feat/x86-s3-integration` (worktree `/home/dmin/worktrees/cellos-s3-int`). The ISO was built from the tree of `283345b32`: the merged S3 tip `087d99ab1` plus the matrix findings' fixes below (kernel memory decoding at claim time when every memory BAR is placed, vmxnet3 all-ones refusal and promiscuous receive).

**Ceiling: VMware local.**
- VMware Workstation 25.0.0 build 24995812 (`vmrun version 1.17.0.24995812`), virtual hardware 22 (read back from every `vmware.log`).
- Windows host with Hyper-V, driven from WSL2; relay under `py -3` 3.14.3.
- `x86_64-vm` only: A3 is false (V0-S5), so H-S3-9 applies and there is no VMware production-guest claim.
- Not CI, ESXi or physical hardware.

**Verdict: 39/39 cells PASS** — the 36 grid cells plus one `pciBridge0` cell per storage type, two boots each, on every dimension. Every VM was created under `C:\Temp\cellos-vmware\s3-lane\` and deleted: `cleanup_confirmed: true` ×39, the lane root was empty and `Total running VMs: 0` after the run.

Run `20261106T020521Z-a3694a`: `python3 scripts/x86/test-native-vmware-matrix.py matrix --iso …/vicell-x86-vm.iso --jobs 3`, which took 559 s. Two earlier full runs also gave 39/39 and are superseded:
- `20261106T013618Z-6528a8` on `14640724a`: vmxnet3 got memory decoding from an early `GrantDma`, an order that was withdrawn (D-S5).
- `20261106T015154Z-4728a7` on `cb015dc1e`: claim-time decoding without the BAR-assigned check.

Run details:
- ISO: `vicell-x86-vm.iso`, sha256 `21af039cf4a30ff222af5a859aa02467322f1483f6082f72d16eb25753add465`. It was built by `make-iso-ci.sh` with `limine-vm.conf` from an out-of-tree headless assembly; tracked `kernel/src/embedded-x86_64/*` was not touched.
- JSON: [s3-phase09-vmware-matrix.json](s3-phase09-vmware-matrix.json). It drops the PCI bridge and root-port enumeration lines from the per-boot markers and is otherwise verbatim. Its `reduction` block names the full summary (`build/x86-backend-evidence/vmware-lane/20261106T020521Z-a3694a/summary.json`, sha256 in the file), which is retained with every serial log, VMX and `vmware.log`.

### How a cell is judged (`matrix_scenarios`, `lane_verdict`)

VMX from the shared `make_vmx`:
- `ethernet0.virtualDev` on NAT; the storage controller (`sata0`, `nvme0`, or `scsi0` with `virtualDev = "pvscsi"`) with the lane disk on target 0;
- `numvcpus` 2 or 4; `firmware` bios or efi (Secure Boot off); `memsize = 1024` for **both** firmwares, now that S3 is fixed;
- root ports `pciBridge4-7` always; `pciBridge0` only in the three bridge cells.

The disk comes from `machine_pin_disk.py`, then a `qemu-img` seed, then `vmware-vdiskmanager -r -t 0`.

Boot 1:
1. typed `uname -a` must print `Cellos cellos-kernel …` (relay input);
2. `vwrite` + `vcat` of a nonce;
3. wait up to 45 s for the lease;
4. `/bin/shutdown`.

Boot 2: `vcat` of the nonce, the lease wait, `/bin/shutdown`. Then the nonce is read back on the host from the powered-off VMDK (`qemu-img` + `mtype`).

A cell passes only when every dimension passes on **both** boots:

| Dimension | Required on each boot |
|---|---|
| Prompt | `Cellos >` inside 120 s; `[x86-gate] board=x86_64-vm`; `[x86-smp] topology online=<vCPUs>`; no panic, `[fault] cell` or tier-2 admission marker |
| NIC | the cell's own VMware identity bound (`[e1000] controller bound 8086:100f`, `[e1000e] controller bound 8086:10d3` or `[vmxnet3] controller bound 15ad:07b0`), `[driver_cell] NIC driver registered`, `[net] DHCP acquired`; no `driver gate closed`, `[platform] registration refused`, `unsupported Ethernet identity`, and no idle line from the cell's own NIC |
| Persistence | `block driver registered`, `[vfs] FAT32 /mnt/sd volume mounted`, boot 1 `machine pin: first boot`, boot 2 `machine pin: match`, no `SAFE MODE`; boot-2 guest `vcat` and host read-back both equal the boot-1 nonce |
| Guest S5 | the VM left `vmrun list` within 30 s of the typed `/bin/shutdown`, with no hard stop |

### Results

Totals: status, prompt, NIC, persistence and S5 are each **PASS 39 / FAIL 0**. Each grid entry gives the time to the prompt from `vmrun start` (boot 1 / boot 2).

| NIC | Storage | 2 vCPU BIOS | 2 vCPU EFI | 4 vCPU BIOS | 4 vCPU EFI |
|---|---|---|---|---|---|
| e1000 | sata | PASS (6.0 / 4.7 s) | PASS (5.9 / 6.8 s) | PASS (6.1 / 6.0 s) | PASS (6.9 / 6.7 s) |
| e1000 | nvme | PASS (5.5 / 5.1 s) | PASS (6.1 / 6.5 s) | PASS (7.7 / 6.6 s) | PASS (7.4 / 7.2 s) |
| e1000 | pvscsi | PASS (6.6 / 4.7 s) | PASS (6.1 / 6.3 s) | PASS (7.0 / 6.7 s) | PASS (7.1 / 8.0 s) |
| e1000e | sata | PASS (6.0 / 6.0 s) | PASS (6.2 / 6.2 s) | PASS (7.8 / 7.2 s) | PASS (5.6 / 6.8 s) |
| e1000e | nvme | PASS (4.4 / 4.5 s) | PASS (5.3 / 4.4 s) | PASS (5.3 / 4.3 s) | PASS (5.1 / 5.3 s) |
| e1000e | pvscsi | PASS (4.1 / 3.5 s) | PASS (5.0 / 4.7 s) | PASS (5.9 / 4.4 s) | PASS (6.0 / 5.2 s) |
| vmxnet3 | sata | PASS (4.4 / 3.6 s) | PASS (4.6 / 3.7 s) | PASS (4.5 / 3.8 s) | PASS (4.4 / 4.1 s) |
| vmxnet3 | nvme | PASS (3.8 / 3.4 s) | PASS (3.5 / 3.6 s) | PASS (9.7 / 5.4 s) | PASS (8.6 / 6.0 s) |
| vmxnet3 | pvscsi | PASS (5.8 / 3.5 s) | PASS (4.3 / 4.6 s) | PASS (4.7 / 4.4 s) | PASS (4.4 / 4.7 s) |

| `pciBridge0` cell | Result | Decisive lines (boot 1) |
|---|---|---|
| `bridge0-sata-e1000-c2-bios` | PASS | `platform-reg 02:00.0 class=02:00:00 bar0=0xfd5c0000 mem=0xfd5c0000+0x20000`, `platform-reg 02:01.0 class=01:06:01 bar0=0x0 mem=0xfd5ef000+0x1000`, `[ahci] ABAR claim ok base=0xfd5ef000 len=0x1000`, `[ahci] controller bound bdf=02:01.0`, `[e1000] controller bound 8086:100f 82545EM`, `DHCP acquired` |
| `bridge0-nvme-e1000e-c4-efi` | PASS | `platform-reg 03:00.0 class=02:00:00 … mem=0xfea20000+0x20000`, `platform-reg 0b:00.0 class=01:08:02 …`, `block driver registered: tid=4 device=pci-0b-00-0`, `[e1000e] controller bound 8086:10d3 82574L link_up=true … irq=msix … vectors=3`, `DHCP acquired` |
| `bridge0-pvscsi-vmxnet3-c2-efi` | PASS | `platform-reg 03:00.0 class=01:07:00 bar0=0x0 mem=0xfea10000+0x8000`, `[pvscsi] pci 03:00.0 15ad:07c0 target=0 lun=0 blocks=1127680 irq=msix vectors=1`, `platform-reg 0b:00.0 class=02:00:00`, `[vmxnet3] controller bound 15ad:07b0 … irq=msix … vectors=3`, `DHCP acquired` |

Decisive lines (quoted from run `…4728a7`; the final run printed the same lines with other generated MACs, e.g. `vmxnet3-pvscsi-c4-efi` → `[vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=10000Mb/s mac=00:0c:29:b5:81:31 irq=msix`):
- **NIC.** `[e1000] controller bound 8086:100f 82545EM mac=00:0c:29:a2:60:3a irq=polled reason=request-driven` (13 cells, bus 0 or behind `pciBridge0`); `[e1000e] controller bound 8086:10d3 82574L link_up=true … irq=msix rx=request-driven vectors=3` (bus 2/3/0x0a behind root ports); `[vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=10000Mb/s mac=00:0c:29:f9:0b:87 irq=msix rx=request-driven vectors=3` (`vmxnet3-pvscsi-c4-efi`). On every boot: `[net] DHCP acquired — IP configured`, `[net] IP address: 192.168.232.128`.
- **Storage.** `[ahci] controller bound bdf=00:11.0 cap=0xc1341f1d version=0x00010300`; NVMe `platform-reg 0a:00.0 class=01:08:02 bar0=0xfe000000 mem=0xfe000000+0x4000` → `block driver registered: tid=4 device=pci-0a-00-0 route=0`; `[pvscsi] pci 02:00.0 15ad:07c0 target=0 lun=0 blocks=1127680 irq=msix vectors=1 irq_wakes=3 park_expiries=0` as the only disk → `/mnt/sd`; then `machine pin: first boot — recorded …` / `machine pin: match …`.
- **S5 (BIOS and EFI).** `[acpi] power: \_S5_ static SLP_TYPa=0 SLP_TYPb=0`, then `System shutting down...` and `[power] poweroff: PM1a_CNT 0x444 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0`; `guest_poweroff: true`, `hard_stop: false` on all 78 boots.
- **PVSCSI on the real device (D6 input).** The bind line reports `irq=msix vectors=1` on all 26 PVSCSI boots of each run (QEMU offers MSI only), with `irq_wakes=0..6 park_expiries=0`. No run has a `[pvscsi] irq wait expired` line. The post-park re-reads of D6 are therefore so far a QEMU-model observation.

### Matrix failures found and fixed (D-S5, D-S6 in the phase file)

Smoke runs on the merged tip, before the fixes:

| Run | Cell | Decisive line | Root cause → fix |
|---|---|---|---|
| `20261106T011319Z-733d6b`, `…011941Z-0c8d5c` | `vmxnet3-pvscsi-c4-efi` | `[vmxnet3] controller initialization failed: unusable MAC ff:ff:ff:ff:ff:ff`, then `[net] NIC driver unavailable: legacy frame path` | BAR1 reads all-ones under VMware EFI only. Under BIOS, the same build read `mac=00:0c:29:bd:6e:34` through the same `GET_PERM_MAC` queries (`20261106T012342Z-e08a62`). `[INFERENCE]` EFI leaves memory decoding off on a function it has no driver for. The cell read BAR1 before anything turned decoding on: only 435 and `GrantDma` did, and both also set `BUS_MASTER`. First fix `6d3192477`: the DMA area was authorized before the first register access (debug run `20261106T012818Z-ce969d`: `controller bound … mac=00:0c:29:86:99:1c`). The DMA/ring review rejected it, because bus mastering then came on before `CMD_RESET_DEV`: without an IOMMU, a relaunched cell's still-active device could DMA into freed frames. **Final fix `173fb292d` + `cb015dc1e` + `283345b32`:** `offer_pcie_device` sets Memory Space alone right after the BDF claim (`pcie_ecam::enable_memory_decode`; bus mastering waits for `GrantDma`). It does so only when every memory BAR has a base (`memory_bars_assigned`), so an unassigned BAR never decodes at address 0. vmxnet3 is back to reset → MSI-X → `GrantDma`, and `negotiate` still refuses an all-ones `VRRS`/`UVRS`. Final run: `vmxnet3-pvscsi-c4-efi` → `controller bound … mac=00:0c:29:b5:81:31`, and all 20 EFI cells (18 grid, 2 bridge) pass |
| `20261106T012342Z-e08a62`, `…011941Z-0c8d5c` | `vmxnet3-sata-c2-bios` | bound, `[net-bridge] first e1000 TX accepted=true`, `first e1000 RX len=131`, no lease; boot 2 `[net-bridge] NIC driver reply timeout; frame not acknowledged` | A debug build (`20261106T013135Z-3e6040`, not committed) showed TX and RX working. The transmitted DHCP DISCOVER is `ffffffffffff 525400123456 0800 …` with chaddr `525400123456`: the net service frames with its fixed `52:54:00:12:34:56`, not the vNIC's `00:0c:29:…`. Every RX was broadcast or multicast, and the unicast OFFER never passed the device's unicast filter. The e1000 cells receive it because they run `RCTL.UPE\|MPE`. QEMU hid this because its default vNIC MAC is the same `52:54:00:12:34:56`. **Fix `14640724a`:** vmxnet3 `rxMode` adds PROMISC (`0x1D`). Re-run `20261106T013523Z-5ae857`: `vmxnet3-sata-c2-bios`, `vmxnet3-pvscsi-c4-efi` and `bridge0-pvscsi-vmxnet3-c2-efi` all PASS |
| `20261106T011319Z-733d6b` | `e1000-sata-c2-efi`, `e1000e-nvme-c2-bios` | `NIC refused: [vmxnet3] no vmxnet3 controller present` | Harness defect: the first sweep treated every idle NIC cell as a refusal. Only the cell's own idle line counts now (unit test `test_refusal_or_own_idle_fails_while_other_idle_cells_do_not`) |

A first hypothesis was rejected. It held that VMware does not answer `GET_PERM_MAC_LO/HI` and that the MAC should be read from `MACL`/`MACH`. Built and run, the EFI cell still read `ff:ff:ff:ff:ff:ff`, and the BIOS run above showed that the queries work. The change was reverted before any commit.

## S3 integration gates (tip `087d99ab1` + this phase's commits)

Host, on the integration tree:

| Gate | Result |
|---|---|
| `cargo test -p cellos-kernel` (host; parallel and `--test-threads=1`) | `ok. 507 passed` both on the merged tip; `509 passed` on `283345b32` (new `claim_enables_memory_decoding_but_never_bus_mastering`, `decoding_waits_for_every_memory_bar_to_be_assigned`) |
| `cargo test -p driver-e1000e -p driver-vmxnet3 -p driver-pvscsi -p service-platform` | `20` / `13` / `8` / `9 passed` (`driver-e1000` has `test = false`) |
| `cargo test -p cellos-boards` / `-p app-init` | `15` / `41 passed` |
| `scripts/test_assemble_x86_image.py` / `scripts/test_cellos_sign.py` | `Ran 40 … OK` / `Ran 67 … OK (skipped=1)`. Assembler pin `8c9b1ed0…07928f` = `sha256(scripts/assemble-x86-image.py)`; the merge changed no input |
| `cellos-sign --check --strict` | `OK: F1 — 142 crates and 1060 files scanned; unsafe confined to 79 allowlisted files` |
| Law-1 scripts | `multi-source WaitCompletion Law-1 items intact`, `the frozen AI interface matches its Law 1 record`, `LookupServiceBound Law-1 items intact` |
| `mutable-globals --check` | `640 declarations, 255 need review, 48 problems` (= base). It reported `docs/generated/mutable-globals.md is not current`, so the doc was regenerated with `--write` (`bbce27082`: upstream `invept::CAPS`, `service_registry::OWNERS`, `va_alloc::SLOTS`, `gpu_scanout` rows) and is current afterwards |
| clippy `-D warnings` | `cellos-kernel` on x86_64 / riscv64 / aarch64; `driver-{e1000,e1000e,vmxnet3,pvscsi}`, `service-platform`, `app-init` on x86_64-unknown-none and riscv64; `--tests` host for e1000e/vmxnet3/pvscsi. All clean; kernel (3 targets) and vmxnet3 re-run after the fixes. `service-platform --tests` on the host reports 15 dead-code errors in functions that are `cfg(not(test))`-only; these predate S3 (the S1 diff only removed a filter) and are not a CI gate |
| `cargo fmt --all --check` | **fails** — not from the fixes here (their hunks are formatted). The diffs are in S3-merged code: `pcie_ecam.rs` `ETHERNET_DRIVER_IDS` comment alignment and an `ethernet_driver_cell` test (phases 08/09), and `acpi/power.rs`, `acpi/tests.rs` (S4). They are also in pre-S3 code: `acpi/{mod,sci}.rs`, the `SystemPower` arms in `syscall.rs`, `supervisor/src/{power,protocol}.rs`, `sys-tools/src/bin/shutdown.rs`, `drm-cellos`/`cpp-hosted-smoke`/`drm-lite-x86` `build.rs`. These were left unformatted because another branch is editing `pcie_ecam.rs` |
| `scripts/x86/test_native_vmware_matrix.py` | `Ran 43 tests … OK` (31 before; 12 new `MatrixSweepTests`) |

QEMU 8.2.2, ISOs from the same tree (`CELLOS_X86_SMP=4` for `x86_64-boot`; `build/vicell-x86-prod-refusal.iso` built by `build-x86_64-prod-refusal-ci.sh`). The same counts passed on every default ISO: the merged tip (sha256 `325a7817…60c79b`), `14640724a` (`bcf1efdf…97c633`), `cb015dc1e` (`3fcd62bc…f4a737`) and the final `283345b32` (`3769ef11…d4bd03`):

| Suite | TCG | KVM |
|---|---|---|
| `x86_64-boot` (SMP 4) | 10 | 10 |
| `nic-x86` | 6 | 6 |
| `vmxnet3-x86` | 2 | 2 |
| `pvscsi-x86` | 3 | 3 |
| `pcie-multibus-x86` | 3 | 3 |
| `ahci-x86` | 15 | 15 |
| `nvme-x86` | 11 | 11 |
| `virtio-x86` | 4 | 4 |
| `power-x86` | 3 | 3 |
| `iommu-dmar-x86` | 3 | 3 |
| `xhci-x86` | 4 | 4 |
| `igb-x86` (with `VICELL_IGB_VTD_ISO`) | 2 | 2 |

Other lanes:
- **OVMF (q35, `-smp 1`, all three default ISOs).** 1 / 2 / 4 GiB reach `Cellos >` under KVM (`clock=kvmclock … apic=x2apic`) and TCG (`clock=hpet … apic=xapic`); e.g. 4 GiB: `allocator: 14 usable range(s), 1037377 frames (4052 MiB)`.
- **riscv64 default lane.** `PASS: FAT16 mounted — kernel booted (no disk)`.
- **aarch64 default lane.** `PASS: aarch64 shell prompt reached — full boot successful`.
- The regenerated `kernel/src/embedded{,-aarch64}/init` were restored.

## Final checkpoint run (tip `eab0755c6`)

Recorded 2026-11-06 on `feat/x86-s3-integration` at `eab0755c6` (worktree `/home/dmin/worktrees/cellos-s3-int`). This tip is everything above plus the first review's fixes, merged from `feat/x86-s3-review-fixes`:
- `9bea3ddad`: syscall 236 records unadmitted Ethernet functions and re-reads the class under the `PCI_DEVICES` lock.
- `fb328b418`: e1000e TX/RX fixes.
- `c162ed509`: the vmxnet3 RX vector is masked only when it serves RX alone.
- `d7cec8514`: PVSCSI classifies expired parks (D6) and refuses a translated DMA area (D5).
- `b626704fb`: new `machine-pin-x86` test `q35_unbound_root_port_nic_is_in_the_pci_set`.
- `290126206` and `12b9c29b6`: a comment fix and an ACPI test.

The VMware matrix above (`…a3694a`) ran before this merge. Every gate below was re-run on this tip.

Images were built outside the repository (`/home/dmin/worktrees/s3int-out/final/rebuild.sh`, the same route as above). Read-back: `published …/kernel_fs.img (12582912 B, sha256 aaa30cf2ea49433055727056aab5da15fa20902993c1fec48749ff758cdb3a15), 32 files`. ISO sha256:
- default `8884456a516d83bcc64fa5527da66180eadf8ba6bd786aca81bd69270c7a6ff3`;
- `limine-pc.conf` `46723fc95a54529ce926a7c71b91a9d07d699de4abc04e6403d6065a75854210`;
- `limine-vm.conf` (the VMware ISO) `322ba0aedd9eb07540c24ea10fce3f140356cb4af625d58003734be7a71764a5`.

`build/vicell-x86-prod-refusal.iso` was rebuilt on this tip (`2d9cfd3a…83de`). Tracked `kernel/src/embedded-x86_64/*` was not touched. The riscv64/aarch64 lanes regenerate `kernel/src/embedded{,-aarch64}/init`; both were restored, and `git status` was clean afterwards.

**Verdict: every requested gate passes.**
- Host: all gates pass.
- QEMU TCG and KVM: 13 suites each, 72/72 tests per accelerator.
- OVMF: 6/6 boots.
- riscv64 and aarch64 lanes pass.
- VMware: **39/39**.

The one non-gate failure is the pre-existing `service-platform --tests` host clippy (see below). No failure needs attribution.

### Host

| Gate | Result |
|---|---|
| `cargo test -p cellos-kernel` (host; parallel and `--test-threads=1`) | `test result: ok. 511 passed` both (509 on `283345b32`) |
| `cargo test -p driver-e1000e -p driver-vmxnet3 -p driver-pvscsi -p service-platform` | `22` / `13` / `10` / `9 passed` (`driver-e1000` has `test = false`) |
| `cargo test -p cellos-boards` / `-p app-init` | `15` / `41 passed` |
| `scripts/test_assemble_x86_image.py` / `scripts/test_cellos_sign.py` | `Ran 40 tests … OK` / `Ran 67 tests … OK (skipped=1)`. Pin check: `X86_CANDIDATE_RECIPE_SHA256 = "8c9b1ed0…d07928f"` equals `sha256sum scripts/assemble-x86-image.py` (`8c9b1ed0f0af27a0ecd9eb04ec10f5270cf73dd216d85d45bbb5b8672d07928f`) |
| `scripts/x86/test_native_vmware_matrix.py` | `Ran 43 tests … OK` |
| `cellos-sign --check --strict` | `OK: F1 — 142 crates and 1060 files scanned; unsafe confined to 79 allowlisted files`, `OK: F5 — rustc f53b654a8882 is the pinned nightly-2026-05-01` |
| Law-1 scripts | `PASS: multi-source WaitCompletion Law-1 items intact …`, `OK: the frozen AI interface matches its Law 1 record`, `PASS: LookupServiceBound Law-1 items intact …` |
| `mutable-globals --check` (CI recipe: `cargo run --release --manifest-path tools/mutable-globals/Cargo.toml -- --check`) | `640 declarations, 255 need review, 48 problems` (= base, ≤ 48). It printed no `is not current` line, so `docs/generated/mutable-globals.md` is current. Tool tests: `7 passed` |
| clippy `-D warnings` | All clean: `cellos-kernel` on x86_64 / riscv64 / aarch64; `driver-{e1000,e1000e,vmxnet3,pvscsi}`, `service-platform` and `app-init` on x86_64-unknown-none and riscv64; `--tests` on the host for `driver-{e1000e,vmxnet3,pvscsi}`. `service-platform --tests` on the host still fails with the same 15 dead-code errors in `cfg(not(test))`-only functions (`function scan_and_register is never used`, …): pre-existing and not a CI gate, as recorded above |

### QEMU (8.2.2; the ISOs above; `CELLOS_X86_SMP=4` for `x86_64-boot`)

| Suite | TCG | KVM |
|---|---|---|
| `x86_64-boot` (SMP 4, incl. `x86_smp_brings_every_cpu_online`) | 10 | 10 |
| `nic-x86` | 6 | 6 |
| `vmxnet3-x86` | 2 | 2 |
| `pvscsi-x86` | 3 | 3 |
| `pcie-multibus-x86` | 3 | 3 |
| `machine-pin-x86` (incl. `q35_unbound_root_port_nic_is_in_the_pci_set`) | 6 | 6 |
| `ahci-x86` | 15 | 15 |
| `nvme-x86` | 11 | 11 |
| `virtio-x86` | 4 | 4 |
| `power-x86` | 3 | 3 |
| `iommu-dmar-x86` | 3 | 3 |
| `xhci-x86` | 4 | 4 |
| `igb-x86` (with `VICELL_IGB_VTD_ISO`) | 2 | 2 |

Each cell is `test result: ok. N passed; 0 failed`.

Decisive lines:
- **vmxnet3, both accelerators, ± VT-d:**
  - `[vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit=0xfed90000`
  - `[vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=1000Mb/s mac=52:54:00:12:34:56 irq=msix rx=request-driven vectors=3 irq_wakes=0 park_expiries=0`
- **e1000e (`nic-x86`, KVM):** `[e1000e] controller bound 8086:10d3 82574L link_up=true … irq=msix rx=request-driven vectors=3 irq_wakes=1 park_expiries=0`.
- **PVSCSI and the D6 expiry counters.** The counters are now classified per cell lifetime (`d7cec8514`). On every boot the bind line is `[pvscsi] pci 00:03.0 15ad:07c0 target=0 lun=0 blocks=528384 irq=msi vectors=1 irq_wakes=W park_expiries=0 expiry_ready=0 expiry_empty=0`:
  - TCG: `W` = 0 on five boots and 9 on one.
  - KVM: `W` = 0, 1, 3 (three boots) and 4.

  After bind, over 3 lanes × 2 boots:

  | Accelerator | `irq wait expired` lines | `ring=ready` | `ring=empty` | `elapsed_ms` |
  |---|---|---|---|---|
  | TCG | 5 | 1 (`… ring=ready elapsed_ms=98 expiry_ready=1 expiry_empty=0`) | 4 | 93–200 |
  | KVM | 4 | 0 | 4 (e.g. `… ring=empty elapsed_ms=99 expiry_ready=0 expiry_empty=2`) | 96–112 |

  The phase-09 recording had TCG 10 / KVM 2 unclassified lines. Most expiries now find the ring **empty**: the park ran out while no completion had been written, so the expiry was not a missed interrupt. A completion missed by the MSI and found on the re-read happened once (TCG).

Other lanes:
- **OVMF** (q35, `-smp 1`, default ISO). All six boots reach `Cellos >`:
  - KVM (`clock=kvmclock`, `apic=x2apic`), 6 s each: 1 GiB `allocator: 13 usable range(s), 250946 frames (980 MiB)`, 2 GiB `14 … 513089 frames (2004 MiB)`, 4 GiB `15 … 1037377 frames (4052 MiB)`.
  - TCG (`clock=hpet`, `apic=xapic`): 1 / 2 / 4 GiB in 7 / 7 / 8 s.
- **riscv64 default lane.** `PASS: FAT16 mounted — kernel booted (no disk)`.
- **aarch64 default lane.** `PASS: aarch64 shell prompt reached — full boot successful`.

### VMware matrix (ceiling: VMware local, `x86_64-vm` only)

Run `20261106T022001Z-7ce6b8`: `python3 scripts/x86/test-native-vmware-matrix.py matrix --iso …/final/vicell-x86-vm.iso --jobs 3`, `ELAPSED 584.55 s`. Host: VMware Workstation 25.0.0 build 24995812 (`vmrun version 1.17.0.24995812`), `virtual_hw_version` `22` on all 78 boots. The cell judgment is the one described above.

JSON: [s3-final-vmware-matrix.json](s3-final-vmware-matrix.json). It applies the same reduction as the phase-09 file: 2574 bridge and root-port marker lines are dropped. The source is `build/x86-backend-evidence/vmware-lane/20261106T022001Z-7ce6b8/summary.json`, sha256 `8ceabf1ca534f4e9751f16aa48b02afdf834d13eb1e73218616590ed8346f8d9`, retained with every serial log, VMX and `vmware.log`.

Cleanup: `cleanup_confirmed: true` ×39. Afterwards `vmrun -T ws list` → `Total running VMs: 0`, and `C:\Temp\cellos-vmware\s3-lane\` is empty.

Totals: status, prompt, network, persistence and poweroff are each **PASS 39 / FAIL 0**. All 78 boots have `dhcp_lease`, `guest_poweroff: true` and `hard_stop: false`. Each entry gives the time to the prompt (boot 1 / boot 2).

| NIC | Storage | 2 vCPU BIOS | 2 vCPU EFI | 4 vCPU BIOS | 4 vCPU EFI |
|---|---|---|---|---|---|
| e1000 | sata | PASS (6.2 / 3.3 s) | PASS (6.1 / 4.6 s) | PASS (6.5 / 4.2 s) | PASS (7.3 / 4.6 s) |
| e1000 | nvme | PASS (3.9 / 3.4 s) | PASS (4.0 / 4.1 s) | PASS (4.6 / 4.1 s) | PASS (4.9 / 5.9 s) |
| e1000 | pvscsi | PASS (4.0 / 4.2 s) | PASS (4.8 / 4.3 s) | PASS (4.7 / 4.0 s) | PASS (5.0 / 6.1 s) |
| e1000e | sata | PASS (4.0 / 4.3 s) | PASS (5.2 / 4.2 s) | PASS (6.6 / 4.1 s) | PASS (4.7 / 6.8 s) |
| e1000e | nvme | PASS (6.2 / 3.3 s) | PASS (5.6 / 3.7 s) | PASS (4.4 / 3.9 s) | PASS (4.2 / 4.2 s) |
| e1000e | pvscsi | PASS (4.0 / 3.5 s) | PASS (5.5 / 5.9 s) | PASS (6.2 / 5.9 s) | PASS (6.6 / 6.8 s) |
| vmxnet3 | sata | PASS (5.7 / 5.1 s) | PASS (5.4 / 5.7 s) | PASS (6.2 / 5.5 s) | PASS (6.2 / 6.3 s) |
| vmxnet3 | nvme | PASS (5.3 / 4.8 s) | PASS (5.6 / 5.3 s) | PASS (5.9 / 5.5 s) | PASS (6.5 / 6.3 s) |
| vmxnet3 | pvscsi | PASS (5.7 / 4.6 s) | PASS (5.5 / 5.4 s) | PASS (6.3 / 5.5 s) | PASS (6.4 / 6.5 s) |

| `pciBridge0` cell | Result | Decisive lines |
|---|---|---|
| `bridge0-sata-e1000-c2-bios` (5.6 / 4.9 s) | PASS | `platform-reg 02:00.0 vendor=8086 device=100f class=02:00:00 bar0=0xfd5c0000 mem=0xfd5c0000+0x20000`, `platform-reg 02:01.0 vendor=15ad device=07e0 class=01:06:01 bar0=0x0 mem=0xfd5ef000+0x1000`, `[ahci] ABAR claim ok base=0xfd5ef000 len=0x1000`, `[ahci] controller bound bdf=02:01.0`, `block driver registered: tid=5 device=pci-02-01-0 route=0`, `[e1000] controller bound 8086:100f 82545EM … irq=polled reason=request-driven`, `DHCP acquired` |
| `bridge0-nvme-e1000e-c4-efi` (6.5 / 6.7 s) | PASS | `platform-reg 0b:00.0 vendor=15ad device=07f0 class=01:08:02 bar0=0xfe200000 mem=0xfe200000+0x4000`, `block driver registered: tid=4 device=pci-0b-00-0 route=0`, `[platform] registered bus 3 device 0 function 0 class 2:0:0`, `[e1000e] controller bound 8086:10d3 82574L link_up=true … irq=msix rx=request-driven vectors=3`, `DHCP acquired`. Boot 2 also has `platform-reg 03:00.0 vendor=8086 device=10d3 class=02:00:00 bar0=0xfea20000 mem=0xfea20000+0x20000`; see the serial note below |
| `bridge0-pvscsi-vmxnet3-c2-efi` (5.6 / 6.1 s) | PASS | `platform-reg 03:00.0 vendor=15ad device=07c0 class=01:07:00 bar0=0x0 mem=0xfea10000+0x8000`, `platform-reg 0b:00.0 vendor=15ad device=07b0 class=02:00:00 bar0=0xfe213000 mem=0xfe213000+0x1000`, `[pvscsi] pci 03:00.0 15ad:07c0 … irq=msix vectors=1 irq_wakes=0 park_expiries=0 expiry_ready=0 expiry_empty=0`, `[vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=10000Mb/s … irq=msix rx=request-driven vectors=3`, `DHCP acquired` |

Marker counts over the 78 boots (MACs and wake counts elided):
- **NIC:**
  - 26 × `[e1000] controller bound 8086:100f 82545EM mac=… irq=polled reason=request-driven`;
  - 26 × `[e1000e] controller bound 8086:10d3 82574L link_up=true mac=… irq=msix rx=request-driven vectors=3 irq_wakes=0 park_expiries=0`;
  - 26 × `[vmxnet3] controller bound 15ad:07b0 vmxnet3 rev=1 link_up=true speed=10000Mb/s mac=… irq=msix rx=request-driven vectors=3 irq_wakes=0 park_expiries=0`;
  - 78 × `[net] DHCP acquired — IP configured` and `[net] IP address: 192.168.232.128`.
  - No `registration refused`, `unsupported Ethernet identity`, `driver gate`, `[fault]`, panic or `SAFE MODE` line.
- **Storage:**
  - AHCI `controller bound bdf=00:10.0` (16), `00:11.0` (8) and `02:01.0` (2);
  - PVSCSI `pci 02:00.0` (24) and `03:00.0` (2);
  - on every cell, boot 1 has `machine pin: first boot — recorded <id>` and boot 2 `machine pin: match <id>`.
- **S5:**
  - 78 × `[acpi] power: \_S5_ static SLP_TYPa=0 SLP_TYPb=0`;
  - `[power] poweroff: PM1a_CNT 0x444 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0` on 40 BIOS boots and `PM1a_CNT 0x1004 …` on 38 EFI boots.
- **PVSCSI on the real device (D6).** All 26 bind lines read `irq=msix vectors=1 irq_wakes=0..3 park_expiries=0 expiry_ready=0 expiry_empty=0`, and there is no `irq wait expired` line. The D6 expiries remain a QEMU (MSI) observation.

Serial note: some log lines are missing from the serial capture. This is not a verdict input. It is the same in the earlier run: `USER: [platform] registered` appears 5/6/7 times per boot, 9/41/28 boots here against 7/46/25 in `…a3694a`. Lines also go missing within one cell, for example `bridge0-nvme-e1000e-c4-efi` boot 1 lacks the e1000e `platform-reg` line that boot 2 has. Every judged marker was present on every boot.
