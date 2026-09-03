#!/usr/bin/env bash
# Build the pinned Ubuntu 24.04 amd64 Tier 3 root disk and PVH boot artifacts.
# The Ubuntu userspace comes from an immutable Canonical cloud-image release;
# the qualified Alpine PVH kernel/initramfs input remains digest-pinned and is
# repacked only to replace /init with a deterministic /dev/vda switch-root.

set -euo pipefail

readonly PROFILE="ubuntu-wide-guest-v1"
readonly UBUNTU_RELEASE="24.04"
readonly UBUNTU_SNAPSHOT="20240821T000000Z"
readonly UBUNTU_ROOTFS_NAME="ubuntu-24.04-server-cloudimg-amd64-root.tar.xz"
readonly UBUNTU_ROOTFS_URL="https://cloud-images.ubuntu.com/releases/noble/release-20240821/${UBUNTU_ROOTFS_NAME}"
readonly UBUNTU_ROOTFS_SHA256="16429c49387eaf783a88ce1896940dfdb10b51cbec38304b2b652e26993276b7"
readonly PINNED_ALPINE_VERSION="3.21.7"
readonly ALPINE_VMLINUZ_SHA256="26bf81ada3e8fc30fd4d81805fe6c8c60be5c7fb18a43563c707e49117e624ca"
readonly ALPINE_INITRD_SHA256="e2562e019a506f9bdac24d06953823106a2ab29da50eea01185d005a3ca4acdf"
readonly EXTRACT_VMLINUX_SHA256="97cfeeeb51de17f4b5928c5442b56e5581314ddef3cedf2523be2049d79394af"
readonly SOURCE_DATE_EPOCH="1724198400"
readonly ROOTFS_UUID="ce110524-0400-4000-8000-000000000024"
readonly MARKER_PACKAGE="sl"
readonly MARKER_VERSION="5.02-1"
readonly MARKER_PATH="/var/lib/cellos/ubuntu-apt-marker-v1"


# shellcheck source=scripts/lib-build-ubuntu-rootfs.sh
source "$(dirname "$0")/lib-build-ubuntu-rootfs.sh"
usage() {
    cat <<'EOF'
Usage: sudo bash scripts/build-ubuntu-wide-guest-x86.sh [options]

Options:
  --output-dir DIR   Artifact directory (default: build/ubuntu-wide-guest-x86)
  --cache-dir DIR    Download cache (default: .ubuntu-cache-x86)
  --disk-size SIZE   ext4 image size, integer M or G (default: 3G)
  --skip-download    Require all pinned source artifacts to exist in cache
  -h, --help         Show this help

Outputs: vmlinux, initrd.gz, guest_disk.img, provenance.txt,
         artifact-sha256sums
EOF
}

OUTPUT_DIR="build/ubuntu-wide-guest-x86"
CACHE_DIR=".ubuntu-cache-x86"
DISK_SIZE="3G"
SKIP_DOWNLOAD=0
while [[ "$#" -gt 0 ]]; do
    case "$1" in
        --output-dir|--cache-dir|--disk-size)
            [[ "$#" -ge 2 && -n "$2" ]] || { echo "ERROR: $1 requires a value" >&2; exit 2; }
            case "$1" in
                --output-dir) OUTPUT_DIR="$2" ;;
                --cache-dir) CACHE_DIR="$2" ;;
                --disk-size) DISK_SIZE="$2" ;;
            esac
            shift 2
            ;;
        --skip-download) SKIP_DOWNLOAD=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "ERROR: unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

[[ "$DISK_SIZE" =~ ^[1-9][0-9]*[MG]$ ]] \
    || { echo "ERROR: --disk-size must be an integer followed by M or G" >&2; exit 2; }
[[ "$EUID" -eq 0 ]] \
    || { echo "ERROR: root is required to preserve Ubuntu rootfs ownership" >&2; exit 1; }

for tool in bash curl sha256sum tar xz gzip cpio find sort touch truncate mkfs.ext4 \
    install readelf grep cut mv rm mkdir ln chmod mktemp awk dd; do
    command -v "$tool" >/dev/null 2>&1 \
        || { echo "ERROR: required host tool not found: $tool" >&2; exit 1; }
done

mkdir -p "$CACHE_DIR" "$OUTPUT_DIR"
ROOTFS_ARCHIVE="$CACHE_DIR/$UBUNTU_ROOTFS_NAME"
ALPINE_CACHE="$CACHE_DIR/alpine"

fetch_pinned() {
    local url="$1" destination="$2" expected="$3" label="$4"
    if [[ -f "$destination" ]]; then
        if [[ "$(sha256sum "$destination" | cut -d' ' -f1)" == "$expected" ]]; then
            echo "[ubuntu-wide] $label: cached digest OK"
            return
        fi
        rm -f "$destination"
        [[ "$SKIP_DOWNLOAD" == 0 ]] \
            || { echo "ERROR: cached $label digest mismatch in --skip-download mode" >&2; exit 1; }
    fi
    [[ "$SKIP_DOWNLOAD" == 0 ]] \
        || { echo "ERROR: pinned $label missing in --skip-download mode: $destination" >&2; exit 1; }
    curl -fL --retry 3 --output "$destination.part" "$url"
    [[ "$(sha256sum "$destination.part" | cut -d' ' -f1)" == "$expected" ]] \
        || { rm -f "$destination.part"; echo "ERROR: SHA256 mismatch for $label" >&2; exit 1; }
    mv "$destination.part" "$destination"
}

fetch_pinned "$UBUNTU_ROOTFS_URL" "$ROOTFS_ARCHIVE" "$UBUNTU_ROOTFS_SHA256" "Ubuntu rootfs"
if [[ "$SKIP_DOWNLOAD" == 1 ]]; then
    for pair in \
        "$ALPINE_CACHE/vmlinuz-virt:$ALPINE_VMLINUZ_SHA256" \
        "$ALPINE_CACHE/initramfs-virt:$ALPINE_INITRD_SHA256" \
        "scripts/.extract-vmlinux:$EXTRACT_VMLINUX_SHA256"; do
        file="${pair%%:*}"
        expected="${pair##*:}"
        [[ -f "$file" && "$(sha256sum "$file" | cut -d' ' -f1)" == "$expected" ]] \
            || { echo "ERROR: missing or mismatched cached input: $file" >&2; exit 1; }
    done
fi
# Re-derive vmlinux every time so a modified cache can never bypass the digest
# checks on the bzImage and on the downloaded extraction program.
rm -f "$ALPINE_CACHE/vmlinux"
ALPINE_VERSION="$PINNED_ALPINE_VERSION" \
VMLINUZ_SHA256="$ALPINE_VMLINUZ_SHA256" \
INITRD_SHA256="$ALPINE_INITRD_SHA256" \
    bash scripts/fetch-alpine-x86.sh "$ALPINE_CACHE"
for pair in \
    "$ALPINE_CACHE/vmlinuz-virt:$ALPINE_VMLINUZ_SHA256" \
    "$ALPINE_CACHE/initramfs-virt:$ALPINE_INITRD_SHA256"; do
    file="${pair%%:*}"
    expected="${pair##*:}"
    [[ -f "$file" && "$(sha256sum "$file" | cut -d' ' -f1)" == "$expected" ]] \
        || { echo "ERROR: missing or mismatched pinned Alpine input: $file" >&2; exit 1; }
done
[[ -f "$ALPINE_CACHE/vmlinux" ]] \
    || { echo "ERROR: extracted Alpine PVH vmlinux is missing" >&2; exit 1; }
readelf -n "$ALPINE_CACHE/vmlinux" 2>/dev/null | grep -F 'Xen' >/dev/null \
    || { echo "ERROR: extracted kernel has no Xen PVH note" >&2; exit 1; }

WORK_DIR="$(mktemp -d "${OUTPUT_DIR}.stage.XXXXXX")"
cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT INT TERM
ROOT="$WORK_DIR/rootfs"
INITRD_TREE="$WORK_DIR/initrd"
mkdir -p "$ROOT" "$INITRD_TREE"
tar --extract --xz --file "$ROOTFS_ARCHIVE" --directory "$ROOT" \
    --numeric-owner --same-owner --same-permissions

configure_ubuntu_rootfs "$ROOT" "$UBUNTU_SNAPSHOT"
build_switch_root_initrd "$ALPINE_CACHE/initramfs-virt" "$INITRD_TREE" "$WORK_DIR/initrd.gz" "$SOURCE_DATE_EPOCH"

find "$ROOT" -exec touch -h -d "@$SOURCE_DATE_EPOCH" {} +
DISK="$WORK_DIR/guest_disk.img"
truncate -s "$DISK_SIZE" "$DISK"
E2FSPROGS_FAKE_TIME="$SOURCE_DATE_EPOCH" mkfs.ext4 -q -F -m 0 \
    -L CELLOS_UBUNTU -U "$ROOTFS_UUID" \
    -E lazy_itable_init=0,lazy_journal_init=0 \
    -d "$ROOT" "$DISK"

install -m 0644 "$ALPINE_CACHE/vmlinux" "$OUTPUT_DIR/vmlinux"
install -m 0644 "$WORK_DIR/initrd.gz" "$OUTPUT_DIR/initrd.gz"
install -m 0644 "$DISK" "$OUTPUT_DIR/guest_disk.img"
cat > "$OUTPUT_DIR/provenance.txt" <<EOF
profile=$PROFILE
ubuntu_release=$UBUNTU_RELEASE
ubuntu_rootfs_url=$UBUNTU_ROOTFS_URL
ubuntu_rootfs_sha256=$UBUNTU_ROOTFS_SHA256
ubuntu_snapshot=$UBUNTU_SNAPSHOT
alpine_version=$PINNED_ALPINE_VERSION
alpine_vmlinuz_sha256=$ALPINE_VMLINUZ_SHA256
alpine_initrd_sha256=$ALPINE_INITRD_SHA256
extract_vmlinux_sha256=$EXTRACT_VMLINUX_SHA256
rootfs_uuid=$ROOTFS_UUID
marker_package=$MARKER_PACKAGE
marker_version=$MARKER_VERSION
marker_path=$MARKER_PATH
source_date_epoch=$SOURCE_DATE_EPOCH
EOF
(
    cd "$OUTPUT_DIR"
    sha256sum vmlinux initrd.gz guest_disk.img > artifact-sha256sums
)

echo "[ubuntu-wide] artifacts ready in $OUTPUT_DIR"
echo "[ubuntu-wide] profile=$PROFILE root=/dev/vda size=$DISK_SIZE"
