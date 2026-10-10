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

/// Observe the hardware processor executing this instruction, without a syscall.
///
/// On x86 this is the topology x2APIC ID (CPUID leaf 0xb), or the legacy
/// local APIC ID when topology enumeration is unavailable. It is an endpoint
/// observation: a task can migrate immediately afterward. Other architectures
/// return `None`; no scheduler CPU index or synthetic ID is substituted.
pub fn processor_id() -> Option<u32> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        #[cfg(target_arch = "x86")]
        use core::arch::x86::{__cpuid, __cpuid_count};
        #[cfg(target_arch = "x86_64")]
        use core::arch::x86_64::{__cpuid, __cpuid_count};

        let maximum = __cpuid(0).eax;
        if maximum >= 0xb {
            let topology = __cpuid_count(0xb, 0);
            if topology.ebx & 0xffff != 0 && (topology.ecx >> 8) & 0xff != 0 {
                return Some(topology.edx);
            }
        }
        if maximum >= 1 {
            let processor = __cpuid(1);
            if processor.edx & (1 << 9) != 0 {
                return Some(processor.ebx >> 24);
            }
        }
        None
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        None
    }
}
