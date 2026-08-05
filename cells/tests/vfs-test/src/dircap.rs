//! Directory capabilities, exercised from a cell's own side.
//!
//! This cell is the first to migrate, and it migrates completely: it acquires
//! the directories it needs, works entirely through handles, and then gives up
//! the ability to name a path at all. The last part is the point. Traversal
//! assertions on the handle operations prove only that the new interface is
//! tight; a cell can pass every one of them while still sending absolute paths
//! through the old interface. The assertion that matters is the one after
//! sealing, where a plain `Write { path }` comes back refused.
//!
//! Runs last, because sealing is one-way and every scenario before it needs the
//! path-string operations.

use api::dir_handles::ViDirHandle;
use api::ipc::{VfsRequest, VfsResponse};

use crate::grant_io;
use crate::{fail, pass, vfs_req};

/// `types::ViError::PermissionDenied`.
const DENIED: u8 = 3;
/// Unknown, stale, or not-this-caller's handle.
const BAD_HANDLE: u8 = 4;
/// The service could not decode the message at all.
const MALFORMED: u8 = 0xFF;

/// Directory this cell builds and then reaches only through a handle.
const WORKDIR: &str = "/tmp/dircap";
const READ_FILE_GRANT_SOURCE: &str = "/tmp/grant-read.txt";

pub fn run() {
    let Some(root) = acquire("/tmp") else {
        return;
    };
    reject_every_traversal_shape(root);

    // Built through the old interface on purpose: a cell bootstraps the
    // directories it needs and only then narrows itself to them.
    let _ = vfs_req(&VfsRequest::Mkdir(WORKDIR));

    let Some(work) = derive(root, "dircap") else {
        return;
    };
    file_operations_through_a_handle(work);
    revoking_a_parent_revokes_what_came_from_it(root, work);

    // Re-acquire: the revocation above took both handles with it, which is
    // exactly what it was asserting.
    let Some(root) = acquire("/tmp") else {
        return;
    };
    let Some(work) = derive(root, "dircap") else {
        return;
    };
    let _ = vfs_req(&VfsRequest::UnlinkAt {
        dir: work,
        name: "note.txt",
    });

    seal_and_prove_paths_are_refused(work);
}

// ── Acquisition ──────────────────────────────────────────────────────────────

fn acquire(path: &str) -> Option<ViDirHandle> {
    match vfs_req(&VfsRequest::OpenRootDir { path }) {
        VfsResponse::DirHandle(h) => {
            pass("dircap: acquiring /tmp yields a directory handle");
            Some(h)
        }
        _ => {
            fail("dircap: acquiring /tmp yields a directory handle");
            None
        }
    }
}

fn derive(parent: ViDirHandle, name: &str) -> Option<ViDirHandle> {
    match vfs_req(&VfsRequest::OpenDir { dir: parent, name }) {
        VfsResponse::DirHandle(h) => {
            pass("dircap: deriving a subdirectory handle");
            Some(h)
        }
        _ => {
            fail("dircap: deriving a subdirectory handle");
            None
        }
    }
}

// ── Nothing outside the handle can be named ──────────────────────────────────

/// Each shape gets its own assertion: they are refused for different reasons,
/// and a single loop would hide which reason stopped firing.
fn reject_every_traversal_shape(dir: ViDirHandle) {
    refused(dir, "..", "dircap: `..` is refused");
    refused(dir, "../..", "dircap: `../..` is refused");
    refused(dir, "/abs", "dircap: an absolute name is refused");
    refused(dir, "a/../../b", "dircap: an embedded traversal is refused");
    refused(dir, ".", "dircap: `.` is refused");
    refused(dir, "", "dircap: an empty name is refused");
    refused(dir, "..\\..\\etc", "dircap: a backslash name is refused");
    refused(dir, "note\ntxt", "dircap: a control byte is refused");
    reject_odd_utf8(dir);
}

fn refused(dir: ViDirHandle, name: &str, msg: &str) {
    // Asserted on every operation that takes a name, not just one: they resolve
    // through the same check, and a single call site would stop proving that if
    // one of them ever stopped using it.
    let attempts = [
        VfsRequest::ReadAt { dir, name },
        VfsRequest::StatAt { dir, name },
        VfsRequest::UnlinkAt { dir, name },
        VfsRequest::OpenDir { dir, name },
        VfsRequest::WriteAt {
            dir,
            name,
            content: b"x",
        },
    ];
    for attempt in &attempts {
        match vfs_req(attempt) {
            VfsResponse::Err(DENIED) => {}
            _ => {
                fail(msg);
                return;
            }
        }
    }
    pass(msg);
}

/// A name that is not valid UTF-8 cannot be built in Rust, so it is put on the
/// wire by hand. This is the one traversal shape the type system cannot stop a
/// caller from sending, and an overlong encoding of `/` is what a decoder that
/// repaired invalid input would turn into a separator.
fn reject_odd_utf8(dir: ViDirHandle) {
    const READ_AT_VARIANT: u8 = 16;
    if dir.0 >= 128 {
        fail("dircap: odd UTF-8 is refused (handle too large to hand-encode)");
        return;
    }
    // ReadAt { dir, name }: variant, handle as a varint, then a 2-byte name.
    let msg = [READ_AT_VARIANT, dir.0 as u8, 2, 0xC0, 0xAF];
    match crate::vfs_raw(&msg) {
        // Refused at decode or refused at resolve; either way the backend never
        // saw it.
        VfsResponse::Err(MALFORMED) | VfsResponse::Err(DENIED) => {
            pass("dircap: odd UTF-8 in a name is refused")
        }
        _ => fail("dircap: odd UTF-8 in a name is refused"),
    }
}

// ── Ordinary work, through the handle only ───────────────────────────────────

fn file_operations_through_a_handle(dir: ViDirHandle) {
    match vfs_req(&VfsRequest::WriteAt {
        dir,
        name: "note.txt",
        content: b"cap",
    }) {
        VfsResponse::Ok => pass("dircap: WriteAt creates a file inside the handle"),
        _ => fail("dircap: WriteAt creates a file inside the handle"),
    }

    match vfs_req(&VfsRequest::StatAt {
        dir,
        name: "note.txt",
    }) {
        VfsResponse::Stat {
            size: 3,
            is_dir: false,
        } => pass("dircap: StatAt reports the file it just wrote"),
        _ => fail("dircap: StatAt reports the file it just wrote"),
    }

    match vfs_req(&VfsRequest::ReadAt {
        dir,
        name: "note.txt",
    }) {
        VfsResponse::Data(b"cap") => pass("dircap: ReadAt returns the bytes written"),
        _ => fail("dircap: ReadAt returns the bytes written"),
    }

    match vfs_req(&VfsRequest::ListAt { dir }) {
        VfsResponse::Data(bytes) => {
            if bytes.windows(10).any(|w| w == b"f:note.txt") {
                pass("dircap: ListAt lists the handle's own directory");
            } else {
                fail("dircap: ListAt lists the handle's own directory");
            }
        }
        _ => fail("dircap: ListAt lists the handle's own directory"),
    }

    // The handle resolved where it claimed to: the same file is visible at the
    // absolute path, which is still reachable because this cell has not sealed.
    match vfs_req(&VfsRequest::Stat("/tmp/dircap/note.txt")) {
        VfsResponse::Stat { size: 3, .. } => {
            pass("dircap: the handle resolved inside the directory it names")
        }
        _ => fail("dircap: the handle resolved inside the directory it names"),
    }
}

// ── Revocation reaches what was derived ──────────────────────────────────────

fn revoking_a_parent_revokes_what_came_from_it(parent: ViDirHandle, derived: ViDirHandle) {
    match vfs_req(&VfsRequest::CloseDir { dir: parent }) {
        VfsResponse::Ok => pass("dircap: a handle can be given up"),
        _ => fail("dircap: a handle can be given up"),
    }
    match vfs_req(&VfsRequest::StatAt {
        dir: derived,
        name: "note.txt",
    }) {
        VfsResponse::Err(BAD_HANDLE) => {
            pass("dircap: revoking a handle revokes what was derived from it")
        }
        _ => fail("dircap: revoking a handle revokes what was derived from it"),
    }
}

// ── The migration is only real once paths stop working ───────────────────────

fn seal_and_prove_paths_are_refused(work: ViDirHandle) {
    let read_file_grant_ready = matches!(
        vfs_req(&VfsRequest::Write {
            path: READ_FILE_GRANT_SOURCE,
            content: b"grant-copy-bytes"
        }),
        VfsResponse::Ok
    );

    match grant_io::read_file_into_short_grant(READ_FILE_GRANT_SOURCE, 5) {
        Ok((grant, bytes))
            if bytes == grant.len() && grant_io::grant_prefix_equals(&grant, b"grant") =>
        {
            pass("grant: ReadFileGrant clamps to grant length");
        }
        _ => fail("grant: ReadFileGrant clamps to grant length"),
    }

    let read_file_grant = match grant_io::read_file_into_grant(READ_FILE_GRANT_SOURCE) {
        Ok((grant, bytes))
            if bytes == grant.len()
                && bytes > 0
                && grant_io::grant_prefix_equals(&grant, b"grant-copy-bytes") =>
        {
            pass("grant: ReadFileGrant copies nonzero bytes");
            Some(grant)
        }
        _ => {
            fail("grant: ReadFileGrant copies nonzero bytes");
            None
        }
    };

    match vfs_req(&VfsRequest::SealPaths) {
        VfsResponse::Ok => pass("dircap: the cell gives up naming paths"),
        _ => {
            fail("dircap: the cell gives up naming paths");
            return;
        }
    }

    // The criterion this whole phase is measured by.
    match vfs_req(&VfsRequest::Write {
        path: "/tmp/after_seal.txt",
        content: b"should not land",
    }) {
        VfsResponse::Err(DENIED) => pass("dircap: Write{path} is refused after sealing"),
        _ => fail("dircap: Write{path} is refused after sealing"),
    }
    // Nothing landed, either — the refusal is before the backend, not a reply
    // written after the fact.
    match vfs_req(&VfsRequest::StatAt {
        dir: work,
        name: "after_seal.txt",
    }) {
        VfsResponse::Err(_) => pass("dircap: the refused write left nothing behind"),
        _ => fail("dircap: the refused write left nothing behind"),
    }

    // Every other path-addressed operation, so the refusal is not one arm.
    for (req, msg) in [
        (
            VfsRequest::GetFile("/tmp/volatile.txt"),
            "dircap: GetFile is refused after sealing",
        ),
        (
            VfsRequest::Stat("/tmp/volatile.txt"),
            "dircap: Stat is refused after sealing",
        ),
        (
            VfsRequest::ListDir("/tmp"),
            "dircap: ListDir is refused after sealing",
        ),
        (
            VfsRequest::Unlink("/tmp/volatile.txt"),
            "dircap: Unlink is refused after sealing",
        ),
        (
            VfsRequest::Mkdir("/tmp/after_seal"),
            "dircap: Mkdir is refused after sealing",
        ),
        (
            VfsRequest::ReadAsync {
                path: "/tmp/volatile.txt",
            },
            "dircap: ReadAsync is refused after sealing",
        ),
        (
            VfsRequest::OpenRootDir { path: "/" },
            "dircap: widening by acquiring a new root is refused after sealing",
        ),
    ] {
        match vfs_req(&req) {
            VfsResponse::Err(DENIED) => pass(msg),
            _ => fail(msg),
        }
    }

    match (read_file_grant_ready, read_file_grant.as_ref()) {
        (true, Some(grant)) => match vfs_req(&VfsRequest::ReadFileGrant {
            path: READ_FILE_GRANT_SOURCE,
            grant: grant.id(),
            max: grant.len(),
        }) {
            VfsResponse::Err(DENIED) => pass("grant: ReadFileGrant is refused after sealing"),
            _ => fail("grant: ReadFileGrant is refused after sealing"),
        },
        _ => fail("grant: ReadFileGrant is refused after sealing"),
    }

    // And the cell still works: sealing removed one way of naming things, not
    // the cell's access to what it holds.
    match vfs_req(&VfsRequest::WriteAt {
        dir: work,
        name: "sealed.txt",
        content: b"ok",
    }) {
        VfsResponse::Ok => pass("dircap: handle operations still work after sealing"),
        _ => fail("dircap: handle operations still work after sealing"),
    }
    match vfs_req(&VfsRequest::UnlinkAt {
        dir: work,
        name: "sealed.txt",
    }) {
        VfsResponse::Ok => pass("dircap: UnlinkAt still works after sealing"),
        _ => fail("dircap: UnlinkAt still works after sealing"),
    }
}
