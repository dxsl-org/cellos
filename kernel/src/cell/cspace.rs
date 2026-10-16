//! Shared Cell CSpace, Principal Discrimination, and Local Generational Handles.
//!
//! Implements candidate specifications D01 r2, D02 r1, and D03 r2 under Phase 02.
//!
//! - **D01**: Opaque 64-bit tickets indexing preallocated slot arenas; 48-byte handle
//!   requests; 32-byte status headers; 48-byte BootFS file requests.
//! - **D02**: Strict `Principal` discrimination (`Kernel` singleton vs `Cell { id, generation }`);
//!   40-byte principal record.
//! - **D03**: Single shared `CellAuthority` per `(CellId, generation)`; 64-byte CSpace request;
//!   single-parent revocation hierarchy with `AdmissionCut` and quiescence tracking.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::task::cap::CapSet;
use types::CellId;

/// Maximum number of active capability slots per Cell incarnation.
pub const MAX_SLOTS_PER_CELL: usize = 256;
/// Maximum depth of ancestry derivation.
pub const MAX_LINEAGE_DEPTH: usize = 8;

/// Status codes for control results per D01 r2.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlStatus {
    Success = 0,
    Denied = 1,
    Stale = 2,
    WrongType = 3,
    Capacity = 4,
    Exhausted = 5,
    Busy = 6,
    Small = 7,
    Invalid = 8,
    Indeterminate = 9,
    Unsupported = 10,
    AdmissionCut = 11,
    Quarantined = 12,
    Quiesced = 13,
    Pending = 14,
}

impl ControlStatus {
    pub const fn from_u32(val: u32) -> Option<Self> {
        match val {
            0 => Some(Self::Success),
            1 => Some(Self::Denied),
            2 => Some(Self::Stale),
            3 => Some(Self::WrongType),
            4 => Some(Self::Capacity),
            5 => Some(Self::Exhausted),
            6 => Some(Self::Busy),
            7 => Some(Self::Small),
            8 => Some(Self::Invalid),
            9 => Some(Self::Indeterminate),
            10 => Some(Self::Unsupported),
            11 => Some(Self::AdmissionCut),
            12 => Some(Self::Quarantined),
            13 => Some(Self::Quiesced),
            14 => Some(Self::Pending),
            _ => None,
        }
    }
}

/// Principal discrimination per D02 r1.
///
/// An exceptional `Kernel` singleton is minted exclusively by kernel bootstrap context.
/// Ordinary cells are identified by their `CellId` and unique monotonic `generation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Principal {
    Kernel,
    Cell { id: CellId, generation: u64 },
}

impl Principal {
    /// Encode a 40-byte LE principal record per D02 r1.
    pub fn encode_record(&self, flags: u64, sender_tid: u64, out: &mut [u8; 40]) {
        out.fill(0);
        out[0..4].copy_from_slice(&1u32.to_le_bytes()); // version: 1
        match self {
            Principal::Kernel => {
                out[4..8].copy_from_slice(&1u32.to_le_bytes()); // kind: 1 = Kernel
                // All other identity words and flags remain 0 for Kernel
            }
            Principal::Cell { id, generation } => {
                out[4..8].copy_from_slice(&2u32.to_le_bytes()); // kind: 2 = Cell
                out[8..16].copy_from_slice(&flags.to_le_bytes());
                out[16..24].copy_from_slice(&id.0.to_le_bytes());
                out[24..32].copy_from_slice(&generation.to_le_bytes());
                out[32..40].copy_from_slice(&sender_tid.to_le_bytes());
            }
        }
    }

    /// Decode a 40-byte LE principal record per D02 r1.
    pub fn decode_record(buf: &[u8]) -> Result<(Self, u64, u64), ControlStatus> {
        if buf.len() < 40 {
            return Err(ControlStatus::Small);
        }
        let version = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if version != 1 {
            return Err(ControlStatus::Invalid);
        }
        let kind = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let flags = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let cell_id = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let generation = u64::from_le_bytes(buf[24..32].try_into().unwrap());
        let sender_tid = u64::from_le_bytes(buf[32..40].try_into().unwrap());

        match kind {
            1 => {
                // Kernel singleton: all other words must be 0
                if flags != 0 || cell_id != 0 || generation != 0 || sender_tid != 0 {
                    return Err(ControlStatus::Invalid);
                }
                Ok((Principal::Kernel, 0, 0))
            }
            2 => {
                // Cell: all three identity words must be nonzero
                if cell_id == 0 || generation == 0 || sender_tid == 0 {
                    return Err(ControlStatus::Invalid);
                }
                // Flags accept only bit 0 (VFS mutate)
                if (flags & !1) != 0 {
                    return Err(ControlStatus::Invalid);
                }
                Ok((
                    Principal::Cell {
                        id: CellId(cell_id),
                        generation,
                    },
                    flags,
                    sender_tid,
                ))
            }
            _ => Err(ControlStatus::Invalid),
        }
    }
}

/// Resource kind for typed slots per D01 r2.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    None = 0,
    BootFSFile = 1,
    CSpaceAuthority = 2,
    MemoryGrant = 3,
    RingBuffer = 4,
    NativeStream = 5,
}

/// 32-byte Control Result Header per D01 r2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlResultHeader {
    pub version: u32,
    pub status: ControlStatus,
    pub ticket: u64,
    pub required_bytes: u64,
    pub progress: u64,
}

impl ControlResultHeader {
    pub const fn new(status: ControlStatus, ticket: u64, required_bytes: u64, progress: u64) -> Self {
        Self {
            version: 1,
            status,
            ticket,
            required_bytes,
            progress,
        }
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<(), ControlStatus> {
        if out.len() < 32 {
            return Err(ControlStatus::Small);
        }
        out[0..4].copy_from_slice(&self.version.to_le_bytes());
        out[4..8].copy_from_slice(&(self.status as u32).to_le_bytes());
        out[8..16].copy_from_slice(&self.ticket.to_le_bytes());
        out[16..24].copy_from_slice(&self.required_bytes.to_le_bytes());
        out[24..32].copy_from_slice(&self.progress.to_le_bytes());
        Ok(())
    }

    pub fn decode(buf: &[u8]) -> Result<Self, ControlStatus> {
        if buf.len() < 32 {
            return Err(ControlStatus::Small);
        }
        let version = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if version != 1 {
            return Err(ControlStatus::Invalid);
        }
        let status_raw = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let status = ControlStatus::from_u32(status_raw).ok_or(ControlStatus::Invalid)?;
        let ticket = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let required_bytes = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let progress = u64::from_le_bytes(buf[24..32].try_into().unwrap());

        Ok(Self {
            version,
            status,
            ticket,
            required_bytes,
            progress,
        })
    }
}

/// 48-byte Handle Control Request per D01 r2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandleControlRequest {
    pub version: u32,
    pub action: u32, // 0=Close, 1=Derive, 2=Revoke, 3=QueryRevoke
    pub ticket: u64,
    pub destination_cell: u64,
    pub destination_generation: u64,
    pub rights: u32,
    pub expiry_present: u32,
    pub expiry_tick: u64,
}

impl HandleControlRequest {
    pub fn decode(buf: &[u8]) -> Result<Self, ControlStatus> {
        if buf.len() < 48 {
            return Err(ControlStatus::Small);
        }
        let version = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if version != 1 {
            return Err(ControlStatus::Invalid);
        }
        let action = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        if action > 3 {
            return Err(ControlStatus::Invalid);
        }
        let ticket = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let destination_cell = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let destination_generation = u64::from_le_bytes(buf[24..32].try_into().unwrap());
        let rights = u32::from_le_bytes(buf[32..36].try_into().unwrap());
        let expiry_present = u32::from_le_bytes(buf[36..40].try_into().unwrap());
        if expiry_present > 1 {
            return Err(ControlStatus::Invalid);
        }
        let expiry_tick = u64::from_le_bytes(buf[40..48].try_into().unwrap());
        if expiry_present == 0 && expiry_tick != 0 {
            return Err(ControlStatus::Invalid);
        }

        // Close/Revoke/Query require destination/rights/expiry fields to be 0
        if action != 1 {
            if destination_cell != 0 || destination_generation != 0 || rights != 0 || expiry_present != 0 || expiry_tick != 0 {
                return Err(ControlStatus::Invalid);
            }
        }

        Ok(Self {
            version,
            action,
            ticket,
            destination_cell,
            destination_generation,
            rights,
            expiry_present,
            expiry_tick,
        })
    }
}

/// 64-byte CSpace Request per D03 r2.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CSpaceRequest {
    pub version: u32,
    pub action: u32, // 0=Close, 1=Derive, 2=Revoke, 3=QueryRevoke
    pub parent_ticket: u64,
    pub target_cell: u64,
    pub target_generation: u64,
    pub class_bits: u64,
    pub mmio_mask: u64,
    pub block_region_mask: u64,
    pub absolute_expiry_tick: u64,
}

impl CSpaceRequest {
    pub fn decode(buf: &[u8]) -> Result<Self, ControlStatus> {
        if buf.len() < 64 {
            return Err(ControlStatus::Small);
        }
        let version = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if version != 1 {
            return Err(ControlStatus::Invalid);
        }
        let action = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        if action > 3 {
            return Err(ControlStatus::Invalid);
        }
        let parent_ticket = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let target_cell = u64::from_le_bytes(buf[16..24].try_into().unwrap());
        let target_generation = u64::from_le_bytes(buf[24..32].try_into().unwrap());
        let class_bits = u64::from_le_bytes(buf[32..40].try_into().unwrap());
        // Only bits 0..8 are valid class bits
        if (class_bits & !0x1FF) != 0 {
            return Err(ControlStatus::Invalid);
        }
        let mmio_mask = u64::from_le_bytes(buf[40..48].try_into().unwrap());
        let block_region_mask = u64::from_le_bytes(buf[48..56].try_into().unwrap());
        let absolute_expiry_tick = u64::from_le_bytes(buf[56..64].try_into().unwrap());

        if action != 1 {
            if target_cell != 0 || target_generation != 0 || class_bits != 0 || mmio_mask != 0 || block_region_mask != 0 || absolute_expiry_tick != 0 {
                return Err(ControlStatus::Invalid);
            }
        }

        Ok(Self {
            version,
            action,
            parent_ticket,
            target_cell,
            target_generation,
            class_bits,
            mmio_mask,
            block_region_mask,
            absolute_expiry_tick,
        })
    }
}

/// One entry in the preallocated Slot Arena.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotEntry {
    pub in_use: bool,
    pub holder_open: bool,
    pub child_count: usize,
    pub ticket: u64,
    pub holder: Principal,
    pub issuer: Principal,
    pub kind: ResourceKind,
    pub rights: u32,
    pub expiry: Option<u64>,
    pub lineage_parent: Option<u32>,
    pub depth: usize,
    pub admission_cut: bool,
    pub quiesced: bool,
}

impl Default for SlotEntry {
    fn default() -> Self {
        Self {
            in_use: false,
            holder_open: false,
            child_count: 0,
            ticket: 0,
            holder: Principal::Kernel,
            issuer: Principal::Kernel,
            kind: ResourceKind::None,
            rights: 0,
            expiry: None,
            lineage_parent: None,
            depth: 0,
            admission_cut: false,
            quiesced: true,
        }
    }
}

/// Shared Cell Authority per (CellId, generation).
pub struct CellAuthority {
    pub cell_id: CellId,
    pub generation: u64,
    pub ceiling: CapSet,
    pub current_rights: CapSet,
    pub slots: Vec<SlotEntry>,
    pub next_ticket_counter: u64,
    pub revoking: bool,
    pub quiesced: bool,
}

impl CellAuthority {
    pub fn new(cell_id: CellId, generation: u64, ceiling: CapSet) -> Self {
        let mut slots = Vec::with_capacity(MAX_SLOTS_PER_CELL);
        for _ in 0..MAX_SLOTS_PER_CELL {
            slots.push(SlotEntry::default());
        }
        Self {
            cell_id,
            generation,
            ceiling,
            current_rights: ceiling,
            slots,
            next_ticket_counter: 1,
            revoking: false,
            quiesced: false,
        }
    }

    /// Resolve an opaque recipient-local ticket; no ticket bits encode an index.
    fn live_slot_index(&self, ticket: u64) -> Result<usize, ControlStatus> {
        if ticket == 0 {
            return Err(ControlStatus::Invalid);
        }
        self.slots.iter().position(|slot|
            slot.in_use && slot.holder_open && slot.ticket == ticket
        ).ok_or(ControlStatus::Stale)
    }

    /// Allocate a new slot in this Cell's arena.
    pub fn allocate_slot(
        &mut self,
        holder: Principal,
        issuer: Principal,
        kind: ResourceKind,
        rights: u32,
        expiry: Option<u64>,
        parent_ticket: Option<u64>,
    ) -> Result<u64, ControlStatus> {
        if self.revoking {
            return Err(ControlStatus::AdmissionCut);
        }
        if kind == ResourceKind::CSpaceAuthority && rights & !0x1FF != 0 {
            return Err(ControlStatus::Invalid);
        }
        let lineage_parent = match parent_ticket {
            Some(ticket) => Some(self.live_slot_index(ticket)? as u32),
            None => None,
        };
        let depth = if let Some(parent_idx) = lineage_parent {
            let p = &self.slots[parent_idx as usize];
            if p.admission_cut {
                return Err(ControlStatus::AdmissionCut);
            }
            if rights & !p.rights != 0
                || p.expiry.is_some_and(|limit| expiry.is_none_or(|value| value > limit))
            {
                return Err(ControlStatus::Denied);
            }
            let next_depth = p.depth + 1;
            if next_depth > MAX_LINEAGE_DEPTH {
                return Err(ControlStatus::Capacity);
            }
            next_depth
        } else {
            0
        };

        let idx = self.slots.iter().position(|slot| !slot.in_use)
            .ok_or(ControlStatus::Capacity)?;
        let next_ticket = self.next_ticket_counter.checked_add(1)
            .ok_or(ControlStatus::Exhausted)?;
        let ticket = self.next_ticket_counter;
        if ticket == 0 {
            return Err(ControlStatus::Exhausted);
        }
        // All fallible checks precede publication or parent-reference mutation.
        self.slots[idx] = SlotEntry {
            in_use: true,
            holder_open: true,
            child_count: 0,
            ticket,
            holder,
            issuer,
            kind,
            rights,
            expiry,
            lineage_parent,
            depth,
            admission_cut: false,
            quiesced: false,
        };
        if let Some(parent_idx) = lineage_parent {
            self.slots[parent_idx as usize].child_count += 1;
        }
        self.next_ticket_counter = next_ticket;
        Ok(ticket)
    }

    /// Verify a ticket and return reference to the slot if valid.
    pub fn verify_ticket(&self, ticket: u64) -> Result<&SlotEntry, ControlStatus> {
        let slot = &self.slots[self.live_slot_index(ticket)?];
        if slot.admission_cut {
            return Err(ControlStatus::AdmissionCut);
        }
        Ok(slot)
    }

    /// Close a ticket (holder close releases reference, does not revoke descendants).
    pub fn close_ticket(&mut self, ticket: u64) -> Result<(), ControlStatus> {
        let idx = self.live_slot_index(ticket)?;
        let slot = &mut self.slots[idx];
        slot.holder_open = false;
        if !self.revoking {
            self.reclaim_closed_lineage(idx);
        }
        crate::cell::cap_registry::remove_ticket_file((self.cell_id.0, self.generation, ticket));
        Ok(())
    }

    /// Release metadata only after its holder and every child reference are gone.
    /// This is not an acknowledgement of resource/device quiescence.
    fn reclaim_closed_lineage(&mut self, mut idx: usize) {
        loop {
            let slot = &mut self.slots[idx];
            if slot.holder_open || slot.child_count != 0 {
                return;
            }
            let parent = slot.lineage_parent.take();
            slot.in_use = false;
            match parent {
                Some(parent_idx) => {
                    idx = parent_idx as usize;
                    self.slots[idx].child_count -= 1;
                }
                None => return,
            }
        }
    }

    /// Revoke a ticket: marks the slot and all its descendants `AdmissionCut`.
    pub fn revoke_ticket(&mut self, ticket: u64) -> Result<u64, ControlStatus> {
        let target_idx = self.live_slot_index(ticket)?;

        // Mark target admission cut
        self.slots[target_idx].admission_cut = true;

        // Cut all descendants in the arena (single-parent tree traversal)
        for i in 0..self.slots.len() {
            if self.slots[i].in_use && !self.slots[i].admission_cut {
                let mut cur = self.slots[i].lineage_parent;
                while let Some(parent_idx) = cur {
                    if parent_idx as usize == target_idx {
                        self.slots[i].admission_cut = true;
                        break;
                    }
                    if parent_idx as usize >= self.slots.len() {
                        break;
                    }
                    cur = self.slots[parent_idx as usize].lineage_parent;
                }
            }
        }

        // Return ticket as stable query ticket
        Ok(ticket)
    }

    /// Query the revocation status of a ticket.
    pub fn query_revoke(&self, ticket: u64) -> Result<ControlStatus, ControlStatus> {
        if self.revoking {
            if ticket == 0 {
                return Err(ControlStatus::Invalid);
            }
            return if self.slots.iter().any(|slot| slot.in_use && slot.ticket == ticket) {
                Ok(ControlStatus::AdmissionCut)
            } else {
                Err(ControlStatus::Stale)
            };
        }
        let slot = &self.slots[self.live_slot_index(ticket)?];
        if slot.quiesced {
            Ok(ControlStatus::Quiesced)
        } else if slot.admission_cut {
            Ok(ControlStatus::AdmissionCut)
        } else {
            Ok(ControlStatus::Pending)
        }
    }
    /// Allocate a CSpace slot for an open BootFS file with given rights.
    pub fn allocate_file_slot(
        &mut self,
        holder: Principal,
        file_box: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
        rights: u32,
    ) -> Result<u64, ControlStatus> {
        let ticket = self.allocate_slot(holder, Principal::Kernel, ResourceKind::BootFSFile, rights, None, None)?;
        let _ = self.live_slot_index(ticket)?;
        crate::cell::cap_registry::attach_file_to_ticket((self.cell_id.0, self.generation, ticket), file_box);
        Ok(ticket)
    }

    /// Take parked Box<dyn ViFile> for a ticket while verifying rights and non-cut admission.
    pub fn park_file(
        &mut self,
        ticket: u64,
        required_rights: u32,
    ) -> Result<alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>, ControlStatus> {
        let slot_idx = self.live_slot_index(ticket)?;
        let slot = &self.slots[slot_idx];
        if slot.kind != ResourceKind::BootFSFile {
            return Err(ControlStatus::WrongType);
        }
        if slot.admission_cut {
            return Err(ControlStatus::AdmissionCut);
        }
        if (slot.rights & required_rights) != required_rights {
            return Err(ControlStatus::Denied);
        }
        crate::cell::cap_registry::park_ticket_file((self.cell_id.0, self.generation, ticket))
            .ok_or(ControlStatus::Busy)
    }

    /// Return previously parked Box<dyn ViFile> back to the ticket's storage.
    pub fn unpark_file(
        &mut self,
        ticket: u64,
        file_box: alloc::boxed::Box<dyn api::fs::ViFile + Send + Sync>,
    ) {
        crate::cell::cap_registry::unpark_ticket_file((self.cell_id.0, self.generation, ticket), file_box);
    }
}
/// Global registry of shared Cell authorities indexed by (CellId, generation).
pub static CELL_AUTHORITIES: crate::sync::Spinlock<BTreeMap<(u64, u64), CellAuthority>> =
    crate::sync::Spinlock::new(BTreeMap::new());

/// Register a new CellAuthority. Returns true if registered or already present;
/// returns false if cell_id is invalid (0 or >= MAX_CELLS).
pub fn register_cell_authority(cell_id: CellId, generation: u64, ceiling: CapSet) -> bool {
    if cell_id.0 == 0 || cell_id.0 as usize >= crate::memory::cell_quota::MAX_CELLS {
        return false;
    }
    let mut auths = CELL_AUTHORITIES.lock();
    auths.entry((cell_id.0, generation))
        .or_insert_with(|| CellAuthority::new(cell_id, generation, ceiling));
    true
}

/// Retire a CellAuthority upon cell termination.
pub fn retire_cell_authority(cell_id: CellId, generation: u64) {
    let mut auths = CELL_AUTHORITIES.lock();
    if let Some(auth) = auths.get_mut(&(cell_id.0, generation)) {
        auth.revoking = true;
        for slot in auth.slots.iter_mut() {
            if slot.in_use {
                slot.admission_cut = true;
            }
        }
    }
}

/// Dispatch a 48-byte Handle Control Request.
pub fn dispatch_handle_control(
    cell_id: CellId,
    generation: u64,
    req_buf: &[u8],
) -> ControlResultHeader {
    let req = match HandleControlRequest::decode(req_buf) {
        Ok(r) => r,
        Err(status) => return ControlResultHeader::new(status, 0, 32, 0),
    };

    let mut auths = CELL_AUTHORITIES.lock();
    let Some(auth) = auths.get_mut(&(cell_id.0, generation)) else {
        return ControlResultHeader::new(ControlStatus::Stale, 0, 32, 0);
    };

    if auth.revoking && req.action != 0 && req.action != 3 {
        return ControlResultHeader::new(ControlStatus::AdmissionCut, 0, 32, 0);
    }

    match req.action {
        0 => {
            // Close
            match auth.close_ticket(req.ticket) {
                Ok(()) => ControlResultHeader::new(ControlStatus::Success, req.ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.ticket, 32, 0),
            }
        }
        1 => {
            // Derive
            let (parent_rights, parent_expiry, kind) = match auth.verify_ticket(req.ticket) {
                Ok(slot) => (slot.rights, slot.expiry, slot.kind),
                Err(status) => return ControlResultHeader::new(status, req.ticket, 32, 0),
            };
            if kind == ResourceKind::CSpaceAuthority && req.rights & !0x1FF != 0 {
                return ControlResultHeader::new(ControlStatus::Invalid, req.ticket, 32, 0);
            }

            // Rights attenuation: requested rights must be a subset of parent rights
            if (req.rights & !parent_rights) != 0 {
                return ControlResultHeader::new(ControlStatus::Denied, req.ticket, 32, 0);
            }

            // Expiry attenuation
            let child_expiry = if req.expiry_present == 1 {
                if let Some(pe) = parent_expiry {
                    if req.expiry_tick > pe {
                        return ControlResultHeader::new(ControlStatus::Denied, req.ticket, 32, 0);
                    }
                }
                Some(req.expiry_tick)
            } else {
                if parent_expiry.is_some() {
                    return ControlResultHeader::new(ControlStatus::Denied, req.ticket, 32, 0);
                }
                None
            };

            let holder = Principal::Cell {
                id: CellId(req.destination_cell),
                generation: req.destination_generation,
            };
            let issuer = Principal::Cell { id: cell_id, generation };

            match auth.allocate_slot(holder, issuer, kind, req.rights, child_expiry, Some(req.ticket)) {
                Ok(new_ticket) => ControlResultHeader::new(ControlStatus::Success, new_ticket, 32, 1),
                Err(status) => ControlResultHeader::new(status, req.ticket, 32, 0),
            }
        }
        2 => {
            // Revoke
            match auth.revoke_ticket(req.ticket) {
                Ok(query_ticket) => ControlResultHeader::new(ControlStatus::AdmissionCut, query_ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.ticket, 32, 0),
            }
        }
        3 => {
            // QueryRevoke
            match auth.query_revoke(req.ticket) {
                Ok(status) => ControlResultHeader::new(status, req.ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.ticket, 32, 0),
            }
        }
        _ => ControlResultHeader::new(ControlStatus::Invalid, req.ticket, 32, 0),
    }
}

/// Dispatch a 64-byte CSpace Request.
pub fn dispatch_cspace_control(
    cell_id: CellId,
    generation: u64,
    req_buf: &[u8],
) -> ControlResultHeader {
    let req = match CSpaceRequest::decode(req_buf) {
        Ok(r) => r,
        Err(status) => return ControlResultHeader::new(status, 0, 32, 0),
    };

    let mut auths = CELL_AUTHORITIES.lock();
    let Some(auth) = auths.get_mut(&(cell_id.0, generation)) else {
        return ControlResultHeader::new(ControlStatus::Stale, 0, 32, 0);
    };

    if auth.revoking && req.action != 0 && req.action != 3 {
        return ControlResultHeader::new(ControlStatus::AdmissionCut, 0, 32, 0);
    }

    match req.action {
        0 => {
            // Close
            match auth.close_ticket(req.parent_ticket) {
                Ok(()) => ControlResultHeader::new(ControlStatus::Success, req.parent_ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.parent_ticket, 32, 0),
            }
        }
        1 => {
            // Derive
            // This internal tranche has no mask-bearing subgrant metadata.
            // Reject unsupported authority rather than silently discard it.
            if req.mmio_mask != 0 || req.block_region_mask != 0 {
                return ControlResultHeader::new(ControlStatus::Invalid, req.parent_ticket, 32, 0);
            }
            let (parent_rights, parent_expiry) = match auth.verify_ticket(req.parent_ticket) {
                Ok(slot) => {
                    if slot.kind != ResourceKind::CSpaceAuthority {
                        return ControlResultHeader::new(ControlStatus::WrongType, req.parent_ticket, 32, 0);
                    }
                    (slot.rights, slot.expiry)
                }
                Err(status) => return ControlResultHeader::new(status, req.parent_ticket, 32, 0),
            };

            let requested_rights = req.class_bits as u32;
            if (requested_rights & !parent_rights) != 0 {
                return ControlResultHeader::new(ControlStatus::Denied, req.parent_ticket, 32, 0);
            }

            let child_expiry = if req.absolute_expiry_tick > 0 {
                if let Some(pe) = parent_expiry {
                    if req.absolute_expiry_tick > pe {
                        return ControlResultHeader::new(ControlStatus::Denied, req.parent_ticket, 32, 0);
                    }
                }
                Some(req.absolute_expiry_tick)
            } else {
                if parent_expiry.is_some() {
                    return ControlResultHeader::new(ControlStatus::Denied, req.parent_ticket, 32, 0);
                }
                None
            };

            let holder = Principal::Cell {
                id: CellId(req.target_cell),
                generation: req.target_generation,
            };
            let issuer = Principal::Cell { id: cell_id, generation };

            match auth.allocate_slot(
                holder,
                issuer,
                ResourceKind::CSpaceAuthority,
                requested_rights,
                child_expiry,
                Some(req.parent_ticket),
            ) {
                Ok(new_ticket) => ControlResultHeader::new(ControlStatus::Success, new_ticket, 32, 1),
                Err(status) => ControlResultHeader::new(status, req.parent_ticket, 32, 0),
            }
        }
        2 => {
            // Revoke
            match auth.revoke_ticket(req.parent_ticket) {
                Ok(query_ticket) => ControlResultHeader::new(ControlStatus::AdmissionCut, query_ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.parent_ticket, 32, 0),
            }
        }
        3 => {
            // QueryRevoke
            match auth.query_revoke(req.parent_ticket) {
                Ok(status) => ControlResultHeader::new(status, req.parent_ticket, 32, 0),
                Err(status) => ControlResultHeader::new(status, req.parent_ticket, 32, 0),
            }
        }
        _ => ControlResultHeader::new(ControlStatus::Invalid, req.parent_ticket, 32, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_principal_encoding_kernel() {
        let mut buf = [0u8; 40];
        Principal::Kernel.encode_record(0, 0, &mut buf);
        let (decoded, flags, tid) = Principal::decode_record(&buf).expect("decode kernel");
        assert_eq!(decoded, Principal::Kernel);
        assert_eq!(flags, 0);
        assert_eq!(tid, 0);

        // Corrupt flag -> reject
        buf[8] = 1;
        assert_eq!(Principal::decode_record(&buf), Err(ControlStatus::Invalid));
    }

    #[test]
    fn test_principal_encoding_cell() {
        let mut buf = [0u8; 40];
        let p = Principal::Cell { id: CellId(42), generation: 7 };
        p.encode_record(1, 100, &mut buf);
        let (decoded, flags, tid) = Principal::decode_record(&buf).expect("decode cell");
        assert_eq!(decoded, p);
        assert_eq!(flags, 1);
        assert_eq!(tid, 100);

        // Cell with zero ID -> reject
        buf[16..24].fill(0);
        assert_eq!(Principal::decode_record(&buf), Err(ControlStatus::Invalid));
    }


    #[test]
    fn test_slot_allocation_and_verify() {
        let mut auth = CellAuthority::new(CellId(10), 1, CapSet::EMPTY);
        let ticket = auth
            .allocate_slot(
                Principal::Cell { id: CellId(10), generation: 1 },
                Principal::Kernel,
                ResourceKind::BootFSFile,
                0x7,
                Some(1000),
                None,
            )
            .expect("allocate slot");

        let slot = auth.verify_ticket(ticket).expect("verify ticket");
        assert_eq!(slot.rights, 0x7);
        assert_eq!(slot.depth, 0);
        assert_eq!(slot.kind, ResourceKind::BootFSFile);
    }

    #[test]
    fn test_lineage_depth_bound() {
        let mut auth = CellAuthority::new(CellId(10), 1, CapSet::EMPTY);
        let mut cur_ticket = auth
            .allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0xFF, None, None)
            .unwrap();

        for _ in 1..=MAX_LINEAGE_DEPTH {
            cur_ticket = auth
                .allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0xFF, None, Some(cur_ticket))
                .unwrap();
        }

        // Next derivation must fail due to depth limit
        let err = auth.allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0xFF, None, Some(cur_ticket));
        assert_eq!(err, Err(ControlStatus::Capacity));
    }

    #[test]
    fn test_close_vs_revoke_semantics() {
        let mut auth = CellAuthority::new(CellId(20), 1, CapSet::EMPTY);
        // P (parent)
        let p_ticket = auth
            .allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0xFF, None, None)
            .unwrap();

        // C (child of P)
        let c_ticket = auth
            .allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0x0F, None, Some(p_ticket))
            .unwrap();

        // S (sibling of P, derived from root)
        let s_ticket = auth
            .allocate_slot(Principal::Kernel, Principal::Kernel, ResourceKind::CSpaceAuthority, 0xFF, None, None)
            .unwrap();

        // Revoking P cuts P and C, but NOT S!
        auth.revoke_ticket(p_ticket).unwrap();
        assert_eq!(auth.verify_ticket(p_ticket), Err(ControlStatus::AdmissionCut));
        assert_eq!(auth.verify_ticket(c_ticket), Err(ControlStatus::AdmissionCut));
        assert!(auth.verify_ticket(s_ticket).is_ok());
    }


    #[test]
    fn closed_parent_cannot_be_reused_to_revoke_surviving_child() {
        let mut auth = CellAuthority::new(CellId(21), 1, CapSet::EMPTY);
        let parent = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 7, None, None).unwrap();
        let parent_idx = auth.live_slot_index(parent).unwrap();
        let child = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 3, None, Some(parent)).unwrap();
        auth.close_ticket(parent).unwrap();
        assert_eq!(auth.verify_ticket(parent), Err(ControlStatus::Stale));
        let unrelated = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 7, None, None).unwrap();
        auth.revoke_ticket(unrelated).unwrap();
        assert_eq!(auth.verify_ticket(child).unwrap().rights, 3);
        auth.close_ticket(child).unwrap();
        assert!(!auth.slots[parent_idx as usize].in_use);
    }

    #[test]
    fn revocation_traverses_closed_ancestors_and_last_child_reclaims_chain() {
        let mut auth = CellAuthority::new(CellId(22), 1, CapSet::EMPTY);
        let root = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 7, None, None).unwrap();
        let parent = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 3, None, Some(root)).unwrap();
        let child = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 1, None, Some(parent)).unwrap();
        auth.close_ticket(parent).unwrap();
        auth.revoke_ticket(root).unwrap();
        assert_eq!(auth.verify_ticket(child), Err(ControlStatus::AdmissionCut));
        auth.close_ticket(root).unwrap();
        auth.close_ticket(child).unwrap();
        assert!(auth.slots.iter().all(|s| !s.in_use));
    }

    #[test]
    fn opaque_tickets_cross_u32_boundary_without_rebinding_closed_handles() {
        let mut auth = CellAuthority::new(CellId(23), 1, CapSet::EMPTY);
        auth.next_ticket_counter = u32::MAX as u64;
        let old = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::BootFSFile, 1, None, None).unwrap();
        auth.close_ticket(old).unwrap();
        let new = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::BootFSFile, 2, None, None).unwrap();
        assert_eq!(new, 1u64 << 32);
        assert_eq!(auth.verify_ticket(old), Err(ControlStatus::Stale));
        assert_eq!(auth.verify_ticket(new).unwrap().rights, 2);
        assert_eq!(auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::BootFSFile, 1, None, Some(old)), Err(ControlStatus::Stale));
    }

    #[test]
    fn ticket_exhaustion_leaves_arena_and_parent_references_unchanged() {
        let mut auth = CellAuthority::new(CellId(24), 1, CapSet::EMPTY);
        auth.next_ticket_counter = u64::MAX - 1;
        let parent = auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 3, None, None).unwrap();
        let before = auth.slots.clone();
        assert_eq!(auth.allocate_slot(Principal::Kernel, Principal::Kernel,
            ResourceKind::CSpaceAuthority, 1, None, Some(parent)), Err(ControlStatus::Exhausted));
        assert_eq!(auth.slots, before);
        assert_eq!(auth.next_ticket_counter, u64::MAX);
        assert_eq!(auth.verify_ticket(parent).unwrap().rights, 3);
    }
}
