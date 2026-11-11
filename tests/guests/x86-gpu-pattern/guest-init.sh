#!/bin/busybox sh
# Tier 3 x86 display fixture: bind virtio-gpu and virtio-input, then fill the
# guest framebuffer with a two-band test pattern the lane checks on screen:
# top half bytes 0x40, bottom half bytes 0xC0 (BGRA grey 64 / grey 192).

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

fail() {
    echo "X86_GPU_FAIL:$1"
    while :; do sleep 3600; done
}

mkdir -p /proc /sys /dev
mount -t proc proc /proc || fail mount-proc
mount -t sysfs sysfs /sys || fail mount-sysfs
mount -t devtmpfs devtmpfs /dev || fail mount-devtmpfs
echo /bin/mdev > /proc/sys/kernel/hotplug

modprobe virtio_gpu || fail modprobe-virtio-gpu
modprobe virtio_input || fail modprobe-virtio-input
mdev -s

attempts=50
while [ ! -e /dev/fb0 ] && [ "$attempts" -gt 0 ]; do
    sleep 0.1
    attempts=$((attempts - 1))
done
[ -e /dev/fb0 ] || fail no-fb0

size="$(cat /sys/class/graphics/fb0/virtual_size)"
width="${size%,*}"
height="${size#*,}"
stride="$(cat /sys/class/graphics/fb0/stride)"
echo "X86_GPU_FB:${width}x${height}:stride=${stride}"

top=$((height / 2))
bottom=$((height - top))
# Build each band as a file first: `dd of=` on a pipe counts short reads as
# blocks and would write fewer bytes than asked.
mkdir -p /tmp && mount -t tmpfs tmpfs /tmp || fail mount-tmp
dd if=/dev/zero bs="$stride" count="$top" 2>/dev/null | tr '\000' '\100' > /tmp/top
dd if=/dev/zero bs="$stride" count="$bottom" 2>/dev/null | tr '\000' '\300' > /tmp/bottom
[ "$(cat /tmp/top /tmp/bottom | wc -c)" -eq $((stride * height)) ] || fail pattern-size
cat /tmp/top /tmp/bottom > /dev/fb0 || fail write-fb
echo "X86_GPU_PATTERN_WRITTEN"

grep -qi 'virtio' /proc/bus/input/devices || fail no-virtio-input
echo "X86_INPUT_BOUND"
echo "X86_DISPLAY_IRQ:$(awk '$1 == "3:" { print $2 }' /proc/interrupts)"

while :; do sleep 3600; done
