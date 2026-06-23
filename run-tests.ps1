# Run ViCell integration tests.
#
# Integration tests are host binaries (they spawn QEMU as a subprocess).
# The .cargo/config.toml at the repo root sets target=riscv64gc, so we must
# cd into tests/integration/ to let its own .cargo/config.toml override the target.
#
# Usage:
#   ./run-tests.ps1                     # run all integration tests
#   ./run-tests.ps1 boot                # run only the "boot" test suite
#   ./run-tests.ps1 boot input_bare_cell # run specific test function
#
# Layer-2 hardware security tests (BTI, MTE, CET-IBT, PKU):
#   These tests require QEMU -cpu max and kernel built with test-hooks feature.
#   They are NOT run by this script (which targets the default RISC-V test suite).
#   Run them manually:
#
#   ARM64 (BTI + MTE):
#     $env:RUSTFLAGS = "-C relocation-model=pic -C target-feature=+bti,+paca,+pacg"
#     cargo build --release -p vicell-kernel --target aarch64-unknown-none-softfloat --features test-hooks
#     $env:RUSTFLAGS = $null
#     qemu-system-aarch64 -machine virt,gic-version=2 -cpu max,+mte -m 256M -nographic \
#       -kernel target/aarch64-unknown-none-softfloat/release/vicell-kernel -no-reboot \
#       -drive if=none,file=disk_arm_virt.img,format=raw,id=hd0 \
#       -device virtio-blk-device,drive=hd0 2>&1 | Select-String "SELFTEST|BTI|MTE|cfi-test"
#
#   x86_64 (CET-IBT + PKU):
#     cargo build --release -p vicell-kernel --target x86_64-unknown-none --features test-hooks
#     # Then boot via run-x86.ps1 (builds ISO + launches QEMU -cpu max) and grep serial output
#     # for "SELFTEST|CET|PKU|cfi-test"
#
#   Expected output:
#     [SELFTEST] MTE-SELFTEST: PASS   (or SKIP if QEMU version < 6.2 without MTE)
#     [SELFTEST] PKU-SELFTEST: PASS   (or SKIP if CPU lacks PKU)
#     cfi-test: SKIP: BTI/CET-IBT not enforced  (on baseline QEMU without -cpu max)

param(
    [string]$Suite = "",
    [string]$TestName = ""
)

$repo = Get-Location

Push-Location "$repo\tests\integration"
try {
    $args_list = @("test")
    if ($Suite)     { $args_list += "--test"; $args_list += $Suite }
    if ($TestName)  { $args_list += $TestName }
    cargo @args_list
} finally {
    Pop-Location
}

# ─── Layer-2 Hardware Security Test Status ────────────────────────────────────
# Layer-2 tests (BTI/MTE/CET-IBT/PKU) run as QEMU-direct invocations, not via
# the cargo integration test harness, because they require:
#   - -cpu max (enables BTI, PAC, MTE, CET, PKU on QEMU)
#   - kernel built with --features test-hooks (enables SELFTEST boot prints)
#   - arch-specific kernel builds (aarch64 / x86_64, not the default riscv64)
#
# See the comment header above for manual invocation commands.
# Tracked in: docs/specs/10-testing.md § Layer-2 Hardware Security Tests
