#!/usr/bin/env bash
# Tier 3 x86 display and network lane (Intel VMX via nested KVM): a Linux guest's
# virtio-gpu framebuffer reaches the Cellos compositor, its virtio-input device
# binds, and its virtio-net device reaches the network beside the host stack.
#
# Builds a private desktop image (assemble-x86-image.py --profile desktop, which
# packages /bin/hypervisor) and repacks its FAT image with the pinned Alpine guest
# kernel and an initramfs carrying tests/guests/x86-gpu-pattern/guest-init.sh
# (selected by the /virtio-e2e file). Boots it with a QEMU user-mode NIC, starts
# the hypervisor with the serial shell's `hv` and from the taskbar's Chrome
# (scripts/tier3_display_check.py), and checks:
#   - guest markers: framebuffer bound, pattern written, virtio-input bound,
#     display interrupts (ISA IRQ 3) delivered, guest XMM registers intact
#     across VM exits (fpu-check.S);
#   - the screen shows the guest's exact two-band pattern (BGRA 0x40 top half,
#     0xC0 bottom half), and the screen before `hv` does not (control);
#   - the guest's own DHCP lease (not the host stack's), DNS and an HTTP fetch;
#     Ocel fetching through the host stack while the guest runs.
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

# shellcheck source=scripts/lib-tier3-desktop-iso.sh
source scripts/lib-tier3-desktop-iso.sh
tier3_preflight as ld

as tests/guests/x86-gpu-pattern/fpu-check.S -o "$W/fpu-check.o"
ld -static "$W/fpu-check.o" -o "$W/fpu-check"
python3 tools/repack-initramfs.py .alpine-cache-x86/initramfs-virt "$W/initrd.gz" \
    --add /bin/virtio-e2e-init "$FIXTURE" 100755 --add /bin/fpu-check "$W/fpu-check" 100755
printf 'rdinit=/bin/virtio-e2e-init\n' > "$W/virtio-e2e"
# The desktop image already carries /vmlinux (the Chrome guest's kernel).
tier3_build_iso "$W" tier3-display "$W/initrd.gz" /initrd.gz "$W/virtio-e2e" /virtio-e2e

echo "==> [tier3-display] booting"
exec python3 scripts/tier3_display_check.py --qemu "$QEMU_X86_BIN" --iso "$W/cellos.iso" \
    --work "$W" --window "$BOOT_WINDOW"
