# Phase 18 evidence: driver model and Tier-1 TCB perimeter

**Date:** 2026-11-16 · **Phase:** [phase-18-driver-model-tcb.md](../phase-18-driver-model-tcb.md) · **Handoff:** [S8b approval](../s8b-handoff-approval.md) · **Census:** [drivers](../research/p18-census-drivers.md), [TCB](../research/p18-census-tcb.md)

## 1. Success criteria

The owner narrowed the two-NIC criterion at census time: traffic on both NICs at once needs an ABI change and a multi-interface net service (Deviation Log).

| Criterion | Result |
|---|---|
| x86 QEMU with two NICs, same model and different models: both registered, traffic on the primary, restarting one driver does not unbind the other | `nic2-x86` passes for e1000e+igb and for two e1000e. Each pair shows `NIC driver registered: … device=pci-00-03-0 route=0` and `… device=pci-00-04-0 route=1`, plus `[net] IP address: 10.0.2.15` on the primary. Killing the primary logs `quiesced 00:03.0`, `NIC route 1 promoted to primary`, `Init: service restarted.`, and the replacement registers route 0 again. `pci-00-04-0` is never re-registered and its driver is never quiesced. Killing the promoted driver hands primary back. No panic, cell fault, restart storm or `not quiescent`. |
| Signing a crate with a `c-ffi` allowlist entry and a Tier-1 manifest fails (negative control) | `scripts/check-tier-class.py --negative-control` runs the real `cli.main --sign` path in temporary git repositories. A `c-ffi` crate with a Tier-1 manifest exits 1 and nothing is signed. The same crate declaring `PROTECTION_CLASS_FFI` is accepted, and an ELF whose `__ViCell_manifest` loads Tier 1 is refused even when its source declares FFI. CI `f1-admission` runs this control. |
| The unsafe scan reports `libs/ostd` (and the rest of the Tier-1 closure) and fails on a new unlisted `unsafe` there | `scripts/tcb-unsafe-baseline.toml` holds 52 per-file ceilings across the in-tree closure, `libs/ostd` included. `scripts/check-tcb-closure.py --negative-control` rejects a new file with `unsafe` in `libs/ostd`, one extra `unsafe` in `syscall.rs` and one extra `#[no_mangle]`. `libs/api` no longer exists (`077909a812`); the criterion is read as the closure (Deviation Log). |
| NIC lanes (e1000, e1000e, igb, vmxnet3, virtio-net) pass on x86; `x86-c2c` passes | `nic-x86` (e1000, e1000-82545em, e1000e, each ± VT-d), `igb-x86`, `vmxnet3-x86`, `virtio-x86` and every `x86c2cp14` lane pass on every run in §4. |

## 2. What changed

### Driver model (steps 2–3)

- **`libs/nic-driver`.** One `NicDevice` trait plus `nic_driver::cell!` replaces nine copies of probe, identity gate, BAR claim, publish-then-register, serve loop, sender gate, OP_TX/OP_RX/OP_GETMAC dispatch and shutdown. The library also holds `try_init_array`, `DescRing`, bounded `FrameBufs`, an authorize-before-program `DmaSet` with chip-ordered extra slots (i40e admin queues first, bcm57xx status block last), the MSI-X `BoundedWait` and one `RegisterFile`. The wire protocol and every bind line are byte-for-byte unchanged.
- **Nine cells migrated** (`facf6f5027`, `5264af5fe3`, `e1dcec534a`, `272b32d0bf`). vmxnet3 keeps its single-allocation DMA area. Eight cells now carry `#![forbid(unsafe_code)]`. NIC `unsafe` lines fell from 102 (all in cells) to 9 in vmxnet3 `dma.rs` plus 7 in the library. The seven `unsafe impl Send` controllers turned out to be unnecessary.
- **NIC routes by device** (`be7240ee0e`). `NIC_DRIVER_STATE` became `NIC_ROUTES`. Syscall 417 keeps no arguments and takes its key from the caller's single owned network device. The first live registration is primary; when the primary dies or loses its device, the lowest live route is promoted and the `NIC_DRIVER` row is re-pointed in place or inserted if absent (`0e5a6deb1b`, `24231eb1c4`, `63132e9a67`). A paused hot-swap row is never re-pointed (`bec0d1acc6`).
- **Fixture.** `libs/boot-config` lets two service-less entries share a `path` (`0b045ac9eb`). `libs/nic-driver` gains a `fault-inject` feature that exits on a frame naming the cell's own function (`8f5caa4f63`). The `nic2-x86` lane, ISO script and CI step are in `7c2c98fd98`.

### Device ownership and DMA (steps 4–5)

- **One `DeviceRecord` table** (`kernel/src/dev/ownership.rs`, lock `DEVICES`) replaces `BDF_OWNERS` and `PCIE_BARS`. One record per PCI function, VirtIO slot and DWC2 aperture holds the owner tid, windows and interrupt lines. MMIO, MSI, DMA grants, DMA export, 434/435, 416 and 417 all read it (`68913d3ed2`, `90684990bf`, `9f17701598`). Teardown releases through one `release_for` (`99cce61d00`); ten view functions are deleted.
- **The table grows** instead of capping at 256 records, and records are never removed, so no interrupt path allocates (`5ee8848e4e`). Syscall 235 creates a record only for a function that answers in ECAM (`82fde87197`).
- **Interrupt proof is one atomic load.** `NIC_LINES`, a 256-bit set rewritten under `DEVICES`, replaces the per-route locked scan in the MSI and VirtIO ISRs (`f4914b8c63`).
- **`DmaRemapper`** (`e2dfea8cf4`, `461c90c21f`). One trait in `dev::iommu`, implemented by VT-d (`iommu_x86::Vtd`) and the RISC-V IOMMU (`iommu_riscv::RiscvIommu`), with `NoRemapper` on aarch64 and 32-bit targets. VT-d-only types left the shared interface. HAL `ArmIommu`/`NoIommu`, `AppleDartPt` and the callerless `map_dma`/`unmap_dma` are deleted (`c732dbc65f`). DMA behaviour is unchanged on every arch.
- **x86 MSI ownership** lives under `kernel/src/arch/x86_64/dev/ownership.rs` (`9a0f969185`).

### Authority closed

| Hole | Before | Now |
|---|---|---|
| 417 (NIC route) | Any live `PcieDriverCap`/`UsbDriverCap` cell, e.g. `/bin/ahci`, `/bin/xhci`, `/bin/intel-gpu`, could take every frame | Only a cell owning exactly one network device (`be7240ee0e`) |
| `RequestMmio` PCIe BAR (X1) | Any `PcieDriverCap` cell, and a cell with the PCIe MMIO manifest class without the cap, could map another driver's BAR | Only the task that claimed the function through 418/424 (`5bf8d5b2f3`). Every in-tree PCI driver already claims first, as review confirmed. |
| `WaitIrq` on riscv64/aarch64 (X2), aarch64 USB unmask | Any driver capability could wait on or drain another driver's line | Only the line's device owner (`d1092034dc`) |
| NET_RX signalling | Any MSI vector the NIC task held | Only vectors of that route's own device |
| VirtIO/DWC2 window takeover (X3) | A replacement could take a window whose dead owner awaited teardown | First requester owns it until reap (`5bf8d5b2f3`) |
| Runtime device-capability revoke | NIC/block route kept naming the revoked task | Routes on freed devices are released, and a stale holder never blocks a new owner (`3ecff4b457`, `a7fcf9ee0a`) |

### Tier-1 TCB perimeter (steps 6–7)

- **Tier-class gate** (`2288da9685`, `5505096470`). Inside the signer policy, run by CI `--check --strict` and before every `--sign`. It reads the manifest class from source and, before signing, from each target ELF's `__ViCell_manifest` with the kernel's rules; a reviewer fuzzed the two parsers differentially over 30,095 inputs with no difference. A Tier-1 crate or binary with a `c-ffi`/`legacy-raw-ptr` entry is refused unless a `[[tier_exception]]` matches its crate, class and manifest. The allowlist `class` field is closed to six values. A file with an `extern` block must be `c-ffi`, `test-harness` or `test-violation`.
- **Signed tier exceptions** (approver `lungmat8 (owner, S8b approval 2026-11-16)`, `review_by` 2027-02-14): `service-net` (legacy-raw-ptr, LEGACY), `service-compositor` (legacy-raw-ptr, no manifest), `driver-bcm-display` (legacy-raw-ptr GpuFlush slice, LEGACY; `3c42aa8034`). `app-c-math-smoke` declares `PROTECTION_CLASS_FFI` instead, which takes `posix-shim` out of the closure.
- **Closure ratchet** (`024208bc5c`, `cd0b931ad6`, `5505096470`, `6b86d31d42`, `f157eb6c65`). `scripts/gen-tier1-closure.py` resolves the Tier-1 closure with `cargo metadata --all-features` for x86_64, riscv64 and aarch64-softfloat; CI checks it for drift. `scripts/cellos_sign/tcb.py` holds per-file ceilings for `unsafe` tokens and unsafe attributes (`#[no_mangle]`, `#[export_name]`, `#[link_section]`, `global_asm!`), plus the combined total. Third-party crates and proc-macros are pinned by name, version and full source. A rise fails everywhere; a drop fails CI strict and is a NOTE on `--sign` until `--write-baseline` lowers it. Computed `include!` paths are refused unless reviewed and pinned to the build script's hash (`service-silo`).
- **Lexer** (`a1d327b822`). A bare quote opens a literal wherever it stands, closing a pre-existing way to hide `unsafe` inside a macro call.
- **Allowlist hygiene** (`4b51fbf2b9`, `8fc0424b53`). 37 entries that would have gone stale before 2027-01-31, which stops all signing, were re-read site by site and given `review_by` 2027-02-14. Three tier2 witnesses moved to `test-violation`.
- **Specs.** Spec 16 (TCB table, F1b/F1c), Spec 13 (device ownership, NIC routes, `DmaRemapper`), Spec 18 §2.1 and Spec 15 (`d6a0b881d5`, `2e83c63c43`, `9ca68e22d2`, `33b35ed0d8`).

| Tier-1 perimeter | `unsafe` tokens | unsafe attributes |
|---|---|---|
| Tier-1 cells | 178 | 11 |
| In-tree closure (43 crates, 52 files) | 411 | 26 |
| Combined ceiling | 589 | 37 |

Third-party: 213 pins.

## 3. Defects found and fixed

| Severity | Defect | Fix |
|---|---|---|
| HIGH (pre-existing) | Any driver capability could take the NIC route (417) | `be7240ee0e` |
| HIGH (pre-existing) | Any PCIe driver could map another device's BAR (X1) | `5bf8d5b2f3` |
| HIGH (pre-existing) | Off x86, any driver could wait on or drain another driver's IRQ line (X2) | `d1092034dc` |
| HIGH (6–7 review) | The signer read the tier from source only; `cfg`-gated, uncalled-macro, auto-bin and lexer-hidden manifests signed while the ELF loaded Tier 1 | `5505096470` |
| HIGH (6–7 review, pre-existing) | Lexer let `unsafe` hide behind `0"/*"`/`0'"'` inside macro calls | `a1d327b822` |
| HIGH (6–7 review) | bcm-display allowlist entries went stale on 2026-11-22, stopping every image signing | `4b51fbf2b9` |
| MED (pre-existing) | `grant_window` took `SCHEDULER` under `RESOURCE_REGISTRY`, against the rank table | `978c801353` |
| MED (2–5 review) | Fixed 256-record device table could drop devices on large servers | `5ee8848e4e` |
| MED (2–5 review) | Runtime revoke left the NIC route held by the revoked task | `3ecff4b457` |
| MED ×5 (6–7 review) | Ceiling drops blocked every `--sign`; closure was default-features only; proc-macros unpinned; `global_asm!` uncounted; Tier-2 `cells/` libraries invisible | `5505096470` |
| LOW (pre-existing, physical-only NICs) | RX copy bounded by device-written length could overrun its page (igc, ixgbe, r8169); ring-programming errors ignored; absent device treated as found; Shutdown ignored | `facf6f5027`, `5264af5fe3` |
| LOW ×5 (2–5 review) | Per-IRQ lock scan, `repoint` on a paused row, logging under higher-ranked locks, VirtIO restart before reap, vmxnet3 alignment | `f4914b8c63`, `bec0d1acc6`, `5ee8848e4e`, `2e25717275`, `ff1670f0a3` |
| LOW ×4 (re-review) | 235 accepted any BDF; revoke race window; paused-row primary never promoted; stale comments | `7c119926a9`, `a7fcf9ee0a`, `24231eb1c4`, `314858ba76` |
| LOW ×8 (6–7 review) | Duplicate package names, git-rev pins, softfloat target, `include!` of build output, negative control off the git path, `extern` class, spec wording, Spec 13 symbol | `5505096470`, `9ca68e22d2` |
| LOW ×5 (final re-review) | 235 bus-range only; check-then-insert promotion; stale `REGISTRY` note; `--write-baseline` TOML parsing; computed `include!` and third-party build inputs | `82fde87197`, `63132e9a67`, `fe9805c98c`, `6b86d31d42`, `f157eb6c65` |
| Gate | `check-hal-boundaries.sh`: 694 `target_arch` lines outside `arch/` against a ceiling of 692 | `9a0f969185` (691) |

## 4. Lanes

Runner `~/worktrees/p16-cp/pr.sh` (now with a `nic2` lane), each run on a snapshot worktree. Logs under `~/worktrees/p16-cp/<run>/`.

| Run (commit) | Lanes | Result |
|---|---|---|
| `p18-a` (`272b32d0bf`, nine cells migrated) | kbuilds (13), hosttests (13), `x86bootp14` (17), `x86c2cp14` (7) | PASS |
| `p18-b` (`461c90c21f`, steps 2–5) | The above plus clippyx, `rvquick`, `rv64disk`, `a64boot` (8), `a64th` | PASS |
| `p18-c` (`9ca68e22d2`) | The above plus `nic2` | PASS. `nic2-x86` shows both registrations and both promotion directions for each pair; `a64th` serial log ends `ALL TESTS PASSED`. |
| `p18-d` (`f412eaa3ab`, final) | Same set | PASS: all 70 steps. `nic2-x86` passes; `a64th` serial log ends `ALL TESTS PASSED`. 2196 s. |

`nic2-x86` also passed 4/4 under TCG in the fixture agent's runs with `--test-threads=2`.

Known failures on `main`, not run or not regressions: `vfs-quota`, 2 `rust-std-promotion` tests, the `x86hv-iso` rollout guard, and `check-lanes.py` (`qemu-tier3-{chrome,display}-x86.sh` have no `lanes.toml` row).

Repo gates on the final head: `cellos-sign --check --strict`, `gen-tier1-closure.py --check`, both negative controls, `check-hal-boundaries.sh`, `check-kernel-residents.py`, `tools/mutable-globals --check`, `check-spec-anchors.py --check`, `check-catalog.py`, `check-host-test-crates.py`, `check-service-wire-literals.py`, `check-cell-va-layout.py`, `generate-code-metrics.py --check` pass. Host tests: kernel 753, `nic-driver` 18 (20 with `fault-inject`), `cellos-boot-config` 19, vmxnet3 13, signer tests 142 (2 skipped: no built cellbox).

## 5. Reviews

All reviews were independent, read-only `haily-reviewer` runs.

| Range | Verdict | Fixed in |
|---|---|---|
| `9e159f218b..461c90c21f` (steps 2–5) | PASS_WITH_RISK: 2 MED, 5 LOW | `5ee8848e4e`..`4952dfc932` |
| Fix round (re-review) | PASS: 5 LOW | `7c119926a9`..`314858ba76` (4); virtio-blk restart recorded below |
| `2288da9685..d6a0b881d5` (steps 6–7) | BLOCKED: 3 HIGH, 5 MED, 13 LOW | `a1d327b822`..`33b35ed0d8` |
| Both fix rounds (final re-review) | Signer PASS_WITH_RISK, kernel PASS: 7 LOW | `82fde87197`, `63132e9a67`, `fe9805c98c`, `6b86d31d42`, `f157eb6c65`; 2 recorded below |

## 6. Residuals

- **Traffic on one NIC at a time.** Only the primary route carries traffic. Concurrent traffic needs a read-only route-list syscall and a multi-interface net service (owner decision; ABI change).
- **Physical-only NICs** (igc, ixgbe, i40e, r8169, bcm57xx) cannot bind on any machine: no catalog grant, `services.toml` entry or kernel Ethernet ID row, and empty device ID lists. They are host-tested only. The S5 physical captures need those rows.
- **virtio-blk restart before reap.** A restarted virtio-blk skips its dead predecessor's slot as live because the slot's `DRIVER_OK` status is never reset at teardown. The retry helps virtio-net only. Unchanged from before Phase 18.
- **Renamed binaries.** A Tier-1 ELF signed under a name that is no `cells/` binary gets a NOTE from the ELF check; the source gate still checks its crate. Spec 16 F1c and Spec 18 record this.
- **Allowlist review wave due before 2027-02-03.** `--sign` treats stale entries as fatal. 15 entries go stale on 2027-02-04, 67 by 2027-02-09, then 109 entries plus the three tier exceptions on 2027-02-15. They need a real re-review before then (Phase 19 handoff).
- **Device-capability revoke** has no end-to-end host test; the route-release function behind it is tested.
- **`nic2-x86` CI cost.** The step builds a second cell set and kernel inside the 90-minute `qemu-x86_64-boot` job; not measured in CI.
- **aarch64 IOMMU teardown self-test** prints `IOMMU-TEARDOWN: SKIP` (no remapper); no lane checks it.
- Evidence ceiling: host and QEMU (TCG, KVM). No physical hardware.
