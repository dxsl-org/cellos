#!/bin/busybox sh

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

fail() {
    echo "VIRTIO_E2E_FAIL:$1"
    while :; do sleep 3600; done
}

irq_count() {
    awk -v key="$1:" '
        $1 == key {
            for (i = 2; i <= NF && $i ~ /^[0-9]+$/; i++) total += $i
            found = 1
        }
        END { if (found) print total + 0; else print -1 }
    ' /proc/interrupts
}

wait_for_path() {
    path="$1"
    attempts=30
    while [ ! -e "$path" ] && [ "$attempts" -gt 0 ]; do
        sleep 1
        attempts=$((attempts - 1))
    done
    [ -e "$path" ]
}

mkdir -p /proc /sys /dev /tmp
mount -t proc proc /proc || fail mount-proc
mount -t sysfs sysfs /sys || fail mount-sysfs
mount -t devtmpfs devtmpfs /dev || fail mount-devtmpfs
echo /bin/mdev > /proc/sys/kernel/hotplug
mdev -s
modprobe virtio_blk || fail modprobe-virtio-blk
modprobe virtio_net || fail modprobe-virtio-net
mdev -s

wait_for_path /dev/vda || fail block-device-timeout
[ -d /sys/block/vda/device ] || fail block-not-virtio
echo VIRTIO_E2E_BLOCK_DISCOVERY_PASS

persistence_marker=CELLOS_X86_VIRTIO_E2E_PERSIST_V1
printf %s "$persistence_marker" > /tmp/expected
marker_len=${#persistence_marker}
irq5_before=$(irq_count 5)
[ "$irq5_before" -ge 0 ] || fail irq5-not-registered
dd if=/dev/vda of=/tmp/actual bs=1 count="$marker_len" 2>/dev/null || fail block-read

if cmp -s /tmp/expected /tmp/actual; then
    run_mode=second
    echo VIRTIO_E2E_BLOCK_READBACK_PASS
else
    run_mode=first
    dd if=/tmp/expected of=/dev/vda bs=1 count="$marker_len" conv=fsync 2>/dev/null \
        || fail block-write
    blockdev --flushbufs /dev/vda || fail block-flush
    echo VIRTIO_E2E_BLOCK_WRITE_FLUSH_PASS
fi

irq5_after=$(irq_count 5)
[ "$irq5_after" -gt "$irq5_before" ] || fail irq5-no-completion
echo VIRTIO_E2E_IRQ5_PASS

net_path=""
attempts=30
while [ -z "$net_path" ] && [ "$attempts" -gt 0 ]; do
    for candidate in /sys/bus/virtio/devices/virtio*/net/*; do
        if [ -d "$candidate" ]; then
            net_path="$candidate"
            break
        fi
    done
    [ -n "$net_path" ] || sleep 1
    attempts=$((attempts - 1))
done
[ -n "$net_path" ] || fail net-device-timeout
net_if=${net_path##*/}
echo VIRTIO_E2E_NET_DISCOVERY_PASS

irq6_before=$(irq_count 6)
[ "$irq6_before" -ge 0 ] || fail irq6-not-registered
ip link set "$net_if" up || fail net-link-up
ip addr add 10.0.2.15/24 dev "$net_if" || fail net-address
ip route add default via 10.0.2.2 dev "$net_if" || fail net-route
ping -c 1 -W 10 10.0.2.2 >/dev/null 2>&1 || fail net-round-trip
echo VIRTIO_E2E_NET_TX_RX_PASS

irq6_after=$(irq_count 6)
[ "$irq6_after" -gt "$irq6_before" ] || fail irq6-no-completion
echo VIRTIO_E2E_IRQ6_PASS

if [ "$run_mode" = first ]; then
    echo VIRTIO_E2E_FIRST_RUN_PASS
else
    echo VIRTIO_E2E_SECOND_RUN_PASS
fi
while :; do sleep 3600; done
