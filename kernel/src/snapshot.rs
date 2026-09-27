//! Warm-snapshot image format, capture and restore for Instant On.
//!
//! Saves every allocated physical frame to the reserved P3 sector range on the
//! boot block device.  On the next boot, `try_restore()` replays those frames
//! back to their recorded physical addresses — skipping ELF loading, heap init,
//! and scheduler setup.
//!
//! # Internal format v2 (`SNAPSHOT_FORMAT_VERSION == 2`)
//!
//! v1 (never shipped, and rejected by the version check — no migration, cold
//! boot) hashed frame bytes only on the write side while hashing header + bytes
//! on the read side, and the reader reconstructed a dense
//! `ram_base + index * 4096` run from a write that skipped free frames.  The
//! allocator skips free frames, so a v1 image replayed frame *k* of the payload
//! to the wrong physical address and corrupted RAM.  v2 replaces the implicit
//! dense run with an explicit address inventory and one canonical checksum:
//!
//! ```text
//! LBA base + 0                 : SnapshotHeader (exactly 512 bytes)
//! LBA base + 1 .. +1+N         : inventory: SnapshotRun[run_count], LE, zero-padded
//! LBA base + 1+N .. +image     : payload: each run's frames at their own PAs,
//!                                8 sectors per frame, runs in inventory order
//! ```
//!
//! Identity is carried explicitly: magic, format version, kernel git hash,
//! `ram_base`/`ram_end` (RAM layout), `sector_size`, run/frame counts and the
//! canonical LBA geometry.  Every one of them is recomputed on read and matched
//! against the live boot before a single byte of RAM is written.
//!
//! ## Canonical checksum
//!
//! `crc32 = CRC32( header.canonical_bytes() || inventory sectors || payload
//! sectors )` where `canonical_bytes()` is the header sector with the `crc32`
//! field (`CRC_FIELD_OFFSET`) zeroed, and the inventory and payload are hashed in
//! the exact order they are written.  The writer hashes its constructed
//! COMMITTED header (CRC field zero) and the bytes it writes; the reader hashes
//! the bytes it reads plus the header it parsed.  One definition, one order,
//! both sides — see [`canonical_hasher`].
//!
//! ## On-disk state machine
//!
//! ```text
//! EMPTY ──▶ WRITING ──▶ COMMITTED ──▶ CONSUMING ──▶ CONSUMED
//! ```
//!
//! Capture ordering: write the `WRITING` header and flush (this both invalidates
//! any previously committed image and records that a capture is in flight),
//! write inventory + payload, flush, write the `COMMITTED` header, flush.  Only
//! the second flush makes the image replayable, so a reset at any earlier point
//! leaves a `WRITING` header that no restore will ever replay.
//!
//! Restore ordering: validate the header, the geometry, the capacity, the
//! inventory and the canonical checksum **first**; only then durably flush the
//! `CONSUMING` header; only then replay RAM; then durably mark `CONSUMED`.  A
//! reboot that sees `CONSUMING` or `CONSUMED` refuses the image (an interrupted
//! replay may have left mixed RAM, and a completed replay has nothing left to
//! restore), so a torn image is never resumed.
//!
//! # Qualification
//!
//! `QUALIFICATION_ENABLED` (feature `snapshot-qualified`) gates the shipping
//! path: capture and restore stay refused until an actual save → reset →
//! restore → resume has been proven on a block-capable board.  All-hart
//! quiescence, coherent staging of the frames under capture, and confirmation
//! that the whole mutable kernel-image closure is in the inventory are the
//! hardware-side halves of phase 07 and are still open; see the phase doc.
//!
//! # Test surface
//!
//! The format, the state machine and the corruption matrix are exercised with
//! an in-memory fake sector device (volatile write-back cache + fault
//! injection) and a sparse frame map.  That fake lives in `#[cfg(test)]`
//! (host lane only) and is never linked into a kernel image.

use crate::memory::frame::FRAME_ALLOCATOR;
use crate::task::drivers::block;
use alloc::vec::Vec;
use core::fmt;

/// Reserved LBA range for snapshot storage in `disk_v3.img` — MBR partition P3.
/// Sector 0 = header; sector 1+ = address inventory then frame payload.
pub const SNAPSHOT_BASE_LBA: u64 = crate::loader::disk_layout::PART_SNAPSHOT_BASE_LBA;

/// Size of the reserved P3 partition in sectors (the capacity bound).
pub const SNAPSHOT_SECTOR_COUNT: u64 = crate::loader::disk_layout::PART_SNAPSHOT_SECTORS;

/// First LBA past the reserved partition.
pub const SNAPSHOT_END_LBA: u64 = SNAPSHOT_BASE_LBA + SNAPSHOT_SECTOR_COUNT;

/// Snapshot format version — increment on breaking header layout changes.
/// v1 was never shipped; a v1 header is rejected here and cold boots.
pub const SNAPSHOT_FORMAT_VERSION: u16 = 2;

/// Magic bytes identifying a ViCell snapshot image (little-endian `VICU`).
pub const SNAPSHOT_MAGIC: u32 = 0x5543_4956; // 'U','C','I','V' as bytes on disk

/// Sector size assumed by the format (and checked against the device).
pub const SECTOR_SIZE: usize = 512;

/// Snapshot frame size — one physical page.
pub const FRAME_SIZE: usize = 4096;

/// Sectors consumed by one payload frame.
pub const SECTORS_PER_FRAME: usize = FRAME_SIZE / SECTOR_SIZE;

/// The header occupies exactly one sector.
pub const HEADER_BYTES: usize = SECTOR_SIZE;

/// Byte offset of the `crc32` field inside the header sector.  The canonical
/// checksum definition zeroes these four bytes before hashing the header.
pub const CRC_FIELD_OFFSET: usize = 64;

/// First LBA of the address inventory (immediately after the header).
pub const INVENTORY_FIRST_LBA: u64 = SNAPSHOT_BASE_LBA + 1;

/// One inventory entry: a run of contiguous frames at an explicit start PA.
pub const RUN_BYTES: usize = 16;

/// Qualification gate for warm snapshot capture and restore.
///
/// Phase-01 containment: the writer hashes payload bytes only while the reader
/// hashes header + payload, the reader reconstructs a dense
/// `pa_base + index * 4096` run from a write that skips free frames, there is no
/// all-hart quiescence check, and the restore replays frames over its own live
/// stack and kernel globals. The format, inventory, checksum and states are now
/// specified and unit-tested (phase 07 step 1 + the device-independent half of
/// step 5), but until phase 07 proves save → reset → restore → resume on a
/// block-capable board the path stays disabled in every shipping image: a
/// capture cannot touch the snapshot region and a restore cannot mutate RAM.
/// `snapshot-qualified` is the single build gate phase 07 turns on for that
/// verified profile.
pub const QUALIFICATION_ENABLED: bool = cfg!(feature = "snapshot-qualified");

/// Git SHA short hash baked in at compile time.  Snapshot is invalid if this
/// changes (i.e., the kernel was recompiled since the snapshot was taken).
const KERNEL_GIT_SHA: &str = env!("VERGEN_GIT_SHA");

/// Parse the first 8 hex chars of the git SHA into a u64.
fn kernel_hash() -> u64 {
    let s = KERNEL_GIT_SHA.trim();
    let end = s.len().min(8);
    u64::from_str_radix(&s[..end], 16).unwrap_or(0)
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Why a capture or restore was refused.  Every variant is fail-closed: the
/// caller either cold boots (restore) or leaves an image that could not possibly
/// be replayed (capture).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    /// `snapshot-qualified` is off — the phase-01 gate.
    GateClosed,
    /// Device sector size is not 512 bytes.
    UnsupportedSectorSize,
    /// Device is smaller than the reserved snapshot partition.
    DeviceTooSmall,
    /// Image geometry does not fit inside the reserved partition.
    CapacityExceeded,
    /// Could not read a sector.
    DeviceRead,
    /// Could not write a sector.
    DeviceWrite,
    /// Could not flush pending writes.
    DeviceFlush,
    /// No runs supplied / no inventory to write.
    NoRuns,
    /// A run is empty, unaligned, out of RAM, out of order, duplicate or overlapping.
    BadRun,
    /// More runs than the format can address.
    TooManyRuns,
    /// Header run count does not match the inventory geometry.
    RunCountMismatch,
    /// Inventory frame total does not match the header frame count.
    FrameCountMismatch,
    /// The requested transition is not allowed from the current on-disk state.
    StateConflict,
    /// Header fields (magic, version, state, kernel hash, geometry) are unusable.
    HeaderInvalid,
    /// Header does not describe this boot (RAM layout, capacity, identity).
    IdentityMismatch,
    /// Canonical checksum mismatch.
    ChecksumMismatch,
    /// A frame could not be read from / written to physical memory.
    MemoryFault,
}

impl SnapshotError {
    /// Stable short description for logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GateClosed => "capture unqualified (phase-01 gate: feature `snapshot-qualified`)",
            Self::UnsupportedSectorSize => "unsupported block sector size",
            Self::DeviceTooSmall => "block device smaller than the snapshot partition",
            Self::CapacityExceeded => "snapshot image exceeds the reserved P3 partition",
            Self::DeviceRead => "block read failed",
            Self::DeviceWrite => "block write failed",
            Self::DeviceFlush => "block flush failed",
            Self::NoRuns => "no allocated frames to snapshot",
            Self::BadRun => "invalid run: empty, unaligned, out of RAM, duplicate or overlapping",
            Self::TooManyRuns => "too many inventory runs",
            Self::RunCountMismatch => "inventory run count does not match the header",
            Self::FrameCountMismatch => "inventory frame total does not match the header",
            Self::StateConflict => "on-disk state transition refused",
            Self::HeaderInvalid => "snapshot header invalid",
            Self::IdentityMismatch => "snapshot identity does not match this boot",
            Self::ChecksumMismatch => "canonical checksum mismatch",
            Self::MemoryFault => "physical frame access failed",
        }
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── On-disk state machine ────────────────────────────────────────────────────

/// On-disk image state, stored in `SnapshotHeader::state`.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapshotState {
    /// No image (or a header whose magic/state is unusable).
    Empty = 0,
    /// Capture in flight: the payload is not yet guaranteed complete.
    Writing = 1,
    /// Capture complete and the checksum is valid: replayable.
    Committed = 2,
    /// A restore validated the image and durably committed to replaying it.
    Consuming = 3,
    /// A restore replayed the image; nothing is left to restore.
    Consumed = 4,
}

impl SnapshotState {
    /// Decode the on-disk byte; `None` for an unknown state.
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
            0 => Some(Self::Empty),
            1 => Some(Self::Writing),
            2 => Some(Self::Committed),
            3 => Some(Self::Consuming),
            4 => Some(Self::Consumed),
            _ => None,
        }
    }

    /// Is this transition allowed?  Every on-disk write of a new state goes
    /// through this table, so a capture can never start on top of a replay in
    /// flight and a replay can never start on top of a torn capture.
    pub const fn can_transition_to(self, next: Self) -> bool {
        use SnapshotState::{Committed, Consumed, Consuming, Empty, Writing};
        matches!(
            (self, next),
            (Empty, Writing)
                | (Writing, Writing)
                | (Writing, Empty)
                | (Writing, Committed)
                | (Committed, Writing)
                | (Committed, Consuming)
                | (Committed, Empty)
                | (Consuming, Consumed)
                | (Consuming, Empty)
                | (Consumed, Empty)
                | (Consumed, Writing)
        )
    }

    /// Only a `COMMITTED` image may be replayed.  `CONSUMING`/`CONSUMED` are
    /// refused after any reboot: the former may have left mixed RAM, the latter
    /// has already been replayed once.
    pub const fn is_replayable(self) -> bool {
        matches!(self, Self::Committed)
    }
}

// ── Header / inventory ───────────────────────────────────────────────────────

/// 512-byte snapshot header at the start of the snapshot partition.
///
/// Field offsets are part of the on-disk contract (see [`CRC_FIELD_OFFSET`]).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SnapshotHeader {
    /// Magic: 0x5543_4956 ("UCIV" on disk = "VICU" LE).
    pub magic: u32,
    /// Format version; cold boot if this doesn't match `SNAPSHOT_FORMAT_VERSION`.
    pub version: u16,
    /// On-disk state; see [`SnapshotState`].
    pub state: u8,
    /// Reserved flags (must be zero; covered by the checksum).
    pub flags: u8,
    /// Kernel git SHA (first 8 hex chars → u64).  Invalidates on rebuild.
    pub kernel_hash: u64,
    /// Physical base of the RAM layout at capture time.
    pub ram_base: u64,
    /// Physical end (exclusive) of the RAM layout at capture time.
    pub ram_end: u64,
    /// Device sector size the image was written with.
    pub sector_size: u32,
    /// Number of [`SnapshotRun`] entries in the inventory.
    pub run_count: u32,
    /// Total number of 4096-byte frames stored (sum of run lengths).
    pub frame_count: u32,
    /// Sectors occupied by the inventory.
    pub inventory_sectors: u32,
    /// Total sectors of the image: header + inventory + payload.
    pub image_sectors: u32,
    /// Reserved padding (keeps `payload_lba` 8-byte aligned).
    pub _pad0: u32,
    /// First LBA of the payload.
    pub payload_lba: u64,
    /// Canonical CRC32 — see the module docs.  Zeroed while hashing.
    pub crc32: u32,
    /// Reserved (zero).
    pub _reserved0: u32,
    /// Reserved padding to fill the header sector (zero).
    pub _reserved: [u8; 440],
}

// Compile-time layout guarantee — the header is exactly one sector, and the
// CRC field sits at the offset the canonical checksum definition zeroes.
const _: () = assert!(core::mem::size_of::<SnapshotHeader>() == HEADER_BYTES);

impl SnapshotHeader {
    /// Bytes as stored on disk (CRC field as-is).
    pub fn write_bytes(&self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        // SAFETY: both sides are valid for HEADER_BYTES reads/writes; the
        // destination is a local array and the struct has no implicit padding
        // (every field is declared, including the reserved ones).
        unsafe {
            core::ptr::copy_nonoverlapping(
                self as *const SnapshotHeader as *const u8,
                out.as_mut_ptr(),
                HEADER_BYTES,
            );
        }
        out
    }

    /// Bytes as hashed by the canonical checksum (CRC field zeroed).
    pub fn canonical_bytes(&self) -> [u8; HEADER_BYTES] {
        let mut out = self.write_bytes();
        out[CRC_FIELD_OFFSET..CRC_FIELD_OFFSET + 4].copy_from_slice(&[0u8; 4]);
        out
    }

    /// Parse a header from a full header sector.
    pub fn parse(sector: &[u8; HEADER_BYTES]) -> Self {
        // SAFETY: `read_unaligned` tolerates the 1-byte alignment of a byte
        // array; every field of the struct is initialized from the sector.
        unsafe { core::ptr::read_unaligned(sector.as_ptr() as *const SnapshotHeader) }
    }
}

/// One inventory entry: `frame_count` contiguous 4096-byte frames starting at
/// the explicit physical address `pa`.  Absent from the payload are frames not
/// in any run — the reader never reconstructs a dense run.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SnapshotRun {
    /// Physical start address; 4096-byte aligned.
    pub pa: u64,
    /// Number of contiguous frames in this run; at least 1.
    pub frame_count: u32,
    /// Reserved (must be zero).
    pub flags: u32,
}

const _: () = assert!(core::mem::size_of::<SnapshotRun>() == RUN_BYTES);

impl SnapshotRun {
    /// Physical address of frame `index` inside this run.
    pub fn frame_pa(&self, index: u32) -> u64 {
        self.pa + index as u64 * FRAME_SIZE as u64
    }

    /// Physical end of this run (exclusive).
    pub fn end_pa(&self) -> u64 {
        self.pa + self.frame_count as u64 * FRAME_SIZE as u64
    }
}

/// The physical RAM layout of a boot: the identity a snapshot is bound to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RamLayout {
    /// Physical base of managed RAM.
    pub base: u64,
    /// Physical end (exclusive) of managed RAM.
    pub end: u64,
}

/// Sectors needed to hold `run_count` inventory entries.
fn inventory_sectors(run_count: u64) -> u64 {
    let bytes = run_count.saturating_mul(RUN_BYTES as u64);
    bytes.div_ceil(SECTOR_SIZE as u64)
}

/// The single canonical checksum definition, shared by writer and reader:
/// header (CRC field zeroed) first, then inventory sectors, then payload
/// sectors, in write order.  Callers then `update()` in exactly that order.
fn canonical_hasher(header: &SnapshotHeader) -> crc32fast::Hasher {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&header.canonical_bytes());
    hasher
}

/// Serialize the inventory into zero-padded sectors (little-endian per field).
fn encode_inventory(runs: &[SnapshotRun]) -> Vec<u8> {
    let sectors = inventory_sectors(runs.len() as u64) as usize;
    let mut out = Vec::new();
    out.resize(sectors * SECTOR_SIZE, 0u8);
    for (i, run) in runs.iter().enumerate() {
        let o = i * RUN_BYTES;
        out[o..o + 8].copy_from_slice(&run.pa.to_le_bytes());
        out[o + 8..o + 12].copy_from_slice(&run.frame_count.to_le_bytes());
        out[o + 12..o + 16].copy_from_slice(&run.flags.to_le_bytes());
    }
    out
}

/// Decode `run_count` inventory entries; trailing padding is ignored.
fn decode_inventory(bytes: &[u8], run_count: usize) -> Result<Vec<SnapshotRun>, SnapshotError> {
    if bytes.len() < run_count * RUN_BYTES {
        return Err(SnapshotError::RunCountMismatch);
    }
    let mut runs = Vec::with_capacity(run_count);
    for i in 0..run_count {
        let o = i * RUN_BYTES;
        let pa = u64::from_le_bytes(bytes[o..o + 8].try_into().unwrap_or([0u8; 8]));
        let frame_count = u32::from_le_bytes(bytes[o + 8..o + 12].try_into().unwrap_or([0u8; 4]));
        let flags = u32::from_le_bytes(bytes[o + 12..o + 16].try_into().unwrap_or([0u8; 4]));
        runs.push(SnapshotRun {
            pa,
            frame_count,
            flags,
        });
    }
    Ok(runs)
}

/// Group explicit frame addresses into an address inventory.
///
/// Input must be ascending and strictly unique (as the allocator enumerates
/// allocated frames); contiguous addresses are merged into one run.  A gap in
/// the input starts a new run — the returned inventory covers **exactly** the
/// supplied addresses and nothing else.
pub fn runs_from_frames(pas: &[u64], layout: RamLayout) -> Result<Vec<SnapshotRun>, SnapshotError> {
    if pas.is_empty() {
        return Err(SnapshotError::NoRuns);
    }
    let mut runs: Vec<SnapshotRun> = Vec::new();
    let mut prev: Option<u64> = None;
    for &pa in pas {
        if pa % FRAME_SIZE as u64 != 0 {
            return Err(SnapshotError::BadRun);
        }
        if pa < layout.base
            || pa
                .checked_add(FRAME_SIZE as u64)
                .is_none_or(|e| e > layout.end)
        {
            return Err(SnapshotError::BadRun);
        }
        if let Some(p) = prev {
            if pa <= p {
                // Duplicate or descending — the writer must not emit either.
                return Err(SnapshotError::BadRun);
            }
            let last = runs.last_mut().expect("non-empty");
            if p + FRAME_SIZE as u64 == pa {
                last.frame_count = last
                    .frame_count
                    .checked_add(1)
                    .ok_or(SnapshotError::BadRun)?;
            } else {
                runs.push(SnapshotRun {
                    pa,
                    frame_count: 1,
                    flags: 0,
                });
            }
        } else {
            runs.push(SnapshotRun {
                pa,
                frame_count: 1,
                flags: 0,
            });
        }
        prev = Some(pa);
    }
    Ok(runs)
}

/// Structural validation shared by writer and reader: every run is non-empty,
/// aligned, inside RAM, ascending, non-overlapping and unflagged.  Returns the
/// total frame count.
pub fn frames_in_runs(runs: &[SnapshotRun], layout: RamLayout) -> Result<u32, SnapshotError> {
    if runs.is_empty() {
        return Err(SnapshotError::NoRuns);
    }
    let mut total: u64 = 0;
    let mut prev_end: Option<u64> = None;
    for run in runs {
        if run.flags != 0 || run.frame_count == 0 {
            return Err(SnapshotError::BadRun);
        }
        if run.pa % FRAME_SIZE as u64 != 0 {
            return Err(SnapshotError::BadRun);
        }
        let end = run
            .pa
            .checked_add(
                (run.frame_count as u64)
                    .checked_mul(FRAME_SIZE as u64)
                    .ok_or(SnapshotError::BadRun)?,
            )
            .ok_or(SnapshotError::BadRun)?;
        if run.pa < layout.base || end > layout.end {
            return Err(SnapshotError::BadRun);
        }
        if let Some(pe) = prev_end {
            // `<` rejects overlap, duplicates and descending order; adjacency
            // (`==`) is harmless and is what the writer merges anyway.
            if run.pa < pe {
                return Err(SnapshotError::BadRun);
            }
        }
        prev_end = Some(end);
        total += run.frame_count as u64;
        if total > u32::MAX as u64 {
            return Err(SnapshotError::TooManyRuns);
        }
    }
    Ok(total as u32)
}

// ── Device / memory traits ───────────────────────────────────────────────────

/// Sector device the format is written to and read from.  Implemented for the
/// kernel block device and, in tests, for an in-memory fake.
pub trait SnapshotDevice {
    /// Read one 512-byte sector.
    fn read_sector(&self, lba: u64, buf: &mut [u8]) -> Result<(), SnapshotError>;
    /// Write one 512-byte sector.
    fn write_sector(&self, lba: u64, buf: &[u8]) -> Result<(), SnapshotError>;
    /// Make every previously written sector durable.
    fn flush(&self) -> Result<(), SnapshotError>;
    /// Device sector size in bytes.
    fn sector_size(&self) -> usize;
    /// Device capacity in sectors.
    fn sector_count(&self) -> u64;
}

/// Physical frame access, so the format never touches raw pointers itself.
pub trait FrameMemory {
    /// Read one 4096-byte frame from `pa`.
    fn read_frame(&self, pa: u64, out: &mut [u8; FRAME_SIZE]) -> Result<(), SnapshotError>;
    /// Write one 4096-byte frame to `pa`.
    fn write_frame(&self, pa: u64, src: &[u8; FRAME_SIZE]) -> Result<(), SnapshotError>;
}

/// The kernel's boot block device (MMC on real boards, null device on QEMU).
struct KernelDevice;

impl SnapshotDevice for KernelDevice {
    fn read_sector(&self, lba: u64, buf: &mut [u8]) -> Result<(), SnapshotError> {
        block::read_sector(lba, buf).map_err(|_| SnapshotError::DeviceRead)
    }
    fn write_sector(&self, lba: u64, buf: &[u8]) -> Result<(), SnapshotError> {
        block::write_sector(lba, buf).map_err(|_| SnapshotError::DeviceWrite)
    }
    fn flush(&self) -> Result<(), SnapshotError> {
        block::flush().map_err(|_| SnapshotError::DeviceFlush)
    }
    fn sector_size(&self) -> usize {
        block::block_device().sector_size()
    }
    fn sector_count(&self) -> u64 {
        block::block_device().sector_count()
    }
}

/// Direct physical memory access for the real capture/restore path.
struct KernelMemory;

impl FrameMemory for KernelMemory {
    fn read_frame(&self, pa: u64, out: &mut [u8; FRAME_SIZE]) -> Result<(), SnapshotError> {
        // SAFETY: the caller only supplies PAs from an inventory that was
        // validated against the live managed RAM range, with harts quiesced.
        unsafe {
            core::ptr::copy_nonoverlapping(pa as *const u8, out.as_mut_ptr(), FRAME_SIZE);
        }
        Ok(())
    }
    fn write_frame(&self, pa: u64, src: &[u8; FRAME_SIZE]) -> Result<(), SnapshotError> {
        // SAFETY: as above; restore is gated on `QUALIFICATION_ENABLED`.
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), pa as *mut u8, FRAME_SIZE);
        }
        Ok(())
    }
}

static KERNEL_DEVICE: KernelDevice = KernelDevice;
static KERNEL_MEMORY: KernelMemory = KernelMemory;

// ── Capture ──────────────────────────────────────────────────────────────────

/// What a successful capture wrote.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaptureReport {
    /// Total frames in the image.
    pub frames: u32,
    /// Inventory runs.
    pub runs: u32,
    /// Total image sectors (header + inventory + payload).
    pub image_sectors: u32,
    /// Canonical checksum of the committed image.
    pub crc32: u32,
}

/// Read the on-disk state without trusting anything else in the header.
fn read_on_disk_state(dev: &dyn SnapshotDevice) -> Result<SnapshotState, SnapshotError> {
    let mut sector = [0u8; HEADER_BYTES];
    dev.read_sector(SNAPSHOT_BASE_LBA, &mut sector)?;
    if u32::from_le_bytes(sector[0..4].try_into().unwrap_or([0u8; 4])) != SNAPSHOT_MAGIC {
        return Ok(SnapshotState::Empty);
    }
    Ok(SnapshotState::from_raw(sector[6]).unwrap_or(SnapshotState::Empty))
}

/// Write the header in the given state and flush it durably.
fn write_state(
    dev: &dyn SnapshotDevice,
    header: &SnapshotHeader,
    state: SnapshotState,
) -> Result<(), SnapshotError> {
    let mut staged = *header;
    staged.state = state as u8;
    dev.write_sector(SNAPSHOT_BASE_LBA, &staged.write_bytes())?;
    dev.flush()
}

/// Capture `runs` from `mem` to `dev`, with the durable commit ordering
/// documented in the module header.
///
/// On any error before the commit flush the image is left `WRITING`, which no
/// restore will ever replay.
pub fn capture_image(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    layout: RamLayout,
    runs: &[SnapshotRun],
) -> Result<CaptureReport, SnapshotError> {
    // ── preflight: structure, then capacity, then identity ───────────────────
    if runs.len() as u64 > u32::MAX as u64 {
        return Err(SnapshotError::TooManyRuns);
    }
    let frame_count = frames_in_runs(runs, layout)?;
    let run_count = runs.len() as u64;
    let inv_sectors = inventory_sectors(run_count);
    let payload_lba = INVENTORY_FIRST_LBA + inv_sectors;
    let payload_sectors = frame_count as u64 * SECTORS_PER_FRAME as u64;
    let image_sectors = 1 + inv_sectors + payload_sectors;

    if dev.sector_size() != SECTOR_SIZE {
        return Err(SnapshotError::UnsupportedSectorSize);
    }
    // Identity: the canonical P3 partition must exist on this device.  Refusing
    // here is what stops a capture from writing past the reserved region.
    if dev.sector_count() < SNAPSHOT_END_LBA {
        return Err(SnapshotError::DeviceTooSmall);
    }
    if image_sectors > SNAPSHOT_SECTOR_COUNT || payload_lba + payload_sectors > SNAPSHOT_END_LBA {
        return Err(SnapshotError::CapacityExceeded);
    }

    // ── state machine: EMPTY/predecessor → WRITING ───────────────────────────
    let current = read_on_disk_state(dev)?;
    if !current.can_transition_to(SnapshotState::Writing) {
        return Err(SnapshotError::StateConflict);
    }

    let committed = SnapshotHeader {
        magic: SNAPSHOT_MAGIC,
        version: SNAPSHOT_FORMAT_VERSION,
        state: SnapshotState::Committed as u8,
        flags: 0,
        kernel_hash: kernel_hash(),
        ram_base: layout.base,
        ram_end: layout.end,
        sector_size: SECTOR_SIZE as u32,
        run_count: run_count as u32,
        frame_count,
        inventory_sectors: inv_sectors as u32,
        image_sectors: image_sectors as u32,
        _pad0: 0,
        payload_lba,
        crc32: 0,
        _reserved0: 0,
        _reserved: [0u8; 440],
    };

    // (1) Invalidate any previously committed image and record WRITING.
    write_state(dev, &committed, SnapshotState::Writing)?;

    // The canonical stream is defined over the COMMITTED header (CRC field
    // zero), so the hash is independent of the state byte on disk.
    let mut hasher = canonical_hasher(&committed);

    // (2) Inventory, then payload, in the order they will be hashed and read.
    let inventory = encode_inventory(runs);
    hasher.update(&inventory);
    for (i, chunk) in inventory.chunks(SECTOR_SIZE).enumerate() {
        dev.write_sector(INVENTORY_FIRST_LBA + i as u64, chunk)?;
    }

    let mut lba = payload_lba;
    let mut frame = [0u8; FRAME_SIZE];
    for run in runs {
        for index in 0..run.frame_count {
            mem.read_frame(run.frame_pa(index), &mut frame)?;
            for sector in 0..SECTORS_PER_FRAME {
                let bytes = &frame[sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE];
                hasher.update(bytes);
                dev.write_sector(lba, bytes)?;
                lba += 1;
            }
        }
    }

    // Payload durable before the commit point.
    dev.flush()?;

    // (3) COMMITTED header — the commit point.  Only this flush makes the image
    // replayable.
    let crc32 = hasher.finalize();
    let mut committed = committed;
    committed.crc32 = crc32;
    write_state(dev, &committed, SnapshotState::Committed)?;

    Ok(CaptureReport {
        frames: frame_count,
        runs: run_count as u32,
        image_sectors: image_sectors as u32,
        crc32,
    })
}

// ── Restore ──────────────────────────────────────────────────────────────────

/// Outcome of a restore attempt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestoreOutcome {
    /// The image was validated, durably marked `CONSUMING`, replayed and marked
    /// `CONSUMED`; the caller resumes the restored scheduler.
    Resumed,
    /// Nothing was replayed — the caller must continue a cold boot.
    ColdBoot(&'static str),
    /// Replay had already begun when it failed: RAM is mixed and a cold boot
    /// would run on top of half-restored memory.  The caller must halt/reset.
    FatalMixedRam(&'static str),
}

/// Refuse and erase an image we must never replay again.
fn invalidate_on(dev: &dyn SnapshotDevice) {
    let zero = [0u8; HEADER_BYTES];
    let _ = dev.write_sector(SNAPSHOT_BASE_LBA, &zero);
    let _ = dev.flush();
}

/// Validate and replay a snapshot image.  Never writes RAM before the image is
/// fully verified and `CONSUMING` is durable.
///
/// Invalidation policy: proven corruption or a state we must never replay again
/// (`WRITING`, `CONSUMING`, `CONSUMED`, version/identity mismatch, bad geometry,
/// bad inventory, checksum mismatch) erases the header.  Ambiguous conditions —
/// an I/O error, a device whose layout we do not trust — leave the region alone
/// so a transient failure does not destroy a possibly good image, and so we
/// never write to a device whose partition map we have not confirmed.
pub fn restore_image(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    live: RamLayout,
) -> RestoreOutcome {
    // ── header ───────────────────────────────────────────────────────────────
    let mut sector = [0u8; HEADER_BYTES];
    if dev.read_sector(SNAPSHOT_BASE_LBA, &mut sector).is_err() {
        return RestoreOutcome::ColdBoot("no block device / header unreadable");
    }
    let header = SnapshotHeader::parse(&sector);
    if header.magic != SNAPSHOT_MAGIC {
        return RestoreOutcome::ColdBoot("no snapshot magic");
    }
    let state = match SnapshotState::from_raw(header.state) {
        Some(state) => state,
        None => {
            invalidate_on(dev);
            return RestoreOutcome::ColdBoot("unknown on-disk state");
        }
    };
    match state {
        SnapshotState::Committed => {}
        SnapshotState::Writing => {
            invalidate_on(dev);
            return RestoreOutcome::ColdBoot("torn capture (WRITING) refused");
        }
        SnapshotState::Consuming | SnapshotState::Consumed => {
            invalidate_on(dev);
            return RestoreOutcome::ColdBoot(
                "image already consumed or replay in progress; refused after reboot",
            );
        }
        SnapshotState::Empty => return RestoreOutcome::ColdBoot("header state EMPTY"),
    }
    if header.version != SNAPSHOT_FORMAT_VERSION {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("format version mismatch (no migration; cold boot)");
    }
    if header.kernel_hash != kernel_hash() {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("kernel identity changed");
    }

    // ── capacity / geometry identity (no writes: never touch a region whose
    //    layout we do not trust) ──────────────────────────────────────────────
    if dev.sector_size() != SECTOR_SIZE || header.sector_size as usize != SECTOR_SIZE {
        return RestoreOutcome::ColdBoot("unsupported sector size");
    }
    if dev.sector_count() < SNAPSHOT_END_LBA {
        return RestoreOutcome::ColdBoot("device smaller than the snapshot partition");
    }
    if header.ram_base != live.base || header.ram_end != live.end {
        return RestoreOutcome::ColdBoot("RAM layout moved since capture");
    }
    let run_count = header.run_count as u64;
    let inv_sectors = inventory_sectors(run_count);
    // `run_count > frame_count` is impossible for any image the writer can emit
    // (every run carries at least one frame); rejecting it here bounds the
    // inventory buffer by the payload bound before anything is allocated.
    if run_count == 0
        || run_count > header.frame_count as u64
        || header.inventory_sectors as u64 != inv_sectors
        || inv_sectors > SNAPSHOT_SECTOR_COUNT
    {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("bad inventory geometry");
    }
    let payload_lba = INVENTORY_FIRST_LBA + inv_sectors;
    if header.payload_lba != payload_lba {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("non-canonical payload offset");
    }
    let payload_sectors = header.frame_count as u64 * SECTORS_PER_FRAME as u64;
    let image_sectors = 1 + inv_sectors + payload_sectors;
    if header.image_sectors as u64 != image_sectors {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("bad image geometry");
    }
    if image_sectors > SNAPSHOT_SECTOR_COUNT || payload_lba + payload_sectors > SNAPSHOT_END_LBA {
        return RestoreOutcome::ColdBoot("image exceeds the snapshot partition");
    }
    // No separate "device smaller than the image" check: the partition bound
    // above, plus `sector_count() >= SNAPSHOT_END_LBA`, already implies it.

    // ── inventory ────────────────────────────────────────────────────────────
    let mut inventory = Vec::new();
    inventory.resize(inv_sectors as usize * SECTOR_SIZE, 0u8);
    for i in 0..inv_sectors {
        let at = i as usize * SECTOR_SIZE;
        if dev
            .read_sector(
                INVENTORY_FIRST_LBA + i,
                &mut inventory[at..at + SECTOR_SIZE],
            )
            .is_err()
        {
            return RestoreOutcome::ColdBoot("inventory read failed before replay");
        }
    }
    let runs = match decode_inventory(&inventory, run_count as usize) {
        Ok(runs) => runs,
        Err(_) => {
            invalidate_on(dev);
            return RestoreOutcome::ColdBoot("inventory unreadable");
        }
    };
    let frame_count = match frames_in_runs(&runs, live) {
        Ok(total) => total,
        Err(_) => {
            invalidate_on(dev);
            return RestoreOutcome::ColdBoot(
                "inventory outside RAM, empty, unaligned, duplicate or overlapping",
            );
        }
    };
    if frame_count != header.frame_count {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("inventory frame total does not match the header");
    }

    // ── canonical checksum over header || inventory || payload ───────────────
    let mut hasher = canonical_hasher(&header);
    hasher.update(&inventory);
    let mut buf = [0u8; SECTOR_SIZE];
    for lba in payload_lba..payload_lba + payload_sectors {
        if dev.read_sector(lba, &mut buf).is_err() {
            return RestoreOutcome::ColdBoot("payload read failed before replay");
        }
        hasher.update(&buf);
    }
    if hasher.finalize() != header.crc32 {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("canonical checksum mismatch");
    }

    // ── COMMITTED → CONSUMING, durably, before a single byte of RAM is written
    if write_state(dev, &header, SnapshotState::Consuming).is_err() {
        return RestoreOutcome::ColdBoot("could not durably mark CONSUMING; RAM untouched");
    }

    // ── replay exactly what the inventory says, in payload order ─────────────
    let mut lba = payload_lba;
    let mut frame = [0u8; FRAME_SIZE];
    for run in &runs {
        for index in 0..run.frame_count {
            for sector in 0..SECTORS_PER_FRAME {
                if dev.read_sector(lba, &mut buf).is_err() {
                    return RestoreOutcome::FatalMixedRam("payload read failed after replay began");
                }
                frame[sector * SECTOR_SIZE..(sector + 1) * SECTOR_SIZE].copy_from_slice(&buf);
                lba += 1;
            }
            if mem.write_frame(run.frame_pa(index), &frame).is_err() {
                return RestoreOutcome::FatalMixedRam("frame write failed after replay began");
            }
        }
    }

    // ── CONSUMING → CONSUMED (best effort: a failure leaves CONSUMING, which
    //    the next boot refuses) ───────────────────────────────────────────────
    if write_state(dev, &header, SnapshotState::Consumed).is_err() {
        log::warn!("[snapshot] CONSUMED marker not durable; next boot will refuse this image");
    }
    RestoreOutcome::Resumed
}

// ── Kernel entry points ──────────────────────────────────────────────────────

/// The live managed RAM layout, or `None` before the allocator exists.
fn live_layout() -> Option<RamLayout> {
    let guard = FRAME_ALLOCATOR.lock();
    let allocator = guard.as_ref()?;
    Some(RamLayout {
        base: allocator.memory_start() as u64,
        end: allocator.memory_end() as u64,
    })
}

/// Build the capture inventory from the allocator bitmap.
///
/// Enumerates allocated, in-RAM frames in ascending order and groups them into
/// contiguous runs.  The allocator lock is held only for this enumeration — no
/// block I/O happens under it.
fn plan_inventory() -> Result<(RamLayout, Vec<SnapshotRun>), SnapshotError> {
    let guard = FRAME_ALLOCATOR.lock();
    let allocator = guard.as_ref().ok_or(SnapshotError::MemoryFault)?;
    let layout = RamLayout {
        base: allocator.memory_start() as u64,
        end: allocator.memory_end() as u64,
    };
    let mut pas: Vec<u64> = Vec::new();
    for index in 0..allocator.total_frames() {
        if !allocator.is_frame_allocated(index) {
            continue;
        }
        let pa = allocator.frame_addr(index) as u64;
        // Defensive: never describe a frame outside managed RAM (MMIO holes,
        // allocator bookkeeping drift).  The bounds check in `runs_from_frames`
        // would refuse the whole capture instead; skipping matches the old
        // behaviour and keeps a stray bitmap bit from blocking every capture.
        if pa >= layout.base && pa + FRAME_SIZE as u64 <= layout.end {
            pas.push(pa);
        } else {
            log::warn!("[snapshot] skipping allocated frame outside RAM: 0x{pa:X}");
        }
    }
    let runs = runs_from_frames(&pas, layout)?;
    Ok((layout, runs))
}

/// Serialize all allocated physical frames to the reserved disk sector range.
///
/// Returns the number of frames written on success.
///
/// # Safety constraints
/// Must be called with all cells quiesced (at a `yield_cpu()` point) so no
/// task stack is mid-function-call when the memory image is frozen, and with no
/// affinity operation racing: the format cannot detect bytes that changed
/// between the read and the write.
pub fn serialize_snapshot() -> Result<u32, SnapshotError> {
    if !QUALIFICATION_ENABLED {
        return Err(SnapshotError::GateClosed);
    }
    #[cfg(target_arch = "riscv64")]
    let t0 = hal::common::timer::read_mtime();

    let (layout, runs) = plan_inventory()?;
    let report = capture_image(&KERNEL_DEVICE, &KERNEL_MEMORY, layout, &runs)?;

    #[cfg(target_arch = "riscv64")]
    let elapsed_ms = (hal::common::timer::read_mtime().wrapping_sub(t0)) / 10_000;
    #[cfg(not(target_arch = "riscv64"))]
    let elapsed_ms = 0u64;

    log::info!(
        "[snapshot] wrote {} frames in {} runs ({} sectors, crc {:08X}) in {} ms to LBA {}",
        report.frames,
        report.runs,
        report.image_sectors,
        report.crc32,
        elapsed_ms,
        SNAPSHOT_BASE_LBA
    );
    Ok(report.frames)
}

/// Attempt to restore the kernel from a previously written snapshot.
///
/// Returns `true` if warm boot succeeded — the caller must skip cold-boot
/// cell initialization.  Returns `false` on any validation failure.
///
/// # Calling contract
/// Must be called AFTER the block device is initialized (needed for disk reads)
/// and BEFORE `EarlyLoader::probe()` or `task::init()` (cells are about to be
/// replaced by the restored task set).
pub fn try_restore() -> bool {
    if !QUALIFICATION_ENABLED {
        // Cold boot only. An image that predates this gate would still replay a
        // header it finds here, so clear one if present — one header sector,
        // before any other snapshot I/O, never a payload write.
        invalidate_stale_snapshot_header();
        return false;
    }
    let live = match live_layout() {
        Some(layout) => layout,
        None => {
            log::warn!("[snapshot] frame allocator not ready → cold boot");
            return false;
        }
    };

    match restore_image(&KERNEL_DEVICE, &KERNEL_MEMORY, live) {
        RestoreOutcome::ColdBoot(reason) => {
            log::info!("[snapshot] cold boot: {reason}");
            false
        }
        RestoreOutcome::FatalMixedRam(reason) => halt_mixed_ram(reason),
        RestoreOutcome::Resumed => {
            #[cfg(target_arch = "riscv64")]
            let t_restore_start = hal::common::timer::read_mtime();

            log::info!("[snapshot] frames restored → reinitializing hardware");

            // Reinitialize hardware — MMIO registers reset on every power cycle.
            // This MUST happen after frame restore because init_driver() writes
            // device registers that were cleared by hardware reset.
            #[cfg(target_arch = "riscv64")]
            if let Some((context, irqs, irq_count)) = crate::platform::riscv_plic_init_data() {
                crate::hal::common::plic::init(context, &irqs[..irq_count]);
            } else {
                log::warn!(
                    "[plic] no active RV64 context mapping during restore; external IRQs stay disabled"
                );
            }

            // Re-run driver init: device registers were reset by the power
            // cycle, so descriptor-ring state inside the restored driver
            // structs no longer matches device-side state.
            crate::task::drivers::init();

            // Re-arm the scheduler timer.
            #[cfg(target_arch = "riscv64")]
            {
                let next = hal::common::timer::read_mtime() + hal::common::timer::TICKS_PER_10MS;
                hal::common::sbi::set_timer(next);
            }

            #[cfg(target_arch = "riscv64")]
            {
                let elapsed_ms =
                    hal::common::timer::read_mtime().wrapping_sub(t_restore_start) / 10_000;
                log::info!("[snapshot] warm boot: {} ms restore", elapsed_ms);
            }
            log::info!("[snapshot] warm boot complete → resuming scheduler");

            // SCHEDULER is now Some(restored) — yield_cpu() picks the first ready
            // task.  Cells resume from their last yield point.
            crate::task::yield_cpu();

            // yield_cpu() should not return (tasks are ready); fall through to
            // cold boot as a safety net if the restored scheduler is empty.
            false
        }
    }
}

/// Replay already began, so RAM is a mix of boot and snapshot state: a cold
/// reboot is the only safe response (firmware re-initializes RAM, and continuing
/// a cold boot on top of torn memory would run corrupted kernel state).
fn halt_mixed_ram(reason: &'static str) -> ! {
    log::error!(
        "[snapshot] FATAL: {reason} — RAM is partially replayed; refusing to continue a cold boot"
    );
    // Same idiom as the panic handler: SBI SRST cold reboot where it exists,
    // halt loop everywhere else.
    #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
    crate::hal::sbi::system_reset(crate::hal::sbi::SBI_RESET_COLD_REBOOT, 0);
    loop {
        #[cfg(any(target_arch = "riscv64", target_arch = "riscv32"))]
        unsafe {
            core::arch::asm!("wfi", options(nomem, nostack));
        }
        #[cfg(not(any(target_arch = "riscv64", target_arch = "riscv32")))]
        core::hint::spin_loop();
    }
}

/// Zero out the snapshot magic to force cold boot on next restart.
///
/// Called when validation fails (identity mismatch, checksum error) to prevent
/// the system from repeatedly attempting to load a stale or corrupt snapshot.
pub fn invalidate_snapshot() {
    invalidate_on(&KERNEL_DEVICE);
    log::info!("[snapshot] snapshot invalidated");
}

/// Clear a snapshot header written by an image that predates the qualification
/// gate, so that image cannot replay it after a downgrade.
///
/// Reads the header sector first and writes only when it carries the snapshot
/// magic, so a cold boot on a board without a snapshot region performs no stray
/// write. Failures are non-fatal: the gate already refuses every read and write
/// of the region on this image.
fn invalidate_stale_snapshot_header() {
    let mut sector = [0u8; HEADER_BYTES];
    if block::read_sector(SNAPSHOT_BASE_LBA, &mut sector).is_err() {
        return;
    }
    let magic = u32::from_le_bytes([sector[0], sector[1], sector[2], sector[3]]);
    if magic != SNAPSHOT_MAGIC {
        return;
    }
    invalidate_snapshot();
    log::warn!(
        "[snapshot] stale header invalidated: capture/restore disabled by the phase-01 \
         qualification gate (feature `snapshot-qualified`)"
    );
}

// ── Header validation (pure, no I/O) ─────────────────────────────────────────

/// Validate a snapshot header without I/O — identity (magic, version, state,
/// kernel git hash) and the sector size / non-empty geometry.  Does NOT verify
/// the canonical checksum (that requires reading the inventory and payload).
pub fn validate_header(h: &SnapshotHeader) -> bool {
    h.magic == SNAPSHOT_MAGIC
        && h.version == SNAPSHOT_FORMAT_VERSION
        && h.state == SnapshotState::Committed as u8
        && h.kernel_hash == kernel_hash()
        && h.sector_size as usize == SECTOR_SIZE
        && h.run_count > 0
        && h.frame_count > 0
}

// ── Test-only fake device / RAM ──────────────────────────────────────────────
//
// Never linked into a kernel image: `cfg(test)` is the host test lane, not the
// bare-metal `test-hooks` kernel.

#[cfg(test)]
mod fake {
    use super::*;
    use alloc::collections::BTreeMap;
    use core::cell::{Cell, RefCell};

    /// Injected fault, keyed by a 1-based operation ordinal.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Fault {
        /// No fault.
        None,
        /// The `n`-th write fails and stores nothing.
        Write(u64),
        /// The `n`-th read fails.
        Read(u64),
        /// The `n`-th flush fails and commits nothing.
        Flush(u64),
        /// The `n`-th write lands durably but only its first `prefix` bytes are
        /// replaced; the rest keeps the previous durable content.  Models a
        /// torn sector that reached the platter.
        Torn { ordinal: u64, prefix: usize },
        /// The `n`-th flush commits and then the "power" dies: the operation
        /// returns an error and every later volatile write is lost.
        CrashAfterFlush(u64),
    }

    #[derive(Default)]
    struct State {
        durable: BTreeMap<u64, [u8; SECTOR_SIZE]>,
        pending: BTreeMap<u64, [u8; SECTOR_SIZE]>,
        writes: u64,
        reads: u64,
        flushes: u64,
        write_log: Vec<(u64, u64)>,
        read_log: Vec<u64>,
        flush_log: Vec<u64>,
    }

    /// In-memory sector device with a volatile write-back cache: a reset
    /// (`power_cycle`) keeps the durable state and drops every unflushed write.
    pub struct FakeDisk {
        inner: RefCell<State>,
        pub capacity: u64,
        pub sector_size: usize,
        pub fault: Cell<Fault>,
    }

    impl FakeDisk {
        pub fn new() -> Self {
            Self {
                inner: RefCell::new(State::default()),
                capacity: SNAPSHOT_END_LBA + 4096,
                sector_size: SECTOR_SIZE,
                fault: Cell::new(Fault::None),
            }
        }

        pub fn with_fault(fault: Fault) -> Self {
            let disk = Self::new();
            disk.fault.set(fault);
            disk
        }

        /// A device with a non-default capacity (identity/capacity tests).
        pub fn with_capacity(mut self, capacity: u64) -> Self {
            self.capacity = capacity;
            self
        }

        /// A device with a non-512 sector size.
        pub fn with_sector_size(mut self, sector_size: usize) -> Self {
            self.sector_size = sector_size;
            self
        }

        pub fn power_cycle(&self) {
            self.inner.borrow_mut().pending.clear();
        }

        pub fn durable_sector(&self, lba: u64) -> [u8; SECTOR_SIZE] {
            self.inner
                .borrow()
                .durable
                .get(&lba)
                .copied()
                .unwrap_or([0u8; SECTOR_SIZE])
        }

        /// Edit durable storage directly (corruption crafting), without
        /// touching the operation counters.
        pub fn patch(&self, lba: u64, offset: usize, bytes: &[u8]) {
            let mut s = self.inner.borrow_mut();
            let slot = s.durable.entry(lba).or_insert([0u8; SECTOR_SIZE]);
            slot[offset..offset + bytes.len()].copy_from_slice(bytes);
            s.pending.remove(&lba);
        }

        pub fn writes(&self) -> u64 {
            self.inner.borrow().writes
        }
        pub fn reads(&self) -> u64 {
            self.inner.borrow().reads
        }
        pub fn flushes(&self) -> u64 {
            self.inner.borrow().flushes
        }
        pub fn write_log(&self) -> Vec<(u64, u64)> {
            self.inner.borrow().write_log.clone()
        }
        pub fn header(&self) -> SnapshotHeader {
            SnapshotHeader::parse(&self.durable_sector(SNAPSHOT_BASE_LBA))
        }
        /// State byte as it stands durably on disk (not via faults).
        pub fn state_byte(&self) -> u8 {
            self.durable_sector(SNAPSHOT_BASE_LBA)[6]
        }
        /// Recompute and store the canonical checksum over the durable image,
        /// so that a crafted image is rejected by structure, not by the CRC.
        /// A crafted header may claim an absurd geometry; anything beyond the
        /// partition capacity is left alone (the reader rejects it structurally).
        pub fn fixup_crc(&self) {
            let header = self.header();
            let inv_sectors = header.inventory_sectors as u64;
            let payload_sectors = header.frame_count as u64 * SECTORS_PER_FRAME as u64;
            if inv_sectors > SNAPSHOT_SECTOR_COUNT || payload_sectors > SNAPSHOT_SECTOR_COUNT {
                return;
            }
            let mut hasher = canonical_hasher(&header);
            for i in 0..inv_sectors {
                hasher.update(&self.durable_sector(INVENTORY_FIRST_LBA + i));
            }
            for i in 0..payload_sectors {
                hasher.update(&self.durable_sector(header.payload_lba + i));
            }
            let crc = hasher.finalize();
            self.patch(SNAPSHOT_BASE_LBA, CRC_FIELD_OFFSET, &crc.to_le_bytes());
        }

        /// Zero the operation counters and logs, so a test can inject a fault at
        /// a predictable ordinal after a fixture has already done I/O.
        pub fn reset_counters(&self) {
            let mut s = self.inner.borrow_mut();
            s.writes = 0;
            s.reads = 0;
            s.flushes = 0;
            s.write_log.clear();
            s.read_log.clear();
            s.flush_log.clear();
        }
    }

    impl SnapshotDevice for FakeDisk {
        fn read_sector(&self, lba: u64, buf: &mut [u8]) -> Result<(), SnapshotError> {
            let mut s = self.inner.borrow_mut();
            if lba >= self.capacity {
                return Err(SnapshotError::DeviceRead);
            }
            s.reads += 1;
            s.read_log.push(lba);
            if let Fault::Read(n) = self.fault.get() {
                if s.reads == n {
                    return Err(SnapshotError::DeviceRead);
                }
            }
            let src = s.pending.get(&lba).or_else(|| s.durable.get(&lba));
            match src {
                Some(bytes) => buf[..SECTOR_SIZE].copy_from_slice(bytes),
                None => buf[..SECTOR_SIZE].fill(0),
            }
            Ok(())
        }

        fn write_sector(&self, lba: u64, buf: &[u8]) -> Result<(), SnapshotError> {
            let mut s = self.inner.borrow_mut();
            if lba >= self.capacity {
                return Err(SnapshotError::DeviceWrite);
            }
            s.writes += 1;
            let ordinal = s.writes;
            s.write_log.push((ordinal, lba));
            if let Fault::Torn { ordinal, prefix } = self.fault.get() {
                if s.writes == ordinal {
                    let mut slot = s.durable.get(&lba).copied().unwrap_or([0u8; SECTOR_SIZE]);
                    let keep = if prefix >= SECTOR_SIZE { 0 } else { prefix };
                    slot[keep..].copy_from_slice(&buf[keep..SECTOR_SIZE]);
                    s.durable.insert(lba, slot);
                    return Ok(());
                }
            }
            if let Fault::Write(n) = self.fault.get() {
                if s.writes == n {
                    return Err(SnapshotError::DeviceWrite);
                }
            }
            let mut slot = [0u8; SECTOR_SIZE];
            slot.copy_from_slice(&buf[..SECTOR_SIZE]);
            s.pending.insert(lba, slot);
            Ok(())
        }

        fn flush(&self) -> Result<(), SnapshotError> {
            let mut s = self.inner.borrow_mut();
            s.flushes += 1;
            let ordinal = s.flushes;
            s.flush_log.push(ordinal);
            if let Fault::Flush(n) = self.fault.get() {
                if s.flushes == n {
                    return Err(SnapshotError::DeviceFlush);
                }
            }
            let pending = core::mem::take(&mut s.pending);
            for (lba, bytes) in pending {
                s.durable.insert(lba, bytes);
            }
            if let Fault::CrashAfterFlush(n) = self.fault.get() {
                if s.flushes == n {
                    return Err(SnapshotError::DeviceFlush);
                }
            }
            Ok(())
        }

        fn sector_size(&self) -> usize {
            self.sector_size
        }

        fn sector_count(&self) -> u64 {
            self.capacity
        }
    }

    /// Sparse physical RAM: only mapped frames exist.
    #[derive(Default)]
    pub struct FakeRam {
        frames: RefCell<BTreeMap<u64, [u8; FRAME_SIZE]>>,
        pub writes: RefCell<Vec<u64>>,
        pub reads: Cell<u64>,
        pub fail_read_at: Cell<Option<u64>>,
        pub fail_write_at: Cell<Option<u64>>,
    }

    impl FakeRam {
        pub fn put(&self, pa: u64, fill: u8) {
            self.frames.borrow_mut().insert(pa, [fill; FRAME_SIZE]);
        }
        pub fn key_set(&self) -> Vec<u64> {
            self.frames.borrow().keys().copied().collect()
        }
        pub fn map(&self) -> BTreeMap<u64, [u8; FRAME_SIZE]> {
            self.frames.borrow().clone()
        }
        pub fn bytes(&self, pa: u64) -> Option<[u8; FRAME_SIZE]> {
            self.frames.borrow().get(&pa).copied()
        }
        pub fn write_count(&self) -> usize {
            self.writes.borrow().len()
        }
    }

    impl FrameMemory for FakeRam {
        fn read_frame(&self, pa: u64, out: &mut [u8; FRAME_SIZE]) -> Result<(), SnapshotError> {
            self.reads.set(self.reads.get() + 1);
            if self.fail_read_at.get() == Some(self.reads.get()) {
                return Err(SnapshotError::MemoryFault);
            }
            match self.frames.borrow().get(&pa) {
                Some(frame) => {
                    *out = *frame;
                    Ok(())
                }
                None => Err(SnapshotError::MemoryFault),
            }
        }

        fn write_frame(&self, pa: u64, src: &[u8; FRAME_SIZE]) -> Result<(), SnapshotError> {
            let mut writes = self.writes.borrow_mut();
            writes.push(pa);
            if self.fail_write_at.get() == Some(writes.len() as u64) {
                return Err(SnapshotError::MemoryFault);
            }
            self.frames.borrow_mut().insert(pa, *src);
            Ok(())
        }
    }
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::fake::{FakeDisk, FakeRam, Fault};
    use super::*;

    /// 64 frames of RAM at the RV64 RAM base.
    const BASE: u64 = 0x8020_0000;
    const FRAMES: u64 = 64;

    fn layout() -> RamLayout {
        RamLayout {
            base: BASE,
            end: BASE + FRAMES * FRAME_SIZE as u64,
        }
    }

    /// Sparse allocated frame set: two adjacent frames, a lone frame, a pair,
    /// and a frame far away — the shape that broke dense reconstruction.
    const SPARSE: [u64; 6] = [0, 1, 3, 7, 8, 20];

    fn sparse_pas() -> Vec<u64> {
        SPARSE
            .iter()
            .map(|i| BASE + i * FRAME_SIZE as u64)
            .collect()
    }

    fn sparse_ram() -> FakeRam {
        let ram = FakeRam::default();
        for (i, index) in SPARSE.iter().enumerate() {
            ram.put(BASE + index * FRAME_SIZE as u64, 0xA0 + i as u8);
        }
        ram
    }

    fn sparse_runs() -> Vec<SnapshotRun> {
        runs_from_frames(&sparse_pas(), layout()).expect("sparse inventory")
    }

    fn valid_header() -> SnapshotHeader {
        SnapshotHeader {
            magic: SNAPSHOT_MAGIC,
            version: SNAPSHOT_FORMAT_VERSION,
            state: SnapshotState::Committed as u8,
            flags: 0,
            kernel_hash: kernel_hash(),
            ram_base: BASE,
            ram_end: BASE + FRAMES * FRAME_SIZE as u64,
            sector_size: SECTOR_SIZE as u32,
            run_count: 2,
            frame_count: 8,
            inventory_sectors: 1,
            image_sectors: 1 + 1 + 8 * SECTORS_PER_FRAME as u32,
            _pad0: 0,
            payload_lba: INVENTORY_FIRST_LBA + 1,
            crc32: 0xDEAD_BEEF,
            _reserved0: 0,
            _reserved: [0u8; 440],
        }
    }

    /// Capture the sparse image and return the disk with a clean fault setting.
    /// Counters are reset so a fault ordinal a test injects afterwards refers to
    /// that test's own I/O, not to the fixture capture.
    fn captured() -> FakeDisk {
        let disk = FakeDisk::new();
        let source = sparse_ram();
        capture_image(&disk, &source, layout(), &sparse_runs()).expect("capture succeeds");
        disk.reset_counters();
        disk
    }

    // ── format ───────────────────────────────────────────────────────────────

    #[test]
    fn snapshot_header_layout_is_stable() {
        assert_eq!(core::mem::size_of::<SnapshotHeader>(), SECTOR_SIZE);
        assert_eq!(core::mem::size_of::<SnapshotRun>(), RUN_BYTES);
        assert_eq!(CRC_FIELD_OFFSET, 64);
        let bytes = valid_header().write_bytes();
        assert_eq!(&bytes[0..4], b"VICU");
        assert_eq!(
            &bytes[CRC_FIELD_OFFSET..CRC_FIELD_OFFSET + 4],
            &0xDEAD_BEEFu32.to_le_bytes()
        );
        let canonical = valid_header().canonical_bytes();
        assert_eq!(
            &canonical[CRC_FIELD_OFFSET..CRC_FIELD_OFFSET + 4],
            &[0u8; 4]
        );
        // Everything else is untouched by zeroing the CRC field.
        assert_eq!(bytes[..CRC_FIELD_OFFSET], canonical[..CRC_FIELD_OFFSET]);
        assert_eq!(
            bytes[CRC_FIELD_OFFSET + 4..],
            canonical[CRC_FIELD_OFFSET + 4..]
        );
    }

    #[test]
    fn snapshot_state_machine_transitions_are_explicit() {
        use SnapshotState::*;
        assert!(Empty.can_transition_to(Writing));
        assert!(Writing.can_transition_to(Committed));
        assert!(Committed.can_transition_to(Consuming));
        assert!(Consuming.can_transition_to(Consumed));
        assert!(Committed.can_transition_to(Writing));
        assert!(Consumed.can_transition_to(Writing));
        assert!(Writing.can_transition_to(Empty));
        assert!(Consuming.can_transition_to(Empty));
        // Denied edges: a capture must not cut in front of a replay, and a
        // committed image cannot skip the CONSUMING marker.
        assert!(!Consuming.can_transition_to(Writing));
        assert!(!Committed.can_transition_to(Consumed));
        assert!(!Empty.can_transition_to(Committed));
        assert!(!Consuming.can_transition_to(Committed));
        assert!(!Empty.can_transition_to(Consuming));
        assert!(Committed.is_replayable());
        for state in [Empty, Writing, Consuming, Consumed] {
            assert!(!state.is_replayable());
            assert_eq!(SnapshotState::from_raw(state as u8), Some(state));
        }
        assert_eq!(SnapshotState::from_raw(9), None);
    }

    #[test]
    fn snapshot_inventory_covers_exactly_the_supplied_frames() {
        let runs = sparse_runs();
        assert_eq!(
            runs,
            alloc::vec![
                SnapshotRun {
                    pa: BASE,
                    frame_count: 2,
                    flags: 0
                },
                SnapshotRun {
                    pa: BASE + 3 * FRAME_SIZE as u64,
                    frame_count: 1,
                    flags: 0
                },
                SnapshotRun {
                    pa: BASE + 7 * FRAME_SIZE as u64,
                    frame_count: 2,
                    flags: 0
                },
                SnapshotRun {
                    pa: BASE + 20 * FRAME_SIZE as u64,
                    frame_count: 1,
                    flags: 0
                },
            ]
        );
        // Union of run frames == input, in order: no gap is ever covered.
        let covered: Vec<u64> = runs
            .iter()
            .flat_map(|r| (0..r.frame_count).map(|i| r.frame_pa(i)))
            .collect();
        assert_eq!(covered, sparse_pas());
        // And the gaps really are gaps.
        assert!(!covered.contains(&(BASE + 2 * FRAME_SIZE as u64)));
        assert_eq!(
            frames_in_runs(&runs, layout()).unwrap(),
            SPARSE.len() as u32
        );

        // Writer-side refusals.
        assert_eq!(runs_from_frames(&[], layout()), Err(SnapshotError::NoRuns));
        assert_eq!(
            runs_from_frames(&[BASE, BASE], layout()),
            Err(SnapshotError::BadRun)
        );
        assert_eq!(
            runs_from_frames(&[BASE + 1, BASE + FRAME_SIZE as u64], layout()),
            Err(SnapshotError::BadRun)
        );
        assert_eq!(
            runs_from_frames(&[BASE, BASE - FRAME_SIZE as u64], layout()),
            Err(SnapshotError::BadRun)
        );
        assert_eq!(
            runs_from_frames(&[BASE - FRAME_SIZE as u64], layout()),
            Err(SnapshotError::BadRun)
        );
        assert_eq!(
            runs_from_frames(&[BASE + FRAMES * FRAME_SIZE as u64], layout()),
            Err(SnapshotError::BadRun)
        );
    }

    // ── round trip ───────────────────────────────────────────────────────────

    #[test]
    fn snapshot_round_trips_address_inventory_exactly() {
        let disk = FakeDisk::new();
        let source = sparse_ram();
        let report = capture_image(&disk, &source, layout(), &sparse_runs()).unwrap();
        assert_eq!(report.frames, SPARSE.len() as u32);
        assert_eq!(report.runs, 4);
        assert_eq!(
            report.image_sectors,
            1 + 1 + SPARSE.len() as u32 * SECTORS_PER_FRAME as u32
        );
        assert_eq!(disk.header().state, SnapshotState::Committed as u8);
        assert_eq!(disk.header().crc32, report.crc32);
        assert_eq!(disk.flushes(), 3);

        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        // Address-inventory-exact: same frames at the same PAs, byte for byte,
        // and no frame written that the inventory did not name.
        assert_eq!(target.map(), source.map());
        assert_eq!(target.key_set(), sparse_pas());
        assert_eq!(target.writes.borrow().as_slice(), sparse_pas().as_slice());
        // The replay is consumed: a further boot must not replay it again.
        assert_eq!(disk.header().state, SnapshotState::Consumed as u8);
        let second = FakeRam::default();
        assert!(matches!(
            restore_image(&disk, &second, layout()),
            RestoreOutcome::ColdBoot(_)
        ));
        assert_eq!(second.write_count(), 0);
        assert_eq!(disk.state_byte(), 0, "refused image is invalidated");
    }

    #[test]
    fn snapshot_sparse_frames_are_not_reconstructed_dense() {
        let disk = FakeDisk::new();
        let source = sparse_ram();
        capture_image(&disk, &source, layout(), &sparse_runs()).unwrap();

        // Payload order follows the inventory, not ascending frame index from
        // ram_base: the payload frames must be exactly the sparse PAs.
        let header = disk.header();
        let first_payload_pa = decode_inventory(
            &(0..header.inventory_sectors)
                .flat_map(|i| disk.durable_sector(INVENTORY_FIRST_LBA + i as u64))
                .collect::<Vec<u8>>(),
            header.run_count as usize,
        )
        .unwrap()[0]
            .pa;
        assert_eq!(first_payload_pa, BASE);

        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        // The free frames in the gaps must never be written: that is precisely
        // what the dense `pa_base + index * 4096` reconstruction did.
        for gap in [2u64, 4, 5, 6, 9] {
            let pa = BASE + gap * FRAME_SIZE as u64;
            assert!(
                target.bytes(pa).is_none(),
                "gap frame 0x{pa:X} must not be written"
            );
            assert!(!target.writes.borrow().contains(&pa));
        }
        assert_eq!(target.key_set(), sparse_pas());
    }

    #[test]
    fn snapshot_capture_leaves_inventory_and_payload_in_identical_order() {
        let disk = FakeDisk::new();
        capture_image(&disk, &sparse_ram(), layout(), &sparse_runs()).unwrap();
        let log = disk.write_log();
        let mut payload_lbas: Vec<u64> = log
            .iter()
            .filter(|(_, lba)| *lba >= INVENTORY_FIRST_LBA + 1)
            .map(|(_, lba)| *lba)
            .collect();
        payload_lbas.dedup();
        let expected: Vec<u64> = (INVENTORY_FIRST_LBA + 1
            ..INVENTORY_FIRST_LBA + 1 + SPARSE.len() as u64 * SECTORS_PER_FRAME as u64)
            .collect();
        assert_eq!(payload_lbas, expected);
    }

    // ── state machine / reset matrix ─────────────────────────────────────────

    #[test]
    fn snapshot_reset_after_every_flush_resumes_only_after_the_commit_flush() {
        for flush in 1..=3u64 {
            let disk = FakeDisk::with_fault(Fault::CrashAfterFlush(flush));
            let source = sparse_ram();
            // The crash kills the capture mid-flight at flush #`flush`.
            assert!(capture_image(&disk, &source, layout(), &sparse_runs()).is_err());
            assert_eq!(
                disk.flushes(),
                flush,
                "crash happened at the flush we asked for"
            );
            disk.power_cycle();

            let target = FakeRam::default();
            let outcome = restore_image(&disk, &target, layout());
            if flush == 3 {
                // The commit flush was durable: the image is complete.
                assert_eq!(outcome, RestoreOutcome::Resumed);
                assert_eq!(target.map(), source.map());
            } else {
                assert!(
                    matches!(outcome, RestoreOutcome::ColdBoot(_)),
                    "reset after flush {flush} must cold boot, got {outcome:?}"
                );
                assert_eq!(target.write_count(), 0, "no RAM replay before commit");
                assert_eq!(disk.state_byte(), 0);
            }
        }
    }

    #[test]
    fn snapshot_consuming_and_consumed_images_are_refused_after_reboot() {
        let disk = captured();
        // Reboot after the CONSUMING flush but before replay finished: the image
        // must be refused and erased.
        let reboot_disk = captured();
        assert_eq!(reboot_disk.state_byte(), SnapshotState::Committed as u8);
        // Crash after the CONSUMING flush (restore flush #1).
        reboot_disk.fault.set(Fault::CrashAfterFlush(1));
        let mixed = FakeRam::default();
        assert_eq!(
            restore_image(&reboot_disk, &mixed, layout()),
            RestoreOutcome::ColdBoot("could not durably mark CONSUMING; RAM untouched")
        );
        assert_eq!(mixed.write_count(), 0);
        assert_eq!(reboot_disk.state_byte(), SnapshotState::Consuming as u8);
        // Reboot: the CONSUMING image is refused and invalidated.
        reboot_disk.fault.set(Fault::None);
        let after = FakeRam::default();
        assert_eq!(
            restore_image(&reboot_disk, &after, layout()),
            RestoreOutcome::ColdBoot(
                "image already consumed or replay in progress; refused after reboot"
            )
        );
        assert_eq!(after.write_count(), 0);
        assert_eq!(reboot_disk.state_byte(), 0);

        // A COMMITTED image whose CONSUMING flush succeeded but which then
        // rebooted before replay is exactly the same case; and after a fully
        // successful restore the CONSUMED image is refused too (see the
        // round-trip test). Both are covered above and below.
        let consumed = captured();
        let sink = FakeRam::default();
        assert_eq!(
            restore_image(&consumed, &sink, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(consumed.state_byte(), SnapshotState::Consumed as u8);
        let after_consumed = FakeRam::default();
        assert_eq!(
            restore_image(&consumed, &after_consumed, layout()),
            RestoreOutcome::ColdBoot(
                "image already consumed or replay in progress; refused after reboot"
            )
        );
        assert_eq!(after_consumed.write_count(), 0);
        assert_eq!(
            disk.state_byte(),
            SnapshotState::Committed as u8,
            "unused fixture untouched"
        );
    }

    #[test]
    fn snapshot_capture_refuses_to_start_on_top_of_a_replay_in_flight() {
        let disk = captured();
        // A capture on a COMMITTED image is allowed (it invalidates first).
        let source = sparse_ram();
        capture_image(&disk, &source, layout(), &sparse_runs()).unwrap();
        assert_eq!(disk.state_byte(), SnapshotState::Committed as u8);

        // A capture on a CONSUMING image is not.
        let consuming = captured();
        consuming.patch(SNAPSHOT_BASE_LBA, 6, &[SnapshotState::Consuming as u8]);
        consuming.fixup_crc();
        assert_eq!(
            capture_image(&consuming, &source, layout(), &sparse_runs()),
            Err(SnapshotError::StateConflict)
        );
        assert_eq!(consuming.state_byte(), SnapshotState::Consuming as u8);
    }

    // ── torn writes ──────────────────────────────────────────────────────────

    #[test]
    fn snapshot_torn_write_at_any_ordinal_never_replays_torn_bytes() {
        let source = sparse_ram();
        let runs = sparse_runs();
        // Header + inventory + payload + committed header.
        let total_writes =
            2 + runs.iter().map(|r| r.frame_count as u64).sum::<u64>() * SECTORS_PER_FRAME as u64;
        assert_eq!(total_writes, 2 + SPARSE.len() as u64 * 8);

        let mut resumed = 0;
        for prefix in [256usize, 384, 448] {
            for ordinal in 1..=total_writes {
                let disk = FakeDisk::with_fault(Fault::Torn { ordinal, prefix });
                let _ = capture_image(&disk, &source, layout(), &runs);
                disk.power_cycle();
                disk.fault.set(Fault::None);
                let target = FakeRam::default();
                let outcome = restore_image(&disk, &target, layout());
                match outcome {
                    RestoreOutcome::Resumed => {
                        // A torn sector may only be replayed when the durable
                        // image is byte-identical in every checksummed byte.
                        assert_eq!(
                            target.map(),
                            source.map(),
                            "ordinal {ordinal} prefix {prefix}: resumed with wrong bytes"
                        );
                        resumed += 1;
                    }
                    RestoreOutcome::ColdBoot(_) => {
                        assert_eq!(
                            target.write_count(),
                            0,
                            "ordinal {ordinal} prefix {prefix}: RAM replayed after refusal"
                        );
                    }
                    RestoreOutcome::FatalMixedRam(r) => {
                        panic!("ordinal {ordinal} prefix {prefix}: unexpected {r}")
                    }
                }
            }
        }
        // Payload tears must always be refused: prove it explicitly per ordinal.
        for ordinal in 3..total_writes {
            let disk = FakeDisk::with_fault(Fault::Torn {
                ordinal,
                prefix: 256,
            });
            let _ = capture_image(&disk, &source, layout(), &runs);
            disk.power_cycle();
            disk.fault.set(Fault::None);
            let target = FakeRam::default();
            assert!(
                matches!(
                    restore_image(&disk, &target, layout()),
                    RestoreOutcome::ColdBoot(_)
                ),
                "torn payload write #{ordinal} must be refused"
            );
        }
        // The only ordinals that may resume are the header tail (reserved,
        // uncovered bytes) — bounded well below the payload count.
        assert!(resumed < 12, "too many resumed torn images: {resumed}");
    }

    #[test]
    fn snapshot_flush_failure_leaves_no_replayable_image() {
        for flush in 1..=3u64 {
            let disk = FakeDisk::with_fault(Fault::Flush(flush));
            let source = sparse_ram();
            assert_eq!(
                capture_image(&disk, &source, layout(), &sparse_runs()),
                Err(SnapshotError::DeviceFlush)
            );
            // Failures after the payload flush may leave volatile writes, which
            // a device reset would drop; keep the durability model honest.
            disk.power_cycle();
            disk.fault.set(Fault::None);
            let target = FakeRam::default();
            assert!(
                matches!(
                    restore_image(&disk, &target, layout()),
                    RestoreOutcome::ColdBoot(_)
                ),
                "flush #{flush} failing must not leave a replayable image"
            );
            assert_eq!(target.write_count(), 0);
        }
    }

    #[test]
    fn snapshot_write_failure_at_any_ordinal_never_leaves_a_replayable_image() {
        let source = sparse_ram();
        let runs = sparse_runs();
        let total_writes =
            2 + runs.iter().map(|r| r.frame_count as u64).sum::<u64>() * SECTORS_PER_FRAME as u64;
        for ordinal in 1..=total_writes {
            let disk = FakeDisk::with_fault(Fault::Write(ordinal));
            assert_eq!(
                capture_image(&disk, &source, layout(), &runs),
                Err(SnapshotError::DeviceWrite),
                "write #{ordinal} was supposed to fail"
            );
            disk.power_cycle();
            disk.fault.set(Fault::None);
            assert_ne!(
                disk.header().state,
                SnapshotState::Committed as u8,
                "write #{ordinal} failing must not leave a COMMITTED header"
            );
            let target = FakeRam::default();
            assert!(
                matches!(
                    restore_image(&disk, &target, layout()),
                    RestoreOutcome::ColdBoot(_)
                ),
                "write #{ordinal} failing must not leave a replayable image"
            );
            assert_eq!(target.write_count(), 0);
        }
    }

    #[test]
    fn snapshot_failing_capture_destroys_the_previous_image_first() {
        let disk = captured();
        assert_eq!(disk.state_byte(), SnapshotState::Committed as u8);
        // New capture fails after the invalidation flush: the old image must be
        // gone (ordering: invalidate + flush before any payload write).
        disk.fault.set(Fault::Flush(2));
        let source = sparse_ram();
        assert!(capture_image(&disk, &source, layout(), &sparse_runs()).is_err());
        disk.power_cycle();
        disk.fault.set(Fault::None);
        let target = FakeRam::default();
        assert!(matches!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot(_)
        ));
        assert_eq!(target.write_count(), 0);
    }

    // ── stale / corrupt headers ──────────────────────────────────────────────

    #[test]
    fn snapshot_stale_and_torn_headers_are_refused() {
        struct Case {
            name: &'static str,
            patch: fn(&FakeDisk),
            invalidated: bool,
        }
        let cases = [
            Case {
                name: "v1 header",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 4, &1u16.to_le_bytes()),
                invalidated: true,
            },
            Case {
                name: "unknown state byte",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 6, &[9]),
                invalidated: true,
            },
            Case {
                name: "WRITING state",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 6, &[SnapshotState::Writing as u8]),
                invalidated: true,
            },
            Case {
                name: "EMPTY state",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 6, &[SnapshotState::Empty as u8]),
                invalidated: false,
            },
            Case {
                name: "mangled magic",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 0, &[0xDE, 0xAD, 0xBE, 0xEF]),
                invalidated: false,
            },
            Case {
                name: "torn header tail (reserved bytes)",
                patch: |d| {
                    for i in 68..SECTOR_SIZE {
                        d.patch(SNAPSHOT_BASE_LBA, i, &[0xFF]);
                    }
                },
                invalidated: true,
            },
            Case {
                name: "foreign kernel hash",
                patch: |d| {
                    d.patch(
                        SNAPSHOT_BASE_LBA,
                        8,
                        &0x1234_5678_9ABC_DEF0u64.to_le_bytes(),
                    )
                },
                invalidated: true,
            },
            Case {
                name: "sector size mismatch in header",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 32, &4096u32.to_le_bytes()),
                invalidated: false,
            },
            Case {
                name: "RAM base moved",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 16, &(BASE + 4096).to_le_bytes()),
                invalidated: false,
            },
            Case {
                name: "non-canonical payload offset",
                patch: |d| {
                    d.patch(
                        SNAPSHOT_BASE_LBA,
                        56,
                        &(INVENTORY_FIRST_LBA + 7).to_le_bytes(),
                    )
                },
                invalidated: true,
            },
            Case {
                name: "inventory geometry lies",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 44, &9u32.to_le_bytes()),
                invalidated: true,
            },
            Case {
                name: "image geometry lies",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 48, &99_999u32.to_le_bytes()),
                invalidated: true,
            },
            Case {
                name: "empty run count",
                patch: |d| d.patch(SNAPSHOT_BASE_LBA, 36, &0u32.to_le_bytes()),
                invalidated: true,
            },
            Case {
                name: "zero frame count",
                patch: |d| {
                    d.patch(SNAPSHOT_BASE_LBA, 40, &0u32.to_le_bytes());
                    d.patch(SNAPSHOT_BASE_LBA, 44, &0u32.to_le_bytes());
                    d.patch(SNAPSHOT_BASE_LBA, 48, &1u32.to_le_bytes());
                    d.patch(SNAPSHOT_BASE_LBA, 56, &INVENTORY_FIRST_LBA.to_le_bytes());
                    d.patch(SNAPSHOT_BASE_LBA, 36, &1u32.to_le_bytes());
                },
                invalidated: true,
            },
        ];
        for case in cases {
            let disk = captured();
            // No checksum fixup here: these are real corruption cases, so the
            // CRC must still describe the writer's intended bytes.
            (case.patch)(&disk);
            let before = disk.state_byte();
            let target = FakeRam::default();
            let outcome = restore_image(&disk, &target, layout());
            assert!(
                matches!(outcome, RestoreOutcome::ColdBoot(_)),
                "{} must cold boot, got {outcome:?}",
                case.name
            );
            assert_eq!(target.write_count(), 0, "{} replayed RAM", case.name);
            if case.invalidated {
                assert_eq!(disk.state_byte(), 0, "{} must be invalidated", case.name);
            } else {
                // Identity / layout mismatches must not write to a region whose
                // layout we do not trust.
                assert_eq!(
                    disk.state_byte(),
                    before,
                    "{} must not touch the region",
                    case.name
                );
            }
        }
    }

    #[test]
    fn snapshot_checksum_mismatch_is_refused_without_touching_ram() {
        // (where, lba, offset)
        let corruptions = [
            ("payload byte", INVENTORY_FIRST_LBA + 1, 0),
            (
                "payload byte, last payload sector",
                INVENTORY_FIRST_LBA + 1 + 47,
                511,
            ),
            ("inventory padding byte", INVENTORY_FIRST_LBA, 100),
            ("committed header flags", SNAPSHOT_BASE_LBA, 7),
            ("committed header crc", SNAPSHOT_BASE_LBA, CRC_FIELD_OFFSET),
            ("committed header reserved", SNAPSHOT_BASE_LBA, 400),
        ];
        for (name, lba, offset) in corruptions {
            let disk = captured();
            let old = disk.durable_sector(lba)[offset];
            disk.patch(lba, offset, &[old ^ 0x5A]);
            let target = FakeRam::default();
            let outcome = restore_image(&disk, &target, layout());
            assert!(
                matches!(outcome, RestoreOutcome::ColdBoot(_)),
                "{name}: must cold boot, got {outcome:?}"
            );
            assert_eq!(target.write_count(), 0, "{name}: RAM was touched");
            assert_eq!(disk.state_byte(), 0, "{name}: image must be invalidated");
        }
    }

    #[test]
    fn snapshot_wrong_kernel_identity_is_refused() {
        let disk = captured();
        disk.patch(
            SNAPSHOT_BASE_LBA,
            8,
            &kernel_hash().wrapping_add(1).to_le_bytes(),
        );
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("kernel identity changed")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(disk.state_byte(), 0);
    }

    // ── capacity / identity preflight ────────────────────────────────────────

    #[test]
    fn snapshot_wrong_capacity_is_refused() {
        // Capture on a device smaller than the reserved partition.
        let small = FakeDisk::new().with_capacity(SNAPSHOT_END_LBA - 1);
        let source = sparse_ram();
        assert_eq!(
            capture_image(&small, &source, layout(), &sparse_runs()),
            Err(SnapshotError::DeviceTooSmall)
        );
        assert_eq!(small.writes(), 0, "no write may precede the capacity check");

        // Capture on a device whose sector size is not 512.
        let wide = FakeDisk::new().with_sector_size(4096);
        assert_eq!(
            capture_image(&wide, &source, layout(), &sparse_runs()),
            Err(SnapshotError::UnsupportedSectorSize)
        );
        assert_eq!(wide.writes(), 0);

        // Restore on a device shrunk below the reserved partition: a valid image
        // present there is refused without touching the region (we must not
        // invalidate an image on a device whose layout we cannot confirm).
        let source_disk = captured();
        let image_sectors = source_disk.header().image_sectors as u64;
        let shrunk = FakeDisk::new().with_capacity(SNAPSHOT_END_LBA - 1);
        for lba in SNAPSHOT_BASE_LBA..SNAPSHOT_BASE_LBA + image_sectors {
            shrunk.patch(lba, 0, &source_disk.durable_sector(lba));
        }
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&shrunk, &target, layout()),
            RestoreOutcome::ColdBoot("device smaller than the snapshot partition")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(shrunk.writes(), 0, "no write to an unconfirmed layout");
        assert_eq!(shrunk.state_byte(), SnapshotState::Committed as u8);

        // A header claiming an image larger than the partition is refused
        // before any inventory allocation.
        let disk4 = captured();
        disk4.patch(SNAPSHOT_BASE_LBA, 40, &u32::MAX.to_le_bytes());
        disk4.patch(SNAPSHOT_BASE_LBA, 48, &u32::MAX.to_le_bytes());
        disk4.fixup_crc();
        let target4 = FakeRam::default();
        let outcome4 = restore_image(&disk4, &target4, layout());
        assert!(
            matches!(outcome4, RestoreOutcome::ColdBoot(_)),
            "oversized image must be refused, got {outcome4:?}"
        );
        assert_eq!(target4.write_count(), 0);
        assert_eq!(
            disk4.state_byte(),
            0,
            "a header claiming an impossible image is corruption: invalidate"
        );

        // A run count that would demand a gigantic inventory is refused before
        // the inventory buffer is allocated (`run_count > frame_count`).
        let disk5 = captured();
        disk5.patch(SNAPSHOT_BASE_LBA, 36, &u32::MAX.to_le_bytes());
        disk5.patch(SNAPSHOT_BASE_LBA, 44, &0x0800_0000u32.to_le_bytes());
        disk5.fixup_crc();
        let target5 = FakeRam::default();
        assert_eq!(
            restore_image(&disk5, &target5, layout()),
            RestoreOutcome::ColdBoot("bad inventory geometry")
        );
        assert_eq!(target5.write_count(), 0);
        assert_eq!(disk5.state_byte(), 0);
    }

    #[test]
    fn snapshot_capture_capacity_bound_refuses_oversized_inventory() {
        let source = sparse_ram();
        // One run of frames far beyond the P3 capacity (240_000 sectors = 30_000
        // frames), inside a huge RAM window.
        let big = RamLayout {
            base: BASE,
            end: BASE + 40_000 * FRAME_SIZE as u64,
        };
        let runs = runs_from_frames(&[BASE], big).unwrap();
        let mut runs = runs;
        runs[0].frame_count = 31_000;
        let disk = FakeDisk::new();
        assert_eq!(
            capture_image(&disk, &source, big, &runs),
            Err(SnapshotError::CapacityExceeded)
        );
        assert_eq!(disk.writes(), 0, "capacity is checked before any write");
    }

    // ── inventory corruption ─────────────────────────────────────────────────

    #[test]
    fn snapshot_duplicate_or_overlapping_runs_are_refused() {
        // Each case rewrites the (single-sector) inventory and re-fixes the CRC
        // so that structure, not the checksum, is what rejects it.
        struct Case {
            name: &'static str,
            overwrite: fn(&FakeDisk),
            frames: u32,
            runs: u32,
        }
        fn run(pa: u64, n: u32, flags: u32) -> [u8; RUN_BYTES] {
            let mut b = [0u8; RUN_BYTES];
            b[0..8].copy_from_slice(&pa.to_le_bytes());
            b[8..12].copy_from_slice(&n.to_le_bytes());
            b[12..16].copy_from_slice(&flags.to_le_bytes());
            b
        }
        let cases = [
            Case {
                name: "duplicate run",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, RUN_BYTES, &run(BASE, 2, 0));
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "overlapping runs",
                overwrite: |d| {
                    d.patch(
                        INVENTORY_FIRST_LBA,
                        RUN_BYTES,
                        &run(BASE + FRAME_SIZE as u64, 2, 0),
                    );
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "descending runs",
                overwrite: |d| {
                    d.patch(
                        INVENTORY_FIRST_LBA,
                        0,
                        &run(BASE + 40 * FRAME_SIZE as u64, 1, 0),
                    );
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "misaligned PA",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 0, &run(BASE + 7, 1, 0));
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "run past RAM end",
                overwrite: |d| {
                    // Run 3 starts exactly at RAM end.
                    d.patch(
                        INVENTORY_FIRST_LBA,
                        3 * RUN_BYTES,
                        &run(BASE + FRAMES * FRAME_SIZE as u64, 1, 0),
                    );
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "run below RAM base",
                overwrite: |d| {
                    d.patch(
                        INVENTORY_FIRST_LBA,
                        3 * RUN_BYTES,
                        &run(BASE - FRAME_SIZE as u64, 1, 0),
                    );
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "zero-length run",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 8, &0u32.to_le_bytes());
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "flagged run",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 12, &1u32.to_le_bytes());
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "frame total mismatch",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 8, &3u32.to_le_bytes());
                },
                frames: 6,
                runs: 4,
            },
            Case {
                name: "run count mismatch",
                overwrite: |d| {
                    // Header says 3 runs, the payload covers 4.
                    d.patch(SNAPSHOT_BASE_LBA, 36, &3u32.to_le_bytes());
                },
                frames: 6,
                runs: 3,
            },
        ];
        for case in cases {
            let disk = captured();
            (case.overwrite)(&disk);
            disk.fixup_crc();
            let header = disk.header();
            assert_eq!(
                header.run_count, case.runs,
                "{}: fixture run count",
                case.name
            );
            assert_eq!(
                header.frame_count, case.frames,
                "{}: fixture frame count",
                case.name
            );
            let target = FakeRam::default();
            let outcome = restore_image(&disk, &target, layout());
            match outcome {
                RestoreOutcome::ColdBoot(reason) => {
                    assert_ne!(
                        reason, "canonical checksum mismatch",
                        "{} must fail structurally, not on the checksum",
                        case.name
                    );
                }
                other => panic!("{} must cold boot, got {other:?}", case.name),
            }
            assert_eq!(target.write_count(), 0, "{} replayed RAM", case.name);
            assert_eq!(disk.state_byte(), 0, "{} must be invalidated", case.name);
        }
    }

    // ── replay failure ───────────────────────────────────────────────────────

    #[test]
    fn snapshot_replay_read_failure_is_fatal_mixed_ram() {
        // A read failure during the checksum pass is a clean cold boot.
        let disk = captured();
        disk.fault.set(Fault::Read(3));
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("payload read failed before replay")
        );
        assert_eq!(target.write_count(), 0);
        // An I/O error is not proof of corruption, so the image is left intact
        // for the next attempt (only proven corruption is invalidated).
        assert_eq!(disk.state_byte(), SnapshotState::Committed as u8);
        assert_eq!(disk.reads(), 3, "refused before reading the payload");

        // A read failure after the CONSUMING marker and after replay began is
        // fatal: RAM is mixed.
        let disk = captured();
        let header = disk.header();
        let cold_reads = 1
            + header.inventory_sectors as u64
            + header.frame_count as u64 * SECTORS_PER_FRAME as u64;
        // Replay begins at read ordinal cold_reads + 1; fail on its 9th sector
        // (second frame of the payload).
        disk.fault.set(Fault::Read(cold_reads + 9));
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::FatalMixedRam("payload read failed after replay began")
        );
        assert!(
            target.write_count() > 0,
            "mixed RAM means replay already wrote frames"
        );
        assert_eq!(disk.state_byte(), SnapshotState::Consuming as u8);
        assert_ne!(target.map(), sparse_ram().map());
    }

    #[test]
    fn snapshot_frame_write_failure_is_fatal_mixed_ram() {
        let disk = captured();
        let target = FakeRam::default();
        target.fail_write_at.set(Some(2));
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::FatalMixedRam("frame write failed after replay began")
        );
        assert_eq!(disk.state_byte(), SnapshotState::Consuming as u8);
    }

    #[test]
    fn snapshot_capture_read_failure_leaves_no_committed_image() {
        let disk = FakeDisk::new();
        let source = sparse_ram();
        source.fail_read_at.set(Some(3));
        assert_eq!(
            capture_image(&disk, &source, layout(), &sparse_runs()),
            Err(SnapshotError::MemoryFault)
        );
        let target = FakeRam::default();
        assert!(matches!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot(_)
        ));
        assert_eq!(target.write_count(), 0);
    }

    // ── pure validation / gate ───────────────────────────────────────────────

    #[test]
    fn snapshot_header_validation_rejects_every_identity_field() {
        let h = valid_header();
        assert!(validate_header(&h));
        let mutate = |f: fn(&mut SnapshotHeader)| {
            let mut h = valid_header();
            f(&mut h);
            assert!(!validate_header(&h));
        };
        mutate(|h| h.magic = 0xDEAD_BEEF);
        mutate(|h| h.version = 1);
        mutate(|h| h.state = SnapshotState::Consuming as u8);
        mutate(|h| h.kernel_hash = h.kernel_hash.wrapping_add(1));
        mutate(|h| h.sector_size = 4096);
        mutate(|h| h.run_count = 0);
        mutate(|h| h.frame_count = 0);
    }

    #[test]
    fn snapshot_header_sector_round_trips() {
        let h = valid_header();
        let bytes = h.write_bytes();
        assert_eq!(
            SnapshotHeader::parse(&bytes).crc32,
            0xDEAD_BEEF,
            "header must survive a sector round trip"
        );
        assert_eq!(&bytes[0..4], b"VICU");
    }

    #[cfg(not(feature = "snapshot-qualified"))]
    #[test]
    fn snapshot_qualification_gate_stays_closed_without_the_feature() {
        assert!(!QUALIFICATION_ENABLED);
        assert_eq!(serialize_snapshot(), Err(SnapshotError::GateClosed));
        // Restore must refuse: the on-disk image is never even considered.
        assert!(!try_restore());
    }

    #[cfg(feature = "snapshot-qualified")]
    #[test]
    fn snapshot_qualification_gate_is_open_with_the_feature() {
        assert!(QUALIFICATION_ENABLED);
    }

    #[test]
    fn snapshot_empty_region_cold_boots() {
        let disk = FakeDisk::new();
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("no snapshot magic")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(disk.writes(), 0, "an empty region is never written");
    }
}
