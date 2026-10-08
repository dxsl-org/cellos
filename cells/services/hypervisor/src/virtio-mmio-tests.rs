use super::{QueueCfg, VirtioDevice, VirtioMmio};

#[derive(Default)]
struct DummyDevice {
    notifications: usize,
    resets: usize,
    queue_cursor: usize,
    last_queue: Option<usize>,
    pub publish_used: bool,
}
impl VirtioDevice for DummyDevice {
    fn device_id(&self) -> u32 {
        0
    }

    fn notify(&mut self, queue: usize, _: &QueueCfg, _: usize, _: usize) -> bool {
        self.notifications += 1;
        self.queue_cursor += 1;
        self.last_queue = Some(queue);
        self.publish_used
    }
    fn reset(&mut self) {
        self.resets += 1;
        self.queue_cursor = 0;
        self.last_queue = None;
    }
}

fn write(mmio: &mut VirtioMmio, device: &mut DummyDevice, offset: u64, value: u32) {
    mmio.mmio_write(offset, value, device, 7, 3);
}

fn set_queue_config(
    mmio: &mut VirtioMmio,
    device: &mut DummyDevice,
    size: u32,
    desc: u64,
    avail: u64,
    used: u64,
) {
    write(mmio, device, 0x038, size);
    write(mmio, device, 0x080, desc as u32);
    write(mmio, device, 0x084, (desc >> 32) as u32);
    write(mmio, device, 0x090, avail as u32);
    write(mmio, device, 0x094, (avail >> 32) as u32);
    write(mmio, device, 0x0a0, used as u32);
    write(mmio, device, 0x0a4, (used >> 32) as u32);
}

fn configure_valid_queue(mmio: &mut VirtioMmio, device: &mut DummyDevice, queue: u32) {
    write(mmio, device, 0x030, queue);
    let offset = queue as u64 * 0x1000;
    set_queue_config(
        mmio,
        device,
        8,
        0x1000 + offset,
        0x2000 + offset,
        0x3000 + offset,
    );
    write(mmio, device, 0x044, 1);
}

fn rejects_config(size: u32, desc: u64, avail: u64, used: u64) {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    set_queue_config(&mut mmio, &mut device, size, desc, avail, used);
    write(&mut mmio, &mut device, 0x044, 1);
    assert!(!mmio.queue_cfg(0).ready);
}

#[test]
fn pending_used_interrupt_retries_until_acknowledged() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();

    mmio.signal_used();
    assert!(mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 1);

    write(&mut mmio, &mut device, 0x064, 2);
    assert!(mmio.interrupt_pending());

    write(&mut mmio, &mut device, 0x064, 1);
    assert!(!mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 0);
}

#[test]
fn invalid_queue_selection_cannot_mutate_the_previous_queue() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    write(&mut mmio, &mut device, 0x038, 8);

    write(&mut mmio, &mut device, 0x030, 2);
    write(&mut mmio, &mut device, 0x038, 16);
    write(&mut mmio, &mut device, 0x044, 1);

    assert_eq!(mmio.mmio_read(0x034, &device), 0);
    assert_eq!(mmio.mmio_read(0x038, &device), 0);
    assert_eq!(mmio.queue_cfg(0).num, 8);
    assert_eq!(mmio.queue_cfg(1).num, 0);

    write(&mut mmio, &mut device, 0x030, 0);
    assert_eq!(mmio.mmio_read(0x038, &device), 8);
}

#[test]
fn queue_ready_accepts_only_complete_bounded_aligned_config() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    configure_valid_queue(&mut mmio, &mut device, 0);
    assert!(mmio.queue_cfg(0).ready);

    write(&mut mmio, &mut device, 0x080, 0x1008);
    assert!(!mmio.queue_cfg(0).ready);
    write(&mut mmio, &mut device, 0x044, 1);
    assert!(!mmio.queue_cfg(0).ready);

    rejects_config(0, 0x1000, 0x2000, 0x3000);
    rejects_config(3, 0x1000, 0x2000, 0x3000);
    rejects_config(512, 0x1000, 0x2000, 0x3000);
    rejects_config(8, 0, 0x2000, 0x3000);
    rejects_config(8, 0x1008, 0x2000, 0x3000);
    rejects_config(8, 0x1000, 0x2001, 0x3000);
    rejects_config(8, 0x1000, 0x2000, 0x3002);
    rejects_config(1, u64::MAX - 15, 0x2000, 0x3000);
}

#[test]
fn malformed_ready_value_revokes_a_ready_queue() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    configure_valid_queue(&mut mmio, &mut device, 0);

    write(&mut mmio, &mut device, 0x044, 2);

    assert!(!mmio.queue_cfg(0).ready);
}

#[test]
fn notifications_require_driver_ok_and_a_valid_ready_queue() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    device.publish_used = true;
    configure_valid_queue(&mut mmio, &mut device, 1);

    write(&mut mmio, &mut device, 0x050, 1);
    assert_eq!(device.notifications, 0);
    assert!(!mmio.interrupt_pending());

    write(&mut mmio, &mut device, 0x070, 0x04);
    write(&mut mmio, &mut device, 0x050, 0);
    assert_eq!(device.notifications, 0);
    assert!(!mmio.interrupt_pending());
    write(&mut mmio, &mut device, 0x050, 1);
    assert_eq!(device.notifications, 1);
    assert_eq!(device.last_queue, Some(1));
    assert!(mmio.interrupt_pending());

    write(&mut mmio, &mut device, 0x064, 1);
    assert!(!mmio.interrupt_pending());

    device.publish_used = false;
    write(&mut mmio, &mut device, 0x050, 1);
    assert_eq!(device.notifications, 2);
    assert!(!mmio.interrupt_pending());

    write(&mut mmio, &mut device, 0x030, 1);
    write(&mut mmio, &mut device, 0x038, 0);
    write(&mut mmio, &mut device, 0x050, 1);
    assert_eq!(device.notifications, 2);
}
#[test]
fn needs_reset_status_is_latched_and_gates_notifications() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    configure_valid_queue(&mut mmio, &mut device, 0);

    write(&mut mmio, &mut device, 0x070, 0x08);
    assert_eq!(mmio.mmio_read(0x070, &device), 0x40);
    write(&mut mmio, &mut device, 0x070, 0x04);
    assert_eq!(mmio.mmio_read(0x070, &device), 0x40);
    write(&mut mmio, &mut device, 0x050, 0);
    assert_eq!(device.notifications, 0);

    write(&mut mmio, &mut device, 0x070, 0);
    configure_valid_queue(&mut mmio, &mut device, 0);
    write(&mut mmio, &mut device, 0x070, 0x84);
    assert_eq!(mmio.mmio_read(0x070, &device), 0xc4);
    write(&mut mmio, &mut device, 0x050, 0);
    assert_eq!(device.notifications, 0);
}

#[test]
fn reset_clears_transport_and_device_owned_queue_state() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    configure_valid_queue(&mut mmio, &mut device, 1);
    write(&mut mmio, &mut device, 0x070, 0x04);
    write(&mut mmio, &mut device, 0x050, 1);
    assert_eq!(device.queue_cursor, 1);

    write(&mut mmio, &mut device, 0x070, 0);

    assert_eq!(device.resets, 1);
    assert_eq!(device.queue_cursor, 0);
    assert_eq!(device.last_queue, None);
    assert_eq!(mmio.mmio_read(0x070, &device), 0);
    assert_eq!(mmio.mmio_read(0x060, &device), 0);
    assert_eq!(mmio.queue_cfg(0).num, 0);
    assert_eq!(mmio.queue_cfg(1).num, 0);
}

#[test]
fn notify_without_published_used_entries_does_not_signal_interrupt() {
    let mut mmio = VirtioMmio::default();
    let mut device = DummyDevice::default();
    configure_valid_queue(&mut mmio, &mut device, 0);
    write(&mut mmio, &mut device, 0x070, 0x04);
    device.publish_used = false;

    write(&mut mmio, &mut device, 0x050, 0);
    assert_eq!(device.notifications, 1);
    assert!(!mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 0);

    device.publish_used = true;
    write(&mut mmio, &mut device, 0x050, 0);
    assert_eq!(device.notifications, 2);
    assert!(mmio.interrupt_pending());
    assert_eq!(mmio.mmio_read(0x060, &device), 1);
}

/// Device whose config space answers one aligned 32-bit word per offset, which is
/// the contract `config_read` documents.
#[derive(Default)]
struct ConfigWordDevice([u32; 2]);

impl VirtioDevice for ConfigWordDevice {
    fn device_id(&self) -> u32 {
        1
    }
    fn notify(&mut self, _: usize, _: &QueueCfg, _: usize, _: usize) -> bool {
        false
    }
    fn config_read(&self, offset: usize) -> u32 {
        match offset {
            0 => self.0[0],
            4 => self.0[1],
            _ => 0,
        }
    }
}

/// The guest reads a multi-byte config field with **byte** accesses (`memcpy_fromio`),
/// so the transport has to serve a byte offset inside a word. The ARM guest's `eth0`
/// came up as `52:00:00:00:BB:00` instead of the advertised `52:54:00:AA:BB:CC`
/// because only offsets 0 and 4 answered, and the L2 bridge then routed the guest's
/// replies to an address it never used.
#[test]
fn config_space_answers_byte_offsets_inside_a_word() {
    let mmio = VirtioMmio::default();
    let device = ConfigWordDevice([0xAA00_5452, 0x1234_CCBB]);

    for (byte, expected) in [
        (0u64, 0x52u64),
        (1, 0x54),
        (2, 0x00),
        (3, 0xAA),
        (4, 0xBB),
        (5, 0xCC),
    ] {
        assert_eq!(
            mmio.mmio_read(0x100 + byte, &device) & 0xff,
            expected,
            "byte {byte} of the config space"
        );
    }

    // A word access still returns the whole word, and a 16-bit field at offset 6
    // (virtio-net `status`) comes out of the second word.
    assert_eq!(mmio.mmio_read(0x100, &device) & 0xffff_ffff, 0xAA00_5452);
    assert_eq!(mmio.mmio_read(0x104, &device) & 0xffff_ffff, 0x1234_CCBB);
    assert_eq!(mmio.mmio_read(0x106, &device) & 0xffff, 0x1234);
}
