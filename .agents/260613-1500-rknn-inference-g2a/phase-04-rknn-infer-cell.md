# Phase 04 — `rknn-infer` Service Cell (Track A — Tier 1b Native FFI)

**Track**: A (Tier 1b native FFI)  
**Status**: 📋 PLANNED  
**Priority**: HIGH (G2 graduation path)  
**Effort**: ~3 weeks  
**Depends on**: Phase 01 (ostd runtime linker), Phase 02 (pthread/mmap shims), Phase 03 (IPC protocol)  
**Hardware prerequisite**: ARM64 ViCell boot on RK3588 (for end-to-end inference test)

---

## Context Links
- `libs/api/src/ipc.rs` — `InferRequest` / `InferResponse` (from Phase 03)
- `libs/api/src/posix.rs` — POSIX shims (from Phase 02)
- `libs/ostd/src/dynlink.rs` — runtime linker (from Phase 01)
- `cells/services/net/src/` — reference service cell dispatch pattern
- `libs/ostd/src/display.rs` — GrantRegister/GrantShare usage model
- RKNN API: `rknn_init`, `rknn_inputs_set`, `rknn_run`, `rknn_outputs_get`, `rknn_destroy`

## Overview

`cells/apps/rknn-infer/` is a new service cell that:
1. Loads an RKNN model file from VFS at startup (buffer-mode: read `.rknn` → malloc → rknn_init)
2. Calls `dynlink::init` at startup to link `librknnrt.so` + companions into the cell's SAS
3. Accepts `InferRequest` messages via the standard IPC dispatch loop
4. On `RegisterInput`: stores the grant_id and calls `sys_grant_slice` to obtain the input tensor pointer
5. On `Run`: calls `rknn_inputs_set` → `rknn_run` → `rknn_outputs_get`, writes output to output_grant
6. Registers as service `service::INFER` via `sys_register_service`

**Build-time only (no hardware):** The cell can be compiled for aarch64 without `librknnrt.so` present — `build.rs` conditionally includes the `.so` blob and sets the link flag. Without the `.so`, the cell compiles but `rknn_*` functions resolve to null at runtime (safe since `rknn_init` returns `RKNN_ERR_DEVICE_UNAVAILABLE` before any pointer call).

---

## Key Insights

### VA base address
VA base for `rknn-infer`: **0x2C000000** (above `posix-shim-test` at 0x2A000000, below the `.so` blob window at 0x7C000000).

### Model loading: buffer mode vs path mode
Use buffer mode (`size > 0`): read the `.rknn` model file from VFS via `sys_send(vfs_tid, VfsRequest::Open{...})` + `Read` chunks, accumulate into a `malloc`'d heap buffer, then call `rknn_init(&ctx, model_buf, model_size, 0, NULL)`. Free the model buffer immediately after `rknn_init` returns (the SDK copies weights to DRM/NPU memory internally).

### Sync inference mode (no RKNN_FLAG_ASYNC_MASK)
Always pass `flags = 0` to `rknn_init`. This disables the async pipeline and avoids `pthread_create` being called for the pipeline worker. If the SDK still spawns threads internally (with `flags=0`), Phase 02's `pthread_create → sys_spawn` stub handles it safely.

### Output format: `want_float = 1`
Set `rknn_output.want_float = 1` to request dequantized float32 output. This ensures the output tensor is directly usable by the app cell without additional quantization handling.

### Service registration timing
Call `sys_register_service(service::INFER)` only after `rknn_init` succeeds. This prevents app cells from sending inference requests before the model is loaded.

---

## Requirements

### Functional
- FR1: Load model path from VFS: `/models/default.rknn` (configurable via a startup argument)
- FR2: Call `dynlink::init` at startup with `librknnrt.so` + `libstdc++.so.6` + `libgcc_s.so.1` + `libm.so.6` blobs
- FR3: Implement InferRequest dispatch loop (RegisterInput → Run → Unregister)
- FR4: `rknn_inputs_set` uses the registered grant VA as `rknn_input.buf` (pass_through=1, RKNN_TENSOR_UINT8 or FLOAT32 depending on model)
- FR5: `rknn_run(ctx, NULL)` — blocking, sync mode
- FR6: `rknn_outputs_get` writes to a local buffer; copy to output_grant VA; reply `Done { bytes }`
- FR7: Register as `service::INFER` after model load succeeds
- FR8: On startup failure (model not found, rknn_init fails), log error and exit — init will restart the cell

### Non-functional
- NF1: Cell declares `#![no_std]`, `#![no_main]`, `#![forbid(unsafe_code)]` — unsafe only in `ostd` (dynlink, shims)
- NF2: `declare_manifest!(block_io = false, network = false, spawn = false)`
- NF3: `declare_syscalls![Send, Recv, Log, LookupService, RegisterService, GrantShare, GrantSlice, GrantRegister, GrantUnregister, GrantFree]`
- NF4: VA base 0x2C000000 in `rknn-infer.ld`

---

## Architecture: Cell Startup Sequence

```
rknn-infer cell startup
├── ostd::startup → dynlink::init([librknnrt_blob, libstdcpp_blob, libgcc_blob, libm_blob])
│   ├── map blobs at 0x7C000000+
│   ├── apply AARCH64 relocations
│   └── run .init_array constructors
│
├── vfs_tid = sys_lookup_service(service::VFS)
├── model_buf = vfs_read("/models/default.rknn")  [heap-allocated]
├── rknn_init(&ctx, model_buf, model_size, 0, NULL)
├── free(model_buf)
│
├── sys_register_service(service::INFER)
│
└── dispatch loop:
    ┌─ sys_try_recv(&buf) → sender
    ├─ decode InferRequest::RegisterInput { grant, byte_len }
    │  └─ input_ptr = sys_grant_slice(grant)
    │     stored in INFER_STATE.input_grant = grant
    │             INFER_STATE.input_ptr = input_ptr
    │             INFER_STATE.input_len = byte_len
    │     sys_send(sender, InferResponse::Ok)
    │
    ├─ decode InferRequest::Run { output_grant, output_len }
    │  └─ inputs[0] = rknn_input { buf: input_ptr, size: input_len, pass_through: 1, ... }
    │     rknn_inputs_set(ctx, 1, inputs)
    │     rknn_run(ctx, NULL)
    │     outputs[0] = rknn_output { want_float: 1, is_prealloc: 0, ... }
    │     rknn_outputs_get(ctx, 1, outputs, NULL)
    │     output_ptr = sys_grant_slice(output_grant)
    │     memcpy(output_ptr, outputs[0].buf, min(outputs[0].size, output_len))
    │     rknn_outputs_release(ctx, 1, outputs)
    │     sys_send(sender, InferResponse::Done { bytes: outputs[0].size })
    │
    └─ decode InferRequest::Unregister
       └─ clear INFER_STATE; sys_send(sender, InferResponse::Ok)
```

---

## Related Code Files

### Create
- `cells/apps/rknn-infer/Cargo.toml`
- `cells/apps/rknn-infer/build.rs` — conditional `.so` blob bundling + link args
- `cells/apps/rknn-infer/rknn-infer.ld` — VA base 0x2C000000
- `cells/apps/rknn-infer/src/main.rs` — dispatch loop
- `cells/apps/rknn-infer/src/rknn_ffi.rs` — `#[repr(C)]` bindings for `rknn_input`, `rknn_output`, `rknn_context`, `rknn_tensor_attr`, and `extern "C"` fn declarations

### Modify
- `Cargo.toml` (workspace root) — add `"cells/apps/rknn-infer"`
- `gen_disk.ps1` — add `$rknn_infer_bin` + optional table entry

---

## Implementation Steps

1. Create `cells/apps/rknn-infer/src/rknn_ffi.rs`: `#[repr(C)]` types + `extern "C"` declarations (aarch64-only `#[cfg]`)
2. Create `rknn-infer.ld` (VA base 0x2C000000; same structure as posix-shim-test.ld)
3. Create `build.rs`: link arg + `#[cfg(aarch64)]` blob embedding logic
4. Create `Cargo.toml`
5. Create `src/main.rs`: startup sequence (dynlink::init → vfs_read → rknn_init → register → dispatch loop)
6. Add workspace member to `Cargo.toml`
7. Add `gen_disk.ps1` entry
8. `cargo check -p rknn-infer --target aarch64-unknown-none` clean (stub mode without .so)
9. Document hardware test procedure in Phase 07

---

## Todo

- [ ] Create `cells/apps/rknn-infer/src/rknn_ffi.rs`
- [ ] Create `cells/apps/rknn-infer/rknn-infer.ld`
- [ ] Create `cells/apps/rknn-infer/build.rs`
- [ ] Create `cells/apps/rknn-infer/Cargo.toml`
- [ ] Create `cells/apps/rknn-infer/src/main.rs`
- [ ] Add to workspace `Cargo.toml`
- [ ] Add to `gen_disk.ps1`
- [ ] `cargo check -p rknn-infer --target aarch64-unknown-none` clean

---

## Success Criteria

1. `cargo check -p rknn-infer --target aarch64-unknown-none` clean
2. Cell starts, logs "INFER: waiting for model at /models/default.rknn" when VFS has no model
3. (Hardware) Cell loads MobileNetV1, registers as INFER service
4. (Hardware) Client cell sends `RegisterInput` + `Run` → receives `Done { bytes: 4000 }` (1000 × f32)
5. (Hardware) Top-1 class matches expected label for test image

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `librknnrt.so` calls `dlopen` to load a plugin `.so` that isn't bundled | Stub `dlopen` to return NULL; `dlsym` returns NULL; rknn falls back to built-in path or returns error |
| C++ exceptions from `libstdc++` propagate past the C boundary | Set `__cxa_throw` stub that calls `rknn_destroy` + logs; test on real hardware |
| `rknn_init` opens `/dev/dri/renderX` before ioctl is wired (Phase 02 returns ENOSYS) | `rknn_init` returns `RKNN_ERR_DEVICE_UNAVAILABLE`; cell logs and retries after delay |
| Model file > 16MB: exceeds ostd heap | Model is read in chunks via VFS; pre-allocate heap or use GrantAlloc for model buffer |
