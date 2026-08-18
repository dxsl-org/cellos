#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

check_file() {
    local path="$1"
    if [[ ! -f "$path" ]]; then
        printf 'missing required board asset: %s\n' "$path" >&2
        exit 1
    fi
}

check_readme_command() {
    local readme="$1"
    local expected="$2"
    if ! grep -Fxq "$expected" "$readme"; then
        printf 'README build command drifted: %s\nexpected: %s\n' "$readme" "$expected" >&2
        exit 1
    fi
}

run() {
    printf '==> %s\n' "$*"
    "$@"
}

failures=()

record_run() {
    local label="$1"
    shift
    set +e
    "$@"
    local status=$?
    set -e
    if [[ $status -ne 0 ]]; then
        failures+=("$label")
    fi
}

expect_compile_error() {
    local label="$1"
    local expected="$2"
    shift 2
    local output
    printf '==> %s\n' "$*"
    set +e
    output="$("$@" 2>&1)"
    local status=$?
    set -e
    printf '%s\n' "$output"
    if [[ $status -eq 0 ]]; then
        printf 'expected failure but command succeeded: %s\n' "$*" >&2
        failures+=("$label")
        return
    fi
    if [[ "$output" != *"$expected"* ]]; then
        printf 'unexpected failure output for: %s\n' "$*" >&2
        failures+=("$label")
    fi
}

board_dirs=(
    "boards/qemu/virt-riscv64"
    "boards/qemu/virt-aarch64"
    "boards/starfive/visionfive-2"
    "boards/milk-v/pioneer"
    "boards/raspberry-pi/3-model-b"
    "boards/raspberry-pi/4-model-b"
)

for dir in "${board_dirs[@]}"; do
    check_file "$dir/README.md"
    check_file "$dir/board.rs"
done

check_file "boards/qemu/virt-riscv64/qemu-virt-riscv64.dts"
check_file "boards/qemu/virt-aarch64/qemu-virt-aarch64.dts"
check_file "boards/starfive/visionfive-2/starfive-visionfive-2.dts"
check_file "boards/milk-v/pioneer/milk-v-pioneer.dts"
check_file "boards/raspberry-pi/3-model-b/raspberry-pi-3-model-b.dts"
check_file "boards/raspberry-pi/4-model-b/raspberry-pi-4-model-b.dts"

check_readme_command \
    "boards/qemu/virt-riscv64/README.md" \
    'cargo build -p cellos-kernel --target riscv64gc-unknown-none-elf'
check_readme_command \
    "boards/qemu/virt-aarch64/README.md" \
    'cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat'
check_readme_command \
    "boards/starfive/visionfive-2/README.md" \
    'RUSTFLAGS="-C relocation-model=pic" cargo build -p cellos-kernel --release --target riscv64gc-unknown-none-elf --features board-vf2'
check_readme_command \
    "boards/milk-v/pioneer/README.md" \
    'RUSTFLAGS="-C relocation-model=pic" cargo build -p cellos-kernel --release --target riscv64gc-unknown-none-elf --features board-pioneer'
check_readme_command \
    "boards/raspberry-pi/3-model-b/README.md" \
    'cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3'
check_readme_command \
    "boards/raspberry-pi/4-model-b/README.md" \
    'cargo build -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi4'

run bash scripts/check-hal-boundaries.sh

record_run boards-crate \
    run cargo check -p cellos-boards --target x86_64-unknown-linux-gnu
record_run qemu-rv64 \
    run cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf
record_run qemu-aarch64 \
    run cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat
record_run vf2 \
    run cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2
record_run pioneer \
    run cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-pioneer
record_run rpi3 \
    run cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3
record_run rpi4 \
    run cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi4

expect_compile_error \
    riscv-conflict \
    'Conflicting RISC-V board features: `board-vf2` and `board-pioneer` cannot be enabled together.' \
    cargo check -p cellos-kernel --target riscv64gc-unknown-none-elf --features board-vf2,board-pioneer

expect_compile_error \
    aarch64-conflict \
    'Conflicting AArch64 board features: `board-rpi3` and `board-rpi4` cannot be enabled together.' \
    cargo check -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-rpi3,board-rpi4

if [[ ${#failures[@]} -ne 0 ]]; then
    printf 'board configuration matrix failed: %s\n' "${failures[*]}" >&2
    exit 1
fi
