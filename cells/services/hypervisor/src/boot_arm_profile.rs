//! Guest profile for the aarch64 guest: memory size, rdinit and kernel command
//! line, chosen at build time.
//!
//! The Pi mirrors the x86 lane's shape (`boot_x86_profile.rs`) instead of
//! inventing a second mechanism: the profile is a build-time feature, it decides
//! how much RAM the Stage-2 carve takes and what the guest runs first.
//!
//! - `alpine` (default): the proven 128 MiB lane — a static or musl binary, a
//!   shell tool, anything whose userspace fits in a small initramfs.
//! - `alpine-wide`: 256 MiB, for a musl userspace with real packages (Python,
//!   Node, a headless browser). The x86 lane measured why: `apk add python3` is
//!   OOM-killed in 128 MiB.
//! - `alpine-gui`: 512 MiB, for a guest that draws a window. The display path
//!   (virtio-gpu surface handed to Cellos' compositor, then to the panel) is not
//!   built yet, so this profile produces a guest that can run a UI toolkit and
//!   cannot show it — a build for the day the presentation path lands, not a
//!   claim that a window appears today.

#[cfg(all(feature = "alpine-wide-guest", feature = "alpine-gui-guest"))]
compile_error!("one guest profile per image: alpine-wide-guest and alpine-gui-guest are exclusive");

#[cfg(all(not(feature = "alpine-wide-guest"), not(feature = "alpine-gui-guest")))]
pub const PROFILE: &str = "alpine";
#[cfg(all(feature = "alpine-wide-guest", not(feature = "alpine-gui-guest")))]
pub const PROFILE: &str = "alpine-wide";
#[cfg(feature = "alpine-gui-guest")]
pub const PROFILE: &str = "alpine-gui";

#[cfg(all(not(feature = "alpine-wide-guest"), not(feature = "alpine-gui-guest")))]
pub const GUEST_RAM_SIZE: u64 = 128 * 1024 * 1024;
#[cfg(all(feature = "alpine-wide-guest", not(feature = "alpine-gui-guest")))]
pub const GUEST_RAM_SIZE: u64 = 256 * 1024 * 1024;
#[cfg(feature = "alpine-gui-guest")]
pub const GUEST_RAM_SIZE: u64 = 512 * 1024 * 1024;

/// Page count for `create_vm`; the carve is one contiguous run, so this is what
/// the host must have free at the moment the guest starts.
pub const GUEST_RAM_PAGES: usize = (GUEST_RAM_SIZE / 4096) as usize;

/// What the guest runs first. The SD profile's init mounts the ext4 guest disk
/// before the shell; the volatile profile's init mounts `/proc`, `/sys` and
/// `/dev` and hands over to the shell (`tools/prepare-rpi3-shell-initramfs.py`
/// writes it into the guest initramfs, and is where app launch will exec).
#[cfg(feature = "volatile-disk")]
pub const RDINIT: &str = "/init";
#[cfg(not(feature = "volatile-disk"))]
pub const RDINIT: &str = "/bin/pi-guest-init";

/// The guest command line.
///
/// Built rather than written out per combination because every printed character
/// traps through Stage-2 MMIO: the release board profile stays quiet (warnings
/// and errors still reach the console), and a future app-launch profile changes
/// `rdinit` here instead of in a table of consts.
#[cfg(feature = "board-rpi3")]
pub fn bootargs() -> alloc::string::String {
    alloc::format!(
        "console=ttyAMA0 earlycon=pl011,0x9000000 rdinit={} panic=1 loglevel=3 quiet",
        RDINIT
    )
}

/// QEMU virt has a real PL011 at the same address but a different console path,
/// and the virt lane keeps the guest chatty for diagnostics.
#[cfg(not(feature = "board-rpi3"))]
pub fn bootargs() -> alloc::string::String {
    alloc::format!(
        "console=hvc0 console=ttyAMA0 earlycon=pl011,0x9000000 rdinit={} panic=1 loglevel=8",
        RDINIT
    )
}
