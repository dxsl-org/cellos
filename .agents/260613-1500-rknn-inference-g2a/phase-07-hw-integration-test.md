# Phase 07 — Hardware Integration Test Plan (All Tracks)

**Track**: All  
**Status**: 📋 PLANNED  
**Priority**: MEDIUM (documents validation procedure; test stubs written now, hardware tests gated)  
**Effort**: ~1 week (stub tests now; hardware execution when RK3588 board arrives)  
**Depends on**: Phase 03 complete; one of Phase 04, 05, or 06 complete

---

## Context Links
- `tests/integration/tests/boot.rs` — existing integration test file and patterns
- `tests/integration/src/lib.rs` — QemuRunner, spawn_echo_server
- Phase 04/05/06 — inference cells under test

## Overview

This phase adds integration tests for the RKNN inference pipeline. Tests are structured in two tiers:

1. **QEMU stub tests** (runnable now, no hardware): verify the IPC protocol round-trip using a stub inference cell that returns mock output data without invoking any RKNN SDK
2. **Hardware tests** (gated on RK3588): marked `#[ignore]` by default; run with `cargo test -- --include-ignored` on a real board; test MobileNetV1 inference with a known-label image

---

## QEMU Stub Test: `infer_ipc_roundtrip`

The stub `rknn-infer-stub` cell:
- Accepts `InferRequest::RegisterInput` — stores grant pointer
- Accepts `InferRequest::Run` — writes known-pattern output to `output_grant` (4-byte magic `[0x42, 0x00, 0x00, 0x00, ...]`)
- No actual RKNN SDK; no hardware needed

```rust
// tests/integration/tests/boot.rs addition:

/// Tier 1b: InferRequest/InferResponse IPC round-trip (QEMU stub, no hardware).
///
/// Boots QEMU with rknn-infer-stub cell, verifies app can register input grant,
/// send Run request, and receive Done response with expected mock output bytes.
#[test]
fn infer_ipc_roundtrip() {
    if !prerequisites_ok() { return; }
    let mut qemu = QemuRunner::boot_with_fresh_disk(&kernel_path(), &disk_path());
    qemu.wait_for("ViCell >", BOOT_TIMEOUT)
        .unwrap_or_else(|e| panic!("prompt: {e}\n{}", qemu.dump()));
    std::thread::sleep(Duration::from_millis(300));
    qemu.send_line("infer-test");  // infer-test cell: sends RegisterInput + Run
    qemu.wait_for("INFER-IPC: OK", CMD_TIMEOUT)
        .unwrap_or_else(|e| panic!("infer IPC failed: {e}\n{}", qemu.dump()));
}

/// Tier 1b: End-to-end RKNN NPU inference on RK3588 hardware.
///
/// Requires: ARM64 ViCell boot + RKNPU kernel driver + /models/mobilenet_v1.rknn on disk.
/// Ignored by default; run with: cargo test infer_rknn_mobilenet -- --ignored
#[test]
#[ignore = "requires RK3588 hardware with RKNPU driver"]
fn infer_rknn_mobilenet() {
    // Hardware test: load MobileNetV1, run inference on cat.jpg, verify top-1 = "tabby cat"
    // Implementation: boot QEMU with ARM64 image + RKNPU passthrough (or run directly on board)
    todo!("hardware test — see phase-07-hw-integration-test.md for procedure")
}
```

---

## Hardware Test Procedure

### Prerequisites
1. Radxa ROCK 5B+ 16GB with ViCell ARM64 booting
2. RKNPU kernel driver loaded (verify: `dmesg | grep rknpu` shows NPU initialized)
3. `/models/mobilenet_v1.rknn` on the VirtIO block device FAT32 partition
4. Test image `/images/cat_224.raw` (224×224×3 uint8, known label = class 281 "tabby cat")

### Test Procedure (Track A or B)
```
1. Boot ViCell on ROCK 5B+
2. Shell: `rknn-infer /models/mobilenet_v1.rknn`
   Expected: "INFER: model loaded, registered as INFER service"
3. Shell: `infer-test /images/cat_224.raw`
   Expected: "INFER: top-1 class=281 score=0.87"
4. Measure latency: run `bench-infer` cell 1000 iterations
   Expected: P50 < 5ms, P99 < 20ms (sync mode, RK3588 NPU)
```

### Test Procedure (Track C — Tier 3b VM)
```
1. Boot ViCell on ROCK 5B+
2. Shell: `rknn-proxy` (starts Alpine VM, waits for infer-daemon)
   Expected: "VM booted, infer-daemon ready"
3. Shell: `infer-test /images/cat_224.raw`
   Expected: "INFER: top-1 class=281 score=0.87"
4. Measure round-trip latency including VM dispatch overhead
```

---

## `infer-test` Cell

A small app cell (`cells/apps/infer-test/`) that:
- Looks up `service::INFER`
- Allocates input grant (602112 bytes = 224×224×3 float32)
- Reads test image into grant buffer (or uses a built-in synthetic tensor)
- Sends `InferRequest::RegisterInput` + `InferRequest::Run`
- Receives `InferResponse::Done { bytes }`
- Prints top-5 class indices from output buffer
- Outputs "INFER-IPC: OK" for QEMU test detection

### QEMU Stub Mode
When running against `rknn-infer-stub` (no real NPU), the stub returns a fixed output tensor with class 42 having the highest score. `infer-test` prints "INFER-IPC: OK" when it receives a well-formed `Done` response regardless of output values.

---

## Related Code Files

### Create
- `cells/apps/rknn-infer-stub/` — stub inference cell (QEMU-testable, no RKNN SDK)
- `cells/apps/infer-test/` — test client cell (QEMU + hardware)
- Add `infer_ipc_roundtrip` + `infer_rknn_mobilenet` tests to `tests/integration/tests/boot.rs`

---

## Todo

- [ ] Create `cells/apps/rknn-infer-stub/` (stub returns mock Done)
- [ ] Create `cells/apps/infer-test/` (RegisterInput + Run + print result)
- [ ] Add `infer_ipc_roundtrip` test to `boot.rs`
- [ ] Add `infer_rknn_mobilenet` (ignored) test to `boot.rs`
- [ ] Add both cells to workspace Cargo.toml + gen_disk.ps1
- [ ] `cargo check` on both cells (riscv64 + aarch64) passes
- [ ] QEMU test `infer_ipc_roundtrip` passes

---

## Success Criteria

1. `infer_ipc_roundtrip` QEMU test passes (stub mode, no hardware)
2. (Hardware) `infer_rknn_mobilenet` produces correct top-1 class for MobileNetV1
3. Latency documented: P50/P99 for each track (Track A, B, C) compared
