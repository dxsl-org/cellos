#!/usr/bin/env bash
# Build an isolated RV64 kernel whose native-domain assertions are available only
# through test hooks. Production build outputs and feature tuples are untouched.
#
# The lane also packs the phase-03 step-5 Tier-2 grant pair (owner + receiver)
# into a THROWAWAY VIFS1 image and installs the two cells on the two *reviewed*
# Tier-2 launch paths the kernel already admits. A brand-new `/bin/<name>` would
# need a kernel-side launch-edge row (`kernel/src/loader/launch_profile/targets.rs`)
# and a boot-ceiling row; this ticket owns only `cells/tests/**` and the lane
# scripts, so it reuses the reviewed rows and keeps the substitution confined to
# this lane's fresh image. The real `/bin/tier2-smoke` and `/bin/tier2-exploit`
# binaries in the shared `kernel/src/embedded-test-hooks` image and in the disk
# images are left untouched — only this throwaway image maps the grant-pair cells
# onto those paths, and every marker they emit is namespaced
# `S22-RV64-GRANT-PAIR-*` so a reader can never mistake them for the tier2 cells.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if command -v python3 >/dev/null 2>&1 && python3 -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python3
elif command -v python >/dev/null 2>&1 && python -c 'import sys' >/dev/null 2>&1; then
    PYTHON_BIN=python
else
    echo "FAIL: a working Python 3 interpreter is required" >&2
    exit 1
fi

REL="target/riscv64gc-unknown-none-elf/release"
TH_DIR="kernel/src/embedded-test-hooks"
DOMAIN_TARGET="target/native-domain-test"
DOMAIN_EMBED="$DOMAIN_TARGET/embedded"
DOMAIN_KERNEL="$REL/cellos-kernel-native-domain-test"

# Reuse the signed test fixture/image construction. It never enables
# native-domains; that feature is compiled only into the isolated kernel below.
bash scripts/build-test-hooks-ci.sh

echo "==> Building the phase-03 Tier-2 grant pair cells..."
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p tier2-grant-owner -p tier2-grant-receiver

OWNER_ELF="$REL/tier2-grant-owner"
RECEIVER_ELF="$REL/tier2-grant-receiver"
for elf in "$OWNER_ELF" "$RECEIVER_ELF"; do
    [[ -f "$elf" ]] || { echo "FAIL: grant pair cell missing: $elf" >&2; exit 1; }
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing the Tier-2 grant pair cells..."
sign_cells "$OWNER_ELF" "$RECEIVER_ELF"

echo "==> Assembling the throwaway grant-pair VIFS1 image..."
rm -rf "$DOMAIN_TARGET"
mkdir -p "$DOMAIN_EMBED"
cp "$TH_DIR/init" "$DOMAIN_EMBED/init"

TMPDIR_KFS=$(mktemp -d)
trap 'rm -rf "$TMPDIR_KFS"' EXIT
printf 'ViCell-test' > "$TMPDIR_KFS/hostname"

# shellcheck source=scripts/lib-bake-policy.sh
source scripts/lib-bake-policy.sh
bake_policy "$TMPDIR_KFS/POLICY.BIN"

# Same cell set as the shared test-hooks image, plus the grant pair installed on
# the two reviewed Tier-2 launch paths this lane's shell edge admits.
"$PYTHON_BIN" tools/mkfat32.py \
    "$DOMAIN_EMBED/kernel_fs.img" \
    "$REL/app-init"         /bin/init \
    "$REL/app-shell"        /bin/shell \
    "$REL/service-vfs"      /bin/vfs \
    "$REL/service-config"   /bin/config \
    "$REL/vfs-test"         /bin/vfs-test \
    "$REL/service-net"      /bin/net \
    "$REL/driver-virtio-net" /bin/virtio-net \
    "$REL/atomic-publication-probe" /bin/atomic-probe \
    "$OWNER_ELF"            /bin/tier2-smoke \
    "$RECEIVER_ELF"         /bin/tier2-exploit \
    "$TMPDIR_KFS/hostname"  /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

[[ -f "$DOMAIN_EMBED/kernel_fs.img" ]] || {
    echo "FAIL: mkfat32.py did not produce the grant-pair kernel_fs.img" >&2; exit 1; }

# Prove the layout rather than trusting the exit code: mkfat32.py exits 0 for a
# well-formed image whose destination paths went astray, and then the boot fails
# as a confusing "cell not found".
"$PYTHON_BIN" tools/inspect_fat.py "$DOMAIN_EMBED/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
for required in "LFN 'vfs-test'" "LFN 'atomic-probe'" "LFN 'tier2-smoke'" "LFN 'tier2-exploit'"; do
    if ! grep -q -- "$required" "$TMPDIR_KFS/fat-layout.txt"; then
        echo "FAIL: grant-pair kernel_fs.img lacks $required" >&2
        cat "$TMPDIR_KFS/fat-layout.txt" >&2
        exit 1
    fi
done
assert_policy_in_image "$TMPDIR_KFS/fat-layout.txt" || exit 1

EMBEDDED_OVERRIDE="$DOMAIN_EMBED" \
CARGO_TARGET_DIR="$DOMAIN_TARGET" \
RUSTFLAGS="-D warnings -C relocation-model=pic" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    --features test-hooks,native-domains \
    -p cellos-kernel

mkdir -p "$REL"
cp "$DOMAIN_TARGET/riscv64gc-unknown-none-elf/release/cellos-kernel" "$DOMAIN_KERNEL"
printf 'PASS: native-domain test-hooks kernel: %s\n' "$DOMAIN_KERNEL"
