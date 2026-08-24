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

ALPINE_VERSION="${ALPINE_VERSION:-3.21.7}"
DEST="${1:-.alpine-cache-x86}"
CDN="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION%.*}/releases/x86_64/netboot-${ALPINE_VERSION}"

# 6.12.81-0-virt — the artifact set recorded as booting under the SVM lane
# (commit 1827b8f3); 3.21.3's 6.12.13 triple-faults early in PVH entry.
VMLINUZ_SHA256="${VMLINUZ_SHA256:-26bf81ada3e8fc30fd4d81805fe6c8c60be5c7fb18a43563c707e49117e624ca}"
INITRD_SHA256="${INITRD_SHA256:-e2562e019a506f9bdac24d06953823106a2ab29da50eea01185d005a3ca4acdf}"

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
# Pinned to a tag, never `master`: this file is downloaded and then EXECUTED, so an
# unpinned ref means running whatever upstream HEAD happens to hold that day. The
# supply-chain rule this script's header states for the Alpine artifacts applies here
# with more force — those are only read, this one runs as code.
# v6.12 and v6.6 ship byte-identical copies; the file has been stable for years.
EXTRACT_TOOL_URL="https://raw.githubusercontent.com/torvalds/linux/v6.12/scripts/extract-vmlinux"
EXTRACT_TOOL_SHA256="97cfeeeb51de17f4b5928c5442b56e5581314ddef3cedf2523be2049d79394af"

extract_vmlinux() {
    local bzimage="$1" out="$2"
    if [[ ! -f "$EXTRACT_TOOL" ]]; then
        curl -fsSL -o "$EXTRACT_TOOL" "$EXTRACT_TOOL_URL" || return 1
    fi
    # Verify on every call, not just after a fresh download: the cache sits in the
    # working tree, so a stale or edited copy has to be rejected as well.
    local got
    got="$(sha256sum "$EXTRACT_TOOL" | awk '{print $1}')"
    if [[ "$got" != "$EXTRACT_TOOL_SHA256" ]]; then
        echo "ERROR: $EXTRACT_TOOL checksum mismatch — refusing to execute it" >&2
        echo "  expected $EXTRACT_TOOL_SHA256" >&2
        echo "  got      $got" >&2
        echo "  Delete the file to re-fetch from $EXTRACT_TOOL_URL" >&2
        return 1
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
