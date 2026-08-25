//! Task-scoped adapters over the phase-03 recoverable user-copy boundary.
//!
//! [`TaskCopyView`] is derived from a task's [`TaskAddressSpace`]: `Sas`
//! yields the shared-space view, `Domain` clones the pinned
//! `Arc<AddressSpace>` (same source as `DomainRef::from_task`; the pin
//! contract from phase-02 applies — liveness is not rechecked here). Every
//! helper routes byte movement through `user_copy`, so an invalid or revoked
//! user range surfaces as a recoverable error instead of a fatal fault.
//!
//! The boundary itself (`user_copy`) is compiled only for RV64 +
//! `native-domains`. Other targets keep today's shared-address-space access
//! verbatim behind the same API, so their semantics are unchanged.

mod scatter;

use super::tcb::Task;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
use super::tcb::TaskAddressSpace;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
use alloc::sync::Arc;
use alloc::vec::Vec;

/// Execution view for byte movement performed on behalf of one task.
///
/// Derive it while holding whatever lock produced the `&Task`; the view owns
/// its state afterwards, so guarded copies can run with that lock dropped.
#[derive(Clone)]
pub(crate) struct TaskCopyView(TaskCopyRepr);

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
#[derive(Clone)]
pub(super) enum TaskCopyRepr {
    Boundary(super::user_copy::CopyView),
    KernelDirect,
}
#[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
#[derive(Clone, Copy)]
pub(super) enum TaskCopyRepr {
    Shared,
}

impl TaskCopyView {
    /// Derive the view from a task's address-space binding.
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    pub(crate) fn of(task: &Task) -> Self {
        use super::user_copy::CopyView;
        let repr = match &task.address_space {
            TaskAddressSpace::Sas if task.user_stack.is_none() => TaskCopyRepr::KernelDirect,
            TaskAddressSpace::Sas => TaskCopyRepr::Boundary(CopyView::Sas),
            TaskAddressSpace::Domain(space) => {
                TaskCopyRepr::Boundary(CopyView::Domain(Arc::clone(space)))
            }
        };
        Self(repr)
    }

    /// Derive the view from a task's address-space binding.
    #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
    pub(crate) fn of(_task: &Task) -> Self {
        Self(TaskCopyRepr::Shared)
    }

    /// Return the shared-address-space view directly, without a task lookup.
    /// Used as a safe fallback when the task record is unavailable but the
    /// address-space context is known to be SAS (e.g. kernel-originated copies).
    #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
    pub(crate) fn sas() -> Self {
        use super::user_copy::CopyView;
        Self(TaskCopyRepr::Boundary(CopyView::Sas))
    }

    #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
    pub(crate) fn sas() -> Self {
        Self(TaskCopyRepr::Shared)
    }

    /// Derive the view for a task by TID under a brief scheduler lock.
    /// Returns `None` if the task is not found (never silently falls back to SAS).
    pub(crate) fn for_task(task_id: usize) -> Option<Self> {
        let guard = super::SCHEDULER.lock();
        guard
            .as_ref()
            .and_then(|s| s.tasks.get(&task_id))
            .map(|t| Self::of(t))
    }

    /// Read `len` bytes at `ptr` into a fresh owned buffer. A zero-length
    /// read is valid and yields an empty buffer. Allocation failure and copy
    /// rejection both map to `Err(())`.
    pub(crate) fn read_bytes(&self, ptr: usize, len: usize) -> Result<Vec<u8>, ()> {
        let mut buf = Vec::new();
        buf.try_reserve_exact(len).map_err(|_| ())?;
        buf.resize(len, 0);
        self.read_into(ptr, &mut buf)?;
        Ok(buf)
    }

    /// Read exactly `dst.len()` bytes at `ptr` into `dst`. On failure `dst`
    /// is untouched (the boundary probes before it commits).
    pub(crate) fn read_into(&self, ptr: usize, dst: &mut [u8]) -> Result<(), ()> {
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        {
            match &self.0 {
                TaskCopyRepr::Boundary(view) => {
                    use super::user_copy::{copy_from_user, UserReadSlice};
                    let src = UserReadSlice::new(ptr, dst.len(), true).map_err(|_| ())?;
                    copy_from_user(view, src, dst).map_err(|_| ())
                }
                TaskCopyRepr::KernelDirect => {
                    validate_kernel_range(ptr, dst.len(), false)?;
                    if !dst.is_empty() {
                        let src =
                            unsafe { core::slice::from_raw_parts(ptr as *const u8, dst.len()) };
                        dst.copy_from_slice(src);
                    }
                    Ok(())
                }
            }
        }
        #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
        {
            if dst.is_empty() {
                return Ok(());
            }
            // Legacy shared-address-space access, unchanged from phase-02.
            let src = unsafe { core::slice::from_raw_parts(ptr as *const u8, dst.len()) };
            dst.copy_from_slice(src);
            Ok(())
        }
    }

    /// Write all of `bytes` at `ptr`. An empty write is a validated no-op.
    pub(crate) fn write_bytes(&self, ptr: usize, bytes: &[u8]) -> Result<(), ()> {
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        {
            match &self.0 {
                TaskCopyRepr::Boundary(view) => {
                    use super::user_copy::{copy_to_user, UserWriteSlice};
                    let dst = UserWriteSlice::new(ptr, bytes.len(), true).map_err(|_| ())?;
                    copy_to_user(view, dst, bytes).map_err(|_| ())
                }
                TaskCopyRepr::KernelDirect => {
                    validate_kernel_range(ptr, bytes.len(), true)?;
                    if !bytes.is_empty() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                bytes.as_ptr(),
                                ptr as *mut u8,
                                bytes.len(),
                            )
                        };
                    }
                    Ok(())
                }
            }
        }
        #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
        {
            if bytes.is_empty() {
                return Ok(());
            }
            // Legacy shared-address-space access, unchanged from phase-02.
            unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
            Ok(())
        }
    }
}

#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
pub(super) fn validate_kernel_range(ptr: usize, len: usize, write: bool) -> Result<(), ()> {
    if len == 0 {
        return Ok(());
    }
    if ptr == 0 {
        return Err(());
    }
    let end = ptr.checked_add(len).ok_or(())?;
    if let Some(root_pa) = super::user_copy::current_satp_root() {
        const SV39_VALID: usize = 1 << 0;
        const SV39_READ: usize = 1 << 1;
        const SV39_WRITE: usize = 1 << 2;
        const PAGE_SIZE: usize = 4096;
        let need = if write { SV39_WRITE } else { SV39_READ };
        let mut page = ptr & !(PAGE_SIZE - 1);
        while page < end {
            let (bits, _) = super::user_copy::sv39_leaf(root_pa, page).ok_or(())?;
            if bits & (SV39_VALID | need) != SV39_VALID | need {
                return Err(());
            }
            page += PAGE_SIZE;
        }
    }
    Ok(())
}
