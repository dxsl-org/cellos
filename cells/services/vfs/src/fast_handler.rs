//! Fast-IPC handler: serves VfsRequest::GetFile without ecall overhead.

use ostd::prelude::Mutex;

use crate::manager::VfsManager;

pub static GLOBAL_VFS: Mutex<Option<VfsManager>> = Mutex::new(None);

/// Fast-IPC handler: serves VfsRequest::GetFile without ecall overhead.
///
/// Authorized exactly like the ecall path. It has to be: `GetFile` replies with a
/// raw `DataPtr`, which in a single address space is permanent read authority that
/// cannot be revoked once handed out — so an ungated fast path would make the gate
/// on the ecall path decorative. `caller` comes from the kernel
/// (`kernel::fast_ipc::call_vfs` resolves it from live scheduler state), never from
/// an argument this cell's client controls; `None` means unattributable, which is
/// refused.
///
/// A cell this service has never served over the ecall path is declined with a
/// zero-length reply, which `call_vfs` callers treat as "fast path unavailable"
/// and retry as an ordinary syscall.
///
/// # Safety
/// Called with S-mode interrupts disabled (guaranteed by `ostd::fast_ipc::call_vfs`).
pub unsafe fn vfs_fast_handler(
    caller: Option<api::caller_identity::CallerIdentity>,
    req: &api::ipc::VfsRequest<'_>,
    out: &mut [u8; api::ipc::IPC_BUF_SIZE],
) -> usize {
    let resp = match caller.map(crate::caller::Caller::from_attested) {
        None => api::ipc::VfsResponse::Err(3),
        Some(caller) => match req {
            api::ipc::VfsRequest::GetFile(path) => {
                if let Some(vfs) = GLOBAL_VFS.lock().as_ref() {
                    if !vfs.dirs.has_met(caller) {
                        return 0;
                    }
                    if vfs.dirs.is_sealed(caller) || !vfs.access.can_read_fast(caller, path) {
                        api::ipc::VfsResponse::Err(3)
                    } else if let Some((ptr, len)) = vfs.get_file_ptr(path) {
                        api::ipc::VfsResponse::DataPtr {
                            ptr: ptr as u64,
                            len: len as u64,
                        }
                    } else {
                        api::ipc::VfsResponse::Err(1)
                    }
                } else {
                    api::ipc::VfsResponse::Err(0xFF)
                }
            }
            _ => api::ipc::VfsResponse::Err(0xFE),
        },
    };
    api::ipc::encode(&resp, out).map(|s| s.len()).unwrap_or(0)
}
