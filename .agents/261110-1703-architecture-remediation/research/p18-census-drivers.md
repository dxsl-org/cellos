# Phase 18 census, part 18a: drivers (S8b step 1, plan steps 2–5)

Measured read-only on worktree `p18-drivers` at `539401b08f`. All paths are relative to the repository root. **[INFERENCE]** marks a claim that was not read in the cited lines or not observed at runtime. Everything else was read at the cited lines. "Changed lines" means the count of `<`/`>` lines from `diff igc/<file> <cell>/<file>`.

Scope: the nine NIC cells and virtio-net (§1), NIC routing (§2), device ownership (§3), the DMA remapper (§4) and a two-NIC QEMU fixture (§5). The Tier-1 TCB perimeter (plan steps 6–7) is not covered here.

## 0. Headline findings

| # | Finding | Evidence |
|---|---|---|
| D1 | **The nine cells form two families, not one.** Five physical-only cells (`igc`, `ixgbe`, `i40e`, `r8169`, `bcm57xx`) are copies of one template: `dispatch.rs` differs by 6 lines and `main.rs` by 35–37 lines, all renames, constants and log strings. The four QEMU cells (`e1000`, `e1000e`, `igb`, `vmxnet3`) differ from that template by 142–239 lines in `main.rs`, but the logic is the same. | §1.1 |
| D2 | **The same bring-up, dispatch and sender gate exist in 11 copies.** The nine NIC cells, `virtio-net` and `dwc2-usb` each call `sender_is_live_provider(service::NET, …)` themselves. | `cells/drivers/igc/src/main.rs:131`; `cells/drivers/virtio-net/src/main.rs:85`; `cells/drivers/dwc2-usb/src/main.rs:577`; §1.4 |
| D3 | **`try_init_array` is copied eight times, with 3 `unsafe` lines each (24 lines).** The nine cells hold 102 non-comment `unsafe` lines in total. 38 of them are descriptor-ring and DMA-area accesses that a generic ring type can absorb. | §1.3 |
| D4 | **A latent probe bug sits in the five physical-only cells.** They treat `Ok(false)` ("device absent") from `sys_find_pcie_device_by_vendor` as found (`.is_ok()`). The four QEMU cells match `Ok(true)`. The bug is dormant only because all five have an empty `QUERY_DEVICE_IDS`. | `cells/drivers/igc/src/main.rs:46`; `cells/drivers/e1000e/src/main.rs:65`; `libs/ostd/src/syscall.rs:3150-3154`; `cells/drivers/igc/src/identity.rs:53` |
| D5 | **The five physical-only cells cannot bind on any machine at HEAD.** They are packaged in the x86 image, but they have no catalog `pcie_driver` row, no `services.toml` entry and no row in the kernel's Ethernet family gate. | `kernel/src/embedded-x86_64/manifest.json:26,89,103,117,173`; `kernel/src/catalog.rs:401-413`; `kernel/src/embedded-x86_64/boot-config/services.toml:9-32`; `kernel/src/dev/pcie_ecam.rs:40-50` |
| D6 | **The NIC route has two singletons, not one.** `NIC_DRIVER_STATE` holds the owner tid and the proven VirtIO IRQ. Separately, `service::NIC_DRIVER` in the service registry is last-writer-wins. The net service reads only the service registry. The kernel reads `NIC_DRIVER_STATE` only to decide whether an IRQ also signals `NET_RX`. | `kernel/src/dev/driver_cell.rs:61-78,250-262`; `kernel/src/cell/service_registry.rs:151-182`; `cells/services/net/src/interface.rs:91-96` |
| D7 | **Any `PcieDriverCap` or `UsbDriverCap` cell can take the NIC route.** `RegisterNicDriver` checks the capability and liveness only, not ownership of a network device. Bit 50 is shared with `FindPcieDevice`/`RegisterBlockDriver`, so `/bin/ahci`, `/bin/xhci` and `/bin/intel-gpu` all hold it. Block routes, by contrast, require an owned device. | `kernel/src/task/syscall/dev.rs:249-255` vs `:326-337`; `libs/abi/abi/syscalls.toml:1268-1275`; `cells/drivers/xhci/src/main.rs:168` |
| D8 | **Routing by BDF needs no new argument to 417.** The kernel can derive the key from the caller's owned network device. **Carrying traffic on a second NIC does need an ABI change**: the net service has no way to enumerate NIC routes, and it is single-interface. | §2.4 |
| D9 | **Two NICs of the same model also need a boot-config change.** `libs/boot-config` refuses two cells with the same path, and the family gate maps each Ethernet ID to exactly one program. | `libs/boot-config/src/lib.rs:356-371`; `kernel/src/catalog.rs:828-845` |
| D10 | **MMIO, IRQ and DMA ownership disagree in six places.** The worst one: `RequestMmio` grants any registered PCIe BAR to any `PcieDriverCap` cell without checking who owns the device's BDF. `QueryPcieFunction`, `AllocPcieIrq` and `GrantDma` all require BDF ownership. | §3.3 |
| D11 | **Off x86, `WaitIrq` checks no IRQ ownership.** A caller passing `mmio_base = 0`, or its own window with a foreign line, becomes the single waiter for another driver's line. | `kernel/src/task/syscall/dev.rs:761-779`; `kernel/src/dev/irq_wait.rs:80-92` |
| D12 | **VT-d is not the only remapper.** A RISC-V IOMMU backend (610 lines) sits behind the same arch dispatch. The plan's "VT-d as the only implementation" holds only for x86. | `kernel/src/arch/riscv64/iommu.rs:7-42`; `kernel/src/arch/riscv64/dev/iommu_riscv.rs`; `kernel/src/arch/riscv64/boot.rs:133` |
| D13 | **There are two empty ARM remapper designs, not one.** One is the HAL `ArmIommu`/`NoIommu` pair. The other is the kernel's `AppleDartPt`, which only its own host tests use. | `hal/arch/arm/src/aarch64/backend/mod.rs:624-636`; `kernel/src/dev/iommu_pt.rs:536-723` |
| D14 | **`iommu::map_dma` and `iommu::unmap_dma` have no callers.** | `kernel/src/dev/iommu.rs:117-123,145-148`; grep across `kernel/` finds only doc links |
| D15 | **Plan path drift.** The plan names `kernel/src/task/drivers/{driver_cell,iommu,iommu_x86*}.rs`. The files live at `kernel/src/dev/{driver_cell,iommu,iommu_pt}.rs` and `kernel/src/arch/x86_64/{iommu.rs,dev/iommu_x86*}`. The handoff request's §3 paths are correct. | `phase-18-driver-model-tcb.md` § Related Code Files |

## 1. NIC cells

### 1.1 Per-file diff matrix (changed lines against `igc`)

"—" means the file does not exist in that cell. Line counts come from `wc -l` at HEAD.

| File | ixgbe | i40e | r8169 | bcm57xx | e1000 | e1000e | igb | vmxnet3 |
|---|---|---|---|---|---|---|---|---|
| `main.rs` (igc 183) | 35 | 35 | 36 | 37 | 215 | 142 | 239 | 187 |
| `dispatch.rs` (igc 51) | 6 | 6 | 6 | 6 | 19 | 67 | 23 | 68 |
| `dma_layout.rs` (igc 100) | 2 | 19 | 2 | 8 | 85 | 131 | 92 | — (`dma.rs`) |
| `rings.rs` (igc 70) | 2 | 37 | 59 | 58 | — | 330 | — | 183 |
| `identity.rs` (igc 112) | 61 | 64 | 103 | 57 | 160 | 145 | 221 | 126 |
| `fake.rs` (igc 79) | 31 | 43 | 70 | 48 | — | 98 | — | — |
| `regs.rs` (igc 119) | 97 | 115 | 142 | 117 | — | 170 | — | 202 |
| `controller.rs` (igc 254) | 140 | 268 | 190 | 229 | 465 | 299 | 568 | 394 |

Chip-only modules: `ixgbe/{sfp,rx_tx}.rs`, `i40e/{aq,hmc}.rs`, `bcm57xx/ape.rs`, `{igc,r8169,bcm57xx,e1000e}/phy.rs`, `e1000e/{pch,reset,irq}.rs`, `igb/sync.rs`, `vmxnet3/{datapath,dma,irq,shared}.rs`.

### 1.2 What is identical and what is chip-specific

**Five-cell template (igc family).** In `main.rs` and `dispatch.rs`, the diffs against `igc` contain only these items:

- the doc line;
- `mod` lines;
- the controller type name;
- the vendor constant (`r8169`, `bcm57xx`);
- `*_BAR0_LEN` and `*_MMIO_MIN`;
- the log tag;
- `ixgbe` passing `identity.mac` to `new` (`cells/drivers/ixgbe/src/main.rs:89`);
- `sys_exit(1);` against `sys_exit(1)` (a missing semicolon).

`ixgbe` and `r8169` differ from `igc` in `dma_layout.rs` by the doc line only.

| Element | Shared shape | Chip-specific part | Evidence |
|---|---|---|---|
| Probe | 200 attempts over `QUERY_DEVICE_IDS`, `yield_now` between rounds | Vendor; ID list; `Ok(true)` against `.is_ok()` (D4) | `cells/drivers/igc/src/main.rs:44-52`; `cells/drivers/e1000e/src/main.rs:63-71` |
| Identity gate | `classify(vendor, device)` → exit 0 "driver gate closed" | `Option<Identity>` (igc, ixgbe, i40e, r8169, bcm57xx, e1000e, vmxnet3) or a `Sku` enum with `Unsupported` (e1000, igb) | `cells/drivers/igc/src/main.rs:61-67`; `cells/drivers/e1000/src/main.rs:84-91`; `cells/drivers/igb/src/main.rs:100-107` |
| BAR claim | BAR0 from `PcieDeviceInfo`, `len = min(info.bar0_len, BAR0_LEN)`, refuse `< MMIO_MIN`, `mmio::request_region` | e1000 claims a fixed 128 KiB with no minimum check. vmxnet3 claims BAR0 and BAR1 from the 434 record. | `cells/drivers/igc/src/main.rs:69-86`; `cells/drivers/e1000/src/main.rs:96-100`; `cells/drivers/vmxnet3/src/main.rs:64-77,106-111` |
| Controller init | `Controller::new(mmio, bdf[, identity])` → exit 1 on error | Constructor arguments | `cells/drivers/igc/src/main.rs:88-97`; `cells/drivers/e1000e/src/main.rs:104`; `cells/drivers/igb/src/main.rs:128` |
| Publish, then register | `STATE = Some(..)`, bind line, `sys_register_nic_driver()`; on error clear `STATE` and exit 1 | Bind-line suffix (lanes assert it, §5.1) | `cells/drivers/igc/src/main.rs:99-116`; `cells/drivers/vmxnet3/src/main.rs:118-137` |
| Absent device | Logs "no … controller present" and exits 0 | `e1000` exits **silently**: `igb-x86` asserts that no `[e1000]` text appears | `cells/drivers/e1000/src/main.rs:75-80`; `tests/integration/tests/igb-x86.rs:190` |
| Shutdown | e1000, e1000e, igb and vmxnet3 exit 0 on `Shutdown`/`ShutdownWith` | The five physical-only cells ignore it (`_ => {}`) | `cells/drivers/e1000/src/main.rs:179-181`; `cells/drivers/igc/src/main.rs:152` |
| Dispatch | `OP_TX` length check against `BUF_SIZE` (2048 in all nine), `OP_RX` 2-byte length prefix, `OP_GETMAC`; unknown op → `Failed` | `poll_rx() -> ViResult<usize>` with `unwrap_or_default` (igc family) or `recv_frame() -> usize` (others) | `cells/drivers/igc/src/dispatch.rs:16-51`; `cells/drivers/e1000/src/dispatch.rs:24-62` |
| Serve loop | `ipc::current()` reply or `sys_try_send` fallback, sender gate, lock `STATE`, map `NicReply` | Inline in `main.rs` (seven cells) or `dispatch::serve` (e1000e, vmxnet3) | `cells/drivers/igc/src/main.rs:122-151`; `cells/drivers/e1000e/src/dispatch.rs:74-96`; `cells/drivers/vmxnet3/src/dispatch.rs:75-97` |
| Manifest | `declare_manifest!` with every flag false (`PcieDriverCap` comes from the launch edge) | None | `cells/drivers/igc/src/main.rs:158-165` |
| Syscall allowlist | 15-entry base set (igc) | e1000e adds `AllocPcieIrq`, `WaitCompletion`. vmxnet3 adds those two plus `QueryPcieFunction` and `GrantFree`. | `cells/drivers/igc/src/main.rs:167-183`; `cells/drivers/e1000e/src/main.rs:169-187`; `cells/drivers/vmxnet3/src/main.rs:168-188` |
| Host-test seam | `RegisterFile { rd32, wr32 }`, defined once per cell | One trait in each of 7 NIC cells | `cells/drivers/igc/src/regs.rs:81-93`; `cells/drivers/e1000e/src/regs.rs:91`; `cells/drivers/vmxnet3/src/regs.rs:76` |

### 1.3 Duplicated `unsafe`

The figure counts non-comment lines containing `unsafe` in `cells/drivers/<cell>/src`. The total is 102: igc 11, ixgbe 11, i40e 13, r8169 9, bcm57xx 9, e1000 16, e1000e 12, igb 12, vmxnet3 9.

| Site | Copies | Lines | Shared form | Evidence |
|---|---|---|---|---|
| `try_init_array` (`MaybeUninit` array and drop guard) | 8 (all but vmxnet3) | 24 | Byte-identical logic. e1000, e1000e and igb add `// SAFETY:` comments. | `cells/drivers/igc/src/dma_layout.rs:5-33` (unsafe at `:16,21,30`); `cells/drivers/e1000/src/dma_layout.rs:17,23,35`; `cells/drivers/igb/src/dma_layout.rs:22,28,40` |
| Descriptor ring `clear` / `read_*_desc` / `write_*_desc` (volatile at `index` in a `DmaBuf`) | 6 | 29 | Same three operations over different `#[repr(C)]` descriptor types. `igc` and `ixgbe` `rings.rs` differ by 2 lines. | `cells/drivers/igc/src/rings.rs:35-58`; `cells/drivers/i40e/src/rings.rs:29-47`; `cells/drivers/r8169/src/rings.rs:25-38`; `cells/drivers/e1000e/src/rings.rs:63-203` |
| Frame copy into or out of a DMA buffer | 7 | about 14 | `copy_nonoverlapping` to `buf.virt()` | `cells/drivers/igc/src/controller.rs:192,235`; `cells/drivers/ixgbe/src/rx_tx.rs:19,62` |
| `unsafe impl Send for <Controller>` | 7 | 7 | Needed because the controller holds `DmaBuf`s | `cells/drivers/igc/src/controller.rs:34`; `cells/drivers/e1000/src/controller.rs:105`; `cells/drivers/igb/src/controller.rs:225` |
| Raw-pointer MMIO | e1000 only | 4 | Chip-local; the other cells use `MmioRegion` | `cells/drivers/e1000/src/controller.rs:110,115,140,147` |
| DMA area (`DmaArea`, `Sink`) | vmxnet3 only | 9 | Chip-local single-allocation layout | `cells/drivers/vmxnet3/src/dma.rs:21-114` |

Allowlist: each cell has a `[[crate]]` entry of class `driver-mmio` (`scripts/unsafe-allowlist.toml:829-946`). Its `[[file]]` entries list `dma_layout.rs` and `rings.rs` per cell (`scripts/unsafe-allowlist.toml:261-451,535`). Moving these sites into `libs/nic-driver` removes 24 + 29 cell lines and adds about 8 library lines (one `try_init_array`, one generic ring with three operations, two bounded frame copies, one `Send` wrapper). That fits the owner's ratchet-down rule **[INFERENCE: final counts depend on how many frame copies convert]**.

### 1.4 Sender authentication gate

There are 10 identical calls `sender_is_live_provider(service::NET, sender_tid)`:

- `cells/drivers/igc/src/main.rs:131`, `cells/drivers/ixgbe/src/main.rs:132`, `cells/drivers/i40e/src/main.rs:132`, `cells/drivers/r8169/src/main.rs:131`, `cells/drivers/bcm57xx/src/main.rs:132`;
- `cells/drivers/e1000/src/main.rs:157`, `cells/drivers/igb/src/main.rs:184`;
- `cells/drivers/e1000e/src/dispatch.rs:83`, `cells/drivers/vmxnet3/src/dispatch.rs:84`;
- `cells/drivers/virtio-net/src/main.rs:85`.

An 11th sits in `cells/drivers/dwc2-usb/src/main.rs:577`. The rule lives once in `libs/ostd/src/service/mod.rs:267-276`. Each cell repeats only the call and the refusal byte.

### 1.5 Registration, IRQ, DMA and wire

| Aspect | Today | Evidence |
|---|---|---|
| Registration | `sys_register_nic_driver()` → syscall 417 with no arguments, called after `STATE` is published | `libs/ostd/src/syscall.rs:2857-2865`; `cells/drivers/igc/src/main.rs:109`; `cells/drivers/e1000/src/main.rs:130`; `cells/drivers/igb/src/main.rs:157`; `cells/drivers/e1000e/src/main.rs:130`; `cells/drivers/vmxnet3/src/main.rs:133` |
| IRQ, seven cells | Polled; no `AllocPcieIrq` in their allowlists | `cells/drivers/igc/src/main.rs:167-183`; bind line `irq=polled reason=request-driven` (`tests/integration/tests/nic-x86.rs:95`) |
| IRQ, e1000e and vmxnet3 | MSI-X via 435 (`WANT_VECTORS = 3`), bounded park through `WaitCompletion(IRQ)` with spin-park-poll fallback; RX stays request-driven. `irq.rs` differs by 166 lines between the two, but the spin-park-poll skeleton is the same; only re-arming and the vector plan differ. | `cells/drivers/e1000e/src/irq.rs:53-160`; `cells/drivers/vmxnet3/src/irq.rs:34-160` |
| DMA, eight cells | Allocate rings and 16+16 one-page buffers (`DmaBuf::alloc`, `try_init_array`), authorize every slot (`DmaBuf::authorize` → `GrantDma`) before programming any address | `cells/drivers/igc/src/controller.rs:93-130`; `cells/drivers/igc/src/dma_layout.rs:72-100` |
| Authorization order | TxRing, TxBuf×16, RxRing, RxBuf×16. **i40e** authorizes AtqRing, ArqRing and AqBuffer first. **bcm57xx** adds StatusBlock last. | `cells/drivers/i40e/src/dma_layout.rs:86-89`; `cells/drivers/bcm57xx/src/dma_layout.rs:79-89` |
| Program order | igc family: TxRingBase, RxRingBase, RxBuffer×16 (i40e emits AtqBase/ArqBase first, bcm57xx emits StatusBlockBase third). e1000, igb and e1000e: TxRingBase, RxDescriptor×16, [RxRingBase], Enable. | `cells/drivers/igc/src/dma_layout.rs:61-70`; `cells/drivers/e1000e/src/dma_layout.rs:91-104` |
| DMA, vmxnet3 | One allocation holding `DriverShared`, queue descriptors, both rings and every buffer, authorized once; `GrantFree` on failure | `cells/drivers/vmxnet3/src/dma.rs:26-38` |
| Wire | `wire::proto::nic`: `OP_TX = 0` `[op, len u16, frame]`, `OP_RX = 1` → `[len u16, frame]`, `OP_GETMAC = 2` → 6 bytes; `NicStatus` Ok/Failed/NotReady | `libs/wire/src/proto/nic.rs:1-26` |

### 1.6 Host tests (kept per chip)

| Cell | `#[test]` sites by file | Registered in `scripts/host-test-crates.txt` |
|---|---|---|
| igc | fake 1, identity 3, phy 1, regs 1, rings 1 | `untested` (`:40`) |
| ixgbe | fake 1, identity 3, regs 1, rings 1, sfp 1 | `untested` (`:42`) |
| i40e | aq 3, fake 1, hmc 2, identity 3, rings 1 | `untested` (`:38`) |
| r8169 | fake 1, identity 3, phy 1, rings 1 | `untested` (`:45`) |
| bcm57xx | ape 3, fake 1, identity 3, phy 1, rings 1 | `untested` (`:33`) |
| e1000 | none; `test = false` (`cells/drivers/e1000/Cargo.toml:10`) | absent. Its `dma_layout.rs` is compiled by path into `tests/integration/tests/e1000-dma-layout.rs:1-7` (lane `e1000-dma-layout`, manual, `scripts/lanes.toml:525-528`). |
| e1000e | dma_layout 1, identity 4, irq 1, pch 6, phy 4, regs 5, reset 4, rings 6 | `ci` (`:35`) |
| igb | identity 3, sync 1 | `untested` (`:39`) |
| vmxnet3 | identity 1, irq 1, regs_tests 3, rings_tests 5, shared_tests 3 | `ci` (`:48`) |

The `e1000-dma-layout` integration test breaks when `cells/drivers/e1000/src/dma_layout.rs` is deleted. Its cases move to `libs/nic-driver` host tests.

### 1.7 virtio-net

- It shares the wire, the serve loop and the gate. Its `dispatch.rs` is the same `handle` with `FRAME_BUF = 1514` instead of 2048 and `NicReply::Mac` instead of `Raw` (`cells/drivers/virtio-net/src/dispatch.rs:10`; `cells/drivers/virtio-net/src/main.rs:47-113`).
- It shares no bring-up and no DMA layout. On x86 it probes with 424 and builds a transport from the 434 record through `libs/virtio-pci-transport` (`cells/drivers/virtio-net/src/pci_x86/mod.rs:34-36,106-117`; `libs/virtio-pci-transport/src/lib.rs:1-17`). On riscv64 and aarch64 it claims a VirtIO MMIO slot (`cells/drivers/virtio-net/src/mmio.rs:149-155`) and waits with `sys_wait_irq` (`:110`).
- `libs/virtio-pci-transport` has no NIC knowledge; virtio-blk and virtio-scsi also use it (`cells/drivers/virtio-blk/Cargo.toml:19`; `cells/drivers/virtio-scsi/Cargo.toml:16`).

### 1.8 Proposed `libs/nic-driver` and the `NicDevice` trait

The library owns probe, identity gate, BAR0 claim, publish-then-register, the serve loop, the sender gate, `OP_*` dispatch, shutdown, `try_init_array`, a generic descriptor ring, bounded frame copies, the authorize-before-program DMA layout and the MSI-X bounded wait. Each chip keeps its register model, PHY, reset, identity table, descriptor formats and programming order.

```rust
pub trait NicDevice: Sized + 'static {
    const TAG: &'static str;                    // "[igc]"
    const VENDOR: u16;
    const QUERY_IDS: &'static [u16];            // empty = Target-only (gated)
    const ABSENT_LINE: Option<&'static str>;    // None keeps e1000 silent
    const MAX_TX: usize = 2048;                 // virtio-net would use 1514
    type Identity: Copy;
    fn classify(vendor: u16, device: u16) -> Option<Self::Identity>;
    fn identity_name(id: &Self::Identity) -> &'static str;
    /// Claims BARs through `probe` (BAR0 helper or the 434 record) and brings
    /// the controller up. DMA addresses are programmed only inside here, after
    /// `nic_driver::dma::authorize_all` has returned every IOVA.
    fn init(probe: &Probe, id: Self::Identity) -> Result<Self, InitError>;
    fn mac(&self) -> [u8; 6];
    /// Text after "{TAG} controller bound vvvv:dddd {name} " — kept byte-exact
    /// because lanes assert it.
    fn bind_detail(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result;
    fn send(&mut self, frame: &[u8]) -> Result<(), TxError>;
    fn recv(&mut self, out: &mut [u8]) -> usize; // 0 = nothing ready
}
nic_driver::cell!(IgcController);   // STATE, handler, run_app!
```

Library components:

- **`dma::Layout<const TX, const RX, const E>`**: chip-declared extra slots, each with a position (`Before` or `After`), so i40e and bcm57xx keep their exact order. It authorizes every slot and returns `Iovas`. It does not emit programs: each chip's `init` programs registers itself.
- **`ring::DescRing<D: Copy>`**: `clear`, `read(i)`, `write(i, d)`.
- **`buf::FrameBufs<N>`**: `copy_in` and `copy_out`, bounds-checked.
- **`irq::BoundedWait`**: `setup(bdf, want)` and `wait_for(rearm, ready, parks, polls)`. e1000e and vmxnet3 pass their own `rearm`.
- **`regs::RegisterFile`**: replaces the seven per-cell copies of the trait, so the host fakes keep working.

`declare_manifest!` and `declare_syscalls!` stay in each cell, because the signed allowlists differ (§1.2).

**Escape hatches: none in the control flow.** Probe, gate, claim, register, serve and shutdown need no chip override. Three chips need the library to be more general than `igc`'s copy. None of these is an escape hatch:

1. **vmxnet3** claims BAR0 and BAR1 from the 434 record (`cells/drivers/vmxnet3/src/main.rs:106-111`). `Probe` exposes `function()`, and `init` claims its own BARs.
2. **i40e** and **bcm57xx** have extra DMA slots (`cells/drivers/i40e/src/dma_layout.rs:37-43,86-88`; `cells/drivers/bcm57xx/src/dma_layout.rs:44,89`). `Layout` takes declared extra slots.
3. **vmxnet3 does not use the shared DMA layout or ring.** Its single-allocation `DmaArea` and its 9 `unsafe` lines stay in the cell (`cells/drivers/vmxnet3/src/dma.rs`). This is the one place where the plan's "library holds the DMA ring layout" does not cover a chip.

**Behaviour changes that unifying the code brings, listed for §4 of the handoff:**

- The five physical-only cells gain `Ok(true)` probing (fixes D4) and exit on `Shutdown`.
- e1000 gains the `min(bar0_len)` and `MMIO_MIN` check. QEMU's e1000 BAR0 is 128 KiB, so nothing changes there **[INFERENCE]**.
- igc's `apply`/`finish_init` errors stop being ignored, if the library propagates them (`cells/drivers/igc/src/controller.rs:124-127`).

virtio-net may adopt `nic_driver::serve` (wire, gate, dispatch with `MAX_TX = 1514`) without the bring-up. That is optional and is not one of the owner-mandated nine.

## 2. NIC routing

### 2.1 Kernel state and every reader

| Item | Behaviour | Evidence |
|---|---|---|
| `NIC_DRIVER_STATE` | `Spinlock<NicDriverState { owner_tid, virtio_irq }>`, rank `NIC_DRIVER_STATE` | `kernel/src/dev/driver_cell.rs:61-78`; `kernel/src/sync/rank.rs:93` |
| Write | `register_nic_driver(tid)` overwrites both fields; the VirtIO IRQ is "proven" by MMIO ownership of a VirtIO slot whose `device_id` register is 1 | `kernel/src/dev/driver_cell.rs:250-262,97-114` |
| Clear | `deregister_nic_driver(tid)` only on an exact tid match, from `deregister_all_for` in `exit_task` | `kernel/src/dev/driver_cell.rs:264-270,357-365`; `kernel/src/sched/scheduler.rs:1796-1799` |
| Reader 1 | x86 MSI ISR: a vector owned by the NIC tid also calls `signal_net_rx()` | `kernel/src/arch/x86_64/dev/pcie_msi.rs:64-70` |
| Reader 2 | VirtIO MMIO ISR (riscv64/aarch64): the same `NET_RX` signal | `kernel/src/dev/virtio_common.rs:155-178` |
| Not a reader | The input service's gate uses `USB_HID_PRODUCER`, not the NIC role | `kernel/src/dev/driver_cell.rs:305-322`; `cells/services/input/src/main.rs:251` |
| Service binding | `register_driver_service(.., service::NIC_DRIVER, ..)` publishes `(tid, cell, generation)` under `ROLE_PUBLICATION`; registry `insert` replaces any existing row | `kernel/src/task/syscall/dev.rs:236-284,544-551`; `kernel/src/cell/service_registry.rs:151-182` |
| Service readers | The net service (`provider_tid(NIC_DRIVER)`, cached, invalidated on transport failure). The dwc2 LAN worker accepts only its parent registered as `NIC_DRIVER`. | `cells/services/net/src/interface.rs:56-100`; `cells/drivers/dwc2-usb/src/lan_worker.rs:31-43` |

The ABI is 417 `RegisterNicDriver`, with no arguments, gate bit 50 (`libs/abi/abi/syscalls.toml:1277-1285`; `libs/abi/abi/syscalls.golden.json:115`). `service::NIC_DRIVER = 10` (`libs/abi/src/syscall.rs:369-371`).

### 2.2 Block routes, for comparison

| Property | Block (416/432) | NIC (417) |
|---|---|---|
| Key | `DeviceKey { kind: PCI \| MMIO, id }` | none (singleton) |
| Ownership proof | `owner_of_bdf(id) == caller` or `lookup_mmio_owner(id) == caller` | none (D7) |
| Table | `[Option<BlockRouteSlot>; MAX_BLOCK_ROUTES = 8]`, append-only keys, leaf lock, no allocation | one struct plus one registry row |
| Rebind | Same key: an empty provider or the same tid may rebind; a live foreign provider is refused | Always overwrites |
| Teardown | Clears the provider, keeps the key | Clears the owner (exact tid) |
| Reader | `LookupBlockRoute` (432) by BlockIoCap/SpawnCap, record `ViBlockRouteV1` | `LookupServiceBound(NIC_DRIVER)` |
| Evidence | `kernel/src/task/syscall/dev.rs:300-347,440-460`; `kernel/src/dev/driver_cell.rs:23-39,137-242`; `libs/abi/src/block_route.rs:15-60` | §2.1 |

### 2.3 How the net service picks a NIC

It caches the tid from `provider_tid(service::NIC_DRIVER)` and re-resolves after a failure (`cells/services/net/src/interface.rs:73-100`). It owns exactly one smoltcp `Interface` over one `VirtioNetDevice` (`cells/services/net/src/net_state.rs:29-30,61,95`). The net service is therefore single-interface. With two NICs registered today, the second registration wins both singletons and the first NIC silently stops carrying traffic.

### 2.4 BDF routing without an ABI change: what it covers and what it does not

**Registration needs no ABI change.** At 417 the kernel can derive the route key from the caller's owned network device:

- PCI: an owned BDF (`kernel/src/dev/pcie_ecam.rs:1330-1336`) whose class is `02:00:00`;
- VirtIO MMIO: the owned slot whose `device_id` is 1 (`kernel/src/dev/driver_cell.rs:97-114`);
- DWC2 (RPi3 LAN9514): the owned `CLASS_DWC2` window (`kernel/src/resource_registry.rs:363-389`).

Exactly one such device gives the key. Zero is refused, which closes D7. More than one is refused; no in-tree cell owns two **[INFERENCE: checked for the nine NIC cells, virtio-net and dwc2-usb only]**. The table becomes a fixed `[Option<NicRoute { key, provider, virtio_irq }>; 8]` with the block-route rebind rules, and stays allocation-free for the ISR readers. `owns_registered_nic_irq` checks every live route. The existing log prefix `[driver_cell] NIC driver registered` is kept and gains a ` device=… route=N` suffix, because lanes match the prefix (`tests/integration/tests/nic-x86.rs:124,164,239`).

**Traffic on a second NIC needs an ABI change, and work outside the Phase 18 file set.**

1. **A NIC route read path.** The net service can name only one provider (`service::NIC_DRIVER`). Neither 432 (readers are BlockIoCap/SpawnCap; kinds are block-only) nor new service IDs fit without a `libs/abi` change. A minimal form is a new syscall `LookupNicRoute` (next free number after 442 in the golden table, `libs/abi/abi/syscalls.golden.json`) returning a `ViBlockRouteV1`-shaped record, readable by the live `service::NET` provider only.
2. **Net service multi-interface.** One smoltcp `Interface` and one device per route, with distinct subnets. `cells/services/net` is not in the §3 file set.
3. **Same-model pairs need two instances of one program path.** `libs/boot-config` refuses duplicate `path` (`libs/boot-config/src/lib.rs:356-371`), and the family gate maps each ID to one program (`kernel/src/dev/pcie_ecam.rs:40-50`; `kernel/src/catalog.rs:828-845`). Nothing in the loader refuses a second live incarnation of one path **[INFERENCE: grep of `kernel/src/loader` and `kernel/src/catalog.rs` found no singleton rule]**.

Without 1–3, the reachable result is: both NICs registered on their own routes; neither registration displaces the other's route or IRQ; `service::NIC_DRIVER` names one *primary* route; and only the primary carries traffic. Choosing the primary needs a rule. "First live registration keeps the binding" can be done in the registration path. Re-publishing a survivor when the primary dies would need a registry insert from `exit_task`, which may run in an ISR, and `insert` allocates (`kernel/src/cell/service_registry.rs:173-182`). So the primary is re-bound only when its own replacement registers.

## 3. Device ownership

### 3.1 Rules today

| Rule | Key | Owner identity | Checked at | Released at |
|---|---|---|---|---|
| PCIe device claim (`BDF_OWNERS`) | BDF (u32) | tid | `claim_bdf_owner` in `offer_pcie_device`, which runs only when 418/424 hand out a device (`kernel/src/resource_registry.rs:1062-1076`; `kernel/src/task/syscall/dev.rs:414`) | `release_bdfs_for(tid)` inside `teardown_dma_authority`, only after quiesce and IOMMU revoke are acknowledged (`kernel/src/task.rs:1869`) |
| Device handout | BDF and `usable` | tid | `pcie_usable_by` (free or self) (`kernel/src/task/syscall/dev.rs:158-160,596-600`); family gate (`kernel/src/dev/pcie_ecam.rs:1306-1327`); catalog PCI row (`kernel/src/task/syscall/authority.rs:125-138`) | — |
| MMIO window (`REGISTRY`) | base address | tid (`MmioRegion.owner_tid`) | `request_mmio` overlap scan (`kernel/src/resource_registry.rs:400-462`); `lookup_mmio_owner` (`:860`) | `release_mmio_for(tid)`, after DMA teardown succeeds (`kernel/src/task.rs:1940-1947,1902`), via `RELEASING`/`QUARANTINED` (`kernel/src/resource_registry.rs:497-522`) |
| PCIe BAR admissibility (`PCIE_BARS`) | base → len, **no device identity** | none | `is_pcie_bar` and `PCIE_BARS` lookup (`kernel/src/resource_registry.rs:255-261,405-411`) | never (boot-lifetime) |
| BAR registration | base, len | Platform cell / boot | ECAM scan (`kernel/src/dev/pcie_ecam.rs:1084,1189`); `RegisterPcieBar` 235 accepts any shape-valid window (`kernel/src/task/syscall/dev.rs:797-806`; `kernel/src/resource_registry.rs:219-250`); VirtIO MMIO slots are registered as "PCIe BARs" (`kernel/src/dev.rs:72-85`) | never |
| MSI/MSI-X (x86) | (BDF, owner) → vector range | tid | `allocate` requires `owner_of_bdf == tid` (`kernel/src/arch/x86_64/dev/pcie_msi.rs:87-95,105-176`); `irq_owned_by` (`:266-275`) | `release_for(tid)` after quiesce (`kernel/src/task.rs:1861-1862`; `kernel/src/arch/x86_64/dev/pcie_msi.rs:330-345`) |
| MSI-X table pages | BAR pages | — | Carved out of the user mapping of a BAR grant (`kernel/src/task/syscall/dev.rs:1075-1083`; `kernel/src/arch/x86_64/dev/pcie_msi.rs:350`) | — |
| IRQ waiter | IRQ line (u8) | tid (single waiter) | x86: `irq_owned_by`. Elsewhere: only `mmio_base == 0` or a window the caller owns (`kernel/src/task/syscall/dev.rs:47-49,743-779`); `register_waiter` CAS (`kernel/src/dev/irq_wait.rs:80-92`) | `release_waiters_for(tid)` in reap (`kernel/src/task.rs:1948`; `kernel/src/dev/irq_wait.rs:167-190`) |
| DMA grant | (tid, BDF, phys range) | tid for the BDF; incarnation for quota | `owner_of_bdf == caller` (`kernel/src/task/syscall/grant.rs:1213`); grant ownership by `(cell, generation)` (`:1245-1255`); DMA quota charged to `Charge(cell, generation)` (`:1259,1284`) | `revoke_dma_for_cell(tid)` (`kernel/src/task.rs:1863`) |
| IOMMU domain | tid per unit; a context entry per BDF points at the tid's domain | tid | VT-d `domains.get(&tid)` (`kernel/src/arch/x86_64/dev/iommu_x86.rs:242-266`); RISC-V `RISCV_DOMAINS` keyed by tid (`kernel/src/arch/riscv64/dev/iommu_riscv.rs:79,405-414`) | `unmap_cell_domain` / `unmap_cell` (`kernel/src/arch/x86_64/dev/iommu_x86.rs:345`; `kernel/src/arch/riscv64/dev/iommu_riscv.rs:471`) |
| DMA export (437–439) | export id → holder window | holder tid; holder `Incarnation` for quota | `owns_bdf(tid, bdf)` (`kernel/src/task/syscall/dma_export.rs:1004-1006`) | `holder_domain_revoked(tid)` and `reap_owner(tid)` (`kernel/src/task.rs:1868,1939`) |
| Bus master | BDF | — | Enabled on the first successful `GrantDma` (`kernel/src/task/syscall/grant.rs:2112`); memory decode enabled at handout (`kernel/src/task/syscall/dev.rs:421-423`) | Quiesce (`kernel/src/dev/pcie_ecam.rs:891-957`; `kernel/src/task.rs:1833-1858`) |
| Block route | `DeviceKey` | (tid, cell, generation) | ownership of the key (`kernel/src/task/syscall/dev.rs:326-337`) | `clear_block_route_provider(tid)` in `exit_task` (`kernel/src/dev/driver_cell.rs:236-242`) |
| NIC role | none | tid; registry row (tid, cell, generation) | capability and liveness only (`kernel/src/task/syscall/dev.rs:249-255`) | `deregister_all_for(tid)` (`kernel/src/dev/driver_cell.rs:357-365`) |

Teardown order today: `exit_task` clears roles and routes (`kernel/src/sched/scheduler.rs:1799`). Reap then runs export revoke, then `teardown_dma_authority` (quiesce each owned BDF → MSI release → IOMMU revoke → export holder revoke → BDF release → acked frames), then MMIO release on success or a queued retry on failure, then IRQ waiter release (`kernel/src/task.rs:1828-1876,1930-1954`). Lock order is documented at `scripts/mutable-globals.toml:1330`.

### 3.2 Owner identity

Every device rule keys on the **claiming task id**. Only the DMA quota (`Charge`) and the route and registry records carry the incarnation `(cell, generation)`. The comment on `BDF_OWNERS` is stale: it says kernel NIC/NVMe drivers call `map_dma_for_cell(0, ..)` (`kernel/src/resource_registry.rs:1047-1052`), and no such caller exists (D14).

### 3.3 Where one rule allows what another refuses

| # | Case | Allowed by | Refused by |
|---|---|---|---|
| X1 | A `PcieDriverCap` cell claims and user-maps another device's BAR before that device's driver does; it can reset or reprogram it | `RequestMmio` PCIe path: "may claim any BAR discovered" (`kernel/src/task/syscall/dev.rs:1132-1149`; `kernel/src/resource_registry.rs:405-411`) | 434, 435 and `GrantDma` (BDF owner) (`kernel/src/arch/x86_64/dev/pcie_msi.rs:91`; `kernel/src/task/syscall/grant.rs:1213`) |
| X2 | Off x86, a driver waits on another driver's IRQ line (`mmio_base = 0`, or its own base with a foreign line) and consumes its pending flag | `wait_irq_base_permitted` plus single-waiter CAS (`kernel/src/task/syscall/dev.rs:761-779`) | x86 `irq_owned_by` (`kernel/src/task/syscall/dev.rs:768-771`) |
| X3 | A replacement claims a BAR window whose dead owner's DMA teardown is still pending. The overlap scan skips a `Terminated` or absent owner, and `insert` at the same base overwrites the record | `kernel/src/resource_registry.rs:436-443,453-460` | BDF claim (dead tid still holds it until teardown is acknowledged; `kernel/src/task.rs:1855,1869`) **[INFERENCE: depends on when the task leaves `Terminated` relative to reap]** |
| X4 | A cell without any NIC device takes the NIC route and receives every frame the net service sends | 417 (`kernel/src/task/syscall/dev.rs:249-255`) | 416 for block (`:326-337`) |
| X5 | Two threads of one incarnation: thread B cannot use the BDF thread A claimed, yet both are charged to one DMA account | tid checks (§3.1) | incarnation quota (`kernel/src/task/syscall/grant.rs:1259`) |
| X6 | One tid owning two devices gives both devices one IOMMU domain, so each can DMA to the other's buffers | per-tid domain (`kernel/src/arch/x86_64/dev/iommu_x86.rs:242-266`) | — (no per-device rule exists) |

`PCIE_BARS` holding Platform-registered windows (`kernel/src/task/syscall/dev.rs:797-806`) and VirtIO slots (`kernel/src/dev.rs:72-85`) with no device identity is the root of X1 for the PCI path and of X2's slot/line mismatch.

### 3.4 Proposed `DeviceRecord` (`kernel/src/dev/ownership.rs`)

```rust
pub(crate) enum DevKey { Pci { seg: u16, bdf: u16 }, Mmio { base: usize } }
pub(crate) struct Owner { tid: usize, cell: u64, generation: u64 }   // claimant + incarnation
pub(crate) enum Life { Free, Owned(Owner), Releasing(Owner), Quarantined(Owner) }
pub(crate) struct DeviceRecord {
    key: DevKey,
    class: u32,                         // PCI class code, or VirtIO/DWC2 kind
    life: Life,
    bars: [Option<(usize, usize)>; 6],  // from the scan/registration; Mmio: the slot window
    irq: IrqSet,                        // x86: Option<PcieIrqAlloc>; MMIO: Option<u8> slot line
    dma: DmaState,                      // domain key; bus-master enabled
}
static DEVICES: Spinlock<[Option<DeviceRecord>; MAX_DEVICES]>;  // fixed array: teardown may run in an ISR
```

| Operation | Rule against the record |
|---|---|
| 418/424 handout | `Free` → `Owned(caller)` (replaces `claim_bdf_owner`) |
| `RequestMmio` | The window must lie in `bars` of a record the caller owns. The first claim of a VirtIO/DWC2 window creates `Owned`. SoC allowlist windows (GPIO, UART, …) stay as today. Closes X1. |
| `WaitIrq` | The line must be in `irq` of a record the caller owns, on every arch. Closes X2. |
| 434/435 | Owned record; 435 stores its allocation in `irq` |
| `GrantDma` / `MapDmaExport` | Owned record; domain key from `dma` |
| 416/417 | Route key = owned record of the right class (closes X4) |
| Teardown | One `release(owner)`: quiesce → MSI → IOMMU → BARs (lower) → `Free`. On failure → `Quarantined`, retried. This keeps today's order (§3.1). Closes X3: a record in `Releasing`/`Quarantined` refuses every claim. |

**Migration order** (one check at a time, each a commit):

1. Records from the scan, `register_device`, VirtIO slots and DWC2. `BDF_OWNERS` functions become views over the records. No behaviour change.
2. DMA, 434, 435 and export read the record. No behaviour change: they already require the BDF owner.
3. IRQ: the x86 allocation moves into the record; non-x86 `WaitIrq` requires line ownership. The only in-tree non-x86 waiter, `virtio-net` MMIO, already waits on its own slot (`cells/drivers/virtio-net/src/mmio.rs:110`).
4. MMIO: a BAR grant requires device ownership. Every in-tree PCI driver receives its device from 418/424 before claiming a BAR **[INFERENCE: true of the nine NIC cells, vmxnet3's 434 path and virtio-net; not re-read for ahci, nvme, xhci, intel-gpu, pvscsi, virtio-blk, virtio-scsi, virtio-gpu]**.
5. Routes (block and NIC) keyed from the record.
6. Teardown through `release`. `PCIE_BARS` and `BDF_OWNERS` are deleted. `scripts/mutable-globals.toml:1802,1846,1857` change.

Keep the **tid** as the check key through step 6 so behaviour stays the same. Switching the check to the incarnation (fixes X5) widens a device's authority to sibling threads: open question 3.

## 4. DMA remapper

### 4.1 Current interface

| Layer | Items | Evidence |
|---|---|---|
| Generic (`dev::iommu`) | `init(DmaIsolationInput) -> Probed`, `activate_isolation(Probed) -> Isolation`, `map_dma_for_cell(tid, bdf, phys, size)`, `revoke_dma_for_cell(tid)`, `is_active`, `set_active`, deferred init (riscv64), `dma_without_remapper`, `classify_dma_publication` (riscv64 only); `map_dma` and `unmap_dma` are dead | `kernel/src/dev/iommu.rs:15-259` |
| Arch dispatch | `init`, `map_inactive`, `map_range_for_cell`, `unmap_range_for_cell`, `revoke_cell`, `activate`, `is_present`, `virt_to_phys`, `phys_to_virt`, `sync_table` in four files | `kernel/src/arch/x86_64/iommu.rs:14-98`; `kernel/src/arch/riscv64/iommu.rs:7-61`; `kernel/src/arch/aarch64/iommu.rs:8-62`; `kernel/src/arch/legacy/iommu.rs` |
| Backends | VT-d (`kernel/src/arch/x86_64/dev/iommu_x86.rs` 543 lines plus 11 submodules, 1922 lines); RISC-V IOMMU (`kernel/src/arch/riscv64/dev/iommu_riscv.rs` 610 lines); aarch64 and 32-bit refuse every map | `kernel/src/arch/aarch64/iommu.rs:13-23` |
| Shared page tables | `Sv39IommuPt` (RISC-V), `VtdSlpt` (VT-d), `AppleDartPt` (no backend) in one generic file | `kernel/src/dev/iommu_pt.rs:1-8,39-182,184-379,536-723` |
| Callers | `GrantDma` (`kernel/src/task/syscall/grant.rs:2091`); teardown (`kernel/src/task.rs:1863`); x86 boot (`kernel/src/arch/x86_64/boot.rs:592-597`); `is_active` readers (`kernel/src/arch/x86_64/dev/pcie_msi/record.rs:118`; `kernel/src/arch/x86_64/dev/pcie_msi/virtio_gate.rs:54`; `kernel/src/machine_facts.rs:24`) | — |
| Side doors to VT-d, bypassing the interface | DMA export (`bdf_translating`, `map_window_for_cell`, `unmap_window_for_cell`) under `cfg(x86_64)` (`kernel/src/task/syscall/dma_export.rs:1008-1071`); MSI interrupt remapping (`kernel/src/arch/x86_64/dev/pcie_msi/program.rs:45`; `kernel/src/arch/x86_64/dev/pcie_msi.rs:83`); p01 trace (`kernel/src/p01_noncpu.rs:278`) | — |

### 4.2 VT-d-only items in the generic interface

| Leak | Evidence |
|---|---|
| `DmaIsolationInput { vtd_base, vtd_units, vtd_include_pci_all }` is the generic `init` argument | `kernel/src/dev/iommu.rs:20-28` |
| `sync_table(virt, len, coherent)` ("non-snooping unit") is in every arch shim | `kernel/src/arch/x86_64/iommu.rs:91-98`; `kernel/src/arch/aarch64/iommu.rs:57-62`; `kernel/src/arch/riscv64/iommu.rs:56-61` |
| `VtdSlpt` (with `coherent`) lives in generic `dev/iommu_pt.rs` | `kernel/src/dev/iommu_pt.rs:184-206` |
| The `activate_isolation` docs name VT-d context entries | `kernel/src/dev/iommu.rs:190-197` |
| Export windows exist only as VT-d free functions, not on the interface | `kernel/src/task/syscall/dma_export.rs:1038-1071` |

### 4.3 HAL `ArmIommu`: what deletion touches

| File | Lines | Content |
|---|---|---|
| `hal/arch/arm/src/aarch64/backend/mod.rs` | 83-84 | `type Iommu: ArmIommu;` in `ArmPlatform` |
| same | 624-636 | `trait ArmIommu { const PRESENT }`, `struct NoIommu`, `impl` |
| `hal/arch/arm/src/aarch64/backend/aarch64_vm.rs` | 22, 35 | import and `type Iommu = NoIommu;` |
| `hal/arch/arm/src/aarch64/backend/apple_t8103.rs` | 14, 64 | same |
| `hal/arch/arm/src/aarch64/backend/bcm2711.rs` | 8, 19 | same |
| `hal/arch/arm/src/aarch64/backend/bcm2837.rs` | 18, 30 | same |
| `hal/arch/arm/src/aarch64/backend/qemu_virt.rs` | 9, 20 | same |

No code reads `<Board as ArmPlatform>::Iommu` or `PRESENT` (grep of `kernel/` and `hal/`). `DMA_GRANULE` (`hal/arch/arm/src/aarch64/backend/mod.rs:85-86`) is used by `GrantDma` (`kernel/src/task/syscall/grant.rs:2044-2045`) and stays. The only other mention is a historical report (`.agents/261031-1241-aarch64-core-backend-split/reports/phase-01-gate.md:14,33`), left as is. The same rule ("delete until a backend exists") applies to `AppleDartPt` and its host tests (`kernel/src/dev/iommu_pt.rs:536-723`, tests from `:725`).

### 4.4 Proposed `DmaRemapper`

```rust
// kernel/src/dev/iommu.rs — generic; no firmware or table type in the signature
pub(crate) trait DmaRemapper: Sync {
    fn state(&self) -> Remapper;                                   // Translating | Inactive | NotFound
    fn map(&self, dom: DomainKey, dev: Bdf, phys: u64, len: usize) -> DmaMapResult;
    fn map_window(&self, dom: DomainKey, dev: Bdf, iova: u64, phys: u64, len: usize, writable: bool) -> DmaMapResult;
    fn unmap_window(&self, dom: DomainKey, iova: u64, len: usize) -> bool;
    fn revoke(&self, dom: DomainKey) -> bool;
    fn translating(&self, dev: Bdf) -> bool;
}
pub(crate) fn remapper() -> &'static dyn DmaRemapper;   // or a per-arch type alias, static dispatch
```

- `DomainKey` is the tid today and the `DeviceRecord` owner later. `Bdf` carries the segment.
- **Moves behind VT-d**: the DMAR inputs (x86 `boot.rs` probes `iommu_x86` directly), `sync_table`/`coherent`, `VtdSlpt`, IR/`remap_msi` (interrupt remapping stays arch-private: it is not DMA), `virt_to_phys`/`phys_to_virt` (HHDM).
- **Moves behind RISC-V**: `Sv39IommuPt`, `classify_dma_publication`, deferred init.
- **Generic**: `dma_without_remapper` (board policy), `map_inactive`'s refuse-or-identity decision, and a `NoRemapper` for aarch64 and 32-bit.
- **Deleted**: `map_dma`, `unmap_dma`, `unmap_range_for_cell` in all shims (D14), `AppleDartPt`, `ArmIommu`.
- DMA export then works through the trait instead of `cfg(x86_64)` free functions. Off x86 it keeps returning `Rejected` (`kernel/src/task/syscall/dma_export.rs:1055-1059`).

## 5. Two-NIC QEMU fixture

### 5.1 Existing x86 NIC lanes

| Lane (`scripts/lanes.toml`) | Test file | Models | Proof |
|---|---|---|---|
| `nic-x86` (`:656-658`, pr) | `tests/integration/tests/nic-x86.rs` | `e1000`, `e1000-82545em`, `e1000e`, each ± VT-d (`:268-296`) | Bind line + irq posture, `[driver_cell] NIC driver registered`, `[net-bridge] first e1000 TX`/`RX`, `[net] IP address:` (SLIRP DHCP), ordering, exactly one bind and one registration (`:116-185,208-266`) |
| `igb-x86` (`:584-586`, pr) | `tests/integration/tests/igb-x86.rs` | `igb` ± VT-d | `[igb] controller bound 8086:10c9`, no `[e1000]` text (`:39,190`) |
| `vmxnet3-x86` (`:812-814`, pr) | `tests/integration/tests/vmxnet3-x86.rs` | `vmxnet3` ± VT-d | Bind line, no sibling bind (`:18-23,90-97`) |
| `virtio-x86` | `tests/integration/tests/virtio-x86.rs` | `virtio-net-pci` | `[virtio-net] ready` (`tests/integration/src/lib.rs:190-215`) |
| `e1000-dma-layout` (`:525-528`, manual) | `tests/integration/tests/e1000-dma-layout.rs` | host only | Layout ordering |

QEMU shape: `-netdev user,id=net0,restrict=on -device <model>,netdev=net0`, plus NVMe, on q35 (`tests/integration/src/lib.rs:1514-1538`).

### 5.2 Driver restart today

- NIC cells are `restart = "never"` (`kernel/src/embedded-x86_64/boot-config/services.toml:9-32`). init honours `Never`/`OnFailure`/`Always` (`cells/tools/init/src/supervisor.rs:169-172`).
- The suite shell cannot kill a cell (`cells/tools/cellbox/src/commands.rs:687-691`). `KillCell` needs SupervisorCap (`libs/abi/abi/syscalls.toml:1239-1245`).
- The precedent is the block fault ISO. `driver-ahci`/`driver-nvme` are built with `fault-inject`, a magic payload makes the driver exit, and init restarts it (`scripts/x86/make-ahci-fault-iso.sh:1-13,50-56`; `tests/integration/tests/ahci-x86.rs:443-446,492-497`). The two-disk independence test kills one of two routes and checks the other keeps serving (`tests/integration/src/block_09c_two_disks.rs:1-4,99`; `tests/integration/tests/ahci-x86.rs:607-616`). Block drivers are supervised as `OnFailure` by init code (`cells/tools/init/src/boot.rs:376-396`), not through `services.toml`.

### 5.3 Fixture plan (`nic2-x86`)

**Image.** A NIC fault ISO, modelled on `make-ahci-fault-iso.sh`, with:

- `nic-driver/fault-inject`: `serve` exits 1, without quiescing, when a TX frame contains `F18-DIE`. One implementation covers every chip.
- its own boot config (`CELLOS_CONFIG_DIR`) that sets the NIC cells to `restart = "on-failure"`.

**QEMU.**

```
-netdev user,id=n0,restrict=on,net=10.0.2.0/24 -device e1000e,netdev=n0,mac=52:54:00:00:00:01
-netdev user,id=n1,restrict=on,net=10.0.3.0/24 -device igb,netdev=n1,mac=52:54:00:00:00:02     # different models
-netdev user,id=n1,restrict=on,net=10.0.3.0/24 -device e1000e,netdev=n1,mac=52:54:00:00:00:02  # same model
```

**Markers and checks.**

| Step | Without ABI change (reachable) | With the ABI change and net multi-interface (§2.4) |
|---|---|---|
| Both bound | Two `[driver_cell] NIC driver registered: tid=… device=pci-00-0X-0 route=0/1` lines, two bind lines | same |
| Traffic | DHCP on the primary only (`[net] IP address: 10.0.2.15`) | One per interface, e.g. `[net] if0 IP address: 10.0.2.15` and `[net] if1 IP address: 10.0.3.15` |
| Restart one | Shell `nc` to the primary's subnet with `F18-DIE` → `[pcie] quiesced 00:0X.0 for tid T` (`kernel/src/task.rs:1836`) → `Init: service restarted.` → the replacement registers **route 0** again | Kill either NIC by sending to its subnet |
| Other stays bound | No new registration or provider change logged for route 1; no "route 1" teardown line | Plus a renewed exchange on the other interface after the restart |
| Hygiene | No `[KERNEL PANIC]`, `[fault] Cell`, `restart storm`, `not quiescent` (as in `tests/integration/tests/nic-x86.rs:174-179`) | same |

**Blockers.**

- The same-model case needs the boot-config duplicate-path change (§2.4.3).
- "Traffic on each" needs the ABI read path and a net service with one interface per NIC (§2.4.1–2).
- Physical-only NICs stay host-test only (handoff §5).
- Whether `nc` in the suite shell can carry a UDP payload (`cells/tools/cellbox/src/ledger.rs:141` lists it as a NetTool) was not checked **[INFERENCE]**. `ping` is the fallback trigger if the fault matches ICMP payload bytes.

## Design proposal

1. **`libs/nic-driver`** (§1.8). Order: library with host tests (moving in the `e1000-dma-layout` cases) → `igc` and `r8169` → `ixgbe`, `i40e`, `bcm57xx` → `e1000`, `igb`, `e1000e` → `vmxnet3` (bring-up, serve and IRQ only). The QEMU lanes `nic-x86`, `igb-x86` and `vmxnet3-x86` gate the last two groups. Delete each cell's `dispatch.rs` and `dma_layout.rs` copies, and their unsafe-allowlist `[[file]]` rows (`scripts/unsafe-allowlist.toml:261-451`). Add a `libs/nic-driver` row to `scripts/host-test-crates.txt` and the root `Cargo.toml`.
2. **NIC routes by device key** (§2.4). 417 keeps no arguments. The kernel derives the key from the caller's single owned network device, refuses a caller without one, keeps per-route IRQ proof, and keeps `service::NIC_DRIVER` on the first live route (the primary). No `libs/abi`/`libs/types` change.
3. **`DeviceRecord`** (§3.4), migrated in the order of §3.4. Checks stay keyed by tid.
4. **`DmaRemapper`** (§4.4): VT-d and the RISC-V IOMMU behind it, `NoRemapper` elsewhere. Delete `ArmIommu`, `AppleDartPt`, `map_dma` and `unmap_dma`.
5. **`nic2-x86`** fixture (§5.3), reachable part first.

## Risks

- **Traffic on two NICs cannot be met without an ABI change.** The success criterion "traffic on each" (`phase-18-driver-model-tcb.md` § Success Criteria) is out of reach under the handoff's no-ABI rule. Proposal 2 delivers both NICs bound plus traffic on the primary.
- **Behaviour changes outside handoff §4**:
  - 417 refused for a caller that owns no network device (D7);
  - non-x86 `WaitIrq` refused for a foreign line (X2);
  - a BAR claim refused for a device the caller does not own (X1);
  - the five physical-only cells exit on `Shutdown` and probe with `Ok(true)` (§1.8).

  All are refusals of authority no in-tree cell uses legitimately **[INFERENCE for X1, see §3.4 step 4]**.
- **The unified bring-up must keep every bind line byte-exact**, and must keep e1000's silence when absent. The lanes assert both (`tests/integration/tests/nic-x86.rs:92-114`; `tests/integration/tests/igb-x86.rs:190`).
- **The five physical-only cells remain unbindable after migration** (D5). Their S5 captures also need catalog, family-gate and `services.toml` rows, which are out of Phase 18 scope.
- **ISR constraints.** `owns_registered_nic_irq` runs in the MSI and VirtIO ISRs (`kernel/src/arch/x86_64/dev/pcie_msi.rs:67`; `kernel/src/dev/virtio_common.rs:157`). The route table and `DEVICES` must stay fixed-size and allocation-free.
- **Lock ranks.** `DEVICES` replaces `RESOURCE_REGISTRY`/`PCIE_BARS`/`BDF_OWNERS`/`PCIE_MSI_STATE` uses (`kernel/src/sync/rank.rs:107-117`). The order `IOMMU_CLEANUP_SERIAL -> BDF_OWNERS -> VTD_DOMAINS` (`scripts/mutable-globals.toml:1330`) must be re-derived for one lock.

## Open questions for the owner

1. **ABI for multi-NIC traffic.** May Phase 18 add one read-only syscall (`LookupNicRoute`, a `ViBlockRouteV1`-shaped record, readable only by the live `service::NET` provider) and make the net service multi-interface? This touches `libs/abi` and `cells/services/net`, both outside the §3 file set. If not, the success criterion becomes "both bound on their own routes, primary carries traffic, restarting one leaves the other's route bound".
2. **Two instances of one driver path.** May `libs/boot-config` accept two cells with the same `path` (or an `instances` count) for driver cells, so two NICs of the same model each get a driver? This changes what a signed `services.toml` can express. If not, the fixture covers different models only.
3. **Device owner identity.** Should `DeviceRecord` checks move from the claiming tid to the incarnation (X5)? That lets sibling threads of one driver use its device: an authority widening inside one cell. The default proposal keeps the tid.
