use crate::SdhciAccessPolicy;

/// Immutable controller layout for one BCM27xx SoC generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bcm27xxMmioLayout {
    pub peripheral_base: usize,
    pub gpio_base: usize,
    pub aux_base: usize,
    pub mini_uart_io: usize,
    pub sdhci_base: usize,
}

/// Data-only SoC facts consumed by board-selected kernel paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Bcm27xxSocProfile {
    pub slug: &'static str,
    pub mmio: Bcm27xxMmioLayout,
    pub sdhci: SdhciAccessPolicy,
}

pub const BCM2837: Bcm27xxSocProfile = Bcm27xxSocProfile {
    slug: "bcm2837",
    mmio: Bcm27xxMmioLayout {
        peripheral_base: 0x3F00_0000,
        gpio_base: 0x3F20_0000,
        aux_base: 0x3F21_5000,
        mini_uart_io: 0x3F21_5040,
        sdhci_base: 0x3F30_0000,
    },
    sdhci: SdhciAccessPolicy {
        word_access_only: true,
        minimum_write_spacing_us: 6,
    },
};
