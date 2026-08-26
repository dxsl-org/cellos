# Phase 04: Handoff Integration Tests

**Priority:** Medium  
**Status:** ☐ Todo  
**Blocked by:** Phase 03  
**Estimate:** 1–2 giờ

## Context Links

- `tests/integration/src/lib.rs` — QemuRunner, boot_* helpers (thêm boot_x86_32, boot_aarch32)
- `tests/integration/tests/handoff.rs` — handoff test file (thêm Phase 05 + 06)
- `tests/integration/Cargo.toml` — không cần thay đổi
- `hal/arch/x86/src/x86_32/boot.rs` — multiboot1 → QEMU `-kernel` mode
- `hal/arch/arm/src/aarch32/boot.rs` — QEMU ARM virt `-kernel` mode

## Overview

Thêm handoff tests cho x86_32 và AArch32 vào file `tests/integration/tests/handoff.rs` hiện tại (Phase 05 và 06). Cũng thêm `QemuRunner::boot_x86_32` và `QemuRunner::boot_aarch32` vào `lib.rs`.

Cả hai boot qua QEMU `-kernel` (không cần ISO), giống RV32 pattern.

## QEMU Command Lines

### x86_32 (i686)
```bash
qemu-system-i386 \
  -M pc \
  -m 128M \
  -nographic \
  -kernel <kernel_elf> \
  -serial tcp:127.0.0.1:<port>,server,nowait \
  -no-reboot \
  -d guest_errors
```

QEMU binary: `qemu-system-i386`

### AArch32 (ARMv7-A)
```bash
qemu-system-arm \
  -M virt \
  -cpu cortex-a15 \
  -m 256M \
  -nographic \
  -kernel <kernel_elf> \
  -serial tcp:127.0.0.1:<port>,server,nowait \
  -no-reboot
```

QEMU binary: `qemu-system-arm`

## Boot Markers Expected

Giống RV32 và AArch64 (bare physical + heap):

| Order | Marker | Timeout |
|-------|--------|---------|
| 1 | `[ViCell] kernel boot v` | 10s |
| 2 | `kernel_phys_base=0x0000000000100` (x86) / `0x0000000040080` (arm) | 12s |
| 3 | `Paging: bare physical` | 12s |
| 4 | `Heap initialized` | 15s |

**Lưu ý về phys_base format:** Kernel print 16-digit zero-padded hex. 
- x86_32: `0x00100000` → `0x0000000000100000` → prefix `0x0000000000100`
- AArch32: `0x40080000` → `0x0000000040080000` → prefix `0x0000000040080`

**Lưu ý về `Scheduler initialized`:** Có thể thêm nếu scheduler hoạt động. Nano profile có thể không reach scheduler nếu không có idle task — kiểm tra sau khi boot.

## Requirements

### Functional
- [ ] `QemuRunner::boot_x86_32(kernel: &str)` function
- [ ] `QemuRunner::boot_aarch32(kernel: &str)` function
- [ ] `qemu_binary_x86_32()` → `"qemu-system-i386"`
- [ ] `qemu_binary_aarch32()` → `"qemu-system-arm"`
- [ ] 4 test functions cho x86_32: `kernel_starts`, `bare_paging`, `heap` (+ optional `phys_base`)
- [ ] 4 test functions cho AArch32: `kernel_starts`, `bare_paging`, `heap`
- [ ] Graceful skip khi kernel không build hoặc QEMU không trên PATH

## Implementation Steps

### Step 1 — Thêm vào `tests/integration/src/lib.rs`

```rust
pub fn qemu_binary_x86_32() -> &'static str { "qemu-system-i386" }
pub fn qemu_binary_aarch32() -> &'static str { "qemu-system-arm" }
```

```rust
impl QemuRunner {
    /// Boot x86_32 kernel via QEMU -kernel (multiboot1, bare physical)
    pub fn boot_x86_32(kernel: &str) -> Self {
        let port = Self::next_port();
        let child = Command::new(qemu_binary_x86_32())
            .args([
                "-M", "pc",
                "-m", "128M",
                "-nographic",
                "-kernel", kernel,
                "-serial", &format!("tcp:127.0.0.1:{port},server,nowait"),
                "-no-reboot",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn qemu-system-i386");
        Self::connect_and_wrap(child, port)
    }

    /// Boot AArch32 kernel via QEMU -kernel (ARM virt, Cortex-A15, bare physical)
    pub fn boot_aarch32(kernel: &str) -> Self {
        let port = Self::next_port();
        let child = Command::new(qemu_binary_aarch32())
            .args([
                "-M", "virt",
                "-cpu", "cortex-a15",
                "-m", "256M",
                "-nographic",
                "-kernel", kernel,
                "-serial", &format!("tcp:127.0.0.1:{port},server,nowait"),
                "-no-reboot",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn qemu-system-arm");
        Self::connect_and_wrap(child, port)
    }
}
```

**Lưu ý:** Cần xem `lib.rs` để hiểu `Self::next_port()` và `Self::connect_and_wrap()` — adapt đúng theo pattern hiện tại.

### Step 2 — Thêm vào `tests/integration/tests/handoff.rs`

Thêm path helpers:
```rust
fn x86_32_kernel_path() -> String {
    repo_root()
        .join("target/i686-unknown-none/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}

fn aarch32_kernel_path() -> String {
    repo_root()
        .join("target/armv7a-none-eabi/release/vicell-kernel")
        .to_string_lossy()
        .into_owned()
}
```

Thêm prerequisite guards:
```rust
fn x86_32_prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(x86_32_kernel_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_x86_32())
        .arg("--version").output().is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP: x86_32 kernel not built ({})\n  build: cargo build --target i686-unknown-none -p vicell-kernel --release",
            x86_32_kernel_path()
        );
    }
    if !qemu_ok { eprintln!("SKIP: qemu-system-i386 not on PATH"); }
    kernel_ok && qemu_ok
}

fn aarch32_prerequisites_ok() -> bool {
    let kernel_ok = PathBuf::from(aarch32_kernel_path()).exists();
    let qemu_ok = std::process::Command::new(qemu_binary_aarch32())
        .arg("--version").output().is_ok();
    if !kernel_ok {
        eprintln!(
            "SKIP: AArch32 kernel not built ({})\n  build: cargo build --target armv7a-none-eabi -p vicell-kernel --release",
            aarch32_kernel_path()
        );
    }
    if !qemu_ok { eprintln!("SKIP: qemu-system-arm not on PATH"); }
    kernel_ok && qemu_ok
}
```

Thêm Phase 05 — x86_32 tests:
```rust
// ---------------------------------------------------------------------------
// Phase 05 — x86_32 handoff tests
//
// Multiboot1 bare-metal boot on QEMU pc machine. No Limine, no ISO required.
// Bare physical (CR0.PG=0). UART via COM1 port I/O.
// ---------------------------------------------------------------------------

#[test]
fn handoff_x86_32_kernel_starts() {
    if !x86_32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_x86_32(&x86_32_kernel_path());
    qemu.wait_for("[ViCell] kernel boot v", 10)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}

#[test]
fn handoff_x86_32_bare_paging() {
    if !x86_32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_x86_32(&x86_32_kernel_path());
    qemu.wait_for("Paging: bare physical", 12)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}

#[test]
fn handoff_x86_32_heap() {
    if !x86_32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_x86_32(&x86_32_kernel_path());
    qemu.wait_for("Heap initialized", HANDOFF_TIMEOUT)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}
```

Thêm Phase 06 — AArch32 tests:
```rust
// ---------------------------------------------------------------------------
// Phase 06 — AArch32 handoff tests
//
// ARM virt machine, Cortex-A15 SVC mode. Bare physical (MMU off).
// PL011 UART at 0x09000000 (same as AArch64 virt).
// ---------------------------------------------------------------------------

#[test]
fn handoff_aarch32_kernel_starts() {
    if !aarch32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_aarch32(&aarch32_kernel_path());
    qemu.wait_for("[ViCell] kernel boot v", 10)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}

#[test]
fn handoff_aarch32_bare_paging() {
    if !aarch32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_aarch32(&aarch32_kernel_path());
    qemu.wait_for("Paging: bare physical", 12)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}

#[test]
fn handoff_aarch32_heap() {
    if !aarch32_prerequisites_ok() { return; }
    let qemu = QemuRunner::boot_aarch32(&aarch32_kernel_path());
    qemu.wait_for("Heap initialized", HANDOFF_TIMEOUT)
        .unwrap_or_else(|e| panic!("{e}\n--- output ---\n{}", qemu.dump()));
}
```

### Step 3 — Update imports trong handoff.rs

```rust
use vicell_integration_tests::{
    qemu_binary, qemu_binary_aarch64, qemu_binary_rv32, qemu_binary_x86,
    qemu_binary_x86_32, qemu_binary_aarch32,  // NEW
    QemuRunner,
};
```

## Todo List

- [ ] Thêm `qemu_binary_x86_32()` và `qemu_binary_aarch32()` vào `tests/integration/src/lib.rs`
- [ ] Thêm `QemuRunner::boot_x86_32()` vào `lib.rs`
- [ ] Thêm `QemuRunner::boot_aarch32()` vào `lib.rs`
- [ ] Thêm path helpers và prerequisite guards vào `handoff.rs`
- [ ] Thêm 3 x86_32 test functions (Phase 05)
- [ ] Thêm 3 AArch32 test functions (Phase 06)
- [ ] Update import line trong `handoff.rs`
- [ ] Verify `cargo test --manifest-path tests/integration/Cargo.toml handoff_x86_32` runs (skip nếu chưa build)
- [ ] Verify `cargo test --manifest-path tests/integration/Cargo.toml handoff_aarch32` runs (skip nếu chưa build)

## Success Criteria

- Tests compile không lỗi
- Tests skip gracefully khi kernel chưa build
- Tests skip gracefully khi QEMU binary không trên PATH
- Không break test hiện có (RV64, AArch64, RV32, x86_64 tests)

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| `qemu-system-i386` vs `qemu-system-x86_64 -M q35` | i386 binary boot được i686 kernel; nếu cần 64-bit host tools vẫn dùng được |
| `qemu-system-arm` conflict với `qemu-system-aarch64` | Binary khác, không conflict |
| Port collision trong parallel test runs | `Self::next_port()` dùng atomic counter — không conflict |
| AArch32 PL011 baud rate sai → garbage chars | Serial over TCP là raw bytes; QEMU emulate PL011 không qua baud; không vấn đề |
| Target path `i686-unknown-none` vs custom JSON name | Adjust `x86_32_kernel_path()` sau khi biết target triple thật |

## Notes

- Nếu kernel x86_32 hoặc AArch32 chưa build, tests skip với message gợi ý build command
- Tests này là regression guards cho Phase 01-03 — chạy sau khi có kernel builds
- `HANDOFF_TIMEOUT = 15` (đã define trong file) — không cần const mới
