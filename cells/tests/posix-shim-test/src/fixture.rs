//! Working-directory setup and file fixtures for the path smokes.
//!
//! The shell launches this cell with a cwd grant, so VFS seals it: path-string
//! requests are refused and the granted directory is its only reach into the
//! tree. `install_working_dir` hands that grant to the POSIX shim; `create`
//! writes fixtures through the same grant, independently of the shim under test.

use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use ostd::service::VfsRef;

/// Give the shim this cell's granted working directory, if it was launched
/// with one. Returns whether the cell is sealed (a grant was installed).
pub(super) fn install_working_dir() -> bool {
    let Ok(context) = ostd::child_context::ChildContext::read_own() else {
        return false;
    };
    let (Some(grant), Ok(cwd)) = (context.cwd_grant(), core::str::from_utf8(context.cwd())) else {
        return false;
    };
    api::posix::install_working_dir(ViDirHandle(grant.id), cwd)
}

/// Create or overwrite the file at absolute `path` with `content`.
pub(super) fn create(path: &str, content: &[u8]) -> bool {
    let Some((cwd_dir, cwd)) = api::posix::working_dir() else {
        // An unsealed cell still names the path.
        return ostd::clients::VfsClient::new()
            .write_file(path, content)
            .is_ok();
    };
    let Some(relative) = path
        .strip_prefix(cwd.trim_end_matches('/'))
        .filter(|rest| rest.starts_with('/'))
    else {
        return false;
    };
    let mut names = relative
        .split('/')
        .filter(|name| !name.is_empty())
        .peekable();
    let mut vfs = VfsRef::new();
    let mut dir = cwd_dir;
    // First handle derived from the grant; closing it revokes the rest.
    let mut derived = None;
    let created = loop {
        let Some(name) = names.next() else {
            break false;
        };
        let mut reply = [0u8; IPC_BUF_SIZE];
        if names.peek().is_none() {
            let req = VfsRequest::WriteAt { dir, name, content };
            break matches!(vfs.call(&req, &mut reply), Ok(VfsResponse::Ok));
        }
        match vfs.call(&VfsRequest::OpenDir { dir, name }, &mut reply) {
            Ok(VfsResponse::DirHandle(sub)) => {
                derived.get_or_insert(sub);
                dir = sub;
            }
            _ => break false,
        }
    };
    if let Some(first) = derived {
        let mut reply = [0u8; IPC_BUF_SIZE];
        let _ = vfs.call::<_, VfsResponse>(&VfsRequest::CloseDir { dir: first }, &mut reply);
    }
    created
}
