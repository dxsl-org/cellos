#!/usr/bin/env bash
# Tier 3 x86 display lane (Intel VMX via nested KVM): a Linux guest's virtio-gpu
# framebuffer reaches the Cellos compositor, and its virtio-input device binds.
#
# Builds a private desktop image (assemble-x86-image.py --profile desktop, which
# packages /bin/hypervisor) and repacks its FAT image with the pinned Alpine guest
# kernel and an initramfs carrying tests/guests/x86-gpu-pattern/guest-init.sh
# (selected by the /virtio-e2e file). Boots it, starts the hypervisor with the
# serial shell's `hv`, and checks:
#   - guest markers: framebuffer bound, pattern written, virtio-input bound,
#     display interrupts (ISA IRQ 3) delivered;
#   - the screen shows the guest's exact two-band pattern (BGRA 0x40 top half,
#     0xC0 bottom half), and the screen before `hv` does not (control).
#
# Environment:
#   CELLOS_TIER3_WORK  work directory (default: a new /tmp/cellos-tier3-display-*)
#   QEMU_X86_BIN       emulator (default: qemu-system-x86_64)
#   BOOT_WINDOW        seconds for the desktop and the guest each (default: 120)
#
# Exit: 0 PASS, 1 FAIL, 2 BLOCKED_ENVIRONMENT.

set -euo pipefail
REPO="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"
W="${CELLOS_TIER3_WORK:-$(mktemp -d /tmp/cellos-tier3-display-XXXX)}"
QEMU_X86_BIN="${QEMU_X86_BIN:-qemu-system-x86_64}"
BOOT_WINDOW="${BOOT_WINDOW:-120}"
FIXTURE="tests/guests/x86-gpu-pattern/guest-init.sh"
mkdir -p "$W"

blocked() { echo "BLOCKED_ENVIRONMENT: $*" >&2; exit 2; }
[[ -w /dev/kvm ]] && grep -qw vmx /proc/cpuinfo || blocked "needs a writable /dev/kvm on an Intel VMX host"
for tool in "$QEMU_X86_BIN" mcopy python3 cargo; do
    command -v "$tool" >/dev/null || blocked "$tool not found"
done
[[ -f .alpine-cache-x86/vmlinux && -f .alpine-cache-x86/initramfs-virt ]] \
    || bash scripts/fetch-alpine-x86.sh .alpine-cache-x86

echo "==> [tier3-display] assembling the desktop image in $W"
CARGO_TARGET_DIR="$W/cells-target" python3 scripts/assemble-x86-image.py --profile desktop \
    --out-dir "$W/embedded"

echo "==> [tier3-display] adding the guest kernel and fixture initramfs"
# mkfat32 sizes the image to its contents, so the image is rebuilt with the guest files.
# The assembler manifest no longer describes the result and is removed.
python3 tools/repack-initramfs.py .alpine-cache-x86/initramfs-virt "$W/initrd.gz" \
    --add /bin/virtio-e2e-init "$FIXTURE" 100755
printf 'rdinit=/bin/virtio-e2e-init\n' > "$W/virtio-e2e"
rm -rf "$W/fat" && mkdir -p "$W/fat"
mcopy -s -n -i "$W/embedded/kernel_fs.img" ::/ "$W/fat/"
PAIRS=()
while IFS= read -r file; do PAIRS+=("$file" "/${file#"$W/fat/"}"); done < <(find "$W/fat" -type f | sort)
PAIRS+=(.alpine-cache-x86/vmlinux /vmlinux "$W/initrd.gz" /initrd.gz "$W/virtio-e2e" /virtio-e2e)
python3 tools/mkfat32.py "$W/embedded/kernel_fs.img" "${PAIRS[@]}"
rm -f "$W/embedded/manifest.json"

echo "==> [tier3-display] building the kernel and ISO"
EMBEDDED_OVERRIDE="$W/embedded" CARGO_TARGET_DIR="$W/kernel-target" \
RUSTFLAGS="-C code-model=kernel -C no-redzone=yes -Z cf-protection=full -C relocation-model=static" \
cargo build --release -p cellos-kernel --target x86_64-unknown-none -Z build-std=core,alloc \
    --features "dev-policy-key,dev-signing-key,dev-weak-rng,signing-required,native-domains,qemu-dev-https,large-cell-va"
X86_KERNEL="$W/kernel-target/x86_64-unknown-none/release/cellos-kernel" X86_ISO_ROOT="$W/iso-root" \
    bash scripts/x86/make-iso-ci.sh "$W/cellos.iso"

echo "==> [tier3-display] booting"
exec python3 scripts/tier3_display_check.py --qemu "$QEMU_X86_BIN" --iso "$W/cellos.iso" \
    --work "$W" --window "$BOOT_WINDOW"
