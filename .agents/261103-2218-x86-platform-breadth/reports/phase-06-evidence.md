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

(All run since; see "Follow-up regression" below. Still not run: the `VICELL_AHCI_FAULT_ISO` fault witnesses, the aarch64 `-kernel Image` and `virt-vhe` lanes, any `/bin/shutdown` exercise on riscv64 or aarch64.)

## Follow-up regression (main `ad989723c`, 2026-11-05)

Detached worktree at the phase-06 merge. Ceilings: host, QEMU 8.2.2 TCG (`qemu64,+pdpe1gb`), local KVM (`CELLOS_X86_ACCEL=kvm`, `-cpu host`). Not physical evidence.
Images, as in CI job `qemu-x86_64-boot`: `assemble-x86-image.py --profile headless` (manifest `kernel_features` = `policy-required`, services.toml carries `/bin/supervisor`, service_id 11), kernel sha256 `45281e3d…4f53f6e`, `make-iso-ci.sh` default / `limine-pc.conf` / `limine-vm.conf`; prod-refusal witness from `scripts/build-x86_64-prod-refusal-ci.sh` (`build/vicell-x86-prod-refusal.iso`). Every suite ran with `CI=true` (a missing prerequisite fails instead of skipping) and each `VICELL_*_ISO` pinned. Regenerated `kernel/src/embedded*/` restored, not committed.

| Suite | TCG | KVM |
|---|---|---|
| x86_64-boot | 9/9 | 9/9 |
| xhci-x86 | 4/4 | 4/4 |
| igb-x86 | 2/2 | 2/2 |
| nic-x86 | 2/2 | 2/2 |
| iommu-dmar-x86 | 3/3 | 3/3 |
| serial-x86 | 1/1 | 1/1 |
| machine-pin-x86 (`--test-threads=1`) | 5/5 | 5/5 |
| power-x86 | 3/3 | 3/3 |
| nvme-x86 | 11/11 (phase-06 branch run above) | 11/11 ¹ |
| ahci-x86 | 15/15 (phase-06 branch run above) | 15/15 ¹ |
| virtio-x86 | 4/4 (phase-06 branch run above) | 4/4 (incl. `kvm_default_vm_acceptance_x86`, not skipped) |

¹ `VICELL_AHCI_FAULT_ISO` was not built, so the 08C/09C fault witnesses return early and count as passed (same as 04b): nvme-x86 8 of 11 (3 exercised), ahci-x86 10 of 15 (5 exercised), each announced by a `SKIP`/`SKIPPED (VICELL_AHCI_FAULT_ISO unset …)` line. No other suite printed a SKIP line on either accelerator.

x86_64-boot: both Tier-2 refusal tests now run against the witness (`x86_tier2_admission_is_refused_by_fleet_profile ... ok`, `x86_tier2_fault_isolation_fixture_is_refused ... ok`). Manual KVM boot of the witness, typing `tier2-smoke`:

```
[INFO] Tier 2 admission: DISABLED (fleet profile) — domain-class artifacts are denied
[ WARN] [loader] PrepareChild refused: caller=7 path=/bin/tier2-smoke error=PermissionDenied
```

power-x86 decisive lines, identical on TCG and KVM: `[supervisor] power: reboot (Operator) froze=5 refused=1 kept=1 settle<=500ms`, `[power] reset: FADT reset register space=1 addr=0xcf9 value=0xf`, second boot `/bin/shutdown` exit 0, `poweroff (Operator) froze=5 refused=1 kept=1`, `[power] poweroff: PM1a_CNT 0x604 SLP_TYPa=0 PM1b_CNT 0x0 SLP_TYPb=0`, `/bin/shutdown: QEMU exit … (0)`, `[acpi] power button: poweroff request relayed to supervisor tid 15`, `poweroff (PowerButton) froze=4 refused=1 kept=1`, `power button: QEMU exit … (0)`. No `triple fault` or `machine still running` line.

### riscv64 / aarch64 boot lanes

- riscv64 (CI `qemu-boot`): `scripts/build-boot-ramdisk-ci.sh`, kernel `--target riscv64gc-unknown-none-elf`, `scripts/qemu-boot-test.sh`: `[qemu-test] Booting (want_shell=0): -machine virt -m 256M -nographic -bios default -kernel …` → `PASS: FAT16 mounted — kernel booted (no disk)`.
- aarch64 (CI `qemu-aarch64-boot`, default ELF lane only): cells + `mkfat32.py` image + kernel + `format-disk-arm.sh`, `BOOT_WINDOW=90 scripts/qemu-aarch64-test.sh`: `lane=virt … cpu=cortex-a57 smp=1` → `PASS: aarch64 shell prompt reached — full boot successful`. The `-kernel Image` and `virt-vhe` lanes were not run.
- Neither lane exercises `/bin/shutdown`: the rv64 bootstrap ramdisk carries init, cellbox, vfs, config, platform, virtio-blk; the aarch64 CI image carries ls/cat/echo/ps/kill from `app-sys-tools` but not `shutdown`. So the non-x86 `/bin/shutdown` behaviour is still established by code reading and cross-build only; the boot results above show the merge does not break either boot.

### The Cell behind `refused=1`

`power.rs::shut_down` reads `sys_get_procs` and calls `sys_freeze_cell` on every row except tid 0, the supervisor's own tid (`sys_lookup_service(SUPERVISOR)`), rows in state 3 (Terminated/Retiring, `encode_task_state`) and the storage-stack tids (`VFS`, `BLOCK_DRIVER`, live block-route providers); a non-`Ok` freeze counts as `refused`. Kernel 413 (`syscall.rs` `FreezeCell`) → `hotswap::freeze_task_with_ceiling`, which returns `PermissionDenied` when `task.is_critical` (or the task is already frozen / already has a ceiling record). `is_critical` is `true` only in `governed_spawn::spawn_init` (the `TaskLaunchState::complete(…, true, …)` for `/bin/init`); governed `SpawnFromPath` and domain admission pass `false`. `tcb.rs`: "When `true`, `FreezeCell` and `KillCell` syscalls are rejected with `PermissionDenied`. Set at spawn for `init` … Prevents a compromised Supervisor Cell from disabling the restart tree."

Run (canonical ISO, manual QEMU, KVM and TCG identical; shell `ps` then `/bin/shutdown`; no source change):

```
[ INFO] Successfully spawned init with complete root authority (tid=2)
[ INFO] [service-registry] 1 -> tid 7 (cell 1 gen 114)          <- VFS
[ INFO] [service-registry] 11 -> tid 15 (cell 9 gen 122)        <- SUPERVISOR
USER: TID  NAME        STATE    CPU%   STACK  SEGMENTS
USER:   2  init        Waiting   new   64.0K      1.3M
USER:   7  vfs         Waiting   new  256.0K     17.5M
USER:  11  serial      Ready     new  256.0K      1.0M
USER:  13  input       Ready     new  256.0K      1.1M
USER:  14  net         Waiting   new   64.0K      9.4M
USER:  15  supervisor  Waiting   new  256.0K      1.0M
USER:  16  cellbox     Running   new   64.0K      9.7M
[ WARN] [policy] narrowed "/bin/shutdown" (tid 17): …
USER: [supervisor] power: poweroff (Operator) froze=5 refused=1 kept=1 settle<=500ms
[ WARN] [kernel] SystemPower: poweroff requested by tid 15
```

(`ps` is the cellbox telemetry view; it hides Retiring tasks, which are the GetProcs state-3 rows the sweep skips. `/bin/ps` is denied GetProcs on this image: `edge withholds the S2-T observation right`.) Sweep set = {2 init, 11 serial, 13 input, 14 net, 16 cellbox, 17 shutdown} = 6 = froze 5 + refused 1; kept = {7 vfs} (no service 9 registration, no live block route) = 1; supervisor 15 skipped. Power-button run: same table without tid 17 → `froze=4 refused=1 kept=1`, so the refused row is among the five long-lived Cells, not an exit race of `/bin/shutdown`; the three serial logs show no `[hotswap]` activity before the sweep. Init (tid 2) is the only `is_critical` task in the set, so **the refused Cell is init**, refused by the kernel's critical-cell guard — the intended protection (init stays runnable through the settle window; the supervisor cannot freeze the restart tree). [INFERENCE, bounded: the supervisor does not log the refused tid; the attribution is the exact count reconciliation above plus `is_critical` being set only for init.] `kept=1` is VFS tid 7 by the same reconciliation (supersedes the inference above).
