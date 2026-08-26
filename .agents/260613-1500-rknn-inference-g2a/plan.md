# RKNN Inference Cell — G2 Level A

**Plan**: 260613-1500-rknn-inference-g2a  
**Status**: 📋 PLANNED  
**Priority**: P1 (G2 graduation milestone — enables NPU inference demo on RK3588)  
**Effort**: ~8–12 weeks (software-only phases runnable now; hardware phases gated on RK3588)  
**Stage**: G2 Level A

---

## Context

The project roadmap identifies three viable paths for RKNN NPU inference on RK3588:

| Track | Mechanism | Key constraint |
|-------|-----------|---------------|
| A — Tier 1b native FFI | ELF userspace runtime linker + POSIX shim extensions; cell bundles `librknnrt.so` | Requires ELF reloc/PLT runtime in `ostd` |
| B — Direct RKNPU ioctl | Native ViCell NPU driver cell; bypasses `librknnrt.so` entirely | Requires RKNPU ioctl protocol reverse-engineering + `.rknn` model format parsing |
| C — Tier 3b hybrid | Alpine Linux VM (KVM EL2) runs `librknnrt.so`; ViCell passes requests via IPC | Requires Tier 3b KVM hypervisor (planned, not yet implemented) |

All three tracks share:
- Phase 03 (InferRequest/InferResponse IPC protocol — Law 1)
- Phase 08 (hardware integration test plan)

**Key finding (research):** `librknnrt.so` ships as a dynamic library only — no static `.a` exists. It requires `libpthread.so.0`, `libstdc++.so.6`, `libm.so.6`, `libdl.so.2`, and direct DRM ioctls to `/dev/dri/renderX` for NPU access. The Track A approach bundles the `.so` blobs in the cell ELF and uses an `ostd` runtime linker that resolves symbols against ViCell POSIX shims.

---

## Phase Overview

| Phase | Title | Track | Status | Depends On |
|-------|-------|-------|--------|-----------|
| [01](phase-01-ostd-runtime-linker.md) | `ostd` ELF runtime linker for companion `.so` | A | 📋 | — |
| [02](phase-02-posix-pthread-mmap.md) | POSIX shim extensions: pthread_mutex + mmap + ioctl + libm | A | 📋 | — (parallel with 01) |
| [03](phase-03-infer-ipc-protocol.md) | InferRequest/InferResponse IPC protocol | Shared | 📋 | — (parallel with 01+02) |
| [04](phase-04-rknn-infer-cell.md) | `rknn-infer` service cell (Track A) | A | 📋 | 01, 02, 03 |
| [05](phase-05-rknpu-ioctl-driver.md) | Direct RKNPU ioctl NPU driver cell (Track B) | B | 📋 | 03 |
| [06](phase-06-tier3b-hybrid.md) | Tier 3b Alpine VM inference demo (Track C) | C | 📋 | 03, KVM prereq |
| [07](phase-07-hw-integration-test.md) | Hardware integration test plan (all tracks) | All | 📋 | RK3588 hardware |

**Parallel-eligible at start:** Phases 01, 02, and 03 are fully independent — all three can run concurrently.

---

## Key Dependencies

### Law 1 Gates
- **Phase 02**: `libs/api/src/posix.rs` — bug-fix-level additions (pthread_mutex, mmap stubs) — 1× confirmation
- **Phase 03**: `libs/api/src/ipc.rs` (new enum variants) + `libs/api/src/syscall.rs` (`service::INFER = 6`) — **2× confirmation required**

### Prerequisite: ARM64 ViCell Boot
All hardware phases require ARM64 ViCell booting on a real RK3588 board (Radxa ROCK 5B+ 16GB). Software phases (01–03) are buildable and partially testable on QEMU aarch64 now.

### Prerequisite: Tier 3b KVM (Track C only)
Phase 06 requires the Tier 3b hypervisor (KVM EL2 on ARM64) — ✅ **SHIPPED** (status update
2026-07-12: all 10 phases of `.agents/260613-2134-tier3b-vmm-arm64-el2/` complete — Alpine boots,
virtio blk/net/console, vGIC). Track C's remaining gate is the real RK3588 board only
(`/dev/rknpu` passthrough into the guest — QEMU has no NPU device).

---

## IPC Protocol Summary (Phase 03)

```
App Cell                        Inference Cell
─────────                       ──────────────
GrantRegister(input_pages)  ─→  accept (stores grant_id)
GrantShare(grant, infer_tid)
                                                 
write tensor data to grant     
InferRequest::Run { grant, len, output_grant }  →
                                rknn_inputs_set + rknn_run + rknn_outputs_get
                            ←  InferResponse::Done { bytes }
read output from output_grant
```

---

## Success Criteria (all tracks)

1. `cargo check -p rknn-infer --target aarch64-unknown-none` clean
2. `cargo check -p npu-driver --target aarch64-unknown-none` clean (Track B)
3. Integration test (QEMU, stub mode): `infer_ipc_roundtrip` passes without hardware
4. Hardware test (RK3588): MobileNetV1 1000-class inference returns top-1 class matching known label
5. P99 latency measured and documented (QEMU TCG caveat noted)

---

## Risk Register

| Risk | Severity | Mitigation |
|------|----------|-----------|
| `librknnrt.so` calls `pthread_create` unconditionally on `rknn_init` | HIGH | Avoid `RKNN_FLAG_ASYNC_MASK`; implement `pthread_create` → `sys_spawn` as Phase 01 extension if needed |
| `librknnrt.so` C++ exceptions propagate to Rust cell | HIGH | Link a stub `libstdc++` ABI that catches at the C boundary; validate on real board |
| RKNPU ioctl protocol changes between kernel versions | MED (Track B) | Pin to Rockchip kernel 6.1 branch used by ROCK 5B+ |
| KVM hypervisor not ready when Track C coding starts | MED (Track C) | Track C is explicitly gated; decoupled from Tracks A+B |
| `.rknn` model format is opaque and partially documented | HIGH (Track B) | Track A + C avoid this entirely; Track B may require Rockchip NDA or community reverse-engineering |
