//! `Transport` wrapper that keeps the PCI transport's initialisation failure
//! observable after the virtio driver has taken ownership of it.
//!
//! `VirtioPciTransport` records failures it detects inside trait methods that
//! cannot return errors (missing `VERSION_1`, features rejected, unreachable
//! notify address) and sets the device's `FAILED` bit, but `VirtIONet::new`
//! still succeeds. Every mutating call here copies the first recorded failure
//! into [`FAILURE`], which the bring-up reads once the constructor returns.

use ostd::sync::Mutex;
use virtio_drivers::transport::{DeviceStatus, DeviceType, Transport};
use virtio_drivers::{PhysAddr, Result};
use virtio_pci_transport::{InitFailure, VirtioPciTransport};

/// First failure recorded since the latest [`CheckedTransport::new`].
static FAILURE: Mutex<Option<InitFailure>> = Mutex::new(None);

/// Delegates to [`VirtioPciTransport`] and mirrors its failures into [`FAILURE`].
pub struct CheckedTransport(VirtioPciTransport);

impl CheckedTransport {
    /// Wraps `inner`, forgetting any failure an earlier transport recorded.
    pub fn new(inner: VirtioPciTransport) -> Self {
        *FAILURE.lock() = None;
        Self(inner)
    }

    fn record(&self) {
        if let Some(failure) = self.0.init_failure() {
            FAILURE.lock().get_or_insert(failure);
        }
    }
}

/// Takes the failure recorded since the latest [`CheckedTransport::new`].
pub fn take_failure() -> Option<InitFailure> {
    FAILURE.lock().take()
}

impl Transport for CheckedTransport {
    fn device_type(&self) -> DeviceType {
        self.0.device_type()
    }

    fn read_device_features(&mut self) -> u64 {
        self.0.read_device_features()
    }

    fn write_driver_features(&mut self, driver_features: u64) {
        self.0.write_driver_features(driver_features);
        self.record();
    }

    fn max_queue_size(&mut self, queue: u16) -> u32 {
        self.0.max_queue_size(queue)
    }

    fn notify(&mut self, queue: u16) {
        self.0.notify(queue);
    }

    fn get_status(&self) -> DeviceStatus {
        self.0.get_status()
    }

    fn set_status(&mut self, status: DeviceStatus) {
        self.0.set_status(status);
        self.record();
    }

    fn set_guest_page_size(&mut self, guest_page_size: u32) {
        self.0.set_guest_page_size(guest_page_size);
    }

    fn requires_legacy_layout(&self) -> bool {
        self.0.requires_legacy_layout()
    }

    fn queue_set(
        &mut self,
        queue: u16,
        size: u32,
        descriptors: PhysAddr,
        driver_area: PhysAddr,
        device_area: PhysAddr,
    ) {
        self.0
            .queue_set(queue, size, descriptors, driver_area, device_area);
        self.record();
    }

    fn queue_unset(&mut self, queue: u16) {
        self.0.queue_unset(queue);
    }

    fn queue_used(&mut self, queue: u16) -> bool {
        self.0.queue_used(queue)
    }

    fn ack_interrupt(&mut self) -> bool {
        self.0.ack_interrupt()
    }

    fn config_space<T: 'static>(&self) -> Result<core::ptr::NonNull<T>> {
        self.0.config_space()
    }
}
