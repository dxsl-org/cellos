# CP2 — phase 06: ACPI reset, S5 poweroff, power button, `SystemPower = 436`

Status: **approved by the owner 2026-11-05 (Law-1 confirmation 2); `SystemPower = 436` frozen** ([record](cp2-06-system-power-approval.md)).
Branch `feat/x86-s2-phase06-power` (base `a6392e1dc`). Handoff: [s2-handoff-request.md](s2-handoff-request.md), [approval](s2-handoff-approval.md). CP1: [cp1-pcie-irq-power-abi.md](cp1-pcie-irq-power-abi.md) §5 (D-4).
Evidence: [reports/phase-06-evidence.md](reports/phase-06-evidence.md).

Commits:
- `d0e43b534` feat(kernel): acpi reset, s5 poweroff, sci power button and SystemPower 436
- `1f2a213bf` feat(supervisor): orderly OP_POWER shutdown and x86 /bin/shutdown [-r] through it
- `ab69da8fe` test(x86): power-x86 lanes for reset, s5 and the acpi power button
- `de5499c19` build(x86): package and start the supervisor in the x86 images (file-set grant, [record](s2-phase06-supervisor-image-grant.md))
- `5a2ba5e3d` fix(supervisor): end each power marker with a newline and print power-x86 evidence lines

## 1. ABI delta as implemented

| Item | As approved (CP1 §5, request §4.1) | As implemented | Delta |
|---|---|---|---|
| Opcode | `SystemPower = 436`, append-only | `ViSyscall::SystemPower = 436`, `436 => SystemPower` decode; 437 stays `Unknown` | none |
| Envelope | `a0` 1 reboot / 2 poweroff; `a1..a5 = 0` | `api::syscall::system_power::{ACTION_REBOOT = 1, ACTION_POWEROFF = 2}`; kernel refuses non-zero `a1..a3` | `a4`/`a5` never reach dispatch (four-argument stub, same as 434/435) |
| Gate | `SupervisorCap`; allowlist bit 49 shared | bit 49 group (with 413-415/419/422); `caller_has_supervisor` checked before any argument | none |
| Success | no return | reboot `-> !`; poweroff does not return when S5 takes effect | none |
| Sentinels (fixed here) | to be fixed at CP2 | `ERR_PERMISSION_DENIED = usize::MAX`, `ERR_INVALID_INPUT = usize::MAX - 3`, `ERR_NOT_SUPPORTED = usize::MAX - 5` (the 434/435 values; api test asserts equality) | none |
| `NotSupported` | poweroff without static `\_S5_` or PM1 control block | also: not x86_64 (both actions), and the machine still running after the sleep write (bounded settle) | widened by two named cases |
| ostd | — | `sys_system_power(SystemPowerAction) -> SystemPowerError` | new SDK call |
| Raw 502 | unchanged everywhere | unchanged (no diff in its arms) | none |

Non-Law-1 wire (H-S2-7): supervisor-private `OP_POWER = 0x04`, `[OP_POWER, action]` in the App SDK envelope, zero padding enforced; status phase 2 on refusal only. Kernel relay sends `[0xAC, 0x00, 0x04, 0x02]` from sender 0; the supervisor accepts sender 0 for poweroff only and otherwise only the task named `shutdown`.

Kernel internals: static `\_S5_` scan (structural walk of namespace terms; methods/conditionals skipped; `\_S5_` only at root depth with two 3-bit integer constants); reset chain FADT register (I/O, memory admitted at init, bus-0 PCI config) → `0xCF9` (2, 6) → 8042 `0xFE` → zero-limit IDT `int3`; SCI vector `0x29` (outside `0x30..=0xDF`), ISR acks `PWRBTN_STS` and sets an atomic; route and `PWRBTN_EN` are programmed from the first hart-0 tick.

## 2. Deviations

1. **SCI route armed from the first hart-0 tick**, not inside the ACPI init block: the I/O APICs and the BSP APIC ID are registered after that block, and `main.rs` is limited to the one `sci::init` call. `relay_pending()` arms once, then relays.
2. **GPE0/GPE1 disabled and cleared at ACPI-mode entry**, reading the GPE block fields from the raw FACP inside `power.rs` (not `fadt.rs`, which is outside the set). Without a GPE handler, an enabled GPE would hold the level SCI asserted.
3. **`sci::init` is not called in the G1 private lane** (`cfg(not(cellos_g1_cpu))`); there 436 reboot still runs the fallback chain and poweroff is `NotSupported`.
4. **Fewer than two integer elements in `\_S5_` is refused** (no single-element packed form).
5. **Fixture capture**: the q35 DSDT was dumped with QMP `pmemsave` (QEMU 8.2.2, `-machine q35,hpet=on -m 256M`), not from a Linux guest; the method fixture is hand-assembled (no `iasl`), source `.asl` beside it.
6. **Supervisor marker** is `[supervisor] power: <action> (<origin>) froze=N refused=M kept=K settle<=500ms`, then `requesting <action>`; there is no "vfs flushed" claim (H-S2-8 (i)).
7. **Supervisor in every x86 image (granted 2026-11-05, [record](s2-phase06-supervisor-image-grant.md)).** The canonical images never shipped `/bin/supervisor`. Root cause of the earlier failed local witness: `scripts/generate-boot-config.py` `selected()` emits the supervisor row of `config/services.toml` only when the init features name `supervisor`, `hostile-backend-recovery` or `hotswap-demo`; the x86 profiles named only `input`(/`ui`). Fix inside the grant: `scripts/assemble-x86-image.py` adds `Cell("supervisor", …)` to `HEADLESS_CELLS` (desktop inherits it) and `supervisor` to both profiles' init features (an artifact-selection feature of `app-init`, no code in init); `scripts/cellos_sign/signing.py` adds `supervisor` to the headless signed set and re-pins the assembler (`7f32d28a…2411`); `scripts/test_assemble_x86_image.py` follows. Effect: every x86 image runs the supervisor (`restart = "always"`, service id 11).

## 3. Acceptance (request §7 row 06)

| Criterion | Result |
|---|---|
| Host `\_S5_` scan, both fixtures | met (q35 → `Static{0,0}`; method fixture → `NotStatic`) |
| 436 decode + bit 49; refused without `SupervisorCap` | met (api 194 passed; kernel `system_power_is_refused_without_supervisor_cap`) |
| `cargo test -p cellos-kernel` (x86_64-linux), `-p api`, `-p ostd`, `-p supervisor` | met: 438 / 194 / 89 / 11 passed |
| clippy `-D warnings` | met: kernel x86_64 and riscv64; `supervisor`, `app-sys-tools`, `api`, `ostd`, `hal-x86` on x86_64-unknown-none |
| `cellos-sign --check`, Law-1 digest scripts, assembler/signing script tests | met (F1/F5 OK; three digest scripts PASS/OK; `104 passed`) |
| Guest TCG: `/bin/shutdown -r` → second banner; `/bin/shutdown` → exit 0; QMP `system_powerdown` → supervisor marker → exit | met, `power-x86`: `3 passed` |
| Guest local KVM (`-cpu host`), same three | met, `3 passed` |
| Regression TCG: nvme-x86, ahci-x86, virtio-x86 | met: 11 / 15 / 4 passed (virtio with the pc-profile ISO built from the same kernel) |
| Regression TCG: x86_64-boot | 7 passed; the 2 Tier-2 refusal tests need the prod-refusal witness ISO (`scripts/build-x86_64-prod-refusal-ci.sh`), not built: "prerequisites missing while CI is set" |
| riscv64 / aarch64 `/bin/shutdown` unchanged | non-x86 path is the pre-change code under `cfg(not(target_arch = "x86_64"))` (same banner, relay window, raw 502, arguments ignored); `shutdown` cross-builds for both targets. Boot lanes **not run** (run budget) |
| Follow-up (main `ad989723c`, `CI=true`): x86_64-boot with the prod-refusal witness | met: TCG 9/9, KVM 9/9; both Tier-2 refusal tests ran (`[loader] PrepareChild refused: caller=7 path=/bin/tier2-smoke error=PermissionDenied` under `Tier 2 admission: DISABLED (fleet profile)`) |
| Follow-up: CI x86 lanes, TCG / KVM | met: xhci 4/4 / 4/4, igb 2/2 / 2/2, nic 2/2 / 2/2, iommu-dmar 3/3 / 3/3, serial 1/1 / 1/1, machine-pin 5/5 / 5/5, power 3/3 / 3/3 |
| Follow-up: nvme-x86, ahci-x86, virtio-x86 on KVM | met: 11/11, 15/15, 4/4 (virtio incl. `kvm_default_vm_acceptance_x86`). Fault witnesses not exercised (`VICELL_AHCI_FAULT_ISO` not built): 8 of the nvme and 10 of the ahci passes are announced early returns |
| Follow-up: riscv64 / aarch64 boot lanes | met for boot: rv64 `qemu-boot-test.sh` `PASS: FAT16 mounted — kernel booted (no disk)`; aarch64 default ELF lane `PASS: aarch64 shell prompt reached — full boot successful` (`-kernel Image`, `virt-vhe` not run). Neither image ships `/bin/shutdown`, so its non-x86 behaviour is still code reading + cross-build only |
| Follow-up: the Cell behind `refused=1` | **init** (tid 2), refused by the kernel critical-cell guard (`freeze_task_with_ceiling`: `is_critical` → `PermissionDenied`; `is_critical` is set only by `spawn_init`) — the intended protection of the restart tree. Evidence: `ps` before `/bin/shutdown` lists init 2, vfs 7, serial 11, input 13, net 14, supervisor 15, cellbox 16, then shutdown 17; sweep set {2, 11, 13, 14, 16, 17} = froze 5 + refused 1, kept {7 = VFS} = 1; the power-button run (no tid 17) gives `froze=4 refused=1`. The supervisor does not log the tid; attribution is by this reconciliation ([reports/phase-06-evidence.md](reports/phase-06-evidence.md) "Follow-up regression") |

## 4. Owner decision

| ID | Question | Recommendation |
|---|---|---|
| CP2-06-1 | Supervisor in the x86 images | **granted 2026-11-05** ([record](s2-phase06-supervisor-image-grant.md)); implemented as deviation 7 |
| CP2-06-2 | Ratify deviations 1–7 | Yes |
| CP2-06-3 | Freeze 436 with the §1 envelope and sentinels | Yes |
| CP2-06-4 | Unrun rows | **Resolved 2026-11-05:** all ran (x86_64-boot 9/9 incl. both prod-refusal tests, xhci, igb, nic, iommu-dmar, serial, machine-pin, power on TCG and KVM; rv64 and aarch64 default boot lanes PASS). Remaining gap: no rv64/aarch64 image ships `/bin/shutdown`, so its non-x86 path is backed by code reading and cross-builds only |
