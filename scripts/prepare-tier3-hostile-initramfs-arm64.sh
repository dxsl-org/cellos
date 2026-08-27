#!/bin/bash
set -eo pipefail

if [ ! -f ".alpine-cache/initramfs-virt" ]; then
    echo "Error: Base Alpine initramfs not found. Please run scripts/fetch-alpine-artifacts.sh first."
    exit 1
fi

echo "Building tier3-hostile-probe..."
cd tests/guests/tier3-hostile-probe
cargo build --release --target aarch64-unknown-none-softfloat
cd ../../../

echo "Repacking initramfs..."
mkdir -p build
python3 tools/repack-initramfs.py .alpine-cache/initramfs-virt build/tier3-hostile-initramfs-arm64.cpio.gz \
    --add bin/tier3-hostile-probe tests/guests/tier3-hostile-probe/target/aarch64-unknown-none-softfloat/release/tier3-hostile-probe 100755 \
    --add bin/sh tests/guests/tier3-hostile-probe/guest-init.sh 100755

echo "Initramfs ready at build/tier3-hostile-initramfs-arm64.cpio.gz"
