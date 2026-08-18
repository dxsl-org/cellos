use crate::BCM2837;

#[test]
fn bcm2837_controller_addresses_match_peripheral_offsets() {
    let mmio = BCM2837.mmio;

    assert_eq!(BCM2837.slug, "bcm2837");
    assert_eq!(mmio.system_timer_base, mmio.peripheral_base + 0x3000);
    assert_eq!(mmio.legacy_irq_base, mmio.peripheral_base + 0xB200);
    assert_eq!(mmio.gpio_base, mmio.peripheral_base + 0x20_0000);
    assert_eq!(mmio.aux_base, mmio.peripheral_base + 0x21_5000);
    assert_eq!(mmio.mini_uart_io, mmio.aux_base + 0x40);
    assert_eq!(mmio.sdhci_base, mmio.peripheral_base + 0x30_0000);
}

#[test]
fn bcm2837_mapping_and_grant_spans_are_bounded() {
    let mmio = BCM2837.mmio;
    let peripheral_end = mmio.peripheral_end().expect("peripheral span overflow");

    assert_eq!(peripheral_end, 0x4000_0000);
    assert_eq!(mmio.local_controller_base, peripheral_end);
    assert_eq!(mmio.local_controller_end(), Some(0x4000_1000));
    assert_eq!(mmio.gpio_grant_size, 0x1_0000);
    assert_eq!(mmio.aux_grant_size, 0x1000);
    assert!(mmio.system_timer_base < peripheral_end);
    assert!(mmio.legacy_irq_base < peripheral_end);
    assert!(mmio.gpio_base + mmio.gpio_grant_size <= peripheral_end);
    assert!(mmio.aux_base + mmio.aux_grant_size <= peripheral_end);
}

#[test]
fn bcm2837_exposes_arasan_word_access_policy() {
    assert!(BCM2837.sdhci.word_access_only);
    assert_eq!(BCM2837.sdhci.minimum_write_spacing_us, 6);
}

#[test]
fn bcm2837_irq_topology_matches_arm_hal_contract() {
    let irq = BCM2837.irq;

    assert!(irq.is_valid());
    assert_eq!(irq.system_timer_c1, 1);
    assert_eq!(irq.aux, 29);
    assert_eq!(irq.gpio_bank0, 49);
    assert_eq!(irq.gpio_bank1, 50);
    assert_eq!(irq.local_timer_ns_mask, 1 << 1);
    assert_eq!(irq.local_timer_hp_mask, 1 << 2);
    assert_eq!(irq.local_gpu_mask, 1 << 8);
}

#[test]
fn irq_topology_rejects_invalid_legacy_and_local_routes() {
    let mut irq = BCM2837.irq;
    irq.aux = irq.system_timer_c1;
    assert!(!irq.is_valid());

    irq = BCM2837.irq;
    irq.gpio_bank1 = 64;
    assert!(!irq.is_valid());

    irq = BCM2837.irq;
    irq.local_gpu_mask = irq.local_timer_ns_mask;
    assert!(!irq.is_valid());
}
