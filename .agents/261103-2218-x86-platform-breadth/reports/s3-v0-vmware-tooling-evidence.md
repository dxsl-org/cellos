# S3 V0 evidence: VMware local lane tooling smoke

Branch `feat/x86-s3-v0-vmware-tooling` from `1609b8b41`. Scripts only; no kernel, HAL, `libs/api` or `libs/types` byte changed. No Law-1 delta.

**Ceiling: VMware local.** VMware Workstation 25.0.0 build 24995812 on a Windows host with Hyper-V present (`Monitor Mode: ULM` in every `vmware.log`), driven from WSL2. Not CI, ESXi or physical hardware. Never promoted.

**Verdict: V0 passes.**
- The `x86_64-vm` ISO boots to `Cellos >` under BIOS and EFI.
- The relay captures the guest serial and types commands into it.
- Two-boot persistence passes on SATA (AHCI) and on NVMe, with 2 and 4 vCPUs, under BIOS and EFI.
- Each VM was powered off and deleted.
- The run also found five Surprises (S1–S5). They are outside the V0 file set and are reported below for phases 08/09 and their owners.

## Tooling (request §2, phase-09 VMware rows)

| File | Change |
|---|---|
| `scripts/x86/vmware_serial_relay.py` (new) | Runs under Windows `py -3` (3.14.3), standard library only (ctypes `kernel32`). Connects as the client of `\\.\pipe\cellos-<run>` (VMware is the server). Guest bytes go to stdout; stdin is typed into the guest; events go to stderr (`relay: connected …`, `relay: closed …`). The reader never blocks in `ReadFile`: it reads only what `PeekNamedPipe` reports, so writes on the same synchronous handle always get through. It exits with `os._exit` once the pipe closes. |
| `scripts/x86/vmware-boot-test.py` | `make_vmx` takes `firmware` (`bios`/`efi`, Secure Boot off), `nic` (`e1000`/`e1000e`/`vmxnet3`, NAT), `storage` (`sata`/`nvme`/`pvscsi`) with a `disk`, `pipe` (server-end named pipe, `tryNoRxLoss`), `viommu` (`vvtd.enable`), `legacy_bridge` (`pciBridge0`) and `hw_version`. One pinned `HW_VERSION = "22"` replaces 10/14. PCIe root ports `pciBridge4-7` are always present. The pipe name is validated by an exact regex because a VMX pipe path needs backslashes, which `vmx_string` rejects. `--cpus` accepts 1, 2, 4. The stale HPET comment is replaced. |
| `scripts/x86/test-native-vmware-matrix.py` | Subcommands `fault` (the default; the existing four AHCI scenarios, now built by the shared `make_vmx`), `lane` (one NIC × storage × vCPU × firmware cell) and `v0`. VMs live only in `C:\Temp\cellos-vmware\s3-lane\<run>-<cell>`. Each run: disk = `machine_pin_disk.py` (P1 FAT32 + blank P5 `/srv`) → `qemu-img` VMDK → `vmware-vdiskmanager.exe -r … -t 0`; ISO copy; relay started before `vmrun start … nogui`; prompt wait; typed `uname -a` (relay-input proof); boot 1 `vwrite`+`vcat` of a nonce; `/bin/shutdown`, with a hard stop as fallback; boot 2 `vcat`; host read-back of the nonce with `qemu-img` + `mtype`; `vmrun deleteVM` + directory removal + `vmrun list` check. JSON evidence: per-cell `result.json` and a run `summary.json`. Persistence is `NOT-RUN` whenever relay input fails. |
| `scripts/x86/test_native_vmware_matrix.py` | 22 new unit tests, synthetic and not runtime evidence: VMX for every {nic × storage × 2/4 vCPU × firmware} cell, pipe-name refusal, root ports and `pciBridge0`, persistence `NOT-RUN` on failed relay input, guest and host read-back both required, boot markers, cleanup, probe verdict, post-deadline marker, relay framing over a fake `kernel32` (partial writes). `Ran 31 tests … OK`. |

Run: `python3 scripts/x86/test-native-vmware-matrix.py v0`. Inputs: `build/vicell-x86-vm.iso` (`make-iso-ci.sh` with `limine-vm.conf`, sha256 `07718858…6e9658`) and `build/vicell-x86-pc.iso` (`limine-pc.conf`, `724e5228…a18e4`). Both use one kernel built from a fresh headless assembly. The regenerated `kernel/src/embedded-x86_64/*` were restored (H-S3-12).

## Acceptance (run `20261106T000356Z-87620f`, [summary](s3-v0-vmware-smoke.json))

| Check | Result | Decisive lines |
|---|---|---|
| ISO boots to `Cellos >` (BIOS) | PASS | `bios-sata-c2` boot 1 prompt at 5.4 s; `[x86-gate] board=x86_64-vm soc-profile=x86_64-vm source=cmdline dma_isolation=Optional` |
| ISO boots to `Cellos >` (EFI) | PASS at 512 MiB (S3) | `efi-sata-c2` boot 1 prompt at 5.8 s |
| Relay types a command and reads its output | PASS (11/11 cells that reached the prompt; `efi-m1024` panics before it) | typed `uname -a` → printed `Cellos cellos-kernel 0.2.1 x86_64` |
| Two-boot persistence, SATA/AHCI, 2 vCPU, BIOS | PASS | boot 1 `Init: machine pin: first boot — recorded 1e3754c4f65d`; boot 2 `Init: machine pin: match 1e3754c4f65d` and `vcat` → `S3LANE-20261106T000356Z-87620f-bios-sata-c2`; host read-back identical |
| Two-boot persistence, SATA/AHCI, 2 vCPU, EFI | PASS | same pattern, nonce `…-efi-sata-c2`, host read-back identical |
| Two-boot persistence, NVMe, 4 vCPU, BIOS / EFI | PASS / PASS | `[driver_cell] block driver registered: tid=4 device=pci-02-00-0 route=0`; `[x86-smp] topology online=4 processor_ids=0;1;2;3 qualified=yes`; nonce read back in guest and on host |
| Disk made with `vmware-vdiskmanager.exe` | PASS | `Virtual disk conversion successful.` |
| VM powered off and deleted | PASS (12/12) | `cleanup_confirmed: true` for every cell; `C:\Temp\cellos-vmware\s3-lane\` empty and `Total running VMs: 0` after the run. BIOS: guest S5 (`guest_poweroff: true`). EFI: hard stop (S4) |
| Pinned VMX hardware version | 22 | `vmware.log`: `DICT virtualHW.version = "22"`, `Log for VMware Workstation … version=25.0.0 build=24995812` |

Earlier runs `20261105T233037Z-e70694` (one cell) and `20261105T233121Z-810f4b` / `20261105T234144Z-dac6f7` (full set) are retained under `build/x86-backend-evidence/vmware-lane/`. They show the topology failures that led to the root-port and `pciBridge0` decisions (D3, S2).

## Host capability facts

| Fact | Result | Evidence |
|---|---|---|
| vCPU 2 | works | `topology online=2 processor_ids=0;1 qualified=yes` (SATA cells) |
| vCPU 4 | works | `topology online=4 processor_ids=0;1;2;3 qualified=yes` (NVMe cells, `nic-vmxnet3`) |
| NIC models VMware offers | `e1000`, `e1000e`, `vmxnet3`: all power on (with root ports) and boot to the prompt | `DICT ethernet0.virtualDev = "e1000"` etc.; cells `nic-*` PASS |
| Cellos today: VMware `e1000` (82545EM) | refused | `[ WARN] [e1000] unsupported Ethernet 8086:100f; driver gate closed` (kernel bus-0 scan, `00:10.0`), then `[platform] unsupported Ethernet identity — registration skipped`, `[net] DHCP: deconfigured` |
| Cellos today: VMware `e1000e`, `vmxnet3` | refused, unbound | only `[platform] unsupported Ethernet identity — registration skipped`. The functions sit behind a PCIe root port (bus ≥ 1), which the kernel's bus-0 scan does not log, and the platform cell prints no ID (S1). The IDs `8086:10d3` / `15ad:07b0` are `[INFERENCE]` from QEMU and VMware documentation, not observed |
| "Virtualize IOMMU" (`vvtd.enable`) | the key is accepted and the VM powers on (BIOS and EFI); VMware exposes an ACPI DMAR, but the remapping unit does not respond | `[acpi] DMAR: units=1 selected base=0xfec10000 include_pci_all=true`; `[acpi] gates: … dmar=true …`; `[vtd] Intel VT-d not present (GCAP=0xffffffffffffffff)` |
| A3 (plan): `x86_64-pc` + EFI + vIOMMU | **false** | `viommu-efi-pc`: `[x86-gate] board=x86_64-pc soc-profile=x86_64-pc source=default dma_isolation=Required`, `[ERROR] [iommu] DMA isolation REQUIRED by board x86_64-pc but no remapper was discovered — refusing untranslated DMA (tid=5 bdf=0x80 …)`, `Init: SAFE MODE — machine pin store unavailable; …`. Under H-S3-9 the matrix runs `x86_64-vm` only, and there is no VMware production-guest claim |

## Decisions (V0)

- **D1** `HW_VERSION = "22"` for every lane VM, recorded from `vmware.log` per boot. The fault scenarios move from 14 to 22 through the shared generator; they were not re-run in V0.
- **D2** Disk: the request (§7.1) names `qemu-img convert`; the V0 brief names `vmware-vdiskmanager.exe`. Both are used. `machine_pin_disk.py` provides a provisioned node (`x86_64-vm` needs the pin store, ADR-0029 §3), `qemu-img` makes a seed VMDK, and `vdiskmanager -r -t 0` writes the VMware-native disk the VM uses. The seed is deleted. Host read-back uses `qemu-img` + `mtype` on that disk.
- **D3** PCIe root ports `pciBridge4-7` are always present. Without them, e1000e, vmxnet3 and NVMe fail at power-on: `[msg.pci.noslotavail] No PCIe slot available for Ethernet0` / `… for nvme0`. One e1000e attempt also crashed `vmware-vmx` with `Exception 0xc0000005 (access violation)`. `pciBridge0` is off by default (`legacy_bridge=False`, see S2) and is kept as a probe.
- **D4** EFI lane cells run with 512 MiB (S3). BIOS cells run with 1024 MiB.
- **D5** Relay-input proof is typed `uname -a`, whose output (`Cellos cellos-kernel …`) cannot come from the echo. The nonce `vcat` must print the nonce as its own line.

## Surprises (outside the V0 file set; stop rule: reported, not fixed)

- **S1 — VMware PCIe NICs are invisible to the S3 discovery path (phases 08/09).** VMware puts e1000e and vmxnet3 behind root ports, on bus ≥ 1. The kernel logs `[pcie] ECAM fallback scan bus 0 @ 0xf0000000 (1 MiB window)`, so the family gate's named refusal (`pcie_ecam.rs`) never sees these functions. Bus ≥ 1 is registered by the platform cell, which skips every class `02:00:00` function except `8086:100E` (`cells/services/platform/src/scan.rs:222-229`). NVMe on bus 2/3 binds, which confirms that the platform path covers bus ≥ 1. Unless the discovery path changes, `/bin/e1000e` and `/bin/vmxnet3` cannot find their VMware devices through 424. `scan.rs` is not in request §2. This needs an owner decision before the phase-08/09 VMware rows can pass. QEMU q35 lanes are unaffected, because every device there is on bus 0. Main was told on 2026-11-06.
- **S2 — AHCI behind VMware's PCI bridge has no usable BAR.** With `pciBridge0.present = "TRUE"` (VMware's own default VMX), SATA moves to `02:00.0` and `[ahci] controller has no usable MMIO BAR (base=0x0 len=0x2000)`. Init then enters `SAFE MODE`. Probe `bridge0-sata` records this. Without `pciBridge0`, AHCI stays at `00:10.0` (`ABAR claim ok base=0xfebff000 len=0x1000`). Owner: AHCI/platform BAR handling, not S3.
- **S3 — EFI with ≥ 1024 MiB panics the kernel on VMware.** `panicked at kernel/src/main.rs:1219:10: frame allocator bitmap and early paging registration must succeed: ()`. EFI 512 MiB boots; EFI 1024/2048/4096 MiB panics; BIOS 1024/2048/4096 MiB boots (runs `20261106T000356Z-87620f/efi-m1024`, `20261106-probe-efi-mem`). The allocator line before the panic is `[boot] allocator: 6 usable range(s), 514423 frames (2009 MiB)`. `[INFERENCE]` `register(bm_start, heap_virt)` (`main.rs:1212-1219`) assumes the frame bitmap lies below the boot heap. Under the larger VMware EFI memory map, the heap run probably lands below the bitmap or overlaps it, which makes `start >= end` or an overlap return `Err(())`. Kernel file; not fixed here.
- **S4 — EFI `\_S5_` is not static on VMware, so guest poweroff is refused.** `[acpi] power: \_S5_ scan stopped at opcode 0x5b offset 0x1f59; poweroff refused` and `[power] poweroff refused: \_S5_ Unparsed { offset: 8025, opcode: 91 }`. BIOS parses `\_S5_ static SLP_TYPa=0 SLP_TYPb=0` and powers off. The lane falls back to a hard stop. Persistence is unaffected because FAT writes are written through. Phase-06 territory.
- **S5 — A3 false.** See the table above: VMware exposes a DMAR whose unit reads all-ones. H-S3-9 applies.

## Gates

- `python3 scripts/x86/test_native_vmware_matrix.py`: `Ran 31 tests … OK`.
- `tools/mutable-globals --check`: base `640 declarations, 255 need review, 48 problems`, recorded before any edit. The branch touches no kernel or HAL file, so the count is unchanged.
- No tracked `kernel/src/embedded*` change; no `libs/api` or `libs/types` change.
