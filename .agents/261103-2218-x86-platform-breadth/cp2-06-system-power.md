# CP2 — phase 06: ACPI reset, S5 poweroff, power button, `SystemPower = 436`

Status: **written 2026-11-05, owner decision pending. Guest acceptance is NOT met (blocker B-1).** Not approved; 436 is not frozen by this document.
Branch `feat/x86-s2-phase06-power` (base `a6392e1dc`). Handoff: [s2-handoff-request.md](s2-handoff-request.md), [approval](s2-handoff-approval.md). CP1: [cp1-pcie-irq-power-abi.md](cp1-pcie-irq-power-abi.md) §5 (D-4).
Evidence: [reports/phase-06-evidence.md](reports/phase-06-evidence.md).

Commits:
- `d0e43b534` feat(kernel): acpi reset, s5 poweroff, sci power button and SystemPower 436
- `1f2a213bf` feat(supervisor): orderly OP_POWER shutdown and x86 /bin/shutdown [-r] through it
- `ab69da8fe` test(x86): power-x86 lanes for reset, s5 and the acpi power button

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
7. **Blocker B-1 (file outside the set):** the canonical x86 image does not ship `/bin/supervisor`. `scripts/assemble-x86-image.py:106-127` (`HEADLESS_CELLS`) has no supervisor row, and `scripts/cellos_sign/signing.py:64-70` pins the assembler digest and the headless signed set (no `supervisor`). On x86 `/bin/shutdown` therefore prints `shutdown: supervisor unavailable` and the relay logs `supervisor unreachable`. A local, uncommitted witness that added the row, the signed name and the new pin still did not start the supervisor (no supervisor line in the boot log); the services template selection (`config/services.toml:101-105` via `scripts/generate-boot-config.py`) was not investigated further. Needs an owner grant for the image/signing files.

## 3. Acceptance (request §7 row 06)

| Criterion | Result |
|---|---|
| Host `\_S5_` scan, both fixtures | met (q35 → `Static{0,0}`; method fixture → `NotStatic`) |
| 436 decode + bit 49; refused without `SupervisorCap` | met (api 194 passed; kernel `system_power_is_refused_without_supervisor_cap`) |
| `cargo test -p cellos-kernel` (x86_64-linux), `-p api`, `-p ostd` | met: 438 / 194 / 89 passed |
| clippy `-D warnings` x86_64 + riscv64 kernel | met (exit 0 both) |
| `cellos-sign --check`, Law-1 digest scripts | met (F1/F5 OK; three scripts PASS/OK) |
| Guest `/bin/shutdown -r`, `/bin/shutdown`, QMP `system_powerdown` (TCG, KVM) | **not met** — B-1. Kernel half observed on TCG: `\_S5_` static, ACPI mode, SCI armed, power-button press reached the relay |
| Regression suites, rv64/aarch64 boot lanes | **not run** (stopped at B-1 within the run budget); clippy riscv64 only |

## 4. Owner decision

| ID | Question | Recommendation |
|---|---|---|
| CP2-06-1 | Grant `scripts/assemble-x86-image.py` (`HEADLESS_CELLS` supervisor row), `scripts/cellos_sign/signing.py` (pin + signed name) and whatever makes init start it, so H-S2-6/H-S2-7 can be exercised | Yes, then rerun §3 guest rows |
| CP2-06-2 | Ratify deviations 1–6 | Yes |
| CP2-06-3 | Freeze 436 with §1 sentinels | Only after the guest rows pass |
