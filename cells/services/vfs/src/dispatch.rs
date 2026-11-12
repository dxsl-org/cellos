//! Typed IPC request dispatch — decodes one `VfsRequest` and produces the
//! `VfsResponse`, routing filesystem ops through the MountTable.
//!
//! Cross-cutting policy lives here, not in backends: AccessTable authorization,
//! quota accounting (net-delta on overwrite), async-read pending table, and
//! zero-copy grant I/O.

use api::services::proto::vfs::VfsError;

use crate::caller::Caller;
use crate::manager::VfsManager;

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
        return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
    };

    // Settle what the kernel says this cell inherited before acting on anything
    // it sent. The answer decides both what it holds and whether it may still
    // name a path, and a cell that reached its first request before the second
    // answer landed would be one path-string operation ahead of the seal.
    crate::dir_admission::admit(vfs, caller);
    if !crate::prepared_cwd::ready(vfs, caller) {
        return api::ipc::VfsResponse::Err(VfsError::Io as u8);
    }

    // Decode typed request; `take_from_bytes` tolerates trailing bytes in the
    // receive buffer — both zeros left by a previous message and the kernel's
    // caller-identity trailer at the very end.
    let req = match api::ipc::decode::<api::ipc::VfsRequest>(buf) {
        Ok(r) => r,
        Err(_) => return api::ipc::VfsResponse::Err(VfsError::Malformed as u8),
    };

    if let api::ipc::VfsRequest::PreparedChildV1(record) = req {
        return crate::prepared_cwd::handle(vfs, caller, record, resp_buf);
    }

    // Centrally deny any mutating request from a caller lacking VfsMutate authority
    // BEFORE path sealing, handle resolution, or backend work. The only exception
    // is init's machine pin store: `Write`/`Mkdir` inside `/srv/.cellos-machine`
    // from the live `MACHINE_PIN` provider (see `AccessTable::is_machine_pin_writer`).
    if req.requires_mutation_authority()
        && !caller.may_mutate()
        && !machine_pin_write(vfs, caller, &req)
    {
        return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
    }
    // A cell that has given up path strings is refused here, before the request
    // reaches an arm that could serve it. Refusing at the entry rather than in
    // each arm is what makes the guarantee hold for every path-addressed
    // operation, including any added later.
    if req.is_path_addressed() && vfs.dirs.is_sealed(caller) {
        return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
    }

    // A source may not disappear between issuer sealing and kernel commit.
    // Abort-if-uncommitted linearizes against commit before the revoke walks
    // the existing directory lineage.
    if let api::ipc::VfsRequest::CloseDir { dir } = req {
        if !crate::prepared_cwd::before_revoke(vfs, caller, dir) {
            return api::ipc::VfsResponse::Err(VfsError::Io as u8);
        }
    }

    if let Some(resp) = crate::trust::handle(vfs, caller, &req) {
        return resp;
    }
    if let Some(resp) = crate::save::handle(vfs, caller, &req) {
        return resp;
    }
    if let Some(resp) = crate::dispatch_paths::handle_path_request(vfs, caller, &req) {
        return resp;
    }

    match req {
        api::ipc::VfsRequest::ListDir(p) => {
            if !vfs.access.can_read(caller, p) {
                return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
            }
            if !vfs.healthy(p) {
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
            }
            if !matches!(vfs.stat(p), Some((_, true))) {
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
            }
            let n = vfs.list_dir(p, resp_buf);
            if !vfs.healthy(p) {
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
            }
            api::ipc::VfsResponse::Data(&resp_buf[..n])
        }
        api::ipc::VfsRequest::ReadAsync { path } => {
            // Authorize BEFORE reading: the read happens now (the disk backend is
            // still blocking) and `Poll` only hands over what was already read, so
            // this is the only point where the path is known.
            if !vfs.access.can_read(caller, path) {
                return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
            }
            // A handle records durable state against the caller, so it needs a
            // caller that cannot be confused with a later one.
            if !caller.may_own_state() {
                return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
            }
            if !vfs.healthy(path) || !matches!(vfs.stat(path), Some((_, false))) {
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
            }
            let lease = if path.starts_with("/srv") {
                let Ok(k) = crate::namespace::NamespaceKey::parse(path) else {
                    return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
                };
                let Ok(l) = vfs.ledger.acquire_service_handle(&k) else {
                    return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
                };
                Some(l)
            } else {
                None
            };
            let data = vfs.read_to_vec(path);
            if !vfs.healthy(path) {
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
            }
            let handle = vfs.pending.insert(caller, path, data, lease);
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
                None => return api::ipc::VfsResponse::Err(VfsError::Handle as u8),
            };
            if !readable {
                return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
            }
            if !vfs
                .pending
                .owned_path(caller, handle)
                .is_some_and(|path| vfs.healthy(path))
            {
                let _ = vfs.pending.poll(caller, handle);
                return api::ipc::VfsResponse::Err(VfsError::Io as u8);
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
                None => api::ipc::VfsResponse::Err(VfsError::Handle as u8),
            }
        }

        // ── Zero-Copy Grant I/O (Storage 2.0) ──────────────────────────────
        api::ipc::VfsRequest::ReadGrant {
            cap,
            offset,
            size,
            grant,
        } => crate::grant_read::read_grant(vfs, caller, cap, offset, size, grant),

        api::ipc::VfsRequest::WriteGrant {
            cap,
            offset,
            grant,
            bytes,
        } => {
            if !caller.may_mutate() {
                return api::ipc::VfsResponse::Err(VfsError::Denied as u8);
            }
            crate::grant_write::write(vfs, caller, cap, offset, grant, bytes)
        }

        api::ipc::VfsRequest::ReadFileGrant { path, grant, max } => {
            crate::grant_read::read_file_grant(vfs, caller, path, grant, max)
        }

        // ── Directory capabilities ──────────────────────────────────────────
        // Grouped and delegated: these arms share a resolution step that has no
        // counterpart above, and interleaving them would put the two addressing
        // models in one match where a reader has to check which one each arm is
        // in.
        api::ipc::VfsRequest::WriteHandleGrant { .. }
        | api::ipc::VfsRequest::SyncHandle { .. }
        | api::ipc::VfsRequest::WriteAt { .. }
        | api::ipc::VfsRequest::UnlinkAt { .. }
        | api::ipc::VfsRequest::MkdirAt { .. }
        | api::ipc::VfsRequest::RmdirAt { .. }
        | api::ipc::VfsRequest::RenameAt { .. }
            if !caller.may_mutate() =>
        {
            api::ipc::VfsResponse::Err(VfsError::Denied as u8)
        }

        api::ipc::VfsRequest::OpenRootDir { .. }
        | api::ipc::VfsRequest::OpenDir { .. }
        | api::ipc::VfsRequest::ReadAt { .. }
        | api::ipc::VfsRequest::WriteAt { .. }
        | api::ipc::VfsRequest::StatAt { .. }
        | api::ipc::VfsRequest::ListAt { .. }
        | api::ipc::VfsRequest::ListAtPage { .. }
        | api::ipc::VfsRequest::FileHandleState { .. }
        | api::ipc::VfsRequest::UnlinkAt { .. }
        | api::ipc::VfsRequest::CloseDir { .. }
        | api::ipc::VfsRequest::SealPaths
        | api::ipc::VfsRequest::OpenFileAt { .. }
        | api::ipc::VfsRequest::ReadFileHandle { .. }
        | api::ipc::VfsRequest::CloseFile { .. }
        | api::ipc::VfsRequest::ReadHandleGrant { .. }
        | api::ipc::VfsRequest::WriteHandleGrant { .. }
        | api::ipc::VfsRequest::SyncHandle { .. }
        | api::ipc::VfsRequest::MkdirAt { .. }
        | api::ipc::VfsRequest::RmdirAt { .. }
        | api::ipc::VfsRequest::RenameAt { .. } => {
            crate::dispatch_dirs::handle(vfs, caller, &req, resp_buf)
        }
        _ => api::ipc::VfsResponse::Err(VfsError::Malformed as u8),
    }
}

/// Whether `req` is the narrow machine pin exception to the central mutate
/// gate: a path `Write` or `Mkdir` that `AccessTable::is_machine_pin_writer`
/// admits. Any other request kind, path or caller still needs the mutate flag.
fn machine_pin_write(vfs: &VfsManager, caller: Caller, req: &api::ipc::VfsRequest<'_>) -> bool {
    match *req {
        api::ipc::VfsRequest::Write { path, .. } | api::ipc::VfsRequest::Mkdir(path) => {
            vfs.access.is_machine_pin_writer(caller, path)
        }
        _ => false,
    }
}
