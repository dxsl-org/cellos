#!/bin/sh
# Chrome guest stage 2 (/sbin/chrome-guest-init in the squashfs root).
#
# Brings up devices with eudev, then runs Chromium fullscreen under cage on the
# virtio-gpu DRM device with the pixman (CPU) renderer. Chromium runs without
# its own sandbox: the VM boundary contains it (ADR-0017). When Chromium exits
# the guest powers off, which ends the hypervisor's session.
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
export HOME=/root XDG_RUNTIME_DIR=/run/user/0
export WLR_RENDERER=pixman WLR_NO_HARDWARE_CURSORS=1 LIBSEAT_BACKEND=seatd

log() { echo "$1" > /dev/ttyS0; }

mount -t tmpfs -o mode=0755 tmpfs /run
mount -t tmpfs -o mode=1777 tmpfs /tmp
mkdir -p /dev/pts /dev/shm "$XDG_RUNTIME_DIR"
mount -t devpts devpts /dev/pts
mount -t tmpfs -o mode=1777 tmpfs /dev/shm
chmod 0700 "$XDG_RUNTIME_DIR"
hostname cellos-chrome

for module in virtio_gpu virtio_input evdev; do
    modprobe "$module" 2>/dev/null
done
udevd --daemon
udevadm trigger --action=add
udevadm settle --timeout=10
[ -e /dev/dri/card0 ] || { log "CHROME_GUEST_FAIL:no-drm"; poweroff -f; }
log "CHROME_GUEST_DEVICES $(free -m | awk '/^Mem:/ { print "mem_total_mib=" $2 }')"

dbus-uuidgen --ensure
mkdir -p /run/dbus
dbus-daemon --system --fork

seatd -g root &
log "CHROME_GUEST_STARTING_CAGE"
cage -s -- chromium --ozone-platform=wayland --disable-gpu --no-sandbox \
    --no-first-run --no-default-browser-check --user-data-dir=/tmp/chrome \
    --disable-dev-shm-usage ${CHROME_URL:-about:blank} > /tmp/cage.log 2>&1
status=$?
log "CHROME_GUEST_EXITED:$status"
tail -n 40 /tmp/cage.log > /dev/ttyS0
poweroff -f
