use crate::{BCM2711, BCM2837};

#[test]
fn bcm2837_controller_addresses_match_peripheral_offsets() {
    let mmio = BCM2837.mmio;

    assert_eq!(BCM2837.slug, "bcm2837");
    assert_eq!(mmio.system_timer_base, mmio.peripheral_base + 0x3000);
    assert_eq!(mmio.legacy_irq_base, mmio.peripheral_base + 0xB200);
    assert_eq!(mmio.mailbox_base, mmio.peripheral_base + 0xB880);
    assert_eq!(mmio.gpio_base, mmio.peripheral_base + 0x20_0000);
    assert_eq!(mmio.spi0_base, mmio.peripheral_base + 0x20_4000);
    assert_eq!(mmio.aux_base, mmio.peripheral_base + 0x21_5000);
    assert_eq!(mmio.mini_uart_io, mmio.aux_base + 0x40);
    assert_eq!(mmio.sdhci_base, mmio.peripheral_base + 0x30_0000);
    assert_eq!(mmio.bsc1_base, mmio.peripheral_base + 0x80_4000);
    assert_eq!(mmio.dwc2_base, mmio.peripheral_base + 0x98_0000);
}

#[test]
fn bcm2837_mapping_and_grant_spans_are_bounded() {
    let mmio = BCM2837.mmio;
    let peripheral_end = mmio.peripheral_end().expect("peripheral span overflow");

    assert_eq!(peripheral_end, 0x4000_0000);
    assert_eq!(mmio.local_controller_base, peripheral_end);
    assert_eq!(mmio.local_controller_end(), Some(0x4000_1000));
    assert_eq!(mmio.gpio_grant_size, 0x1000);
    assert_eq!(mmio.mailbox_grant_size, 0x40);
    assert_eq!(mmio.bsc1_grant_size, 0x1000);
    assert_eq!(mmio.spi0_grant_size, 0x1000);
    assert_eq!(mmio.aux_grant_size, 0x1000);
    assert_eq!(mmio.dwc2_grant_size, 0x20000);
    assert!(mmio.legacy_irq_base < peripheral_end);
    assert!(mmio.mailbox_base < peripheral_end);
    assert!(mmio.legacy_irq_base < peripheral_end);
    assert!(mmio.gpio_base + mmio.gpio_grant_size <= peripheral_end);
    assert!(mmio.bsc1_base + mmio.bsc1_grant_size <= peripheral_end);
    assert!(mmio.spi0_base + mmio.spi0_grant_size <= peripheral_end);
    assert!(mmio.aux_base + mmio.aux_grant_size <= peripheral_end);
    assert!(mmio.gpio_base + mmio.gpio_grant_size <= mmio.spi0_base);
    assert!(mmio.spi0_base + mmio.spi0_grant_size <= mmio.aux_base);
}

/// The kernel USER-maps only whole pages inside a grant (H-18): every page
/// window must be page-aligned, and the one sub-page grant — the mailbox —
/// must stop short of the legacy IRQ controller and ARM timer it shares a
/// page with, because the kernel performs its accesses byte-exactly.
#[test]
fn bcm2837_grants_are_page_windows_except_the_mediated_mailbox() {
    const PAGE: usize = 0x1000;
    let mmio = BCM2837.mmio;
    for (base, size) in [
        (mmio.gpio_base, mmio.gpio_grant_size),
        (mmio.bsc1_base, mmio.bsc1_grant_size),
        (mmio.spi0_base, mmio.spi0_grant_size),
        (mmio.aux_base, mmio.aux_grant_size),
        (mmio.dwc2_base, mmio.dwc2_grant_size),
    ] {
        assert_eq!(base % PAGE, 0, "{base:#x} is not page-aligned");
        assert_eq!(size % PAGE, 0, "{base:#x}+{size:#x} is not whole pages");
    }

    let mailbox_page = mmio.mailbox_base & !(PAGE - 1);
    let mailbox_end = mmio.mailbox_base + mmio.mailbox_grant_size;
    assert_eq!(mailbox_page, mmio.legacy_irq_base & !(PAGE - 1));
    assert!(mmio.mailbox_grant_size < PAGE);
    // IRQ registers end at +0x228, the ARM timer at +0x424; the mailbox
    // window starts after both and stays inside the page.
    assert!(mmio.legacy_irq_base + 0x228 <= mmio.mailbox_base);
    assert!(mmio.peripheral_base + 0xB424 <= mmio.mailbox_base);
    assert!(mailbox_end <= mailbox_page + PAGE);
}

#[test]
fn bcm2837_exposes_arasan_word_access_policy() {
    let word_access_only = core::hint::black_box(BCM2837.sdhci.word_access_only);
    assert!(word_access_only);
    assert_eq!(BCM2837.sdhci.minimum_write_spacing_us, 6);
}

#[test]
fn bcm2837_irq_topology_matches_arm_hal_contract() {
    let irq = BCM2837.irq;

    assert!(irq.is_valid());
    assert_eq!(irq.system_timer_c1, 1);
    assert_eq!(irq.usb, 9);
    assert_eq!(irq.aux, 29);
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

#[test]
fn bcm2711_exposes_bounded_gic_and_emmc2_layout() {
    let mmio = BCM2711.mmio;
    let end = mmio.peripheral_end().expect("BCM2711 aperture overflow");

    assert_eq!(end, 0x1_0000_0000);
    assert_eq!(mmio.gpio_grant_size, 0x1000);
    assert_eq!(mmio.uart_base, mmio.gpio_base + 0x1000);
    assert_eq!(mmio.uart_grant_size, 0x1000);
    assert_eq!(mmio.sdhci_base, 0xFE34_0000);
    assert_eq!(mmio.sdhci_grant_size, 0x1000);
    assert!(mmio.gpio_base + mmio.gpio_grant_size <= end);
    assert!(mmio.sdhci_base + mmio.sdhci_grant_size <= end);
    assert!(mmio.gpio_base + mmio.gpio_grant_size <= mmio.uart_base);
    assert!(mmio.uart_base + mmio.uart_grant_size <= mmio.sdhci_base);
    assert!(mmio.gic_distributor_base < end);
    assert!(mmio.gic_cpu_base < end);
    assert_eq!(mmio.gic_distributor_size, 0x1000);
    assert_eq!(mmio.gic_cpu_size, 0x1000);
    assert!(mmio.gic_distributor_base + mmio.gic_distributor_size <= mmio.gic_cpu_base);
    assert!(mmio.gic_cpu_base + mmio.gic_cpu_size <= end);
    let word_access_only = core::hint::black_box(BCM2711.sdhci.word_access_only);
    assert!(!word_access_only);
    assert_eq!(BCM2711.sdhci.minimum_write_spacing_us, 0);
}
