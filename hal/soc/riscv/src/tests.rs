use crate::{
    PlicContextPolicy, RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy, GENERIC_VIRT, JH7110,
    SG2042,
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
