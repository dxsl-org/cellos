use crate::{
    PlicContextPolicy, RiscvMmioRegion, RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy,
    GENERIC_VIRT, JH7110, SG2042,
};

#[test]
fn generic_and_jh7110_keep_supported_discovery_enabled() {
    assert_eq!(GENERIC_VIRT.rtc_access, RtcAccessPolicy::Mmio);
    assert!(GENERIC_VIRT.allows_rtc_mmio());

    for profile in [GENERIC_VIRT, JH7110] {
        assert_eq!(profile.uart_access, UartAccessPolicy::Mmio);
        assert_eq!(profile.virtio_mmio, VirtioMmioPolicy::Discover);
        assert!(profile.allows_uart_mmio());
        assert!(profile.discovers_virtio_mmio());
    }

    assert_eq!(JH7110.rtc_access, RtcAccessPolicy::Unavailable);
    assert!(!JH7110.allows_rtc_mmio());
    assert_eq!(JH7110.sdhci.expect("JH7110 SDHCI").base, 0x1604_0000);
}

#[test]
fn profiles_map_physical_harts_to_their_checked_plic_contexts() {
    for profile in [GENERIC_VIRT, SG2042] {
        assert_eq!(
            profile.plic_context,
            PlicContextPolicy::machine_then_supervisor()
        );
        assert_eq!(profile.plic_context_for_physical_hart(0), Some(1));
        assert_eq!(profile.plic_context_for_physical_hart(1), Some(3));
        assert_eq!(profile.plic_context_for_physical_hart(usize::MAX), None);
    }

    assert_eq!(JH7110.plic_context, PlicContextPolicy::jh7110());
    assert_eq!(JH7110.plic_context_for_physical_hart(0), None);
    assert_eq!(JH7110.plic_context_for_physical_hart(1), Some(2));
    assert_eq!(JH7110.plic_context_for_physical_hart(2), Some(4));
    assert_eq!(JH7110.plic_context_for_physical_hart(usize::MAX), None);
}

#[test]
fn sg2042_disables_mmio_uart_rtc_and_virtio() {
    assert!(SG2042.plic_compatibles.contains(&"thead,c900-plic"));
    assert!(SG2042.clint_compatibles.contains(&"thead,c900-clint"));
    assert_eq!(SG2042.uart_access, UartAccessPolicy::SbiDbcnOnly);
    assert_eq!(SG2042.rtc_access, RtcAccessPolicy::Unavailable);
    assert_eq!(SG2042.virtio_mmio, VirtioMmioPolicy::Absent);
    assert!(!SG2042.allows_uart_mmio());
    assert!(!SG2042.allows_rtc_mmio());
    assert!(!SG2042.discovers_virtio_mmio());
}

#[test]
fn fallback_mmio_is_soc_owned_and_structurally_valid() {
    for profile in [GENERIC_VIRT, JH7110, SG2042] {
        assert!(profile.fallback_mmio.is_valid(), "{}", profile.slug);
        assert_eq!(profile.fallback_mmio.plic.base, 0x0C00_0000);
        assert_eq!(profile.fallback_mmio.clint.base, 0x0200_0000);
    }

    let generic = GENERIC_VIRT.fallback_mmio;
    assert_eq!(generic.uart.expect("generic UART").base, 0x1000_0000);
    assert_eq!(generic.uart.expect("generic UART").irq, Some(10));
    assert_eq!(generic.rtc.expect("generic RTC").base, 0x0010_1000);
    assert_eq!(generic.virtio.len(), 5);
    assert_eq!(generic.virtio[4].irq, Some(5));

    assert!(JH7110.fallback_mmio.rtc.is_none());
    assert!(JH7110.fallback_mmio.virtio.is_empty());
    assert_eq!(
        SG2042.fallback_mmio.uart.expect("Pioneer UART").base,
        0x70_4000_0000
    );
}

#[test]
fn mmio_region_rejects_zero_and_overflow() {
    assert!(!RiscvMmioRegion {
        base: 0,
        size: 0x1000,
        irq: None,
    }
    .is_valid());
    assert!(!RiscvMmioRegion {
        base: usize::MAX,
        size: 2,
        irq: None,
    }
    .is_valid());
}
