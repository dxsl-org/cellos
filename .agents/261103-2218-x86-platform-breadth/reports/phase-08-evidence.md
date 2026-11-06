# Phase 08 evidence — e1000 82545EM and e1000e 82574L

Recorded 2026-11-06 on branch `feat/x86-s3-phase08-e1000` (worktree `/home/dmin/worktrees/cellos-s3-phase08-e1000`, base `1609b8b41`). Ceiling: **QEMU TCG + QEMU/KVM (local)**. Host: WSL2 kernel `6.18.40.1-microsoft-standard-WSL2`, `QEMU emulator version 8.2.2 (Debian 1:8.2.2+ds-0ubuntu1.17)`. No VMware run (the VMware rows belong to the phase-09 matrix, A2). Nothing here is physical NIC qualification.

Images were assembled outside the repository on the dev-key candidate route (`assemble-x86-image.py --profile headless --out-dir /home/dmin/worktrees/s3p08-out/embedded`, `CARGO_TARGET_DIR` outside the repo), the kernel built with `EMBEDDED_OVERRIDE` and the manifest's `kernel_features` (`policy-required`), and the ISOs with `make-iso-ci.sh` (default, `limine-pc.conf`, `limine-vm.conf`). The tracked `kernel/src/embedded-x86_64/*` was not touched (H-S3-12). Image read-back: `published …/kernel_fs.img (12582912 B, sha256 47b3b0243532c96bba4df8684adea5e1103e55bade71ad3e8f31aa032ff62bfd), 30 files`; manifest carries `/bin/e1000` and `/bin/e1000e`.

## Host gates

| Gate | Result |
|---|---|
| `cargo test -p driver-e1000e --target x86_64-unknown-linux-gnu` | `test result: ok. 20 passed` — identity (Driveable/Target/foreign), 82574 `EERD` layout + NVM read + wedged timeout, `MDIC` encoding, autoneg restart against a fake PHY, MDIC error/wedge, legacy TX/RX descriptor offsets, ring length alignment, `IVAR` routing (3/2/1 vectors), interrupt programming (MSI-X vs polled), reset ordering (bus-master quiesce before `CTRL.RST`), MAC from `RAL0/RAH0` with `EERD` fallback and all-zero refusal, DMA authorization ordering |
| `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | `test result: ok. 500 passed`; includes `pcie_ecam::tests::ethernet_family_gate_covers_driver_cells_only` (100E/100F → `/bin/e1000`, 10D3 → `/bin/e1000e`, 10C9/1533 → `/bin/igb`; 1539, 157B, 1536, 10F6, 15B7, 1011, `8086:beef`, `10ec:8168`, `1af4:1042` refused) and `class_triple_serves_only_the_legacy_family` (`ETHERNET_LEGACY_CLASS_IDS == [(0x8086, 0x100E)]`), boot-ceiling and launch-profile tests |
| `cargo test -p cellos-boards` | `test result: ok. 15 passed` (pc and vm declare `EthernetE1000e`) |
| `python3 -m unittest scripts/test_assemble_x86_image.py` | `Ran 40 tests … OK` |
| `python3 scripts/cellos-sign --check --strict` | `OK: F1 — 140 crates and 1034 files scanned; unsafe confined to 77 allowlisted files` |
| `bash scripts/check-multi-source-wait-law1-digests.sh` | `PASS: multi-source WaitCompletion Law-1 items intact` |
| `tools/mutable-globals --check` | base before edits `48 problems`; after `48 problems` (no new static) |
| clippy `-D warnings` | `driver-e1000`, `driver-e1000e` on `riscv64gc-unknown-none-elf` and `x86_64-unknown-none`; `driver-e1000e --tests` on host; `cellos-kernel` on `x86_64-unknown-none` — all clean |
| `cargo fmt --all --check` | no diff in phase-08 files (pre-existing diffs elsewhere: supervisor, acpi, sys-tools …) |
| `libs/api`, `libs/types`, `cells/services/**`, `kernel/src/task/syscall.rs` | untouched (`git status`) |

## QEMU `nic-x86` (6 lanes)

`VICELL_NIC_ISO=…/vicell-x86.iso cargo test --test nic-x86 -- --test-threads=2` — TCG: `test result: ok. 6 passed … finished in 29.27s`; KVM (`CELLOS_X86_ACCEL=kvm`): `test result: ok. 6 passed … finished in 10.03s`. Each lane asserts bind < registration < TX < RX < DHCP, exactly one NIC cell bound (`[e1000]`/`[e1000e]`/`[igb] controller bound`, `[virtio-net] ready` counted), exactly one `[driver_cell] NIC driver registered`, no `driver gate closed`. Decisive TCG lines (as printed by the test):

```
[nic-x86 e1000] [e1000] controller bound 8086:100e 82540EM mac=52:54:00:12:34:56 irq=polled reason=request-driven
[nic-x86 e1000] [net-bridge] first e1000 TX accepted=true
[nic-x86 e1000] [net-bridge] first e1000 RX len=590
[nic-x86 e1000] [net] DHCP acquired — IP configured
[nic-x86 e1000-82545em] [vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit=0xfed90000
[nic-x86 e1000-82545em] [e1000] controller bound 8086:100f 82545EM mac=52:54:00:12:34:56 irq=polled reason=request-driven
[nic-x86 e1000-82545em] [net] DHCP acquired — IP configured
[nic-x86 e1000e] [vtd] Intel VT-d: DMA isolation ACTIVE (per-Cell domains, Sv39 SLPT) unit=0xfed90000
[nic-x86 e1000e] [e1000e] controller bound 8086:10d3 82574L link_up=true mac=52:54:00:12:34:56 irq=msix rx=request-driven vectors=3 irq_wakes=1 park_expiries=3
[nic-x86 e1000e] [driver_cell] NIC driver registered: tid=11
[nic-x86 e1000e] [net-bridge] first e1000 TX accepted=true
[nic-x86 e1000e] [net-bridge] first e1000 RX len=590
[nic-x86 e1000e] [net] DHCP acquired — IP configured
```

KVM e1000e: `… irq=msix rx=request-driven vectors=3 irq_wakes=1 park_expiries=1`. Kernel side of 435: `[ INFO] [msi] 00:02.0 tid=11 kind=msix irq=0x30+3`.

## `scripts/qemu-x86_64-test.sh` (default boot + explicit models)

TCG (`BOOT_WINDOW=90`) and KVM (`X86_ACCEL=kvm X86_CPU_MODEL=host`, `BOOT_WINDOW=60`): all five runs `PASS: x86_64 shell prompt reached`.

| Model | Decisive lines |
|---|---|
| none (q35 default NIC, H-S3-7) | TCG `[e1000e] controller bound 8086:10d3 82574L … irq=msix rx=request-driven vectors=3 irq_wakes=1 park_expiries=3`, `[net] DHCP acquired — IP configured`; KVM `… irq_wakes=1 park_expiries=0`, `[net] IP address: 10.0.2.15` |
| `e1000` | `[e1000] controller bound 8086:100e 82540EM … irq=polled reason=request-driven`, `[net] IP address: 10.0.2.15` |
| `e1000-82545em` | `[e1000] controller bound 8086:100f 82545EM … irq=polled reason=request-driven`, `[net] IP address: 10.0.2.15` |
| `e1000e` | `[e1000e] controller bound 8086:10d3 82574L … irq=msix rx=request-driven vectors=3 irq_wakes=1 park_expiries=4` (TCG) / `park_expiries=0` (KVM), `[net] IP address: 10.0.2.15` |
| `igb` | `[igb] controller bound 8086:10c9 82576 (QEMU model)`, `[net] IP address: 10.0.2.15` |

## Regression (TCG, same ISOs)

| Suite | Result |
|---|---|
| `x86_64-boot` | 10/10: 8 passed on the first run; the two tier-2 refusal tests first stopped in `ci_guard` (witness ISO missing, `CI=true` in this shell), then `ok. 2 passed` after `bash scripts/build-x86_64-prod-refusal-ci.sh` built `build/vicell-x86-prod-refusal.iso` (that image carries the new exact-ID `/bin/e1000`) |
| `-smp 4` (`CELLOS_X86_SMP=4 … x86_smp_brings_every_cpu_online`) | `ok. 1 passed` |
| `igb-x86` (`VICELL_IGB_ISO`, `VICELL_IGB_VTD_ISO`) | `ok. 2 passed` |
| `virtio-x86` (default, pc, vm ISOs) | `ok. 4 passed` |
| `ahci-x86` | `ok. 15 passed` |
| `xhci-x86` | `ok. 4 passed` after the lane's `-nic none` (see deviation D6); before it, `xhci_injected_key_decodes_in_cell_x86` failed on its "no NIC device" oracle because `/bin/e1000e` now binds q35's default NIC |
| `iommu-dmar-x86` | `ok. 3 passed` |
| `machine-pin-x86` | `ok. 5 passed` |
| `serial-x86` | `ok. 1 passed` |
| `power-x86` | `ok. 3 passed` |

## Not run / open

- VMware rows (A2): phase 09 matrix.
- `cargo test -p driver-e1000e` now runs in CI's host-test step (`cargo test -p driver-nvme -p app-init -p driver-e1000e`), approved by the integrator 2026-11-06 (phase file D7).
- `tests/integration/tests/driver-registration-contract.rs::virtio_drivers_publish_ready_state_and_fail_closed` fails at base (`virtio-blk must check registration result`); files untouched by this phase.
