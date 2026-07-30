//! Typed IPC request dispatch — decodes one `VfsRequest` and produces the
//! `VfsResponse`, routing filesystem ops through the MountTable.
//!
//! Cross-cutting policy lives here, not in backends: AccessTable authorization,
//! quota accounting (net-delta on overwrite), async-read pending table, and
//! zero-copy grant I/O.

use crate::caller::Caller;
use crate::manager::VfsManager;

/// `types::ViError::PermissionDenied` as the wire code.
const ERR_DENIED: u8 = 3;
/// The path does not exist, or the backend refused the operation.
const ERR_IO: u8 = 1;
/// Quota exceeded.
const ERR_QUOTA: u8 = 2;
/// Stale, unknown, or not-this-caller's async-read handle.
const ERR_HANDLE: u8 = 4;

/// Handle one decoded request on behalf of a caller the kernel has attested.
///
/// `caller` is `None` when the kernel did not vouch for the sender — an
/// unattested delivery path, or a sender that died before it could be resolved.
/// Every request is then refused: a service that guesses at an identity is a
/// service with no authorization at all, and "some cell that owns nothing" is not
/// a safe guess, because owning nothing still reads everything that is unowned.
///
/// `resp_buf` backs `VfsResponse::Data` payloads, so the response borrows it;
/// callers encode before reusing the buffer.
pub fn handle_request<'a>(
    vfs: &mut VfsManager,
    buf: &[u8; api::ipc::IPC_BUF_SIZE],
    caller: Option<Caller>,
    resp_buf: &'a mut [u8; api::ipc::IPC_BUF_SIZE],
) -> api::ipc::VfsResponse<'a> {
    let Some(caller) = caller else {
        return api::ipc::VfsResponse::Err(ERR_DENIED);
    };

    // Decode typed request; `take_from_bytes` tolerates trailing bytes in the
    // receive buffer — both zeros left by a previous message and the kernel's
    // caller-identity trailer at the very end.
    let req = match api::ipc::decode::<api::ipc::VfsRequest>(buf) {
        Ok(r) => r,
        Err(_) => return api::ipc::VfsResponse::Err(0xFF), // malformed request
    };

    match req {
        api::ipc::VfsRequest::GetFile(p) => {
            // Authorize BEFORE resolving: the reply is a raw pointer into VFS
            // memory, which in a single address space is permanent read authority
            // that cannot be taken back once handed out.
            if !vfs.access.can_read(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            if let Some((ptr, len)) = vfs.get_file_ptr(p) {
                api::ipc::VfsResponse::DataPtr {
                    ptr: ptr as u64,
                    len: len as u64,
                }
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::ListDir(p) => {
            // Authorize BEFORE listing: a directory listing is itself information.
            if !vfs.access.can_read(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            let n = vfs.list_dir(p, resp_buf);
            api::ipc::VfsResponse::Data(&resp_buf[..n])
        }

        api::ipc::VfsRequest::Stat(p) => {
            // Authorize BEFORE stat: size and existence are what an unauthorized
            // caller would be probing for.
            if !vfs.access.can_read(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            match vfs.stat(p) {
                Some((size, is_dir)) => api::ipc::VfsResponse::Stat { size, is_dir },
                None => api::ipc::VfsResponse::Err(ERR_IO),
            }
        }

        api::ipc::VfsRequest::Write { path, content } => {
            if !vfs.access.can_write(caller, path) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            // Overwriting charges the delta, not the full new size — otherwise
            // repeated overwrites inflate usage.  The delta is only a delta for
            // the cell that was charged for the old contents: if another cell
            // wrote them, this caller gets nothing back and must afford the whole
            // new size.
            let old_size = vfs.file_size(path);
            let new_size = content.len() as u64;
            let refunded_to_caller = if vfs.quota.writer_of(path) == Some(caller.cell) {
                old_size
            } else {
                0
            };
            let net_charge = new_size.saturating_sub(refunded_to_caller);
            if net_charge > 0 && !vfs.quota.can_charge(caller.cell, net_charge) {
                return api::ipc::VfsResponse::Err(ERR_QUOTA);
            }
            if vfs.write(path, content) {
                // Release the old contents to whoever was charged for them, then
                // charge the new contents to this caller.
                vfs.quota.release_path(path, old_size);
                let _ = vfs.quota.charge(caller.cell, new_size);
                vfs.quota.set_writer(path, caller.cell);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::Append { path, content } => {
            if !vfs.access.can_write(caller, path) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            let append_len = content.len() as u64;
            if !vfs.quota.can_charge(caller.cell, append_len) {
                return api::ipc::VfsResponse::Err(ERR_QUOTA);
            }
            if vfs.append(path, content) {
                let _ = vfs.quota.charge(caller.cell, append_len);
                // Only claims the path if nobody was charged for it yet; an
                // append does not move the earlier bytes' ownership.
                vfs.quota.record_writer(path, caller.cell);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::Mkdir(p) => {
            if !vfs.access.can_write(caller, p) {
                api::ipc::VfsResponse::Err(ERR_DENIED)
            } else if vfs.mkdir(p) {
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::Rmdir(p) => {
            // Destructive: authorize before touching the backend.  A path the caller
            // may not write is a path it may not delete.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            // Verifies the target IS a directory — POSIX ENOTDIR semantics.
            if vfs.rmdir(p) {
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::Unlink(p) => {
            // Authorize BEFORE `file_size`: an unauthorized caller must learn nothing,
            // not even whether the file exists or how large it is.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            let file_size = vfs.file_size(p);
            if vfs.unlink(p) {
                // Credit the cell that was charged, never the cell that asked:
                // otherwise deleting another cell's file mints quota for the
                // deleter and leaves the writer charged for bytes that are gone.
                vfs.quota.release_path(p, file_size);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::RmdirRecursive(p) => {
            // Authorize BEFORE the walk: that walk lists the whole subtree, so
            // checking after it would leave a directory-size probe open to callers
            // who may not write the path.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            // Measure per file while the tree still exists: rmdir_recursive
            // returns only bool, and two files in one tree can be charged to two
            // different cells.
            let files = crate::subtree::files_under(vfs, p, 32);
            if vfs.rmdir_recursive(p) {
                for (path, size) in files {
                    vfs.quota.release_path(&path, size);
                }
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(ERR_IO)
            }
        }

        api::ipc::VfsRequest::ReadAsync { path } => {
            // Authorize BEFORE reading: the read happens now (the disk backend is
            // still blocking) and `Poll` only hands over what was already read, so
            // this is the only point where the path is known.
            if !vfs.access.can_read(caller, path) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            // A handle records durable state against the caller, so it needs a
            // caller that cannot be confused with a later one.
            if !caller.may_own_state() {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            let data = vfs.read_to_vec(path);
            let handle = vfs.pending.insert(caller, path, data);
            api::ipc::VfsResponse::PendingHandle(handle)
        }

        api::ipc::VfsRequest::Poll { handle } => {
            // With a synchronous backend data is always ready on first poll.
            //
            // Two independent checks, in this order.  Ownership first: a handle
            // owned by another cell is refused with the same code as a stale one,
            // so sweeping the sequential handle space reveals nothing about other
            // cells.  Then the path rules again — the slot stores the path it was
            // filled from, because the authorization at `ReadAsync` time proves
            // only what policy said then, and a slot can outlive a rule change.
            let readable = match vfs.pending.owned_path(caller, handle) {
                Some(path) => vfs.access.can_read(caller, path),
                None => return api::ipc::VfsResponse::Err(ERR_HANDLE),
            };
            if !readable {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            match vfs.pending.poll(caller, handle) {
                Some(data) => {
                    // Cap at 480, not resp_buf.len(): the reply must still fit
                    // the 512-byte IPC frame AFTER the postcard envelope. A
                    // full 512-byte payload made encode fail and the client
                    // saw an empty reply (surfaced by /bin ELF reads).
                    let n = data.len().min(480);
                    resp_buf[..n].copy_from_slice(&data[..n]);
                    api::ipc::VfsResponse::Data(&resp_buf[..n])
                }
                None => api::ipc::VfsResponse::Err(ERR_HANDLE),
            }
        }

        // ── Zero-Copy Grant I/O (Storage 2.0) ──────────────────────────────
        api::ipc::VfsRequest::ReadGrant {
            cap,
            offset,
            size,
            grant,
        } => {
            // Re-authorize the handle's path before anything is copied, for the
            // same reason as `Poll`: the open-time decision can be stale.  A cap
            // the caller does not own is indistinguishable from an unknown one
            // (`None` here, zero bytes below), but a cap it *does* own whose path
            // is now denied gets a straight refusal — that leaks nothing it did
            // not already know.
            match vfs.handles.path_of(caller, api::cap::CapId(cap)) {
                Some(path) if !vfs.access.can_read(caller, path) => {
                    // Refuse before touching the grant: nothing is read, so there
                    // is no F14 drain obligation.
                    return api::ipc::VfsResponse::Err(ERR_DENIED);
                }
                _ => {}
            }
            // Validate: VFS must have been GrantShare'd access by the app.
            match ostd::syscall::sys_grant_slice(grant) {
                None => api::ipc::VfsResponse::Err(ERR_IO), // no access
                Some(ptr) => {
                    // A cap owned by another cell reports zero bytes, exactly like
                    // an unknown cap — the caller cannot tell the two apart.
                    let bytes =
                        if let Some(entry) = vfs.handles.get_mut(caller, api::cap::CapId(cap)) {
                            let avail = entry.data_len.saturating_sub(offset as usize);
                            let n = size.min(avail).min(4096);
                            // SAFETY: data_ptr is a valid in-memory VAddr; ptr is a
                            // kernel-allocated, identity-mapped grant buffer.
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    (entry.data_ptr + offset as usize) as *const u8,
                                    ptr,
                                    n,
                                );
                            }
                            n
                        } else {
                            0 // unknown cap, or not this caller's — nothing is copied
                        };
                    // F14: reply AFTER filling the buffer.
                    api::ipc::VfsResponse::GrantDone { bytes }
                }
            }
        }

        api::ipc::VfsRequest::WriteGrant {
            cap,
            offset,
            grant,
            bytes,
        } => {
            // FAIL-CLOSED until cap→path routing exists.
            //
            // This arm cannot resolve the target path from `cap`, so it cannot run
            // `access.can_write` — and an unauthorizable write must be refused, not
            // performed.  It previously drained the grant, dropped the bytes, and
            // replied `GrantDone { bytes }`: a success report for a write that never
            // happened, with no authorization check anywhere on the path.  Reporting
            // success for a discarded write is worse than refusing: a caller cannot
            // distinguish it from a real write, and wiring the routing later would
            // silently turn the same unchecked path into real disk writes.
            //
            // Refuse before touching the grant — nothing is read, so there is no
            // F14 drain obligation and no `unsafe` needed here.  When cap→path
            // routing lands, authorize with `can_write` (and charge quota with the
            // net-delta pattern from the `Write` arm) BEFORE writing anything.
            let _ = (cap, offset, grant, bytes);
            api::ipc::VfsResponse::Err(ERR_DENIED)
        }

        api::ipc::VfsRequest::ReadFileGrant { path, grant, max } => {
            // Authorize BEFORE the grant is even resolved: this arm copies a whole
            // file, so it is the widest read in the interface.
            if !vfs.access.can_read(caller, path) {
                return api::ipc::VfsResponse::Err(ERR_DENIED);
            }
            match ostd::syscall::sys_grant_slice(grant) {
                None => api::ipc::VfsResponse::Err(ERR_IO), // grant not shared to VFS
                Some(ptr) => {
                    // Resolve via the mount table (BinOverlay → cell-store for /bin),
                    // then copy the WHOLE file into the caller's grant in one shot.
                    let data = vfs.read_to_vec(path);
                    let n = data.len().min(max);
                    // SAFETY: ptr is the caller's identity-mapped grant, GrantShare'd
                    // RW and (per `max`) large enough for n bytes; `data` is a fresh
                    // owned Vec. The caller's ipc_call blocks until we reply, so it
                    // cannot free the grant before this copy completes.
                    unsafe {
                        core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, n);
                    }
                    api::ipc::VfsResponse::GrantDone { bytes: n }
                }
            }
        }
    }
}
