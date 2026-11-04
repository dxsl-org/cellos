# Phase 04a evidence — PCIe function ABI and MSI/MSI-X (2026-11-04)

Base HEAD `7fdc088af`; working tree uncommitted. Evidence ceiling: host +
QEMU 8.2.2 TCG + QEMU/KVM (local). CP2 package:
[cp2-04a-pcie-function-abi.md](../cp2-04a-pcie-function-abi.md).

## Throwaway smoke (QEMU nvme, not committed)

The NVMe cell was patched temporarily. It called 434, then 435 with `want=2`
and MSI-X. It parked in `WaitIrq(first_irq)` after every admin doorbell, before
its normal completion poll. The same lines appeared under TCG and KVM
(`-machine q35 -smp 2 -device nvme`):

```text
[msi] vectors=0x30-0xdf firmware_forbids=0 no interrupt remapping
[smoke-04a] query flags=0x1 msix_n=65 table_bar=0 off=0x2000 pba=0x3000 bar0=0xfebd4000/0x4000 flags=0x2 caps=3
[smoke-04a] foreign query=Err(PermissionDenied)
[msi] 00:03.0 tid=3 kind=msix irq=0x30+2
[smoke-04a] alloc PcieIrqAlloc { first_irq: 48, count: 2, kind: 1 } again=Ok(PcieIrqAlloc { first_irq: 48, count: 2, kind: 1 })
[smoke-04a] foreign wait=Err(PermissionDenied)
[smoke-04a] admin opc=6 msix wake=Ok(())   (x2, Identify)
[smoke-04a] admin opc=5 msix wake=Ok(())   (Create I/O CQ)
[smoke-04a] admin opc=1 msix wake=Ok(())   (Create I/O SQ)
[driver_cell] block driver registered: tid=3 device=pci-00-03-0 route=0
```

What these lines show:

- Every admin completion woke the cell through MSI-X vector 0x30.
- The BAR0 MSI-X pages (0x2000–0x3FFF) were carved (`flags=0x2`), and the cell
  still registered its block route.
- A query on a BDF the cell does not own was refused, and so was a wait on a
  vector it does not own.
- A repeat 435 call returned the first allocation.

**Death path.** The same smoke ran under the fault-injection ISO
(`make-ahci-fault-iso.sh`, `nvme_driver_restart_resets_and_serves_again_x86`,
TCG). The test passed and its witness logged:

- `[pcie] quiesced 00:02.0 for tid 3: … msi=none msix=masked`;
- the replacement `tid=10` received the freed block again (`irq=0x30+2`) and
  took four admin completions by MSI-X.

The hook and the regenerated `kernel/src/embedded-x86_64/` were then reverted.

## Host

- `cargo test -p cellos-kernel --target x86_64-unknown-linux-gnu`: 421 passed
  (13 more than phase 03). The new tests cover:
  - `pcie_msi::vectors` ×4: exhaustion, alignment, int80 skip, free/reuse;
  - `pcie_msi::layout` ×6: register decode, carve-safe and refused layouts,
    range subtraction;
  - `pcie_msi::program` ×1: MSI rounding;
  - `syscall::tests` ×2:
    - skip-owned lookup with two identical functions (A, then B, then none,
      then A again after reap);
    - the 434/435 gate and their typed sentinels.
- `cargo test -p api`: 190 passed. New: record layout offsets, round trip,
  strict decode refusals, zeroing past `cap_count`/`len`, 435 pack/unpack,
  434/435 on allowlist bit 50.
- `cargo test -p hal-x86`: 25 passed. New: MSI vector range, message encoding,
  and IDT policy routes `0x30`/`0xDF` → `Msi`, `0xE0` → fatal.
- `cargo fmt --check` passes for the touched crates.
- `cargo clippy -D warnings` is clean for the kernel on x86_64-unknown-none and
  riscv64, and for `ostd` on x86_64.
- The aarch64 kernel passes `cargo check`. Its clippy run stops on two
  pre-existing errors in `libs/api/src/services/posix/setjmp.rs`, a file this
  phase did not touch.

## Integration (clean ISO, drivers unconverted)

| Suite | tcg | kvm |
|---|---|---|
| serial-x86 | 1/1 | 1/1 |
| nvme-x86 | 11/11 | 11/11 |
| ahci-x86 | 15/15 | 15/15 |
| nic-x86 | 2/2 | 2/2 |
| igb-x86 | 2/2 | 2/2 |
| xhci-x86 | 4/4 | 4/4 |
| virtio-x86 | 1/1 (1 ignored) | — |
| iommu-dmar-x86 | 3/3 | 3/3 |
| pcie-multibus-x86 | 2/2 | 2/2 |
| x86_64-boot | 7/9 (pre-existing prod-refusal witness failures, phase 02 evidence) | — |
| local-service-lifecycle-x86 | 2/3 | 2/3 |

The lifecycle failure is `x86_multi_source_wait_holds_on_two_cpus`, with the
pre-existing `CANCEL-RACE` signature (`total=256 cancelled=64
indeterminate=192`). Run alone afterwards, the test failed 1 time in 6 (tcg
1/3, kvm 0/3); the baseline rate was 1 in 8. `[INFERENCE]` The cause is
unrelated: the race is in IPC cancel timing, and no code on that path changed.
In both full-suite runs, however, this test failed under tcg and under kvm,
which is more often than the baseline rate predicts.
