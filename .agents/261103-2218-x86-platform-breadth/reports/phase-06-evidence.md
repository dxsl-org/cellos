# Phase 06 evidence — ACPI reset, S5, power button, 436

Branch `feat/x86-s2-phase06-power`: `d0e43b534`, `1f2a213bf`, `ab69da8fe`, `de5499c19`, `5a2ba5e3d`. Ceilings: host, QEMU 8.2.2 TCG, local KVM (`-cpu host`). Not physical evidence.
ISOs built from the branch: headless image (`assemble-x86-image.py --profile headless`), kernel with the manifest features, `make-iso-ci.sh` default, `limine-vm.conf` and `limine-pc.conf` variants. Regenerated `kernel/src/embedded-x86_64/**` restored before every commit.

## Host

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `438 passed`, including `s5_scan_reads_the_q35_dsdt`, `s5_scan_refuses_a_name_created_inside_a_method`, `s5_scan_accepts_only_a_root_name_with_constant_slp_types`, `discovery_scans_the_dsdt_then_listed_ssdts_and_reads_gpe_blocks`, `system_power_is_refused_without_supervisor_cap`, `system_power_checks_action_and_arguments_behind_the_gate`.
- `-p api` `194 passed`; `-p ostd` `89 passed`; `-p supervisor` `11 passed`.
- `python3 -m pytest scripts/test_assemble_x86_image.py scripts/test_cellos_sign.py`: `104 passed`.
- clippy `-D warnings`: kernel x86_64-unknown-none and riscv64gc-unknown-none-elf exit 0; `supervisor app-sys-tools api ostd hal-x86` (x86_64-unknown-none) exit 0.
- `cellos-sign --check`: `OK: F5`, `OK: F1 — 139 crates and 998 files scanned`. Law-1 digest scripts: AI `OK`, LookupServiceBound `PASS`, multi-source wait `PASS`.
- `shutdown` cross-builds for riscv64gc-unknown-none-elf and aarch64-unknown-none-softfloat.

## Guest: `tests/integration/tests/power-x86.rs`

TCG `test result: ok. 3 passed` (13.19 s); KVM `test result: ok. 3 passed` (8.60 s). Identical decisive lines on both:

```
[ INFO] [acpi] power: \_S5_ static SLP_TYPa=0 SLP_TYPb=0
[ INFO] [acpi] sci: ACPI mode on, sci_int=9 gsi=9 flags=0xd vector=0x29
[ INFO] [acpi] sci: armed gsi=9 vector=0x29 apic=0 PWRBTN_EN

# /bin/shutdown -r
USER: System rebooting...
USER: [supervisor] power: reboot (Operator) froze=5 refused=1 kept=1 settle<=500ms
USER: [supervisor] power: requesting reboot
[ WARN] [power] reset: FADT reset register space=1 addr=0xcf9 value=0xf
[Cellos] kernel boot v0.2.1            <- second banner; second boot then powers off, exit 0

# /bin/shutdown
USER: System shutting down...
USER: [supervisor] power: poweroff (Operator) froze=5 refused=1 kept=1 settle<=500ms
[ WARN] [power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0
/bin/shutdown: QEMU exit Some(ExitStatus(unix_wait_status(0)))

# QMP system_powerdown
[ WARN] [acpi] power button: poweroff request relayed to supervisor tid 15
USER: [supervisor] power: poweroff (PowerButton) froze=4 refused=1 kept=1 settle<=500ms
[ WARN] [power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0
power button: QEMU exit Some(ExitStatus(unix_wait_status(0)))
```

No `[power] reset: triple fault` and no `machine still running` line in any lane. `refused=1` is one Cell the kernel refused to freeze; [INFERENCE] the freeze gate's `is_critical` refusal (which Cell was not logged). `kept=1` is VFS (no block route on the plain lane) [INFERENCE: from the keep rule, not logged by tid].

## Regression (TCG)

- nvme-x86 `11 passed`; ahci-x86 `15 passed` (its `/bin/shutdown` now enters S5 instead of `hlt`; the stale comment at `ahci-x86.rs:280-282` is left as logged); virtio-x86 `4 passed` (with `VICELL_X86_PC_ISO` from the same kernel; the first run without it failed only on "prerequisites missing while CI is set").
- x86_64-boot `7 passed, 2 failed`: both failures are the Tier-2 refusal tests, which need the prod-refusal witness ISO (`scripts/build-x86_64-prod-refusal-ci.sh`); not built here ("prerequisites missing while CI is set").

## Not run

riscv64 and aarch64 boot lanes (the non-x86 `/bin/shutdown` path is unchanged code under `cfg`; cross-build only); x86_64-boot prod-refusal witness; xhci/igb/iommu-dmar/machine-pin/serial CI lanes.
