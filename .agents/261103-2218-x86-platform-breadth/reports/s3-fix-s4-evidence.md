# S3 fix S4 evidence: static `\_S5_` scan on VMware's EFI DSDT

Branch `feat/x86-s3-fix-s5-scan` from `464e168a8` (V0 branch). Fix commit `b8b111156`. Scope ([grant](../s3-v0-findings-grant.md), S4): `kernel/src/acpi/power.rs` and its fixtures/tests. No `libs/api` or `libs/types` byte changed; no Law-1 delta. `SystemPower = 436` untouched.

**Ceilings:** host; QEMU 8.2.2 TCG (`qemu64,+pdpe1gb`) and local KVM (`CELLOS_X86_ACCEL=kvm`); VMware local (Workstation 25.0.0 build 24995812, virtual hardware 22, Windows host with Hyper-V, driven from WSL2). Not CI, ESXi or physical hardware.

**Verdict: S4 fixed.** VMware EFI now reads `\_S5_ static SLP_TYPa=0 SLP_TYPb=0`, and `/bin/shutdown` powers the VM off through S5 on both boots of `efi-sata-c2`.

## Root cause

The defect is not the ExtOp term itself. The scan already decodes `OperationRegion`, `Field`, `Device`, `Processor`, `Mutex` and the other ExtOp namespace terms. The term at DSDT offset `0x1f59` is:

```
5b 80 'EICH' 00 | 72 'ECFG' 0b 0040 00 | 0b 0040
OperationRegion (EICH, SystemMemory, (ECFG + 0x4000), 0x4000)   // in \_SB.PCI0.LPC.HPET (Device)
```

Its RegionOffset is an `AddOp` (`0x72`) expression with a `NullName` target. `skip_term` accepted only data objects and names, so `OP_REGION_OP` failed and the scan reported the term's first byte, `Unparsed { offset: 8025 (0x1f59), opcode: 91 (0x5b) }`. That is the exact V0 line. On the unfixed scanner, the new fixture test fails with `left: Unparsed { offset: 8025, opcode: 91 }` (checked with `power.rs` stashed). `\_S5_` is a root `Name (_S5, Package (0x02) { Zero, Zero })` near the end of the table (iasl line 11568), so it was never reached.

## Fix

`skip_term` now also accepts integer expressions as TermArgs. The accepted ops are `Add`, `Subtract`, `Multiply`, `ShiftLeft`, `ShiftRight`, `And`, `Nand`, `Or`, `Nor`, `Xor` and `Mod` (two operands), and `Not` (one operand). Each operand is a TermArg, and the expression's target must be `NullName` (`0x00`). An expression that stores its result while the table loads is still reported unparsed, so loading cannot rewrite `\_S5_` without the scan noticing. Recursion is bounded by `MAX_NEST`. The `\_S5_` rules are unchanged: root depth only, two 3-bit integer constants, and method, in-body or computed → refuse. Nothing else needed decoding: after this change, both VMware DSDTs walk to the root `\_S5_`.

## Fixtures (`tests/fixtures/acpi/`)

| File | Source | Bytes | sha256 | Scan |
|---|---|---|---|---|
| `vmware-efi-dsdt.aml` | lane VM, EFI, 2 vCPU, SATA, 512 MiB | 37954 | `f0e56768…6b01f0` | `Static { 0, 0 }` |
| `vmware-bios-dsdt.aml` | lane VM, BIOS, 2 vCPU, SATA, 1024 MiB | 66014 | `0698973d…a3c318` | `Static { 0, 0 }` |

How they were captured (no kernel code involved): a throwaway script outside the repo built the lane VM with `make_vmx`/`make_lane_disk` under `C:\Temp\cellos-vmware\s3-lane\` plus `mainMem.useNamedFile = "TRUE"`. It booted the V0 ISO to `Cellos >`, ran `vmrun suspend … hard`, and copied the `.vmem`. It then followed RSDP → XSDT → FADT → `X_DSDT`, with every checksum valid. The EFI RSDP was at `0xfbf1000` and the DSDT at `0xfbf1160`; the BIOS RSDP at `0xf6a00` and the DSDT at `0x3feeec95`. Both are OEM `PTLTD `/`Custom  `. Neither firmware lists an SSDT. Each VM was then stopped, deleted and its directory removed (`cleaned: true`). The serial log of the EFI capture shows the defect: `[acpi] power: \_S5_ scan stopped at opcode 0x5b offset 0x1f59; poweroff refused`.

## Checks

| Check | Result | Decisive lines |
|---|---|---|
| S5 host tests | 4/4 | `s5_scan_reads_the_q35_dsdt` (Static{0,0}), `s5_scan_refuses_a_name_created_inside_a_method` (NotStatic), `s5_scan_reads_the_vmware_dsdts` (EFI and BIOS Static{0,0}), `s5_scan_accepts_only_a_root_name_with_constant_slp_types` (adds: `OperationRegion` with `Add(…, NullName)` → Static{5,7}; with target `\_S5_` → `Unparsed { offset: 36, opcode: 0x5B }`) |
| `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | pass | `test result: ok. 501 passed; 0 failed` |
| clippy `-D warnings`, `-p cellos-kernel`, x86_64-unknown-none / riscv64gc-unknown-none-elf / aarch64-unknown-none-softfloat | exit 0 ×3 | — |
| `tools/mutable-globals --check` | unchanged | `640 declarations, 255 need review, 48 problems` |
| power-x86 TCG | 3/3 | `test result: ok. 3 passed` (7.69 s); `[power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0`; `[acpi] power button: poweroff request relayed to supervisor tid 15`; `/bin/shutdown`, `power button`, `second boot /bin/shutdown`: `QEMU exit … (0)` |
| power-x86 KVM | 3/3 | same lines; `test result: ok. 3 passed` (5.45 s) |
| x86_64-boot TCG / KVM (`CI=true`, prod-refusal witness built) | 10/10 / 10/10 | includes `x86_tier2_admission_is_refused_by_fleet_profile ... ok`, `x86_smp_brings_every_cpu_online ... ok` |
| VMware `efi-sata-c2` (`lane --firmware efi --storage sata --cpus 2 --boots 2 --memory-mib 512`, run `20261106T003902Z-1e6fce`) | PASS | both boots `[acpi] power: \_S5_ static SLP_TYPa=0 SLP_TYPb=0`, `[supervisor] power: poweroff (Operator) froze=5 refused=1 kept=2`, `[power] poweroff: PM1a_CNT 0x444 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0`; `vmware.log`: `ConfigDB: Setting softPowerOff = "TRUE"`, `Transitioned vmx/execState/val to poweredOff`; `guest_poweroff: true`, `hard_stop: false` on both boots; persistence PASS; `cleanup_confirmed: true` |
| VMware `bios-sata-c2` (regression, run `20261106T004232Z-6ae7b9`) | PASS | `\_S5_ static SLP_TYPa=0 SLP_TYPb=0`, `PM1a_CNT 0x1004`; `guest_poweroff: true`, `hard_stop: false` on both boots |
| VMware power button | not available | The lane tool has no power-button action. A throwaway probe ran `vmrun stop soft` at the prompt (EFI). It blocked for 120 s on VMware Tools (`Tools: [AppStatus] Last heartbeat value 3 (last received 120s ago)`), and the guest logged no `[acpi] power button` line. VMware's soft stop goes through Tools, not an ACPI power-button event. The probe VM was hard-stopped and deleted (`cleaned: true`). The power-button path stays QEMU-only evidence (power-x86) |

Images: `assemble-x86-image.py --profile headless`, kernel `--features policy-required` (sha256 `dc670bc7…6285e`), `make-iso-ci.sh` default (`3fb1b6d7…187d02`), `limine-vm.conf` (`32151bf1…d402db`), `limine-pc.conf` (`d926f40c…2eb564`), and `build-x86_64-prod-refusal-ci.sh` (`99eb2870…8b4b8d`). The regenerated `kernel/src/embedded-x86_64/*` were restored and are not committed. No VM is left running (`Total running VMs: 0`), and the lane directory holds no S4 VM.

## Deviation

Recorded here instead of in `s3-v0-vmware-tooling-evidence.md`, so parallel S3 fixes do not conflict; the integrator updates the phase file. The capture and probe scripts are throwaway and stay out of the repo.

## Open risks

- A name used as a TermArg is still taken as a plain reference (unchanged from phase 06). A namespace-level method invocation with arguments would desynchronise the walk, and the argument bytes would then be decoded as terms. That normally ends in `Unparsed`. Neither VMware DSDT nor q35 has such an invocation.
- Other firmware may use expression ops outside the accepted set (e.g. `Divide`, `Concatenate`, `ToInteger`). They are reported `Unparsed` and poweroff is refused, as before.
- The EFI DSDT embeds memory-size-dependent addresses (`OEMD` at `0x0DF72040`). The fixture is one configuration; the structure is the same at other sizes `[INFERENCE]`.
