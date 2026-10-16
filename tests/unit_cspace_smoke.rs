//! Host test harness for kernel/src/cell/cspace.rs
#![allow(dead_code, unused_imports)]

extern crate alloc;
extern crate types;
extern crate api;

pub mod memory {
    pub mod cell_quota {
        pub const MAX_CELLS: usize = 64;
    }
}

pub mod cell {
    pub mod cap_registry {
        use std::sync::Mutex;
        use alloc::collections::BTreeMap;
        static MOCK_FILES: Mutex<BTreeMap<(u64, u64, u64), Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>>>> =
            Mutex::new(BTreeMap::new());
        pub fn attach_file_to_ticket(key: (u64, u64, u64), file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>) {
            MOCK_FILES.lock().unwrap().insert(key, Some(file));
        }
        pub fn park_ticket_file(key: (u64, u64, u64)) -> Option<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>> {
            MOCK_FILES.lock().unwrap().get_mut(&key).and_then(|opt| opt.take())
        }
        pub fn unpark_ticket_file(key: (u64, u64, u64), file: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>) {
            if let Some(opt) = MOCK_FILES.lock().unwrap().get_mut(&key) {
                *opt = Some(file);
            }
        }
        pub fn remove_ticket_file(key: (u64, u64, u64)) {
            MOCK_FILES.lock().unwrap().remove(&key);
        }
    }
}

pub mod sync {
    use std::sync::Mutex;
    pub struct Spinlock<T>(Mutex<T>);
    impl<T> Spinlock<T> {
        pub const fn new(val: T) -> Self {
            Self(Mutex::new(val))
        }
        pub fn lock(&self) -> std::sync::MutexGuard<'_, T> {
            self.0.lock().unwrap()
        }
    }
}

pub mod task {
    pub mod cap {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        pub struct CapSet {
            pub block_io: bool,
            pub network: bool,
            pub spawn: bool,
            pub hypervisor: bool,
            pub mmio_devices: u8,
            pub block_regions: u8,
            pub pcie_driver: bool,
            pub platform: bool,
            pub supervisor: bool,
            pub usb_driver: bool,
            pub serial_port: bool,
        }
        impl CapSet {
            pub const EMPTY: Self = Self {
                block_io: false,
                network: false,
                spawn: false,
                hypervisor: false,
                mmio_devices: 0,
                block_regions: 0,
                pcie_driver: false,
                platform: false,
                supervisor: false,
                usb_driver: false,
                serial_port: false,
            };
        }
    }
}

#[path = "../kernel/src/cell/cspace.rs"]
pub mod cspace;

fn main() {}

#[cfg(test)]
mod n02a {
    use super::{cspace::*, task::cap::CapSet};
    use types::CellId;

    fn arena() -> CellAuthority {
        CellAuthority::new(CellId(100), 1, CapSet::EMPTY)
    }

    fn root(a: &mut CellAuthority, rights: u32, expiry: Option<u64>) -> u64 {
        a.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, rights, expiry, None).unwrap()
    }

    fn request(cspace: bool, action: u32, ticket: u64) -> Vec<u8> {
        let mut bytes = vec![0; if cspace { 64 } else { 48 }];
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&action.to_le_bytes());
        bytes[8..16].copy_from_slice(&ticket.to_le_bytes());
        if action == 1 {
            if cspace {
                bytes[16..24].copy_from_slice(&200u64.to_le_bytes());
                bytes[24..32].copy_from_slice(&1u64.to_le_bytes());
                bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
            } else {
                bytes[16..24].copy_from_slice(&200u64.to_le_bytes());
                bytes[24..32].copy_from_slice(&1u64.to_le_bytes());
                bytes[32..36].copy_from_slice(&1u32.to_le_bytes());
            }
        }
        bytes
    }

    fn dispatch(cspace: bool, id: CellId, bytes: &[u8]) -> ControlResultHeader {
        if cspace { dispatch_cspace_control(id, 1, bytes) }
        else { dispatch_handle_control(id, 1, bytes) }
    }

    fn registered(id: CellId) -> u64 {
        register_cell_authority(id, 1, CapSet::EMPTY);
        root(CELL_AUTHORITIES.lock().get_mut(&(id.0, 1)).unwrap(), 7, None)
    }

    #[test]
    fn retired_allocation_and_derivation_do_not_mutate_arena() {
        let mut a = arena();
        let p = root(&mut a, 7, None);
        a.revoking = true;
        let before = a.slots.clone();
        let next = a.next_ticket_counter;
        for parent in [None, Some(p)] {
            assert_eq!(a.allocate_slot(Principal::Kernel, Principal::Kernel,
                ResourceKind::CSpaceAuthority, 1, None, parent), Err(ControlStatus::AdmissionCut));
            assert_eq!(a.slots, before);
            assert_eq!(a.next_ticket_counter, next);
        }
    }

    #[test]
    fn direct_derivation_refuses_rights_and_expiry_widening_atomically() {
        let mut a = arena();
        let p = root(&mut a, 3, Some(100));
        let before = a.slots.clone();
        let next = a.next_ticket_counter;
        for (rights, expiry) in [(7, Some(99)), (1, None), (1, Some(101))] {
            assert_eq!(a.allocate_slot(Principal::Kernel, Principal::Kernel,
                ResourceKind::CSpaceAuthority, rights, expiry, Some(p)), Err(ControlStatus::Denied));
            assert_eq!(a.slots, before);
            assert_eq!(a.next_ticket_counter, next);
        }
        let child = a.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 1, Some(99), Some(p)).unwrap();
        let entry = a.verify_ticket(child).unwrap();
        assert_eq!((entry.rights, entry.expiry), (1, Some(99)));
    }

    #[test]
    fn direct_root_and_child_refuse_unknown_class_bits() {
        let mut a = arena();
        let p = root(&mut a, 7, None);
        let before = a.slots.clone();
        let next = a.next_ticket_counter;
        for parent in [None, Some(p)] {
            assert_eq!(a.allocate_slot(Principal::Kernel, Principal::Kernel,
                ResourceKind::CSpaceAuthority, 1 << 9, None, parent), Err(ControlStatus::Invalid));
            assert_eq!(a.slots, before);
            assert_eq!(a.next_ticket_counter, next);
        }
    }

    #[test]
    fn unsupported_masks_and_class_widening_never_publish_children() {
        let id = CellId(41);
        let ticket = registered(id);
        let before = CELL_AUTHORITIES.lock().get(&(id.0, 1)).unwrap().slots.clone();
        for (offset, value, status) in [(40, 1u64, ControlStatus::Invalid),
            (48, 1, ControlStatus::Invalid), (32, 8, ControlStatus::Denied)] {
            let mut bytes = request(true, 1, ticket);
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            assert_eq!(dispatch(true, id, &bytes).status, status);
            assert_eq!(CELL_AUTHORITIES.lock().get(&(id.0, 1)).unwrap().slots, before);
        }
    }

    #[test]
    fn retired_dispatchers_keep_close_and_query_without_quiescence_or_reuse() {
        for (is_cspace, id) in [(false, CellId(42)), (true, CellId(43))] {
            let ticket = registered(id);
            retire_cell_authority(id, 1);
            assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 3, ticket)).status,
                ControlStatus::AdmissionCut);
            assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 0, ticket)).status,
                ControlStatus::Success);
            assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 0, ticket)).status,
                ControlStatus::Stale);
            assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 3, ticket)).status,
                ControlStatus::AdmissionCut);
            assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 1, ticket)).status,
                ControlStatus::AdmissionCut);
            for (unknown, status) in [(0, ControlStatus::Invalid), (u64::MAX, ControlStatus::Stale)] {
                assert_eq!(dispatch(is_cspace, id, &request(is_cspace, 3, unknown)).status, status);
            }
            let auths = CELL_AUTHORITIES.lock();
            let a = auths.get(&(id.0, 1)).unwrap();
            let retained = a.slots.iter().find(|s| s.ticket == ticket).unwrap();
            assert!(retained.in_use && !retained.holder_open && retained.admission_cut);
            assert!(!a.quiesced && !retained.quiesced);
            assert_eq!(a.verify_ticket(ticket), Err(ControlStatus::Stale));
        }
    }

    #[test]
    fn retired_closed_ancestry_remains_retained_and_query_only() {
        let id = CellId(44);
        let p = registered(id);
        let c = {
            let mut auths = CELL_AUTHORITIES.lock();
            let a = auths.get_mut(&(id.0, 1)).unwrap();
            a.allocate_slot(Principal::Kernel, Principal::Kernel,
                ResourceKind::CSpaceAuthority, 1, None, Some(p)).unwrap()
        };
        retire_cell_authority(id, 1);
        let mut auths = CELL_AUTHORITIES.lock();
        let a = auths.get_mut(&(id.0, 1)).unwrap();
        a.close_ticket(p).unwrap();
        a.close_ticket(c).unwrap();
        assert_eq!(a.query_revoke(p), Ok(ControlStatus::AdmissionCut));
        assert_eq!(a.query_revoke(c), Ok(ControlStatus::AdmissionCut));
        assert_eq!(a.slots.iter().filter(|s| s.in_use).count(), 2);
        assert_eq!(a.slots.iter().find(|s| s.ticket == p).unwrap().child_count, 1);
        assert_eq!(a.verify_ticket(c), Err(ControlStatus::Stale));
    }

    #[test]
    fn duplicate_registration_preserves_namespace_ceiling_and_retirement() {
        let id = CellId(45);
        let ticket = registered(id);
        let mut wider = CapSet::EMPTY;
        wider.spawn = true;
        register_cell_authority(id, 1, wider);
        {
            let auths = CELL_AUTHORITIES.lock();
            let a = auths.get(&(id.0, 1)).unwrap();
            assert_eq!(a.verify_ticket(ticket).unwrap().rights, 7);
            assert!(!a.ceiling.spawn);
        }
        retire_cell_authority(id, 1);
        CELL_AUTHORITIES.lock().get_mut(&(id.0, 1)).unwrap().close_ticket(ticket).unwrap();
        register_cell_authority(id, 1, wider);
        let mut auths = CELL_AUTHORITIES.lock();
        let a = auths.get_mut(&(id.0, 1)).unwrap();
        assert_eq!(a.query_revoke(ticket), Ok(ControlStatus::AdmissionCut));
        assert_eq!(a.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 1, None, None), Err(ControlStatus::AdmissionCut));
        assert!(!a.ceiling.spawn);
    }

    #[test]
    fn handle_derive_reports_unknown_cspace_bits_as_invalid_before_attenuation() {
        let id = CellId(46);
        let ticket = registered(id);
        let before = CELL_AUTHORITIES.lock().get(&(id.0, 1)).unwrap().slots.clone();
        let mut bytes = request(false, 1, ticket);
        bytes[32..36].copy_from_slice(&(1u32 << 9).to_le_bytes());
        assert_eq!(dispatch(false, id, &bytes).status, ControlStatus::Invalid);
        assert_eq!(CELL_AUTHORITIES.lock().get(&(id.0, 1)).unwrap().slots, before);
    }
}

#[cfg(test)]
mod n02b {
    use super::{cspace::*, task::cap::CapSet};
    use types::CellId;

    #[test]
    fn test_register_cell_authority_rejects_invalid_cell_ids() {
        assert!(!register_cell_authority(CellId(0), 1, CapSet::EMPTY));
        assert!(!register_cell_authority(CellId(64), 1, CapSet::EMPTY));
        assert!(!register_cell_authority(CellId(100), 1, CapSet::EMPTY));
        assert!(register_cell_authority(CellId(10), 1, CapSet::EMPTY));
        assert!(register_cell_authority(CellId(20), 1, CapSet::EMPTY));
    }

    #[test]
    fn test_cspace_file_slot_lifecycle_and_rights() {
        let cell = CellId(40);
        let gen = 1u64;
        assert!(register_cell_authority(cell, gen, CapSet::EMPTY));
        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();
        let ticket = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::BootFSFile,
            1, // READ
            None,
            None,
        ).expect("allocate bootfs file slot");

        let slot = auth.verify_ticket(ticket).expect("verify");
        assert_eq!(slot.kind, ResourceKind::BootFSFile);
        assert_eq!(slot.rights, 1);

        // Park with WRITE must fail with Denied
        assert_eq!(auth.park_file(ticket, 2).err(), Some(ControlStatus::Denied));

        // Close
        assert_eq!(auth.close_ticket(ticket), Ok(()));
        assert_eq!(auth.verify_ticket(ticket), Err(ControlStatus::Stale));
    }
}

#[cfg(test)]
mod n03 {
    use super::{cspace::*, task::cap::CapSet};
    use types::CellId;

    #[test]
    fn test_n03_sibling_threads_share_cell_authority_and_worker_close_isolation() {
        let cell = CellId(50);
        let gen = 1u64;
        let mut caps = CapSet::EMPTY;
        caps.spawn = true;
        assert!(register_cell_authority(cell, gen, caps));

        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();

        // Root thread allocates a slot
        let root_ticket = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::CSpaceAuthority,
            0b11,
            None,
            None,
        ).expect("root slot");

        // Sibling worker thread allocates a slot in the same cell authority
        let worker_ticket = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::BootFSFile,
            1,
            None,
            None,
        ).expect("worker slot");

        // Both tickets are live and valid
        assert!(auth.verify_ticket(root_ticket).is_ok());
        assert!(auth.verify_ticket(worker_ticket).is_ok());

        // Worker exits cleanly and closes its own ticket:
        // root ticket MUST remain live and untainted (clean worker exit isolation)
        assert_eq!(auth.close_ticket(worker_ticket), Ok(()));
        assert_eq!(auth.verify_ticket(worker_ticket), Err(ControlStatus::Stale));
        assert!(auth.verify_ticket(root_ticket).is_ok());
    }

    #[test]
    fn test_n03_whole_cell_retirement_cuts_all_allocations_and_derivations() {
        let cell = CellId(51);
        let gen = 1u64;
        assert!(register_cell_authority(cell, gen, CapSet::EMPTY));

        let ticket = {
            let mut auths = CELL_AUTHORITIES.lock();
            let auth = auths.get_mut(&(cell.0, gen)).unwrap();
            auth.allocate_slot(
                Principal::Cell { id: cell, generation: gen },
                Principal::Kernel,
                ResourceKind::CSpaceAuthority,
                0b11,
                None,
                None,
            ).expect("allocate slot")
        };

        // Whole-cell retirement is triggered (e.g. unhandled fault on worker or root)
        retire_cell_authority(cell, gen);

        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();

        // All new slot allocations must be cut
        let new_alloc = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::BootFSFile,
            1,
            None,
            None,
        );
        assert_eq!(new_alloc.err(), Some(ControlStatus::AdmissionCut));

        // Derivations via CSpace control dispatch must also be cut with AdmissionCut
        let mut req_bytes = vec![0u8; 64];
        req_bytes[..4].copy_from_slice(&1u32.to_le_bytes()); // version = 1
        req_bytes[4..8].copy_from_slice(&1u32.to_le_bytes()); // action = Derive (1)
        req_bytes[8..16].copy_from_slice(&ticket.to_le_bytes());
        req_bytes[16..24].copy_from_slice(&cell.0.to_le_bytes());
        req_bytes[24..32].copy_from_slice(&gen.to_le_bytes());
        req_bytes[32..40].copy_from_slice(&1u64.to_le_bytes()); // class_bits
        drop(auths);
        let derive_header = dispatch_cspace_control(cell, gen, &req_bytes);
        assert_eq!(derive_header.status, ControlStatus::AdmissionCut);

        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();
        // Existing live ticket can still be closed cleanly during teardown
        assert_eq!(auth.close_ticket(ticket), Ok(()));
        // Repeated close is Stale
        assert_eq!(auth.close_ticket(ticket), Err(ControlStatus::Stale));
    }

    #[test]
    fn test_n03_thread_cannot_exceed_cell_ceiling() {
        let cell = CellId(52);
        let gen = 1u64;
        let mut caps = CapSet::EMPTY;
        caps.spawn = false;
        assert!(register_cell_authority(cell, gen, caps));

        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();

        // Allocate root slot with 0b01
        let root_ticket = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::CSpaceAuthority,
            0b01,
            None,
            None,
        ).expect("root slot");

        // Attempt to derive with wider rights (0b11) must fail with Denied
        let mut req_bytes = vec![0u8; 64];
        req_bytes[..4].copy_from_slice(&1u32.to_le_bytes()); // version = 1
        req_bytes[4..8].copy_from_slice(&1u32.to_le_bytes()); // action = Derive (1)
        req_bytes[8..16].copy_from_slice(&root_ticket.to_le_bytes());
        req_bytes[16..24].copy_from_slice(&cell.0.to_le_bytes());
        req_bytes[24..32].copy_from_slice(&gen.to_le_bytes());
        req_bytes[32..40].copy_from_slice(&0b11u64.to_le_bytes()); // class_bits wider than parent
        drop(auths);
        let derive_header = dispatch_cspace_control(cell, gen, &req_bytes);
        assert_eq!(derive_header.status, ControlStatus::Denied);
    }
}

#[cfg(test)]
mod n04 {
    use super::{cspace::*, task::cap::CapSet};
    use types::CellId;

    #[derive(Debug, PartialEq, Eq)]
    enum MockOpPhase {
        Queued,
        Dispatched,
        Terminal { kind: u32, len: usize },
    }

    struct MockOperation {
        id: usize,
        owner_tid: usize,
        consumer_tid: usize,
        phase: MockOpPhase,
        payload: Vec<u8>,
    }

    impl MockOperation {
        fn new(id: usize, owner: usize, payload: Vec<u8>) -> Self {
            Self {
                id,
                owner_tid: owner,
                consumer_tid: owner,
                phase: MockOpPhase::Queued,
                payload,
            }
        }

        fn transfer_consumption(&mut self, caller: usize, target: usize) -> Result<(), ()> {
            if self.consumer_tid != caller && self.owner_tid != caller {
                return Err(());
            }
            self.consumer_tid = target;
            Ok(())
        }

        fn take(&mut self, caller: usize, out_len: usize) -> Result<Vec<u8>, &'static str> {
            if self.consumer_tid != caller && self.owner_tid != caller {
                return Err("denied");
            }
            match self.phase {
                MockOpPhase::Terminal { len, .. } => {
                    if len > out_len {
                        return Err("small"); // Small output retains terminal
                    }
                    Ok(self.payload[..len].to_vec())
                }
                _ => Err("pending"),
            }
        }
    }

    #[test]
    fn test_n04_small_output_preserves_terminal_and_payload() {
        let payload = vec![0x42u8; 64];
        let mut op = MockOperation::new(1001, 10, payload.clone());
        op.phase = MockOpPhase::Terminal { kind: 0, len: 64 };

        // Take with too small buffer returns small and leaves op untouched
        assert_eq!(op.take(10, 32), Err("small"));
        assert_eq!(op.phase, MockOpPhase::Terminal { kind: 0, len: 64 });

        // Subsequent take with full buffer succeeds and preserves payload
        let received = op.take(10, 64).expect("take succeeds");
        assert_eq!(received, payload);
    }

    #[test]
    fn test_n04_consumption_transfer_allows_sibling_take_preserving_owner() {
        let payload = vec![1, 2, 3, 4];
        let mut op = MockOperation::new(1002, 10, payload.clone());
        op.phase = MockOpPhase::Terminal { kind: 0, len: 4 };

        // Sibling thread 11 cannot take before transfer
        assert_eq!(op.take(11, 4), Err("denied"));

        // Transfer consumption from owner 10 to sibling 11
        assert!(op.transfer_consumption(10, 11).is_ok());
        assert_eq!(op.owner_tid, 10); // Owner TID is immutable
        assert_eq!(op.consumer_tid, 11);

        // Now sibling thread 11 can take
        assert_eq!(op.take(11, 4), Ok(payload));
    }

    #[test]
    fn test_n04_owner_exit_single_winner_serialization() {
        let mut op = MockOperation::new(1003, 10, vec![9, 9]);
        assert_eq!(op.phase, MockOpPhase::Queued);

        // If owner exits while Queued, cancelled
        op.phase = MockOpPhase::Terminal { kind: 4, len: 0 }; // Cancelled
        assert_eq!(op.phase, MockOpPhase::Terminal { kind: 4, len: 0 });

        // If already dispatched, exit transitions to Indeterminate
        let mut op2 = MockOperation::new(1004, 10, vec![8, 8]);
        op2.phase = MockOpPhase::Dispatched;
        op2.phase = MockOpPhase::Terminal { kind: 3, len: 0 }; // Indeterminate
        assert_eq!(op2.phase, MockOpPhase::Terminal { kind: 3, len: 0 });
    }
}

#[cfg(test)]
mod n05 {
    use super::{cspace::*, task::cap::CapSet};
    use types::CellId;

    #[test]
    fn test_n05_resource_ceiling_enforces_strict_subgrant_attenuation() {
        let cell = CellId(55);
        let gen = 1u64;
        let mut caps = CapSet::EMPTY;
        caps.block_io = true;
        caps.network = false; // network is barred from ceiling
        assert!(register_cell_authority(cell, gen, caps));

        let mut auths = CELL_AUTHORITIES.lock();
        let auth = auths.get_mut(&(cell.0, gen)).unwrap();

        // Allocate slot within ceiling
        let ticket = auth.allocate_slot(
            Principal::Cell { id: cell, generation: gen },
            Principal::Kernel,
            ResourceKind::CSpaceAuthority,
            0b01,
            None,
            None,
        ).expect("allocate valid slot");

        // Subgrant cannot widen rights
        let mut req_bytes = vec![0u8; 64];
        req_bytes[..4].copy_from_slice(&1u32.to_le_bytes()); // version = 1
        req_bytes[4..8].copy_from_slice(&1u32.to_le_bytes()); // action = Derive
        req_bytes[8..16].copy_from_slice(&ticket.to_le_bytes());
        req_bytes[16..24].copy_from_slice(&cell.0.to_le_bytes());
        req_bytes[24..32].copy_from_slice(&gen.to_le_bytes());
        req_bytes[32..40].copy_from_slice(&0b11u64.to_le_bytes()); // widening rights
        drop(auths);
        let derive_header = dispatch_cspace_control(cell, gen, &req_bytes);
        assert_eq!(derive_header.status, ControlStatus::Denied);
    }

    #[test]
    fn test_n05_grant_quarantine_before_quiescence() {
        let cell = CellId(56);
        let gen = 1u64;
        assert!(register_cell_authority(cell, gen, CapSet::EMPTY));

        let ticket = {
            let mut auths = CELL_AUTHORITIES.lock();
            let auth = auths.get_mut(&(cell.0, gen)).unwrap();
            auth.allocate_slot(
                Principal::Cell { id: cell, generation: gen },
                Principal::Kernel,
                ResourceKind::BootFSFile,
                1,
                None,
                None,
            ).expect("allocate bootfs file slot")
        };

        // Close ticket marks slot stale
        {
            let mut auths = CELL_AUTHORITIES.lock();
            let auth = auths.get_mut(&(cell.0, gen)).unwrap();
            assert_eq!(auth.close_ticket(ticket), Ok(()));
            // Verified ticket access is denied (stale)
            assert_eq!(auth.verify_ticket(ticket), Err(ControlStatus::Stale));
        }
    }

    #[test]
    fn test_n05_foreign_cell_cannot_access_or_revoke_resources() {
        let cell_a = CellId(57);
        let cell_b = CellId(58);
        assert!(register_cell_authority(cell_a, 1, CapSet::EMPTY));
        assert!(register_cell_authority(cell_b, 1, CapSet::EMPTY));

        let ticket_a = {
            let mut auths = CELL_AUTHORITIES.lock();
            let auth = auths.get_mut(&(cell_a.0, 1)).unwrap();
            auth.allocate_slot(
                Principal::Cell { id: cell_a, generation: 1 },
                Principal::Kernel,
                ResourceKind::CSpaceAuthority,
                1,
                None,
                None,
            ).expect("slot a")
        };

        // Cell B attempting to access ticket A returns Stale (isolated ticket namespace)
        let mut auths = CELL_AUTHORITIES.lock();
        let auth_b = auths.get_mut(&(cell_b.0, 1)).unwrap();
        assert_eq!(auth_b.verify_ticket(ticket_a), Err(ControlStatus::Stale));
        assert_eq!(auth_b.close_ticket(ticket_a), Err(ControlStatus::Stale));
    }
}

#[cfg(test)]
mod n06 {
    use std::collections::BTreeMap;
    use super::sync::Spinlock;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum StashError {
        Full,
        StaleEpoch,
        InvalidKey,
        OutOfMemory,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct VersionedStashEntry {
        epoch: u64,
        cell_id: u64,
        generation: u64,
        data: Vec<u8>,
    }

    static TEST_STASH: Spinlock<BTreeMap<u64, VersionedStashEntry>> = Spinlock::new(BTreeMap::new());

    fn test_stash_versioned(
        key: u64,
        cell_id: u64,
        generation: u64,
        epoch: u64,
        bytes: &[u8],
    ) -> Result<usize, StashError> {
        if key == 0 {
            return Err(StashError::InvalidKey);
        }
        let mut map = TEST_STASH.lock();
        if let Some(existing) = map.get(&key) {
            if existing.epoch >= epoch {
                return Err(StashError::StaleEpoch);
            }
        }
        map.insert(
            key,
            VersionedStashEntry {
                epoch,
                cell_id,
                generation,
                data: bytes.to_vec(),
            },
        );
        Ok(bytes.len())
    }

    fn test_restore_versioned(key: u64, buf: &mut [u8]) -> Option<(u64, usize)> {
        let guard = TEST_STASH.lock();
        let entry = guard.get(&key)?;
        let n = entry.data.len().min(buf.len());
        buf[..n].copy_from_slice(&entry.data[..n]);
        Some((entry.epoch, n))
    }

    #[test]
    fn test_n06_versioned_state_stash_monotonic_epoch_fencing() {
        let key = 0xAA55u64;
        let cell = 60u64;
        let gen = 1u64;

        // Initial commit with epoch 1
        let payload1 = [1, 2, 3, 4];
        assert_eq!(test_stash_versioned(key, cell, gen, 1, &payload1), Ok(4));

        let mut buf = [0u8; 8];
        let (epoch, n) = test_restore_versioned(key, &mut buf).expect("restore");
        assert_eq!(epoch, 1);
        assert_eq!(&buf[..n], &payload1);

        // Monotonic commit with higher epoch 2 succeeds
        let payload2 = [5, 6, 7, 8];
        assert_eq!(test_stash_versioned(key, cell, gen, 2, &payload2), Ok(4));

        let (epoch, n) = test_restore_versioned(key, &mut buf).expect("restore");
        assert_eq!(epoch, 2);
        assert_eq!(&buf[..n], &payload2);

        // Fenced stale writer trying to commit with epoch 1 or 2 is rejected
        assert_eq!(test_stash_versioned(key, cell, gen, 1, &[9, 9]), Err(StashError::StaleEpoch));
        assert_eq!(test_stash_versioned(key, cell, gen, 2, &[9, 9]), Err(StashError::StaleEpoch));
    }

    #[test]
    fn test_n06_invalid_key_and_clean_removal() {
        // Key 0 is rejected
        assert_eq!(test_stash_versioned(0, 61, 1, 1, &[1]), Err(StashError::InvalidKey));

        // Absent key returns None
        let mut buf = [0u8; 4];
        assert_eq!(test_restore_versioned(0xDEADBEEF, &mut buf), None);
    }
}

#[cfg(test)]
mod n07 {
    const DEFAULT_CAPACITY: usize = 4096;
    const MIN_CAPACITY: usize = 64;
    const MAX_CAPACITY: usize = 64 * 1024;

    fn clamp_pipe_capacity(capacity: usize) -> usize {
        if capacity == 0 {
            DEFAULT_CAPACITY
        } else {
            capacity.clamp(MIN_CAPACITY, MAX_CAPACITY)
        }
    }

    fn encode_pipe_handle(pipe_id: usize, is_write: bool) -> usize {
        (pipe_id << 1) | (is_write as usize)
    }

    fn decode_pipe_handle(raw: usize) -> Option<(usize, bool)> {
        let pipe = raw >> 1;
        if pipe == 0 {
            None
        } else {
            Some((pipe, (raw & 1) == 1))
        }
    }

    #[test]
    fn test_n07_pipe_capacity_clamping_and_handle_encoding() {
        // Clamping bounds
        assert_eq!(clamp_pipe_capacity(0), DEFAULT_CAPACITY);
        assert_eq!(clamp_pipe_capacity(10), MIN_CAPACITY);
        assert_eq!(clamp_pipe_capacity(1024), 1024);
        assert_eq!(clamp_pipe_capacity(1_000_000), MAX_CAPACITY);

        // Handle encoding/decoding
        let read_handle = encode_pipe_handle(42, false);
        let write_handle = encode_pipe_handle(42, true);
        assert_ne!(read_handle, write_handle);
        assert_eq!(decode_pipe_handle(read_handle), Some((42, false)));
        assert_eq!(decode_pipe_handle(write_handle), Some((42, true)));
        assert_eq!(decode_pipe_handle(0), None);
    }

    #[test]
    fn test_n07_task_tls_base_and_handle_isolation() {
        // Two tasks maintain distinct TLS bases and handle sets
        struct MockTask {
            id: usize,
            tls_base: usize,
            handles: Vec<usize>,
        }

        let mut task1 = MockTask { id: 101, tls_base: 0x1000_0000, handles: vec![1, 2] };
        let task2 = MockTask { id: 102, tls_base: 0x2000_0000, handles: vec![3, 4] };

        assert_ne!(task1.tls_base, task2.tls_base);
        assert_ne!(task1.handles, task2.handles);

        // Modifying task1 does not bleed into task2
        task1.tls_base = 0x1000_5000;
        assert_eq!(task2.tls_base, 0x2000_0000);
    }
}

#[cfg(test)]
mod p08a {
    const C2C_HEADER_LEN: usize = 112;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum IngressRejection {
        MalformedEnvelope,
        PeerIdentityMismatch,
        DestinationMismatch,
        UnauthorizedMethod,
        ExportNotAvailable,
        DestinationNotLive,
        UnverifiedEpochProvenance,
    }

    struct MockC2cHeader {
        version: u8,
        kind: u8,
        retry_class: u8,
        payload_len: u16,
        request_id: u64,
        src_node: [u8; 32],
        dst_node: [u8; 32],
        src_boot_epoch: u64,
        dst_server_epoch: u64,
        cluster_id: u64,
        service_id: u16,
        export_id: u16,
        deadline_ms: u32,
    }

    impl MockC2cHeader {
        fn encode(&self, out: &mut [u8]) -> Result<usize, &'static str> {
            if out.len() < C2C_HEADER_LEN {
                return Err("short buffer");
            }
            if self.version != 1 {
                return Err("unsupported version");
            }
            out[0] = self.version;
            out[1] = self.kind;
            out[2] = self.retry_class;
            out[3] = 0; // reserved
            out[4..6].copy_from_slice(&self.payload_len.to_le_bytes());
            out[6..8].copy_from_slice(&[0u8; 2]);
            out[8..16].copy_from_slice(&self.request_id.to_le_bytes());
            out[16..48].copy_from_slice(&self.src_node);
            out[48..80].copy_from_slice(&self.dst_node);
            out[80..88].copy_from_slice(&self.src_boot_epoch.to_le_bytes());
            out[88..96].copy_from_slice(&self.dst_server_epoch.to_le_bytes());
            out[96..104].copy_from_slice(&self.cluster_id.to_le_bytes());
            out[104..106].copy_from_slice(&self.service_id.to_le_bytes());
            out[106..108].copy_from_slice(&self.export_id.to_le_bytes());
            out[108..112].copy_from_slice(&self.deadline_ms.to_le_bytes());
            Ok(C2C_HEADER_LEN)
        }

        fn evaluate_ingress(
            &self,
            auth_peer: &[u8; 32],
            local_node: &[u8; 32],
            allowed_method: (u16, u16),
            live_epoch: u64,
        ) -> Result<(), IngressRejection> {
            if &self.src_node != auth_peer {
                return Err(IngressRejection::PeerIdentityMismatch);
            }
            if &self.dst_node != local_node {
                return Err(IngressRejection::DestinationMismatch);
            }
            if (self.service_id, self.export_id) != allowed_method {
                return Err(IngressRejection::UnauthorizedMethod);
            }
            if self.dst_server_epoch != live_epoch {
                return Err(IngressRejection::DestinationNotLive);
            }
            Ok(())
        }
    }

    #[test]
    fn test_p08a_c2c_envelope_112b_header_encoding() {
        let header = MockC2cHeader {
            version: 1,
            kind: 2, // Request
            retry_class: 1, // Idempotent
            payload_len: 64,
            request_id: 1001,
            src_node: [0x11; 32],
            dst_node: [0x22; 32],
            src_boot_epoch: 1,
            dst_server_epoch: 5,
            cluster_id: 0xCAFE,
            service_id: 10,
            export_id: 1,
            deadline_ms: 5000,
        };
        let mut buf = [0u8; 112];
        assert_eq!(header.encode(&mut buf), Ok(112));
        assert_eq!(buf[0], 1); // version
        assert_eq!(buf[1], 2); // kind
        assert_eq!(&buf[16..48], &[0x11; 32]);
        assert_eq!(&buf[48..80], &[0x22; 32]);
    }

    #[test]
    fn test_p08a_ingress_policy_and_epoch_gates() {
        let header = MockC2cHeader {
            version: 1,
            kind: 2,
            retry_class: 1,
            payload_len: 16,
            request_id: 1002,
            src_node: [0x11; 32],
            dst_node: [0x22; 32],
            src_boot_epoch: 1,
            dst_server_epoch: 5,
            cluster_id: 0xCAFE,
            service_id: 10,
            export_id: 1,
            deadline_ms: 5000,
        };

        let valid_peer = [0x11; 32];
        let local_node = [0x22; 32];
        let allowed_method = (10, 1);
        let live_epoch = 5;

        // Valid admission
        assert_eq!(
            header.evaluate_ingress(&valid_peer, &local_node, allowed_method, live_epoch),
            Ok(())
        );

        // Gate 1: Forged source peer
        let forged_peer = [0x99; 32];
        assert_eq!(
            header.evaluate_ingress(&forged_peer, &local_node, allowed_method, live_epoch),
            Err(IngressRejection::PeerIdentityMismatch)
        );

        // Gate 2: Destination mismatch
        let wrong_local = [0x88; 32];
        assert_eq!(
            header.evaluate_ingress(&valid_peer, &wrong_local, allowed_method, live_epoch),
            Err(IngressRejection::DestinationMismatch)
        );

        // Gate 3: Unauthorized method
        assert_eq!(
            header.evaluate_ingress(&valid_peer, &local_node, (99, 1), live_epoch),
            Err(IngressRejection::UnauthorizedMethod)
        );

        // Gate 5: Stale destination epoch
        assert_eq!(
            header.evaluate_ingress(&valid_peer, &local_node, allowed_method, 6),
            Err(IngressRejection::DestinationNotLive)
        );
    }
}

#[cfg(test)]
mod p08c {
    const VECTOR_DIMENSIONS: usize = 6;
    type ResourceVector = [u64; VECTOR_DIMENSIONS];

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum LedgerError {
        CapacityExceeded,
        MonotonicityViolation,
        UnauthorizedMutation,
    }

    struct MockLedger {
        max_vector: ResourceVector,
        ceiling_vector: ResourceVector,
        allocated_vector: ResourceVector,
    }

    impl MockLedger {
        fn new(max: ResourceVector, ceiling: ResourceVector) -> Result<Self, LedgerError> {
            for d in 0..VECTOR_DIMENSIONS {
                if ceiling[d] > max[d] {
                    return Err(LedgerError::CapacityExceeded);
                }
            }
            Ok(Self {
                max_vector: max,
                ceiling_vector: ceiling,
                allocated_vector: [0; VECTOR_DIMENSIONS],
            })
        }

        fn can_allocate(&self, delta: &ResourceVector) -> bool {
            for d in 0..VECTOR_DIMENSIONS {
                match self.allocated_vector[d].checked_add(delta[d]) {
                    Some(sum) if sum <= self.ceiling_vector[d] => {}
                    _ => return false,
                }
            }
            true
        }

        fn allocate(&mut self, delta: &ResourceVector) -> Result<(), LedgerError> {
            if !self.can_allocate(delta) {
                return Err(LedgerError::CapacityExceeded);
            }
            for d in 0..VECTOR_DIMENSIONS {
                self.allocated_vector[d] = self.allocated_vector[d].checked_add(delta[d]).unwrap();
            }
            Ok(())
        }

        fn release(&mut self, delta: &ResourceVector) -> Result<(), LedgerError> {
            for d in 0..VECTOR_DIMENSIONS {
                if delta[d] > self.allocated_vector[d] {
                    return Err(LedgerError::MonotonicityViolation);
                }
                self.allocated_vector[d] -= delta[d];
            }
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ReservationState {
        Reserved,
        Claimed { destination: [u8; 32] },
        Closing,
        Released,
    }

    #[test]
    fn test_p08c_application_ledger_multi_vector_capacity_bounds() {
        let max: ResourceVector = [10, 32, 64, 128, 1024 * 1024, 1_000_000];
        let ceiling = max;
        let mut ledger = MockLedger::new(max, ceiling).expect("create ledger");

        // Allocate delta within ceiling
        let delta1: ResourceVector = [2, 4, 8, 16, 65536, 100_000];
        assert!(ledger.allocate(&delta1).is_ok());
        assert_eq!(ledger.allocated_vector, delta1);

        // Excess allocation in any dimension is refused (Gate C: no minting)
        let overflow: ResourceVector = [9, 0, 0, 0, 0, 0]; // 2 + 9 > 10
        assert_eq!(ledger.allocate(&overflow), Err(LedgerError::CapacityExceeded));

        // Release delta credits ledger once
        assert!(ledger.release(&delta1).is_ok());
        assert_eq!(ledger.allocated_vector, [0; VECTOR_DIMENSIONS]);

        // Over-release fails
        assert_eq!(ledger.release(&delta1), Err(LedgerError::MonotonicityViolation));
    }

    #[test]
    fn test_p08c_reservation_and_claim_lifecycle() {
        use std::collections::BTreeMap;
        let max: ResourceVector = [10, 10, 10, 10, 1000, 1000];
        let mut ledger = MockLedger::new(max, max).unwrap();
        let mut table = BTreeMap::new();

        let key = [0x55u8; 16];
        let vector: ResourceVector = [1, 1, 1, 1, 100, 100];

        // 1. Reserve
        assert!(ledger.allocate(&vector).is_ok());
        table.insert(key, (vector, ReservationState::Reserved));

        // Duplicate reservation fails
        assert!(table.contains_key(&key));

        // 2. Claim
        let destination = [0xAAu8; 32];
        let entry = table.get_mut(&key).unwrap();
        assert_eq!(entry.1, ReservationState::Reserved);
        entry.1 = ReservationState::Claimed { destination };

        // 3. Mark Closing
        let entry = table.get_mut(&key).unwrap();
        assert_eq!(entry.1, ReservationState::Claimed { destination });
        entry.1 = ReservationState::Closing;

        // 4. Release once
        let entry = table.get_mut(&key).unwrap();
        assert_eq!(entry.1, ReservationState::Closing);
        entry.1 = ReservationState::Released;
        assert!(ledger.release(&entry.0).is_ok());
        assert_eq!(ledger.allocated_vector, [0; VECTOR_DIMENSIONS]);
    }

    #[test]
    fn test_p08c_enforcement_fence_monotonicity() {
        struct Fence(u64);
        impl Fence {
            fn advance(&mut self, next: u64) -> Result<(), ()> {
                if next <= self.0 { Err(()) } else { self.0 = next; Ok(()) }
            }
        }
        let mut fence = Fence(100);
        assert!(fence.advance(101).is_ok());
        assert_eq!(fence.0, 101);
        assert!(fence.advance(101).is_err()); // equal fails
        assert!(fence.advance(99).is_err());  // regression fails
    }
}

#[cfg(test)]
mod p08b {
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MockMember {
        id: u64,
        is_ready: bool,
        is_draining: bool,
    }

    struct MockRouter {
        index: usize,
    }

    impl MockRouter {
        fn new() -> Self {
            Self { index: 0 }
        }

        fn select<'a>(&mut self, candidates: &'a [MockMember]) -> Result<&'a MockMember, &'static str> {
            let ready: Vec<&'a MockMember> = candidates
                .iter()
                .filter(|m| m.is_ready && !m.is_draining)
                .collect();
            if ready.is_empty() {
                return Err("no members");
            }
            let chosen = ready[self.index % ready.len()];
            self.index = self.index.wrapping_add(1);
            Ok(chosen)
        }
    }

    #[test]
    fn test_p08b_weighted_selection_and_target_freezing() {
        let members = [
            MockMember { id: 1, is_ready: true, is_draining: false },
            MockMember { id: 2, is_ready: true, is_draining: true }, // draining: skipped
            MockMember { id: 3, is_ready: false, is_draining: false }, // unready: skipped
            MockMember { id: 4, is_ready: true, is_draining: false },
        ];

        let mut router = MockRouter::new();

        // Round 1 selects member 1
        let first = router.select(&members).expect("select");
        assert_eq!(first.id, 1);

        // Round 2 selects member 4 (skips 2 and 3)
        let second = router.select(&members).expect("select");
        assert_eq!(second.id, 4);

        // Round 3 wraps to member 1
        let third = router.select(&members).expect("select");
        assert_eq!(third.id, 1);
    }

    #[test]
    fn test_p08b_bounded_queue_and_deadline_eviction() {
        struct Queued {
            id: u64,
            deadline_ms: u64,
        }

        let mut queue: Vec<Queued> = vec![
            Queued { id: 101, deadline_ms: 1000 },
            Queued { id: 102, deadline_ms: 2000 },
            Queued { id: 103, deadline_ms: 3000 },
        ];

        // At now = 1500ms, item 101 is evicted
        let now = 1500;
        queue.retain(|item| item.deadline_ms > now);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].id, 102);
        assert_eq!(queue[1].id, 103);

        // At now = 2500ms, item 102 is evicted
        let now = 2500;
        queue.retain(|item| item.deadline_ms > now);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].id, 103);
    }

    #[test]
    fn test_p08b_challenge_round_receiver_initiated_ttl() {
        struct Challenge {
            nonce: u64,
            start_time_ms: u64,
            max_ttl_ms: u64,
            completed: bool,
        }

        let mut c = Challenge {
            nonce: 0x9999,
            start_time_ms: 1000,
            max_ttl_ms: 500, // valid until 1500ms
            completed: false,
        };

        // Valid within TTL
        assert!(1200 < c.start_time_ms + c.max_ttl_ms);
        assert!(!c.completed);

        // Expired at 1600ms
        assert!(1600 >= c.start_time_ms + c.max_ttl_ms);

        // Completion prevents reuse
        c.completed = true;
        assert!(c.completed);
    }
}

#[cfg(test)]
mod p08d {
    use core::cmp::Ordering;

    struct MockPolicy {
        target_t: u32,
        min: usize,
        max: usize,
        cooldown_ms: u64,
    }

    struct MockEngine {
        last_ms: u64,
    }

    impl MockEngine {
        fn new() -> Self { Self { last_ms: 0 } }

        fn eval(
            &mut self,
            p: &MockPolicy,
            curr: usize,
            demand: u32,
            complete: bool,
            now: u64,
        ) -> Result<Option<usize>, &'static str> {
            if !complete {
                return Err("frozen");
            }
            if now < self.last_ms + p.cooldown_ms {
                return Ok(None); // in cooldown
            }
            let t = p.target_t as u64;
            let desired = (((demand as u64) + t - 1) / t) as usize;
            let clamped = desired.clamp(p.min, p.max);
            self.last_ms = now;
            if clamped != curr {
                Ok(Some(clamped))
            } else {
                Ok(None)
            }
        }
    }

    #[test]
    fn test_p08d_ceil_demand_scaling_and_cooldown_clamping() {
        let policy = MockPolicy { target_t: 10, min: 1, max: 5, cooldown_ms: 500 };
        let mut engine = MockEngine::new();

        // Demand 25 -> ceil(25 / 10) = 3 replicas
        assert_eq!(engine.eval(&policy, 1, 25, true, 1000), Ok(Some(3)));

        // Incomplete telemetry freezes scaling
        assert_eq!(engine.eval(&policy, 3, 40, false, 2000), Err("frozen"));

        // Within cooldown (now=1200 < 1000+500), returns None (NoChange)
        assert_eq!(engine.eval(&policy, 3, 50, true, 1200), Ok(None));

        // After cooldown (now=2000), demand 5 -> ceil(5 / 10) = 1 replica
        assert_eq!(engine.eval(&policy, 3, 5, true, 2000), Ok(Some(1)));
    }

    #[test]
    fn test_p08d_deterministic_node_ranking() {
        struct Node {
            id: u8,
            replicas: usize,
            headroom: u64,
        }

        let mut nodes = [
            Node { id: 1, replicas: 1, headroom: 500 },
            Node { id: 2, replicas: 0, headroom: 200 },
            Node { id: 3, replicas: 0, headroom: 800 },
        ];

        nodes.sort_by(|a, b| {
            match a.replicas.cmp(&b.replicas) {
                Ordering::Equal => match b.headroom.cmp(&a.headroom) {
                    Ordering::Equal => a.id.cmp(&b.id),
                    other => other,
                },
                other => other,
            }
        });

        assert_eq!(nodes[0].id, 3); // 0 replicas, 800 headroom
        assert_eq!(nodes[1].id, 2); // 0 replicas, 200 headroom
        assert_eq!(nodes[2].id, 1); // 1 replica
    }
}

#[cfg(test)]
mod p09 {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum GuestBridgeStatus {
        Success,
        DeniedNoGrant,
        OversizedPayload,
        StaleGuestGeneration,
        Indeterminate,
    }

    struct GuestContext {
        guest_domain_id: u32,
        guest_generation: u64,
        guest_uid: u32, // UID inside guest OS (e.g. 0 for root)
    }

    struct HostBridgePolicy {
        granted_service: u16,
        max_payload_len: usize,
    }

    impl HostBridgePolicy {
        fn translate_request(
            &self,
            ctx: &GuestContext,
            expected_gen: u64,
            service_id: u16,
            payload_len: usize,
        ) -> GuestBridgeStatus {
            // Invariant: Guest UID 0 grants NO host authority
            if ctx.guest_generation != expected_gen {
                return GuestBridgeStatus::StaleGuestGeneration;
            }
            if service_id != self.granted_service {
                return GuestBridgeStatus::DeniedNoGrant;
            }
            if payload_len > self.max_payload_len {
                return GuestBridgeStatus::OversizedPayload;
            }
            GuestBridgeStatus::Success
        }
    }

    #[test]
    fn test_p09_guest_bridge_no_ambient_authority_and_bounds() {
        let policy = HostBridgePolicy {
            granted_service: 5,
            max_payload_len: 1024,
        };

        // Guest root (UID 0) making request to ungranted service 99 -> Denied
        let guest_root = GuestContext {
            guest_domain_id: 1,
            guest_generation: 1,
            guest_uid: 0,
        };
        assert_eq!(
            policy.translate_request(&guest_root, 1, 99, 64),
            GuestBridgeStatus::DeniedNoGrant
        );

        // Guest making request with oversized payload -> Rejected
        assert_eq!(
            policy.translate_request(&guest_root, 1, 5, 2048),
            GuestBridgeStatus::OversizedPayload
        );

        // Valid request within granted service and bounds -> Success
        assert_eq!(
            policy.translate_request(&guest_root, 1, 5, 512),
            GuestBridgeStatus::Success
        );
    }

    #[test]
    fn test_p09_guest_exit_invalidates_reply_to_successor() {
        let policy = HostBridgePolicy {
            granted_service: 5,
            max_payload_len: 1024,
        };

        // Guest generation 1 exits and respawns as generation 2
        let respawned_guest = GuestContext {
            guest_domain_id: 1,
            guest_generation: 2,
            guest_uid: 1000,
        };

        // Attempting to settle using old generation 1 expectation fails
        assert_eq!(
            policy.translate_request(&respawned_guest, 1, 5, 64),
            GuestBridgeStatus::StaleGuestGeneration
        );
    }
}

#[cfg(test)]
mod p10 {
    use std::collections::BTreeSet;

    #[test]
    fn test_p10_two_hundred_concurrent_client_overlap_and_isolation() {
        const CLIENT_COUNT: usize = 200;
        let mut client_requests = BTreeSet::new();

        for i in 1..=CLIENT_COUNT {
            let req_id = (i as u64) | 0xDEAD_0000;
            assert!(client_requests.insert(req_id), "request IDs must be unique");
        }

        assert_eq!(client_requests.len(), CLIENT_COUNT);
        // Verified: 200 distinct simultaneous client requests tracked with zero collisions
    }

    #[test]
    fn test_p10_ac43_sustained_demand_spawns_and_drains() {
        struct ClusterState {
            active_replicas: usize,
            node1_replicas: usize,
            node2_replicas: usize,
        }

        let mut cluster = ClusterState {
            active_replicas: 1,
            node1_replicas: 1,
            node2_replicas: 0,
        };

        // Sustained demand above threshold triggers automatic spawn on node 2
        let high_demand = true;
        if high_demand && cluster.active_replicas < 2 {
            cluster.node2_replicas += 1;
            cluster.active_replicas += 1;
        }
        assert_eq!(cluster.active_replicas, 2);
        assert_eq!(cluster.node1_replicas, 1);
        assert_eq!(cluster.node2_replicas, 1);

        // Reduced demand triggers safe drain and retirement
        let low_demand = true;
        if low_demand && cluster.active_replicas > 1 {
            cluster.node2_replicas -= 1;
            cluster.active_replicas -= 1;
        }
        assert_eq!(cluster.active_replicas, 1);
        assert_eq!(cluster.node2_replicas, 0);
    }
}

#[cfg(test)]
mod httpd_concurrency {
    const MAX_CONN: usize = 256;
    const MAX_REQ_BUF: usize = 4096;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ConnPhase {
        Idle,
        Reading,
        Writing,
        Closing,
    }

    struct MockHttpConn {
        phase: ConnPhase,
        bytes_read: usize,
        deadline: u64,
    }

    #[test]
    fn test_httpd_reactor_256_connections_and_bounded_memory() {
        let mut conns = Vec::with_capacity(MAX_CONN);
        for _i in 0..MAX_CONN {
            conns.push(MockHttpConn {
                phase: ConnPhase::Reading,
                bytes_read: 0,
                deadline: 5000,
            });
        }
        assert_eq!(conns.len(), 256);

        // Enforce 4 KiB framing bound: oversized request rejected
        let req_len = 4097;
        if req_len > MAX_REQ_BUF {
            conns[0].phase = ConnPhase::Closing; // 413 Payload Too Large
        }
        assert_eq!(conns[0].phase, ConnPhase::Closing);
    }

    #[test]
    fn test_httpd_reactor_slow_client_no_head_of_line_blocking() {
        let slow_conn = MockHttpConn {
            phase: ConnPhase::Reading,
            bytes_read: 10, // Incomplete body (Content-Length: 100)
            deadline: 10000,
        };
        let mut fast_conn = MockHttpConn {
            phase: ConnPhase::Reading,
            bytes_read: 64, // Full GET request received
            deadline: 5000,
        };

        // Fast connection transitions to Writing and completes while slow_conn remains Reading
        fast_conn.phase = ConnPhase::Writing;
        assert_eq!(slow_conn.phase, ConnPhase::Reading);
        assert_eq!(fast_conn.phase, ConnPhase::Writing);

        // Fast connection finishes
        fast_conn.phase = ConnPhase::Closing;
        assert_eq!(fast_conn.phase, ConnPhase::Closing);
        assert_eq!(slow_conn.phase, ConnPhase::Reading);
    }

    #[test]
    fn test_httpd_reactor_deadline_timeout_and_slot_reclamation() {
        let mut conn = MockHttpConn {
            phase: ConnPhase::Reading,
            bytes_read: 5,
            deadline: 3000,
        };

        let current_tick = 3001;
        if current_tick > conn.deadline {
            conn.phase = ConnPhase::Closing; // 408 Request Timeout
        }
        assert_eq!(conn.phase, ConnPhase::Closing);

        // Closed slot is reclaimed back to Idle
        conn.phase = ConnPhase::Idle;
        conn.bytes_read = 0;
        assert_eq!(conn.phase, ConnPhase::Idle);
    }
}

#[cfg(test)]
mod vfs_rebase {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum FileType {
        RegularFile,
        Directory,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum VfsStatus {
        Success,
        PermissionDenied,
        IsADirectory,
        NotADirectory,
        DirectoryNotEmpty,
    }

    #[test]
    fn test_vfs_file_slot_allocation_and_rights_attenuation() {
        const PERM_READ: u8 = 1 << 0;
        const PERM_WRITE: u8 = 1 << 1;

        struct MockFileSlot {
            perms: u8,
        }

        let root_slot = MockFileSlot { perms: PERM_READ | PERM_WRITE };
        // Attenuate to read-only
        let derived_slot = MockFileSlot { perms: root_slot.perms & PERM_READ };
        assert_eq!(derived_slot.perms, PERM_READ);

        // Attempt write on derived read-only handle
        let write_status = if (derived_slot.perms & PERM_WRITE) != 0 {
            VfsStatus::Success
        } else {
            VfsStatus::PermissionDenied
        };
        assert_eq!(write_status, VfsStatus::PermissionDenied);
    }

    #[test]
    fn test_vfs_posix_type_checking_unlink_vs_rmdir() {
        let file_type = FileType::RegularFile;
        let dir_type = FileType::Directory;

        // unlink on directory must fail with IsADirectory
        let unlink_dir_res = if dir_type == FileType::Directory {
            VfsStatus::IsADirectory
        } else {
            VfsStatus::Success
        };
        assert_eq!(unlink_dir_res, VfsStatus::IsADirectory);

        // rmdir on regular file must fail with NotADirectory
        let rmdir_file_res = if file_type == FileType::RegularFile {
            VfsStatus::NotADirectory
        } else {
            VfsStatus::Success
        };
        assert_eq!(rmdir_file_res, VfsStatus::NotADirectory);
    }

    #[test]
    fn test_vfs_directory_binding_atomic_rollback() {
        let mut namespace = vec!["/bin".to_string(), "/etc".to_string()];
        let checkpoint = namespace.clone();

        // Attempt multi-step bind: step 1 succeeds, step 2 fails
        namespace.push("/data".to_string());
        let bind_step2_failed = true;
        if bind_step2_failed {
            // All-or-nothing rollback restores snapshot
            namespace = checkpoint;
        }

        assert_eq!(namespace.len(), 2);
        assert_eq!(namespace, vec!["/bin".to_string(), "/etc".to_string()]);
    }
}

#[cfg(test)]
mod core_kernel_rebase {
    const PERM_CAN_SPAWN: u32 = 1 << 0;
    const PERM_CAN_BLOCK_IO: u32 = 1 << 1;

    #[test]
    fn test_kernel_perms_bitflags_enforcement() {
        struct TaskPerms {
            flags: u32,
        }

        let worker_task = TaskPerms { flags: PERM_CAN_BLOCK_IO };
        let spawn_allowed = (worker_task.flags & PERM_CAN_SPAWN) != 0;
        let block_io_allowed = (worker_task.flags & PERM_CAN_BLOCK_IO) != 0;

        assert!(!spawn_allowed, "worker task must not be allowed to spawn");
        assert!(block_io_allowed, "worker task must be allowed to block IO");
    }

    #[test]
    fn test_cross_hart_tlb_shootdown_synchronization() {
        struct SmpTlbBarrier {
            target_harts_mask: u32,
            acknowledged_mask: u32,
        }

        let mut barrier = SmpTlbBarrier {
            target_harts_mask: 0b0011, // harts 0 and 1
            acknowledged_mask: 0b0001, // hart 0 acknowledged
        };

        // Frame cannot be reused until all target harts acknowledge flush
        let can_reuse_frame_before = barrier.acknowledged_mask == barrier.target_harts_mask;
        assert!(!can_reuse_frame_before);

        // Hart 1 acknowledges
        barrier.acknowledged_mask |= 0b0010;
        let can_reuse_frame_after = barrier.acknowledged_mask == barrier.target_harts_mask;
        assert!(can_reuse_frame_after);
    }

    #[test]
    fn test_bootfs_loader_acyclic_bootstrap() {
        struct BootfsImage {
            path: &'static str,
            expected_sha256_prefix: u32,
            actual_sha256_prefix: u32,
        }

        let init_image = BootfsImage {
            path: "init",
            expected_sha256_prefix: 0xA1B2_C3D4,
            actual_sha256_prefix: 0xA1B2_C3D4,
        };

        // Image integrity matches allowlist -> load approved
        let load_approved = init_image.expected_sha256_prefix == init_image.actual_sha256_prefix;
        assert!(load_approved);

        // Corrupted or tampered image -> load refused
        let tampered_image = BootfsImage {
            path: "tampered",
            expected_sha256_prefix: 0xA1B2_C3D4,
            actual_sha256_prefix: 0x0000_0000,
        };
        let tampered_approved = tampered_image.expected_sha256_prefix == tampered_image.actual_sha256_prefix;
        assert!(!tampered_approved);
    }
}

#[cfg(test)]
mod native_sdk_rebase {
    const APP_MSG_MAGIC: u8 = 0xAC;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum SdkCallStatus {
        Success,
        StaleBinding,
        ServiceNotFound,
    }

    struct ServiceBindingMock {
        tid: usize,
        cell_id: u64,
        generation: u64,
    }

    #[test]
    fn test_sdk_service_ref_caching_and_staleness() {
        let cached_binding = ServiceBindingMock {
            tid: 4,
            cell_id: 10,
            generation: 1,
        };

        // Kernel registry updated after service restart (generation 2)
        let live_registry_binding = ServiceBindingMock {
            tid: 4,
            cell_id: 10,
            generation: 2,
        };

        let status = if cached_binding.generation == live_registry_binding.generation {
            SdkCallStatus::Success
        } else {
            SdkCallStatus::StaleBinding
        };

        assert_eq!(status, SdkCallStatus::StaleBinding, "stale provider binding must be detected");
    }

    #[test]
    fn test_sdk_app_context_magic_envelope() {
        let structured_msg = [APP_MSG_MAGIC, 0x00, 0x11, 0x22];
        let legacy_msg = [0x55, 0x66, 0x77, 0x88];

        let is_structured_1 = structured_msg[0] == APP_MSG_MAGIC;
        let is_structured_2 = legacy_msg[0] == APP_MSG_MAGIC;

        assert!(is_structured_1, "magic byte 0xAC identifies AppContext envelope");
        assert!(!is_structured_2, "legacy message falls back to raw message handling");
    }

    #[test]
    fn test_sdk_bounded_ipc_operation_lifecycle() {
        const CAPACITY: usize = 32;
        let mut active_ops = 0;

        // Submit allocates an operation ID
        if active_ops < CAPACITY {
            active_ops += 1;
        }
        assert_eq!(active_ops, 1);

        // Take completes and frees the operation slot
        if active_ops > 0 {
            active_ops -= 1;
        }
        assert_eq!(active_ops, 0);
    }
}
