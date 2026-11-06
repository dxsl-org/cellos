# Phase 09 evidence — vmxnet3 and PVSCSI cells (09.1-09.2); VMware matrix pending

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

## VMware matrix (09.3-4) — pending

Not run. Waits for the V0 VMware lane tooling (S3V0VmwareTooling). Note for the matrix: on VMware Workstation 25 e1000e/vmxnet3/NVMe sit behind PCIe root ports, and the bus ≥ 1 probe above shows `/bin/vmxnet3` cannot see such a device until the platform cell's Ethernet filter is decided.
