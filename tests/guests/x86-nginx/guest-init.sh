#!/bin/busybox sh
# Tier 3 gate fixture — run nginx inside the Alpine Linux guest.
#
# Booted as `rdinit=/bin/virtio-e2e-init` (the x86 E2E profile's guest slot;
# see cells/services/hypervisor/src/boot_x86_profile.rs) on the x86_64 PVH
# VirtIO-MMIO guest. Installs nginx with `apk` from the pinned Alpine v3.21
# repository, starts it, and fetches a page from inside the guest.
#
# @BUILD_UTC@ / @BUILD_UTC_STR@ are substituted at repack time
# (scripts/prepare-x86-app-initramfs.sh): the emulated guest RTC reads back no
# time, so without this the CDN's TLS chain fails verification and the install
# silently falls back to plain HTTP (which v3.21 still serves, but the gate
# should exercise the verified path).
#
# Emits NGINX_IN_VM_* markers; drops to an interactive shell on failure.

export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

say() { echo "NGINX_IN_VM_$1"; }
die() { say "FAIL: $1"; exec /bin/sh; }

mkdir -p /proc /sys /dev /tmp /run /lib/apk/db /var/cache/apk /var/log
mount -t proc proc /proc || die mount-proc
mount -t sysfs sysfs /sys || die mount-sysfs
mount -t devtmpfs devtmpfs /dev || die mount-devtmpfs
echo /bin/mdev > /proc/sys/kernel/hotplug
mdev -s
modprobe virtio_blk || true
modprobe virtio_net || true
mdev -s

say "GUEST_INIT_START"
echo "guest kernel: $(uname -r)"
apk --version
date -u -s '@BUILD_UTC@' >/dev/null 2>&1 \
    || date -u -s '@BUILD_UTC_STR@' >/dev/null 2>&1 \
    || say "CLOCK_SET_FAIL"
echo "guest clock: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"

# The guest CRNG is not seeded at boot, and /dev/random blocks until it is.
# Generating the first TLS ClientHello needs those bytes: measured with a
# packet capture, the connection completed its handshake and then sat idle for
# ~60 s of host time before the ClientHello appeared — long enough for the
# SLIRP peer's idle timeout to close it (`SSL routines::unexpected eof while
# reading`), while the second attempt succeeded immediately. Wait for the pool
# instead of racing it.
if timeout 240 head -c 32 /dev/random >/dev/null 2>&1; then
    say "CRNG_READY"
else
    say "CRNG_WAIT_TIMEOUT"
fi

# ── Guest network: the repository install path needs it ────────────────────
net_if=""
i=0
while [ -z "$net_if" ] && [ "$i" -lt 30 ]; do
    for cand in /sys/class/net/*; do
        [ "${cand##*/}" = lo ] && continue
        net_if="${cand##*/}"
    done
    [ -n "$net_if" ] || { i=$((i + 1)); sleep 1; }
done
[ -n "$net_if" ] || die net-device-timeout
echo "guest net iface: $net_if"
ip link set "$net_if" up || die net-link-up
ip link set lo up || true
ip addr add 10.0.2.15/24 dev "$net_if" || die net-address
ip route add default via 10.0.2.2 dev "$net_if" || die net-route
echo 'nameserver 10.0.2.3' > /etc/resolv.conf
if ping -c 1 -W 5 10.0.2.2 >/dev/null 2>&1; then
    say "GATEWAY_PING_PASS"
else
    say "GATEWAY_PING_FAIL"
fi
nslookup dl-cdn.alpinelinux.org >/tmp/nslookup.txt 2>&1 && say "DNS_PASS" || say "DNS_FAIL"

# ── Install nginx from the pinned repository ───────────────────────────────
write_repos() {
    printf '%s\n' \
        "$1://dl-cdn.alpinelinux.org/alpine/v3.21/main" \
        "$1://dl-cdn.alpinelinux.org/alpine/v3.21/community" \
        > /etc/apk/repositories
}
installed=""
write_repos https
if timeout 600 apk --initdb add --no-cache nginx >/tmp/apk-add.txt 2>&1; then
    say "APK_REPO_INSTALL_PASS"
    installed=repo-https
else
    say "APK_REPO_INSTALL_FAIL"
    echo "--- https install attempt, first 12 lines ---"
    head -n 12 /tmp/apk-add.txt
    write_repos http
    if timeout 600 apk --initdb add --no-cache nginx >/tmp/apk-add-http.txt 2>&1; then
        say "APK_REPO_HTTP_INSTALL_PASS"
        installed=repo-http
    else
        say "APK_REPO_HTTP_INSTALL_FAIL"
    fi
fi
if [ -z "$installed" ]; then
    tail -n 25 /tmp/apk-add.txt /tmp/apk-add-http.txt 2>/dev/null
    die nginx-not-installed
fi
echo "nginx install path: $installed"
nginx -v 2>&1
command -v nginx || die nginx-binary-missing

# ── Serve a page from inside the guest ─────────────────────────────────────
mkdir -p /etc/nginx /var/www /var/log/nginx /var/lib/nginx/tmp/client_body
echo 'cellos-tier3-nginx-ok' > /var/www/index.html
cat > /etc/nginx/nginx.conf <<'EOF'
user root;
worker_processes 1;
error_log /tmp/nginx-error.log warn;
pid /run/nginx.pid;
events { worker_connections 64; }
http {
    access_log /tmp/nginx-access.log;
    client_body_temp_path /var/lib/nginx/tmp/client_body;
    server {
        listen 80;
        root /var/www;
        index index.html;
    }
}
EOF
nginx -t 2>&1 || die nginx-config-test
nginx || die nginx-start
sleep 2

# fork() is the reason this workload belongs in a Tier 3 guest: master + worker.
workers=$(ps | grep -c '[n]ginx')
echo "nginx processes: $workers"
[ "$workers" -ge 2 ] && say "FORK_MASTER_WORKER_PASS" || die nginx-single-process

ok=0
for attempt in 1 2 3; do
    if wget -q -T 5 -O /tmp/index.html http://127.0.0.1/; then ok=1; break; fi
    sleep 2
done
[ "$ok" = 1 ] || { cat /tmp/nginx-error.log 2>/dev/null; die http-fetch; }
if grep -q 'cellos-tier3-nginx-ok' /tmp/index.html; then
    say "HTTP_SERVE_PASS"
    echo "served body: $(cat /tmp/index.html)"
else
    say "HTTP_BODY_MISMATCH"
    cat /tmp/index.html
    die http-body
fi

say "ALL_PASS"
exec /bin/sh
