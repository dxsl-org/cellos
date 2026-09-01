#!/usr/bin/env bash
# Install the exact QEMU-TCG build qualified by the x86 hypervisor smoke lane.

set -euo pipefail

VERSION="10.2.0"
ARCHIVE="qemu-${VERSION}.tar.xz"
ARCHIVE_SHA256="9e30ad1b8b9f7b4463001582d1ab297f39cfccea5d08540c0ca6d6672785883a"
PREFIX="${QEMU_X86_PREFIX:-$HOME/.cache/cellos/qemu-${VERSION}}"
QEMU_BIN="$PREFIX/bin/qemu-system-x86_64"
EXPECTED_VERSION="QEMU emulator version ${VERSION}"

case "$PREFIX" in
    */qemu-${VERSION}) ;;
    *)
        echo "FAIL: QEMU_X86_PREFIX must end in /qemu-${VERSION}: $PREFIX" >&2
        exit 1
        ;;
esac

version_is_exact() {
    [[ "$1" == "$EXPECTED_VERSION" ]]
}

exact_version_installed() {
    [[ -x "$QEMU_BIN" ]] && version_is_exact "$("$QEMU_BIN" --version | sed -n '1p')"
}

if exact_version_installed; then
    echo "[qemu-x86-ci] using cached $($QEMU_BIN --version | sed -n '1p')"
    exit 0
fi

for required in curl sha256sum tar python3 ninja pkg-config cc; do
    command -v "$required" >/dev/null 2>&1 || {
        echo "FAIL: required QEMU build tool not found: $required" >&2
        exit 1
    }
done

parent="$(dirname "$PREFIX")"
stage="${PREFIX}.install.$$"
work="$(mktemp -d)"
cleanup() {
    rm -rf "$work" "$stage"
}
trap cleanup EXIT

mkdir -p "$parent"
rm -rf "$stage"
curl -fL --retry 3 --output "$work/$ARCHIVE" "https://download.qemu.org/$ARCHIVE"
echo "$ARCHIVE_SHA256  $work/$ARCHIVE" | sha256sum --check --strict
tar -C "$work" -xf "$work/$ARCHIVE"

mkdir "$work/qemu-${VERSION}/build"
(
    cd "$work/qemu-${VERSION}/build"
    ../configure \
        --prefix="$stage" \
        --target-list=x86_64-softmmu \
        --disable-download \
        --disable-docs \
        --disable-guest-agent \
        --disable-tools \
        --enable-slirp \
        --disable-werror
    ninja -j "$(nproc)"
    ninja install
)

installed="$stage/bin/qemu-system-x86_64"
[[ -x "$installed" ]] || {
    echo "FAIL: QEMU build did not install $installed" >&2
    exit 1
}
version_is_exact "$("$installed" --version | sed -n '1p')" || {
    echo "FAIL: installed QEMU version is not ${VERSION}" >&2
    exit 1
}

rm -rf "$PREFIX"
mv "$stage" "$PREFIX"
echo "[qemu-x86-ci] installed $($QEMU_BIN --version | sed -n '1p') at $PREFIX"
