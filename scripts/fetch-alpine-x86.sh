#!/usr/bin/env bash
# Fetch Alpine Linux x86_64 netboot artifacts for the ViCell x86 hypervisor and
# extract the uncompressed `vmlinux` ELF the PVH boot protocol needs.
#
# The shipped `vmlinuz-virt` is a bzImage; the PVH entry note
# (XEN_ELFNOTE_PHYS32_ENTRY, name "Xen", type 18) lives ONLY in the
# uncompressed `vmlinux` ELF embedded in it. This decompresses that payload and
# asserts the note is present, so P05 boots via PVH rather than the heavier
# bzImage protocol.
#
# Usage: bash scripts/fetch-alpine-x86.sh [dest-dir]   (default .alpine-cache-x86)
#
# Security: supply-chain pinning — set VMLINUZ_SHA256/INITRD_SHA256 to enforce
# checksum verification (from the release SHA256SUMS).

set -euo pipefail

ALPINE_VERSION="${ALPINE_VERSION:-3.21.3}"
DEST="${1:-.alpine-cache-x86}"
CDN="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION%.*}/releases/x86_64/netboot"

VMLINUZ_SHA256="${VMLINUZ_SHA256:-UPDATE_ME_FROM_SHA256SUMS}"
INITRD_SHA256="${INITRD_SHA256:-UPDATE_ME_FROM_SHA256SUMS}"

mkdir -p "$DEST"

fetch_and_verify() {
    local url="$1" dest="$2" expected="$3" name="$4"
    if [[ -f "$dest" && "$expected" != "UPDATE_ME_FROM_SHA256SUMS" ]]; then
        [[ "$(sha256sum "$dest" | awk '{print $1}')" == "$expected" ]] && {
            echo "[fetch-x86] $name: cached OK"; return 0; }
        rm -f "$dest"
    fi
    echo "[fetch-x86] downloading $name ..."
    if command -v curl &>/dev/null; then curl -fSL --retry 3 -o "$dest" "$url"
    else wget -q --tries=3 -O "$dest" "$url"; fi
    if [[ "$expected" == "UPDATE_ME_FROM_SHA256SUMS" ]]; then
        echo "[fetch-x86] WARNING: no checksum for $name — verify manually:"
        echo "            sha256sum $dest"
    elif [[ "$(sha256sum "$dest" | awk '{print $1}')" != "$expected" ]]; then
        echo "ERROR: SHA256 mismatch for $name" >&2; rm -f "$dest"; exit 1
    fi
}

# Decompress the vmlinux ELF out of a bzImage using the kernel's canonical
# `extract-vmlinux` (its `tr`-based offset search handles the payload robustly;
# a hand-rolled `grep -P` magic scan misses Alpine's gzip payload). Cached under
# scripts/ so re-runs work offline.
EXTRACT_TOOL="scripts/.extract-vmlinux"
extract_vmlinux() {
    local bzimage="$1" out="$2"
    if [[ ! -f "$EXTRACT_TOOL" ]]; then
        curl -fsSL -o "$EXTRACT_TOOL" \
            https://raw.githubusercontent.com/torvalds/linux/master/scripts/extract-vmlinux \
            || return 1
    fi
    bash "$EXTRACT_TOOL" "$bzimage" > "$out" 2>/dev/null || return 1
    [[ "$(dd if="$out" bs=1 count=4 2>/dev/null)" == $'\x7fELF' ]]
}

fetch_and_verify "$CDN/vmlinuz-virt"   "$DEST/vmlinuz-virt"   "$VMLINUZ_SHA256" "vmlinuz-virt"
fetch_and_verify "$CDN/initramfs-virt" "$DEST/initramfs-virt" "$INITRD_SHA256"  "initramfs-virt"

VMLINUX="$DEST/vmlinux"
if [[ ! -f "$VMLINUX" || "$DEST/vmlinuz-virt" -nt "$VMLINUX" ]]; then
    echo "[fetch-x86] extracting uncompressed vmlinux ELF ..."
    extract_vmlinux "$DEST/vmlinuz-virt" "$VMLINUX" || {
        echo "ERROR: could not extract a vmlinux ELF from vmlinuz-virt" >&2
        echo "  Install one of: gzip xz-utils zstd, or supply vmlinux manually." >&2
        exit 1
    }
fi

# Assert the PVH entry note exists (name "Xen"): otherwise this kernel is not
# PVH-capable and the cell would need the bzImage fallback path.
if command -v readelf &>/dev/null; then
    if readelf -n "$VMLINUX" 2>/dev/null | grep -qi "Xen"; then
        echo "[fetch-x86] PVH note present (XEN_ELFNOTE_PHYS32_ENTRY) — PVH-capable ✓"
    else
        echo "ERROR: vmlinux carries no Xen PVH note — CONFIG_PVH missing?" >&2
        exit 1
    fi
else
    echo "[fetch-x86] WARNING: readelf not found — PVH-note check skipped"
fi

echo ""
echo "[fetch-x86] Artifacts ready in $DEST/:"
ls -lh "$DEST/vmlinux" "$DEST/initramfs-virt"
echo "Next: bash scripts/make-hypervisor-fs-x86.sh"
