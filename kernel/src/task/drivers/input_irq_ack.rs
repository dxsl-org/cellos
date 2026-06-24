//! Minimal VirtIO input IRQ ACK shim.
//!
//! Keeps InterruptStatus cleared when the input Cell is not yet running or has
//! crashed — without this, an unacknowledged input IRQ becomes an interrupt storm.
//!
//! This is the ONLY kernel-side remnant of the old virtio_input driver.
//! All event routing is handled by the input service Cell (cells/services/input).
//! Full deletion of this shim is a G2 task once input Cell reliability is confirmed.

use crate::sync::Spinlock;
use crate::task::drivers::virtio_hal::VirtioHal;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicU32, Ordering};
use virtio_drivers::device::input::VirtIOInput;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::transport::{DeviceType, Transport};

/// IRQ number of the probed VirtIO input device (0 = not found).
static INPUT_IRQ: AtomicU32 = AtomicU32::new(0);

struct InputAckDriver {
    inner: VirtIOInput<VirtioHal, MmioTransport>,
}

static KEYBOARD_ACK: Spinlock<Option<InputAckDriver>> = Spinlock::new(None);

/// Probe VirtIO MMIO slots for an input device and cache it for IRQ ACK.
pub fn init_driver() {
    use crate::task::drivers::virtio_common::virtio_slots;
    for slot in virtio_slots() {
        let header = unsafe { NonNull::new_unchecked(slot.base as *mut VirtIOHeader) };
        match unsafe { MmioTransport::new(header) } {
            Ok(transport) => {
                if transport.device_type() == DeviceType::Input {
                    match VirtIOInput::<VirtioHal, MmioTransport>::new(transport) {
                        Ok(inner) => {
                            log::info!("[input_irq_ack] input device at {:#x} irq={}", slot.base, slot.irq);
                            INPUT_IRQ.store(slot.irq, Ordering::Release);
                            *KEYBOARD_ACK.lock() = Some(InputAckDriver { inner });
                            return;
                        }
                        Err(e) => log::warn!("[input_irq_ack] init failed at {:#x}: {:?}", slot.base, e),
                    }
                } else {
                    core::mem::forget(transport);
                }
            }
            Err(_) => {}
        }
    }
}

/// Force-release this module's locks during fault teardown.
///
/// # Safety
/// Single-hart; called only from the fault/panic path with interrupts disabled.
pub unsafe fn force_unlock_locks() {
    KEYBOARD_ACK.force_unlock();
}

/// ACK the input IRQ to prevent interrupt storms.
///
/// Returns `true` if this IRQ belonged to the input device and was acknowledged.
/// Only needed while `has_waiter(irq)` is false (input Cell not yet registered).
pub fn ack_if_input(irq: u32) -> bool {
    let device_irq = INPUT_IRQ.load(Ordering::Relaxed);
    if device_irq == 0 || device_irq != irq {
        return false;
    }
    if let Some(drv) = KEYBOARD_ACK.lock().as_mut() {
        drv.inner.ack_interrupt();
    }
    true
}
