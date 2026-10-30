//! The pure listing matcher behind `VfsBlobIo`'s absent-versus-failed rule:
//! `Absent` is reported only for a listing that names no entry of that name.

use super::*;
use crate::ssh::persist::blob_io::{classify_listing, Listed};

const A: &str = "ssh-a.bin";
const B: &str = "ssh-b.bin";

#[test]
fn file_entries_match_in_both_forms() {
    assert_eq!(classify_listing(b"f:ssh-a.bin\n", A), Listed::File);
    assert_eq!(classify_listing(b"ssh-a.bin\n", A), Listed::File);
    // No trailing newline.
    assert_eq!(classify_listing(b"f:ssh-a.bin", A), Listed::File);
    assert_eq!(classify_listing(b"ssh-a.bin", A), Listed::File);
}

#[test]
fn a_directory_entry_is_not_a_file() {
    assert_eq!(classify_listing(b"d:ssh-a.bin\n", A), Listed::Directory);
    assert_eq!(classify_listing(b"d:ssh-a.bin", A), Listed::Directory);
    // A file entry wins over a directory entry of the same name.
    assert_eq!(
        classify_listing(b"d:ssh-a.bin\nf:ssh-a.bin\n", A),
        Listed::File
    );
}

#[test]
fn near_misses_do_not_match() {
    for listing in [
        &b"xssh-a.bin\n"[..],
        b"f:xssh-a.bin\n",
        b"ssh-a.bin.tmp\n",
        b"f:ssh-a.bin.tmp\n",
        b"ssh-a.binx\n",
        b"ssh-a.bi\n",
        b"ssh-a\n",
        b" ssh-a.bin\n",
        b"ssh-a.bin \n",
        b"g:ssh-a.bin\n",
        b"f:\n",
        b"SSH-A.BIN\n",
        b"slot-a.bin\nslot-b.bin\n",
    ] {
        assert_eq!(
            classify_listing(listing, A),
            Listed::Missing,
            "{:?}",
            core::str::from_utf8(listing)
        );
    }
}

#[test]
fn empty_and_blank_listings_name_nothing() {
    assert_eq!(classify_listing(b"", A), Listed::Missing);
    assert_eq!(classify_listing(b"\n", A), Listed::Missing);
    assert_eq!(classify_listing(b"\n\n\n", B), Listed::Missing);
}

#[test]
fn both_files_and_other_entries() {
    let listing = b"f:slot-a.bin\nf:ssh-a.bin\nf:ssh-b.bin\nd:sub\n";
    assert_eq!(classify_listing(listing, A), Listed::File);
    assert_eq!(classify_listing(listing, B), Listed::File);
    let only_a = b"f:ssh-a.bin\n";
    assert_eq!(classify_listing(only_a, A), Listed::File);
    assert_eq!(classify_listing(only_a, B), Listed::Missing);
    // Entry order does not matter.
    assert_eq!(
        classify_listing(b"f:zzz\nf:ssh-b.bin\nf:aaa\n", B),
        Listed::File
    );
}

#[test]
fn slot_names_are_the_two_files() {
    assert_eq!(Slot::A.file_name(), A);
    assert_eq!(Slot::B.file_name(), B);
    assert_eq!((Slot::A.id(), Slot::B.id()), (0, 1));
    assert_eq!(Slot::A.other(), Slot::B);
    assert_eq!(Slot::B.other(), Slot::A);
}
