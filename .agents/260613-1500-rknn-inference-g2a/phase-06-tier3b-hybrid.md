# Phase 06 — Tier 3b Hybrid: Alpine VM Inference Demo (Track C)

**Track**: C (Tier 3b VM path)  
**Status**: 📋 PLANNED  
**Priority**: MEDIUM (fastest path to end-to-end demo once KVM is ready)  
**Effort**: ~2 weeks (given Tier 3b KVM hypervisor already working)  
**Depends on**: Phase 03 (IPC protocol) + **Tier 3b KVM hypervisor (external prerequisite)**  
**Hardware prerequisite**: ARM64 ViCell boot on RK3588 + KVM EL2 working

---

## Context Links
- `docs/specs/05-application.md §6` — Tier 3b hypervisor spec (note: some paths in spec are wrong per `project-tier3-hypervisor-strategy.md` memory)
- Project memory: `project-tier3-hypervisor-strategy.md` — minimal VMM custom ~9K LOC; RISC-V H-ext absent; ARM64 EL2 confirmed
- Phase 03: `InferRequest`/`InferResponse` — the same IPC surface Track C uses

## Overview

Track C runs `librknnrt.so` inside an Alpine Linux VM (KVM EL2) on RK3588. ViCell's data plane passes inference requests to the VM via a virtio-based shared memory ring. The VM runs a simple inference daemon that calls `rknn_init`/`rknn_run` natively inside full Linux with all `.so` dependencies satisfied by Alpine's package manager.

**This is the most pragmatic path for a G2 demo** — it avoids all Tier 1b shim work and leverages the RKNN SDK exactly as Rockchip ships it. The tradeoff is that inference happens outside ViCell's SAS (inside the VM), adding ~1–3ms round-trip latency.

---

## Key Insights

### Virtio shared memory path
The fastest ViCell → VM communication is through a virtio-mem device: ViCell maps a fixed physical memory region (16 MB at a known PA) into the VM as a virtio-mem device. Both sides access the same physical pages:
- ViCell writes input tensor + request header to the shared region
- VM's daemon polls a doorbell, reads tensor, runs inference, writes output back
- ViCell polls result-ready flag

This avoids virtio-net overhead and achieves near-zero-copy latency (only cache coherency cost).

### Alpine Linux + RKNN on RK3588
Alpine Linux supports ARM64 and can install `librknnrt.so` from Rockchip's unofficial APK or by bundling it in the VM image at build time. The inference daemon is a 50-line C program:
```c
while (1) {
    wait_for_request(&shmem->doorbell);
    rknn_inputs_set(ctx, 1, &input);  // buf = &shmem->input_tensor
    rknn_run(ctx, NULL);
    rknn_outputs_get(ctx, 1, &output, NULL);
    memcpy(shmem->output_tensor, output.buf, output.size);
    shmem->result_ready = 1;
}
```

### No new IPC protocol needed
Track C still uses `InferRequest`/`InferResponse` (Phase 03) for the ViCell app cell → ViCell inference proxy cell path. The proxy cell then communicates with the VM daemon via shared memory. From the app cell's perspective, the protocol is identical to Tracks A and B.

---

## Architecture

```
App Cell
  │  InferRequest::Run (Phase 03 IPC)
  ▼
rknn-proxy cell (new, ~100 LOC)
  │  write tensor to virtio-mem shared region
  │  set doorbell
  │  poll result_ready
  │  read output tensor
  ▼
Alpine Linux VM (KVM EL2)
  │  infer-daemon: rknn_init + rknn_run loop
  ▼
  RKNPU hardware (via /dev/rknpu in VM)
```

---

## Requirements

### Functional
- FR1: `rknn-proxy` cell: a thin wrapper that forwards `InferRequest` over virtio-mem shared memory to the Alpine VM daemon
- FR2: Alpine VM image contains `librknnrt.so` + `infer-daemon` binary
- FR3: VM image stored at `/models/alpine-infer.img` on the VirtIO block device
- FR4: `rknn-proxy` cell registered as `service::INFER` (same as Tracks A and B — app cell is agnostic to which track is active)

### External prerequisites
- Tier 3b KVM hypervisor: ViCell must be able to spawn a KVM VM with virtio-mem + virtio-blk
- ROCK 5B+ hardware with RKNPU accessible from inside the VM (passthrough or virt model)

---

## Related Code Files

### Create
- `cells/apps/rknn-proxy/` — inference proxy cell
- `tools/build-alpine-infer-vm/` — build script for Alpine VM image with infer-daemon

---

## Todo (conditional on Tier 3b KVM being ready)

- [ ] Confirm Tier 3b KVM prerequisite status
- [ ] Design virtio-mem shared region layout (header + input + output + doorbell)
- [ ] Create `cells/apps/rknn-proxy/` cell
- [ ] Build Alpine VM image with infer-daemon
- [ ] End-to-end test: ViCell → proxy → VM → RKNPU → result

---

## Success Criteria (hardware-gated)

1. `rknn-proxy` cell starts, spawns Alpine VM, VM boots to infer-daemon ready state
2. App cell sends `InferRequest::Run` → receives `InferResponse::Done { bytes: 4000 }` (1000 × f32)
3. Round-trip latency measured (P50/P99)
4. Top-1 class for test image matches expected label

---

## Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|-----------|
| Tier 3b KVM hypervisor not ready in time | HIGH | This phase is explicitly gated; don't start until KVM boots a minimal VM |
| RKNPU device passthrough to Alpine VM blocked by IOMMU setup | MED | If passthrough fails, use rknn_server proxy approach (VM runs as PC-debug server) |
| Alpine Linux boots but RKNPU device not recognized | MED | Use Rockchip vendor kernel (not mainline) inside the VM |
