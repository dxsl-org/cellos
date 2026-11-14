#!/usr/bin/env bash
# Build an isolated RV64 kernel whose native-domain assertions are available only
# through test hooks. Production build outputs and feature tuples are untouched.
#
# The lane also packs the phase-03 step-5 Tier-2 grant pair (owner + receiver)
# into a THROWAWAY VIFS1 image, on the two reviewed launch paths whose real
# services this image does not build — `/bin/silo` (owner) and
# `/bin/net-broker` (receiver) — and a fixture launcher built with
# `tier2-grant-pair`, which launches the pair from the boot order. That is the
# AArch64 test-hooks arrangement (Phase 15): the guest shell refuses to
# background an external cell inside a serial session, so the pair can no
# longer be typed at it. A brand-new `/bin/<name>` would need a kernel-side
# launch-edge row and a boot-ceiling row, so the reviewed rows are reused and the
# substitution stays confined to this lane's fresh image: the shared
# `kernel/src/embedded-test-hooks` image and its launcher are left untouched,
# and every marker the pair emits is namespaced `S22-RV64-GRANT-PAIR-*`.
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

OUT_REL="target/riscv64gc-unknown-none-elf/release"
# build-test-hooks-ci.sh builds its cells under its own cargo target directory
# (CELLOS_TEST_HOOKS_TARGET_DIR, default target/test-hooks) and copies only its
# kernel back to OUT_REL. Reading cells from OUT_REL therefore packed stale (or
# missing) binaries, so resolve the same directory once and export it so the
# child script and this one cannot disagree.
export CELLOS_TEST_HOOKS_TARGET_DIR="${CELLOS_TEST_HOOKS_TARGET_DIR:-target/test-hooks}"
REL="$CELLOS_TEST_HOOKS_TARGET_DIR/riscv64gc-unknown-none-elf/release"
TH_DIR="kernel/src/embedded-test-hooks"
DOMAIN_TARGET="target/native-domain-test"
DOMAIN_EMBED="$DOMAIN_TARGET/embedded"
DOMAIN_KERNEL="$OUT_REL/cellos-kernel-native-domain-test"

# Reuse the signed test fixture/image construction. It never enables
# native-domains; that feature is compiled only into the isolated kernel below.
bash scripts/build-test-hooks-ci.sh

echo "==> Building the phase-03 Tier-2 grant pair cells and service-input..."
# Same target directory as the test-hooks cells: the image below is assembled
# from one directory, so every binary in it comes from this invocation's builds.
# service-input is here because cellbox creates its serial session through the
# INPUT service; without it the grant-pair case boots to "shell: authenticated
# serial session failed: NotFound" and never sees a prompt.
CARGO_TARGET_DIR="$CELLOS_TEST_HOOKS_TARGET_DIR" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    -p tier2-grant-owner -p tier2-grant-receiver -p service-input

# The pair's launcher gets its own target directory so the shared test-hooks
# image's default-feature launcher is never overwritten. `tier2-grant-pair` is a
# launcher-only feature, so the launcher is built on its own.
LAUNCHER_TARGET="target/native-domain-launcher"
echo "==> Building the fixture launcher with tier2-grant-pair..."
CARGO_TARGET_DIR="$LAUNCHER_TARGET" \
cargo build --release \
    --target riscv64gc-unknown-none-elf \
    -Z build-std=core,alloc \
    --features tier2-grant-pair \
    -p fixture-launcher

OWNER_ELF="$REL/tier2-grant-owner"
RECEIVER_ELF="$REL/tier2-grant-receiver"
INPUT_ELF="$REL/service-input"
LAUNCHER_ELF="$LAUNCHER_TARGET/riscv64gc-unknown-none-elf/release/fixture-launcher"
for elf in "$OWNER_ELF" "$RECEIVER_ELF" "$INPUT_ELF" "$LAUNCHER_ELF"; do
    [[ -f "$elf" ]] || { echo "FAIL: native-domain image cell missing: $elf" >&2; exit 1; }
done

# shellcheck source=scripts/lib-sign-cells.sh
source scripts/lib-sign-cells.sh
echo "==> Signing the Tier-2 grant pair cells, its launcher and service-input..."
sign_cells "$OWNER_ELF" "$RECEIVER_ELF" "$INPUT_ELF" "$LAUNCHER_ELF"

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

# Same cell set as the shared test-hooks image, plus /bin/input and the grant
# pair on the two reviewed launch paths the fixture launcher's edges admit, with
# the pair launcher in place of the shared one. `input` selects /bin/input in
# services.toml (cellbox's serial session needs it); the generator also requires
# the artifact.
"$PYTHON_BIN" tools/mkfat32.py --config-features input \
    --config-output-dir "$TMPDIR_KFS/boot-config" \
    "$DOMAIN_EMBED/kernel_fs.img" \
    "$REL/app-init"         /bin/init \
    "$REL/cellbox"        /bin/cellbox \
    "$REL/service-vfs"      /bin/vfs \
    "$REL/service-config"   /bin/config \
    "$REL/vfs-test"         /bin/vfs-test \
    "$LAUNCHER_ELF"         /bin/fixture-launcher \
    "$REL/service-net"      /bin/net \
    "$REL/driver-virtio-net" /bin/virtio-net \
    "$REL/atomic-publication-probe" /bin/atomic-probe \
    "$INPUT_ELF"            /bin/input \
    "$OWNER_ELF"            /bin/silo \
    "$RECEIVER_ELF"         /bin/net-broker \
    "$TMPDIR_KFS/hostname"  /etc/hostname \
    "$TMPDIR_KFS/POLICY.BIN" /POLICY.BIN

grep -Fxq 'path = "/bin/input"' "$TMPDIR_KFS/boot-config/services.toml" || {
    echo "FAIL: services.toml does not start /bin/input; cellbox needs it for its serial session" >&2
    exit 1; }

[[ -f "$DOMAIN_EMBED/kernel_fs.img" ]] || {
    echo "FAIL: mkfat32.py did not produce the grant-pair kernel_fs.img" >&2; exit 1; }

# Prove the layout rather than trusting the exit code: mkfat32.py exits 0 for a
# well-formed image whose destination paths went astray, and then the boot fails
# as a confusing "cell not found".
"$PYTHON_BIN" tools/inspect_fat.py "$DOMAIN_EMBED/kernel_fs.img" > "$TMPDIR_KFS/fat-layout.txt"
for required in "LFN 'vfs-test'" "LFN 'fixture-launcher'" "LFN 'atomic-probe'" "LFN 'input'" "LFN 'silo'" "LFN 'net-broker'"; do
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

mkdir -p "$OUT_REL"
cp "$DOMAIN_TARGET/riscv64gc-unknown-none-elf/release/cellos-kernel" "$DOMAIN_KERNEL"
printf 'PASS: native-domain test-hooks kernel: %s\n' "$DOMAIN_KERNEL"
