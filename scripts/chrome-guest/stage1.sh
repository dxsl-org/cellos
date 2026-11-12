#!/bin/busybox sh
# Chrome guest stage 1 (initramfs, rdinit=/bin/chrome-guest-stage1).
#
# Mounts the read-only Chromium root (squashfs on the hypervisor's only disk,
# /dev/vda) under a tmpfs overlay and switches into it. Nothing in the guest
# is persistent. Every failure is printed as CHROME_GUEST_FAIL:<step> on the
# serial console and powers the guest off.
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

fail() {
    echo "CHROME_GUEST_FAIL:$1" > /dev/ttyS0
    poweroff -f
}

mount -t proc proc /proc || fail mount-proc
mount -t sysfs sysfs /sys || fail mount-sys
mount -t devtmpfs devtmpfs /dev || fail mount-dev
for module in virtio_mmio virtio_blk squashfs overlay; do
    modprobe "$module" 2>/dev/null
done
tries=0
while [ ! -b /dev/vda ]; do
    tries=$((tries + 1))
    [ "$tries" -gt 100 ] && fail no-vda
    sleep 0.1
done

mkdir -p /lower /rw /newroot
mount -t squashfs -o ro /dev/vda /lower || fail mount-squashfs
mount -t tmpfs -o mode=0755 tmpfs /rw || fail mount-rw
mkdir -p /rw/upper /rw/work
mount -t overlay overlay -o lowerdir=/lower,upperdir=/rw/upper,workdir=/rw/work /newroot \
    || fail mount-overlay
for fs in proc sys dev; do
    mount --move "/$fs" "/newroot/$fs" || fail "move-$fs"
done
echo "CHROME_GUEST_ROOT_MOUNTED" > /newroot/dev/ttyS0
exec switch_root /newroot /sbin/chrome-guest-init
fail switch-root
