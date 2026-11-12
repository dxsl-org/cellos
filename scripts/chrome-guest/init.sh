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

# Every marker carries the guest uptime, so a slow start can be placed.
log() { echo "$1 up=$(cut -d' ' -f1 /proc/uptime)" > /dev/ttyS0; }

mount -t tmpfs -o mode=0755 tmpfs /run
mount -t tmpfs -o mode=1777 tmpfs /tmp
mkdir -p /dev/pts /dev/shm "$XDG_RUNTIME_DIR"
mount -t devpts devpts /dev/pts
mount -t tmpfs -o mode=1777 tmpfs /dev/shm
chmod 0700 "$XDG_RUNTIME_DIR"
hostname cellos-chrome

for module in virtio_gpu virtio_input evdev virtio_net af_packet; do
    modprobe "$module" 2>/dev/null
done
udevd --daemon
udevadm trigger --action=add
udevadm settle --timeout=10
[ -e /dev/dri/card0 ] || { log "CHROME_GUEST_FAIL:no-drm"; poweroff -f; }
log "CHROME_GUEST_DEVICES $(free -m | awk '/^Mem:/ { print "mem_total_mib=" $2 }') utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# The lease comes from the upstream network (the CellOS net service bridges this
# guest's MAC beside its own stack). udhcpc stays running to renew it; without a
# lease Chromium still starts and shows its own network error.
ip link set lo up
if ip link set eth0 up 2>/dev/null && udhcpc -i eth0 -t 10 -T 3 -b -p /run/udhcpc.pid >/dev/null 2>&1 \
    && ip -4 addr show eth0 | grep -q 'inet '; then
    log "CHROME_GUEST_NET lease=$(ip -4 addr show eth0 | awk '/inet / { print $2; exit }') dns=$(awk '/^nameserver/ { print $2; exit }' /etc/resolv.conf)"
else
    log "CHROME_GUEST_NET_FAIL:no-lease"
fi

dbus-uuidgen --ensure
mkdir -p /run/dbus
dbus-daemon --system --fork

seatd -g root &

# The launcher's window size reaches the guest as a virtio-gpu display change, a
# DRM hotplug uevent; the kernel's connector then lists the new size first. Cage
# keeps both its mode and its stale mode list on hotplug, so set that size as a
# custom mode; Chromium follows the output size.
follow_display() {
    udevadm monitor --udev --subsystem-match=drm | while read -r line; do
        case "$line" in
        *change*)
            output=$(wlr-randr 2>/dev/null | awk 'NR == 1 { print $1 }')
            size=$(head -n 1 /sys/class/drm/card0-*/modes 2>/dev/null)
            [ -n "$output" ] && [ -n "$size" ] || continue
            if wlr-randr --output "$output" --custom-mode "$size" 2>/dev/null; then
                log "CHROME_GUEST_DISPLAY $size"
            else
                log "CHROME_GUEST_DISPLAY_FAIL:$output:$size"
            fi
            ;;
        esac
    done
}
# Not exported globally: cage would take WAYLAND_DISPLAY as a parent compositor.
(export WAYLAND_DISPLAY=wayland-0; follow_display) &

log "CHROME_GUEST_STARTING_CAGE"
# Key repeat comes from the host (see no-key-repeat.c); only cage loads the shim.
LD_PRELOAD=/usr/lib/chrome-guest/no-key-repeat.so \
    cage -s -- env -u LD_PRELOAD chromium --ozone-platform=wayland --disable-gpu --no-sandbox \
    --no-first-run --no-default-browser-check --user-data-dir=/tmp/chrome \
    --disable-dev-shm-usage ${CHROME_URL:-about:blank} > /tmp/cage.log 2>&1
status=$?
log "CHROME_GUEST_EXITED:$status"
tail -n 40 /tmp/cage.log > /dev/ttyS0
poweroff -f
