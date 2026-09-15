# Phase 05 — Full Gate Closure: Network HTTPD, C2C Oracle, and x86 Hypervisor

**Status**: implemented and verified locally; ready for hosted push
**Ceiling**: hosted CI gate across all 23 jobs

## 1. Network Data-Path Integration: HTTPD Test Failures

### The Defect
In `tests/integration/tests/boot.rs`, `network_httpd_serves_file` and `network_httpd_dynamic_content` failed with empty HTTP responses when requesting files over QEMU hostfwd.

### Root Cause
Commit `09d2b6b` replaced `$httpd_bin = "$rel_dir/httpd"` (`app-net-tools`) with `service-httpd` in `gen_disk.ps1` to support `/api/infer` for the AI lane. However, `service-httpd` hardcoded port 8080 and ignored CLI arguments (`httpd 9091 /tmp/resp.txt &`). When the test connected to forwarded port 9091, no server was listening on that port, yielding 0 bytes.

### Fix
- `cells/services/httpd/src/main.rs`: Parse `ostd::args()` to extract `<port> [vfs_path]`. Defaults to port 8080 when no args are provided.
- `cells/services/httpd/src/router.rs`: When `file_to_serve` is provided, bypass standard REST routing and directly serve the requested file via `handlers::serve_file(cap, net_ep, vfs_ep, target_file)`.
- Output log formatting: print `httpd: listening on :<port>`, matching both `wait_for("httpd: listening")` in `boot.rs` and `wait_for("httpd: listening on :8080")` in `http-infer.rs` and `aarch64-boot.rs`.

### Verification
- `cargo test --test boot network_httpd_serves_file`: **PASS** (17.20s)
- `cargo test --test boot network_httpd_dynamic_content`: **PASS** (17.98s)
- `cargo test --test http-infer`: **PASS** (88.45s) — verifies standard AI inference routing on port 8080 remains functional.

---

## 2. C2C Broker Oracle (single-guest local-runtime QEMU)

### The Defect
`c2c-broker-oracle` failed with `oracle shell did not boot: timeout: pattern "Cellos >" not seen in 120s`. On the guest serial log, `platform` faulted at `0x30000000`, `block` faulted at `0x10001000`, and `vfs` faulted at `0x80cbe000`.

### Root Cause
`scripts/run-c2c-broker-oracle-qemu.sh` built the 11 cell binaries but omitted the `sign_cells` step. Under ADR-0015 (`kernel/src/loader/governed_spawn.rs`), unsigned cells are admitted to Tier 2 Paged Domains. Driver cells (`platform` needing PCIe ECAM MMIO at `0x30000000`, `block` needing VirtIO-BLK MMIO at `0x10001000`, and `vfs`) ran under isolated `satp` roots that lacked peripheral MMIO mappings, causing immediate load/store page faults on boot.

### Fix
- `scripts/run-c2c-broker-oracle-qemu.sh`: Added `source scripts/lib-sign-cells.sh` and `sign_cells "${CELL_BINARIES[@]}"`, matching all other CI image assembly scripts (`build-test-hooks-ci.sh`, `run-ai-inference-oracle-qemu.sh`, etc.).
- With valid dev signatures, driver and system cells are verified and admitted as Tier 1 Trusted SAS cells with full MMIO/DMA access.

### Verification
- `bash scripts/run-c2c-broker-oracle-qemu.sh`: **PASS** (`test result: ok. 1 passed; 0 failed; finished in 9.76s`). All sweeps (1..16), soak (10,000/10,000), restart, and role gate assertions pass cleanly.

---

## 3. QEMU Hypervisor Boot-to-Shell (x86_64)

### The Defect
`scripts/qemu-hypervisor-smoke-x86.sh build/vicell-x86-hv.iso` in `HV_SMOKE_MODE=boot` timed out after 600s without seeing the Alpine `/ #` prompt.

### Root Cause
Under `HV_VOLATILE_DISK=1` (set by CI in `ci.yml:894`), `cells/services/hypervisor/src/virtio_blk.rs` allocates a 4 MiB `Vec<u8>` for the scratch disk (`const DISK_SIZE: usize = 4 * 1024 * 1024`). `service-hypervisor` never declared a custom heap, so it inherited `ostd`'s default 1 MiB heap (`HEAP_SIZE = 1024 * 1024`). Allocating 4 MiB triggered an immediate `OOM: cell heap exhausted — size=4194304`, putting the hypervisor cell into an infinite crash/restart loop that starved the guest.

### Fix
- `cells/services/hypervisor/src/main.rs`: Added `ostd::declare_custom_heap!(10 * 1024 * 1024);` and `init_custom_heap();` in `main()`, within the 16 MiB `DEFAULT_QUOTA_BYTES` ceiling.

### Verification
- `HV_SMOKE_MODE=machinery`: **PASS** in 30s.
- `HV_SMOKE_MODE=boot`: OOM crash loop eliminated; Alpine Linux kernel boots through SMP, devtmpfs, and cpuidle continuously without a single restart.
