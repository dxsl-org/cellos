#!/bin/busybox sh
# Tier-3 guest endpoint fixture (S10-11 T3-A): OpenSSH server inside the Alpine PVH guest.
#
# Booted as `rdinit=/bin/virtio-e2e-init` (the x86 E2E profile's guest slot, selected by the
# `/virtio-e2e` file in the FAT image; cells/services/hypervisor/src/boot_x86_profile.rs).
# Everything is local to the initramfs: the pinned OpenSSH apks are staged flat as
# /media/t3-apk-<file> and installed with `apk add --no-network` from /apks/x86_64 (the repack
# tool adds files only and the kernel unpacker creates no parent directories, so every staged
# file sits in a directory the Alpine initramfs already has). The per-run host key, password,
# sshd_config and p9 helper are staged by scripts/prepare-tier3-sshd-initramfs-x86.sh as
# /etc/t3-* and installed after apk (apk would keep an existing /etc file as .apk-new). Static address 10.0.2.16/24, gateway
# 10.0.2.2: the outer Net cell's DHCP lease is 10.0.2.15.
#
# The console is the hypervisor's UART: every line shows up on the host serial log as
# `[guest-uart] ...`. Markers: TIER3_SSHD_*. The password is never printed.

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

say() { echo "TIER3_SSHD_$1"; }
die() { say "FAIL: $1"; while :; do sleep 3600; done; }

mkdir -p /proc /sys /dev /tmp /run /lib/apk/db /var/cache/apk /var/log /var/empty
# The Alpine initramfs archive carries `.` with mode 0700: without this the unprivileged
# `tester` cannot even exec /bin/sh ("Could not chdir to home directory ... Permission
# denied", T3-B boot 2). /tmp is the ledger directory of p9 and the scenarios.
chmod 755 /
chmod 1777 /tmp
mount -t proc proc /proc || die mount-proc
mount -t sysfs sysfs /sys || die mount-sysfs
mount -t devtmpfs devtmpfs /dev || die mount-devtmpfs
mkdir -p /dev/pts
mount -t devpts devpts /dev/pts || say "DEVPTS_MOUNT_FAIL"
echo /bin/mdev > /proc/sys/kernel/hotplug
mdev -s
modprobe virtio_net || say "MODPROBE_VIRTIO_NET_FAIL"
mdev -s

say "GUEST_INIT_START"
say "KERNEL $(uname -r)"

# No virtio-rng in this VMM: the CRNG fills from jitter. sshd needs it for key exchange.
if timeout 600 head -c 32 /dev/random >/dev/null 2>&1; then
    say "CRNG_READY"
else
    say "CRNG_WAIT_TIMEOUT"
fi

# ── network ───────────────────────────────────────────────────────────────
net_if=""
i=0
while [ -z "$net_if" ] && [ "$i" -lt 60 ]; do
    for cand in /sys/class/net/*; do
        [ "${cand##*/}" = lo ] && continue
        net_if="${cand##*/}"
    done
    [ -n "$net_if" ] || { i=$((i + 1)); sleep 1; }
done
[ -n "$net_if" ] || die net-device-timeout
ip link set lo up || true
ip link set "$net_if" up || die net-link-up
ip addr add 10.0.2.16/24 dev "$net_if" || die net-address
ip route add default via 10.0.2.2 dev "$net_if" || die net-route
say "NET_UP $net_if 10.0.2.16/24 gw 10.0.2.2"
if ping -c 1 -W 10 10.0.2.2 >/dev/null 2>&1; then say "GATEWAY_PING_PASS"; else say "GATEWAY_PING_FAIL"; fi

# ── OpenSSH from the pinned apk set, offline ──────────────────────────────
mkdir -p /apks/x86_64
for f in /media/t3-apk-*; do mv "$f" "/apks/x86_64/${f#/media/t3-apk-}" || die apk-stage; done
apk add --initdb --no-network --no-cache --repositories-file /dev/null --repository /apks openssh-server >/tmp/apk-add.txt 2>&1 \
    || { cat /tmp/apk-add.txt; die apk-add; }
rm -rf /apks
# sshd refuses to start unless it is executed with an absolute path.
SSHD=/usr/sbin/sshd
[ -x "$SSHD" ] || die sshd-binary-missing
say "APK_INSTALLED $("$SSHD" -V 2>&1 | head -1)"

: > /etc/shadow && chmod 600 /etc/shadow
addgroup -S sshd 2>/dev/null
adduser -S -D -H -h /var/empty -s /sbin/nologin -G sshd -g sshd sshd 2>/dev/null
mkdir -p /home /etc/ssh /run/sshd
adduser -D -s /bin/sh tester || die adduser
pw=$(cat /etc/t3-password)
echo "tester:$pw" | chpasswd -c sha512 || die chpasswd
unset pw
install -m 600 /etc/t3-host_ed25519 /etc/ssh/ssh_host_ed25519_key
install -m 644 /etc/t3-sshd_config /etc/ssh/sshd_config
install -m 755 /etc/t3-p9 /usr/bin/p9
rm -f /etc/t3-host_ed25519 /etc/t3-password
"$SSHD" -t -f /etc/ssh/sshd_config || die sshd-config-test
say "HOSTKEY $(ssh-keygen -l -f /etc/ssh/ssh_host_ed25519_key)"

# sshd -D -e: its log lines (Server listening, Connection from, Accepted/Failed) go to the
# console unmodified. Backgrounded so the init can print the listening marker.
"$SSHD" -D -e -f /etc/ssh/sshd_config &
sshd_pid=$!
i=0
# /proc/net/tcp: local port 0x0016, state 0A (LISTEN)
until grep -q ':0016 00000000:0000 0A' /proc/net/tcp 2>/dev/null; do
    kill -0 "$sshd_pid" 2>/dev/null || die sshd-exited
    i=$((i + 1))
    [ "$i" -lt 120 ] || die sshd-not-listening
    sleep 1
done
say "LISTENING"
wait "$sshd_pid"
say "SSHD_EXITED $?"
while :; do sleep 3600; done
