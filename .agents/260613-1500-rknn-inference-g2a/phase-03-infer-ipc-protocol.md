# Phase 03 — InferRequest/InferResponse IPC Protocol

**Track**: Shared (all three tracks converge here)  
**Status**: 📋 PLANNED  
**Priority**: HIGH — gates Phases 04, 05, and 06  
**Effort**: ~3 days  
**Depends on**: nothing (parallel-eligible with 01, 02)  
**Law 1**: `libs/api/src/ipc.rs` + `libs/api/src/syscall.rs` — **2× user confirmation required before implementation**

---

## Context Links
- `libs/api/src/ipc.rs` — `NetRequest`, `NetResponse`, `VfsRequest`, `VfsResponse` (model)
- `libs/api/src/syscall.rs` — `service::*` constants (line ~414)
- `libs/ostd/src/syscall.rs` — `sys_lookup_service`, `sys_grant_alloc`, `sys_grant_share`, `sys_grant_slice`
- Research findings: GrantRegister is the right primitive for persistent input tensor buffer

## Overview

All three inference tracks use the same app-cell → inference-service IPC protocol. The protocol is grant-based (zero-copy tensor transfer) — the app cell writes tensor data into a shared grant buffer, then sends a small `InferRequest::Run` control message. The inference service runs the model and writes the output into a separate per-request output grant.

**No new kernel syscalls needed.** Only new variants in `libs/api/src/ipc.rs` (existing postcard IPC) and a new service ID constant in `libs/api/src/syscall.rs`.

---

## Key Insights

### Why GrantRegister for input, GrantAlloc for output
- **Input tensor** (≥ 602 KB for 224×224×3 float32): allocate once per connection with `GrantRegister` — persistent across many inference calls; app writes directly, no re-allocation overhead
- **Output tensor** (≤ 4 KB for 1000-class softmax; ≤ 100 KB for detection outputs): allocated per-request with `GrantAlloc`; app pre-allocates, shares with inference service, gets result back

This mirrors the `ostd::display` + compositor pattern exactly (display surface = persistent, damage IPC = control message).

### Postcard size budget
`InferRequest::Run { grant: usize, byte_len: usize, output_grant: usize, output_len: usize }` serialized via postcard: 1 (discriminant) + 8+1 (grant varint) + 8+1 + 8+1 + 8+1 ≈ 38 bytes. Well within the 4096-byte IPC_BUF_SIZE.

### Service ID selection
Current service IDs in `libs/api/src/syscall.rs::service`: VFS=1, NET=2 (from `project-service-id-registry.md` memory). The research found NET=2 is confirmed. INFER=6 is the next safe assignment after INPUT=3, CONFIG=4, COMPOSITOR=5.

---

## Requirements

### Functional — ipc.rs additions

```rust
/// Sent by an app cell to the inference service (resolved via service::INFER).
#[derive(Serialize, Deserialize)]
pub enum InferRequest<'a> {
    /// Register a GrantRegister buffer as the persistent input tensor channel.
    /// Must be called once before any Run requests.
    /// `grant` = grant_id from sys_grant_register; `byte_len` = allocated byte count.
    RegisterInput { grant: usize, byte_len: usize },
    /// Run inference on data already written to the registered input grant.
    /// `output_grant` = GrantAlloc the app pre-allocated and GrantShare'd with INFER service.
    /// `output_len` = byte capacity of output_grant.
    Run { output_grant: usize, output_len: usize },
    /// Release the registered input grant (cell shutting down or model switch).
    Unregister,
}

/// Sent by the inference service back to the requesting app cell.
#[derive(Serialize, Deserialize)]
pub enum InferResponse {
    /// RegisterInput accepted (service has sliced the grant).
    Ok,
    /// Run complete; `bytes` = number of output bytes written to output_grant.
    Done { bytes: usize },
    /// Error; `code` maps to a ViError discriminant.
    Err(u8),
}
```

### Functional — syscall.rs addition

```rust
// In libs/api/src/syscall.rs, service module
pub const INFER: u16 = 6;
```

### Non-functional
- NF1: `InferRequest` / `InferResponse` follow existing naming convention (same file as `NetRequest`, `VfsRequest`)
- NF2: `InferRequest` has `'a` lifetime only if `data: &'a [u8]` inline payload variant is ever added (currently not needed — use grant instead)
- NF3: No changes to `NetRequest`, `VfsRequest`, or any existing variants (additive only)
- NF4: `cargo check -p api` clean on both riscv64 and aarch64 targets

---

## Architecture

```
App Cell (rknn_infer client)
├── service_tid = sys_lookup_service(service::INFER)
├── input_grant = sys_grant_register(input_pages)
│   sys_grant_share(input_grant, service_tid, RW)
│   sys_send(service_tid, InferRequest::RegisterInput { grant: input_grant, byte_len })
│   resp = sys_recv()  →  InferResponse::Ok
│
├── [Per inference call:]
│   write tensor into input_grant VA
│   output_grant = sys_grant_alloc(output_pages)
│   sys_grant_share(output_grant, service_tid, RW)
│   sys_send(service_tid, InferRequest::Run { output_grant, output_len })
│   resp = sys_recv()  →  InferResponse::Done { bytes }
│   read output from output_grant VA
│   sys_grant_free(output_grant)
│
└── [On shutdown:]
    sys_send(service_tid, InferRequest::Unregister)
    sys_grant_free(input_grant)

Inference Service (rknn-infer or npu-driver)
├── recv InferRequest::RegisterInput → sys_grant_slice(grant) → store input ptr
├── recv InferRequest::Run → run model → sys_grant_slice(output_grant) → write output → Done
└── recv InferRequest::Unregister → clear stored ptr
```

---

## Related Code Files

### Modify
- `libs/api/src/ipc.rs` — add `InferRequest` + `InferResponse` enums (after existing service types)
- `libs/api/src/syscall.rs` — add `pub const INFER: u16 = 6` to `service` module

---

## Implementation Steps

1. Read `libs/api/src/ipc.rs` in full — confirm current line count and last variant position
2. Add `InferRequest<'a>` enum after `VfsResponse` (or in a logically grouped section)
3. Add `InferResponse` enum after `InferRequest`
4. Read `libs/api/src/syscall.rs` service constants — add `INFER = 6` after existing entries
5. `cargo check -p api --target riscv64gc-unknown-none-elf` clean
6. `cargo check -p api --target aarch64-unknown-none` clean

---

## Todo

- [ ] **CONFIRM**: 2× user confirmation that Law 1 edit to `libs/api/src/ipc.rs` + `syscall.rs` is approved
- [ ] Read `libs/api/src/ipc.rs` — find insertion point
- [ ] Add `InferRequest<'a>` + `InferResponse` to `ipc.rs`
- [ ] Add `service::INFER = 6` to `syscall.rs`
- [ ] `cargo check -p api` on riscv64 target passes
- [ ] `cargo check -p api` on aarch64 target passes

---

## Success Criteria

1. `cargo check -p api --target riscv64gc-unknown-none-elf` clean
2. `cargo check -p api --target aarch64-unknown-none` clean
3. A postcard round-trip test: `encode(InferRequest::Run { output_grant: 0x1234, output_len: 4096 })` → `decode::<InferRequest>` → same values
4. `service::INFER` constant visible and correct value (6)

---

## Security Considerations

- `InferRequest::RegisterInput` grants the inference service RW access to the input tensor — the service must validate byte_len against the grant's actual size (via `sys_grant_slice` return length) before accessing
- `output_grant` passed in `Run` must be validated by the inference service the same way — grants can only be sliced by their owner or authorized grantee; the kernel enforces this
- The inference service must NOT forward the grant to a third party — it receives RW access temporarily and must clear its stored reference on `Unregister`
