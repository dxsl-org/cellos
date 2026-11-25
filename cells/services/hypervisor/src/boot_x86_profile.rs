//! Profile, memory sizing, and kernel command-line selection for x86 PVH boot.

use ostd::io::println;

#[cfg(all(not(feature = "ubuntu-wide-guest"), not(feature = "alpine-wide-guest")))]
const CONSOLE_RAM_SIZE: u64 = 128 * 1024 * 1024;
#[cfg(all(not(feature = "ubuntu-wide-guest"), feature = "alpine-wide-guest"))]
const CONSOLE_RAM_SIZE: u64 = 256 * 1024 * 1024;
#[cfg(feature = "ubuntu-wide-guest")]
const CONSOLE_RAM_SIZE: u64 = 512 * 1024 * 1024;
/// Chromium's guest sizes, largest first; the guest kernel reports about 96 %
/// of a size as usable. Each is one contiguous host run, and `boot_x86::boot`
/// takes the first the kernel can carve.
const CHROME_RAM_SIZES: [u64; 3] = [3072 << 20, 2048 << 20, 1536 << 20];
const CONSOLE_RAM_SIZES: [u64; 1] = [CONSOLE_RAM_SIZE];
/// An image file that asks for more than one vCPU for the Chrome guest: one
/// decimal number. The shipped image has none, so the guest gets one vCPU:
/// with four, parallel JavaScript ran about twice as fast but scrolling and
/// painting lost 15-45 % of their frames (plan 261125-1100 phase 05, bench).
/// The Chrome lane adds it with 4.
const CHROME_VCPUS_PATH: &str = "/chrome-guest-vcpus";
/// The most vCPUs the Chrome guest gets, whatever the file says.
const CHROME_MAX_VCPUS: u8 = 4;

pub const VMLINUX_PATH: &str = "/vmlinux";
const INITRD_PATH: &str = "/initrd.gz";
/// Alpine's initramfs plus `scripts/chrome-guest/stage1.sh`.
const CHROME_INITRD_PATH: &str = "/chrome-initrd.gz";
/// The read-only Chromium root on the CellOS SD volume
/// (`scripts/build-chrome-guest-image.sh`).
pub const CHROME_DISK_NAME: &str = "chrome-guest.sqfs";

// Slots 3 (virtio-gpu) and 4 (virtio-input) share ISA IRQ 3 (`x86_irq_dispatch::DISPLAY_IRQ`),
// and so do slot 2 (the clipboard virtio-console) and slot 5 (the Downloads virtio-9p), which
// only the Chrome guest is told of.
#[cfg(not(feature = "ubuntu-wide-guest"))]
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/sh panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3 -- -i";
const E2E_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/virtio-e2e-init panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3";
// `loglevel=4`: every console byte is two port exits to this Cell (line status,
// then data). Measured during Chromium's start: 32.5k of 60k `RunVcpu` returns
// were the UART, almost all of it the kernel's boot log. The guest init's
// `CHROME_GUEST_*` markers do not go through printk and stay.
const CHROME_CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 loglevel=4 nox2apic pci=off lpj=4000000 nohz=off highres=off rdinit=/bin/chrome-guest-stage1 panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000400:3 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3 virtio_mmio.device=512@0xd0000a00:3";
#[cfg(feature = "ubuntu-wide-guest")]
const CMDLINE: &str = "earlycon=uart8250,io,0x3f8,115200 console=ttyS0 nox2apic pci=off lpj=4000000 nohz=off highres=off root=/dev/vda rw rootfstype=ext4 rootwait init=/sbin/init systemd.unit=multi-user.target net.ifnames=0 panic=1 virtio_mmio.device=512@0xd0000000:5 virtio_mmio.device=512@0xd0000200:6 virtio_mmio.device=512@0xd0000600:3 virtio_mmio.device=512@0xd0000800:3";

/// Keeps a guest on a plain periodic tick at jiffy resolution: what a guest
/// whose only clock event is the PIT in this Cell can afford, because every
/// reprogramming of a one-shot PIT is three port exits.
const PERIODIC_TICK_ONLY: &str = " nohz=off highres=off";

/// Whether the kernel gives a guest a local APIC whose timer fires: its VMX
/// backend models one (TSC-deadline mode, in the kernel), its SVM backend
/// leaves a plain page where the registers are. The backend follows the CPU's
/// vendor. On an Intel CPU without the VMX-preemption timer the model has no
/// deadline mode; the guest then drives the PIT one-shot, which works and
/// costs the port exits.
fn guest_has_apic_timer() -> bool {
    let vendor = core::arch::x86_64::__cpuid(0);
    // "GenuineIntel", as EBX, EDX, ECX.
    (vendor.ebx, vendor.edx, vendor.ecx) == (0x756e_6547, 0x4965_6e69, 0x6c65_746e)
}

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
            profile.ram_sizes()[0] >> 20
        ));
        profile
    }

    /// Guest RAM sizes to try, largest first. Never empty.
    pub fn ram_sizes(self) -> &'static [u64] {
        match self {
            Self::Console | Self::Fixture => &CONSOLE_RAM_SIZES,
            Self::Chrome => &CHROME_RAM_SIZES,
        }
    }

    /// vCPUs to ask for. The Chrome guest gets what [`CHROME_VCPUS_PATH`] asks
    /// for (one without it), at most [`CHROME_MAX_VCPUS`] and one per two host
    /// CPUs: the other half stays for this Cell's devices, the compositor and
    /// the desktop (a vCPU holds its CPU while it runs, idle or not). Every
    /// other guest gets one.
    pub fn vcpus(self) -> u8 {
        match self {
            Self::Console | Self::Fixture => 1,
            Self::Chrome => {
                let half = (ostd::syscall::sys_get_cpu_count() / 2).min(255) as u8;
                chrome_vcpus_asked().min(CHROME_MAX_VCPUS).min(half).max(1)
            }
        }
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
        // The Chrome guest takes one-shot, high-resolution clock events from
        // its local APIC where the kernel gives it one with a timer.
        let base = if matches!(self, Self::Chrome) && guest_has_apic_timer() {
            base.replace(PERIODIC_TICK_ONLY, "")
        } else {
            alloc::string::String::from(base)
        };
        let base = base.as_str();
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

/// The number in [`CHROME_VCPUS_PATH`]; 1 when the file is absent or holds no
/// number.
fn chrome_vcpus_asked() -> u8 {
    let Ok(cap) = ostd::syscall::sys_open_cap(CHROME_VCPUS_PATH) else {
        return 1;
    };
    let mut text = [0u8; 8];
    let read = ostd::syscall::sys_read_cap(cap, &mut text);
    ostd::syscall::sys_close_cap(cap);
    read.ok()
        .and_then(|len| core::str::from_utf8(&text[..len]).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(1)
}
