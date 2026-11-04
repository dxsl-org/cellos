// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `Transport` implementation: feature policy, status tracking, queue setup
//! and notification.

use super::{VirtioPciTransport, NOTIFY_CACHE_QUEUES, NOTIFY_UNCACHED};
use crate::error::InitFailure;
use crate::features::driver_features;
use crate::regs::NO_VECTOR;
use core::mem::{align_of, size_of};
use core::ptr::NonNull;
use virtio_drivers::transport::{DeviceStatus, DeviceType, Transport};
use virtio_drivers::{Error, PhysAddr};

/// ISR status bits: bit 0 queue interrupt, bit 1 configuration change.
const ISR_ANY: u8 = 0b11;

impl Transport for VirtioPciTransport {
    fn device_type(&self) -> DeviceType {
        self.device_type
    }

    fn read_device_features(&mut self) -> u64 {
        self.common.device_features()
    }

    /// Writes `driver_features` amended by [`crate::features::driver_features`]
    /// (re-reading the device's offer, checked against the construction-time
    /// [`crate::AccessPlatform`] policy). On [`InitFailure::Version1NotOffered`]
    /// or [`InitFailure::AccessPlatformMissing`] nothing is written and the
    /// failure is recorded, which turns the following `FEATURES_OK` write into
    /// `FAILED`.
    fn write_driver_features(&mut self, driver_features_requested: u64) {
        let device = self.common.device_features();
        match driver_features(device, driver_features_requested, self.policy) {
            Ok(features) => self.common.set_driver_features(features),
            Err(failure) => self.fail(failure),
        }
    }

    fn max_queue_size(&mut self, queue: u16) -> u32 {
        self.common.select_queue(queue);
        self.common.queue_size().into()
    }

    /// Writes `queue` to the queue's notify address. Nothing is written while
    /// a failure is recorded, nor for queues whose notify address lies outside
    /// the notify region (their `queue_set` recorded
    /// [`InitFailure::NotifyOutOfRange`]).
    fn notify(&mut self, queue: u16) {
        if self.failure.is_some() {
            return;
        }
        let cached = self
            .notify_cache
            .get(usize::from(queue))
            .copied()
            .filter(|&offset| offset != NOTIFY_UNCACHED);
        let offset = cached.or_else(|| {
            self.common.select_queue(queue);
            self.selected_notify_offset()
        });
        if let Some(offset) = offset {
            // SAFETY: offset + 2 <= notify_len (selected_notify_offset), the
            // notify region lies inside a mapped BAR, and offset is even
            // (even multiplier) on a 2-aligned base.
            unsafe {
                let addr = self.notify.as_ptr().add(offset as usize).cast::<u16>();
                addr.write_volatile(queue);
            }
        }
    }

    fn get_status(&self) -> DeviceStatus {
        DeviceStatus::from_bits_truncate(self.common.status().into())
    }

    /// Writing an empty status resets the device and waits, bounded by
    /// [`crate::RESET_SPIN_BUDGET`], for the reset to complete (spec: before
    /// reinitialising, device_status must read 0). A completed reset clears
    /// any recorded failure and the notify cache; one that never completes
    /// (or reads the device as gone) records [`InitFailure::ResetFailed`].
    /// After a failure every other write becomes `FAILED`. A `FEATURES_OK`
    /// write is read back: a cleared bit records
    /// [`InitFailure::FeaturesRejected`]; an accepted one routes the
    /// configuration-change interrupt to [`NO_VECTOR`].
    fn set_status(&mut self, status: DeviceStatus) {
        if status.is_empty() {
            self.features_ok = false;
            self.notify_cache = [NOTIFY_UNCACHED; NOTIFY_CACHE_QUEUES];
            self.failure = (!self.common.reset()).then_some(InitFailure::ResetFailed);
            return;
        }
        if let Some(failure) = self.failure {
            self.fail(failure);
            return;
        }
        self.common.set_status(status.bits() as u8);
        if status.contains(DeviceStatus::FEATURES_OK) && !self.features_ok {
            if self.get_status().contains(DeviceStatus::FEATURES_OK) {
                self.features_ok = true;
                self.common.set_msix_config(NO_VECTOR);
            } else {
                self.fail(InitFailure::FeaturesRejected);
            }
        }
    }

    fn set_guest_page_size(&mut self, _guest_page_size: u32) {}

    fn requires_legacy_layout(&self) -> bool {
        false
    }

    /// Programs and enables the queue with interrupts disabled
    /// ([`NO_VECTOR`]) and caches its notify offset. While a failure is
    /// recorded the queue is left untouched. A queue whose notify address
    /// falls outside the notify region is neither programmed nor enabled, and
    /// [`InitFailure::NotifyOutOfRange`] is recorded.
    fn queue_set(
        &mut self,
        queue: u16,
        size: u32,
        descriptors: PhysAddr,
        driver_area: PhysAddr,
        device_area: PhysAddr,
    ) {
        if self.failure.is_some() {
            return;
        }
        self.common.select_queue(queue);
        let Some(offset) = self.selected_notify_offset() else {
            self.fail(InitFailure::NotifyOutOfRange(queue));
            return;
        };
        self.common.set_queue_size(size as u16);
        self.common
            .set_queue_areas(descriptors as u64, driver_area as u64, device_area as u64);
        self.common.set_queue_msix_vector(NO_VECTOR);
        if let Some(slot) = self.notify_cache.get_mut(usize::from(queue)) {
            *slot = offset;
        }
        self.common.set_queue_enable(1);
    }

    /// No-op: virtio PCI has no way to disable a single enabled queue short
    /// of resetting the device.
    fn queue_unset(&mut self, _queue: u16) {}

    fn queue_used(&mut self, queue: u16) -> bool {
        self.common.select_queue(queue);
        self.common.queue_enable() == 1
    }

    /// Reads (and thereby clears) the ISR status byte; true when a queue or
    /// configuration-change interrupt was pending.
    fn ack_interrupt(&mut self) -> bool {
        // SAFETY: parse_caps proved the ISR region holds at least one byte
        // inside a BAR the mapper mapped for our lifetime.
        let isr = unsafe { self.isr.as_ptr().read_volatile() };
        isr & ISR_ANY != 0
    }

    /// The device configuration structure as `T`.
    ///
    /// # Errors
    /// `ConfigSpaceMissing` without a device-config capability,
    /// `ConfigSpaceTooSmall` when `T` is longer than the region, and
    /// `InvalidParam` when the region is not aligned for `T`.
    fn config_space<T: 'static>(&self) -> Result<NonNull<T>, Error> {
        let (ptr, len) = self.device_cfg.ok_or(Error::ConfigSpaceMissing)?;
        if size_of::<T>() > len as usize {
            return Err(Error::ConfigSpaceTooSmall);
        }
        if !(ptr.as_ptr() as usize).is_multiple_of(align_of::<T>()) {
            return Err(Error::InvalidParam);
        }
        Ok(ptr.cast())
    }
}
