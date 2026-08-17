//! User-visible operating-system metadata shared by Cell tools.

/// Operating-system name exposed by shell and system-information tools.
pub const OS_NAME: &str = "Cellos";

/// User-visible kernel artifact name.
pub const KERNEL_NAME: &str = "cellos-kernel";

/// Current kernel release.
pub const KERNEL_VERSION: &str = "0.2.1";

#[cfg(target_arch = "aarch64")]
pub const ARCH: &str = "aarch64";
#[cfg(target_arch = "arm")]
pub const ARCH: &str = "arm";
#[cfg(target_arch = "riscv32")]
pub const ARCH: &str = "riscv32";
#[cfg(target_arch = "riscv64")]
pub const ARCH: &str = "riscv64";
#[cfg(target_arch = "x86")]
pub const ARCH: &str = "x86";
#[cfg(target_arch = "x86_64")]
pub const ARCH: &str = "x86_64";
#[cfg(not(any(
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "x86",
    target_arch = "x86_64",
)))]
pub const ARCH: &str = "unknown";
