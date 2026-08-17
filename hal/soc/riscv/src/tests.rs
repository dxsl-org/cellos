use crate::{RtcAccessPolicy, UartAccessPolicy, VirtioMmioPolicy, GENERIC_VIRT, JH7110, SG2042};

#[test]
fn generic_and_jh7110_keep_mmio_discovery_enabled() {
    for profile in [GENERIC_VIRT, JH7110] {
        assert_eq!(profile.uart_access, UartAccessPolicy::Mmio);
        assert_eq!(profile.rtc_access, RtcAccessPolicy::Mmio);
        assert_eq!(profile.virtio_mmio, VirtioMmioPolicy::Discover);
        assert!(profile.allows_uart_mmio());
        assert!(profile.allows_rtc_mmio());
        assert!(profile.discovers_virtio_mmio());
    }
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
