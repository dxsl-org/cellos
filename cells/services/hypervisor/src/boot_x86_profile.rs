//! Profile, memory sizing, and kernel command-line selection for x86 PVH boot.

use ostd::io::println;

#[cfg(not(feature = "ubuntu-wide-guest"))]
pub const GUEST_RAM_SIZE: u64 = 128 * 1024 * 1024;
#[cfg(feature = "ubuntu-wide-guest")]
pub const GUEST_RAM_SIZE: u64 = 512 * 1024 * 1024;
pub const GUEST_RAM_PAGES: usize = (GUEST_RAM_SIZE / 4096) as usize;

pub const VMLINUX_PATH: &str = "/vmlinux";
pub const INITRD_PATH: &str = "/initrd.gz";

#[cfg(not(feature = "ubuntu-wide-guest"))]
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic lpj=4000000 nohz=off highres=off rdinit=/bin/sh panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 -- -i";
#[cfg(not(feature = "ubuntu-wide-guest"))]
const E2E_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/virtio-e2e-init panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6";
#[cfg(feature = "ubuntu-wide-guest")]
const UBUNTU_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off root=/dev/vda rw rootfstype=ext4 rootwait init=/sbin/init systemd.unit=multi-user.target net.ifnames=0 panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6";

#[cfg(feature = "ubuntu-wide-guest")]
pub fn guest_cmdline() -> &'static str {
    println("[hv-x86] guest profile: ubuntu-wide-guest (512 MiB, /dev/vda)");
    UBUNTU_CMDLINE
}

#[cfg(not(feature = "ubuntu-wide-guest"))]
pub fn guest_cmdline() -> &'static str {
    let Ok(cap) = ostd::syscall::sys_open_cap("/virtio-e2e") else {
        return CMDLINE;
    };
    ostd::syscall::sys_close_cap(cap);
    println("[hv-x86] guest rdinit: /bin/virtio-e2e-init");
    E2E_CMDLINE
}
