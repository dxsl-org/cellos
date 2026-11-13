// SPDX-License-Identifier: Apache-2.0 OR MIT
//! [`VirtioPciTransport`] construction and the state its `Transport` impl
//! (in `ops`) shares.

mod ops;

use crate::caps::{parse_caps, CapRegion, CfgType};
use crate::error::{InitFailure, TransportError};
use crate::features::{AccessPlatform, ACCESS_PLATFORM};
use crate::identity::device_type_for;
use crate::regs::CommonRegs;
use abi::pcie_function::{ViPcieFunctionV1, PCIE_FUNCTION_BARS};
use core::ptr::NonNull;
use virtio_drivers::transport::{DeviceStatus, DeviceType};

/// Maps a BAR into the caller's address space.
///
/// A Driver Cell implements this with `ostd::mmio::request_region(base, len)`
/// (in the single-address-space model the returned VA equals `base`); tests
/// hand out fake memory. The mapping must stay valid, readable and writable
/// for the whole lifetime of the transport built from it.
pub trait BarMapper {
    /// Maps all `len` bytes of BAR `bar`, whose physical address is `base`.
    ///
    /// Called at most once per BAR. Returns the start of the mapping, or
    /// `None` when mapping fails.
    fn map(&mut self, bar: u8, base: u64, len: u64) -> Option<NonNull<u8>>;
}

/// Queues whose notify offset is cached by `queue_set`; higher queues read
/// `queue_notify_off` on every notify instead.
const NOTIFY_CACHE_QUEUES: usize = 16;
const NOTIFY_UNCACHED: u32 = u32::MAX;

/// Virtio 1.x PCI transport for a function described by a 434 record.
///
/// Interrupts are never routed: `msix_config` and every queue vector are set
/// to [`crate::NO_VECTOR`], so drivers poll.
pub struct VirtioPciTransport {
    device_type: DeviceType,
    common: CommonRegs,
    notify: NonNull<u8>,
    notify_len: u32,
    notify_off_multiplier: u32,
    isr: NonNull<u8>,
    device_cfg: Option<(NonNull<u8>, u32)>,
    /// Byte offset into `notify` per queue, or `NOTIFY_UNCACHED`.
    notify_cache: [u32; NOTIFY_CACHE_QUEUES],
    /// Re-checked against the device's offer at every feature write.
    policy: AccessPlatform,
    failure: Option<InitFailure>,
    /// The device kept `FEATURES_OK` set on read-back since the last reset.
    features_ok: bool,
}

// SAFETY: the pointers address device MMIO, which any CPU may access.
unsafe impl Send for VirtioPciTransport {}
// SAFETY: `&self` methods only perform MMIO reads or hand out the config-space
// pointer; concurrent reads are harmless.
unsafe impl Sync for VirtioPciTransport {}

/// Maps `region`'s BAR (once) and returns the region's start.
fn region_ptr(
    record: &ViPcieFunctionV1,
    mapper: &mut impl BarMapper,
    bars: &mut [Option<NonNull<u8>>; PCIE_FUNCTION_BARS],
    kind: CfgType,
    region: CapRegion,
    align: usize,
) -> Result<NonNull<u8>, TransportError> {
    let index = region.bar as usize;
    let base = match bars[index] {
        Some(base) => base,
        None => {
            let bar = &record.bars[index];
            let base = mapper
                .map(region.bar, bar.base, bar.len)
                .ok_or(TransportError::MapFailed(region.bar))?;
            bars[index] = Some(base);
            base
        }
    };
    // SAFETY: parse_caps proved offset + length <= bar.len, and the mapper
    // mapped all bar.len bytes, so the result stays inside the mapping.
    let ptr = unsafe { base.add(region.offset as usize) };
    if !(ptr.as_ptr() as usize).is_multiple_of(align) {
        return Err(TransportError::MisalignedRegion(kind));
    }
    Ok(ptr)
}

impl VirtioPciTransport {
    /// Builds a transport for the function `record` describes.
    ///
    /// Maps each BAR that holds a virtio structure exactly once through
    /// `mapper`, then resets the device and waits (bounded by
    /// [`crate::RESET_SPIN_BUDGET`]) for the reset to complete: a previous
    /// owner may have left it running on stale rings, and it must be quiesced
    /// before this cell's DMA grant re-enables bus mastering. Finally reads
    /// the device's upper feature word to enforce `policy`.
    ///
    /// # Errors
    /// Identity errors from [`crate::device_type_for`], capability errors from
    /// [`crate::parse_caps`], [`TransportError::MapFailed`],
    /// [`TransportError::MisalignedRegion`] (common/device structures need
    /// 4-byte alignment, notify 2-byte), [`TransportError::ResetFailed`], and
    /// [`TransportError::AccessPlatformMissing`].
    pub fn new(
        record: &ViPcieFunctionV1,
        mapper: &mut impl BarMapper,
        policy: AccessPlatform,
    ) -> Result<Self, TransportError> {
        let device_type = device_type_for(record.vendor_id, record.device_id)?;
        let caps = parse_caps(record)?;
        let mut bars = [None; PCIE_FUNCTION_BARS];
        let mut map =
            |kind, region, align| region_ptr(record, &mut *mapper, &mut bars, kind, region, align);
        let common = map(CfgType::Common, caps.common, 4)?;
        let notify = map(CfgType::Notify, caps.notify, 2)?;
        let isr = map(CfgType::Isr, caps.isr, 1)?;
        let device_cfg = match caps.device {
            Some(region) => Some((map(CfgType::Device, region, 4)?, region.length)),
            None => None,
        };
        // SAFETY: parse_caps proved the common region holds COMMON_CFG_LEN
        // bytes inside a BAR the mapper mapped for our lifetime; 4-aligned above.
        let common = unsafe { CommonRegs::new(common) };
        if !common.reset() {
            return Err(TransportError::ResetFailed);
        }
        let offers_access_platform =
            (u64::from(common.device_features_hi()) << 32) & ACCESS_PLATFORM != 0;
        if policy == AccessPlatform::Required && !offers_access_platform {
            return Err(TransportError::AccessPlatformMissing);
        }
        Ok(Self {
            device_type,
            common,
            notify,
            notify_len: caps.notify.length,
            notify_off_multiplier: caps.notify_off_multiplier,
            isr,
            device_cfg,
            notify_cache: [NOTIFY_UNCACHED; NOTIFY_CACHE_QUEUES],
            policy,
            failure: None,
            features_ok: false,
        })
    }

    /// True once the device has accepted the negotiated features
    /// (`FEATURES_OK` read back set) and no [`InitFailure`] has occurred since
    /// the last reset. The generic `begin_init` never checks this; drivers
    /// must, before using the device.
    pub fn negotiated_ok(&self) -> bool {
        self.features_ok && self.failure.is_none()
    }

    /// The first failure since the last reset, if any. When set, the device
    /// status carries `FAILED` and `DRIVER_OK` is never written.
    pub fn init_failure(&self) -> Option<InitFailure> {
        self.failure
    }

    /// Records the first failure and tells the device the driver gave up.
    /// Status bits already set are kept: the spec forbids clearing them.
    fn fail(&mut self, failure: InitFailure) {
        self.failure.get_or_insert(failure);
        let failed = self.common.status() | DeviceStatus::FAILED.bits() as u8;
        self.common.set_status(failed);
    }

    /// Byte offset of the selected queue's notify address inside the notify
    /// region, or `None` when a 16-bit write there would leave the region.
    fn selected_notify_offset(&self) -> Option<u32> {
        let offset =
            u64::from(self.common.queue_notify_off()) * u64::from(self.notify_off_multiplier);
        (offset + 2 <= u64::from(self.notify_len)).then_some(offset as u32)
    }
}

impl Drop for VirtioPciTransport {
    /// Resets the device so it stops DMA into memory the driver may free next.
    fn drop(&mut self) {
        use virtio_drivers::transport::Transport;
        self.set_status(DeviceStatus::empty());
    }
}
