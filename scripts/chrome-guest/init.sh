#!/bin/sh
# Chrome guest stage 2 (/sbin/chrome-guest-init in the squashfs root).
#
# Brings up devices with eudev, then runs Chromium under cage on the virtio-gpu
# DRM device with the pixman (CPU) renderer. The window shows Chromium and
# nothing else of the guest: cage has no desktop and takes no VT switch,
# Chromium draws no window buttons of its own, and the pointer is the host's.
# Chromium runs as an unprivileged user with its own sandbox on; the VM boundary
# contains what gets past it (ADR-0017). When Chromium exits the guest powers
# off, which ends the hypervisor's session.
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
# No WLR_NO_HARDWARE_CURSORS: cage puts its pointer on the virtio-gpu cursor
# plane, which the hypervisor does not draw into a launcher's window.
export WLR_RENDERER=pixman LIBSEAT_BACKEND=seatd

# Every marker carries the guest uptime, so a slow start can be placed.
log() { echo "$1 up=$(cut -d' ' -f1 /proc/uptime)" > /dev/ttyS0; }

mount -t tmpfs -o mode=0755 tmpfs /run
mount -t tmpfs -o mode=1777 tmpfs /tmp
mkdir -p /dev/pts /dev/shm
mount -t devpts devpts /dev/pts
mount -t tmpfs -o mode=1777 tmpfs /dev/shm
hostname cellos-chrome

for module in virtio_gpu virtio_input evdev virtio_net af_packet 9pnet_virtio 9p virtio_snd; do
    modprobe "$module" 2>/dev/null
done
udevd --daemon
udevadm trigger --action=add
udevadm settle --timeout=10
[ -e /dev/dri/card0 ] || { log "CHROME_GUEST_FAIL:no-drm"; poweroff -f; }
log "CHROME_GUEST_DEVICES $(free -m | awk '/^Mem:/ { print "mem_total_mib=" $2 }') cpus=$(nproc) utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# The sound card Chromium plays through (ALSA's default device), if the machine has one.
log "CHROME_GUEST_SOUND cards=$(grep -c '^ *[0-9]' /proc/asound/cards 2>/dev/null || echo 0) first=$(awk 'NR == 1 { print $2 }' /proc/asound/cards 2>/dev/null)"
# Which clock event the kernel settled on, and whether its timers are
# high-resolution: `lapic-deadline` and 1 where the hypervisor gives the guest a
# local APIC timer, the PIT and 0 where it does not.
log "CHROME_GUEST_CLOCK event=$(cat /sys/devices/system/clockevents/clockevent0/current_device 2>/dev/null) source=$(cat /sys/devices/system/clocksource/clocksource0/current_clocksource 2>/dev/null) hres=$(awk '/hres_active/ { print $3; exit }' /proc/timer_list 2>/dev/null)"
# Vector extensions the guest kernel enabled (Chromium's software raster picks
# its code paths from them): 1 where the hypervisor gives the guest AVX state.
log "CHROME_GUEST_SIMD avx=$(grep -qw avx /proc/cpuinfo && echo 1 || echo 0) avx2=$(grep -qw avx2 /proc/cpuinfo && echo 1 || echo 0)"

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

# Chromium's sandbox does not run as root, so Chromium gets a user of its own
# (the root is a tmpfs overlay: the account lasts as long as the guest).
adduser -D -h /home/chrome -s /bin/sh chrome || { log "CHROME_GUEST_FAIL:no-user"; poweroff -f; }
# The sound card's device nodes belong to group audio.
addgroup chrome audio 2>/dev/null
export HOME=/home/chrome XDG_RUNTIME_DIR="/run/user/$(id -u chrome)"
mkdir -p "$XDG_RUNTIME_DIR" /tmp/chrome/Default
# "Use system title bar and borders": Chromium then draws no minimize, maximize
# and close buttons of its own; the window's controls are the CellOS window's.
echo '{"browser":{"custom_chrome_frame":false}}' > /tmp/chrome/Default/Preferences
chown -R chrome:chrome "$XDG_RUNTIME_DIR" /tmp/chrome
chmod 0700 "$XDG_RUNTIME_DIR"

# Downloads: the host folder the hypervisor shares over virtio-9p (tag `downloads`; a data
# disk's Chrome folder, or a temporary one in the host's /tmp). A policy points Chromium's
# downloads there and stops it asking where to save. Without the share they stay in the
# guest's tmpfs and go when the window closes.
mkdir -p /home/chrome/Downloads /etc/chromium/policies/managed
chown chrome:chrome /home/chrome/Downloads
if mount -t 9p -o trans=virtio,version=9p2000.L,msize=131072,access=any \
    downloads /home/chrome/Downloads 2>/dev/null; then
    log "CHROME_GUEST_DOWNLOADS mounted"
else
    log "CHROME_GUEST_DOWNLOADS_FAIL:no-share"
fi
echo '{"DownloadDirectory":"/home/chrome/Downloads","PromptForDownloadLocation":false}' \
    > /etc/chromium/policies/managed/downloads.json

# Errors only: seatd's per-client lines go to the serial console, and every
# console byte costs the hypervisor a VM exit.
seatd -u chrome -l error &

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

# The CellOS clipboard (clipboard-agent): the hypervisor's virtio console port
# is the agent's alone. A guest without the port (an older hypervisor) runs
# without a clipboard.
if [ -c /dev/hvc0 ] && chown chrome /dev/hvc0; then
    su chrome -s /bin/sh -c "/usr/lib/chrome-guest/clipboard-agent /dev/hvc0 /tmp/clipboard.log" &
    log "CHROME_GUEST_CLIPBOARD port=hvc0"
else
    log "CHROME_GUEST_CLIPBOARD_FAIL:no-port"
fi

# Key repeat comes from the host (see no-key-repeat.c) and the clipboard agent
# needs wlr-data-control (see data-control.c); only cage loads the shims.
# No `-s`: Ctrl+Alt+Fn must not leave Chromium for a Linux console.
run_chromium() {
    su chrome -s /bin/sh -c "LD_PRELOAD=/usr/lib/chrome-guest/no-key-repeat.so:/usr/lib/chrome-guest/data-control.so \
        cage -- env -u LD_PRELOAD chromium --ozone-platform=wayland --disable-gpu \
        --no-first-run --no-default-browser-check --user-data-dir=/tmp/chrome \
        --hide-crash-restore-bubble \
        --disable-dev-shm-usage ${CHROME_URL:-about:blank} > /tmp/cage.log 2>&1"
}

# cage (and with it Chromium) killed by a signal is started again, twice at
# most: about one launch in twenty under the CellOS hypervisor dies of SIGSEGV
# (seen in cage, inside ld-musl, twice in a row in one guest). Before the
# restart the guest says why: the kernel's fault line, and the checksums of the
# code that crashed as cached in memory and as read again from the disk. Sums
# that differ mean something wrote into this guest's page cache.
# Leaving on its own, by quitting or with an error it reports, ends the guest.
code_sums() {
    md5sum /lib/ld-musl-x86_64.so.1 /usr/bin/cage /usr/lib/libwlroots*.so* 2>/dev/null \
        | awk '{ printf "%s=%s ", $2, substr($1, 1, 12) }'
}
attempt=1
while :; do
    log "CHROME_GUEST_STARTING_CAGE"
    run_chromium
    status=$?
    log "CHROME_GUEST_EXITED:$status"
    tail -n 40 /tmp/cage.log > /dev/ttyS0
    # Why Chromium left, when it says so (its log is mostly GPU-process noise).
    grep -E 'FATAL|Goodbye|Check failed|exit_code|Shutting down' /tmp/cage.log | tail -n 6 \
        | sed 's/^/CHROME_GUEST_WHY /' > /dev/ttyS0
    tail -n 12 /tmp/clipboard.log 2>/dev/null | sed 's/^/CHROME_GUEST_CLIPBOARD_LOG /' > /dev/ttyS0
    [ "$status" -ge 128 ] && [ "$attempt" -lt 3 ] || break
    dmesg | grep -E 'segfault|traps:|general protection|Out of memory' | tail -n 4 > /dev/ttyS0
    cached=$(code_sums)
    sync
    echo 3 > /proc/sys/vm/drop_caches
    reread=$(code_sums)
    if [ "$cached" = "$reread" ]; then
        log "CHROME_GUEST_CODE_SAME $cached"
    else
        log "CHROME_GUEST_CODE_CHANGED cached: $cached"
        log "CHROME_GUEST_CODE_CHANGED disk: $reread"
    fi
    attempt=$((attempt + 1))
done
poweroff -f
