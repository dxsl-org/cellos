//! Client memory behind GEM objects (CP1 D-G1 option B): the shim allocates a buffer in
//! this Cell, exports it to the `XE_GPU` driver (`ExportDmaRange`, 437), and revokes and
//! frees it once the object is gone (`RevokeDmaExport`, 439).

use alloc::collections::BTreeMap;
use ostd::dma::DmaBuf;
use ostd::syscall::DmaExportError;

/// A buffer of this Cell's own memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientBuffer {
    /// Allocator-private name.
    pub id: usize,
    /// Where the CPU reaches byte 0 in this Cell.
    pub addr: u64,
    pub size: usize,
}

/// Why a revoke did not finish.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeError {
    /// The kernel has not confirmed the device drain yet; the range stays locked and the
    /// kernel retries on its own. Ask again later.
    Pending,
}

/// Allocation and export of client memory. Production: [`DmaClientMemory`]; host tests
/// supply a fake.
pub trait ClientMemory {
    /// `size` bytes (a page multiple) of zero-filled memory, or `None` if this Cell cannot
    /// back them.
    fn alloc(&mut self, size: usize) -> Option<ClientBuffer>;
    /// Export `buf` to the GPU driver; `device_write` lets the device write it.
    fn export(&mut self, buf: &ClientBuffer, device_write: bool) -> Option<u64>;
    /// Revoke an export as its owner. `Ok` once the device can no longer reach it, or
    /// when the lease is already gone (driver or kernel revoked it).
    fn revoke(&mut self, export_id: u64) -> Result<(), RevokeError>;
    /// Return the memory. Only called after its export is revoked.
    fn free(&mut self, buf: ClientBuffer);
}

/// [`ClientMemory`] from Grants: `DmaBuf::alloc` (zeroed, physically contiguous) and
/// `DmaBuf::export` to `service::XE_GPU`. A Tier-2 private-root Cell gets no Grant and no
/// export (D-G0 large memory is not implemented), so its allocations fail.
#[derive(Default)]
pub struct DmaClientMemory {
    buffers: BTreeMap<usize, DmaBuf>,
}

impl ClientMemory for DmaClientMemory {
    fn alloc(&mut self, size: usize) -> Option<ClientBuffer> {
        let buf = DmaBuf::alloc(size.div_ceil(4096))?;
        let handle = ClientBuffer {
            id: buf.phys(),
            addr: buf.phys() as u64,
            size,
        };
        self.buffers.insert(handle.id, buf);
        Some(handle)
    }

    fn export(&mut self, buf: &ClientBuffer, device_write: bool) -> Option<u64> {
        self.buffers
            .get(&buf.id)?
            .export(0, buf.size, api::syscall::service::XE_GPU, device_write)
            .ok()
    }

    fn revoke(&mut self, export_id: u64) -> Result<(), RevokeError> {
        match ostd::dma::revoke_export(export_id) {
            // Revoked now, or already gone with the driver or by its own revoke.
            Ok(()) | Err(DmaExportError::NotFound) => Ok(()),
            // Undrained, or refused: the lease may still be live, so the memory must not
            // be freed yet.
            Err(_) => Err(RevokeError::Pending),
        }
    }

    fn free(&mut self, buf: ClientBuffer) {
        if let Some(buf) = self.buffers.remove(&buf.id) {
            buf.free();
        }
    }
}
