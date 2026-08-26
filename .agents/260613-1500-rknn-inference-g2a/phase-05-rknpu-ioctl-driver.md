# Phase 05 — Direct RKNPU ioctl NPU Driver Cell (Track B)

**Track**: B (native ViCell, bypasses librknnrt.so)  
**Status**: 📋 PLANNED  
**Priority**: MEDIUM (alternative/fallback to Track A; also validates the ViCell driver cell pattern for future PCIe drivers)  
**Effort**: ~4 weeks  
**Depends on**: Phase 03 (InferRequest/InferResponse IPC protocol)  
**Hardware prerequisite**: ARM64 ViCell boot on RK3588 + RKNPU kernel driver loaded

---

## Context Links
- RKNPU reverse engineering: https://github.com/phhusson/rknpu-reverse-engineering
- RKNPU Linux kernel driver: `drivers/rknpu/` in Rockchip kernel fork (linux-6.1-rk35xx branch)
- Phase 03: `InferRequest`/`InferResponse` protocol
- `cells/drivers/` — existing driver cell pattern

## Overview

Track B bypasses `librknnrt.so` entirely. Instead, `cells/drivers/npu/` is a ViCell driver cell that directly speaks the RKNPU kernel driver ioctl protocol. The approach:

1. The driver cell opens `/dev/rknpu` (or `/dev/dri/renderDX`) — a VFS device node backed by a ViCell platform driver
2. Issues `RKNPU_IOCTL_MEM_CREATE` to allocate NPU-accessible DMA buffers
3. Parses the `.rknn` model format header to extract graph IR + weight tensors
4. Issues `RKNPU_IOCTL_SUBMIT_TASK` with the model graph and input tensor
5. Polls `RKNPU_IOCTL_WAIT` for completion
6. Reads output tensor from the mapped result buffer

**Key tradeoff vs Track A:**
- ✅ No `.so` loading, no dynamic linker, no `libpthread`/`libstdc++` complexity
- ✅ Pure Rust cell — auditable, no opaque binary blob
- ⚠️ Must parse `.rknn` model format (partially reverse-engineered; incomplete documentation)
- ⚠️ Must implement the RKNPU ioctl protocol (documented by community; may drift with kernel versions)
- ⚠️ Model format parsing is the hard part — `librknnrt.so` does graph optimization at init time

---

## Key Insights

### RKNPU ioctl interface (confirmed from reverse engineering)

From `phhusson/rknpu-reverse-engineering` and Rockchip kernel source `drivers/rknpu/rknpu_ioctl.h`:

```c
// Key ioctls (RKNPU driver)
RKNPU_IOCTL_MEM_CREATE       // allocate DRM GEM buffer (physaddr + virtaddr + fd)
RKNPU_IOCTL_MEM_MAP          // mmap GEM buffer into userspace VA
RKNPU_IOCTL_MEM_DESTROY      // release GEM buffer
RKNPU_IOCTL_SUBMIT           // submit a task (graph + inputs) to the NPU
RKNPU_IOCTL_WAIT             // poll/wait for task completion
RKNPU_IOCTL_GET_HW_VERSION   // query NPU hardware version (RK3588 = NPU v2)
RKNPU_IOCTL_SET_CORE         // set which NPU core (0/1/2 on RK3588)
```

### .rknn model format
The `.rknn` file format is a container with:
- Header: magic `0x4E4E4B52` ("RKNK"), version, section count
- Sections: model IR (compiled graph), weight data, metadata
- The compiled graph is NPU-architecture-specific (RK3588 uses NPU arch v2)

**Community documentation status:** Partial. The header format is reverse-engineered; the inner graph IR is mostly opaque (NPU-specific bytecode). Rockchip provides the compiler (rknn-toolkit2) to convert ONNX/TFLite → `.rknn`, but the output format is not fully documented publicly.

**Practical implication:** The driver cell can extract weight data sections from the `.rknn` file without fully understanding the graph IR — the NPU hardware interprets the graph IR directly after `RKNPU_IOCTL_SUBMIT`. The cell just needs to locate and DMA-map the relevant sections.

### `/dev/rknpu` device node in ViCell
This requires a ViCell platform driver cell for the RKNPU device:
- Reads DTB/ACPI to find RKNPU MMIO base address (RK3588: `0xFDAB0000`)
- Maps RKNPU MMIO registers via `ostd::mmio::MmioRegion`
- Implements ioctl-like IPC messages routed from the driver cell via the VFS device node path

For Phase 05, the simplest approach: the NPU driver cell registers itself as a service (e.g., `service::NPU_DRV`) and handles raw ioctl-equivalent IPC messages directly. No VFS device node needed initially — higher-level cells talk to it via `sys_lookup_service(service::NPU_DRV)`.

---

## Requirements

### Functional
- FR1: NPU driver cell opens RKNPU MMIO at `0xFDAB0000` via `ostd::mmio::MmioRegion`
- FR2: Implements `rknpu_mem_create` (allocate CMA/DMA buffer using `sys_grant_alloc` + set physical address constraint)
- FR3: Parses `.rknn` file header: validates magic, extracts weight section offset+size, extracts graph IR section offset+size
- FR4: Implements `rknpu_submit(graph_ir, input_buf, output_buf)` — formats `rknpu_submit_task` struct, issues hardware command
- FR5: Implements `rknpu_wait(timeout_ms)` → polls completion register
- FR6: Exposes `InferRequest`/`InferResponse` protocol (Phase 03) — clients see the same IPC surface as Track A

### Non-functional
- NF1: Pure Rust driver cell — no C FFI, no POSIX shims needed
- NF2: Driver cell VA base: `0x2E000000` (above rknn-infer at 0x2C000000)
- NF3: `declare_manifest!(block_io = false, network = false, spawn = false)`
- NF4: `declare_syscalls![Send, Recv, Log, LookupService, RegisterService, GrantShare, GrantSlice, GrantFree]`

---

## Architecture

```
cells/drivers/npu/src/
├── main.rs           — startup + RKNPU MMIO init + dispatch loop
├── mmio.rs           — RKNPU register layout (MMIO addresses from Rockchip TRM)
├── mem.rs            — CMA buffer allocation + DMA setup
├── rknn_model.rs     — .rknn file format parser (header + section extraction)
└── submit.rs         — rknpu_submit_task formatting + hardware command
```

---

## Related Code Files

### Create
- `cells/drivers/npu/Cargo.toml`
- `cells/drivers/npu/build.rs`
- `cells/drivers/npu/npu-driver.ld` (VA base 0x2E000000)
- `cells/drivers/npu/src/main.rs`
- `cells/drivers/npu/src/mmio.rs`
- `cells/drivers/npu/src/mem.rs`
- `cells/drivers/npu/src/rknn_model.rs`
- `cells/drivers/npu/src/submit.rs`

### Modify
- `Cargo.toml` (workspace root) — add `"cells/drivers/npu"`
- `gen_disk.ps1` — add `$npu_driver_bin`

---

## Implementation Steps

1. Create cell skeleton (Cargo.toml, build.rs, linker script, src/main.rs)
2. Add `mmio.rs`: RKNPU MMIO register constants from Rockchip TRM (RK3588 NPU TRM §5)
3. Add `mem.rs`: allocate physically-contiguous DMA buffer (CMA); ViCell approach: `sys_grant_alloc` returns physical-equals-virtual in SAS; flag the allocation as DMA-capable
4. Add `rknn_model.rs`: parse `.rknn` header (magic + version + section table); extract graph IR and weight sections as byte slices
5. Add `submit.rs`: format `RKNPU_TASK_SUBMIT` structure (IRQ task list, input descriptor, output descriptor); write to MMIO task queue
6. Wire dispatch loop in `main.rs`: receive `InferRequest`, load model, run inference, reply
7. Add workspace member; add gen_disk.ps1 entry
8. `cargo check -p npu-driver --target aarch64-unknown-none` clean

---

## Todo

- [ ] Create cell skeleton files
- [ ] Implement `mmio.rs` RKNPU register layout
- [ ] Implement `mem.rs` DMA buffer allocation
- [ ] Implement `rknn_model.rs` .rknn format parser
- [ ] Implement `submit.rs` RKNPU task submission
- [ ] Wire dispatch loop in `main.rs`
- [ ] `cargo check -p npu-driver --target aarch64-unknown-none` clean

---

## Success Criteria

1. `cargo check -p npu-driver --target aarch64-unknown-none` clean
2. `.rknn` model parser correctly identifies magic + version + section offsets (unit-testable with a real .rknn file on host)
3. (Hardware) RKNPU MMIO maps without fault at `0xFDAB0000`
4. (Hardware) MobileNetV1 inference completes with correct top-1 class

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `.rknn` graph IR format is opaque — cannot reconstruct input/output tensor descriptors | HIGH | Focus on extracting pre-compiled graph and passing it verbatim; query tensor shapes from `RKNPU_IOCTL_GET_HW_VERSION` + model header |
| Rockchip TRM (NPU chapter) not publicly available | HIGH | Use community reverse engineering + Rockchip kernel driver source (open source) as reference |
| RKNPU ioctl ABI changes between kernel 5.10 and 6.1 | MED | Pin to kernel 6.1 (ROCK 5B+ default); document version assumption |
| DMA buffer allocation without CMA reserved pool | MED | Use RKNPU's `RKNPU_IOCTL_MEM_CREATE` which handles CMA internally via the kernel driver |

---

## Note: Track B vs Track A Decision

Track B succeeds without any `.so` loading infrastructure. However, if the `.rknn` model format proves too opaque to implement without a Rockchip NDA (likely for complex quantization schemes), Track A (`librknnrt.so`) is the pragmatic fallback since the SDK handles all model format complexity internally.

Both tracks are worth implementing: Track B validates ViCell's native driver cell architecture; Track A validates the Tier 1b FFI path for other vendor SDKs.
