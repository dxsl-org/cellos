//! Syscall ABI encode/decode tests.
//!
//! Verifies that every `ViSyscall` variant survives a `usize → ViSyscall`
//! round-trip and that the `Unknown` fallback is produced for unrecognised IDs.
//!
//! Run on the host with:
//!   cargo test -p api --target x86_64-pc-windows-msvc

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};

    use crate::syscall::{ProcessInfo, ProcessInfoV2, SyscallSet, ViMemInfoV1, ViSyscall};

    /// All (id, expected_variant) pairs that must round-trip correctly.
    const CASES: &[(usize, ViSyscall)] = &[
        (0, ViSyscall::Send),
        (1, ViSyscall::Recv),
        (2, ViSyscall::Call),
        (3, ViSyscall::Reply),
        (4, ViSyscall::TrySend),
        (5, ViSyscall::Spawn),
        (7, ViSyscall::TryRecv),
        (8, ViSyscall::Wait),
        (10, ViSyscall::SpawnFromMem),
        (11, ViSyscall::Log),
        (12, ViSyscall::SpawnFromPath),
        (13, ViSyscall::OpenCap),
        (14, ViSyscall::ReadCap),
        (15, ViSyscall::CloseCap),
        (228, ViSyscall::SeekCap),
        (229, ViSyscall::WriteCap),
        (230, ViSyscall::StatCap),
        (231, ViSyscall::TruncateCap),
        (232, ViSyscall::SyncCap),
        (233, ViSyscall::GrantDma),
        (239, ViSyscall::GetProcs2),
        (240, ViSyscall::SpawnSetDirs),
        (241, ViSyscall::QueryDirHandles),
        (242, ViSyscall::WaitCompletion),
        (243, ViSyscall::MemInfo),
        (244, ViSyscall::ResolveCellOwner),
        (245, ViSyscall::WatchCellOwner),
        (246, ViSyscall::CancelCellOwnerWatch),
        (247, ViSyscall::ResolveCellOwnerRecord),
        (248, ViSyscall::WatchCellOwnerRecord),
        (249, ViSyscall::GrantCacheSyncBegin),
        (250, ViSyscall::GrantCacheSyncComplete),
        (251, ViSyscall::RegisterDisplayFramebuffer),
        (20, ViSyscall::ShmAlloc),
        (21, ViSyscall::ShmMap),
        (30, ViSyscall::GetProcs),
        (35, ViSyscall::SetTimer),
        (60, ViSyscall::Exit),
        (61, ViSyscall::ForceExit),
        (101, ViSyscall::Open),
        (102, ViSyscall::Read),
        (103, ViSyscall::Close),
        (104, ViSyscall::Yield),
        (105, ViSyscall::ReadDir),
        (106, ViSyscall::Seek),
        (107, ViSyscall::FileOp),
        (109, ViSyscall::Write),
        (120, ViSyscall::GetTime),
        (218, ViSyscall::AudioPlay),
        (219, ViSyscall::CapRevoke),
        (401, ViSyscall::HotSwapReady),
        (420, ViSyscall::Snapshot),
        (421, ViSyscall::SpawnReplacement),
        (422, ViSyscall::PauseService),
        (237, ViSyscall::ReadLog),
        (238, ViSyscall::SpawnFromElf),
        (310, ViSyscall::NetTx),
        (311, ViSyscall::NetRx),
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
        assert_eq!(ViSyscall::Send as usize, 0);
        assert_eq!(ViSyscall::Recv as usize, 1);
        assert_eq!(ViSyscall::Call as usize, 2);
        assert_eq!(ViSyscall::Reply as usize, 3);
        assert_eq!(ViSyscall::Spawn as usize, 5);
        assert_eq!(ViSyscall::SpawnFromPath as usize, 12);
        assert_eq!(ViSyscall::Log as usize, 11);
        assert_eq!(ViSyscall::Exit as usize, 60);
        assert_eq!(ViSyscall::Open as usize, 101);
        assert_eq!(ViSyscall::Read as usize, 102);
        assert_eq!(ViSyscall::Close as usize, 103);
        assert_eq!(ViSyscall::GetProcs as usize, 30);
        assert_eq!(ViSyscall::GetProcs2 as usize, 239);
        assert_eq!(ViSyscall::SpawnSetDirs as usize, 240);
        assert_eq!(ViSyscall::QueryDirHandles as usize, 241);
        assert_eq!(ViSyscall::WaitCompletion as usize, 242);
        assert_eq!(ViSyscall::MemInfo as usize, 243);
        assert_eq!(ViSyscall::ResolveCellOwner as usize, 244);
        assert_eq!(ViSyscall::WatchCellOwner as usize, 245);
        assert_eq!(ViSyscall::CancelCellOwnerWatch as usize, 246);
        assert_eq!(ViSyscall::ResolveCellOwnerRecord as usize, 247);
        assert_eq!(ViSyscall::WatchCellOwnerRecord as usize, 248);
        assert_eq!(ViSyscall::HotSwapReady as usize, 401);
        assert_eq!(ViSyscall::Snapshot as usize, 420);
        assert_eq!(ViSyscall::SpawnReplacement as usize, 421);
        assert_eq!(ViSyscall::PauseService as usize, 422);
    }

    /// The appended opcodes must sit past every previously shipped id. An
    /// appended variant that reused a live discriminant has already cost this
    /// project one silent IPC collision.
    #[test]
    fn appended_opcodes_do_not_collide_with_shipped_ids() {
        for &(id, variant) in CASES {
            if matches!(
                variant,
                ViSyscall::SpawnSetDirs
                    | ViSyscall::QueryDirHandles
                    | ViSyscall::WaitCompletion
                    | ViSyscall::MemInfo
                    | ViSyscall::ResolveCellOwner
                    | ViSyscall::WatchCellOwner
                    | ViSyscall::CancelCellOwnerWatch
                    | ViSyscall::ResolveCellOwnerRecord
                    | ViSyscall::WatchCellOwnerRecord
                    | ViSyscall::SpawnReplacement
                    | ViSyscall::PauseService
            ) {
                continue;
            }
            assert_ne!(id, ViSyscall::SpawnSetDirs as usize);
            assert_ne!(id, ViSyscall::QueryDirHandles as usize);
            assert_ne!(id, ViSyscall::WaitCompletion as usize);
            assert_ne!(id, ViSyscall::MemInfo as usize);
            assert_ne!(id, ViSyscall::ResolveCellOwner as usize);
            assert_ne!(id, ViSyscall::WatchCellOwner as usize);
            assert_ne!(id, ViSyscall::CancelCellOwnerWatch as usize);
            assert_ne!(id, ViSyscall::SpawnReplacement as usize);
            assert_ne!(id, ViSyscall::PauseService as usize);
            assert_ne!(id, ViSyscall::ResolveCellOwnerRecord as usize);
            assert_ne!(id, ViSyscall::WatchCellOwnerRecord as usize);
        }
        // Previously unmapped ids must still decode as Unknown, so nothing that
        // used to be rejected is now silently accepted as a new opcode.
        assert_eq!(ViSyscall::GrantCacheSyncBegin as usize, 249);
        assert_eq!(ViSyscall::GrantCacheSyncComplete as usize, 250);
        assert_eq!(ViSyscall::RegisterDisplayFramebuffer as usize, 251);
        assert_eq!(ViSyscall::from(400), ViSyscall::Unknown);
        assert_eq!(ViSyscall::from(423), ViSyscall::Unknown);
    }

    #[test]
    fn hotswap_ready_and_snapshot_preserve_legacy_bit_32() {
        assert_eq!(ViSyscall::HotSwapReady.allowlist_bit(), Some(32));
        assert_eq!(ViSyscall::Snapshot.allowlist_bit(), Some(32));
    }

    #[test]
    fn display_cache_operations_use_new_disjoint_allowlist_bits() {
        assert_eq!(ViSyscall::GrantCacheSyncBegin.allowlist_bit(), Some(58));
        assert_eq!(ViSyscall::GrantCacheSyncComplete.allowlist_bit(), Some(58));
        assert_eq!(
            ViSyscall::RegisterDisplayFramebuffer.allowlist_bit(),
            Some(59)
        );
        assert_ne!(ViSyscall::ReadLog.allowlist_bit(), Some(58));
        assert_ne!(ViSyscall::SpawnReplacement.allowlist_bit(), Some(58));
    }

    /// `WaitCompletion` parks on the same authority as `WaitForEvent` and shares
    /// its allowlist bit. A separate bit would deny the call to every cell whose
    /// allowlist section was generated before that bit existed.
    #[test]
    fn wait_completion_shares_the_wait_for_event_authority() {
        assert_eq!(ViSyscall::WaitForEvent.allowlist_bit(), Some(42));
        assert_eq!(ViSyscall::WaitCompletion.allowlist_bit(), Some(42));
        let legacy = SyscallSet::EMPTY.with(ViSyscall::WaitForEvent);
        assert!(legacy.permits(ViSyscall::WaitCompletion));
    }

    #[test]
    fn completion_source_bits_are_stable_and_disjoint() {
        assert_eq!(crate::syscall::events::NET_RX, 1 << 0);
        assert_eq!(crate::syscall::events::TIMER, 1 << 1);
        assert_eq!(
            crate::syscall::events::NET_RX,
            crate::completion::source::NET_RX
        );
        assert_eq!(
            crate::syscall::events::TIMER,
            crate::completion::source::TIMER
        );
        assert_eq!(
            crate::syscall::events::NET_RX & crate::syscall::events::TIMER,
            0
        );
    }

    /// v1 must stay byte-for-byte identical for every existing `GetProcs`
    /// caller; v2 is a separate fixed-width row so the two never alias.
    #[test]
    fn process_info_layouts_are_stable() {
        // v1 predates the fixed-width rule: its id/state are `usize`, so the row
        // is pointer-width dependent (40 bytes on rv32, 48 on the 64-bit targets).
        // Pinned per width rather than relaxed — a silent field addition is the
        // regression this guards against.
        let v1_expected = 2 * size_of::<usize>() + 32;
        assert_eq!(size_of::<ProcessInfo>(), v1_expected);
        assert_eq!(align_of::<ProcessInfo>(), align_of::<usize>());

        // v2 is fixed-width by construction, so it is identical on RV32, RV64,
        // AArch64 and x86_64: u64 + u32 + u32 + [u8; 32] + 4 × u64.
        assert_eq!(size_of::<ProcessInfoV2>(), 80);
        assert_eq!(align_of::<ProcessInfoV2>(), 8);
    }

    #[test]
    fn mem_info_v1_layout_is_stable() {
        assert_eq!(size_of::<ViMemInfoV1>(), 32);
        assert_eq!(align_of::<ViMemInfoV1>(), 8);
    }

    #[test]
    fn unknown_id_decodes_to_unknown_variant() {
        // IDs that have no assigned meaning must produce Unknown, not panic.
        let unassigned = [9, 50, 99, 100, 108, 999, usize::MAX];
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

#[cfg(test)]
mod allowlist {
    use crate::syscall::{SyscallSet, ViSyscall};

    #[test]
    fn syscall_set_empty_permits_nothing() {
        assert!(!SyscallSet::EMPTY.permits(ViSyscall::Send));
        assert!(!SyscallSet::EMPTY.permits(ViSyscall::Recv));
        assert!(!SyscallSet::EMPTY.permits(ViSyscall::Log));
    }

    #[test]
    fn syscall_set_all_permits_everything() {
        assert!(SyscallSet::ALL.permits(ViSyscall::Send));
        assert!(SyscallSet::ALL.permits(ViSyscall::Recv));
        assert!(SyscallSet::ALL.permits(ViSyscall::Log));
    }

    #[test]
    fn syscall_set_with_adds_bit() {
        let set = SyscallSet::EMPTY.with(ViSyscall::Send);
        assert!(set.permits(ViSyscall::Send));
    }

    #[test]
    fn syscall_set_does_not_permit_unset() {
        let set = SyscallSet::EMPTY.with(ViSyscall::Send);
        assert!(!set.permits(ViSyscall::Recv));
        assert!(!set.permits(ViSyscall::Log));
    }

    #[test]
    fn syscall_set_always_permitted_syscalls() {
        // Exit, Yield, and NotifyOnExit have no allowlist bit — permits() returns
        // true regardless of the stored bitmask (they are always allowed).
        assert!(SyscallSet::EMPTY.permits(ViSyscall::Exit));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::Yield));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::NotifyOnExit));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::ResolveCellOwner));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::WatchCellOwner));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::CancelCellOwnerWatch));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::ResolveCellOwnerRecord));
        assert!(SyscallSet::EMPTY.permits(ViSyscall::WatchCellOwnerRecord));
    }

    #[test]
    fn declare_syscalls_bits_are_stable() {
        // Verifies the known bit assignments used by declare_syscalls!.
        // Send=bit0, Recv=bit1, Log=bit10 → mask = 1|2|1024 = 0x403.
        // If any of these asserts fail, cells with declare_syscalls![Send, Recv, Log]
        // will produce a different allowlist mask — a breaking ABI change.
        assert_eq!(ViSyscall::Send.allowlist_bit(), Some(0));
        assert_eq!(ViSyscall::Recv.allowlist_bit(), Some(1));
        assert_eq!(ViSyscall::Log.allowlist_bit(), Some(10));
        assert_eq!(ViSyscall::GetProcs.allowlist_bit(), Some(14));
        assert_eq!(ViSyscall::GetProcs2.allowlist_bit(), Some(55));
        assert_eq!(ViSyscall::MemInfo.allowlist_bit(), Some(56));
        assert_eq!(ViSyscall::SpawnReplacement.allowlist_bit(), Some(57));
        assert_eq!(ViSyscall::PauseService.allowlist_bit(), Some(49));

        let mask = SyscallSet::EMPTY
            .with(ViSyscall::Send)
            .with(ViSyscall::Recv)
            .with(ViSyscall::Log)
            .0;
        assert_eq!(mask, 0x403u64, "bit-packing mismatch: got {:#x}", mask);
    }
}
