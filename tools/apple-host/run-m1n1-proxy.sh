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

# m1n1 checkout with a built stage-2 `build/m1n1.macho`; the hypervisor boots
# the Image through it, with the device tree (Linux `apple/t8103-j274.dtb`)
# appended as a payload because the Apple backend requires a firmware DTB.
M1N1_DIR="${M1N1_DIR:-/tmp/m1n1-tools}"
DTB="${M1N1_DTB:-}"

if [[ -z "$DTB" || ! -f "$DTB" ]]; then
    echo "ERROR: set M1N1_DTB to the t8103-j274 device tree blob" >&2
    exit 1
fi

echo "==> Deploying Cellos to Mac mini M1 via m1n1 proxy hypervisor..."
echo "    Image: $KERNEL_IMAGE ($(wc -c < "$KERNEL_IMAGE") bytes)"
echo "    Device: $TTY"

# The proxy client finds the device through M1N1DEVICE (default /dev/m1n1).
M1N1DEVICE="$TTY" python3 "$M1N1_DIR/proxyclient/tools/run_guest.py" \
    "$M1N1_DIR/build/m1n1.macho" -a "$DTB" -a "$KERNEL_IMAGE"
