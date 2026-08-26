# Test Report — 2026-08-06 — Phase 01 baseline

## Test Results Overview
- **Total**: 5 commands run
- **Passed**: 3 | **Failed**: 2 | **Skipped**: 0
- **Duration**: 176.40s for the QEMU network batch

## Coverage Metrics
- **Lines**: n/a
- **Branches**: n/a
- **Functions**: n/a
- **Threshold**: 80%
- **Status**: unavailable because `cargo llvm-cov` is not installed in this checkout (`error: no such command: llvm-cov`)

## Build Status
- `cargo fmt --all --check`: PASS
- `cargo check -p vicell-kernel --target riscv64gc-unknown-none-elf -Z build-std=core,alloc`: PASS
- `bash scripts/build-test-hooks-ci.sh`: PASS
- `bash scripts/measure-coverage.sh`: FAIL, tool missing
- Worktree: clean after restoring generated `kernel/src/embedded-test-hooks/init`

## Failed Tests
### `tests/boot.rs` — `network_dhcp_acquires_ip`
- **Error**: `DHCP did not complete: timeout: pattern "DHCP acquired" not seen in 40s`
- **Stack**: `tests/boot.rs:169:9`

### `tests/boot.rs` — `network_tcp_send_recv`
- **Error**: `DHCP failed: timeout: pattern "DHCP acquired" not seen in 40s`
- **Stack**: `tests/boot.rs:200:29`

### `tests/boot.rs` — `network_curl_http_get`
- **Error**: `DHCP failed: timeout: pattern "DHCP acquired" not seen in 40s`
- **Stack**: `tests/boot.rs:237:29`

### `tests/boot.rs` — `network_tcp_listen_accept`
- **Error**: `DHCP failed: timeout: pattern "DHCP acquired" not seen in 40s`
- **Stack**: `tests/boot.rs:265:29`

## Critical Issues
1. The RV64/QEMU network boot lane is not acquiring DHCP, so every selected network integration test times out before the TCP/HTTP assertions can run.
2. Coverage cannot be measured until `cargo llvm-cov` is installed or added to the environment.

## Recommendations
1. Investigate the QEMU NIC/DHCP path in `tests/boot.rs` and the associated boot image/network setup before any source change is considered green.
2. Install `cargo-llvm-cov` or document an alternate coverage path, then rerun the baseline coverage gate.

## Unresolved Questions
- Is the DHCP timeout environment-driven, or is the RV64 NIC producer path regressed before the network tests reach their protocol assertions?
