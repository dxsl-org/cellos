//! H-27: a client may attach only a Grant it registered itself.
//!
//! `admit_grant` asks the kernel to resolve the Grant as the sender's own. The
//! fake kernel below mirrors the frozen `GrantSlice` a2 semantics: a Grant
//! whose owner is not the expected tid resolves exactly as an unknown id.

extern crate std;

use wire::display::{compositor_ops, AttachGrant, PixelFormat, SurfaceRole};

use super::admit_grant;
use crate::surface_table::SurfaceTable;

const VICTIM: usize = 7;
const ATTACKER: usize = 9;
const REG_ID: usize = 0x8000_0000;

fn attach_frame(cap: u64) -> AttachGrant {
    AttachGrant {
        opcode: compositor_ops::ATTACH_GRANT,
        fmt: PixelFormat::Bgra8888 as u8,
        _pad: [0; 2],
        cap: cap as u32,
        reg_id: REG_ID as u64,
        width: 2,
        height: 2,
    }
}

/// A kernel that holds one 16-byte Grant `REG_ID` registered by `owner` and
/// shared to the compositor, recording the owner each lookup expected.
fn kernel(
    owner: usize,
    ptr: *mut u8,
    asked: &mut Option<usize>,
) -> impl FnOnce(usize, usize) -> Option<(*mut u8, usize)> + '_ {
    move |reg_id, expected_owner| {
        *asked = Some(expected_owner);
        (reg_id == REG_ID && expected_owner == owner).then_some((ptr, 16))
    }
}

#[test]
fn attaching_a_grant_owned_by_another_tid_is_refused() {
    let mut backing = std::vec![0u8; 16];
    let mut table = SurfaceTable::new();
    // The victim shared its Grant to the compositor but has not attached it.
    let cap = table
        .create(0, 0, 2, 2, ATTACKER, SurfaceRole::Interactive)
        .unwrap();
    let mut asked = None;
    let resolve = kernel(VICTIM, backing.as_mut_ptr(), &mut asked);
    assert!(!admit_grant(
        &attach_frame(cap),
        ATTACKER,
        &mut table,
        resolve
    ));
    // The lookup named the requester, not "anyone the Grant is shared to".
    assert_eq!(asked, Some(ATTACKER));
    assert_eq!(table.grant_holder(REG_ID), None);
}

#[test]
fn the_owner_attaches_its_own_grant() {
    let mut backing = std::vec![0u8; 16];
    let mut table = SurfaceTable::new();
    let cap = table
        .create(0, 0, 2, 2, VICTIM, SurfaceRole::Interactive)
        .unwrap();
    let mut asked = None;
    let resolve = kernel(VICTIM, backing.as_mut_ptr(), &mut asked);
    assert!(admit_grant(&attach_frame(cap), VICTIM, &mut table, resolve));
    assert_eq!(asked, Some(VICTIM));
    assert_eq!(table.grant_holder(REG_ID), Some(VICTIM));
}
