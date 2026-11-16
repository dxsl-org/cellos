# S8b handoff request: driver model and Tier-1 TCB perimeter (Phase 18)

**Date:** 2026-11-16 · **Plan:** [plan.md](plan.md) · **Prior:** Phase 17 closed 2026-11-16 ([evidence](reports/phase-17-evidence.md)); `main` is at `9e07017af1`.

Phase 18 is the second S8 phase. It is not a checkpoint phase.

## 1. Scope (measured on `9e07017af1`)

| Surface | Today |
|---|---|
| NIC cells | Nine cells, each with its own `main.rs`, `dispatch.rs` and `dma_layout.rs`: `igc`, `ixgbe`, `i40e`, `r8169`, `bcm57xx`, `e1000`, `e1000e`, `igb`, `vmxnet3`. `dispatch.rs` is 51 lines in five of them, and only 6 lines of it differ from `igc`'s in `ixgbe`, `i40e`, `r8169` and `bcm57xx`. `dma_layout.rs` differs by 2–19 lines. virtio-net uses `libs/virtio-pci-transport`. |
| NIC routing | A last-writer-wins singleton: `NIC_DRIVER_STATE` (`kernel/src/dev/driver_cell.rs:77,250-261`). A second NIC driver replaces the first. Block devices already use per-device routes. |
| Device ownership | MMIO (`resource_registry`), IRQ/MSI and DMA (`iommu`) are each decided by a separate rule. |
| DMA remapper | VT-d is the only backend. The HAL has an `ArmIommu` trait with only a `NoIommu` implementation (`hal/arch/arm/src/aarch64/backend/mod.rs:84,626-634`). |
| Tier-1 perimeter | The F1 unsafe scan covers `cells/` only (`scripts/cellos_sign/__init__.py`: "libs/* is trusted TCB, reviewed rather than ratcheted"). `libs/ostd` has 239 `unsafe` lines and links into every cell. The allowlist has 41 `c-ffi` entries, 6 `legacy-raw-ptr`, 65 `driver-mmio`, 7 `hypervisor`, 13 `test-harness` and 8 `test-violation`. A cell's tier comes from its own manifest class (`loader/governed_spawn.rs:56-81`), and nothing ties a `c-ffi` allowlist class to an FFI/UNTRUSTED manifest. Spec 16's TCB table omits `ostd`. |

## 2. Staging (proposed)

1. **Census.** Read only; produces `research/p18-census.md`. It diffs the nine NIC cells and fixes the `NicDevice` trait. It maps the MMIO, IRQ and DMA ownership rules and the remapper interface. It joins each allowlist class with its crate's manifest class to give the mismatch list, and runs the scanner on the libraries Tier-1 cells link.
2. **`libs/nic-driver`.** It holds bring-up, dispatch, sender authentication and the DMA ring layout, with a small `NicDevice` trait per chip. Migrate two cells first (`igc`, `r8169`), then the rest. The NIC wire protocol stays as Phase 12 left it.
3. **NIC routes keyed by BDF.** Fixture: two NICs in QEMU, of the same model and of different models, both stay bound, both carry traffic, and restarting one leaves the other bound.
4. **One `DeviceRecord`** (owner incarnation, BARs, IRQs, DMA domain), consulted by MMIO, IRQ and DMA checks, one check at a time. Teardown releases through it.
5. **One kernel `DmaRemapper` interface** with VT-d behind it. `ArmIommu` is deleted until a backend exists.
6. **Tier-class gate.** `scripts/check-tier-class.py`, wired into the signer: a crate with a `c-ffi` or `legacy-raw-ptr` allowlist class is refused unless its manifest class is FFI or UNTRUSTED. Negative control fixture. The mismatch list goes to you (question 2).
7. **Scan the Tier-1 dependency closure.** List `libs/*` entries, set a baseline and ratchet it. Correct Spec 16 and Spec 13.

The working mode is the same as S7c and S8: a worktree, one commit per step, a rebase before the merge, nothing pushed. An independent review runs after steps 2–5 and again after 6–7.

## 3. File set

- **Create:** `libs/nic-driver/`, `kernel/src/dev/ownership.rs`, `scripts/check-tier-class.py`, the two-NIC fixture, `research/p18-census.md`, `reports/phase-18-evidence.md`.
- **Modify:**
  - the nine NIC cells;
  - `kernel/src/dev/{driver_cell,iommu*}.rs`, `kernel/src/arch/x86_64/dev/**` (VT-d behind the interface), `kernel/src/resource_registry.rs`, and the MMIO/IRQ/DMA syscall arms in `kernel/src/task/syscall/dev.rs`;
  - `hal/arch/arm/src/aarch64/backend/mod.rs` (deleting `ArmIommu`);
  - `scripts/cellos_sign/**`, `scripts/unsafe-allowlist.toml`;
  - NIC lane scripts and `scripts/lanes.toml`;
  - root `Cargo.toml` (workspace member), `scripts/host-test-crates.txt`, `scripts/mutable-globals.toml`, `kernel/residents.toml`;
  - `docs/specs/{13-peripherals,16-rustc-tcb}.md`.
- **Delete:** the per-cell copies of `dispatch.rs`/`dma_layout.rs` and `ArmIommu`.
- **All steps:** this plan directory, `.agents/plan-portfolio.md`, `docs/project-changelog.md`.

`libs/abi` and `libs/types` do not change. `sys_register_nic_driver` keeps its number and arguments; routing by BDF is decided in the kernel from the caller's owned device. If the census finds the routing needs a new argument, work stops and comes back to you for Law-1.

## 4. Behaviour to expect

- **Two NICs both work.** Before, the second registration silently replaced the first.
- **A cell's MMIO, IRQ and DMA authority comes from one record.** A claim that one rule used to allow but another did not is now refused consistently. The census lists every such case before anything changes.
- **Signing refuses a mismatched crate.** Already-signed artifacts are unaffected.
- **NIC behaviour on the wire is unchanged.** QEMU covers e1000, e1000e, igb, vmxnet3 and virtio-net. `igc`, `ixgbe`, `i40e`, `r8169` and `bcm57xx` are physical-only and are covered by host tests only, as today.

## 5. Risks and conflicts

- **x86 platform breadth S5 (phases 13–19):** the I219, I350, igc, r8169, BCM5720, ixgbe and i40e drivers are "code complete, capture pending physical machine". Refactoring them changes the code those captures would test (question 4).
- **The tier gate may flag Tier-1 cells that cannot move yet.** The gate is not weakened; exceptions are explicit and signed.
- **The `libs/*` scan baseline will be large.** It is accepted as the starting point, and the ratchet only goes down.
- **Other session in `/home/dmin/cellos`:** its only changes are untracked files, with no overlap with this file set.

## 6. Questions to the owner

1. Grant S8b (Phase 18) staged as in §2, continuing without stops as S7c and S8 did. It stops only on a behaviour difference beyond §4, or if an ABI or manifest change is needed.
2. **Tier-gate mismatches** (crates with a `c-ffi`/`legacy-raw-ptr` class and a Tier-1 manifest). Proposed: each becomes an explicit, signed exception in the allowlist, naming the crate and a reason, and the list goes in the evidence for later triage. Alternatives: move each to an FFI/UNTRUSTED manifest now (Tier 2 runs on x86 since Phase 15), or refuse them now.
3. **`libs/*` scan baseline.** Proposed: today's `unsafe` sites in the Tier-1 closure (`ostd`, `wire`, `abi` and the rest the census finds) are the baseline, and any new site fails. Alternative: review each site before the baseline is accepted.
4. **Physical-only NIC drivers vs the pending S5 captures.** Proposed: migrate all nine to `libs/nic-driver`, keeping each chip's host tests; the physical captures then test the shared code. Alternative: migrate only the QEMU-testable cells (e1000, e1000e, igb, vmxnet3), and leave igc/ixgbe/i40e/r8169/bcm57xx until their captures are done.
