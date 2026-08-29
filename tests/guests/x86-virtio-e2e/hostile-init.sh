#!/bin/busybox sh

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

fail() {
    echo "VIRTIO_HOSTILE_FAIL:$1"
    while :; do sleep 3600; done
}

mkdir -p /proc /sys /dev /tmp
mount -t proc proc /proc || fail mount-proc
mount -t sysfs sysfs /sys || fail mount-sysfs
mount -t devtmpfs devtmpfs /dev || fail mount-devtmpfs
mdev -s

# Linux owns each transport as soon as the virtio-mmio platform driver matches
# it, even when the block/net function drivers are modules. Unbind the two
# dedicated transports before mapping their shared register page.
: > /tmp/virtio-bindings
isolated=0
driver=/sys/bus/platform/drivers/virtio-mmio
[ -d "$driver" ] || fail virtio-mmio-driver-missing
for device in "$driver"/virtio-mmio.*; do
    [ -L "$device" ] || continue
    name=${device##*/}
    printf '%s|%s\n' "$name" "$driver" >> /tmp/virtio-bindings
    printf '%s' "$name" > "$driver/unbind" || fail "unbind:$name"
    isolated=$((isolated + 1))
done
[ "$isolated" -eq 2 ] || fail "expected-two-transports-isolated:$isolated"
echo "[VIRTIO_HOSTILE] TRANSPORTS_ISOLATED"

/bin/virtio-hostile-mmio || fail helper-exit
fail helper-returned
