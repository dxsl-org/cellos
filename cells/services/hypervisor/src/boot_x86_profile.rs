//! Profile, memory sizing, and kernel command-line selection for x86 PVH boot.

use ostd::io::println;

#[cfg(all(not(feature = "ubuntu-wide-guest"), not(feature = "alpine-wide-guest")))]
const CONSOLE_RAM_SIZE: u64 = 128 * 1024 * 1024;
#[cfg(all(not(feature = "ubuntu-wide-guest"), feature = "alpine-wide-guest"))]
const CONSOLE_RAM_SIZE: u64 = 256 * 1024 * 1024;
#[cfg(feature = "ubuntu-wide-guest")]
const CONSOLE_RAM_SIZE: u64 = 512 * 1024 * 1024;
/// Chromium's guest: the guest kernel reports 1475 MiB usable at this size.
/// The carve is one contiguous host run, so the outer machine needs margin.
const CHROME_RAM_SIZE: u64 = 1536 * 1024 * 1024;

pub const VMLINUX_PATH: &str = "/vmlinux";
const INITRD_PATH: &str = "/initrd.gz";
/// Alpine's initramfs plus `scripts/chrome-guest/stage1.sh`.
const CHROME_INITRD_PATH: &str = "/chrome-initrd.gz";
/// The read-only Chromium root on the CellOS SD volume
/// (`scripts/build-chrome-guest-image.sh`).
pub const CHROME_DISK_NAME: &str = "chrome-guest.sqfs";

// Slots 3 (virtio-gpu) and 4 (virtio-input) share ISA IRQ 3 (`x86_irq_dispatch::DISPLAY_IRQ`).
#[cfg(not(feature = "ubuntu-wide-guest"))]
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/sh panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3 -- -i";
const E2E_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/virtio-e2e-init panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3";
const CHROME_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/chrome-guest-stage1 panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3";
#[cfg(feature = "ubuntu-wide-guest")]
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off root=/dev/vda rw rootfstype=ext4 rootwait init=/sbin/init systemd.unit=multi-user.target net.ifnames=0 panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3";

/// What this hypervisor boots, chosen once at start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuestProfile {
    /// The build's console guest (`hv` from the shell).
    Console,
    /// A test image's `/virtio-e2e` selector: its fixture init, console sizing.
    Fixture,
    /// A Chrome launcher's guest: Chromium under cage on a read-only disk.
    Chrome,
}

impl GuestProfile {
    /// A test fixture wins over the launcher, so lanes keep exercising the
    /// session path without the Chromium image.
    pub fn select(app_mode: bool) -> Self {
        let profile = if fixture_present() {
            Self::Fixture
        } else if app_mode {
            Self::Chrome
        } else {
            Self::Console
        };
        println(&alloc::format!(
            "[hv-x86] guest profile: {:?} ({} MiB)",
            profile,
            profile.ram_size() / (1024 * 1024)
        ));
        profile
    }

    pub fn ram_size(self) -> u64 {
        match self {
            Self::Console | Self::Fixture => CONSOLE_RAM_SIZE,
            Self::Chrome => CHROME_RAM_SIZE,
        }
    }

    pub fn ram_pages(self) -> usize {
        (self.ram_size() / 4096) as usize
    }

    pub fn initrd_path(self) -> &'static str {
        match self {
            Self::Console | Self::Fixture => INITRD_PATH,
            Self::Chrome => CHROME_INITRD_PATH,
        }
    }

    /// The kernel command line, with the TSC frequency when it could be
    /// measured (see [`tsc_khz`]).
    pub fn cmdline(self) -> alloc::string::String {
        let base = match self {
            Self::Console => CMDLINE,
            Self::Fixture => E2E_CMDLINE,
            Self::Chrome => CHROME_CMDLINE,
        };
        let Some(khz) = tsc_khz() else {
            return base.into();
        };
        // Kernel parameters go before ` -- `; what follows it belongs to init.
        let (kernel, init) = base
            .split_once(" -- ")
            .map_or((base, None), |(k, i)| (k, Some(i)));
        let mut line = alloc::format!("{kernel} tsc_early_khz={khz} tsc=reliable");
        if let Some(init) = init {
            line.push_str(" -- ");
            line.push_str(init);
        }
        line
    }
}

/// Host TSC frequency in kHz, measured against the kernel clock.
///
/// The guest fails to calibrate its TSC (`tsc: Fast TSC calibration failed`)
/// and falls back to jiffies, which advance only when a PIT tick is injected
/// at an exit boundary; a guest blocked on slow device I/O loses almost all
/// its time (its `sleep 2` took minutes). With the frequency on the command
/// line and the TSC marked reliable, guest time follows the real clock. RDTSC
/// does not exit and the TSC offset is 0, so the guest reads this counter
/// ([`ostd::vmm::read_tsc`]).
fn tsc_khz() -> Option<u64> {
    const WINDOW_NS: u64 = 20_000_000;
    let rdtsc = ostd::vmm::read_tsc;
    let start_ns = ostd::syscall::sys_get_time();
    let start_tsc = rdtsc();
    if start_ns == 0 {
        return None;
    }
    let mut now_ns = start_ns;
    while now_ns.wrapping_sub(start_ns) < WINDOW_NS {
        ostd::syscall::sys_yield();
        now_ns = ostd::syscall::sys_get_time();
    }
    let cycles = rdtsc().wrapping_sub(start_tsc);
    let khz = u128::from(cycles) * 1_000_000 / u128::from(now_ns - start_ns);
    let khz = u64::try_from(khz).ok().filter(|khz| *khz > 0)?;
    println(&alloc::format!("[hv-x86] guest TSC: {khz} kHz"));
    Some(khz)
}

fn fixture_present() -> bool {
    let Ok(cap) = ostd::syscall::sys_open_cap("/virtio-e2e") else {
        return false;
    };
    ostd::syscall::sys_close_cap(cap);
    true
}
