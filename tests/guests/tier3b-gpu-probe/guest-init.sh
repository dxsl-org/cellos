#!/bin/busybox sh

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
busybox --install -s /bin
mkdir -p /dev /proc /sys /run /modloop /lib
mount -t devtmpfs devtmpfs /dev
mount -t proc proc /proc
mount -t sysfs sysfs /sys

if ! mount -t squashfs -o loop /modloop.squashfs /modloop; then
    echo "TIER3B_T1_CARD0_FAIL modloop"
    poweroff -f
fi
rm -rf /lib/modules
ln -s /modloop/modules /lib/modules
depmod -a
modprobe virtio_gpu
mdev -s

attempt=0
while [ ! -e /dev/dri/card0 ] && [ "$attempt" -lt 50 ]; do
    sleep 0.1
    attempt=$((attempt + 1))
done

/tier3b-gpu-probe
result=$?
sync
poweroff -f
exit "$result"
