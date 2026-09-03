#!/usr/bin/env bash
# Helper functions for Ubuntu 24.04 amd64 wide-guest image creation.
# Sourced by scripts/build-ubuntu-wide-guest-x86.sh.

set -euo pipefail

configure_ubuntu_rootfs() {
    local root="$1" snapshot="$2"
    rm -f "$root/etc/apt/sources.list"
    rm -rf "$root/etc/apt/sources.list.d"
    mkdir -p "$root/etc/apt/sources.list.d" "$root/etc/apt/apt.conf.d"
    cat > "$root/etc/apt/sources.list.d/cellos.sources" <<EOF
Types: deb
URIs: https://snapshot.ubuntu.com/ubuntu/${snapshot}/
Suites: noble noble-updates noble-security
Components: main universe
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
    cat > "$root/etc/apt/apt.conf.d/99cellos-snapshot" <<'EOF'
Acquire::Check-Valid-Until "false";
APT::Install-Recommends "false";
EOF

    printf 'cellos-ubuntu\n' > "$root/etc/hostname"
    printf '127.0.0.1 localhost\n127.0.1.1 cellos-ubuntu\n' > "$root/etc/hosts"
    rm -f "$root/etc/resolv.conf"
    printf 'nameserver 10.0.2.3\noptions timeout:2 attempts:3\n' > "$root/etc/resolv.conf"
    printf '/dev/vda / ext4 defaults 0 1\n' > "$root/etc/fstab"
    : > "$root/etc/machine-id"
    rm -f "$root/var/lib/systemd/random-seed"
    mkdir -p "$root/etc/cloud"
    touch "$root/etc/cloud/cloud-init.disabled"

    mkdir -p "$root/etc/systemd/network" \
        "$root/etc/systemd/system/multi-user.target.wants" \
        "$root/etc/systemd/system/getty.target.wants" \
        "$root/etc/systemd/system/serial-getty@ttyS0.service.d"
    cat > "$root/etc/systemd/network/20-virtio.network" <<'EOF'
[Match]
Name=eth*

[Network]
DHCP=yes
IPv6AcceptRA=no
EOF
    cat > "$root/etc/systemd/system/cellos-boot-ready.service" <<'EOF'
[Unit]
Description=Cellos Ubuntu multi-user evidence marker
After=systemd-user-sessions.service

[Service]
Type=oneshot
ExecStart=/bin/sh -c 'printf "CELLOS_UBUNTU_MULTI_USER_READY_V1\n" > /dev/ttyS0'
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF
    cat > "$root/etc/systemd/system/serial-getty@ttyS0.service.d/autologin.conf" <<'EOF'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I 115200,38400,9600 $TERM
EOF
    ln -sfn /usr/lib/systemd/system/systemd-networkd.service \
        "$root/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
    ln -sfn /usr/lib/systemd/system/serial-getty@.service \
        "$root/etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service"
    ln -sfn ../cellos-boot-ready.service \
        "$root/etc/systemd/system/multi-user.target.wants/cellos-boot-ready.service"
    ln -sfn /dev/null "$root/etc/systemd/system/apt-daily.timer"
    ln -sfn /dev/null "$root/etc/systemd/system/apt-daily-upgrade.timer"
    cat > "$root/root/.profile" <<'EOF'
export TERM=vt100
export PS1='CELLOS_UBUNTU_ROOT# '
EOF
    chmod 0600 "$root/root/.profile"
}

build_switch_root_initrd() {
    local base_initrd="$1" initrd_tree="$2" output_initrd="$3" epoch="$4"
    gzip -dc "$base_initrd" | \
        (cd "$initrd_tree" && cpio -idmu --no-absolute-filenames >/dev/null 2>&1)
    cat > "$initrd_tree/init" <<'EOF'
#!/bin/busybox sh
set -eu
/bin/busybox --install -s /bin
mkdir -p /dev /proc /sys /run /newroot
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mount -t proc proc /proc
mount -t sysfs sysfs /sys
modprobe virtio_mmio 2>/dev/null || true
modprobe virtio_blk 2>/dev/null || true
modprobe ext4 2>/dev/null || true
attempt=0
while ! mount -t ext4 -o rw /dev/vda /newroot 2>/dev/null; do
    attempt=$((attempt + 1))
    if [ "$attempt" -ge 120 ]; then
        echo "CELLOS_UBUNTU_ROOT_MOUNT_FAIL_V1" > /dev/console
        reboot -f
        while :; do sleep 60; done
    fi
    sleep 1
done
mount --move /dev /newroot/dev
mount --move /proc /newroot/proc
mount --move /sys /newroot/sys
exec switch_root /newroot /sbin/init
EOF
    chmod 0755 "$initrd_tree/init"
    find "$initrd_tree" -exec touch -h -d "@$epoch" {} +
    (
        cd "$initrd_tree"
        find . -print0 | LC_ALL=C sort -z | \
            cpio --null -o --format=newc --owner=0:0 2>/dev/null | gzip -n -9
    ) > "$output_initrd"
}
