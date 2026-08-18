use crate::BCM2837;

#[test]
fn bcm2837_controller_addresses_match_peripheral_offsets() {
    let mmio = BCM2837.mmio;

    assert_eq!(BCM2837.slug, "bcm2837");
    assert_eq!(mmio.gpio_base, mmio.peripheral_base + 0x20_0000);
    assert_eq!(mmio.aux_base, mmio.peripheral_base + 0x21_5000);
    assert_eq!(mmio.mini_uart_io, mmio.aux_base + 0x40);
    assert_eq!(mmio.sdhci_base, mmio.peripheral_base + 0x30_0000);
}

#[test]
fn bcm2837_exposes_arasan_word_access_policy() {
    assert!(BCM2837.sdhci.word_access_only);
    assert_eq!(BCM2837.sdhci.minimum_write_spacing_us, 6);
}
