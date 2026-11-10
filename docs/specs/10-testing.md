# Cellos Architecture: Testing & Verification
**Version**: 0.3 (SAS-Specific Quality Assurance)
**Status**: Definitive — updated 2026-08-01 (D12 PKU test-scope correction)

---

## 1. Triết lý: Test trong "Nồi lẩu" SAS
Trong mô hình Single Address Space, một lỗi nhỏ có thể phá hủy toàn bộ hệ thống. Do đó, testing không chỉ là kiểm tra logic mà là kiểm tra ranh giới (Boundaries).

## 2. Các tầng Testing
1. **KUnit (In-Kernel Unit Tests)**:
    * Chạy trực tiếp bên trong Ring 0 (QEMU hoặc Hardware).
    * Kiểm tra các Trait thực thi của `hal/core` và các logic lõi của Nano Kernel.
2. **Cell Integration Tests**:
    * Nạp/gỡ Cell liên tục và kiểm tra các authority store thực sự: task-owned frames,
      grants, pin/quarantine state, quota, và resource registry.
3. **SAS boundary gates**:
    * Unsafe allowlist/signing checks, W^X and VA-collision negative tests,
      grant/pin/quarantine lifecycle tests, and architecture-specific protection tests.
      Không có công cụ "SASan" độc lập; mọi negative test phải gắn với cơ chế enforcement thật.

## 3. Fault Injection Cell (Kẻ phá hoại)
Một Cell đặc biệt được thiết kế để:
* Gây fault/panic trong Cell để kiểm tra terminate → reap → notify → supervisor policy.
* Chiếm dụng 99% RAM để test cơ chế **Memory Quota**.
* Gây ra **Deadlock** để test bộ phận giám sát (Watchdog).

## 4. Hardware-in-the-loop (HITL)
Đặc biệt quan trọng cho Robot:
* Kiểm tra độ trễ phản hồi (Latency) từ lúc có IRQ thực tế đến khi Driver Cell nhận được `Waker`.
* Kiểm tra việc tiêu thụ năng lượng của các Task trong trạng thái **Tickless Idle**.

---

## 5. Integration Test Guide (English)

This section documents the QEMU-driven integration test harness introduced in
Phase 11 and how to add new tests.

### Test Pyramid

```
   ┌─────────────────────────────────┐
   │  Integration (QEMU-driven)      │   ~10 tests, ~3 min
   │  tests/integration/*.rs         │
   ├─────────────────────────────────┤
   │  Boot-time kernel tests         │   ~30 tests, invoked at boot
   │  kernel/src/*/tests.rs          │
   │  kernel/src/loader/elf_tests.rs │
   ├─────────────────────────────────┤
   │  Host unit tests (cargo test)   │   15+ tests, <1 sec
   │  libs/types/src/lib.rs          │
   │  libs/api/src/syscall_tests.rs  │
   └─────────────────────────────────┘
```

### Host Unit Tests

Run with an explicit host target (the workspace default is RISC-V bare-metal):

```bash
# Windows
cargo test -p types -p api --target x86_64-pc-windows-msvc

# Linux / macOS
cargo test -p types -p api --target x86_64-unknown-linux-gnu
```

`libs/types` and `libs/api` use `#![cfg_attr(not(test), no_std)]` so that the
`#[test]` harness can link against `std` on the host while production builds
remain bare-metal.

### Boot-time Kernel Tests

These tests run inside the kernel at boot (Ring 0, no MMU isolation required).
They are `pub fn run_all()` functions called from `kernel/src/main.rs`:

```rust
// kernel/src/main.rs (boot sequence)
memory::tests::run_all();
task::tests::run_scheduler_tests();
loader::elf_tests::run_all();
```

Add a new boot-time test:

1. Create `kernel/src/<module>/my_tests.rs` with a `pub fn run_all()` function
2. Add `pub mod my_tests;` to `kernel/src/<module>.rs`
3. Call `<module>::my_tests::run_all()` from `kernel/src/main.rs`

### QEMU Integration Tests

Integration tests live in `tests/integration/` and use `QemuRunner` from
`tests/integration/harness.rs` to spawn QEMU, inject serial input, and assert
on serial output.

**Canonical pattern:**

```rust
// tests/integration/my_scenario.rs
use super::harness::QemuRunner;

const KERNEL: &str = "target/riscv64gc-unknown-none-elf/release/kernel";

pub fn test_my_feature() {
    let mut q = QemuRunner::new_rv64(KERNEL);
    q.wait_for("[Cellos]", 30).expect("kernel banner not seen");
    q.wait_for("my-expected-output", 30)
        .expect("feature output not seen");
    assert!(!q.output_contains("PANIC"));
}
```

**Adding a new integration test:**

1. Create `tests/integration/<name>.rs` using the pattern above
2. Reference the harness with `use super::harness::QemuRunner;`
3. Run via a future `Cellos-tests` host-target crate (use `--target x86_64-...`)
4. Mark QEMU-dependent tests `#[ignore]` in CI until QEMU is available on the runner

**Existing integration test files:**

| File | Covers |
|------|--------|
| `tests/integration/harness.rs` | `QemuRunner` helper |
| `tests/integration/ring3_smoke.rs` | boot banner, Ring-3 hello, shell prompt, no-panic |
| `tests/integration/multi_cell.rs` | init→config→vfs→shell chain |

### Coverage Measurement

```bash
# Requires llvm-tools-preview component
rustup component add llvm-tools-preview

# Run coverage (host-runnable crates only)
cargo llvm-cov --target x86_64-unknown-linux-gnu -p types -p api --html
# Report at: target/llvm-cov/html/index.html
```

For kernel coverage, use `scripts/measure-coverage.sh` (requires QEMU and
instrumented kernel build).

---

## 6. Hardware Security Tests

This matrix distinguishes enforcement tests from enablement or register-plumbing tests.
A feature is not an isolation boundary until a negative test proves the forbidden access
faults; an "enabled" log or register round-trip is insufficient.

| Feature | Arch | Mechanism | Expected result | QEMU flag |
|---------|------|-----------|-----------------|-----------|
| BTI | aarch64 | `cfi-test` cell: `BLR` to the instruction after a `BTI c` pad in the cell's executable `.text` | Today: `cfi-test: SKIP` (kernel sets no stage-1 GP bit on cell pages). Once armed: `[fault-probe] a64 ... ec=0xd` at the printed target | `-cpu max` |
| PAC-RET | aarch64 | Boot log `CFI: PAC-RET enabled` | No fault on normal return | `-cpu max` |
| MTE | aarch64 | Kernel self-test (tag/re-tag round-trip via STG/LDG) | `MTE-SELFTEST: PASS` | `-cpu max,+mte` |
| CET-IBT | x86_64 | `cfi-test` cell: `CALL reg` to the instruction after an `ENDBR64` in the cell's executable `.text` | Today: `cfi-test: SKIP` (kernel never writes `IA32_U_CET`). Once armed: CPL3 `#CP` (vector 21) termination | `-cpu max` |
| PKU plumbing | x86_64 | Kernel self-test (PKRU constants + kernel RDPKRU verification) | `PKU-SELFTEST: PASS`; does **not** prove page isolation | `-cpu max` |

Current PKU pages all retain PTE key 0 because the loader does not stamp bits `[62:59]`.
The self-test therefore validates PKRU computation and register access only; it does not
map differently keyed pages or provoke a protection-key `#PF`. End-to-end PKU enforcement
remains pending alongside PTE-key and shared/grant-page semantics (Spec 19 Layer C).

### Test locations

| Test | Location | Trigger |
|------|----------|---------|
| CFI violation cell | `cells/demos/cfi-test/` | Spawned from shell: `cfi-test` |
| MTE self-test | `kernel/src/layer2_selftest.rs` | Auto at boot with `test-hooks` feature |
| PKU self-test | `kernel/src/layer2_selftest.rs` | Auto at boot with `test-hooks` feature |

### Running Layer-2 tests

**ARM64 (BTI + MTE) — requires QEMU ≥ 6.2 with MTE support:**

```powershell
# 1. Build aarch64 kernel with test-hooks
$env:RUSTFLAGS = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
cargo build --release -p cellos-kernel `
    --target aarch64-unknown-none-softfloat `
    --features test-hooks
$env:RUSTFLAGS = $null

# 2. Run with -cpu max,+mte and grep Layer-2 output
qemu-system-aarch64 -machine virt,gic-version=2 -cpu "max,+mte" -m 256M -nographic `
    -kernel target/aarch64-unknown-none-softfloat/release/cellos-kernel `
    -drive if=none,file=disk_arm_virt.img,format=raw,id=hd0 `
    -device virtio-blk-device,drive=hd0 -no-reboot 2>&1 |
    Select-String "SELFTEST|BTI|MTE|cfi-test"
```

**x86_64 (CET-IBT + PKU) — requires QEMU ≥ 7.0:**

```powershell
# 1. Build x86_64 kernel with test-hooks
cargo build --release -p cellos-kernel `
    --target x86_64-unknown-none `
    --features test-hooks

# 2. Boot via run-x86.ps1 (builds Limine ISO + launches QEMU -cpu max)
#    then grep serial for PKU-SELFTEST and cfi-test output
.\run-x86.ps1 2>&1 | Select-String "SELFTEST|CET|PKU|cfi-test"
```

### Test availability and SKIP behaviour

The kernel self-tests print `SKIP` (never `FAIL`) when the feature is absent from
the hardware or QEMU CPU model:

- `MTE-SELFTEST: SKIP` — QEMU does not emulate MTE without `+mte` flag
- `PKU-SELFTEST: SKIP` — CPU lacks PKU (CPUID leaf 7 ECX[3] = 0)

`cfi-test` runs a control call first, then the violation, and reports one of:

- `cfi-test: control OK: ...` — an indirect call to the landing pad ran, so the
  target page is executable and a later fault cannot be NX.
- **Kernel fault log, no further cell output** — enforcement works: aarch64
  `[fault-probe] a64 ... ec=0xd ... pc=<non-landing-pad target>`; x86_64 a CPL3
  vector-21 (`#CP`) termination. The cell cannot print a PASS after dying.
- `cfi-test: SKIP: BTI not enforced for EL0: ...` / `cfi-test: SKIP: CET-IBT not
  enforced at CPL3: ...` followed by a `cfi-test: reason:` line, exit 0 — the
  violating call returned. This is the current result on every CPU model,
  including `-cpu max` with `CFI: BTI enabled`: BTI only checks branches into
  guarded pages and no cell page carries the GP bit; x86 enables supervisor IBT
  only (`IA32_S_CET`), never user IBT (`IA32_U_CET`).
- `cfi-test: SKIP: riscv64 forward-edge CFI (Zicfilp landing pads) is not enabled
  by the kernel; nothing to probe` — riscv64 does not probe.
- `cfi-test: FAIL: ...`, exit 1 — the control call (or the returned value) was
  wrong; the test itself is broken.

The `SKIP` outputs let CI run on baseline QEMU (without `-cpu max`) without producing
false failures. Explicit `-cpu max` is necessary to exercise supported instructions, but
PKU enforcement still requires the missing denied-page-access test described above, and
BTI/IBT enforcement for cells requires the kernel gaps above to be closed.
