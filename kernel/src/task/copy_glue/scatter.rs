//! Scatter write adapter for TaskCopyView.

use super::TaskCopyView;
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
use super::{validate_kernel_range, TaskCopyRepr};
#[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
use alloc::vec::Vec;

impl TaskCopyView {
    /// Write contiguous payload `bytes` scattered across multiple destination
    /// user ranges `(ptr, len)`.
    ///
    /// Multi-destination write transaction: every destination is validated
    /// and staged (with reader pins retained for Domain views) BEFORE any byte
    /// is committed to user memory. If any destination fails, no destination
    /// is modified.
    pub(crate) fn write_scatter(
        &self,
        ranges: &[(usize, usize)],
        payload: &[u8],
    ) -> Result<(), ()> {
        #[cfg(all(feature = "native-domains", target_arch = "riscv64"))]
        {
            match &self.0 {
                TaskCopyRepr::Boundary(view) => {
                    use crate::task::user_copy::{copy_to_user_scatter, UserWriteSlice};
                    let mut writes = Vec::new();
                    if writes.try_reserve_exact(ranges.len()).is_err() {
                        return Err(());
                    }
                    let mut pos = 0usize;
                    for &(ptr, len) in ranges {
                        let chunk_len = len.min(payload.len().saturating_sub(pos));
                        let slice = UserWriteSlice::new(ptr, chunk_len, true).map_err(|_| ())?;
                        let chunk = if chunk_len > 0 {
                            &payload[pos..pos + chunk_len]
                        } else {
                            &[][..]
                        };
                        writes.push((slice, chunk));
                        pos += chunk_len;
                    }
                    copy_to_user_scatter(view, &writes).map_err(|_| ())
                }
                TaskCopyRepr::KernelDirect => {
                    let mut pos = 0usize;
                    for &(ptr, len) in ranges {
                        let chunk_len = len.min(payload.len().saturating_sub(pos));
                        validate_kernel_range(ptr, chunk_len, true)?;
                        pos += chunk_len;
                    }
                    pos = 0;
                    for &(ptr, len) in ranges {
                        let chunk_len = len.min(payload.len().saturating_sub(pos));
                        if chunk_len > 0 {
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    payload[pos..pos + chunk_len].as_ptr(),
                                    ptr as *mut u8,
                                    chunk_len,
                                );
                            }
                            pos += chunk_len;
                        }
                    }
                    Ok(())
                }
            }
        }
        #[cfg(not(all(feature = "native-domains", target_arch = "riscv64")))]
        {
            let mut pos = 0usize;
            for &(ptr, len) in ranges {
                let chunk_len = len.min(payload.len().saturating_sub(pos));
                if chunk_len > 0 {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            payload[pos..pos + chunk_len].as_ptr(),
                            ptr as *mut u8,
                            chunk_len,
                        );
                    }
                    pos += chunk_len;
                }
            }
            Ok(())
        }
    }
}
