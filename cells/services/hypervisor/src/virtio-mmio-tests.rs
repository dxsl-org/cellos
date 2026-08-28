use super::{QueueCfg, VirtioDevice, VirtioMmio};

struct DummyDevice;

impl VirtioDevice for DummyDevice {
    fn device_id(&self) -> u32 {
        0
    }

    fn notify(&mut self, _: usize, _: &QueueCfg, _: usize, _: usize) {}
}

#[test]
fn pending_used_interrupt_retries_until_acknowledged() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice;

    mmio.signal_used();
    assert!(mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 1);

    mmio.mmio_write(0x064, 2, &mut device, 0, 0);
    assert!(mmio.interrupt_pending());

    mmio.mmio_write(0x064, 1, &mut device, 0, 0);
    assert!(!mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 0);
}
