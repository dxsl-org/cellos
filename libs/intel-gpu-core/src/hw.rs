//! What the hardware-independent `xe` backend asks of the GPU and of the kernel.
//!
//! [`crate::backend::XeSession`] keeps the uAPI bookkeeping (handles, VMs, queues) and
//! calls a [`GpuHw`] for every effect that reaches the device: PPGTT leaves, GuC contexts,
//! ring submission, and the client-memory export leases of CP1 D-G1 option B. The driver
//! Cell implements it over MMIO, its DMA pool and syscalls 438/439; host tests implement it
//! with recording fakes. Keeping the effects behind one trait is what lets the uAPI rules be
//! tested byte for byte without a GPU.

use crate::backend::XeError;
use crate::pte::PatIndex;

/// A client export as `MapDmaExport` (438) described it (`ViDmaExportV1`, CP1 §5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportRecord {
    pub export_id: u64,
    /// The exporting Cell: must equal the identity of the request that carried the id.
    pub owner_cell_id: u64,
    pub owner_generation: u64,
    /// Base of the kernel-chosen IOVA window in the driver's VT-d domain (4 KiB aligned).
    pub iova: u64,
    /// Bytes the window covers.
    pub len: u64,
    /// `EXPORT_DEVICE_WRITE`: clear means the remapper lets the device only read.
    pub device_write: bool,
}

/// Why a 438/439 call failed, mirroring `ostd::syscall::DmaExportError`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportError {
    PermissionDenied,
    NotFound,
    InvalidInput,
    NotSupported,
    OutOfMemory,
    /// 439: the window drain is not acknowledged yet; the kernel retries it on its own.
    TryAgain,
    /// 438: the window was published but its invalidation was not acknowledged.
    Unconfirmed,
    /// The kernel wrote a record the v1 decoder refuses.
    Malformed,
}

/// The holder half of the export syscalls (CP1 §5.2-5.3).
pub trait ExportMapper {
    /// `MapDmaExport` (438): map `export_id` into the driver's device domain. Repeat
    /// calls return the same record.
    fn map_export(&mut self, export_id: u64) -> Result<ExportRecord, ExportError>;
    /// `RevokeDmaExport` (439) as the lease holder. Called only after the GPU can no
    /// longer reach the window through any PPGTT this driver wrote.
    fn revoke_export(&mut self, export_id: u64) -> Result<(), ExportError>;
}

/// A hardware address space (one per-client PPGTT), named by the [`GpuHw`] that made it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HwVm(pub u32);

/// A hardware execution context (one GuC context with its LRC and ring).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HwQueue(pub u32);

/// Device effects of the `xe` uAPI.
///
/// Every method that reports success has completed its effect on the device side (leaves
/// written, H2G message published); nothing is queued for later.
pub trait GpuHw: ExportMapper {
    /// Allocate an empty per-client PPGTT.
    fn vm_create(&mut self) -> Result<HwVm, XeError>;
    /// Write 4 KiB leaves for `[va, va + len)` pointing at device addresses
    /// `[dma, dma + len)`. A failure leaves no leaf of this call behind.
    fn vm_map(
        &mut self,
        vm: HwVm,
        va: u64,
        dma: u64,
        len: u64,
        pat: PatIndex,
        writable: bool,
    ) -> Result<(), XeError>;
    /// Write `XE_PTE_NULL` leaves for `[va, va + len)`: reads return zero, writes drop.
    fn vm_map_null(&mut self, vm: HwVm, va: u64, len: u64, pat: PatIndex) -> Result<(), XeError>;
    /// Clear every present leaf in `[va, va + len)` and invalidate the GPU TLBs, so the
    /// range is unreachable when this returns.
    fn vm_unmap(&mut self, vm: HwVm, va: u64, len: u64);
    /// Free the PPGTT's tables. The caller has unmapped every range and destroyed every
    /// queue that used the VM.
    fn vm_destroy(&mut self, vm: HwVm);
    /// Create and register a GuC context of `engine_class` running in `vm`.
    fn queue_create(&mut self, vm: HwVm, engine_class: u16) -> Result<HwQueue, XeError>;
    /// Run the batch at GPU VA `batch_va` on `queue`. `Err(TryAgain)` when the ring has
    /// no room for another job.
    fn queue_submit(&mut self, queue: HwQueue, batch_va: u64) -> Result<(), XeError>;
    /// The `DRM_XE_EXEC_QUEUE_BAN_FLAG_*` bits the GuC reported for `queue`, 0 if none.
    fn queue_ban(&mut self, queue: HwQueue) -> u32;
    /// Stop and deregister the context and free its LRC.
    fn queue_destroy(&mut self, queue: HwQueue);
}

#[cfg(test)]
#[path = "hw/fake.rs"]
pub(crate) mod fake;
