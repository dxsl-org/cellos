#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# Run Cellos AArch64 raw Image on Apple Silicon bare-metal via m1n1 proxy hypervisor.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL_IMAGE="${1:-$ROOT/build/cellos-apple-t8103.Image}"
TTY="${M1N1_TTY:-/dev/ttyACM0}"

if [[ ! -f "$KERNEL_IMAGE" ]]; then
    echo "ERROR: Kernel image not found: $KERNEL_IMAGE" >&2
    echo "Build with: cargo build --release -p cellos-kernel --target aarch64-unknown-none-softfloat --features board-apple-t8103" >&2
    echo "            bash scripts/build-aarch64-image.sh target/aarch64-unknown-none-softfloat/release/cellos-kernel $KERNEL_IMAGE" >&2
    exit 1
fi

if [[ ! -e "$TTY" ]]; then
    echo "WARN: USB CDC-ACM device $TTY not found." >&2
    echo "Ensure the Mac mini M1 is connected via USB-C and m1n1 proxy mode is active." >&2
    echo "If using WSL2, attach the device via Windows PowerShell: usbipd attach --wsl --busid <BUSID>" >&2
fi

M1N1_CLIENT="${M1N1_CLIENT_DIR:-/tmp/m1n1-tools/proxy/client}"

if [[ -d "$M1N1_CLIENT" ]]; then
    export PYTHONPATH="$M1N1_CLIENT:${PYTHONPATH:-}"
fi

echo "==> Deploying Cellos to Mac mini M1 via m1n1 proxy hypervisor..."
echo "    Image: $KERNEL_IMAGE ($(wc -c < "$KERNEL_IMAGE") bytes)"
echo "    Device: $TTY"

python3 -m m1n1.run_guest --vuart "$KERNEL_IMAGE"
