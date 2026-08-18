use crate::SdhciAccessPolicy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bcm2711MmioLayout {
    pub peripheral_base: usize,
    pub peripheral_size: usize,
    pub system_timer_base: usize,
    pub gpio_base: usize,
    pub gpio_grant_size: usize,
    pub uart_base: usize,
    pub uart_grant_size: usize,
    pub sdhci_base: usize,
    pub sdhci_grant_size: usize,
    pub gic_distributor_base: usize,
    pub gic_distributor_size: usize,
    pub gic_cpu_base: usize,
    pub gic_cpu_size: usize,
}

impl Bcm2711MmioLayout {
    pub const fn peripheral_end(self) -> Option<usize> {
        self.peripheral_base.checked_add(self.peripheral_size)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bcm2711SocProfile {
    pub slug: &'static str,
    pub mmio: Bcm2711MmioLayout,
    pub sdhci: SdhciAccessPolicy,
}

pub const BCM2711: Bcm2711SocProfile = Bcm2711SocProfile {
    slug: "bcm2711",
    mmio: Bcm2711MmioLayout {
        peripheral_base: 0xFE00_0000,
        peripheral_size: 0x0200_0000,
        system_timer_base: 0xFE00_3000,
        gpio_base: 0xFE20_0000,
        gpio_grant_size: 0x1000,
        uart_base: 0xFE20_1000,
        uart_grant_size: 0x1000,
        sdhci_base: 0xFE34_0000,
        sdhci_grant_size: 0x1000,
        gic_distributor_base: 0xFF84_1000,
        gic_distributor_size: 0x1000,
        gic_cpu_base: 0xFF84_2000,
        gic_cpu_size: 0x1000,
    },
    sdhci: SdhciAccessPolicy {
        word_access_only: false,
        minimum_write_spacing_us: 0,
    },
};
