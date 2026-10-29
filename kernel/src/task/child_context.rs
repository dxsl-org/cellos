//! Immutable, bounded launch context validation before scheduler publication.
use api::cell_session::{ObjectRefV1, Reader, Status, CONTEXT_MAX, CONTEXT_MAX_GRANTS,
    GRANT_KIND_INPUT_FRONTEND, SLOT_FRONTEND};
use core::ops::Range;

pub(crate) const MAX_LEN: usize = CONTEXT_MAX;
pub(crate) const MAX_GRANTS: usize = CONTEXT_MAX_GRANTS;
/// Kind-1 completion-record writer slot (write-only, same rules as stdout).
pub(crate) const SLOT_COMPLETION: u16 = 5;
/// Kind-1/slot-0 inspection projection: the four stdio grant records.
pub(crate) const STDIO_PROJECTION_LEN: usize = 4 * 40;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Grant {
    pub kind: u16,
    pub rights: u16,
    pub slot: u16,
    pub owner_service: u64,
    pub object: ObjectRefV1,
    pub wire: [u8; 40],
}
pub(crate) struct Context {
    pub argv: Range<usize>,
    pub cwd: Range<usize>,
    /// Indexed by slot: 0..=3 kernel endpoints (stdio), 4 VFS cwd, 5 kernel
    /// completion writer, 6..=7 reserved, 8 input frontend.
    pub grants: [Option<Grant>; MAX_GRANTS],
    pub digest: [u8; 32],
}

pub(crate) fn validate(bytes: &[u8], operation: ObjectRefV1) -> Result<Context, Status> {
    if !(48..=MAX_LEN).contains(&bytes.len()) { return Err(Status::Malformed); }
    let mut r = Reader::new(bytes);
    if r.u32()? != 1 { return Err(Status::UnsupportedVersion); }
    if r.u32()? as usize != bytes.len() || r.u32()? != 0 { return Err(Status::Malformed); }
    let argv_len = r.u32()? as usize;
    let env_count = r.u16()? as usize;
    let grant_count = r.u16()? as usize;
    let cwd_len = r.u32()? as usize;
    let env_len = r.u32()? as usize;
    if r.u32()? != 0 || r.reference()? != operation || argv_len > 512 || cwd_len > 4096
        || env_count > 64 || env_len > 4096 || grant_count > MAX_GRANTS { return Err(Status::Malformed); }
    let argv = r.take(argv_len)?;
    // The prefix is the established structured argv carrier, with no argv[0].
    if !argv.is_empty() {
        if !argv.starts_with(b"\0argv1\0") || argv.last() != Some(&0) { return Err(Status::Malformed); }
        for arg in argv[7..].split_inclusive(|b| *b == 0) {
            if arg.last() != Some(&0) || core::str::from_utf8(&arg[..arg.len()-1]).is_err() {
                return Err(Status::Malformed);
            }
        }
    }
    let cwd = r.take(cwd_len)?;
    if cwd.contains(&0) { return Err(Status::Malformed); }
    let mut env = Reader::new(r.take(env_len)?);
    let mut previous: Option<&[u8]> = None;
    for _ in 0..env_count {
        let kl = env.u16()? as usize;
        let vl = env.u16()? as usize;
        let key = env.take(kl)?;
        let value = env.take(vl)?;
        if key.is_empty() || key.contains(&0) || key.contains(&b'=') || value.contains(&0)
            || core::str::from_utf8(key).is_err() || core::str::from_utf8(value).is_err()
            || previous.is_some_and(|p| p >= key) { return Err(Status::Malformed); }
        previous = Some(key);
    }
    env.finish()?;
    let mut grants = [None; MAX_GRANTS];
    for _ in 0..grant_count {
        let wire: [u8; 40] = r.take(40)?.try_into().map_err(|_| Status::Malformed)?;
        let mut g = Reader::new(&wire);
        let kind = g.u16()?;
        let rights = g.u16()?;
        let slot = g.u16()?;
        if g.u16()? != 0 { return Err(Status::Malformed); }
        let owner_service = g.u64()?;
        let object = g.reference()?;
        if g.u64()? != 0 || object.id == 0 || object.generation == 0 || slot as usize >= MAX_GRANTS
            || grants[slot as usize].is_some() { return Err(Status::Malformed); }
        match kind {
            1 if (slot <= 3 || slot == SLOT_COMPLETION) && owner_service == 0 && rights & !31 == 0
                && (if slot == 0 || slot == 3 { rights & 3 == 1 } else { rights & 3 == 2 }) => {},
            2 if slot == 4 && rights == 0 && owner_service == api::syscall::service::VFS as u64 => {},
            3..=5 => return Err(Status::Unsupported),
            GRANT_KIND_INPUT_FRONTEND if slot == SLOT_FRONTEND && rights == 0
                && owner_service == api::syscall::service::INPUT as u64 => {},
            _ => return Err(Status::Malformed),
        }
        grants[slot as usize] = Some(Grant { kind, rights, slot, owner_service, object, wire });
    }
    r.finish()?;
    if cwd_len != 0 && grants[4].is_none() { return Err(Status::Denied); }
    Ok(Context { argv: 48..48+argv_len, cwd: 48+argv_len..48+argv_len+cwd_len,
        grants, digest: crate::sha256::sha256(bytes) })
}

impl Context {
    /// The stdio projection input inspects (kind 1, slot 0): exactly the four
    /// stdio grant records of slots 0..=3, never the completion or later slots.
    pub(crate) fn stdio_projection(&self) -> Result<[u8; STDIO_PROJECTION_LEN], Status> {
        let mut stdio = [0u8; STDIO_PROJECTION_LEN];
        for (slot, bytes) in stdio.chunks_exact_mut(40).enumerate() {
            bytes.copy_from_slice(&self.grants[slot].ok_or(Status::Denied)?.wire);
        }
        Ok(stdio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use api::syscall::service::{INPUT, VFS};

    const OPERATION: ObjectRefV1 = ObjectRefV1 { id: 7, generation: 1 };
    const OBJECT: ObjectRefV1 = ObjectRefV1 { id: 9, generation: 2 };

    fn grant(kind: u16, rights: u16, slot: u16, owner: u64) -> [u8; 40] {
        let mut g = [0u8; 40];
        g[0..2].copy_from_slice(&kind.to_le_bytes());
        g[2..4].copy_from_slice(&rights.to_le_bytes());
        g[4..6].copy_from_slice(&slot.to_le_bytes());
        g[8..16].copy_from_slice(&owner.to_le_bytes());
        g[16..32].copy_from_slice(&OBJECT.to_bytes());
        g
    }
    fn context(grants: &[[u8; 40]]) -> Vec<u8> {
        let len = 48 + grants.len() * 40;
        let mut bytes = Vec::new();
        for word in [1u32, len as u32, 0, 0] { bytes.extend_from_slice(&word.to_le_bytes()); }
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(grants.len() as u16).to_le_bytes());
        for word in [0u32, 0, 0] { bytes.extend_from_slice(&word.to_le_bytes()); }
        bytes.extend_from_slice(&OPERATION.to_bytes());
        for g in grants { bytes.extend_from_slice(g); }
        bytes
    }
    fn check(grants: &[[u8; 40]]) -> Result<Context, Status> { validate(&context(grants), OPERATION) }

    #[test]
    fn frontend_grant_is_kind_6_in_slot_8_only() {
        let parsed = check(&[grant(6, 0, 8, INPUT as u64)]).expect("kind 6 slot 8");
        let g = parsed.grants[8].expect("slot 8 populated");
        assert_eq!((g.kind, g.slot, g.rights, g.object), (6, 8, 0, OBJECT));
        assert!(parsed.grants[..8].iter().all(Option::is_none));
        // Wrong rights, owner or slot for kind 6.
        assert_eq!(check(&[grant(6, 1, 8, INPUT as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(6, 0, 8, VFS as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(6, 0, 8, 0)]).err(), Some(Status::Malformed));
        for slot in 0..8 {
            assert_eq!(check(&[grant(6, 0, slot, INPUT as u64)]).err(), Some(Status::Malformed), "slot {slot}");
        }
        // Slot 8 admits nothing but kind 6; slots past 8 do not exist.
        assert_eq!(check(&[grant(1, 1, 8, 0)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(1, 2, 8, 0)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(2, 0, 8, VFS as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(6, 0, 9, INPUT as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(7, 0, 8, INPUT as u64)]).err(), Some(Status::Malformed));
        // Duplicate slot 8 is refused.
        assert_eq!(check(&[grant(6, 0, 8, INPUT as u64), grant(6, 0, 8, INPUT as u64)]).err(), Some(Status::Malformed));
    }

    #[test]
    fn existing_kinds_and_reserved_slots_are_unchanged() {
        for (slot, rights) in [(0, 1), (1, 2), (2, 2), (3, 1)] {
            let parsed = check(&[grant(1, rights | 16, slot, 0)]).expect("kind 1");
            assert_eq!(parsed.grants[slot as usize].map(|g| g.kind), Some(1));
        }
        assert_eq!(check(&[grant(1, 2, 0, 0)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(1, 1, 4, 0)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(2, 0, 4, VFS as u64)]).map(|c| c.grants[4].map(|g| g.kind)).ok(), Some(Some(2)));
        assert_eq!(check(&[grant(2, 0, 4, INPUT as u64)]).err(), Some(Status::Malformed));
        for kind in 3..=5 {
            for slot in [4, 5, 6, 7, 8] {
                assert_eq!(check(&[grant(kind, 0, slot, INPUT as u64)]).err(), Some(Status::Unsupported));
            }
        }
        for slot in 5..=7 {
            assert_eq!(check(&[grant(1, 1, slot, 0)]).err(), Some(Status::Malformed));
            assert_eq!(check(&[grant(2, 0, slot, VFS as u64)]).err(), Some(Status::Malformed));
        }
    }

    fn at(mut g: [u8; 40], id: u64) -> [u8; 40] {
        g[16..32].copy_from_slice(&ObjectRefV1 { id, generation: 2 }.to_bytes());
        g
    }

    #[test]
    fn completion_grant_is_kind_1_write_only_in_slot_5() {
        for rights in [2u16, 6, 10, 18, 26, 30] {
            let parsed = check(&[grant(1, rights, 5, 0)]).expect("kind 1 slot 5 writer");
            let g = parsed.grants[5].expect("slot 5 populated");
            assert_eq!((g.kind, g.slot, g.rights, g.object), (1, 5, rights, OBJECT));
            assert!(parsed.grants.iter().enumerate().all(|(slot, g)| slot == 5 || g.is_none()));
        }
        // Read, bidirectional, directionless and out-of-mask rights are refused.
        for rights in [0u16, 1, 3, 17, 29, 31, 34, 0x8002] {
            assert_eq!(check(&[grant(1, rights, 5, 0)]).err(), Some(Status::Malformed), "rights {rights}");
        }
        // Only the kernel owns endpoint grants; kind 2 and kind 6 are not slot-5 kinds.
        assert_eq!(check(&[grant(1, 2, 5, VFS as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(1, 2, 5, INPUT as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(2, 0, 5, VFS as u64)]).err(), Some(Status::Malformed));
        assert_eq!(check(&[grant(6, 0, 5, INPUT as u64)]).err(), Some(Status::Malformed));
        // Slots 6..=7 stay reserved for every endpoint direction.
        for slot in 6..=7 {
            for rights in [1u16, 2, 18, 30] {
                assert_eq!(check(&[grant(1, rights, slot, 0)]).err(), Some(Status::Malformed), "slot {slot}");
            }
        }
        // One completion writer per context.
        assert_eq!(check(&[grant(1, 2, 5, 0), at(grant(1, 2, 5, 0), 40)]).err(), Some(Status::Malformed));
    }

    #[test]
    fn stdio_projection_is_exactly_slots_0_to_3() {
        let stdio = [at(grant(1, 17, 0, 0), 20), at(grant(1, 30, 1, 0), 21), at(grant(1, 30, 2, 0), 22),
            at(grant(1, 17, 3, 0), 23)];
        let plain = check(&stdio).expect("stdio only").stdio_projection().expect("projection");
        assert_eq!(plain.len(), 160);
        assert_eq!(plain.as_slice(), stdio.concat().as_slice());
        // The completion writer, listed first or last and with the frontend, never
        // enters the projection input inspects.
        let completion = at(grant(1, 18, 5, 0), 25);
        let frontend = at(grant(6, 0, 8, INPUT as u64), 26);
        let mut first = Vec::from([completion]);
        first.extend_from_slice(&stdio);
        let mut last = Vec::from(stdio);
        last.extend_from_slice(&[frontend, completion]);
        for grants in [first, last] {
            let parsed = check(&grants).expect("stdio + completion");
            assert_eq!(parsed.grants[5].map(|g| g.object.id), Some(25));
            assert_eq!(parsed.stdio_projection().expect("projection"), plain);
        }
        // A completion writer does not stand in for a missing stdio grant.
        let partial = check(&[stdio[0], stdio[1], stdio[2], completion]).expect("three stdio + completion");
        assert_eq!(partial.stdio_projection().err(), Some(Status::Denied));
    }

    #[test]
    fn nine_grants_fit_and_ten_do_not() {
        let full = [grant(1, 1, 0, 0), grant(1, 2, 1, 0), grant(1, 2, 2, 0), grant(1, 1, 3, 0),
            grant(6, 0, 8, INPUT as u64)];
        let parsed = check(&full).expect("stdio + frontend");
        assert!(parsed.grants[..4].iter().all(Option::is_some) && parsed.grants[8].is_some());
        // A header claiming ten grants is refused before any grant is read.
        let mut ten = context(&[grant(6, 0, 8, INPUT as u64); 10]);
        assert_eq!(validate(&ten, OPERATION).err(), Some(Status::Malformed));
        ten.truncate(48 + 9 * 40);
        let len = ten.len() as u32;
        ten[4..8].copy_from_slice(&len.to_le_bytes());
        ten[18..20].copy_from_slice(&9u16.to_le_bytes());
        // Nine records, but duplicated slot 8: still refused by slot uniqueness.
        assert_eq!(validate(&ten, OPERATION).err(), Some(Status::Malformed));
        assert_eq!(MAX_LEN, 9112);
    }
}
