# Phase 01 — rustls 0.23 compile spike (go/no-go gate)

## Overview
- **Priority:** P0 (blocker cho toàn bộ plan) · **Tier:** fast · **Status:** Planned
- Xác minh rustls 0.23 compile được trên `riscv64gc-unknown-none-elf` với `no_std + alloc`.
- Research (confidence 85%) xác nhận `UnbufferedServerConnection` không gate sau `std` feature. Nhưng **chưa ai chạy trên bare-metal RISC-V** — spike này là xác minh thực tế trước khi commit effort.
- Kết quả: PASS hoặc FAIL + root cause → quyết định tiếp tục hay fallback.

## Key Insight
rustls 0.23 tách `std`-gated APIs khỏi `alloc`-only APIs từ v0.23.0. `UnbufferedServerConnection::new()` và `ServerConfig::builder_with_details()` đều **không** có `#[cfg(feature = "std")]` gate. Nhưng các transitive dep (session ticket handling, cert parsing) có thể kéo `std` vào theo — spike mới biết chắc.

## Requirements
- Tạo một crate tối giản `spike-rustls-noalloc` chỉ có `Cargo.toml` + `lib.rs`
- Thêm `rustls 0.23` với `default-features = false, features = ["alloc", "hashbrown"]`
- `lib.rs` import `rustls::unbuffered::UnbufferedServerConnection` và gọi type-check (không cần logic)
- Chạy `cargo check --target riscv64gc-unknown-none-elf --no-default-features` 
- Ghi rõ kết quả: OK / lỗi gì / dep nào kéo `std`

## Nơi chạy spike
```
d:\Cellos\spike-rustls-check\
├── Cargo.toml
└── src\lib.rs
```
(gitignored, xóa sau khi spike xong)

## Implementation Steps

1. Tạo `spike-rustls-check/Cargo.toml`:
```toml
[package]
name = "spike-rustls-check"
version = "0.1.0"
edition = "2021"

[dependencies]
rustls = { version = "0.23", default-features = false, features = ["alloc", "hashbrown"] }

[lib]
path = "src/lib.rs"
```

2. Tạo `spike-rustls-check/src/lib.rs`:
```rust
#![no_std]
extern crate alloc;

use alloc::sync::Arc;
use rustls::unbuffered::UnbufferedServerConnection;
use rustls::ServerConfig;

// type-check only — never called
pub fn _spike_check(cfg: Arc<ServerConfig>) -> UnbufferedServerConnection {
    UnbufferedServerConnection::new(cfg).unwrap()
}
```

3. Chạy check (từ thư mục spike):
```powershell
cd spike-rustls-check
cargo check --target riscv64gc-unknown-none-elf 2>&1
```

4. Nếu lỗi `use of std`:
   - Tìm dep nào kéo vào: thêm `cargo tree --target riscv64gc-unknown-none-elf -e features 2>&1 | Select-String "std"`
   - Thử tắt thêm features: `features = ["alloc", "hashbrown", "no_std"]` (rustls có thể có explicit `no_std` feature)
   - Ghi root cause vào file này

5. Dọn dẹp spike dir sau khi kết quả đã ghi lại.

## Success Criteria
- `cargo check --target riscv64gc-unknown-none-elf` hoàn thành KHÔNG có `error[E0463]` hay `use of std feature` error
- `UnbufferedServerConnection` và `ServerConfig` visible + usable trong no_std context
- Ghi kết quả vào section SPIKE RESULT bên dưới trước khi chuyển P02

## SPIKE RESULT
<!-- Điền vào sau khi chạy -->
- Status: [ ] PASS / [ ] FAIL
- rustls version tested:
- Error (nếu có):
- Root cause dep (nếu có):
- Action: [ ] Continue to P02 / [ ] Apply patch / [ ] Fallback

## Fallback nếu FAIL
1. Kiểm tra rustls repo có `no_std` issue/PR đang mở không — có thể có workaround patch
2. Thử rustls `0.22` (cũng có no_std flag, API hơi khác)
3. Nếu không fix được → tạm dừng plan này; giữ Phase 04 robot-swarm ở HMAC; ghi kết quả vào project memory; revisit khi rustls cải thiện no_std support

## Risk
| Risk | Xử lý |
|------|-------|
| Session-ticket handling kéo `std` (SystemTime, HashMap std) | `hashbrown` feature thay HashMap; session tickets có thể disable |
| `ring` hoặc `aws-lc-rs` được pull transitively | Không — spike dùng `default-features = false`, chọn provider sau |
| `alloc` crate không có trong spike target | Thêm `#![no_std] extern crate alloc;` + global allocator stub nếu cần cho check |
