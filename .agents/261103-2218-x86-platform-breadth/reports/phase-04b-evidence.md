# Phase 04b evidence — `WaitCompletion(IRQ)` and IRQ-driven storage/USB cells (2026-11-05)

Branch `feat/x86-04b-irq-drivers`, base `6fee21927`. Evidence ceiling: host tests, QEMU 8.2.2 TCG, QEMU 8.2.2 KVM (local `/dev/kvm`), QEMU riscv64 TCG. No physical machine. CP2: [cp2-04b-waitcompletion-irq.md](../cp2-04b-waitcompletion-irq.md).

## Host

| Command | Result |
|---|---|
| `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu` | 432 passed (421 before; +11) |
| `cargo test -p api` | 192 passed (+2) |
| `cargo test -p ostd` | 88 lib tests passed (+1) |
| `cargo test -p driver-nvme` | 11 passed |
| `bash scripts/check-multi-source-wait-law1-digests.sh` | `PASS: multi-source WaitCompletion Law-1 items intact …` |
| `python3 scripts/cellos-sign --check --strict` | `OK: F1 — 137 crates and 990 files scanned; unsafe confined to 75 allowlisted files` |
| `cargo clippy -D warnings`: kernel (x86_64-unknown-none, riscv64gc), ostd+api (x86_64-unknown-none), driver-nvme/ahci/xhci (x86_64-unknown-none; nvme/ahci also `fault-inject`) | clean |
| `cargo fmt --check` (api, ostd, kernel, the three cells) | clean |

New kernel tests (CP1 §9 host rows):
- `completion_wait::tests::four_source_rotor_serves_irq_between_net_rx_and_timer_without_consuming_it`, `async_ipc::…::source_rotation_reaches_every_ready_source_within_four_selections` — 4-source rotor, every ready source within 4 selections, fresh rotor starts at NET_RX.
- `irq_lines_are_served_round_robin_after_the_cursor` — line round robin.
- `a_pending_snapshot_line_stops_a_park_and_wakes_the_sweep_but_an_outside_line_does_not` — `IrqPending`; the sweep predicate (`irq_wake_due`, the whole body of the new sweep arm) ignores a pending line outside the snapshot.
- `a_take_after_revoke_fails_and_an_empty_refresh_releases_the_slot`, `pcie_msi::tests::a_revoked_line_is_neither_taken_nor_repended_by_its_former_owner` — revoke, re-allocation to another owner, `PermissionDenied` with the slot released.
- `a_failed_copy_out_re_pends_the_irq_line`, `an_irq_wait_without_lines_is_refused_before_reserving`.
- `syscall::tests::wait_completion_refuses_mask_bits_above_31_instead_of_dropping_them` (D-B-7).
- `irq_wait::tests` ×3 (line set, lock-free peek, wake hint).
- Lone NET_RX/TIMER paths: the existing completion/NET_RX tests pass unchanged.

ostd: `an_irq_record_decodes_to_its_line_and_an_out_of_range_line_is_refused` (`Wake::Irq(0x41)`; 256 and −1 → `Decode`).

## Boot self-tests

- x86_64 (TCG and KVM boots of the lane ISO): `[INFO] completion-queue self-test PASS (reserve, land, bound, defer)`, `[INFO] ipc-pending self-test PASS (deferred delivery, bounds, quota)`.
- riscv64 (`scripts/build-boot-ramdisk-ci.sh`, `scripts/qemu-boot-test.sh`, TCG): `[INFO] completion-queue self-test PASS (reserve, land, bound, defer)` — that self-test now requires `IRQ` to be refused off x86_64 — and `PASS: FAT16 mounted — kernel booted (no disk)`.

## QEMU lanes (clean image, every 04b commit, no instrumentation)

Image: `scripts/assemble-x86-image.py --profile headless`, kernel `--features policy-required`, `scripts/x86/make-iso-ci.sh` (default, `limine-pc.conf`, `limine-vm.conf`). The regenerated `kernel/src/embedded-x86_64/` was not committed.

| Suite | TCG | KVM |
|---|---|---|
| nvme-x86 | 11/11 | 11/11 |
| ahci-x86 | 15/15 | 15/15 |
| xhci-x86 | 4/4 | 4/4 |
| iommu-dmar-x86 | 3/3 | 3/3 |
| virtio-x86 | 4/4 | 4/4 |
| nic-x86 | 2/2 | 2/2 |
| igb-x86 | 2/2 | 2/2 |
| serial-x86 | 1/1 | 1/1 |

Not run: `x86_64-boot`, `local-service-lifecycle-x86`, `pcie-multibus-x86`, `machine-pin-x86`, and the `async_lifecycle` bench.

The fault-injection witnesses in nvme-x86/ahci-x86 (08C/09C) need `VICELL_AHCI_FAULT_ISO` and skipped in these runs (they count as passed); the fault ISO was not rebuilt. `nvme_driver_cell_registers_x86`, `ahci_driver_cell_registers_block_driver_x86` and `xhci_driver_cell_enumerates_usb_keyboard_x86` now assert `[nvme] irq=msix`, `[ahci] irq=msi`, `[xhci] irq=msix` (and no `irq=polled`).

Manual q35 boot with NVMe + SATA + `qemu-xhci`/`usb-kbd`, KVM (TCG identical apart from ordering):

```text
[ INFO] [msi] 00:02.0 tid=4 kind=msix irq=0x30+2
USER: [nvme] irq=msix vectors=0x30+2
[ INFO] [msi] 00:1f.2 tid=5 kind=msi irq=0x32+1
USER: [ahci] irq=msi vectors=0x32+1
[ INFO] [msi] 00:03.0 tid=12 kind=msix irq=0x33+1
USER: [xhci] irq=msix vectors=0x33+1
USER: [xhci] USB HID keyboard ready vid=0x0627 pid=0x0001
USER: Cellos >
```

## Throwaway runs (instrumented, reverted, not committed)

Instrumentation: `pcie_msi::on_msi` stored `hal::clock::now_ns()` per vector; `wait_multi` logged `now − msi_ns` for an IRQ record returned after the waiter had parked. Drivers' spin-before-park set to 0 so every wait parks.

- **Every line wakes its waiter (KVM, forced parks):** samples on 0x30 (nvme admin) ×4, 0x31 (nvme I/O) ×9, 0x32 (ahci) ×2; no `irq wait expired` line from any cell. Under TCG, QEMU completes ahci/xhci commands inside the doorbell write, so the line is already pending when the wait starts (no park, no sample) — still no expiry from any cell, i.e. every wait was ended by its line.
- **Lost-vector path:** the first (pre-fix) build allocated NVMe MSI-X after `CC.EN`; QEMU then never signalled vector 0. Every admin wait took the park-expiry path and logged `[nvme] irq wait expired after 10 ticks; re-reading the queue` (KVM ×4, TCG ×1), and the cell still registered its block route and the shell came up. This is the masked-vector criterion of CP1 §9, observed on a real lost vector rather than a synthetic mask; it also found deviation 3.
- **MSI-to-wake latency, idle guest (KVM, `-smp 1`):** after the shell was idle for 5 s, four `vwrite`/`vcat` commands on a FAT32 NVMe volume: `idle n=130 min=7 median=14 max=8466` (µs). Boot-time (busy) KVM: `n=15 min=4 median=6 max=10` µs. TCG boot: `n=4 median=24 max=657` µs. Median is far below 1 ms. **Not measured:** the same run with the wake hint removed (the "today's tick bound" baseline); the 10 ms bound is the CP1 E9 analysis, not a measurement from this phase.
