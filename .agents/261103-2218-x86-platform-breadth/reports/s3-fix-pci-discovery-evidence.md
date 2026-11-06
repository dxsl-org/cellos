# S3 fix evidence: S1 + S2 (PCI discovery behind bridges)

Branch `feat/x86-s3-fix-pci-discovery` from the V0 branch (`b91dcfb81`). Grant: [s3-v0-findings-grant.md](../s3-v0-findings-grant.md), rows S1 and S2. No `libs/api`, `libs/types` or `libs/ostd` byte changed. No Law-1 delta. `SystemPower = 436` untouched.

**Ceiling.** QEMU q35 (TCG and KVM) on this WSL2 host. VMware Workstation 25.0.0 local lane (same host as V0). Not CI, not ESXi, not physical hardware.

**Verdict: S1 and S2 fixed.**
- With `pciBridge0` present, AHCI on `02:00.0` claims its ABAR. BIOS and EFI cells boot to the prompt and pass two-boot persistence. NVMe behind the bridge topology passes too.
- Ethernet functions behind bridges and root ports now pass through the kernel's family gate, the one Ethernet ID table:
  - With phase 08 merged, the VMware e1000e (`8086:10d3`, behind a root port) and e1000 (`8086:100f`, behind `pciBridge0`) register, bind and acquire DHCP.
  - A function the gate refuses is named by ID.

## Root cause

### S2 — zero ABAR behind `pciBridge0`

| Evidence | Line |
|---|---|
| V0 `bridge0-sata` (run `20261106T000356Z-87620f`) | `[pcie] ECAM fallback scan bus 0 …` (the kernel scans bus 0 only) → `[pcie] platform-reg 02:00.0 class=01:06:01 bar0=0x0` → `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)` → `Init: SAFE MODE …` |
| QEMU repro on the unfixed tree (`q35,sata=off` + `pci-bridge` + `ich9-ahci` at `01:01.0`) | `[pcie] platform-reg 01:01.0 class=01:06:01 bar0=0x0` → `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)` |

Mechanism:
- A function on bus ≥ 1 enters `PCI_DEVICES` only through `register_device` (syscall 236). That function stored a stub with only the BAR0 the Platform Cell passed (`bars[0]`), and set `msix`/`pm` to `None`.
- AHCI decodes its ABAR at BAR5, both on VMware and on ICH9. `offer_pcie_device` takes the first memory BAR from `dev.bars`, finds none, and falls back to BAR0: base `0`, length the `0x4000` default. The AHCI cell clamps that length to `AHCI_BAR_LEN = 0x2000`, which is exactly the V0 line.
- `syscall.rs` already noted this gap: "The Platform-Cell registration path stores only BAR0 today".

Ruled out:
- **Bridge window or BAR programming.** After the fix, the same VMware VM claims `ABAR claim ok base=0xfd5ff000 len=0x1000` (BIOS) and `base=0xfc010000` (EFI) on `02:00.0`, and IDENTIFY succeeds. The firmware had programmed the window and BAR5 correctly.
- **ECAM range.** The Platform Cell scans buses `0-127` (V0 log) and registers `02:00.0`.
- **Conventional PCI bridge vs PCIe root port.** Neither path treats the two differently. NVMe behind a root port (BAR0) always worked, and AHCI fails only because its BAR is not BAR0.

### S1 — Ethernet behind root ports

- `cells/services/platform/src/scan.rs:222-229` skipped every `02:00:00` function except `8086:100E`, before registration, and printed no ID.
- The kernel's family gate (`ETHERNET_DRIVER_IDS` / `ethernet_id_has_driver`, `pcie_ecam.rs`) names refusals only in the bus-0 scan.
- QEMU repro on the unfixed tree: a `pcnet` behind root port 2 printed only `[platform] unsupported Ethernet identity — registration skipped`, without its ID.

## Fix

| File | Change |
|---|---|
| `kernel/src/task/drivers/pcie_ecam.rs` | `decode_function` (BARs of type-0 headers, plus MSI-X/PM) is now shared by `scan` and by the new `decode_function_in_window`, which reads one function through its bus's window in the admitted range. `register_device` (x86) does the following in order: (1) re-reads the identity; (2) refuses an Ethernet function that `ethernet_id_has_driver` does not admit (`platform_registration_admits`) and returns `false`; (3) skips duplicates as before; (4) only for a new BDF, decodes every BAR and capability exactly as the bus-0 scan does, falling back to the cell's BAR0 when no function answers. `first_memory_bar` is now one helper. The `platform-reg` log line adds `mem=<base>+<len>`. Three host tests were added. |
| `kernel/src/task/syscall.rs` | 236 returns `NotSupported` when `register_device` refuses (the function is not recorded); the existing error path is unchanged. `offer_pcie_device` uses `first_memory_bar`. The stale "stores only BAR0" note is gone. |
| `cells/services/platform/src/scan.rs` | The hand-kept `100E` filter is removed. Every function is offered to the kernel, and a refusal prints `[platform] registration refused bus B device D function F id vvvv:dddd class C:S:P`. |
| `tests/integration/src/lib.rs`, `tests/integration/tests/pcie-multibus-x86.rs` | New `boot_x86_bios_with_bridged_devices` (VMware-shaped q35: `sata=off`, `ich9-ahci` behind a `pci-bridge` carrying the boot CD, `e1000` behind root port 1, `pcnet` behind root port 2) and the test `bridged_ahci_and_root_port_nics_follow_the_scan_rules`. |

**Single table and sync.** The cell keeps no Ethernet list. The kernel table cannot be reached from a cell as a crate: the cell depends only on `types`/`api`/`ostd`, all stop-rule libs. It is reached through registration instead: the kernel applies `ethernet_id_has_driver` to the identity it reads itself. Phases 08 and 09 add rows to `ETHERNET_DRIVER_IDS` and change nothing else. The platform path follows automatically, and no second copy needs to stay in sync. The host test pins this: for `100F`/`10D3`/`07B0`, the gate and the registration path agree whatever the table holds. That test passes on the phase-08 merge too.

**Merge.** `git merge-tree` against `feat/x86-s3-phase08-e1000` is clean, and so are fix-efi-memory, fix-s5-scan and v0. Against `feat/x86-s3-phase09-vmxnet3-pvscsi`, the code files (`pcie_ecam.rs`, `lib.rs`) auto-merge. The one conflict is `phase-09-vmxnet3-pvscsi.md`, which comes from the V0 branch's doc commits and is not a file this branch touches.

**Deviations (for the integrator).**
- **D-S1.** S1's added scope names only `scan.rs`. The single-table rule needed the gate decision in `pcie_ecam.rs::register_device` and a refusal result from 236 (`syscall.rs`). Both files are in S2's added scope. This is kernel policy, not wire: same syscall, same args, an existing error.
- **D-S1b.** A refused function's BAR windows are still registered, because the cell registers BARs before the device so that no Driver Cell can find a device whose windows are not yet registered. This matches the bus-0 scan, whose `init` registers windows for gated functions too. A window gives nothing without the 418/424 hand-out, which stays gated.

## Checks

| Check | Result | Decisive lines |
|---|---|---|
| Host tests `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | PASS | `503 passed`; new `platform_registration_gates_ethernet_by_the_family_table`, `registered_function_keeps_every_bar_of_its_bus`, `first_memory_bar_skips_empty_and_io_slots` all `ok`. On the 08 merge: `pcie_ecam` `13 passed` |
| `cargo test -p service-platform` (host) | PASS | `9 passed` |
| clippy `-D warnings` | PASS | `-p cellos-kernel -p service-platform` on x86_64-unknown-none and riscv64gc-unknown-none-elf; `-p cellos-kernel` on aarch64-unknown-none-softfloat |
| `tools/mutable-globals --check` | 48 = base | `640 declarations, 255 need review, 48 problems`; no new static |
| QEMU repro (manual, TCG) | unfixed → fixed | unfixed: `platform-reg 01:01.0 … bar0=0x0`, `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)`, `unsupported Ethernet identity — registration skipped`. Fixed: `platform-reg 01:01.0 class=01:06:01 bar0=0x0 mem=0xfe800000+0x1000`, `[ahci] ABAR claim ok base=0xfe800000`, `[ahci] controller bound bdf=01:01.0`, `[ahci] irq=msi`, `IDENTIFY DEVICE ok`, `registered bus 2 device 0 function 0 class 2:0:0` + `NIC driver registered` + `DHCP acquired`, `registration refused bus 3 device 0 function 0 id 1022:2000 class 2:0:0` |
| New test on the unfixed ISO | FAIL (as expected) | `"[platform] registration refused bus 3 … id 1022:2000 …" not seen` |
| QEMU TCG: x86_64-boot / ahci-x86 / nvme-x86 / pcie-multibus-x86 / iommu-dmar-x86 | PASS | `10` / `15` / `11` / `3` / `3 passed`. The x86_64-boot tier-2 pair needs `build/vicell-x86-prod-refusal.iso` (built with `scripts/build-x86_64-prod-refusal-ci.sh`); without it, `CI=true` turns the skip into a failure. The AHCI/NVMe fault-ISO tests skip (`VICELL_AHCI_FAULT_ISO` unset), as before |
| QEMU KVM (`CELLOS_X86_ACCEL=kvm`): same five suites | PASS | same counts |
| Phase-08 merge (scratch, removed): nic-x86 / pcie-multibus-x86 / ahci-x86 TCG, nic-x86 / pcie-multibus-x86 KVM | PASS | `6` / `3` / `15`; `6` / `3 passed` |

## VMware (lane tooling, cells created under `C:\Temp\cellos-vmware\s3-lane`, deleted after)

Cells were run through `test-native-vmware-matrix.py`'s `lane_main` with replaced scenarios (driver in `/tmp`, not committed). Every cell reports `cleanup_confirmed: true`, and the lane root is empty afterwards.

Run `20261106T004857Z-57b103`, branch ISO `vicell-x86-vm.iso` sha256 `8c3cc597…14ce`:

| Cell (`pciBridge0` present) | Result | Decisive lines |
|---|---|---|
| `bridge0-bios-sata-c2`, 2 boots | PASS, persistence PASS | `[pcie] platform-reg 02:00.0 class=01:06:01 bar0=0x0 mem=0xfd5ff000+0x1000`, `[ahci] ABAR claim ok base=0xfd5ff000 len=0x1000`, `[ahci] controller bound bdf=02:00.0`, `IDENTIFY DEVICE ok … model="VMware Virtual SATA Hard Drive"`, `block driver registered: tid=5 device=pci-02-00-0`, boot 1 `machine pin: first boot — recorded 37adad260e0b`, boot 2 `machine pin: match 37adad260e0b` + nonce read back in guest and on host |
| `bridge0-efi-sata-c2` (512 MiB), 2 boots | PASS, persistence PASS | `mem=0xfc010000+0x1000`, `ABAR claim ok base=0xfc010000`, `machine pin: match 12424417cd62`, nonce read back |
| `bridge0-bios-nvme-c4`, 2 boots | PASS, persistence PASS | `platform-reg 03:00.0 class=01:08:02 bar0=0xfd5fc000 mem=0xfd5fc000+0x4000`, `block driver registered: tid=4 device=pci-03-00-0`, `machine pin: match 23377a828003` |

Runs `20261106T005305Z-5cee8f` and `20261106T005658Z-cbfc7a`. The ISO is this branch merged with `feat/x86-s3-phase08-e1000` (scratch merge `63c7b832b`, not kept), `vicell-x86-vm.iso` sha256 `0a38a6c4…38d9`:

| Cell | Result | Decisive lines |
|---|---|---|
| `nic-e1000e` (root ports, no `pciBridge0`) | PASS | `platform-reg 02:00.0 class=02:00:00 bar0=0xfdfa0000 mem=0xfdfa0000+0x20000`, `[platform] registered bus 2 device 0 function 0 class 2:0:0`, `[e1000e] controller bound 8086:10d3 82574L link_up=true … irq=msix … vectors=3`, `NIC driver registered`, `[net] DHCP acquired — IP configured` |
| `bridge0-nic-e1000e` | PASS | `registered bus 3 device 0 function 0 class 2:0:0`, `[e1000e] controller bound 8086:10d3 … irq=msix … vectors=3`, `DHCP acquired` |
| `bridge0-nic-e1000-b2` (82545EM behind `pciBridge0`, 2 boots) | PASS, persistence PASS | both boots: `platform-reg 02:00.0 class=02:00:00 … mem=0xfd5c0000+0x20000`, `registered bus 2 device 0 function 0 class 2:0:0`, `[e1000] controller bound 8086:100f 82545EM`, `first e1000 RX`, `DHCP acquired`. The one-boot `bridge0-nic-e1000` cell (PASS) bound and passed traffic (`first e1000 RX len=62`), but the lane stopped it before the lease line |
| `nic-e1000` (no `pciBridge0`; e1000 on bus 0, kernel scan) | PASS | `[e1000] controller bound 8086:100f`, `DHCP acquired` |
| `bridge0-nic-vmxnet3` (no vmxnet3 cell before phase 09) | PASS | `[platform] registration refused bus 3 device 0 function 0 id 15ad:07b0 class 2:0:0` |

## Open risks

- Under VMware, the AHCI at `02:00.0` keeps the registration-time decode only. A firmware that leaves a bridged BAR unassigned would still give `base=0`, and the AHCI cell fails closed as before.
- `[INFERENCE]` The e1000e MSI-X result behind a root port (`vectors=3`) also depends on this fix: the MSI-X table lives in BAR3, and the carve check needs that BAR's length. No pre-fix VMware run on the phase-08 merge exists to compare against.
- The QEMU and VMware results are QEMU and VMware-local evidence only.
