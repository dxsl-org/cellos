# shellcheck shell=bash
# Shared by the Tier 3 x86 KVM lanes (qemu-tier3-display-x86.sh,
# qemu-tier3-chrome-x86.sh): preflight and a private desktop ISO whose FAT image
# also carries the lane's guest files. Source from the repository root with
# `set -euo pipefail`; QEMU_X86_BIN must be set.

blocked() { echo "BLOCKED_ENVIRONMENT: $*" >&2; exit 2; }

# Nested VMX, the build tools and the pinned Alpine guest kernel/initramfs.
tier3_preflight() {
    [[ -w /dev/kvm ]] && grep -qw vmx /proc/cpuinfo \
        || blocked "needs a writable /dev/kvm on an Intel VMX host"
    local tool
    for tool in "$QEMU_X86_BIN" mcopy python3 cargo "$@"; do
        command -v "$tool" >/dev/null || blocked "$tool not found"
    done
    [[ -f .alpine-cache-x86/vmlinux && -f .alpine-cache-x86/initramfs-virt ]] \
        || bash scripts/fetch-alpine-x86.sh .alpine-cache-x86
}

# tier3_build_iso WORK LABEL [SOURCE IMAGE_PATH]...
# Builds the desktop image (assemble-x86-image.py --profile desktop, which packages
# /bin/hypervisor), adds every SOURCE at IMAGE_PATH to its FAT image (none: the
# image as shipped), and writes WORK/cellos.iso.
tier3_build_iso() {
    local work="$1" label="$2"
    shift 2
    echo "==> [$label] assembling the desktop image in $work"
    CARGO_TARGET_DIR="$work/cells-target" python3 scripts/assemble-x86-image.py \
        --profile desktop --out-dir "$work/embedded"

    if (( $# )); then
        echo "==> [$label] adding the lane's guest files"
        # mkfat32 sizes the image to its contents, so the image is rebuilt with the
        # extra files. The assembler manifest no longer describes the result.
        rm -rf "$work/fat" && mkdir -p "$work/fat"
        mcopy -s -n -i "$work/embedded/kernel_fs.img" ::/ "$work/fat/"
        local pairs=() file
        while IFS= read -r file; do pairs+=("$file" "/${file#"$work/fat/"}"); done \
            < <(find "$work/fat" -type f | sort)
        python3 tools/mkfat32.py "$work/embedded/kernel_fs.img" "${pairs[@]}" "$@"
        rm -f "$work/embedded/manifest.json"
    fi

    echo "==> [$label] building the kernel and ISO"
    EMBEDDED_OVERRIDE="$work/embedded" CARGO_TARGET_DIR="$work/kernel-target" \
    RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
    cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc \
        --features "dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,qemu-dev-https,large-cell-va"
    X86_KERNEL="$work/kernel-target/x86_64-unknown-none/release/cellos-kernel" \
        X86_ISO_ROOT="$work/iso-root" bash scripts/x86/make-iso-ci.sh "$work/cellos.iso"
}
