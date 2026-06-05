//! Syscall ABI encode/decode tests.
//!
//! Verifies that every `ViSyscall` variant survives a `usize → ViSyscall`
//! round-trip and that the `Unknown` fallback is produced for unrecognised IDs.
//!
//! Run on the host with:
//!   cargo test -p api --target x86_64-pc-windows-msvc

#[cfg(test)]
mod tests {
    use crate::syscall::ViSyscall;

    /// All (id, expected_variant) pairs that must round-trip correctly.
    const CASES: &[(usize, ViSyscall)] = &[
        (0,    ViSyscall::Send),
        (1,    ViSyscall::Recv),
        (2,    ViSyscall::Call),
        (3,    ViSyscall::Reply),
        (5,    ViSyscall::Spawn),
        (7,    ViSyscall::TryRecv),
        (8,    ViSyscall::Wait),
        (10,   ViSyscall::SpawnFromMem),
        (11,   ViSyscall::Log),
        (12,   ViSyscall::SpawnFromPath),
        (13,   ViSyscall::OpenCap),
        (14,   ViSyscall::ReadCap),
        (15,   ViSyscall::CloseCap),
        (20,   ViSyscall::ShmAlloc),
        (21,   ViSyscall::ShmMap),
        (30,   ViSyscall::GetProcs),
        (35,   ViSyscall::SetTimer),
        (60,   ViSyscall::Exit),
        (61,   ViSyscall::ForceExit),
        (101,  ViSyscall::Open),
        (102,  ViSyscall::Read),
        (103,  ViSyscall::Close),
        (104,  ViSyscall::Yield),
        (105,  ViSyscall::ReadDir),
        (106,  ViSyscall::Seek),
        (107,  ViSyscall::FileOp),
        (109,  ViSyscall::Write),
        (120,  ViSyscall::GetTime),
        (310,  ViSyscall::NetTx),
        (311,  ViSyscall::NetRx),
    ];

    #[test]
    fn all_known_ids_decode_to_correct_variant() {
        for &(id, expected) in CASES {
            let got = ViSyscall::from(id);
            assert_eq!(
                got, expected,
                "ViSyscall::from({}) should be {:?}, got {:?}",
                id, expected, got
            );
        }
    }

    #[test]
    fn known_variants_have_stable_discriminants() {
        // Discriminants are part of the ABI between kernel and cells — they
        // must never change without a coordinated version bump.
        assert_eq!(ViSyscall::Send      as usize,   0);
        assert_eq!(ViSyscall::Recv      as usize,   1);
        assert_eq!(ViSyscall::Call      as usize,   2);
        assert_eq!(ViSyscall::Reply     as usize,   3);
        assert_eq!(ViSyscall::Spawn     as usize,   5);
        assert_eq!(ViSyscall::SpawnFromPath as usize, 12);
        assert_eq!(ViSyscall::Log       as usize,  11);
        assert_eq!(ViSyscall::Exit      as usize,  60);
        assert_eq!(ViSyscall::Open      as usize, 101);
        assert_eq!(ViSyscall::Read      as usize, 102);
        assert_eq!(ViSyscall::Close     as usize, 103);
    }

    #[test]
    fn unknown_id_decodes_to_unknown_variant() {
        // IDs that have no assigned meaning must produce Unknown, not panic.
        let unassigned = [4, 9, 50, 99, 100, 108, 999, usize::MAX];
        for id in unassigned {
            let got = ViSyscall::from(id);
            assert_eq!(
                got,
                ViSyscall::Unknown,
                "id {} should decode to Unknown, got {:?}",
                id,
                got
            );
        }
    }

    #[test]
    fn all_cases_are_non_unknown() {
        // Sanity check: every case in CASES must decode to a non-Unknown variant.
        for &(id, _) in CASES {
            let got = ViSyscall::from(id);
            assert_ne!(
                got,
                ViSyscall::Unknown,
                "id {} decoded to Unknown — add it to the From<usize> impl",
                id
            );
        }
    }

    #[test]
    fn no_two_known_ids_map_to_same_variant() {
        // Detect accidental aliasing: if two IDs both map to the same variant
        // (other than Unknown), one of them is almost certainly wrong.
        let mut seen: alloc::vec::Vec<(usize, ViSyscall)> = alloc::vec::Vec::new();
        for &(id, variant) in CASES {
            for &(prev_id, prev_variant) in &seen {
                if variant == prev_variant && id != prev_id {
                    panic!(
                        "id {} and id {} both map to {:?} — collision in syscall table",
                        id, prev_id, variant
                    );
                }
            }
            seen.push((id, variant));
        }
    }
}
