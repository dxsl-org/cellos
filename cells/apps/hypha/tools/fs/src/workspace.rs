//! Workspace confinement for `tool-fs`.
//!
//! WHY: every path `tool-fs` receives was chosen by LLM output that crossed the
//! network, and the cell holds VfsMutate. Unconfined, `read_file`/`write_file`
//! are an arbitrary read/write of the whole VFS (`/bin`, policy, keys). Every
//! request is therefore confined to one workspace directory.
//!
//! Invariant: an accepted path is [`WORKSPACE_ROOT`] or
//! `WORKSPACE_ROOT/<c1>/…/<cn>`, where every component is non-empty, is not `.`
//! or `..`, and the whole path is printable ASCII without `\`. The check is
//! purely lexical and nothing is normalised afterwards, so the string checked is
//! byte-for-byte the string VFS receives (minus at most one trailing `/`).

/// The only directory tree `tool-fs` reads, lists or writes.
pub const WORKSPACE_ROOT: &str = "/data/hypha";
/// Generous for a workspace-relative name, far below one IPC message.
pub const MAX_PATH_BYTES: usize = 192;

/// Why a path was refused; reported back to `core` verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRefusal {
    Empty,
    TooLong,
    NotAbsolute,
    /// Non-printable, non-ASCII or `\` (a JSON escape the args parser does not
    /// decode, so VFS would see something other than what was displayed).
    BadByte,
    /// Empty, `.` or `..` component.
    BadComponent,
    OutsideWorkspace,
}

/// Accept `path` only if it names [`WORKSPACE_ROOT`] or something beneath it.
/// Returns the path with one trailing `/` removed.
pub fn confine(path: &str) -> Result<&str, PathRefusal> {
    if path.is_empty() {
        return Err(PathRefusal::Empty);
    }
    if path.len() > MAX_PATH_BYTES {
        return Err(PathRefusal::TooLong);
    }
    if !path.starts_with('/') {
        return Err(PathRefusal::NotAbsolute);
    }
    if path
        .bytes()
        .any(|b| !(0x20..0x7f).contains(&b) || b == b'\\')
    {
        return Err(PathRefusal::BadByte);
    }
    let path = match path.strip_suffix('/') {
        Some(trimmed) if !trimmed.is_empty() => trimmed,
        _ => path,
    };
    if path[1..]
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(PathRefusal::BadComponent);
    }
    let inside = path == WORKSPACE_ROOT
        || path
            .strip_prefix(WORKSPACE_ROOT)
            .is_some_and(|rest| rest.starts_with('/'));
    if inside {
        Ok(path)
    } else {
        Err(PathRefusal::OutsideWorkspace)
    }
}
