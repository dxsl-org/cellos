// SPDX-License-Identifier: Apache-2.0
//! Host tests for the rules that decide what a directory handle may resolve.
//!
//! Each traversal shape is asserted on its own rather than through a loop over a
//! list, because the shapes fail for different reasons and a single loop hides
//! which reason fired. A rule that rejects `..` for the wrong reason still
//! rejects `..`, right up until the reason stops applying.

#![cfg(test)]

use super::dir_name::{
    join_component, validate_dir_component, validate_dir_path, DirNameError, MAX_DIR_NAME_LEN,
    MAX_DIR_PATH_LEN,
};
use super::ipc::VfsRequest;

// ── Component rules ──────────────────────────────────────────────────────────

#[test]
fn a_plain_name_is_accepted_unchanged() {
    assert_eq!(validate_dir_component(b"report.txt"), Ok("report.txt"));
}

#[test]
fn parent_is_rejected() {
    assert_eq!(
        validate_dir_component(b".."),
        Err(DirNameError::Traversal),
        ".. must never resolve"
    );
}

#[test]
fn repeated_parent_is_rejected_as_a_separator_not_as_traversal() {
    // "../.." carries a separator, so it dies before the `..` comparison is even
    // reached. Both refusals matter: the separator rule is what makes rejecting
    // the single `..` sufficient.
    assert_eq!(
        validate_dir_component(b"../.."),
        Err(DirNameError::Separator)
    );
}

#[test]
fn an_absolute_path_is_not_a_component() {
    assert_eq!(
        validate_dir_component(b"/abs"),
        Err(DirNameError::Separator)
    );
    assert_eq!(
        validate_dir_component(b"/etc/shadow"),
        Err(DirNameError::Separator)
    );
}

#[test]
fn an_embedded_traversal_is_rejected() {
    assert_eq!(
        validate_dir_component(b"a/../../b"),
        Err(DirNameError::Separator)
    );
}

#[test]
fn current_directory_is_rejected() {
    assert_eq!(validate_dir_component(b"."), Err(DirNameError::CurrentDir));
}

#[test]
fn an_empty_name_is_rejected() {
    assert_eq!(validate_dir_component(b""), Err(DirNameError::Empty));
}

#[test]
fn a_backslash_is_a_separator_here_even_though_no_backend_treats_it_as_one() {
    assert_eq!(
        validate_dir_component(b"..\\..\\etc"),
        Err(DirNameError::Separator)
    );
}

#[test]
fn odd_utf8_never_becomes_a_separator() {
    // 0xC0 0xAF is the overlong encoding of '/'. It is invalid UTF-8, so it is
    // refused outright — a decoder that "helpfully" repaired it would have
    // produced the separator this layer exists to exclude.
    assert_eq!(
        validate_dir_component(&[0xC0, 0xAF]),
        Err(DirNameError::NotUtf8)
    );
    // A lone continuation byte, and a truncated multi-byte sequence.
    assert_eq!(validate_dir_component(&[0x80]), Err(DirNameError::NotUtf8));
    assert_eq!(
        validate_dir_component(&[0xE2, 0x82]),
        Err(DirNameError::NotUtf8)
    );
}

#[test]
fn legitimate_multibyte_names_still_work() {
    assert_eq!(
        validate_dir_component("tài-liệu".as_bytes()),
        Ok("tài-liệu")
    );
}

#[test]
fn a_nul_byte_is_rejected_because_a_c_backend_would_stop_reading_there() {
    assert_eq!(
        validate_dir_component(b"safe\0/../etc"),
        Err(DirNameError::Control)
    );
    assert_eq!(validate_dir_component(b"a\0b"), Err(DirNameError::Control));
}

#[test]
fn control_bytes_and_delete_are_rejected() {
    assert_eq!(validate_dir_component(b"a\nb"), Err(DirNameError::Control));
    assert_eq!(
        validate_dir_component(b"a\x7Fb"),
        Err(DirNameError::Control)
    );
}

#[test]
fn a_name_longer_than_the_limit_is_rejected_rather_than_truncated() {
    let long = vec![b'a'; MAX_DIR_NAME_LEN + 1];
    assert_eq!(
        validate_dir_component(&long),
        Err(DirNameError::TooLong),
        "truncating would silently address a different entry"
    );
    let at_limit = vec![b'a'; MAX_DIR_NAME_LEN];
    assert!(validate_dir_component(&at_limit).is_ok());
}

#[test]
fn names_that_merely_start_with_dots_are_ordinary_names() {
    // Only the exact forms are traversal; "..." and "..a" name real entries and
    // cannot leave the directory because they carry no separator.
    assert_eq!(validate_dir_component(b"..."), Ok("..."));
    assert_eq!(validate_dir_component(b"..hidden"), Ok("..hidden"));
    assert_eq!(validate_dir_component(b".config"), Ok(".config"));
}

// ── Root-path rules ──────────────────────────────────────────────────────────

#[test]
fn the_root_itself_is_a_valid_root_path() {
    assert_eq!(validate_dir_path(b"/"), Ok("/"));
}

#[test]
fn a_relative_root_path_is_rejected() {
    assert_eq!(validate_dir_path(b"tmp"), Err(DirNameError::NotAbsolute));
    assert_eq!(validate_dir_path(b""), Err(DirNameError::Empty));
}

#[test]
fn traversal_inside_a_root_path_is_rejected() {
    assert_eq!(validate_dir_path(b"/tmp/.."), Err(DirNameError::Traversal));
    assert_eq!(
        validate_dir_path(b"/tmp/../../srv"),
        Err(DirNameError::Traversal)
    );
    assert_eq!(validate_dir_path(b"/tmp/."), Err(DirNameError::CurrentDir));
}

#[test]
fn a_doubled_or_trailing_separator_is_rejected_rather_than_collapsed() {
    // Collapsing them would be normalisation, and normalising before checking is
    // the mistake this whole layer is arranged to avoid.
    assert_eq!(validate_dir_path(b"//tmp"), Err(DirNameError::Empty));
    assert_eq!(validate_dir_path(b"/tmp//x"), Err(DirNameError::Empty));
    assert_eq!(validate_dir_path(b"/tmp/"), Err(DirNameError::Empty));
}

#[test]
fn an_over_long_root_path_is_rejected() {
    let mut long = vec![b'/'];
    long.extend(std::iter::repeat(b'a').take(MAX_DIR_PATH_LEN));
    assert_eq!(validate_dir_path(&long), Err(DirNameError::TooLong));
}

#[test]
fn a_nested_root_path_is_accepted() {
    assert_eq!(validate_dir_path(b"/tmp/dircap"), Ok("/tmp/dircap"));
}

// ── Joining ──────────────────────────────────────────────────────────────────

#[test]
fn joining_adds_exactly_one_component() {
    assert_eq!(join_component("/tmp", "x"), "/tmp/x");
    assert_eq!(join_component("/", "x"), "/x");
    assert_eq!(join_component("/tmp/a", "b"), "/tmp/a/b");
}

// ── Which requests a sealed cell may still send ──────────────────────────────

#[test]
fn every_path_naming_request_is_classified_as_path_addressed() {
    assert!(VfsRequest::GetFile("/x").is_path_addressed());
    assert!(VfsRequest::ListDir("/x").is_path_addressed());
    assert!(VfsRequest::Stat("/x").is_path_addressed());
    assert!(VfsRequest::Write {
        path: "/x",
        content: b""
    }
    .is_path_addressed());
    assert!(VfsRequest::Append {
        path: "/x",
        content: b""
    }
    .is_path_addressed());
    assert!(VfsRequest::Mkdir("/x").is_path_addressed());
    assert!(VfsRequest::Rmdir("/x").is_path_addressed());
    assert!(VfsRequest::Unlink("/x").is_path_addressed());
    assert!(VfsRequest::RmdirRecursive("/x").is_path_addressed());
    assert!(VfsRequest::ReadAsync { path: "/x" }.is_path_addressed());
    assert!(VfsRequest::ReadFileGrant {
        path: "/x",
        grant: 0,
        max: 0
    }
    .is_path_addressed());
    assert!(VfsRequest::OpenRootDir { path: "/x" }.is_path_addressed());
}

#[test]
fn no_handle_addressed_request_is_classified_as_path_addressed() {
    use crate::dir_handles::ViDirHandle;
    let dir = ViDirHandle(1);
    assert!(!VfsRequest::OpenDir { dir, name: "x" }.is_path_addressed());
    assert!(!VfsRequest::ReadAt { dir, name: "x" }.is_path_addressed());
    assert!(!VfsRequest::WriteAt {
        dir,
        name: "x",
        content: b""
    }
    .is_path_addressed());
    assert!(!VfsRequest::StatAt { dir, name: "x" }.is_path_addressed());
    assert!(!VfsRequest::ListAt { dir }.is_path_addressed());
    assert!(!VfsRequest::UnlinkAt { dir, name: "x" }.is_path_addressed());
    assert!(!VfsRequest::CloseDir { dir }.is_path_addressed());
    assert!(!VfsRequest::SealPaths.is_path_addressed());
}

// ── Wire compatibility ───────────────────────────────────────────────────────

#[test]
fn the_existing_variants_keep_their_discriminants() {
    // The whole point of appending is that these bytes do not move. A failure
    // here means an in-flight message from an unmigrated cell would be decoded
    // as a different operation.
    let mut buf = [0u8; 64];
    let encoded = super::ipc::encode(&VfsRequest::GetFile("a"), &mut buf).unwrap();
    assert_eq!(encoded[0], 0, "GetFile must stay variant 0");

    let mut buf = [0u8; 64];
    let encoded = super::ipc::encode(
        &VfsRequest::ReadFileGrant {
            path: "a",
            grant: 1,
            max: 2,
        },
        &mut buf,
    )
    .unwrap();
    assert_eq!(
        encoded[0], 13,
        "ReadFileGrant was the last variant before the directory ops"
    );
}

#[test]
fn a_handle_encodes_as_its_inner_value_and_survives_a_round_trip() {
    use crate::dir_handles::ViDirHandle;
    let mut buf = [0u8; 64];
    let encoded = super::ipc::encode(
        &VfsRequest::ReadAt {
            dir: ViDirHandle(7),
            name: "x",
        },
        &mut buf,
    )
    .unwrap();
    assert_eq!(encoded[0], 16, "ReadAt sits after OpenRootDir and OpenDir");
    assert_eq!(encoded[1], 7, "the handle is a bare varint, no wrapper");

    let decoded: VfsRequest = super::ipc::decode(encoded).unwrap();
    match decoded {
        VfsRequest::ReadAt { dir, name } => {
            assert_eq!(dir, ViDirHandle(7));
            assert_eq!(name, "x");
        }
        other => panic!("round trip changed the request: {other:?}"),
    }
}

#[test]
fn a_directory_request_fits_the_ipc_buffer_with_room_to_spare() {
    use crate::dir_handles::ViDirHandle;
    let name = "a".repeat(MAX_DIR_NAME_LEN);
    let content = vec![b'c'; 1024];
    let mut buf = [0u8; super::ipc::IPC_BUF_SIZE];
    let encoded = super::ipc::encode(
        &VfsRequest::WriteAt {
            dir: ViDirHandle(u64::MAX),
            name: &name,
            content: &content,
        },
        &mut buf,
    )
    .expect("the widest directory request must encode");
    // A handle plus a component is strictly smaller than the absolute path it
    // replaces, so the migration cannot make a message stop fitting.
    assert!(encoded.len() < super::ipc::IPC_BUF_SIZE);
}
