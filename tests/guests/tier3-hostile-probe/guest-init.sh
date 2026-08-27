#!/bin/busybox sh
/bin/busybox --install -s /bin
export PATH=/bin:/sbin:/usr/bin:/usr/sbin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev

echo "Initializing hostile probe environment..."

# 1. Bounds
/bin/tier3-hostile-probe 1

# 2. Descriptor shape
/bin/tier3-hostile-probe 2

# 3. Backend unavailable
/bin/tier3-hostile-probe 3

# Bounds, descriptor, and backend inputs have no guest-visible VMM/VirtIO
# transport. The budget loop and power-off remain real stimuli for later runners.
/bin/tier3-hostile-probe 5 &
sleep 2
echo "[HOSTILE_PROBE] RESET_TEST_STARTED"
poweroff -f
while true; do sleep 1000; done
