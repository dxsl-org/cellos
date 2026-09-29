#!/bin/busybox sh
# CPython workload inside the Alpine Tier 3 guest, not a Cellos-native Python VM.
# Build-time UTC placeholders are filled by prepare-x86-app-initramfs.sh.
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
/bin/busybox --install -s /bin

say() { echo "PYTHON_IN_VM_$1"; }
die() { say "FAIL: $1"; exec /bin/sh; }

mkdir -p /proc /sys /dev /tmp /run /lib/apk/db /var/cache/apk
mount -t proc proc /proc || die mount-proc
mount -t sysfs sysfs /sys || die mount-sysfs
mount -t devtmpfs devtmpfs /dev || die mount-devtmpfs
echo /bin/mdev > /proc/sys/kernel/hotplug
mdev -s
modprobe virtio_net || true
mdev -s
say GUEST_INIT_START
date -u -s '@BUILD_UTC@' >/dev/null 2>&1 \
    || date -u -s '@BUILD_UTC_STR@' >/dev/null 2>&1 \
    || die clock-set
# Under nested TCG the first TLS handshake can time out while the CRNG seeds.
timeout 240 head -c 32 /dev/random >/dev/null 2>&1 || die crng-timeout
say CRNG_READY

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
ip link set "$net_if" up || die net-link-up
ip addr add 10.0.2.15/24 dev "$net_if" || die net-address
ip route add default via 10.0.2.2 dev "$net_if" || die net-route
printf 'nameserver 10.0.2.3\n' > /etc/resolv.conf
nslookup dl-cdn.alpinelinux.org >/tmp/dns.log 2>&1 || die dns
say DNS_PASS
printf '%s\n' 'https://dl-cdn.alpinelinux.org/alpine/v3.21/main' \
    'https://dl-cdn.alpinelinux.org/alpine/v3.21/community' > /etc/apk/repositories
say APK_FETCH_START
if ! timeout 600 apk --initdb add --no-cache python3; then
    die apk-python3
fi
python3 --version || die python3-missing
say APK_PASS

# A small batch-processing task: parse records, use exact money arithmetic,
# aggregate by project, and publish a sorted CSV consumed by a second process.
cat > /tmp/transactions.jsonl <<'JSON'
{"project":"field","amount":"7.25"}
{"project":"lab","amount":"12.50"}
{"project":"field","amount":"0.75"}
{"project":"lab","amount":"-2.00"}
JSON
cat > /tmp/report.py <<'PY'
import csv
import json
import sys
from decimal import Decimal

sums = {}
with open(sys.argv[1], encoding="utf-8") as source:
    for line in source:
        record = json.loads(line)
        project = record["project"]
        sums[project] = sums.get(project, Decimal("0")) + Decimal(record["amount"])
with open(sys.argv[2], "w", newline="", encoding="utf-8") as result:
    writer = csv.writer(result, lineterminator="\n")
    writer.writerow(("project", "net"))
    for project in sorted(sums):
        writer.writerow((project, f"{sums[project]:.2f}"))
PY
python3 /tmp/report.py /tmp/transactions.jsonl /tmp/report.csv || die report-script
printf 'project,net\nfield,8.00\nlab,10.50\n' > /tmp/expected.csv
cmp -s /tmp/expected.csv /tmp/report.csv || die report-content
# A separate CPython process consumes the produced artifact; this exercises
# the guest's fork/exec and stdlib rather than merely checking the interpreter.
python3 -c 'import csv,sys; rows=list(csv.DictReader(open(sys.argv[1], newline="", encoding="utf-8"))); assert rows == [{"project":"field","net":"8.00"},{"project":"lab","net":"10.50"}]' /tmp/report.csv || die child-consumer
say WORKLOAD_PASS
exec /bin/sh
