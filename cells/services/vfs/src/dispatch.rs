//! Typed IPC request dispatch — decodes one `VfsRequest` and produces the
//! `VfsResponse`, routing filesystem ops through the MountTable.
//!
//! Cross-cutting policy lives here, not in backends: AccessTable authorization,
//! quota accounting (net-delta on overwrite), async-read pending table, and
//! zero-copy grant I/O.

use crate::manager::VfsManager;

/// Handle one decoded request. `resp_buf` backs `VfsResponse::Data` payloads,
/// so the response borrows it; callers encode before reusing the buffer.
pub fn handle_request<'a>(
    vfs: &mut VfsManager,
    buf: &[u8; api::ipc::IPC_BUF_SIZE],
    sender: usize,
    resp_buf: &'a mut [u8; api::ipc::IPC_BUF_SIZE],
) -> api::ipc::VfsResponse<'a> {
    // The ONLY place caller identity is established.  Every owner comparison and
    // quota charge below reads `caller`, so a change of identity source is a
    // change here and nowhere else.
    //
    // `sender` is the tid the kernel reports for the caller, and the loader
    // assigns `cell_id == CellId(tid)` to every cell it spawns — so for a cell,
    // this value IS its cell id.  A thread receives its own tid while inheriting
    // its parent's cell id, so this derivation would misattribute a thread; no
    // cell spawns threads today.
    //
    // tid 0 is not a caller.  An identity that cannot be resolved is denied
    // outright rather than treated as some cell that happens to own nothing —
    // owning nothing would still let it read unowned state as new tables appear.
    if sender == 0 {
        return api::ipc::VfsResponse::Err(3); // 3 = PermissionDenied
    }
    let caller = types::CellId(sender as u64);

    // Decode typed request; `take_from_bytes` tolerates trailing zeros in the
    // 512-byte receive buffer left over from previous messages.
    let req = match api::ipc::decode::<api::ipc::VfsRequest>(buf) {
        Ok(r) => r,
        Err(_) => return api::ipc::VfsResponse::Err(0xFF), // malformed request
    };

    match req {
        api::ipc::VfsRequest::GetFile(p) => {
            if let Some((ptr, len)) = vfs.get_file_ptr(p) {
                api::ipc::VfsResponse::DataPtr {
                    ptr: ptr as u64,
                    len: len as u64,
                }
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::ListDir(p) => {
            let n = vfs.list_dir(p, resp_buf);
            api::ipc::VfsResponse::Data(&resp_buf[..n])
        }

        api::ipc::VfsRequest::Stat(p) => match vfs.stat(p) {
            Some((size, is_dir)) => api::ipc::VfsResponse::Stat { size, is_dir },
            None => api::ipc::VfsResponse::Err(1),
        },

        api::ipc::VfsRequest::Write { path, content } => {
            // Access check: only authorized cells may write to this path.
            if !vfs.access.can_write(caller, path) {
                return api::ipc::VfsResponse::Err(3); // 3 = PermissionDenied
            }
            // Capture size of any existing file to release its quota share.
            // Overwriting an existing file should charge the delta, not the
            // full new size — otherwise repeated overwrites inflate usage.
            let old_size = vfs.file_size(path);
            let new_size = content.len() as u64;
            // Net quota delta: may be negative (file shrunk) or positive.
            let net_charge = new_size.saturating_sub(old_size);
            if net_charge > 0 && !vfs.quota.can_charge(caller, net_charge) {
                return api::ipc::VfsResponse::Err(2); // 2 = quota exceeded
            }
            if vfs.write(path, content) {
                // Release old bytes and charge new size.
                vfs.quota.release(caller, old_size);
                let _ = vfs.quota.charge(caller, new_size);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::Append { path, content } => {
            if !vfs.access.can_write(caller, path) {
                return api::ipc::VfsResponse::Err(3);
            }
            let append_len = content.len() as u64;
            if !vfs.quota.can_charge(caller, append_len) {
                return api::ipc::VfsResponse::Err(2); // quota exceeded
            }
            if vfs.append(path, content) {
                let _ = vfs.quota.charge(caller, append_len);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::Mkdir(p) => {
            if !vfs.access.can_write(caller, p) {
                api::ipc::VfsResponse::Err(3)
            } else if vfs.mkdir(p) {
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::Rmdir(p) => {
            // Destructive: authorize before touching the backend.  A path the caller
            // may not write is a path it may not delete.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(3);
            }
            // Verifies the target IS a directory — POSIX ENOTDIR semantics.
            if vfs.rmdir(p) {
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::Unlink(p) => {
            // Authorize BEFORE `file_size`: an unauthorized caller must learn nothing,
            // not even whether the file exists or how large it is.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(3);
            }
            // Capture file size before deletion for quota release.
            let file_size = vfs.file_size(p);
            if vfs.unlink(p) {
                // Release the quota that was charged when the file was written.
                vfs.quota.release(caller, file_size);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::RmdirRecursive(p) => {
            // Authorize BEFORE `collect_dir_bytes`: that walk lists the whole subtree,
            // so checking after it would leave a directory-size probe open to callers
            // who may not write the path.
            if !vfs.access.can_write(caller, p) {
                return api::ipc::VfsResponse::Err(3);
            }
            // Compute bytes to release BEFORE deletion: rmdir_recursive returns
            // only bool (adding a bytes-freed value to FsBackend would be a Law 1
            // ABI change).  Walk the subtree via list+file_size while it still exists.
            let freed = collect_dir_bytes(vfs, p, 32);
            if vfs.rmdir_recursive(p) {
                vfs.quota.release(caller, freed);
                api::ipc::VfsResponse::Ok
            } else {
                api::ipc::VfsResponse::Err(1)
            }
        }

        api::ipc::VfsRequest::ReadAsync { path } => {
            // Read file data synchronously (disk is still blocking in this backend).
            // Store under a handle and return immediately — caller polls.
            // The handle is bound to the requesting cell here; only it can poll.
            let data = vfs.read_to_vec(path);
            let handle = vfs.pending.insert(caller, data);
            api::ipc::VfsResponse::PendingHandle(handle)
        }

        api::ipc::VfsRequest::Poll { handle } => {
            // With a synchronous backend data is always ready on first poll.
            // A handle owned by another cell is refused with the same Err(4) as a
            // stale one, so sweeping handles reveals nothing about other cells.
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
                None => api::ipc::VfsResponse::Err(4), // 4 = stale/unknown handle
            }
        }

        // ── Zero-Copy Grant I/O (Storage 2.0, Phase 02) ────────────────
        api::ipc::VfsRequest::ReadGrant {
            cap,
            offset,
            size,
            grant,
        } => {
            // Validate: VFS must have been GrantShare'd access by the app.
            match ostd::syscall::sys_grant_slice(grant) {
                None => api::ipc::VfsResponse::Err(1), // no access
                Some(ptr) => {
                    // Look up the cap in the VFS handle table.  A cap owned by
                    // another cell reports zero bytes, exactly like an unknown
                    // cap — the caller cannot tell the two apart.
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
            api::ipc::VfsResponse::Err(3)
        }

        api::ipc::VfsRequest::ReadFileGrant { path, grant, max } => {
            match ostd::syscall::sys_grant_slice(grant) {
                None => api::ipc::VfsResponse::Err(1), // grant not shared to VFS
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

/// Walk the subtree rooted at `path` and return the total bytes occupied by all
/// regular files inside it, bounded to `depth` recursion levels.
///
/// Called by `RmdirRecursive` BEFORE the delete so quota can be released after.
/// Uses 512-byte stack buffers per level (adequate for embedded directory sizes).
fn collect_dir_bytes(vfs: &VfsManager, path: &str, depth: u8) -> u64 {
    if depth == 0 {
        return 0;
    }
    let mut scratch = [0u8; 512];
    let n = vfs.list_dir(path, &mut scratch);
    let listing = core::str::from_utf8(&scratch[..n]).unwrap_or("");
    let base = path.trim_end_matches('/');
    let mut total = 0u64;
    for line in listing.split('\n') {
        if let Some(name) = line.strip_prefix("f:") {
            let mut child = alloc::string::String::with_capacity(base.len() + 1 + name.len());
            child.push_str(base);
            child.push('/');
            child.push_str(name);
            total += vfs.file_size(&child);
        } else if let Some(name) = line.strip_prefix("d:") {
            let mut child = alloc::string::String::with_capacity(base.len() + 1 + name.len());
            child.push_str(base);
            child.push('/');
            child.push_str(name);
            total += collect_dir_bytes(vfs, &child, depth - 1);
        }
    }
    total
}
