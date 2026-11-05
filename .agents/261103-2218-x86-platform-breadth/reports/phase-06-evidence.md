# Phase 06 evidence — ACPI reset, S5, power button, 436

Branch `feat/x86-s2-phase06-power`, commits `d0e43b534`, `1f2a213bf`, `ab69da8fe`. Ceiling: host + QEMU 8.2.2 TCG (local). KVM lanes not run.

## Host

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: `test result: ok. 438 passed; 0 failed`
  - `acpi::tests::s5_scan_reads_the_q35_dsdt ... ok`
  - `acpi::tests::s5_scan_refuses_a_name_created_inside_a_method ... ok`
  - `acpi::tests::s5_scan_accepts_only_a_root_name_with_constant_slp_types ... ok`
  - `acpi::tests::discovery_scans_the_dsdt_then_listed_ssdts_and_reads_gpe_blocks ... ok`
  - `task::syscall::tests::system_power_is_refused_without_supervisor_cap ... ok`
  - `task::syscall::tests::system_power_checks_action_and_arguments_behind_the_gate ... ok`
- `cargo test -p api`: `194 passed` (`system_power_shares_the_supervisor_op_bit`, `system_power_actions_and_sentinels_are_fixed`)
- `cargo test -p ostd`: `89 passed` (`system_power_decoder_names_each_sentinel`)
- `cargo test -p supervisor`: `11 passed` (power request parse, kernel relay bytes)
- `cargo test -p hal-x86` was not run separately; the policy test change (`0x29 → Sci`) compiles in the kernel build.
- clippy `-D warnings`: `cellos-kernel` x86_64-unknown-none exit 0; riscv64gc-unknown-none-elf exit 0.
- `cellos-sign --check`: `OK: F5 …`, `OK: F1 — 139 crates and 998 files scanned`.
- Law-1 digest scripts: `OK: the frozen AI interface…`, `PASS: LookupServiceBound…`, `PASS: multi-source WaitCompletion…`.

## Guest (TCG, ISO built from this branch, `tests/integration/tests/power-x86.rs`)

Kernel lines from the first boot:

```
[ INFO] [acpi] FADT: rev=3 pm_timer=0x608/24 reset=true hw_reduced=false msi_forbidden=false sci=9
[ INFO] [acpi] power: \_S5_ static SLP_TYPa=0 SLP_TYPb=0
[ INFO] [acpi] power: reset_reg=true space=1 addr=0xcf9 value=0xf
[ INFO] [acpi] sci: ACPI mode on, sci_int=9 gsi=9 flags=0xd vector=0x29
[ INFO] [acpi] sci: armed gsi=9 vector=0x29 apic=0 PWRBTN_EN
```

QMP `system_powerdown` reached the ISR and the relay:

```
[ WARN] [acpi] power button: supervisor unreachable; retried every tick
```

`/bin/shutdown -r` and `/bin/shutdown`:

```
USER: System rebooting...
USER: shutdown: supervisor unavailable
```

Result: `test result: FAILED. 0 passed; 3 failed` — all three at the supervisor marker. Cause: the canonical x86 image carries no `/bin/supervisor` (`scripts/assemble-x86-image.py` `HEADLESS_CELLS`). A local, uncommitted witness adding the row, the signed name and a new recipe pin in `scripts/cellos_sign/signing.py` built, but the supervisor still did not start (same three failures). Not investigated further.

Not run: KVM lanes, the reset/poweroff writes themselves (never reached), x86 regression suites (x86_64-boot, nvme, ahci, virtio), rv64/aarch64 boot lanes.
