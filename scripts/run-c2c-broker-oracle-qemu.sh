#!/usr/bin/env bash
# Build and run the local C2C broker oracle without touching shared build artifacts.

set -euo pipefail
umask 077

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="riscv64gc-unknown-none-elf"
QEMU_BIN="${ViCell_QEMU:-qemu-system-riscv64}"

for tool in cargo rustc mktemp cp chmod rm mkdir grep sed setsid sleep truncate "$QEMU_BIN"; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "FAIL: required tool not found: $tool" >&2
        exit 1
    }
done

if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys; raise SystemExit(sys.version_info < (3, 8))'; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys; raise SystemExit(sys.version_info < (3, 8))'; then
    PYTHON_BIN=python
else
    echo "FAIL: Python 3.8 or newer is required" >&2
    exit 1
fi
export PYTHONDONTWRITEBYTECODE=1

for required in \
    tools/mkfat32.py \
    tools/inspect_fat.py \
    scripts/sign-policy.py \
    scripts/lib-run-scoped-workspace.sh \
    tests/integration/Cargo.toml; do
    [[ -f "$required" ]] || {
        echo "FAIL: required artifact not found: $required" >&2
        exit 1
    }
done

# shellcheck source=scripts/lib-run-scoped-workspace.sh
source scripts/lib-run-scoped-workspace.sh

CC_riscv64gc_unknown_none_elf="${CC_riscv64gc_unknown_none_elf:-riscv64-unknown-elf-gcc}"
command -v "$CC_riscv64gc_unknown_none_elf" >/dev/null 2>&1 || {
    echo "FAIL: RV64 cross compiler not found: $CC_riscv64gc_unknown_none_elf" >&2
    exit 1
}
export CC_riscv64gc_unknown_none_elf
export CFLAGS_riscv64gc_unknown_none_elf="${CFLAGS_riscv64gc_unknown_none_elf:--march=rv64gc -mabi=lp64d -mcmodel=medany -ffreestanding -DLFS_NO_INTRINSICS -I$ROOT/third_party/freestanding-include}"
export CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS="${CARGO_TARGET_RISCV64GC_UNKNOWN_NONE_ELF_RUSTFLAGS:--C relocation-model=pic}"

WORK_PREFIX="${TMPDIR:-/tmp}/cellos-c2c-oracle"
cleanup_stale_run_scoped_workspaces "$WORK_PREFIX"
WORKLOAD_PID=
WORKLOAD_START=

WORK="$(mktemp -d "$WORK_PREFIX.XXXXXXXX")"
cleanup() {
    local status=$?
    trap - EXIT
    terminate_run_scoped_process_group "$WORKLOAD_PID" "$WORKLOAD_START"
    if [[ "$WORKLOAD_PID" =~ ^[0-9]+$ ]]; then
        wait "$WORKLOAD_PID" 2>/dev/null || true
    fi
    rm -rf -- "$WORK"
    exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
chmod 0700 "$WORK"
printf '%s\n' "$$" > "$WORK/owner.pid"

export CARGO_TARGET_DIR="$WORK/target"
EMBEDDED="$WORK/embedded"
DISK="$WORK/disk_v3.img"
KEY_FILE="$WORK/cluster.key"
mkdir -m 0700 "$EMBEDDED"
truncate -s 64M "$DISK"
"$PYTHON_BIN" -c 'import os, pathlib, sys; pathlib.Path(sys.argv[1]).write_bytes(os.urandom(32))' "$KEY_FILE"
chmod 0600 "$KEY_FILE"

COMMON_PACKAGES=(
    -p app-init
    -p app-shell
    -p service-config
    -p service-platform
    -p driver-virtio-blk
    -p driver-virtio-net
    -p service-net
    -p service-net-broker
    -p app-bench
)

echo "[c2c-oracle-runner] building isolated RV64 oracle cells"
cargo build --quiet --locked --release --target "$TARGET" "${COMMON_PACKAGES[@]}"
cargo build --quiet --locked --release --target "$TARGET" \
    -p service-net -p service-net-broker -p app-bench \
    --features service-net/ipc-wake-oracle,service-net-broker/restart-oracle,app-bench/restart-oracle
cargo build --quiet --locked --release --target "$TARGET" \
    -p app-init --features c2c-broker
CELLOS_C2C_ORACLE_K1_FILE="$KEY_FILE" \
    cargo build --quiet --locked --release --target "$TARGET" \
        -p service-vfs --features c2c-oracle-k1-fixture
rm -f -- "$KEY_FILE"

REL="$CARGO_TARGET_DIR/$TARGET/release"
CELL_BINARIES=(
    "$REL/app-init"
    "$REL/app-shell"
    "$REL/service-vfs"
    "$REL/service-config"
    "$REL/platform"
    "$REL/driver-virtio-blk"
    "$REL/driver-virtio-net"
    "$REL/service-net"
    "$REL/service-net-broker"
    "$REL/bench"
    "$REL/bench-probe"
)
for binary in "${CELL_BINARIES[@]}"; do
    [[ -s "$binary" ]] || {
        echo "FAIL: isolated cell build did not produce: $binary" >&2
        exit 1
    }
done

# This local dev kernel intentionally keeps `signing-required` disabled. Signing
# would claim the repository-wide F1/F5 production gate passed; unrelated open
# governance findings must remain fail-closed instead of being bypassed here.


"$PYTHON_BIN" scripts/sign-policy.py --out "$WORK/POLICY.BIN" >/dev/null
printf 'Cellos-C2C-Oracle\n' > "$WORK/hostname"
printf 'Cellos C2C broker oracle\n' > "$WORK/readme.txt"

"$PYTHON_BIN" tools/mkfat32.py \
    "$EMBEDDED/kernel_fs.img" \
    "$REL/app-init"          /bin/init \
    "$REL/app-shell"         /bin/shell \
    "$REL/service-vfs"       /bin/vfs \
    "$REL/service-config"    /bin/config \
    "$REL/platform"          /bin/platform \
    "$REL/driver-virtio-blk" /bin/block \
    "$REL/driver-virtio-net" /bin/virtio-net \
    "$REL/service-net"       /bin/net \
    "$REL/service-net-broker" /bin/net-broker \
    "$REL/bench"             /bin/bench \
    "$REL/bench-probe"       /bin/bench-probe \
    "$WORK/hostname"         /etc/hostname \
    "$WORK/readme.txt"       /readme.txt \
    "$WORK/POLICY.BIN"       /POLICY.BIN
cp -- "$REL/app-init" "$EMBEDDED/init"

"$PYTHON_BIN" tools/inspect_fat.py "$EMBEDDED/kernel_fs.img" > "$WORK/fat-layout.txt"
for required in \
    "LFN 'vfs'" \
    "LFN 'virtio-net'" \
    "LFN 'net-broker'" \
    "LFN 'bench'" \
    "LFN 'bench-probe'" \
    "SFN POLICY.BIN"; do
    grep -Fq -- "$required" "$WORK/fat-layout.txt" || {
        echo "FAIL: isolated VIFS1 image is missing $required" >&2
        exit 1
    }
done

# EMBEDDED_OVERRIDE is scoped to this kernel build. Both the embedded key-bearing
# VFS ELF and the resulting kernel stay below the private workspace.
echo "[c2c-oracle-runner] building isolated RV64 kernel"
EMBEDDED_OVERRIDE="$EMBEDDED" cargo build --quiet --locked --release --target "$TARGET" -p cellos-kernel
KERNEL="$REL/cellos-kernel"
[[ -s "$KERNEL" ]] || {
    echo "FAIL: isolated kernel build did not produce $KERNEL" >&2
    exit 1
}

HOST_TARGET="$(rustc -vV | sed -n 's/^host: //p')"
[[ -n "$HOST_TARGET" ]] || {
    echo "FAIL: unable to determine the host Rust target" >&2
    exit 1
}

echo "[c2c-oracle-runner] booting real QEMU oracle"
setsid env \
    CELLOS_C2C_ORACLE_KERNEL="$KERNEL" \
    CELLOS_C2C_ORACLE_DISK="$DISK" \
    ViCell_QEMU="$QEMU_BIN" \
    cargo test --quiet \
        --locked \
        --manifest-path tests/integration/Cargo.toml \
        --target "$HOST_TARGET" \
        --test c2c-broker-oracle \
        local_c2c_broker_oracle_meets_baseline_contract \
        -- --exact --nocapture &
WORKLOAD_PID=$!
WORKLOAD_START="$(run_scoped_process_start "$WORKLOAD_PID")"
printf '%s %s\n' "$WORKLOAD_PID" "$WORKLOAD_START" > "$WORK/workload.pid"
wait "$WORKLOAD_PID"
