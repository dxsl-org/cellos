use crate::{
    PlicContextPolicy, RiscvSocProfile, RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy,
};

const UART_COMPATIBLES: &[&str] = &["ns16550a", "ns16550", "snps,dw-apb-uart"];
const PLIC_COMPATIBLES: &[&str] = &["sifive,plic-1.0.0", "riscv,plic0", "thead,c900-plic"];
const CLINT_COMPATIBLES: &[&str] = &["sifive,clint0", "riscv,clint0", "thead,c900-clint"];
const RTC_COMPATIBLES: &[&str] = &["google,goldfish-rtc"];

/// Generic QEMU `virt`-style baseline with DTB-driven MMIO discovery enabled.
pub const GENERIC_VIRT: RiscvSocProfile = RiscvSocProfile {
    slug: "generic-virt",
    uart_compatibles: UART_COMPATIBLES,
    plic_compatibles: PLIC_COMPATIBLES,
    clint_compatibles: CLINT_COMPATIBLES,
    rtc_compatibles: RTC_COMPATIBLES,
    plic_context: PlicContextPolicy::machine_then_supervisor(),
    uart_access: UartAccessPolicy::Mmio,
    rtc_access: RtcAccessPolicy::Mmio,
    virtio_mmio: VirtioMmioPolicy::Discover,
};

/// JH7110 currently reuses the same DTB lookup families as the generic path.
pub const JH7110: RiscvSocProfile = RiscvSocProfile {
    slug: "jh7110",
    plic_context: PlicContextPolicy::jh7110(),
    ..GENERIC_VIRT
};

/// SG2042 keeps interrupt-controller compat probing but disables unsupported
/// MMIO paths so the kernel remains on SBI DBCN and empty VirtIO slots.
pub const SG2042: RiscvSocProfile = RiscvSocProfile {
    slug: "sg2042",
    uart_compatibles: UART_COMPATIBLES,
    plic_compatibles: PLIC_COMPATIBLES,
    clint_compatibles: CLINT_COMPATIBLES,
    rtc_compatibles: RTC_COMPATIBLES,
    plic_context: PlicContextPolicy::machine_then_supervisor(),
    uart_access: UartAccessPolicy::SbiDbcnOnly,
    rtc_access: RtcAccessPolicy::Unavailable,
    virtio_mmio: VirtioMmioPolicy::Absent,
};
