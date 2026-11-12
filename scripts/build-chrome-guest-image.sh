#!/usr/bin/env bash
# Build the Tier 3 Chrome guest root: a read-only xz squashfs holding Alpine
# 3.21 with cage, Chromium, fonts, dbus, eudev, seatd and wlr-randr, plus the guest init
# (scripts/chrome-guest/init.sh). The hypervisor serves it as its only disk
# (/dev/vda); stage 1 in the Alpine initramfs mounts it under a tmpfs overlay.
#
# Pinned: the builder image digest and every package version below. A version
# the mirror no longer carries fails the build (bump the pin, never float).
#
# Output (OUT_DIR, default build/chrome-guest):
#   chrome-guest.sqfs   the guest root
#   packages.txt        installed package set (name-version), sorted
#   SHA256SUMS          checksums of both
#   INPUTS.sha256       digest of this builder, the guest init and the cage key-repeat
#                       shim, so a consumer (scripts/make-chrome-guest-disk.sh) can tell
#                       a stale image
#
# Usage: bash scripts/build-chrome-guest-image.sh [OUT_DIR]
set -euo pipefail

REPO_ROOT="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$(realpath -m "${1:-$REPO_ROOT/build/chrome-guest}")"
BUILDER="alpine@sha256:ce64758a109eb420d874a118f87920e625e12d3634e03b4a5573fd9f6e5d3507"
# Kernel modules for the guest kernel (6.12.81-0-virt, scripts/fetch-alpine-x86.sh): the
# initramfs lacks evdev, and the v3.21 repository no longer carries this kernel's package.
MODLOOP_URL="https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot-3.21.7/modloop-virt"
MODLOOP_SHA256="9865c6dc154fcf40d5ad30c2e0aa586382bb716752940a0b9b8a9c3eb3937385"
MODLOOP="$REPO_ROOT/.alpine-cache-x86/modloop-virt"
PACKAGES=(
    alpine-baselayout busybox musl
    chromium=136.0.7103.113-r0
    cage=0.2.0-r0
    wlr-randr=0.4.1-r0
    seatd=0.9.1-r0
    eudev=3.2.14-r5
    font-dejavu=2.37-r5
    dbus=1.14.10-r4
    mesa-dri-gallium=24.2.8-r0
)

command -v docker >/dev/null 2>&1 || { echo "ERROR: docker is required" >&2; exit 1; }
mkdir -p "$OUT_DIR" "$(dirname "$MODLOOP")"
rm -f "$OUT_DIR/chrome-guest.sqfs" "$OUT_DIR/packages.txt" "$OUT_DIR/SHA256SUMS" \
    "$OUT_DIR/INPUTS.sha256"
if [[ ! -f "$MODLOOP" ]]; then
    curl -fsSL -o "$MODLOOP.part" "$MODLOOP_URL"
    mv "$MODLOOP.part" "$MODLOOP"
fi
echo "$MODLOOP_SHA256  $MODLOOP" | sha256sum -c --quiet - \
    || { echo "ERROR: $MODLOOP checksum mismatch" >&2; exit 1; }
docker run --rm -i \
    -v "$REPO_ROOT/scripts/chrome-guest:/in:ro" -v "$OUT_DIR:/out" -v "$MODLOOP:/modloop:ro" \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    "$BUILDER" sh -eu -s -- "${PACKAGES[@]}" <<'EOF'
apk add -q --no-cache squashfs-tools gcc musl-dev
mkdir -p /rootfs/etc/apk
cp -a /etc/apk/keys /rootfs/etc/apk/
cp /etc/apk/repositories /rootfs/etc/apk/
apk add -q --root /rootfs --initdb --no-cache "$@"
apk info -q --root /rootfs -v | sort > /out/packages.txt
install -m 0755 /in/init.sh /rootfs/sbin/chrome-guest-init
mkdir -p /rootfs/usr/lib/chrome-guest
gcc -O2 -shared -fPIC -Wall -Werror -o /rootfs/usr/lib/chrome-guest/no-key-repeat.so \
    /in/no-key-repeat.c
unsquashfs -q -n -d /modloop-root /modloop
mkdir -p /rootfs/lib/modules
cp -a /modloop-root/modules/. /rootfs/lib/modules/
rm -rf /rootfs/var/cache/apk/* /rootfs/usr/share/doc /rootfs/usr/share/man
find /rootfs/usr/lib/chromium/locales -type f ! -name 'en-US.pak' -delete
mksquashfs /rootfs /out/chrome-guest.sqfs -comp xz -noappend -no-xattrs \
    -all-root -mkfs-time 0 -all-time 0 -quiet
cd /out && sha256sum chrome-guest.sqfs packages.txt > SHA256SUMS
chown "$HOST_UID:$HOST_GID" /out/chrome-guest.sqfs /out/packages.txt /out/SHA256SUMS
EOF

echo "[chrome-guest] $(du -h "$OUT_DIR/chrome-guest.sqfs" | cut -f1) $OUT_DIR/chrome-guest.sqfs"
echo "[chrome-guest] $(wc -l < "$OUT_DIR/packages.txt") packages"
cat "$OUT_DIR/SHA256SUMS"
bash "$REPO_ROOT/scripts/chrome-guest/inputs-digest.sh" > "$OUT_DIR/INPUTS.sha256"
