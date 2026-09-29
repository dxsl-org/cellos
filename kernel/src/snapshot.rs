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
//! restore → resume has been proven on a block-capable board.  The capture
//! preflight refuses **without any block I/O** unless
//!
//! - the boot reserved a scratch workspace the capture can stage in, and that
//!   workspace is outside every planned run ([`ScratchRegion`],
//!   [`CaptureScratch`], [`assert_scratch_outside_runs`]) — the region is taken
//!   from the frame allocator by [`reserve_scratch_region_at_boot`] and a build
//!   without one refuses with [`SnapshotError::NoReservedScratch`];
//! - every online hart is parked at an acknowledged safe point
//!   ([`crate::task::quiesce`]) — a no-op on a single-hart system, and on RV64 a
//!   real request answered by each hart's trap path;
//! - the device has a trusted monotonic epoch source and the image can be
//!   authenticated ([`SnapshotDevice::current_epoch`], [`SNAPSHOT_TRUST_KEY`]).
//!
//! Every buffer the frozen window fills is carved from that workspace before the
//! park ([`FrozenScratch`]), and the window then *runs there*: its stack, its
//! sector buffers and the staging frame are all in the region, which is what
//! makes the staging check honest.  Inside the frozen window the buffers are
//! capacity-bounded ([`BoundedVec`], [`ScratchVec`]) and a shortage is refused
//! rather than allocated, because a parked hart may be holding the heap's
//! non-masking spin lock.  The hardware witness (a save → reset → restore →
//! resume on a board) remains the hardware-side half of phase 07; see the phase
//! doc.
//!
//! # Authenticated freshness
//!
//! The format is authenticated by a keyed MAC over the header, and the header
//! carries a monotonic `epoch`:
//!
//! - the tag is HMAC-SHA256 (RFC 2104, [`hmac_sha256`]) over
//!   [`SnapshotHeader::signed_bytes`] (only the tag's own field zeroed), built
//!   on the kernel's own [`crate::sha256`] — the same construction the workspace
//!   already carries in `libs/attestation/src/hkdf.rs:14`, no new crypto and no
//!   new dependency;
//! - through the header the tag binds the epoch, the identity, the geometry and
//!   the payload/inventory digest (the header's CRC, which the tag includes);
//!   a deliberate re-CRC of a tampered payload is then a MAC mismatch too, though
//!   a crafted CRC-32 collision would defeat that binding;
//! - capture writes `epoch = device.current_epoch() + 1`; a reader refuses an
//!   image whose epoch is **not strictly newer** than the value the device
//!   reports as current, and then durably advances the device epoch to the
//!   consumed image's, so a replay of a consumed (or older) image is refused
//!   even though its tag is genuine;
//! - a device with no monotonic source, or a build with no provisioned key,
//!   refuses capture and restore rather than assuming either.
//!
//! The key is a dev/test key under `dev-signing-key` and *absent* otherwise, so
//! a production build fails closed until a key is provisioned.  The device half
//! is real only for the in-memory fake: on hardware the monotonic source must be
//! an MMC/eMMC counter the host cannot roll back, which is not modelled here.
//!
//! # Capture staging and the frozen window
//!
//! The capture's own code, stack and buffers must not be inside the captured
//! runs, or it would save a span it is still writing (its own in-flight stack or
//! inventory buffer).  [`CaptureScratch`] declares those spans as physical
//! addresses; [`assert_scratch_outside_runs`] refuses the capture before any
//! block I/O when a declared span intersects a planned run.  The park hook makes
//! the complementary claim — nothing *else* can write a frame while it is read —
//! true for the rest of the machine.
//!
//! The declaration is not a claim, it is enforced.  The boot reserves one bounded
//! workspace ([`ScratchRegion`]) from the frame allocator
//! ([`reserve_scratch_region_at_boot`], called from the boot path once the
//! allocator exists), and:
//!
//! - its frames are **excluded** from the inventory ([`take_allocated_frame`]):
//!   they are the capture's workspace, so a restored image can never replay the
//!   capture's own stack or buffers over live scratch.  The exclusion is proven,
//!   not assumed — a workspace frame that reaches the plan anyway is refused by
//!   [`assert_scratch_outside_runs`];
//! - the frozen window's buffers and its stack are carved from it
//!   ([`FrozenScratch`], [`ScratchArena`]), and the pre-freeze bound is derived
//!   from the region's size rather than from the whole allocator, capped by the
//!   partition geometry;
//! - the window runs on the region's stack ([`run_on_stack`]) and refuses unless
//!   the stack pointer it observes is inside that region — the boot stack lives
//!   in the captured image span, so a window that ran on it would save its own
//!   frame;
//! - a missing workspace, one too small for the computed bound, or one
//!   overlapping a planned run all refuse with the existing error vocabulary
//!   ([`SnapshotError::NoReservedScratch`], [`SnapshotError::CapacityExceeded`],
//!   [`SnapshotError::ScratchOverlapsRun`]) before any park and before any block
//!   I/O.
//!
//! # Image-kind runs: what they close, and what they do not
//!
//! A survey of the allocator bitmap alone cannot describe a resumable image:
//! [`crate::memory::frame::FrameAllocator::new_from_map`] manages only
//! `MemoryType::Usable` ranges, so `MemoryType::Kernel` frames — the kernel's
//! own `.data`/`.bss`, which hold `SCHEDULER`, the allocator's bitmap and
//! metadata, page-table roots and hart-local state — are never in the
//! allocator's owned set and can never appear in an inventory built from
//! allocated frames alone.
//!
//! Those frames are added as a second, explicit run kind.  An inventory entry
//! with [`RUN_FLAG_IMAGE`] set covers trusted kernel-image frames; an entry with
//! flags `0` covers allocator-owned frames.  The mutable span is derived from
//! the linker (`__domain_writable_start` … `__domain_writable_end`) by
//! [`kernel_image`], and every image-kind run is validated against the *live*
//! trusted span ([`ImageRegion`]) on both sides of the format:
//!
//! - the writer derives the span, refuses when it is empty, misaligned or not
//!   inside the trusted image ([`image_runs`]), and merges it with the allocated
//!   runs without ever coalescing one kind into the other ([`merge_runs`]);
//! - an image-kind run that overlaps or duplicates an allocator-owned run is
//!   refused ([`SnapshotError::ImageRangeConflict`]) rather than silently
//!   merged: the allocator must not own image frames, and image frames must not
//!   be described as allocator-owned;
//! - the reader re-checks every image-kind run against *its own* trusted span
//!   ([`frames_in_runs`]), so an inventory naming image frames that this build
//!   does not own is refused before a byte of RAM is written.  The span is not
//!   stored in the header: [`kernel_hash`] pins the build, and the span is a
//!   function of that build.
//!
//! What this does **not** prove — stated, not claimed closed:
//!
//! - **Byte coverage is not semantic closure.**  The allocator's state is now
//!   byte-covered but not proven resumable.  The `FrameAllocator` static itself
//!   (range plan, counters, next-fit cursor, bitmap handle) sits in the writable
//!   image span, so its bytes are in the inventory; but by the time a capture or
//!   restore can run, the boot path has *already rebuilt* it with
//!   `FrameAllocator::new_from_map`, and its lock is live — replaying old bytes
//!   over that is a different thing from rebuilding it, and which fields are
//!   restored and which re-derived is not decided here.  The bitmap *words* live
//!   in the first frames of the largest managed range, i.e. in allocator-owned
//!   frames, so they are covered as an ordinary allocated run.  Byte coverage is
//!   the precondition for closure, not the closure.
//! - Pointer relinking, lock re-initialization and the exclusion of changing
//!   driver/MMC transport state are phase 07 steps 3 and 4.
//! - Capture staging is enforced as a *refusal* on a build without a workspace,
//!   and as a *layout* on one with it: the boot-reserved region is excluded from
//!   the inventory, the window's buffers and stack live in it, and the window
//!   refuses unless it observes its own stack pointer inside it.  The host lane
//!   drives the reservation against a synthetic allocator and the staged
//!   capture against the fake device; **no board has run either**, so the
//!   reservation's interaction with a real memory map, the live `enumerate`
//!   exclusion and the stack switch on RV64/AArch64 are unexecuted.
//! - The requester can still take an interrupt while the window runs: the park
//!   protocol covers the *other* harts, and nothing in the window masks
//!   interrupts, so a timer tick can still write kernel state (a trap stack in
//!   the image span) between two frame reads.  Step 3's drain is what closes
//!   that, not this slice.
//! - The format still cannot detect a byte that changed between the frame read
//!   and the block write; that is what the park hook and the staging check
//!   together are for, and neither is proven on a board.
//! - Freshness is modelled against a fake monotonic device.  Nothing here proves
//!   a real MMC/eMMC monotonic source exists, that it cannot be rolled back, or
//!   that a build has a provisioned key: those are the hardware/provisioning
//!   gates.  The MAC is over the header (payload bound through the CRC); a
//!   strong keyed digest over the payload bytes would need a streaming SHA-256,
//!   which the kernel's one-shot [`crate::sha256::sha256`] does not expose.
//! - The device exposes one monotonic value ("what a restore has consumed"), and
//!   capture takes `value + 1` without advancing it.  Two captures with no
//!   consume in between therefore carry the same epoch: the region holds one
//!   image, but an attacker who archived the first one could replay it before any
//!   consume and it would look fresh.  Closing that needs a second, durable
//!   "issued" watermark on the device; with a single value the strictly-newer
//!   rule the format requires cannot both admit a fresh capture and refuse it.
//! - On x86-64 the kernel is linked into the higher half and riscv32/aarch32/
//!   x86-32 do not delimit the writable span, so [`kernel_image`] returns `None`
//!   and every capture preflight refuses with
//!   [`SnapshotError::ImageRegionUnavailable`] — fail-closed, not a claim that
//!   those targets have no mutable image state.
//!
//! # Test surface
//!
//! The format, the state machine and the corruption matrix are exercised with
//! an in-memory fake sector device (volatile write-back cache + fault
//! injection) and a sparse frame map.  The quiescence preflight is exercised
//! with the fake hart set in `task::quiesce` (a clock that advances one tick per
//! poll, programmed acknowledgements, logged requests).  The frozen window, the
//! staging check and the freshness matrix are exercised through
//! [`capture_frozen_with_allocated`] (an explicit allocated set and image span,
//! since a host build has neither) and a fake device whose monotonic epoch
//! survives `power_cycle`.  The scratch workspace is exercised too: the
//! reservation runs against a synthetic allocator
//! ([`crate::memory::frame::allocator_for_tests`]) and the frozen window against
//! a host-lane buffer standing in for the reserved region (`host_scratch`), so
//! the exclusion, the derived bound, the stack switch and the three refusals are
//! all host-observable.  Every fake lives in `#[cfg(test)]` (host lane only) and
//! is never linked into a kernel image.

use crate::memory::frame::{phys_to_virt, FrameAllocator, FRAME_ALLOCATOR};
use crate::sync::Spinlock;
use crate::task::drivers::block;
use crate::task::quiesce;
use alloc::vec::Vec;
use core::fmt;
use core::mem::MaybeUninit;
#[cfg(any(feature = "test-hooks", test))]
use core::sync::atomic::{AtomicU64, Ordering};

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

/// Bytes of the header's keyed-MAC tag (HMAC-SHA256).
pub const AUTH_BYTES: usize = 32;

/// Byte offset of the `auth` field inside the header sector.  Like the CRC
/// field, it is zeroed by [`SnapshotHeader::canonical_bytes`]: the tag is
/// computed over the header that carries it.
pub const AUTH_FIELD_OFFSET: usize = 80;

/// Byte offset of the authenticated `epoch` field inside the header sector.
pub const EPOCH_FIELD_OFFSET: usize = 72;

/// Maximum number of inventory runs the kernel-image half can contribute.  The
/// image span is contiguous, so it is one run; the bound is a capacity
/// reservation, so it is stated explicitly rather than derived.
pub const IMAGE_RUNS_MAX: usize = 1;

/// First LBA of the address inventory (immediately after the header).
pub const INVENTORY_FIRST_LBA: u64 = SNAPSHOT_BASE_LBA + 1;

/// One inventory entry: a run of contiguous frames at an explicit start PA.
pub const RUN_BYTES: usize = 16;

/// Inventory entry flag: this run covers trusted kernel-image frames
/// (the mutable `.data`/`.bss`/stack/GOT span), not allocator-owned frames.
/// Flags `0` means "allocator-owned"; every other bit is reserved and refused.
pub const RUN_FLAG_IMAGE: u32 = 0b1;

/// Qualification gate for warm snapshot capture and restore.
///
/// Phase-01 containment: the writer hashed payload bytes only while the reader
/// hashed header + payload, the reader reconstructed a dense
/// `pa_base + index * 4096` run from a write that skips free frames, and the
/// restore replays frames over its own live stack and kernel globals. The
/// format, inventory, checksum, states, authenticated epoch and staging checks
/// are specified and unit-tested (phase 07 steps 1, 3 and the device-independent
/// half of step 5), and the all-hart park hook now exists on RV64. What still
/// keeps this gate closed is the hardware side: no save → reset → restore →
/// resume has been proven on a block-capable board, no target has a reserved
/// scratch region outside the captured runs (so the staging check refuses the
/// shipping capture), a real device has no modelled monotonic epoch source, and
/// closure completeness is not proven. Until then every shipping image keeps the
/// path disabled: a capture cannot touch the snapshot region and a restore
/// cannot mutate RAM. `snapshot-qualified` is the single build gate phase 07
/// turns on for that verified profile.
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
    /// Not every online hart could be parked at an acknowledged safe point, so
    /// the memory image cannot be frozen. Nothing was written.
    HartsNotQuiesced,
    /// No reserved capture storage: the capture cannot name its own code, stack
    /// and buffers as physical spans, so it cannot prove it is not about to
    /// save its own in-flight stack or buffer.
    NoReservedScratch,
    /// A reserved capture-scratch span intersects a planned run: the capture
    /// would save a buffer (or stack) that it is still writing.
    ScratchOverlapsRun,
    /// No provisioned keyed-MAC key, so the header's epoch cannot be
    /// authenticated. Refused rather than assumed.
    NoTrustKey,
    /// The device has no trusted monotonic epoch source, so an image's freshness
    /// cannot be checked. Refused rather than assumed.
    NoFreshnessSource,
    /// The image's authenticated epoch is not strictly newer than the epoch the
    /// device reports: a replay of an image this device has already consumed
    /// (or an older one).
    StaleEpoch,
    /// The header's keyed MAC does not verify: the header (epoch included) was
    /// tampered with, or was written by a different trust key.
    AuthenticationFailed,
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
    /// The mutable kernel-image span could not be derived or represented as
    /// frame runs (not delimited on this target, empty, misaligned).
    ImageRegionUnavailable,
    /// An image-kind run lies outside this boot's trusted kernel-image span.
    ImageRangeOutsideImage,
    /// An image-kind run overlaps or duplicates an allocator-owned run.
    ImageRangeConflict,
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
            Self::HartsNotQuiesced => "online harts are not parked at an acknowledged safe point",
            Self::NoReservedScratch => "no reserved capture storage outside the captured runs",
            Self::ScratchOverlapsRun => "capture scratch intersects a planned run",
            Self::NoTrustKey => "no provisioned key for the authenticated epoch",
            Self::NoFreshnessSource => "the device has no trusted monotonic epoch source",
            Self::StaleEpoch => "image epoch is not newer than the device epoch (replay)",
            Self::AuthenticationFailed => "header keyed MAC does not verify",
            Self::UnsupportedSectorSize => "unsupported block sector size",
            Self::DeviceTooSmall => "block device smaller than the snapshot partition",
            Self::CapacityExceeded => "snapshot image exceeds the reserved P3 partition",
            Self::DeviceRead => "block read failed",
            Self::DeviceWrite => "block write failed",
            Self::DeviceFlush => "block flush failed",
            Self::NoRuns => "no allocated frames to snapshot",
            Self::ImageRegionUnavailable => {
                "the mutable kernel-image span cannot be represented as frame runs"
            }
            Self::ImageRangeOutsideImage => "image-kind run outside the trusted kernel-image span",
            Self::ImageRangeConflict => "image-kind run overlaps or duplicates an allocated run",
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
    /// Authenticated monotonic epoch: the value of the device's trusted
    /// monotonic source at capture time, plus one.  Covered by `auth`, so it
    /// cannot be raised by rewriting the sector.
    pub epoch: u64,
    /// Keyed MAC (HMAC-SHA256) over [`SnapshotHeader::canonical_bytes`].  The
    /// writer computes it after the CRC is final; a reader refuses the image
    /// unless it verifies.
    pub auth: [u8; AUTH_BYTES],
    /// Reserved padding to fill the header sector (zero).
    pub _reserved: [u8; 400],
}

// Compile-time layout guarantee — the header is exactly one sector, and the
// CRC/auth fields sit at the offsets the canonical checksum zeroes.
const _: () = assert!(core::mem::size_of::<SnapshotHeader>() == HEADER_BYTES);
const _: () = assert!(core::mem::offset_of!(SnapshotHeader, crc32) == CRC_FIELD_OFFSET);
const _: () = assert!(core::mem::offset_of!(SnapshotHeader, auth) == AUTH_FIELD_OFFSET);
const _: () = assert!(core::mem::offset_of!(SnapshotHeader, epoch) == EPOCH_FIELD_OFFSET);

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

    /// Bytes as hashed by the canonical checksum (CRC and MAC fields zeroed).
    pub fn canonical_bytes(&self) -> [u8; HEADER_BYTES] {
        let mut out = self.write_bytes();
        out[CRC_FIELD_OFFSET..CRC_FIELD_OFFSET + 4].copy_from_slice(&[0u8; 4]);
        out[AUTH_FIELD_OFFSET..AUTH_FIELD_OFFSET + AUTH_BYTES].copy_from_slice(&[0u8; AUTH_BYTES]);
        out
    }

    /// Bytes as signed by the keyed MAC: only the MAC's own field is zeroed.
    ///
    /// The CRC stays in the input, so the tag binds the payload and inventory
    /// digest the CRC covers as well as the epoch, identity and geometry.
    pub fn signed_bytes(&self) -> [u8; HEADER_BYTES] {
        let mut out = self.write_bytes();
        out[AUTH_FIELD_OFFSET..AUTH_FIELD_OFFSET + AUTH_BYTES].copy_from_slice(&[0u8; AUTH_BYTES]);
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
    /// Trusted kernel-image span of this boot.  An image-kind run must lie
    /// inside it, and this is the only bound image runs are checked against:
    /// the kernel image is `MemoryType::Kernel`, so the allocator's managed
    /// window may not contain it.  [`ImageRegion::EMPTY`] means this build
    /// cannot derive the span, and any image-kind run is then refused.
    ///
    /// Not part of the on-disk identity: [`kernel_hash`] pins the build, and the
    /// span is a function of that build.
    pub image: ImageRegion,
}

/// A physical, 4096-aligned span of the kernel image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageRegion {
    /// Physical start (inclusive).
    pub base: u64,
    /// Physical end (exclusive).
    pub end: u64,
}

impl ImageRegion {
    /// The "no span" sentinel.  A real kernel image is never empty.
    pub const EMPTY: Self = Self { base: 0, end: 0 };

    /// No address is covered.
    pub const fn is_empty(&self) -> bool {
        self.base >= self.end
    }

    /// Is `[base, end)` entirely inside this span?
    pub const fn contains_span(&self, base: u64, end: u64) -> bool {
        base >= self.base && end <= self.end
    }

    /// Is every frame of `run` inside this span?
    pub fn contains_run(&self, run: &SnapshotRun) -> bool {
        self.contains_span(run.pa, run.end_pa())
    }
}

/// The kernel image of the running build, as physical spans.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KernelImage {
    /// The whole image (`.text` … writable end).  Image-kind runs must lie here.
    pub trusted: ImageRegion,
    /// The mutable part: `.data`, `.bss`, the kernel stack and the GOT.  This is
    /// what [`crate::memory::frame::FrameAllocator`] cannot describe, because
    /// `new_from_map` excludes `MemoryType::Kernel`.
    pub mutable: ImageRegion,
}

/// Physical image spans of this build, from the linker script.
///
/// riscv64 (0x8020_0000), aarch64 QEMU-virt (0x4008_0000) and RPi3 (0x8_0000)
/// link the kernel at the address it is loaded at, so `__domain_text_start` /
/// `__domain_writable_start` / `__domain_writable_end` are physical addresses.
///
/// `None` where that is not true (x86-64 links into the higher half) or where
/// the linker script does not delimit the writable span (riscv32, aarch32,
/// x86-32): the capture preflight then refuses rather than guess.
#[cfg(any(target_arch = "riscv64", target_arch = "aarch64"))]
pub fn kernel_image() -> Option<KernelImage> {
    unsafe extern "C" {
        static __domain_text_start: u8;
        static __domain_writable_start: u8;
        static __domain_writable_end: u8;
    }
    // `addr_of!` takes the linker-assigned address without reading the object.
    let mutable = ImageRegion {
        base: core::ptr::addr_of!(__domain_writable_start) as u64,
        end: core::ptr::addr_of!(__domain_writable_end) as u64,
    };
    let trusted = ImageRegion {
        base: core::ptr::addr_of!(__domain_text_start) as u64,
        end: mutable.end,
    };
    if trusted.is_empty() || mutable.is_empty() || !trusted.contains_span(mutable.base, mutable.end)
    {
        return None;
    }
    Some(KernelImage { trusted, mutable })
}

/// See the riscv64/aarch64 definition: no physical image span can be derived
/// from the linker here, so capture refuses instead of describing the wrong
/// frames.
#[cfg(not(any(target_arch = "riscv64", target_arch = "aarch64")))]
pub fn kernel_image() -> Option<KernelImage> {
    None
}

/// Sectors needed to hold `run_count` inventory entries.
const fn inventory_sectors(run_count: u64) -> u64 {
    let bytes = run_count * RUN_BYTES as u64;
    (bytes + SECTOR_SIZE as u64 - 1) / SECTOR_SIZE as u64
}

/// The single canonical checksum definition, shared by writer and reader:
/// header (CRC field zeroed) first, then inventory sectors, then payload
/// sectors, in write order.  Callers then `update()` in exactly that order.
fn canonical_hasher(header: &SnapshotHeader) -> crc32fast::Hasher {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&header.canonical_bytes());
    hasher
}

// ── Authenticated epoch ──────────────────────────────────────────────────────

/// The keyed-MAC key that authenticates the header (and therefore the epoch).
///
/// The kernel has one hash primitive, [`crate::sha256::sha256`], and the
/// workspace's HMAC-SHA256 construction over it lives in
/// `libs/attestation/src/hkdf.rs:14` — written there precisely because the
/// kernel and its neighbours must not take a crypto dependency.  This mirrors
/// that RFC 2104 construction over the kernel's own SHA-256: a standard
/// construction, no new algorithm, no new crate.
///
/// Trust material is provisioned, not invented: with the dev posture
/// (`dev-signing-key`, the same switch [`crate::signing`] uses for the cell
/// trust anchor) a reproducible dev key is compiled in; without it there is no
/// key at all and every authenticated-freshness check refuses, so a production
/// build cannot silently fall back to an unauthenticated epoch.
#[cfg(feature = "dev-signing-key")]
const SNAPSHOT_TRUST_KEY: Option<[u8; 32]> = Some(*b"ViCell-snapshot-epoch-mac-key--1");

#[cfg(not(feature = "dev-signing-key"))]
const SNAPSHOT_TRUST_KEY: Option<[u8; 32]> = None;

const SHA256_BLOCK: usize = 64;
const SHA256_LEN: usize = 32;

// The tag is a SHA-256 output; the format stores exactly that many bytes.
const _: () = assert!(AUTH_BYTES == SHA256_LEN);

/// HMAC-SHA256 (RFC 2104) over `msg`, which must be one header sector — the
/// only thing ever authenticated.  Bounded stack buffers, no allocation, so
/// this is safe inside the frozen window.
fn hmac_sha256(key: &[u8], msg: &[u8], out: &mut [u8; SHA256_LEN]) {
    debug_assert!(msg.len() <= HEADER_BYTES);
    let mut key_block = [0u8; SHA256_BLOCK];
    if key.len() > SHA256_BLOCK {
        key_block[..SHA256_LEN].copy_from_slice(&crate::sha256::sha256(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; SHA256_BLOCK];
    let mut opad = [0x5cu8; SHA256_BLOCK];
    for i in 0..SHA256_BLOCK {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = [0u8; SHA256_BLOCK + HEADER_BYTES];
    inner[..SHA256_BLOCK].copy_from_slice(&ipad);
    inner[SHA256_BLOCK..SHA256_BLOCK + msg.len()].copy_from_slice(msg);
    let inner_hash = crate::sha256::sha256(&inner[..SHA256_BLOCK + msg.len()]);

    let mut outer = [0u8; SHA256_BLOCK + SHA256_LEN];
    outer[..SHA256_BLOCK].copy_from_slice(&opad);
    outer[SHA256_BLOCK..].copy_from_slice(&inner_hash);
    *out = crate::sha256::sha256(&outer);
}

/// The header's keyed MAC tag.  Covers [`SnapshotHeader::signed_bytes`]: the
/// CRC field is included, so the tag binds the payload and inventory the CRC
/// covers as well as the epoch, identity and geometry.  Rewriting the epoch
/// upward (the replay-forgery attack) cannot survive this.
fn header_mac(key: &[u8; 32], header: &SnapshotHeader) -> [u8; AUTH_BYTES] {
    let msg = header.signed_bytes();
    let mut tag = [0u8; AUTH_BYTES];
    hmac_sha256(key, &msg, &mut tag);
    tag
}

/// Constant-time equality for MAC tags (no early exit on the first difference).
fn mac_eq(a: &[u8; AUTH_BYTES], b: &[u8; AUTH_BYTES]) -> bool {
    let mut diff = 0u8;
    for i in 0..AUTH_BYTES {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Serialize the inventory into `out` (zero-padded sectors, little-endian per
/// field), never growing it past the capacity its caller reserved.
fn encode_inventory_into(
    runs: &[SnapshotRun],
    out: &mut impl ZeroedSink,
) -> Result<(), SnapshotError> {
    let sectors = inventory_sectors(runs.len() as u64) as usize;
    let needed = sectors * SECTOR_SIZE;
    out.sink_resize_zeroed(needed)?;
    let out = out.sink_slice_mut();
    for (i, run) in runs.iter().enumerate() {
        let o = i * RUN_BYTES;
        out[o..o + 8].copy_from_slice(&run.pa.to_le_bytes());
        out[o + 8..o + 12].copy_from_slice(&run.frame_count.to_le_bytes());
        out[o + 12..o + 16].copy_from_slice(&run.flags.to_le_bytes());
    }
    Ok(())
}

/// Allocation-owning wrapper: reserve exactly what the inventory needs.
fn encode_inventory(runs: &[SnapshotRun]) -> Vec<u8> {
    let sectors = inventory_sectors(runs.len() as u64) as usize;
    let mut out = BoundedVec::reserved(sectors * SECTOR_SIZE);
    encode_inventory_into(runs, &mut out).expect("reserved exactly");
    out.into_vec()
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

/// Group explicit frame addresses into an address inventory, writing into
/// `out` without ever growing it past its reserved capacity.
///
/// Input must be ascending and strictly unique (as the allocator enumerates
/// allocated frames); contiguous addresses are merged into one run.  A gap in
/// the input starts a new run — the inventory covers **exactly** the supplied
/// addresses and nothing else.
fn runs_from_frames_into(
    pas: &[u64],
    layout: RamLayout,
    out: &mut impl BoundedSink<SnapshotRun>,
) -> Result<(), SnapshotError> {
    out.sink_clear();
    if pas.is_empty() {
        return Err(SnapshotError::NoRuns);
    }
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
            let last = out.sink_slice_mut().last_mut().expect("non-empty");
            if p + FRAME_SIZE as u64 == pa {
                last.frame_count = last
                    .frame_count
                    .checked_add(1)
                    .ok_or(SnapshotError::BadRun)?;
            } else {
                out.sink_push(SnapshotRun {
                    pa,
                    frame_count: 1,
                    flags: 0,
                })?;
            }
        } else {
            out.sink_push(SnapshotRun {
                pa,
                frame_count: 1,
                flags: 0,
            })?;
        }
        prev = Some(pa);
    }
    Ok(())
}

/// Allocation-owning wrapper: reserve one run per input address (the worst case).
pub fn runs_from_frames(pas: &[u64], layout: RamLayout) -> Result<Vec<SnapshotRun>, SnapshotError> {
    let mut out = BoundedVec::reserved(pas.len());
    runs_from_frames_into(pas, layout, &mut out)?;
    Ok(out.into_vec())
}

/// Structural validation shared by writer and reader: every run is non-empty,
/// aligned, ascending, non-overlapping, carries a known run kind, and lies
/// inside the bound that kind is accountable to — allocator-owned runs inside
/// the managed RAM window, image-kind runs inside the trusted kernel-image span.
/// Returns the total frame count.
pub fn frames_in_runs(runs: &[SnapshotRun], layout: RamLayout) -> Result<u32, SnapshotError> {
    if runs.is_empty() {
        return Err(SnapshotError::NoRuns);
    }
    let mut total: u64 = 0;
    let mut prev: Option<SnapshotRun> = None;
    for run in runs {
        if run.flags & !RUN_FLAG_IMAGE != 0 || run.frame_count == 0 {
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
        if run.flags & RUN_FLAG_IMAGE != 0 {
            // Image frames are `MemoryType::Kernel`: the allocator's managed
            // window does not have to contain them, so the trusted linker-
            // delimited image span is the bound — and it is checked against the
            // *live* span, so an inventory from another image is refused here.
            if !layout.image.contains_span(run.pa, end) {
                return Err(SnapshotError::ImageRangeOutsideImage);
            }
        } else if run.pa < layout.base || end > layout.end {
            return Err(SnapshotError::BadRun);
        }
        if let Some(p) = prev {
            // `<` rejects overlap, duplicates and descending order; adjacency
            // (`==`) is harmless and is what `runs_from_frames` merges anyway.
            if run.pa < p.end_pa() {
                return Err(if run.flags != p.flags {
                    SnapshotError::ImageRangeConflict
                } else {
                    SnapshotError::BadRun
                });
            }
        }
        prev = Some(*run);
        total += run.frame_count as u64;
        if total > u32::MAX as u64 {
            return Err(SnapshotError::TooManyRuns);
        }
    }
    Ok(total as u32)
}

/// The image-kind inventory for a mutable image span, written into `out`
/// without growing it past its reserved capacity.
///
/// The kernel image is contiguous, so this is one run.  Refuses a span that
/// cannot be represented as frame runs (empty, not 4096-aligned, more frames
/// than the inventory can address) with [`SnapshotError::ImageRegionUnavailable`],
/// and a span outside this boot's trusted image with
/// [`SnapshotError::ImageRangeOutsideImage`].
fn image_runs_into(
    mutable: ImageRegion,
    layout: RamLayout,
    out: &mut impl BoundedSink<SnapshotRun>,
) -> Result<(), SnapshotError> {
    out.sink_clear();
    if mutable.is_empty()
        || mutable.base % FRAME_SIZE as u64 != 0
        || mutable.end % FRAME_SIZE as u64 != 0
    {
        return Err(SnapshotError::ImageRegionUnavailable);
    }
    if !layout.image.contains_span(mutable.base, mutable.end) {
        return Err(SnapshotError::ImageRangeOutsideImage);
    }
    let frames = (mutable.end - mutable.base) / FRAME_SIZE as u64;
    if frames > u32::MAX as u64 {
        return Err(SnapshotError::ImageRegionUnavailable);
    }
    out.sink_push(SnapshotRun {
        pa: mutable.base,
        frame_count: frames as u32,
        flags: RUN_FLAG_IMAGE,
    })
}

/// Allocation-owning wrapper: reserve the one run the contiguous span yields.
pub fn image_runs(
    mutable: ImageRegion,
    layout: RamLayout,
) -> Result<Vec<SnapshotRun>, SnapshotError> {
    let mut out = BoundedVec::reserved(IMAGE_RUNS_MAX);
    image_runs_into(mutable, layout, &mut out)?;
    Ok(out.into_vec())
}

/// Merge image-kind runs into an already-built allocator-owned inventory,
/// in place, preserving each run's kind.
///
/// `allocated` must already be a valid allocator-owned inventory and must have
/// room reserved for `image.len()` more entries.  An image run that overlaps or
/// duplicates an allocator-owned run is refused with
/// [`SnapshotError::ImageRangeConflict`] — never silently merged: the allocator
/// must not own image frames, and image frames must not be described as
/// allocator-owned.  Adjacent runs of different kinds are kept separate; they
/// are never coalesced, so the kind survives the merge.
fn merge_runs_into(
    allocated: &mut impl BoundedSink<SnapshotRun>,
    image: &[SnapshotRun],
    layout: RamLayout,
) -> Result<(), SnapshotError> {
    if !allocated.sink_slice().is_empty() {
        frames_in_runs(allocated.sink_slice(), layout)?;
    }
    if !image.is_empty() {
        frames_in_runs(image, layout)?;
    }
    for run in image {
        allocated.sink_push(*run)?;
    }
    allocated.sink_slice_mut().sort_by_key(|run| run.pa);
    let mut prev: Option<SnapshotRun> = None;
    for run in allocated.sink_slice().iter() {
        if let Some(p) = prev {
            if run.pa < p.end_pa() {
                return Err(if run.flags != p.flags {
                    SnapshotError::ImageRangeConflict
                } else {
                    SnapshotError::BadRun
                });
            }
        }
        prev = Some(*run);
    }
    if allocated.sink_slice().is_empty() {
        return Err(SnapshotError::NoRuns);
    }
    Ok(())
}

/// Allocation-owning wrapper: reserve the two inputs' total run count.
pub fn merge_runs(
    allocated: &[SnapshotRun],
    image: &[SnapshotRun],
    layout: RamLayout,
) -> Result<Vec<SnapshotRun>, SnapshotError> {
    let mut merged = BoundedVec::reserved(allocated.len() + image.len());
    merged.sink_extend(allocated)?;
    merge_runs_into(&mut merged, image, layout)?;
    Ok(merged.into_vec())
}

// ── Capture staging ──────────────────────────────────────────────────────────

/// A physical span the capture itself occupies — its code, stack or a buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScratchSpan {
    /// Physical start (inclusive).
    pub base: u64,
    /// Physical end (exclusive).
    pub end: u64,
}

impl ScratchSpan {
    /// A scratch span.
    pub const fn new(base: u64, end: u64) -> Self {
        Self { base, end }
    }

    /// No address is covered.
    pub const fn is_empty(&self) -> bool {
        self.base >= self.end
    }

    /// Does this span intersect `[base, end)`?
    pub const fn overlaps(&self, base: u64, end: u64) -> bool {
        self.base < end && base < self.end
    }
}

/// Bytes of scratch stack the frozen window runs on.
///
/// The window holds one 4096-byte staging frame plus a few 512-byte sector and
/// header buffers, so this matches the kernel's own 64 KiB kernel stack
/// (`kernel/linker.ld`) rather than being tightened to the current usage.
pub const SCRATCH_STACK_BYTES: usize = 64 * 1024;

/// A bounded physical workspace the capture owns, reserved at boot.
///
/// It is the capture's *workspace*, not machine state: every frame in it is
/// excluded from the inventory ([`take_allocated_frame`]), so a restored image
/// can never replay the capture's own stack or buffers over live scratch, and
/// the region is what [`assert_scratch_outside_runs`] proves the planned runs do
/// not touch.  The frozen window's stack and every buffer it fills live inside
/// it, which is what makes that proof honest rather than a claim about a span
/// the capture is not actually using.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScratchRegion {
    /// Physical base (4096-aligned, inside the allocator's managed RAM).
    pub base: u64,
    /// Reserved size in bytes (a whole number of frames).
    pub bytes: usize,
}

impl ScratchRegion {
    /// Physical end (exclusive).
    pub const fn end(&self) -> u64 {
        self.base + self.bytes as u64
    }

    /// Does this region own the frame at physical address `pa`?
    pub const fn contains_frame(&self, pa: u64) -> bool {
        pa >= self.base && pa < self.end()
    }

    /// Frames reserved.
    pub const fn frames(&self) -> usize {
        self.bytes / FRAME_SIZE
    }

    /// The region as a declared capture-scratch span.
    pub const fn span(&self) -> ScratchSpan {
        ScratchSpan::new(self.base, self.end())
    }

    /// The virtual span the region is accessed through.
    pub fn virt(&self) -> (usize, usize) {
        let base = phys_to_virt(self.base as usize);
        (base, base + self.bytes)
    }

    /// Bytes the frozen window's buffers may use: everything but the stack tail.
    pub const fn buffer_bytes(&self) -> usize {
        self.bytes.saturating_sub(SCRATCH_STACK_BYTES)
    }

    /// The stack the frozen window runs on — the region's last
    /// [`SCRATCH_STACK_BYTES`], 16-byte aligned at the top.
    pub fn stack_top(&self) -> usize {
        let (_, end) = self.virt();
        end & !0xF
    }

    /// Is `sp` a stack pointer inside this region's stack span?
    ///
    /// Read from *inside* the frozen window, this is the proof that the staging
    /// really runs in the declared region and not on the boot stack (which is
    /// inside the captured image span).
    pub fn owns_stack(&self, sp: usize) -> bool {
        let (_, end) = self.virt();
        let stack_base = end.saturating_sub(SCRATCH_STACK_BYTES);
        sp >= stack_base && sp < end
    }
}

/// Where the capture's own code, stack and buffers live, as physical spans.
///
/// A capture may not save a span it is still writing: the frame read would
/// race the capture's own stack or the buffer it is filling.  The park hook
/// makes "nothing else writes it" true for the *rest* of the machine; the
/// capture's own scratch has to be outside the captured runs by construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureScratch {
    /// The scratch region this boot reserved from the frame allocator
    /// ([`reserve_scratch_region_at_boot`]).  The frozen window runs inside it.
    Region(ScratchRegion),
    /// Reserved capture storage outside every captured run, at these physical
    /// spans.  Must be non-empty.
    Reserved(&'static [ScratchSpan]),
    /// This build/boot has no reserved capture storage.  The capture cannot
    /// name its own spans, so it cannot prove it is not about to save its own
    /// in-flight stack or buffer: it must refuse rather than guess.
    None,
}

impl CaptureScratch {
    /// The declared spans, or `None` when the capture cannot name them.
    pub const fn spans(self) -> Option<&'static [ScratchSpan]> {
        match self {
            Self::Reserved(spans) => Some(spans),
            Self::Region(_) | Self::None => None,
        }
    }

    /// The reserved region, when the declaration is the live boot reservation.
    pub const fn region(self) -> Option<ScratchRegion> {
        match self {
            Self::Region(region) => Some(region),
            Self::Reserved(_) | Self::None => None,
        }
    }

    /// Visit every declared span, returning how many were visited.
    fn for_each_span(self, mut visit: impl FnMut(ScratchSpan)) -> usize {
        match self {
            Self::Region(region) => {
                visit(region.span());
                1
            }
            Self::Reserved(spans) => {
                for span in spans {
                    visit(*span);
                }
                spans.len()
            }
            Self::None => 0,
        }
    }
}

/// Prove that the capture can name its own scratch storage, or refuse.
///
/// A missing, empty or zero-length declaration means the capture cannot say
/// where its own code/stack/buffers are, so it cannot prove it will not save an
/// in-flight buffer.
fn require_capture_scratch(scratch: CaptureScratch) -> Result<(), SnapshotError> {
    let mut declared = 0usize;
    let mut degenerate = 0usize;
    scratch.for_each_span(|span| {
        declared += 1;
        if span.is_empty() {
            degenerate += 1;
        }
    });
    if declared > 0 && degenerate == 0 {
        Ok(())
    } else {
        Err(SnapshotError::NoReservedScratch)
    }
}

/// Prove that no declared capture-scratch span intersects a planned run.
///
/// Checked before any block I/O.  A missing or empty declaration is
/// [`SnapshotError::NoReservedScratch`] and an intersection is
/// [`SnapshotError::ScratchOverlapsRun`]; both refuse the capture, so it never
/// snapshots its own in-flight buffer.
fn assert_scratch_outside_runs(
    scratch: CaptureScratch,
    runs: &[SnapshotRun],
) -> Result<(), SnapshotError> {
    require_capture_scratch(scratch)?;
    let mut overlaps = false;
    scratch.for_each_span(|span| {
        for run in runs {
            if span.overlaps(run.pa, run.end_pa()) {
                overlaps = true;
            }
        }
    });
    if overlaps {
        Err(SnapshotError::ScratchOverlapsRun)
    } else {
        Ok(())
    }
}

// ── The reserved capture workspace ───────────────────────────────────────────

/// The capture's reserved scratch region, published by the boot reservation.
static SNAPSHOT_SCRATCH: Spinlock<Option<ScratchRegion>> = Spinlock::new(None);

/// The scratch region this boot reserved, if any.
pub fn scratch_region() -> Option<ScratchRegion> {
    *SNAPSHOT_SCRATCH.lock()
}

/// Take the capture's scratch workspace from `allocator`.
///
/// One contiguous run of [`SCRATCH_FRAMES`] frames, sized from the format
/// geometry — the buffers for the partition capacity bound plus a kernel-sized
/// stack — and never from the allocator's own size.  The frames stay allocated
/// for the kernel's lifetime: the region is the capture's workspace, never
/// freed and never part of an inventory.
///
/// Refuses (never degrades) when no such contiguous run exists, and when the
/// run would intersect `image` — the trusted kernel-image span, which the plan
/// saves, so a workspace there would make the capture's own declaration a lie.
/// Pure: it does not publish into [`SNAPSHOT_SCRATCH`], so the host lane can
/// exercise the real allocation against a synthetic allocator.
pub fn take_scratch_region(
    allocator: &mut FrameAllocator,
    image: Option<ImageRegion>,
) -> Result<ScratchRegion, SnapshotError> {
    let base = crate::memory::frame::reserve_contiguous_run(allocator, SCRATCH_FRAMES)
        .ok_or(SnapshotError::NoReservedScratch)?;
    let region = ScratchRegion {
        base: base as u64,
        bytes: SCRATCH_FRAMES * FRAME_SIZE,
    };
    // Where the image span is not derivable (x86-64's higher half, the 32-bit
    // targets) capture refuses with `ImageRegionUnavailable` before it stages,
    // so there is nothing to check and `image` is `None`.
    if let Some(image) = image {
        if region_intersects_image(region, image) {
            // Give the frames back: an unusable region is not a workspace.
            for frame in 0..SCRATCH_FRAMES {
                allocator.deallocate_frame(base + frame * FRAME_SIZE);
            }
            log::error!(
                "[snapshot] scratch {:#x}..{:#x} intersects the kernel image {:#x}..{:#x}",
                region.base,
                region.end(),
                image.base,
                image.end
            );
            return Err(SnapshotError::ScratchOverlapsRun);
        }
    }
    Ok(region)
}

/// Does a candidate workspace intersect the frames the plan saves?
fn region_intersects_image(region: ScratchRegion, image: ImageRegion) -> bool {
    region.base < image.end && image.base < region.end()
}

/// Reserve and publish the capture's scratch workspace, once.
///
/// Called from the boot path ([`crate::boot::reserve_snapshot_scratch`]) once
/// the frame allocator exists and before any capture can run, so a capture never
/// has to take its own workspace with a live machine.  Never panics and never
/// degrades: a failure logs and leaves no region, and every capture then refuses
/// with [`SnapshotError::NoReservedScratch`].  A no-op unless the build is
/// `snapshot-qualified`, so no shipping image pays for a workspace it cannot use.
pub fn reserve_scratch_region_at_boot() {
    if !QUALIFICATION_ENABLED {
        return;
    }
    if scratch_region().is_some() {
        return;
    }
    let mut guard = FRAME_ALLOCATOR.lock();
    let Some(allocator) = guard.as_mut() else {
        log::warn!("[snapshot] no frame allocator at boot → no capture scratch");
        return;
    };
    match take_scratch_region(allocator, kernel_image().map(|image| image.trusted)) {
        Ok(region) => {
            *SNAPSHOT_SCRATCH.lock() = Some(region);
            log::info!(
                "[snapshot] reserved {} bytes ({} frames) of capture scratch at {:#x}..{:#x}",
                region.bytes,
                region.frames(),
                region.base,
                region.end()
            );
        }
        Err(err) => log::error!("[snapshot] no capture scratch: {err}"),
    }
}

// ── The frozen window's stack ────────────────────────────────────────────────

/// The frozen window runs on the region's stack, inside the frames the plan
/// never saves, or it refuses.
const SCRATCH_STACK_SUPPORTED: bool = cfg!(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
));

/// The stack pointer of the running frame, as a virtual address.
#[cfg(target_arch = "riscv64")]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `sp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp, options(nomem, nostack)) };
    sp
}

/// The stack pointer of the running frame, as a virtual address.
#[cfg(target_arch = "aarch64")]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `sp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack)) };
    sp
}

/// The stack pointer of the running frame, as a virtual address.
#[cfg(target_arch = "x86_64")]
fn stack_pointer() -> usize {
    let sp: usize;
    // SAFETY: reading `rsp` has no effect on memory or the control flow.
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nomem, nostack)) };
    sp
}

/// No stack switch exists here, so [`run_on_stack`] cannot stage outside the
/// in-flight stack; the caller's stack-pointer proof refuses the capture.
#[cfg(not(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
)))]
fn stack_pointer() -> usize {
    0
}

// The switch itself: `__cellos_snapshot_stack_run(new_sp, run, arg)` keeps the
// old stack pointer in a callee-saved register, moves `sp` to the reserved
// region, calls `run(arg)`, then restores `sp` — so the caller returns normally
// while everything `run` does (its frame, its buffers) is in the region.

#[cfg(target_arch = "riscv64")]
core::arch::global_asm!(
    ".globl __cellos_snapshot_stack_run",
    "__cellos_snapshot_stack_run:",
    "addi sp, sp, -32",
    "sd ra, 0(sp)",
    "sd s0, 8(sp)",
    "mv s0, sp",
    "mv sp, a0",
    "mv t0, a1",
    "mv a0, a2",
    "jalr t0",
    "mv sp, s0",
    "ld s0, 8(sp)",
    "ld ra, 0(sp)",
    "addi sp, sp, 32",
    "ret",
);

#[cfg(target_arch = "aarch64")]
core::arch::global_asm!(
    ".globl __cellos_snapshot_stack_run",
    "__cellos_snapshot_stack_run:",
    "stp x29, x30, [sp, #-32]!",
    "str x19, [sp, #16]",
    "mov x29, sp",
    "mov sp, x0",
    "mov x19, x1",
    "mov x0, x2",
    "blr x19",
    "mov sp, x29",
    "ldr x19, [sp, #16]",
    "ldp x29, x30, [sp], #32",
    "ret",
);

#[cfg(target_arch = "x86_64")]
core::arch::global_asm!(
    ".text",
    ".globl __cellos_snapshot_stack_run",
    "__cellos_snapshot_stack_run:",
    "push rbp",
    "mov rbp, rsp",
    "mov rsp, rdi",
    "mov rax, rsi",
    "mov rdi, rdx",
    "call rax",
    "mov rsp, rbp",
    "pop rbp",
    "ret",
);

#[cfg(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
))]
unsafe extern "C" {
    /// Switch to `new_sp` and run `run(arg)` there, then switch back.
    fn __cellos_snapshot_stack_run(new_sp: usize, run: extern "C" fn(*mut u8), arg: *mut u8);
}

/// Run `f` on the reserved scratch stack whose top is `stack_top`.
///
/// `F` and its result stay on the caller's stack; only the execution moves, so
/// the closure's captured state is untouched and the return value is copied back
/// after the switch.
///
/// # Safety
/// `stack_top` must be a 16-byte-aligned address with [`SCRATCH_STACK_BYTES`] of
/// writable, mapped space below it inside the capture's reserved region, and `f`
/// must not unwind (the kernel is `panic = "abort"`; the host lane's fixtures
/// depend on neither panicking).
unsafe fn run_on_stack<F, R>(stack_top: usize, f: F) -> R
where
    F: FnOnce() -> R,
{
    struct Job<F, R> {
        run: Option<F>,
        out: *mut Option<R>,
    }

    extern "C" fn trampoline<F: FnOnce() -> R, R>(arg: *mut u8) {
        // SAFETY: `arg` is the `&mut Job` passed below, which outlives the call.
        let job = unsafe { &mut *(arg as *mut Job<F, R>) };
        let run = job.run.take().expect("the scratch stack runs the job once");
        let value = run();
        // SAFETY: as above; `out` points at the caller's slot, on the old stack.
        unsafe { *job.out = Some(value) };
    }

    let mut out: Option<R> = None;
    let mut job = Job {
        run: Some(f),
        out: &mut out,
    };
    #[cfg(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    ))]
    // SAFETY: the caller guarantees `stack_top` addresses the reserved region's
    // stack; `trampoline` never unwinds.
    unsafe {
        __cellos_snapshot_stack_run(
            stack_top,
            trampoline::<F, R>,
            &mut job as *mut Job<F, R> as *mut u8,
        );
    }
    #[cfg(not(any(
        target_arch = "riscv64",
        target_arch = "aarch64",
        target_arch = "x86_64"
    )))]
    {
        // No switch exists on this target: run the job in place, through the
        // same trampoline.  The stack-pointer proof in `capture_window` then
        // refuses, because `stack_pointer` reports 0.
        let _ = stack_top;
        trampoline::<F, R>(&mut job as *mut Job<F, R> as *mut u8);
    }
    out.expect("the scratch stack ran the job")
}

// ── Capacity-bounded buffers ─────────────────────────────────────────────────

/// Test-hooks: how many times the capture would have reached the heap with the
/// pre-freeze reservation already exhausted — i.e. how many bounds were wrong.
///
/// Must be zero.  Incremented only by a [`BoundedVec::reserved`] buffer meeting
/// a shortage, so no ordinary (growable) buffer ever perturbs it.
#[cfg(any(feature = "test-hooks", test))]
pub static FROZEN_WINDOW_ALLOC_ATTEMPTS: AtomicU64 = AtomicU64::new(0);

/// Test-hooks: read the frozen-window allocation-attempt counter.
#[cfg(any(feature = "test-hooks", test))]
pub fn frozen_window_alloc_attempts() -> u64 {
    FROZEN_WINDOW_ALLOC_ATTEMPTS.load(Ordering::SeqCst)
}

/// Test-hooks: zero the frozen-window allocation-attempt counter.
#[cfg(any(feature = "test-hooks", test))]
pub fn reset_frozen_window_alloc_attempts() {
    FROZEN_WINDOW_ALLOC_ATTEMPTS.store(0, Ordering::SeqCst);
}

/// Count a refused heap access from a reserved buffer.
#[inline]
fn frozen_alloc_attempt() {
    #[cfg(any(feature = "test-hooks", test))]
    FROZEN_WINDOW_ALLOC_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
}

/// A buffer whose backing store is reserved up front by its caller.
///
/// It never grows: a push or resize past the reservation is refused (and
/// counted), so the capture's frozen window cannot reach the heap — a parked
/// hart may be holding the heap's non-masking lock.  Every caller reserves its
/// worst case, so a refusal means a bound was wrong, not that a capture was
/// denied a legitimate buffer.
struct BoundedVec<T> {
    items: Vec<T>,
}

impl<T> BoundedVec<T> {
    /// Reserve exactly `capacity` slots and never grow past them.
    fn reserved(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }

    fn clear(&mut self) {
        self.items.clear();
    }

    fn into_vec(self) -> Vec<T> {
        self.items
    }

    fn try_push(&mut self, value: T) -> Result<(), SnapshotError> {
        if self.items.len() == self.items.capacity() {
            frozen_alloc_attempt();
            return Err(SnapshotError::CapacityExceeded);
        }
        self.items.push(value);
        Ok(())
    }
}

impl BoundedVec<u8> {
    /// Clear and resize to `needed` zero bytes, refusing a shortage rather than
    /// growing.
    fn resize_zeroed(&mut self, needed: usize) -> Result<(), SnapshotError> {
        if self.items.capacity() < needed {
            frozen_alloc_attempt();
            return Err(SnapshotError::CapacityExceeded);
        }
        self.items.clear();
        self.items.resize(needed, 0u8);
        Ok(())
    }
}

/// A capacity-bounded destination for the inventory builders.
///
/// One planner serves both backings: [`BoundedVec`] (heap, reserved before the
/// park) and [`ScratchVec`] (the capture's reserved region, used inside the
/// frozen window).  Neither can grow past its reservation, so a shortage is a
/// refusal — and on [`ScratchVec`] it is also counted, so a bound that was
/// wrong is observable.
trait BoundedSink<T: Copy> {
    fn sink_clear(&mut self);
    fn sink_push(&mut self, value: T) -> Result<(), SnapshotError>;
    fn sink_extend(&mut self, values: &[T]) -> Result<(), SnapshotError>;
    fn sink_slice(&self) -> &[T];
    fn sink_slice_mut(&mut self) -> &mut [T];
}

/// A byte sink that can be zero-filled to an exact length.
trait ZeroedSink: BoundedSink<u8> {
    fn sink_resize_zeroed(&mut self, needed: usize) -> Result<(), SnapshotError>;
}

impl<T: Copy> BoundedSink<T> for BoundedVec<T> {
    fn sink_clear(&mut self) {
        self.clear();
    }
    fn sink_push(&mut self, value: T) -> Result<(), SnapshotError> {
        self.try_push(value)
    }
    fn sink_extend(&mut self, values: &[T]) -> Result<(), SnapshotError> {
        for &value in values {
            self.try_push(value)?;
        }
        Ok(())
    }
    fn sink_slice(&self) -> &[T] {
        &self.items
    }
    fn sink_slice_mut(&mut self) -> &mut [T] {
        &mut self.items
    }
}

impl ZeroedSink for BoundedVec<u8> {
    fn sink_resize_zeroed(&mut self, needed: usize) -> Result<(), SnapshotError> {
        self.resize_zeroed(needed)
    }
}

/// Bump allocator over the capture's reserved scratch region.
///
/// Buffers grow up from the region's base; the stack is the region's tail, so
/// the two can never collide.  Every buffer the frozen window fills is carved
/// from here, which is what keeps the window's working set inside the region:
/// it never touches the heap (whose lock a parked hart may hold) and never
/// writes a frame the plan saves.
struct ScratchArena {
    cursor: usize,
    limit: usize,
}

impl ScratchArena {
    fn new(region: ScratchRegion) -> Self {
        let (base, end) = region.virt();
        Self {
            cursor: base,
            limit: end - SCRATCH_STACK_BYTES,
        }
    }

    /// Carve `count` slots of `T`, or `None` when they do not fit.
    fn take<T: 'static>(&mut self, count: usize) -> Option<&'static mut [MaybeUninit<T>]> {
        let align = core::mem::align_of::<T>().max(1);
        let start = (self.cursor + align - 1) & !(align - 1);
        let end = start.checked_add(count.checked_mul(core::mem::size_of::<T>())?)?;
        if end > self.limit {
            return None;
        }
        self.cursor = end;
        // SAFETY: `start..end` lies inside the region the capture reserved at
        // boot.  The frames are allocator-owned and excluded from every
        // inventory, so no other owner can hand them out or replay over them;
        // the region is mapped for the kernel's lifetime; and the bump cursor
        // never returns the same bytes twice, so the slice is exclusive.
        Some(unsafe { core::slice::from_raw_parts_mut(start as *mut MaybeUninit<T>, count) })
    }

    /// Bytes still available for buffers.
    fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.cursor)
    }
}

/// A capacity-bounded buffer carved from the capture's reserved region.
///
/// The frozen-window counterpart of [`BoundedVec`]: the same never-grow
/// contract, but the bytes live in the scratch region instead of on the heap.
struct ScratchVec<T: 'static> {
    slots: &'static mut [MaybeUninit<T>],
    len: usize,
}

impl<T: Copy + 'static> ScratchVec<T> {
    fn new(slots: &'static mut [MaybeUninit<T>]) -> Self {
        Self { slots, len: 0 }
    }

    /// Slots reserved.  Host-lane only: the live path never asks (a shortage
    /// is a refusal, not something to measure against).
    #[cfg(test)]
    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn len(&self) -> usize {
        self.len
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    fn as_slice(&self) -> &[T] {
        // SAFETY: the first `len` slots were initialized by `sink_push`.
        unsafe { &*(&self.slots[..self.len] as *const [MaybeUninit<T>] as *const [T]) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: as `as_slice`, with exclusive access to the slots.
        unsafe { &mut *(&mut self.slots[..self.len] as *mut [MaybeUninit<T>] as *mut [T]) }
    }

    fn try_push(&mut self, value: T) -> Result<(), SnapshotError> {
        if self.len == self.slots.len() {
            frozen_alloc_attempt();
            return Err(SnapshotError::CapacityExceeded);
        }
        self.slots[self.len].write(value);
        self.len += 1;
        Ok(())
    }
}

impl<T: Copy + 'static> BoundedSink<T> for ScratchVec<T> {
    fn sink_clear(&mut self) {
        self.clear();
    }
    fn sink_push(&mut self, value: T) -> Result<(), SnapshotError> {
        self.try_push(value)
    }
    fn sink_extend(&mut self, values: &[T]) -> Result<(), SnapshotError> {
        for &value in values {
            self.try_push(value)?;
        }
        Ok(())
    }
    fn sink_slice(&self) -> &[T] {
        self.as_slice()
    }
    fn sink_slice_mut(&mut self) -> &mut [T] {
        self.as_mut_slice()
    }
}

impl ScratchVec<u8> {
    fn resize_zeroed(&mut self, needed: usize) -> Result<(), SnapshotError> {
        if self.slots.len() < needed {
            frozen_alloc_attempt();
            return Err(SnapshotError::CapacityExceeded);
        }
        for slot in &mut self.slots[..needed] {
            slot.write(0);
        }
        self.len = needed;
        Ok(())
    }
}

impl ZeroedSink for ScratchVec<u8> {
    fn sink_resize_zeroed(&mut self, needed: usize) -> Result<(), SnapshotError> {
        self.resize_zeroed(needed)
    }
}

/// Pre-allocated storage for one capture's frozen window.
///
/// Carved from the scratch region the boot reserved, **before** the machine is
/// frozen: the frame list, the runs, the encoded inventory and the stack the
/// window runs on all live in that region, so nothing in the window touches the
/// heap (and therefore the heap lock, which a parked hart may be holding) and
/// nothing it writes is a frame the plan saves.  The pre-freeze bound comes from
/// the region's own size — never from the whole allocator — and the region is
/// required to cover the partition capacity bound.
struct FrozenScratch {
    region: ScratchRegion,
    frame_bound: u64,
    pas: ScratchVec<u64>,
    runs: ScratchVec<SnapshotRun>,
    image: ScratchVec<SnapshotRun>,
    inventory: ScratchVec<u8>,
    /// The stack pointer this window proved it ran on (0 before it runs).
    ///
    /// Host-lane only, and per-window rather than a process-global so a test can
    /// read its own window's proof while other tests run windows in parallel.
    /// The live path records nothing: the window refuses unless the pointer it
    /// observes is inside the declared workspace, so the proof is in-band.
    #[cfg(test)]
    window_stack: core::cell::Cell<u64>,
}

/// The largest frame count whose image can fit the reserved partition, using
/// the same geometry the writer emits (worst case: one run per frame).
const fn capacity_frame_bound() -> u64 {
    let mut frames = SNAPSHOT_SECTOR_COUNT / SECTORS_PER_FRAME as u64;
    loop {
        let inv = inventory_sectors(frames + IMAGE_RUNS_MAX as u64);
        if 1 + inv + frames * SECTORS_PER_FRAME as u64 <= SNAPSHOT_SECTOR_COUNT {
            return frames;
        }
        frames -= 1;
    }
}

/// Bytes to reserve for an inventory of at most `run_bound` runs.
const fn inventory_byte_bound(run_bound: u64) -> usize {
    inventory_sectors(run_bound) as usize * SECTOR_SIZE
}

/// Bytes the frozen window's buffers need for `frame_bound` frame addresses and
/// `run_bound` runs (worst case: one run per frame).
const fn scratch_bytes_for_bounds(frame_bound: u64, run_bound: u64) -> usize {
    frame_bound as usize * core::mem::size_of::<u64>()
        + run_bound as usize * core::mem::size_of::<SnapshotRun>()
        + IMAGE_RUNS_MAX * core::mem::size_of::<SnapshotRun>()
        + inventory_byte_bound(run_bound)
}

/// Bytes the buffers need for the worst-case run bound of `frame_bound` frames.
const fn scratch_bytes_for(frame_bound: u64) -> usize {
    scratch_bytes_for_bounds(frame_bound, frame_bound + IMAGE_RUNS_MAX as u64)
}

/// The largest frame bound whose buffers fit in `bytes`.
///
/// Derived from the region's size — 24 bytes per frame (an 8-byte address plus
/// a 16-byte worst-case run) plus the inventory — not from the allocator's
/// frame count.  Capped by the partition capacity bound, since no image can be
/// larger than the reserved P3 partition.
fn frame_bound_for_scratch(bytes: usize) -> u64 {
    /// PA plus worst-case run per frame.
    const BYTES_PER_FRAME: usize = 24;
    let cap = capacity_frame_bound();
    let mut bound = core::cmp::min((bytes / BYTES_PER_FRAME) as u64, cap);
    while bound > 0 && scratch_bytes_for(bound) > bytes {
        bound -= 1;
    }
    while bound < cap && scratch_bytes_for(bound + 1) <= bytes {
        bound += 1;
    }
    bound
}

/// Frames the boot reserves for the capture's scratch region: the buffers for
/// the partition capacity bound plus the frozen window's stack.
const SCRATCH_BYTES: usize = scratch_bytes_for(capacity_frame_bound()) + SCRATCH_STACK_BYTES;
const SCRATCH_FRAMES: usize = (SCRATCH_BYTES + FRAME_SIZE - 1) / FRAME_SIZE;

/// Host-lane backing for the frozen window.
///
/// A host build has no boot path and no frame allocator, so it cannot reserve a
/// physical region; these fixtures own a process-lifetime static buffer that
/// stands in for one.  Everything downstream — the arena, the bound arithmetic,
/// the exclusion and the stack switch — is the same code the live path runs.
#[cfg(test)]
mod host_scratch {
    use super::{MaybeUninit, ScratchRegion, FRAME_SIZE, SCRATCH_BYTES};
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// Bytes in one host region: enough for a capacity-sized window, so
    /// [`super::FrozenScratch::new`] is exercised exactly as the live path runs
    /// it.
    pub const REGION_BYTES: usize = SCRATCH_BYTES + FRAME_SIZE;

    /// Regions handed out per test process (one per test that carves a window).
    const REGIONS: usize = 24;

    /// One slot per region, plus a frame of slack so a slot's base can be
    /// rounded up to a frame boundary and still hold [`REGION_BYTES`].
    const SLOT_BYTES: usize = REGION_BYTES + FRAME_SIZE;

    struct Pool {
        bytes: [MaybeUninit<u8>; REGIONS * SLOT_BYTES],
    }

    static mut POOL: Pool = Pool {
        bytes: [const { MaybeUninit::uninit() }; REGIONS * SLOT_BYTES],
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// The next host region, `bytes` long (rounded up to whole frames).
    pub fn region(bytes: usize) -> ScratchRegion {
        assert!(bytes <= REGION_BYTES, "host scratch region too small");
        let slot = NEXT.fetch_add(1, Ordering::SeqCst);
        assert!(slot < REGIONS, "host scratch pool exhausted");
        // A raw pointer, never a `&mut POOL` reference: a static-mut reference
        // would be UB-adjacent and the pool is handed out by slot, not borrowed.
        let pool = unsafe { core::ptr::addr_of_mut!(POOL.bytes) } as usize;
        let slot_base = pool + slot * SLOT_BYTES;
        // Whole frames, so a host region can stand in for an allocator-owned run
        // and the exclusion (`contains_frame`) has real frame addresses to work
        // with.
        let base = (slot_base + FRAME_SIZE - 1) & !(FRAME_SIZE - 1);
        debug_assert!(base + REGION_BYTES <= slot_base + SLOT_BYTES);
        // The host lane has no physical offset, so `phys_to_virt` is the
        // identity and this buffer *is* the region the arena carves.
        debug_assert_eq!(super::phys_to_virt(base), base);
        ScratchRegion {
            base: base as u64,
            bytes: (bytes + FRAME_SIZE - 1) / FRAME_SIZE * FRAME_SIZE,
        }
    }
}

/// Take one allocator-owned frame into the capture's frame list, unless it
/// belongs to the capture's own scratch region.
///
/// The region is the capture's *workspace*, not machine state: an inventory must
/// never name its frames, or a restore would replay the capture's own stack and
/// buffers over live scratch.  Returning `false` for an excluded frame is the
/// single definition of that exclusion — the live enumeration and the host
/// lane's fixture both go through here, and `assert_scratch_outside_runs`
/// refuses the capture if a region frame reaches the plan anyway.
fn take_allocated_frame(
    pas: &mut ScratchVec<u64>,
    region: ScratchRegion,
    pa: u64,
) -> Result<bool, SnapshotError> {
    if region.contains_frame(pa) {
        return Ok(false);
    }
    pas.try_push(pa)?;
    Ok(true)
}

impl FrozenScratch {
    /// Carve the frozen window's working set out of the boot-reserved region.
    ///
    /// Refuses ([`SnapshotError::CapacityExceeded`]) rather than degrading when
    /// the region cannot hold the buffers for [`capacity_frame_bound`] frames:
    /// a smaller bound would silently refuse a capture on a busy machine, which
    /// is the same failure discovered later and after a park.
    fn new(region: ScratchRegion) -> Result<Self, SnapshotError> {
        let frame_bound = frame_bound_for_scratch(region.buffer_bytes());
        if frame_bound < capacity_frame_bound() {
            return Err(SnapshotError::CapacityExceeded);
        }
        Self::carve(
            region,
            frame_bound,
            frame_bound + IMAGE_RUNS_MAX as u64,
            true,
        )
    }

    /// Carve `region` for an explicit `(frame_bound, run_bound)`.
    ///
    /// `require_capacity` is the live rule (the bound must cover the partition
    /// capacity); the host lane's counter fixture passes `false` to build a
    /// deliberately under-sized window and watch the bounded buffers refuse
    /// instead of growing.
    fn carve(
        region: ScratchRegion,
        frame_bound: u64,
        run_bound: u64,
        require_capacity: bool,
    ) -> Result<Self, SnapshotError> {
        if frame_bound == 0 || (require_capacity && frame_bound < capacity_frame_bound()) {
            return Err(SnapshotError::CapacityExceeded);
        }
        let mut arena = ScratchArena::new(region);
        if scratch_bytes_for_bounds(frame_bound, run_bound) > arena.remaining() {
            return Err(SnapshotError::CapacityExceeded);
        }
        let pas = arena
            .take::<u64>(frame_bound as usize)
            .ok_or(SnapshotError::CapacityExceeded)?;
        let runs = arena
            .take::<SnapshotRun>(run_bound as usize)
            .ok_or(SnapshotError::CapacityExceeded)?;
        let image = arena
            .take::<SnapshotRun>(IMAGE_RUNS_MAX)
            .ok_or(SnapshotError::CapacityExceeded)?;
        let inventory = arena
            .take::<u8>(inventory_byte_bound(run_bound))
            .ok_or(SnapshotError::CapacityExceeded)?;
        log::info!(
            "[snapshot] frozen window carved from scratch 0x{:X}..0x{:X} ({} bytes left)",
            region.base,
            region.end(),
            arena.remaining()
        );
        Ok(Self {
            region,
            frame_bound,
            pas: ScratchVec::new(pas),
            runs: ScratchVec::new(runs),
            image: ScratchVec::new(image),
            inventory: ScratchVec::new(inventory),
            #[cfg(test)]
            window_stack: core::cell::Cell::new(0),
        })
    }

    /// Storage reserved for `frame_bound` frames and `run_bound` runs, sized
    /// from the bound itself.  Host lane only: a host build has no boot path,
    /// so it has no reserved region to carve.
    #[cfg(test)]
    fn with_bounds(frame_bound: u64, run_bound: u64) -> Self {
        let bytes = scratch_bytes_for_bounds(frame_bound, run_bound) + SCRATCH_STACK_BYTES;
        let region = host_scratch::region(bytes);
        Self::carve(region, frame_bound, run_bound, false).expect("region sized for the bound")
    }

    /// Enumerate the live allocated frames into `pas` and return the live RAM
    /// layout.  The allocator lock is held only for the enumeration (a masking
    /// [`crate::sync::Spinlock`], so a hart interrupted into a park can never
    /// be holding it); no block I/O, and no allocation.
    ///
    /// The capture's own scratch region is **excluded**: those frames are the
    /// capture's workspace, not machine state, so an inventory must never name
    /// them (see [`take_allocated_frame`]).
    fn enumerate(&mut self) -> Result<RamLayout, SnapshotError> {
        let image = kernel_image().ok_or(SnapshotError::ImageRegionUnavailable)?;
        let guard = FRAME_ALLOCATOR.lock();
        let allocator = guard.as_ref().ok_or(SnapshotError::MemoryFault)?;
        self.pas.clear();
        let mut excluded = 0u32;
        for index in 0..allocator.total_frames() {
            if !allocator.is_frame_allocated(index) {
                continue;
            }
            let pa = allocator.frame_addr(index) as u64;
            // Defensive: never describe a frame outside managed RAM (MMIO
            // holes, allocator bookkeeping drift).  The bounds check in
            // `runs_from_frames` would refuse the whole capture instead;
            // skipping matches the old behaviour and keeps a stray bitmap bit
            // from blocking every capture.
            if pa >= allocator.memory_start() as u64
                && pa + FRAME_SIZE as u64 <= allocator.memory_end() as u64
            {
                if !take_allocated_frame(&mut self.pas, self.region, pa)? {
                    excluded += 1;
                }
            } else {
                log::warn!("[snapshot] skipping allocated frame outside RAM: 0x{pa:X}");
            }
        }
        if excluded > 0 {
            log::info!(
                "[snapshot] excluded {excluded} capture-scratch frame(s) from the inventory"
            );
        }
        Ok(RamLayout {
            base: allocator.memory_start() as u64,
            end: allocator.memory_end() as u64,
            image: image.trusted,
        })
    }

    /// Build the runs and the encoded inventory from `pas` (already filled) and
    /// an explicit image span.  Pure reserve-and-fill: no allocation when the
    /// reservation was sized from the pre-freeze bound.
    fn plan_from(&mut self, mutable: ImageRegion, layout: RamLayout) -> Result<(), SnapshotError> {
        if self.pas.len() as u64 > self.frame_bound {
            return Err(SnapshotError::CapacityExceeded);
        }
        plan_runs_into(
            mutable,
            layout,
            self.pas.as_slice(),
            &mut self.runs,
            &mut self.image,
        )?;
        encode_inventory_into(self.runs.as_slice(), &mut self.inventory)?;
        log::info!(
            "[snapshot] inventory: {} run(s) spanning image 0x{:X}..0x{:X}",
            self.runs.len(),
            mutable.base,
            mutable.end
        );
        Ok(())
    }

    /// The live frozen-window plan: enumerate the allocator, then build the
    /// runs and inventory.
    fn plan(&mut self) -> Result<RamLayout, SnapshotError> {
        let image = kernel_image().ok_or(SnapshotError::ImageRegionUnavailable)?;
        let layout = self.enumerate()?;
        self.plan_from(image.mutable, layout)?;
        Ok(layout)
    }
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

    /// The device's trusted monotonic epoch: the highest epoch a restore has
    /// already consumed.  `None` means this device has no monotonic source, and
    /// authenticated freshness must refuse rather than assume.
    ///
    /// Default: no source.  Real boards must supply it from a monotonic source
    /// the host cannot roll back (an MMC/eMMC RPMB write counter or equivalent).
    fn current_epoch(&self) -> Option<u64> {
        None
    }

    /// Durably advance the device's monotonic epoch to at least `epoch`.  Must
    /// never move it backwards; a device that cannot do so must return an error.
    fn commit_epoch(&self, _epoch: u64) -> Result<(), SnapshotError> {
        Err(SnapshotError::NoFreshnessSource)
    }
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
/// Allocating wrapper: encodes the inventory into a freshly reserved buffer and
/// delegates to [`capture_image_prepared`].  The frozen path calls the core
/// directly with a buffer reserved before the park.
pub fn capture_image(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    layout: RamLayout,
    runs: &[SnapshotRun],
) -> Result<CaptureReport, SnapshotError> {
    let inventory = encode_inventory(runs);
    capture_image_prepared(dev, mem, layout, runs, &inventory)
}

/// Capture `runs`, hashing and writing the caller's already-encoded
/// `inventory`.
///
/// On any error before the commit flush the image is left `WRITING`, which no
/// restore will ever replay.
pub fn capture_image_prepared(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    layout: RamLayout,
    runs: &[SnapshotRun],
    inventory: &[u8],
) -> Result<CaptureReport, SnapshotError> {
    // ── preflight: structure, then capacity, then identity ───────────────────
    if runs.len() as u64 > u32::MAX as u64 {
        return Err(SnapshotError::TooManyRuns);
    }
    let frame_count = frames_in_runs(runs, layout)?;
    let run_count = runs.len() as u64;
    let inv_sectors = inventory_sectors(run_count);
    if inventory.len() != inv_sectors as usize * SECTOR_SIZE {
        return Err(SnapshotError::RunCountMismatch);
    }
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

    // ── authenticated freshness: refuse before any block I/O ─────────────────
    // No keyed-MAC key, no monotonic device epoch, or an epoch that cannot be
    // advanced: refuse rather than write an image whose freshness is assumed.
    let trust_key = SNAPSHOT_TRUST_KEY.ok_or(SnapshotError::NoTrustKey)?;
    let device_epoch = dev
        .current_epoch()
        .ok_or(SnapshotError::NoFreshnessSource)?;
    let epoch = device_epoch
        .checked_add(1)
        .ok_or(SnapshotError::NoFreshnessSource)?;

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
        epoch,
        auth: [0u8; AUTH_BYTES],
        _reserved: [0u8; 400],
    };

    // (1) Invalidate any previously committed image and record WRITING.
    write_state(dev, &committed, SnapshotState::Writing)?;

    // The canonical stream is defined over the COMMITTED header (CRC and MAC
    // fields zero), so the hash is independent of the state byte on disk.
    let mut hasher = canonical_hasher(&committed);

    // (2) Inventory, then payload, in the order they will be hashed and read.
    hasher.update(inventory);
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
    // replayable.  The MAC is computed last, over the header that carries the
    // final CRC (and so binds the payload digest the reader will check).
    let crc32 = hasher.finalize();
    let mut committed = committed;
    committed.crc32 = crc32;
    committed.auth = header_mac(&trust_key, &committed);
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

    // ── authenticated header ─────────────────────────────────────────────────
    // The MAC binds the epoch, identity and geometry; through the header's CRC
    // it also binds the payload digest the checksum step will verify.  A header
    // whose epoch was rewritten to forge freshness fails here.
    let Some(trust_key) = SNAPSHOT_TRUST_KEY else {
        // No provisioned key: we cannot judge the image.  Unsupported, not
        // proven bad — leave the region alone.
        return RestoreOutcome::ColdBoot("no provisioned key for the authenticated epoch");
    };
    if !mac_eq(&header.auth, &header_mac(&trust_key, &header)) {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("header authentication failed");
    }

    // ── freshness ────────────────────────────────────────────────────────────
    // A replay of an older (or already consumed) image carries an epoch that is
    // not strictly newer than the device's monotonic source, and is refused.
    let device_epoch = match dev.current_epoch() {
        Some(epoch) => epoch,
        None => {
            // Unsupported device, not proven-bad image: leave the region alone.
            return RestoreOutcome::ColdBoot("device has no trusted monotonic epoch source");
        }
    };
    if header.epoch <= device_epoch {
        invalidate_on(dev);
        return RestoreOutcome::ColdBoot("stale epoch: image is not newer than the device's");
    }

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
                "inventory outside RAM or the kernel image, empty, unaligned, conflicting or overlapping",
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

    // The device's monotonic epoch advances with the decision to replay, so an
    // interrupted replay cannot re-accept this image even if the on-disk
    // CONSUMING marker were lost.  A device that cannot advance refuses.
    if dev.commit_epoch(header.epoch).is_err() {
        return RestoreOutcome::ColdBoot(
            "could not durably advance the device epoch; RAM untouched",
        );
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

/// The live managed RAM layout and this build's trusted image span, or `None`
/// when either cannot be established (allocator not ready, or the target does
/// not delimit the kernel image — see [`kernel_image`]).
fn live_layout() -> Option<RamLayout> {
    let image = kernel_image()?;
    let guard = FRAME_ALLOCATOR.lock();
    let allocator = guard.as_ref()?;
    Some(RamLayout {
        base: allocator.memory_start() as u64,
        end: allocator.memory_end() as u64,
        image: image.trusted,
    })
}

/// Build the capture inventory from an image span and an explicit allocated
/// frame list: allocator-owned runs plus the image-kind run for `mutable`.
///
/// Split out of the capture planner so the host lane can drive the exact
/// planning the capture path runs, with a simulated image and a simulated
/// allocated set.  `layout.image` must be the trusted span of the same image
/// `mutable` came from; the image run is refused when it is not inside it.
///
/// An image frame that the allocator also owns is [`SnapshotError::ImageRangeConflict`],
/// not something to merge.
///
/// `allocated_out` must have room for `allocated_pas.len() + image.len()` runs;
/// the merged inventory is returned in it.
fn plan_runs_into(
    mutable: ImageRegion,
    layout: RamLayout,
    allocated_pas: &[u64],
    allocated_out: &mut impl BoundedSink<SnapshotRun>,
    image_out: &mut impl BoundedSink<SnapshotRun>,
) -> Result<(), SnapshotError> {
    runs_from_frames_into(allocated_pas, layout, allocated_out)?;
    image_runs_into(mutable, layout, image_out)?;
    merge_runs_into(allocated_out, image_out.sink_slice(), layout)
}

/// Allocation-owning wrapper for [`plan_runs_into`], for the host lane.
#[cfg(test)]
fn plan_runs(
    mutable: ImageRegion,
    layout: RamLayout,
    allocated_pas: &[u64],
) -> Result<Vec<SnapshotRun>, SnapshotError> {
    let mut allocated = BoundedVec::reserved(allocated_pas.len() + IMAGE_RUNS_MAX);
    let mut image = BoundedVec::reserved(IMAGE_RUNS_MAX);
    plan_runs_into(mutable, layout, allocated_pas, &mut allocated, &mut image)?;
    Ok(allocated.into_vec())
}

/// Serialize all allocated physical frames to the reserved disk sector range.
///
/// Returns the number of frames written on success.
///
/// # Preflight
/// The capture refuses — before it reads a single frame and before any block
/// I/O — when it cannot stage its own storage, when every online hart other
/// than this one is not parked at an acknowledged safe point ([`quiesce`]), when
/// the device has no trusted monotonic epoch source, or when the image cannot be
/// authenticated.  A refusal leaves nothing parked and nothing written.
///
/// # Safety constraints
/// Once quiescence is acquired no other hart may run kernel code that mutates
/// the captured frames: the format cannot detect bytes that changed between the
/// read and the block write.  The requester must therefore also not allocate
/// while it holds the guard: a parked hart can be holding the heap's non-masking
/// `spinning_top` lock, and an allocation here would spin on it forever.  Every
/// buffer the frozen window touches is therefore reserved before the park
/// ([`FrozenScratch`]), and the capture refuses unless its own scratch spans are
/// declared outside the planned runs ([`CaptureScratch`]).
pub fn serialize_snapshot() -> Result<u32, SnapshotError> {
    if !QUALIFICATION_ENABLED {
        return Err(SnapshotError::GateClosed);
    }
    // The workspace the capture stages its own stack and buffers in, reserved at
    // boot from the frame allocator.  A boot that never reserved one (or whose
    // reservation failed) refuses rather than guessing a region.
    let region = scratch_region().ok_or(SnapshotError::NoReservedScratch)?;
    capture_staged(
        &quiesce::KERNEL_STATE,
        &quiesce::KERNEL_HARTS,
        &KERNEL_DEVICE,
        &KERNEL_MEMORY,
        region,
    )
}

/// The capture path with the reserved region injected, in the order
/// [`serialize_snapshot`] runs it: declare the workspace, carve the frozen
/// window out of it, and only then park harts and touch the device.
fn capture_staged<'a>(
    state: &'a quiesce::QuiesceState,
    harts: &'a dyn quiesce::QuiesceHarts,
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    region: ScratchRegion,
) -> Result<u32, SnapshotError> {
    let capture_scratch = CaptureScratch::Region(region);
    require_capture_scratch(capture_scratch)?;
    let mut scratch = FrozenScratch::new(region)?;
    capture_record(state, harts, dev, mem, capture_scratch, &mut scratch)
}

/// Prove that no hart but the requester can mutate the memory image, or refuse.
///
/// The returned guard releases the harts when it drops, so quiescence covers
/// exactly the caller's capture.
fn capture_preflight<'a>(
    state: &'a quiesce::QuiesceState,
    harts: &'a dyn quiesce::QuiesceHarts,
) -> Result<quiesce::Guard<'a>, SnapshotError> {
    match state.acquire(harts) {
        Ok(guard) => Ok(guard),
        Err(err) => {
            log::warn!("[snapshot] capture refused: {err}");
            Err(SnapshotError::HartsNotQuiesced)
        }
    }
}

/// The capture path with every collaborator injected, so a host test can prove
/// the preflight ordering — staging and quiescence first, block I/O last —
/// without a live allocator or a block device.  [`serialize_snapshot`] is this
/// with the live collaborators.
fn capture_record<'a>(
    state: &'a quiesce::QuiesceState,
    harts: &'a dyn quiesce::QuiesceHarts,
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    capture_scratch: CaptureScratch,
    scratch: &mut FrozenScratch,
) -> Result<u32, SnapshotError> {
    // A capture that cannot name its own storage refuses before it freezes
    // anything: there is nothing to prove later, and no reason to park harts.
    require_capture_scratch(capture_scratch)?;

    // `scratch` was reserved by the caller before this point — after the park
    // nothing in the capture may touch the heap (a parked hart can be holding
    // the heap's non-masking lock).
    let _quiesced = capture_preflight(state, harts)?;

    capture_frozen(dev, mem, capture_scratch, scratch)
}

/// The frozen window itself, split out so a test can hand it deliberately
/// under-sized scratch and watch the allocation-attempt counter fire.
fn capture_frozen(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    capture_scratch: CaptureScratch,
    scratch: &mut FrozenScratch,
) -> Result<u32, SnapshotError> {
    let layout = scratch.plan()?;
    capture_planned(dev, mem, capture_scratch, scratch, layout)
}

/// The frozen window with the plan already built: stage-check, switch to the
/// region's stack, then write.
fn capture_planned(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    capture_scratch: CaptureScratch,
    scratch: &mut FrozenScratch,
    layout: RamLayout,
) -> Result<u32, SnapshotError> {
    // The capture's own scratch must be outside every planned run: refuse rather
    // than save an in-flight stack or buffer.  Checked before any block I/O.
    assert_scratch_outside_runs(capture_scratch, scratch.runs.as_slice())?;

    // ...and the window must actually run inside that scratch.  Staging on the
    // region's stack is what makes the declaration above true: the boot stack
    // lives in the captured image span, so a window that ran on it would save its
    // own in-flight frame.
    let stack_top = scratch.region.stack_top();
    // SAFETY: `stack_top` is the reserved region's stack top — 16-byte aligned
    // with SCRATCH_STACK_BYTES of mapped writable space below it — and the
    // window body cannot unwind (`panic = "abort"`).
    unsafe { run_on_stack(stack_top, || capture_window(dev, mem, scratch, layout)) }
}

/// The frozen window body.  Runs on the reserved scratch stack, so its frame,
/// its sector buffers and the staging frame inside [`capture_image_prepared`]
/// are all inside the region the stage-check just approved.
fn capture_window(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    scratch: &mut FrozenScratch,
    layout: RamLayout,
) -> Result<u32, SnapshotError> {
    let sp = stack_pointer();
    if !SCRATCH_STACK_SUPPORTED || !scratch.region.owns_stack(sp) {
        // Not running on the workspace the stage-check approved: refuse rather
        // than save the caller's own in-flight stack.
        return Err(SnapshotError::NoReservedScratch);
    }
    #[cfg(test)]
    scratch.window_stack.set(sp as u64);

    #[cfg(target_arch = "riscv64")]
    let t0 = hal::common::timer::read_mtime();

    let report = capture_image_prepared(
        dev,
        mem,
        layout,
        scratch.runs.as_slice(),
        scratch.inventory.as_slice(),
    )?;

    #[cfg(target_arch = "riscv64")]
    let elapsed_ms = (hal::common::timer::read_mtime().wrapping_sub(t0)) / 10_000;
    #[cfg(not(target_arch = "riscv64"))]
    let elapsed_ms = 0u64;

    log::info!(
        "[snapshot] wrote {} frames in {} runs ({} sectors, crc {:08X}) in {} ms to LBA {} from scratch stack 0x{:X}",
        report.frames,
        report.runs,
        report.image_sectors,
        report.crc32,
        elapsed_ms,
        SNAPSHOT_BASE_LBA,
        sp
    );
    Ok(report.frames)
}

/// Host-test seam: the frozen window driven from an explicit allocated set and
/// image span instead of the live allocator/linker (which a host build has no
/// access to).  It fills the pre-reserved buffers through the same
/// `take_allocated_frame`/`plan_from`/`capture_planned` the live path uses, and
/// writes through the same `capture_image_prepared`.
#[cfg(test)]
fn capture_frozen_with_allocated(
    dev: &dyn SnapshotDevice,
    mem: &dyn FrameMemory,
    capture_scratch: CaptureScratch,
    scratch: &mut FrozenScratch,
    allocated_pas: &[u64],
    mutable: ImageRegion,
    layout: RamLayout,
) -> Result<u32, SnapshotError> {
    scratch.pas.clear();
    for &pa in allocated_pas {
        // The same exclusion the live enumeration applies: the capture's own
        // scratch frames are its workspace, not inventory.
        let _ = take_allocated_frame(&mut scratch.pas, scratch.region, pa)?;
    }
    scratch.plan_from(mutable, layout)?;
    capture_planned(dev, mem, capture_scratch, scratch, layout)
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
            log::warn!("[snapshot] allocator or kernel-image span unavailable → cold boot");
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
        /// The fake device's trusted monotonic epoch.  Deliberately *outside*
        /// `pending`: `power_cycle` drops unflushed sectors, not the device's
        /// own monotonic state — that is the whole point of a monotonic source.
        epoch: u64,
        /// Whether this fake models a device that has one at all.
        epoch_supported: bool,
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
            let mut state = State::default();
            state.epoch_supported = true;
            Self {
                inner: RefCell::new(state),
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

        /// Model a device with no trusted monotonic epoch source at all.
        pub fn without_freshness(mut self) -> Self {
            self.inner.get_mut().epoch_supported = false;
            self
        }

        /// Start the device's monotonic epoch at `epoch` (e.g. "already consumed
        /// a newer image" or "a fresh device").
        pub fn with_epoch(mut self, epoch: u64) -> Self {
            self.inner.get_mut().epoch = epoch;
            self
        }

        /// The device's current monotonic epoch, as it would report it.
        pub fn epoch(&self) -> u64 {
            self.inner.borrow().epoch
        }

        /// Move the device's monotonic epoch forward (never backwards), as a
        /// later capture/restore on the same device would.
        pub fn set_epoch(&self, epoch: u64) {
            let mut state = self.inner.borrow_mut();
            state.epoch = state.epoch.max(epoch);
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
        /// The canonical CRC over the durable image, or `None` when the header
        /// claims a geometry beyond the partition (nothing to recompute).
        fn image_crc(&self) -> Option<u32> {
            let header = self.header();
            let inv_sectors = header.inventory_sectors as u64;
            let payload_sectors = header.frame_count as u64 * SECTORS_PER_FRAME as u64;
            if inv_sectors > SNAPSHOT_SECTOR_COUNT || payload_sectors > SNAPSHOT_SECTOR_COUNT {
                return None;
            }
            let mut hasher = canonical_hasher(&header);
            for i in 0..inv_sectors {
                hasher.update(&self.durable_sector(INVENTORY_FIRST_LBA + i));
            }
            for i in 0..payload_sectors {
                hasher.update(&self.durable_sector(header.payload_lba + i));
            }
            Some(hasher.finalize())
        }

        /// Does the stored CRC match a recomputation over the durable image?
        pub fn crc_is_consistent(&self) -> bool {
            self.image_crc() == Some(self.header().crc32)
        }

        /// Recompute and store only the canonical checksum — exactly what an
        /// attacker without the MAC key can do.  The tag is left stale.
        pub fn fixup_crc_no_mac(&self) {
            if let Some(crc) = self.image_crc() {
                self.patch(SNAPSHOT_BASE_LBA, CRC_FIELD_OFFSET, &crc.to_le_bytes());
            }
        }

        /// Recompute and store the canonical checksum over the durable image,
        /// so that a crafted image is rejected by structure, not by the CRC —
        /// and re-sign the header, so it is rejected by structure rather than
        /// by the MAC either.  A crafted header may claim an absurd geometry;
        /// anything beyond the partition capacity is left alone (the reader
        /// rejects it structurally, before it looks at the MAC).
        pub fn fixup_crc(&self) {
            self.fixup_crc_no_mac();
            if let Some(key) = SNAPSHOT_TRUST_KEY {
                let tag = header_mac(&key, &self.header());
                self.patch(SNAPSHOT_BASE_LBA, AUTH_FIELD_OFFSET, &tag);
            }
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

        fn current_epoch(&self) -> Option<u64> {
            let state = self.inner.borrow();
            state.epoch_supported.then_some(state.epoch)
        }

        fn commit_epoch(&self, epoch: u64) -> Result<(), SnapshotError> {
            let mut state = self.inner.borrow_mut();
            if !state.epoch_supported {
                return Err(SnapshotError::NoFreshnessSource);
            }
            // Monotonic: never move backwards.
            state.epoch = state.epoch.max(epoch);
            Ok(())
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
        /// Write raw bytes into a mapped frame without touching the counters, so
        /// a test can plant a byte-exact marker (e.g. inside a simulated `.bss`).
        pub fn put_bytes(&self, pa: u64, offset: usize, bytes: &[u8]) {
            let mut frames = self.frames.borrow_mut();
            let slot = frames.entry(pa).or_insert([0u8; FRAME_SIZE]);
            slot[offset..offset + bytes.len()].copy_from_slice(bytes);
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
    use crate::task::quiesce;

    /// 64 frames of RAM at the RV64 RAM base.
    const BASE: u64 = 0x8020_0000;
    const FRAMES: u64 = 64;

    /// Simulated kernel image inside the fake RAM window, standing in for the
    /// linker-delimited spans a real build gets from [`kernel_image`]: an
    /// immutable text/rodata half at frame 24 and a mutable `.data`/`.bss` half
    /// at frames 32..40.  No sparse allocated frame is inside either span, so
    /// the two halves are cleanly separable (and a test that wants a conflict
    /// has to construct one).
    const IMAGE_TRUSTED_START: u64 = BASE + 24 * FRAME_SIZE as u64;
    const IMAGE_MUTABLE_START: u64 = BASE + 32 * FRAME_SIZE as u64;
    const IMAGE_MUTABLE_END: u64 = BASE + 40 * FRAME_SIZE as u64;
    const IMAGE_MUTABLE_FRAMES: u32 = 8;

    fn trusted_image() -> ImageRegion {
        ImageRegion {
            base: IMAGE_TRUSTED_START,
            end: IMAGE_MUTABLE_END,
        }
    }

    fn mutable_image() -> ImageRegion {
        ImageRegion {
            base: IMAGE_MUTABLE_START,
            end: IMAGE_MUTABLE_END,
        }
    }

    /// The simulated mutable image as image-kind runs.
    fn mutable_runs() -> Vec<SnapshotRun> {
        image_runs(mutable_image(), layout()).expect("image inventory")
    }

    /// Allocator-owned runs plus the image-kind run, as the capture planner
    /// emits them — through the planner's own entry point.
    fn planned_runs() -> Vec<SnapshotRun> {
        plan_runs(mutable_image(), layout(), &sparse_pas()).expect("planned inventory")
    }

    fn layout() -> RamLayout {
        RamLayout {
            base: BASE,
            end: BASE + FRAMES * FRAME_SIZE as u64,
            image: trusted_image(),
        }
    }

    /// Sparse allocated frames plus a distinct filler byte for every frame of
    /// the simulated mutable image, so an image run can be captured.
    fn ram_with_image() -> FakeRam {
        let ram = sparse_ram();
        for i in 0..IMAGE_MUTABLE_FRAMES {
            ram.put(
                IMAGE_MUTABLE_START + i as u64 * FRAME_SIZE as u64,
                0xB0 + i as u8,
            );
        }
        ram
    }

    /// The inventory as it stands on the device.
    fn inventory_of(disk: &FakeDisk) -> Vec<SnapshotRun> {
        let header = disk.header();
        decode_inventory(
            &(0..header.inventory_sectors)
                .flat_map(|i| disk.durable_sector(INVENTORY_FIRST_LBA + i as u64))
                .collect::<Vec<u8>>(),
            header.run_count as usize,
        )
        .expect("inventory decodes")
    }

    /// Sparse allocated frame set: two adjacent frames, a lone frame, a pair,
    /// and a frame far away — the shape that broke dense reconstruction.
    const SPARSE: [u64; 6] = [0, 1, 3, 7, 8, 20];
    /// Runs `SPARSE` groups into: `(0,1)`, `(3)`, `(7,8)`, `(20)`.
    const SPARSE_RUNS: usize = 4;

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

    /// A capture-scratch span that covers no planned run: frame 50 of the fake
    /// RAM window is neither an allocated sparse frame nor inside the image.
    const OUTSIDE_RUNS: &[ScratchSpan] = &[ScratchSpan::new(
        BASE + 50 * FRAME_SIZE as u64,
        BASE + 51 * FRAME_SIZE as u64,
    )];

    /// A capture-scratch span that intersects an allocated run (frame 3).
    const INSIDE_ALLOCATED_RUN: &[ScratchSpan] = &[ScratchSpan::new(
        BASE + 3 * FRAME_SIZE as u64,
        BASE + 4 * FRAME_SIZE as u64,
    )];

    /// A capture-scratch span that intersects the mutable image run (frame 34).
    const INSIDE_IMAGE_RUN: &[ScratchSpan] = &[ScratchSpan::new(
        BASE + 34 * FRAME_SIZE as u64,
        BASE + 35 * FRAME_SIZE as u64,
    )];

    /// A zero-length declaration: still "the capture cannot name its storage".
    const EMPTY_SCRATCH: &[ScratchSpan] = &[ScratchSpan::new(0x1000, 0x1000)];

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
            epoch: 1,
            auth: [0xA5u8; AUTH_BYTES],
            _reserved: [0u8; 400],
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
        assert_eq!(AUTH_FIELD_OFFSET, 80);
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
        assert_eq!(
            &canonical[AUTH_FIELD_OFFSET..AUTH_FIELD_OFFSET + AUTH_BYTES],
            &[0u8; AUTH_BYTES]
        );
        // Everything outside the zeroed CRC and MAC fields is untouched.
        assert_eq!(bytes[..CRC_FIELD_OFFSET], canonical[..CRC_FIELD_OFFSET]);
        assert_eq!(
            bytes[CRC_FIELD_OFFSET + 4..AUTH_FIELD_OFFSET],
            canonical[CRC_FIELD_OFFSET + 4..AUTH_FIELD_OFFSET]
        );
        assert_eq!(
            bytes[AUTH_FIELD_OFFSET + AUTH_BYTES..],
            canonical[AUTH_FIELD_OFFSET + AUTH_BYTES..]
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
        assert_eq!(inventory_of(&disk)[0].pa, BASE);

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

    // ── image-kind runs (the mutable kernel-image half) ──────────────────────

    #[test]
    fn snapshot_image_span_round_trips_a_bss_marker_byte_exactly() {
        let disk = FakeDisk::new();
        let source = ram_with_image();
        // A marker in the simulated `.bss`, straddling a sector boundary, plus
        // one in the last sector of the span.
        let marker = b"cellos-bss-marker";
        let marker_pa = IMAGE_MUTABLE_START + 2 * FRAME_SIZE as u64;
        source.put_bytes(marker_pa, 508, marker);
        let tail_pa = IMAGE_MUTABLE_END - FRAME_SIZE as u64;
        source.put_bytes(tail_pa, FRAME_SIZE - 2, &[0xC0, 0xDE]);

        let report =
            capture_image(&disk, &source, layout(), &planned_runs()).expect("capture succeeds");
        assert_eq!(report.frames, SPARSE.len() as u32 + IMAGE_MUTABLE_FRAMES);
        assert_eq!(
            report.runs,
            SPARSE_RUNS as u32 + 1,
            "image run is its own run"
        );

        // The on-disk inventory is exactly what the planner produced, it carries
        // the kind, and only the image run does.
        let inventory = inventory_of(&disk);
        assert_eq!(inventory, planned_runs());
        assert_eq!(inventory.len(), SPARSE_RUNS + 1);
        let image = *inventory.last().expect("image run");
        assert_eq!(image.flags, RUN_FLAG_IMAGE);
        assert_eq!(image.pa, IMAGE_MUTABLE_START);
        assert_eq!(image.frame_count, IMAGE_MUTABLE_FRAMES);
        assert!(
            inventory[..inventory.len() - 1]
                .iter()
                .all(|r| r.flags == 0),
            "allocator-owned runs stay unflagged"
        );
        assert!(frames_in_runs(&inventory, layout()).is_ok());
        assert!(layout().image.contains_run(&image));

        // Byte-exact round trip of the whole image, marker included.
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(target.map(), source.map());
        assert_eq!(
            &target.bytes(marker_pa).expect("marker frame")[508..508 + marker.len()],
            marker,
            "the `.bss` marker must survive capture and restore byte-exactly"
        );
        assert_eq!(
            &target.bytes(tail_pa).expect("tail frame")[FRAME_SIZE - 2..],
            &[0xC0, 0xDE]
        );
        // Payload order: allocated runs first, then the image run last, at its
        // own physical addresses.
        let writes = target.writes.borrow().clone();
        let expected: Vec<u64> = sparse_pas()
            .into_iter()
            .chain(
                (0..IMAGE_MUTABLE_FRAMES as u64)
                    .map(|i| IMAGE_MUTABLE_START + i * FRAME_SIZE as u64),
            )
            .collect();
        assert_eq!(writes, expected);

        // Red witness for the hole this closes: the same fixture captured from
        // the allocator's owned frames alone replays no image frame at all, so
        // the `.bss` marker does not come back.
        let allocated_only = FakeDisk::new();
        capture_image(&allocated_only, &source, layout(), &sparse_runs()).unwrap();
        let blind = FakeRam::default();
        assert_eq!(
            restore_image(&allocated_only, &blind, layout()),
            RestoreOutcome::Resumed
        );
        assert!(
            blind.bytes(marker_pa).is_none(),
            "allocated frames alone cannot carry the kernel image's `.bss`"
        );
        assert_eq!(blind.key_set(), sparse_pas());
    }

    #[test]
    fn snapshot_image_range_overlapping_an_allocated_run_is_refused() {
        let image = mutable_runs();
        // Three shapes of the same defect: the allocator owns a frame, or two,
        // inside the mutable image; overlap must never be merged, whatever the
        // order the two runs are supplied in.
        for passed in [
            &[IMAGE_MUTABLE_START + 2 * FRAME_SIZE as u64][..],
            &[IMAGE_MUTABLE_START][..],
            &[
                IMAGE_MUTABLE_START + FRAME_SIZE as u64,
                IMAGE_MUTABLE_START + 2 * FRAME_SIZE as u64,
            ][..],
        ] {
            let allocated = runs_from_frames(passed, layout()).expect("allocated runs");
            assert_eq!(
                merge_runs(&allocated, &image, layout()),
                Err(SnapshotError::ImageRangeConflict),
                "image range overlapping 0x{:X} must be refused",
                passed[0]
            );
        }
        // Two overlapping *image* runs are a plain structural failure.
        let mut doubled = image.clone();
        doubled.extend_from_slice(&image);
        assert_eq!(
            merge_runs(&[], &doubled, layout()),
            Err(SnapshotError::BadRun)
        );

        // The planner's own entry point refuses the conflict too — the shape the
        // live capture path would hit if the allocator ever owned an image frame.
        assert_eq!(
            plan_runs(
                mutable_image(),
                layout(),
                &[IMAGE_MUTABLE_START + 2 * FRAME_SIZE as u64]
            ),
            Err(SnapshotError::ImageRangeConflict)
        );

        // And the capture preflight refuses the same conflict with no block I/O:
        // a hand-built inventory cannot smuggle an image-kind run over an
        // allocator-owned frame.
        let conflicting = alloc::vec![
            SnapshotRun {
                pa: IMAGE_MUTABLE_START,
                frame_count: 2,
                flags: 0,
            },
            SnapshotRun {
                pa: IMAGE_MUTABLE_START + FRAME_SIZE as u64,
                frame_count: 2,
                flags: RUN_FLAG_IMAGE,
            },
        ];
        let disk = FakeDisk::new();
        assert_eq!(
            capture_image(&disk, &ram_with_image(), layout(), &conflicting),
            Err(SnapshotError::ImageRangeConflict)
        );
        assert_eq!(disk.writes(), 0, "capacity/structure precede any write");
        assert_eq!(disk.reads(), 0);
    }

    #[test]
    fn snapshot_image_range_outside_the_trusted_image_is_refused() {
        let layout = layout();
        // Below and past the trusted span.
        assert_eq!(
            image_runs(
                ImageRegion {
                    base: IMAGE_TRUSTED_START - FRAME_SIZE as u64,
                    end: IMAGE_MUTABLE_END,
                },
                layout
            ),
            Err(SnapshotError::ImageRangeOutsideImage)
        );
        assert_eq!(
            image_runs(
                ImageRegion {
                    base: IMAGE_MUTABLE_START,
                    end: IMAGE_MUTABLE_END + FRAME_SIZE as u64,
                },
                layout
            ),
            Err(SnapshotError::ImageRangeOutsideImage)
        );
        // Unrepresentable spans: empty, and not frame-aligned.
        assert_eq!(
            image_runs(ImageRegion::EMPTY, layout),
            Err(SnapshotError::ImageRegionUnavailable)
        );
        assert_eq!(
            image_runs(
                ImageRegion {
                    base: IMAGE_MUTABLE_START,
                    end: IMAGE_MUTABLE_END - 1,
                },
                layout
            ),
            Err(SnapshotError::ImageRegionUnavailable)
        );
        // A live boot that cannot delimit its image refuses every image-kind
        // run: `kernel_image() == None` is exactly this shape (x86-64,
        // riscv32/aarch32/x86-32), and it must not be read as "no image state".
        let blind = RamLayout {
            image: ImageRegion::EMPTY,
            ..layout
        };
        assert_eq!(
            image_runs(mutable_image(), blind),
            Err(SnapshotError::ImageRangeOutsideImage)
        );
        assert_eq!(
            frames_in_runs(&mutable_runs(), blind),
            Err(SnapshotError::ImageRangeOutsideImage)
        );
        // The capture preflight refuses it too, before any block I/O.
        let disk = FakeDisk::new();
        assert_eq!(
            capture_image(&disk, &ram_with_image(), blind, &mutable_runs()),
            Err(SnapshotError::ImageRangeOutsideImage)
        );
        assert_eq!(disk.writes(), 0);
        assert_eq!(disk.reads(), 0);

        // A run with a reserved flag bit is malformed, not an image run.
        let unknown = alloc::vec![SnapshotRun {
            pa: IMAGE_MUTABLE_START,
            frame_count: 1,
            flags: 0b10,
        }];
        assert_eq!(frames_in_runs(&unknown, layout), Err(SnapshotError::BadRun));
    }

    #[test]
    fn snapshot_read_is_refused_when_the_image_run_is_not_this_boot_s_image() {
        // A committed image whose inventory names an image-kind frame outside
        // this boot's trusted span must be refused by the *reader*, before a
        // byte of RAM is written — the region check is not writer-only.
        let disk = captured();
        let outside = BASE + 50 * FRAME_SIZE as u64;
        let mut run = [0u8; RUN_BYTES];
        run[0..8].copy_from_slice(&outside.to_le_bytes());
        run[8..12].copy_from_slice(&1u32.to_le_bytes());
        run[12..16].copy_from_slice(&RUN_FLAG_IMAGE.to_le_bytes());
        disk.patch(INVENTORY_FIRST_LBA, SPARSE_RUNS * RUN_BYTES, &run);
        disk.patch(
            SNAPSHOT_BASE_LBA,
            36,
            &(SPARSE_RUNS as u32 + 1).to_le_bytes(),
        );
        disk.patch(
            SNAPSHOT_BASE_LBA,
            40,
            &(SPARSE.len() as u32 + 1).to_le_bytes(),
        );
        let image_sectors = 1 + 1 + (SPARSE.len() as u32 + 1) * SECTORS_PER_FRAME as u32;
        disk.patch(SNAPSHOT_BASE_LBA, 48, &image_sectors.to_le_bytes());
        disk.fixup_crc();
        assert_eq!(
            inventory_of(&disk).last().expect("added run").flags,
            RUN_FLAG_IMAGE
        );

        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot(
                "inventory outside RAM or the kernel image, empty, unaligned, conflicting or overlapping"
            )
        );
        assert_eq!(
            target.write_count(),
            0,
            "no RAM replay before the region check"
        );
        assert_eq!(
            disk.state_byte(),
            0,
            "an image run we do not own is corruption"
        );
    }

    #[test]
    fn snapshot_image_span_is_subject_to_the_capacity_bound() {
        // The image half is not exempt from the P3 capacity bound: an image span
        // larger than the partition is refused with no block I/O at all.
        let huge = RamLayout {
            base: BASE,
            end: BASE + 40_000 * FRAME_SIZE as u64,
            image: ImageRegion {
                base: BASE,
                end: BASE + 40_000 * FRAME_SIZE as u64,
            },
        };
        let image = image_runs(
            ImageRegion {
                base: BASE,
                end: BASE + 31_000 * FRAME_SIZE as u64,
            },
            huge,
        )
        .expect("one image run");
        assert_eq!(image[0].frame_count, 31_000);
        let allocated =
            runs_from_frames(&[BASE + 39_000 * FRAME_SIZE as u64], huge).expect("allocated run");
        let runs = merge_runs(&allocated, &image, huge).expect("merged");

        let disk = FakeDisk::new();
        assert_eq!(
            capture_image(&disk, &FakeRam::default(), huge, &runs),
            Err(SnapshotError::CapacityExceeded)
        );
        assert_eq!(disk.writes(), 0, "capacity is checked before any write");
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
            image: trusted_image(),
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
                name: "reserved run flag bit",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 12, &0b10u32.to_le_bytes());
                },
                frames: 6,
                runs: 4,
            },
            Case {
                // The image kind is a known flag, but run 0 is at `BASE`, far
                // outside the trusted image span of this boot.
                name: "image-kind run outside the image",
                overwrite: |d| {
                    d.patch(INVENTORY_FIRST_LBA, 12, &RUN_FLAG_IMAGE.to_le_bytes());
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

    // ── capture preflight: all-hart quiescence ───────────────────────────────

    #[test]
    fn snapshot_capture_preflight_is_a_no_op_with_one_hart() {
        // One hart: the requester is the only hart that could mutate the image,
        // so the preflight succeeds with no park hook, no request and no wait.
        let harts = quiesce::fake::FakeHarts::new(&[0], 0).with_hook(false);
        let state = quiesce::QuiesceState::new();
        let guard = capture_preflight(&state, &harts).expect("one hart is a no-op");
        assert!(guard.all_parked());
        assert!(harts.requests().is_empty(), "no hart may be asked to park");
        assert_eq!(harts.clock_ticks(), 0, "no wait may be entered");
        drop(guard);
        assert!(harts.releases().is_empty(), "nothing was parked to release");
    }

    #[test]
    fn snapshot_capture_writes_nothing_when_memory_cannot_be_frozen() {
        // Two harts, one of them silent: the capture must refuse with the
        // unavailable result, leave the disk (and the frames) untouched, and
        // restore the hart that did park.
        let disk = FakeDisk::new();
        let ram = sparse_ram();
        let harts = quiesce::fake::FakeHarts::new(&[0, 1], 0).with_budget(4);
        let state = quiesce::QuiesceState::new();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, SPARSE_RUNS as u64 + 1);
        assert_eq!(
            capture_record(
                &state,
                &harts,
                &disk,
                &ram,
                CaptureScratch::Reserved(OUTSIDE_RUNS),
                &mut scratch,
            ),
            Err(SnapshotError::HartsNotQuiesced)
        );
        assert_eq!(disk.writes(), 0, "no sector may be written");
        assert_eq!(disk.reads(), 0, "not even the header is read");
        assert_eq!(disk.flushes(), 0);
        assert_eq!(ram.reads.get(), 0, "no frame is read from memory");
        assert_eq!(harts.releases(), vec![1], "the parked hart is restored");
        assert!(harts.outstanding().is_empty());

        // The refusal released the claim and the park: a retry where every hart
        // acknowledges gets past the preflight.
        harts.ack_on_request(&[1]);
        let guard = capture_preflight(&state, &harts).expect("retry after a refusal");
        assert!(guard.all_parked());
        assert_eq!(harts.parked(), vec![1]);
        drop(guard);
    }

    // ── frozen window: no allocation between park and release ────────────────

    /// A run bound that covers the fake allocated set and the image run.
    fn fake_run_bound() -> u64 {
        (SPARSE.len() + SPARSE_RUNS + IMAGE_RUNS_MAX) as u64
    }

    /// Drive the frozen window over the fake allocated set and image span.
    fn frozen_capture(
        disk: &FakeDisk,
        ram: &FakeRam,
        capture_scratch: CaptureScratch,
        scratch: &mut FrozenScratch,
    ) -> Result<u32, SnapshotError> {
        capture_frozen_with_allocated(
            disk,
            ram,
            capture_scratch,
            scratch,
            &sparse_pas(),
            mutable_image(),
            layout(),
        )
    }

    #[test]
    fn snapshot_frozen_capture_never_reaches_the_heap() {
        // The allocation-attempt counter is process-global, so a successful
        // capture and the positive control that increments it must live in one
        // test: no other test overruns a reservation, so the zero reading is
        // not racing anything.
        reset_frozen_window_alloc_attempts();
        let disk = FakeDisk::new();
        let ram = ram_with_image();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
        let frames = frozen_capture(
            &disk,
            &ram,
            CaptureScratch::Reserved(OUTSIDE_RUNS),
            &mut scratch,
        )
        .expect("frozen capture succeeds");
        assert_eq!(frames, SPARSE.len() as u32 + IMAGE_MUTABLE_FRAMES);
        assert_eq!(
            frozen_window_alloc_attempts(),
            0,
            "the frozen window must never reach the heap"
        );
        // The buffers the window filled are the ones reserved before it.
        assert_eq!(scratch.pas.capacity(), FRAMES as usize);

        // The image it wrote round-trips.
        disk.power_cycle();
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(target.map(), ram.map());

        // The same window staged through its own reserved workspace: the buffers
        // and the stack are the region's, and the counter is still zero.
        let region = scratch.region;
        let disk = FakeDisk::new();
        let ram = ram_with_image();
        let frames = frozen_capture(&disk, &ram, CaptureScratch::Region(region), &mut scratch)
            .expect("a capture declaring the workspace it stages in succeeds");
        assert_eq!(frames, SPARSE.len() as u32 + IMAGE_MUTABLE_FRAMES);
        assert_eq!(
            frozen_window_alloc_attempts(),
            0,
            "staging through the reserved workspace must not reach the heap"
        );
        assert!(
            region.owns_stack(scratch.window_stack.get() as usize),
            "the window must run on the workspace's stack"
        );

        // Positive control: an under-sized reservation is refused and counted,
        // never grown.
        reset_frozen_window_alloc_attempts();

        // (a) the allocated frame list does not fit the reserved `pas`.
        let disk = FakeDisk::new();
        let mut small_pas = FrozenScratch::with_bounds(2, fake_run_bound());
        assert_eq!(
            frozen_capture(
                &disk,
                &ram,
                CaptureScratch::Reserved(OUTSIDE_RUNS),
                &mut small_pas,
            ),
            Err(SnapshotError::CapacityExceeded)
        );
        assert!(
            frozen_window_alloc_attempts() > 0,
            "an exceeded bound must be counted, not allocated"
        );
        assert_eq!(disk.writes(), 0, "refused before any block I/O");
        assert_eq!(disk.reads(), 0);

        // (b) the run list does not fit the reserved `runs`.
        reset_frozen_window_alloc_attempts();
        let mut small_runs = FrozenScratch::with_bounds(FRAMES, SPARSE_RUNS as u64);
        assert_eq!(
            frozen_capture(
                &disk,
                &ram,
                CaptureScratch::Reserved(OUTSIDE_RUNS),
                &mut small_runs,
            ),
            Err(SnapshotError::CapacityExceeded)
        );
        assert!(
            frozen_window_alloc_attempts() > 0,
            "an exceeded bound is counted"
        );
        assert_eq!(disk.writes(), 0);
    }

    // ── capture staging: the capture's own storage is outside the runs ───────

    #[test]
    fn snapshot_capture_refuses_scratch_inside_a_planned_run() {
        for (name, spans) in [
            ("allocated run", INSIDE_ALLOCATED_RUN),
            ("image run", INSIDE_IMAGE_RUN),
        ] {
            let disk = FakeDisk::new();
            let ram = ram_with_image();
            let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
            assert_eq!(
                frozen_capture(&disk, &ram, CaptureScratch::Reserved(spans), &mut scratch),
                Err(SnapshotError::ScratchOverlapsRun),
                "{name}: scratch inside a run must refuse rather than be saved"
            );
            assert_eq!(disk.writes(), 0, "{name}: refused before any block I/O");
            assert_eq!(disk.reads(), 0, "{name}: not even the header is read");
        }
    }

    #[test]
    fn snapshot_capture_refuses_when_its_storage_is_not_reserved() {
        let disk = FakeDisk::new();
        let ram = sparse_ram();
        // Two harts: if the scratch check did not run first, the preflight
        // would park one and only fail after its budget.
        let harts = quiesce::fake::FakeHarts::new(&[0, 1], 0)
            .with_hook(true)
            .with_budget(4);
        let state = quiesce::QuiesceState::new();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
        assert_eq!(
            capture_record(
                &state,
                &harts,
                &disk,
                &ram,
                CaptureScratch::None,
                &mut scratch,
            ),
            Err(SnapshotError::NoReservedScratch)
        );
        assert!(harts.requests().is_empty(), "no hart may be parked");
        assert_eq!(disk.writes(), 0);
        assert_eq!(disk.reads(), 0);

        // An empty or zero-length declaration is the same refusal.
        assert_eq!(
            require_capture_scratch(CaptureScratch::Reserved(&[])),
            Err(SnapshotError::NoReservedScratch)
        );
        assert_eq!(
            require_capture_scratch(CaptureScratch::Reserved(EMPTY_SCRATCH)),
            Err(SnapshotError::NoReservedScratch)
        );
    }

    // ── the reserved capture workspace ───────────────────────────────────────

    /// A synthetic managed range for the reservation tests: 16 MiB, far more
    /// than the workspace needs, so a contiguous run always exists.
    const RESERVE_BASE: usize = 0x8000_0000;
    const RESERVE_FRAMES: usize = 4096;

    fn reserve_allocator() -> crate::memory::frame::FrameAllocator {
        crate::memory::frame::allocator_for_tests(&[(RESERVE_BASE, RESERVE_FRAMES)])
    }

    #[test]
    fn snapshot_scratch_reservation_takes_a_contiguous_run_from_the_allocator() {
        let mut allocator = reserve_allocator();
        let owned = allocator.used_frames();
        let region =
            take_scratch_region(&mut allocator, None).expect("a fresh allocator has a run");

        // One contiguous, frame-aligned run of exactly the computed size.
        assert_eq!(region.bytes, SCRATCH_FRAMES * FRAME_SIZE);
        assert_eq!(region.frames(), SCRATCH_FRAMES);
        assert_eq!(region.base % FRAME_SIZE as u64, 0, "frame-aligned");
        assert!(region.base >= RESERVE_BASE as u64);
        assert!(region.end() <= (RESERVE_BASE + RESERVE_FRAMES * FRAME_SIZE) as u64);
        assert_eq!(
            allocator.used_frames() - owned,
            SCRATCH_FRAMES,
            "the whole workspace is allocator-owned (and so excluded from the inventory)"
        );
        for frame in 0..SCRATCH_FRAMES {
            let pa = region.base as usize + frame * FRAME_SIZE;
            let index = (pa - RESERVE_BASE) / FRAME_SIZE;
            assert!(
                allocator.is_frame_allocated(index),
                "scratch frame {frame} must be marked used"
            );
        }
        // Every frame the workspace covers is managed RAM, so the capture can
        // name its frames as physical addresses.
        assert!(allocator.manages(region.base as usize));
        assert!(allocator.manages(region.end() as usize - 1));

        // The bound the region yields is the partition capacity bound: the
        // workspace is sized from the format geometry, not from the allocator.
        assert_eq!(
            frame_bound_for_scratch(region.buffer_bytes()),
            capacity_frame_bound()
        );
        let scratch = FrozenScratch::new(region).expect("a capacity-sized workspace carves");
        assert_eq!(scratch.frame_bound, capacity_frame_bound());
        assert_eq!(scratch.pas.capacity(), capacity_frame_bound() as usize);
    }

    #[test]
    fn snapshot_pre_freeze_bound_is_derived_from_the_region_size() {
        // The bound is a function of the region's size alone...
        assert_eq!(frame_bound_for_scratch(0), 0);
        assert_eq!(frame_bound_for_scratch(scratch_bytes_for(64)), 64);
        assert_eq!(frame_bound_for_scratch(scratch_bytes_for(4096)), 4096);
        assert!(frame_bound_for_scratch(scratch_bytes_for(64)) < 4096);
        // ...and the partition geometry caps it, not the machine's frame count.
        assert_eq!(
            frame_bound_for_scratch(scratch_bytes_for(capacity_frame_bound() * 2)),
            capacity_frame_bound()
        );
    }

    #[test]
    fn snapshot_reservation_refuses_a_workspace_inside_the_kernel_image() {
        let mut allocator = reserve_allocator();
        let owned = allocator.used_frames();
        // The kernel owns the frames the allocator would hand out first.
        let image = ImageRegion {
            base: RESERVE_BASE as u64,
            end: RESERVE_BASE as u64 + 512 * FRAME_SIZE as u64,
        };
        assert_eq!(
            take_scratch_region(&mut allocator, Some(image)),
            Err(SnapshotError::ScratchOverlapsRun)
        );
        assert_eq!(
            allocator.used_frames(),
            owned,
            "a refused workspace gives its frames back"
        );

        // A trusted span that does not touch the run is fine.
        let elsewhere = ImageRegion {
            base: RESERVE_BASE as u64 - 0x1000_0000,
            end: RESERVE_BASE as u64 - 0x0FFF_F000,
        };
        let region =
            take_scratch_region(&mut allocator, Some(elsewhere)).expect("disjoint image span");
        assert!(!region_intersects_image(region, elsewhere));
        assert!(
            allocator.manages(region.base as usize),
            "the workspace is managed RAM"
        );
    }

    #[test]
    fn snapshot_workspace_intersection_is_span_exact() {
        let image = ImageRegion {
            base: 0x8020_0000,
            end: 0x8040_0000,
        };
        for (name, region, expected) in [
            (
                "inside",
                ScratchRegion {
                    base: 0x8030_0000,
                    bytes: FRAME_SIZE,
                },
                true,
            ),
            (
                "straddling the base",
                ScratchRegion {
                    base: 0x801F_F000,
                    bytes: 4 * FRAME_SIZE,
                },
                true,
            ),
            (
                "straddling the end",
                ScratchRegion {
                    base: 0x803F_F000,
                    bytes: 4 * FRAME_SIZE,
                },
                true,
            ),
            (
                "containing",
                ScratchRegion {
                    base: 0x8000_0000,
                    bytes: 0x0080_0000,
                },
                true,
            ),
            (
                "ending at the base",
                ScratchRegion {
                    base: 0x801F_F000,
                    bytes: FRAME_SIZE,
                },
                false,
            ),
            (
                "starting at the end",
                ScratchRegion {
                    base: 0x8040_0000,
                    bytes: FRAME_SIZE,
                },
                false,
            ),
        ] {
            assert_eq!(region_intersects_image(region, image), expected, "{name}");
        }
    }

    #[test]
    fn snapshot_frozen_window_runs_on_the_reserved_scratch_stack() {
        let here = stack_pointer();
        let disk = FakeDisk::new();
        let ram = ram_with_image();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
        let region = scratch.region;
        assert!(
            !region.owns_stack(here),
            "the test's own stack is not the workspace"
        );

        let frames = frozen_capture(&disk, &ram, CaptureScratch::Region(region), &mut scratch)
            .expect("a capture declaring the workspace it stages in succeeds");
        assert_eq!(frames, SPARSE.len() as u32 + IMAGE_MUTABLE_FRAMES);

        // The window recorded the stack it proved it was running on: the
        // workspace's, not the caller's.
        let sp = scratch.window_stack.get();
        assert_ne!(sp, 0, "the frozen window must record where it ran");
        assert!(
            region.owns_stack(sp as usize),
            "the window must run on the workspace's stack, not the boot stack: sp={sp:#x}"
        );
        assert!(
            !region.owns_stack(here),
            "the caller keeps running on its own stack"
        );
        assert!(disk.writes() > 0, "the capture wrote an image");

        // The image round-trips, and the stack switch left no trace in it.
        disk.power_cycle();
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(target.map(), ram.map());
    }

    #[test]
    fn snapshot_stack_switch_returns_to_the_caller() {
        let region = host_scratch::region(SCRATCH_STACK_BYTES);
        let caller_sp = stack_pointer();
        // The closure's own state stays on the caller's stack; only execution
        // moves.
        let observed = unsafe { run_on_stack(region.stack_top(), stack_pointer) };
        assert!(
            region.owns_stack(observed),
            "the switched frame must run in the workspace: sp={observed:#x}"
        );
        assert!(!region.owns_stack(caller_sp));
        assert_eq!(
            stack_pointer(),
            caller_sp,
            "the caller's stack must be restored"
        );
    }

    #[test]
    fn snapshot_scratch_frames_never_appear_in_the_inventory() {
        let disk = FakeDisk::new();
        let ram = FakeRam::default();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
        let region = scratch.region;

        // The workspace's frames, presented as if the allocator owned them —
        // exactly what `enumerate` sees.
        let scratch_pas: Vec<u64> = (0..region.frames() as u64)
            .map(|i| region.base + i * FRAME_SIZE as u64)
            .collect();
        let mut allocated = sparse_pas();
        allocated.extend_from_slice(&scratch_pas);
        allocated.sort_unstable();
        assert!(allocated.windows(2).all(|w| w[0] < w[1]), "ascending");
        assert!(
            allocated.iter().any(|pa| region.contains_frame(*pa)),
            "the input must contain frames the exclusion has to drop"
        );

        // A mutable image span above the workspace, inside the same layout.
        let mutable = ImageRegion {
            base: region.end(),
            end: region.end() + IMAGE_MUTABLE_FRAMES as u64 * FRAME_SIZE as u64,
        };
        // The layout spans the fake RAM window and wherever the host pool
        // landed, so the planner's own bounds describe both.
        let layout = RamLayout {
            base: core::cmp::min(BASE, region.base),
            end: mutable.end,
            image: mutable,
        };

        // Map every frame the capture *should* read, and the workspace's frames
        // too: if the exclusion were skipped they would be read and saved.
        for pa in sparse_pas() {
            ram.put(pa, 0xA0);
        }
        for pa in scratch_pas.iter().copied() {
            ram.put(pa, 0x5C);
        }
        for i in 0..IMAGE_MUTABLE_FRAMES as u64 {
            ram.put(mutable.base + i * FRAME_SIZE as u64, 0xB0);
        }

        let frames = capture_frozen_with_allocated(
            &disk,
            &ram,
            CaptureScratch::Region(region),
            &mut scratch,
            &allocated,
            mutable,
            layout,
        )
        .expect("the workspace frames are excluded, so nothing overlaps the declaration");

        // Nothing the capture saved is in the workspace...
        let runs = inventory_of(&disk);
        for run in &runs {
            assert!(
                run.end_pa() <= region.base || run.pa >= region.end(),
                "run {run:?} covers the capture's own workspace"
            );
        }
        assert_eq!(
            frames,
            (allocated.len() - scratch_pas.len()) as u32 + IMAGE_MUTABLE_FRAMES
        );
        // ...and nothing in the workspace was read.
        assert_eq!(
            ram.reads.get(),
            (allocated.len() - scratch_pas.len()) as u64 + IMAGE_MUTABLE_FRAMES as u64,
            "the capture read exactly the frames it saved"
        );

        // Red witness: the same plan *without* the exclusion does contain the
        // workspace, and the stage-check refuses it rather than saving it.
        let unexcluded = plan_runs(mutable, layout, &allocated).expect("plan builds");
        assert!(
            unexcluded
                .iter()
                .any(|run| run.pa < region.end() && region.base < run.end_pa()),
            "the unexcluded plan must cover the workspace"
        );
        assert_eq!(
            assert_scratch_outside_runs(CaptureScratch::Region(region), &unexcluded),
            Err(SnapshotError::ScratchOverlapsRun),
            "a workspace frame in the plan must be refused, not saved"
        );
    }

    #[test]
    fn snapshot_capture_refuses_when_the_workspace_cannot_hold_the_bound() {
        let disk = FakeDisk::new();
        let ram = sparse_ram();
        // Two harts, a working hook, a short budget: a refusal that parked a
        // hart would show up as a request.
        let harts = quiesce::fake::FakeHarts::new(&[0, 1], 0)
            .with_hook(true)
            .with_budget(4);
        let state = quiesce::QuiesceState::new();

        // Enough for a small window, far short of the partition capacity bound.
        let small = host_scratch::region(SCRATCH_STACK_BYTES + FRAME_SIZE);
        assert_eq!(
            capture_staged(&state, &harts, &disk, &ram, small),
            Err(SnapshotError::CapacityExceeded)
        );
        assert!(
            harts.requests().is_empty(),
            "no hart may be parked for a refusal"
        );
        assert_eq!(disk.writes(), 0, "refused before any block I/O");
        assert_eq!(disk.reads(), 0);
        assert_eq!(disk.flushes(), 0);
        assert_eq!(ram.reads.get(), 0, "no frame may be read");

        // Smaller than the stack alone, and empty, are the same refusal.
        assert_eq!(
            FrozenScratch::new(host_scratch::region(FRAME_SIZE)).err(),
            Some(SnapshotError::CapacityExceeded)
        );
        assert_eq!(
            FrozenScratch::new(ScratchRegion { base: 0, bytes: 0 }).err(),
            Some(SnapshotError::CapacityExceeded)
        );
    }

    #[test]
    fn snapshot_capture_refuses_a_workspace_overlapping_a_planned_run() {
        let disk = FakeDisk::new();
        let ram = FakeRam::default();
        let mut scratch = FrozenScratch::with_bounds(FRAMES, fake_run_bound());
        let region = scratch.region;

        // The build's mutable image span lies *inside* the workspace: the plan
        // would save the frames the window runs in and buffers from.  The boot
        // reservation refuses such a region (`region_intersects_image`), so
        // reaching the stage-check means the reservation was bypassed — and the
        // check still refuses rather than saving its own stack.
        let mutable = ImageRegion {
            base: region.base + FRAME_SIZE as u64,
            end: region.base + 2 * FRAME_SIZE as u64,
        };
        let layout = RamLayout {
            base: region.base,
            end: region.end() + FRAME_SIZE as u64,
            image: mutable,
        };
        let allocated = [region.end()];

        assert_eq!(
            capture_frozen_with_allocated(
                &disk,
                &ram,
                CaptureScratch::Region(region),
                &mut scratch,
                &allocated,
                mutable,
                layout,
            ),
            Err(SnapshotError::ScratchOverlapsRun)
        );
        assert_eq!(disk.writes(), 0, "refused before any block I/O");
        assert_eq!(disk.reads(), 0);
        assert_eq!(ram.reads.get(), 0);
    }

    #[test]
    fn snapshot_boot_reservation_publishes_nothing_without_an_allocator() {
        // The host lane has no frame allocator and never runs the boot path: the
        // boot entry point must leave no workspace rather than invent one, which
        // is what makes the live entry point's refusal deterministic.
        reserve_scratch_region_at_boot();
        assert!(scratch_region().is_none());
    }

    #[cfg(feature = "snapshot-qualified")]
    #[test]
    fn snapshot_qualified_build_without_a_reserved_workspace_refuses() {
        assert!(QUALIFICATION_ENABLED);
        assert!(
            scratch_region().is_none(),
            "no host test publishes a workspace"
        );
        assert_eq!(
            serialize_snapshot(),
            Err(SnapshotError::NoReservedScratch),
            "a qualified capture without a reserved workspace must refuse, not guess"
        );
    }

    // ── authenticated freshness ──────────────────────────────────────────────

    fn capture_authentic(disk: &FakeDisk) {
        let source = sparse_ram();
        capture_image(disk, &source, layout(), &sparse_runs()).expect("authentic capture");
    }

    /// The attacker's saved copy of a `Committed` image: put the state byte back
    /// and re-fix the (unkeyed) CRC.  The MAC cannot be forged, and the epoch
    /// has not moved, so freshness is what refuses this.
    fn recommit(disk: &FakeDisk) {
        disk.patch(SNAPSHOT_BASE_LBA, 6, &[SnapshotState::Committed as u8]);
        disk.fixup_crc();
    }

    fn copy_image(from: &FakeDisk, to: &FakeDisk) {
        let sectors = from.header().image_sectors as u64;
        for lba in SNAPSHOT_BASE_LBA..SNAPSHOT_BASE_LBA + sectors {
            to.patch(lba, 0, &from.durable_sector(lba));
        }
    }

    #[test]
    fn snapshot_newer_epoch_restores_and_advances_the_device() {
        let disk = FakeDisk::new().with_epoch(7);
        capture_authentic(&disk);
        assert_eq!(disk.header().epoch, 8, "capture records device epoch + 1");
        disk.power_cycle();
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(
            disk.epoch(),
            8,
            "the replay decision advances the device epoch"
        );
    }

    #[test]
    fn snapshot_equal_epoch_is_refused() {
        let disk = FakeDisk::new();
        capture_authentic(&disk);
        disk.power_cycle();
        let first = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &first, layout()),
            RestoreOutcome::Resumed
        );
        assert_eq!(disk.epoch(), 1);

        // The attacker replays the saved `Committed` header: its epoch now
        // equals what the device reports, and strictly-newer is required.
        recommit(&disk);
        let replay = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &replay, layout()),
            RestoreOutcome::ColdBoot("stale epoch: image is not newer than the device's")
        );
        assert_eq!(replay.write_count(), 0, "no RAM replay");
        assert_eq!(disk.state_byte(), 0, "the replayed image is erased");
    }

    #[test]
    fn snapshot_older_epoch_is_refused() {
        let disk = FakeDisk::new();
        capture_authentic(&disk); // image epoch 1
        disk.set_epoch(5); // the device has consumed newer images since
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("stale epoch: image is not newer than the device's")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(disk.state_byte(), 0);
    }

    #[test]
    fn snapshot_tampered_epoch_is_refused() {
        let disk = FakeDisk::new();
        capture_authentic(&disk);
        // Forge a fresher epoch: the MAC covers it, so the header stops
        // authenticating even though the attacker can re-fix the CRC.
        disk.patch(
            SNAPSHOT_BASE_LBA,
            EPOCH_FIELD_OFFSET,
            &9999u64.to_le_bytes(),
        );
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("header authentication failed")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(disk.state_byte(), 0, "a forged header is erased");
    }

    #[test]
    fn snapshot_payload_tamper_with_a_refixed_crc_is_refused() {
        let disk = FakeDisk::new();
        capture_authentic(&disk);
        // The attacker tampers a payload byte and re-fixes the (unkeyed) CRC.
        // The accidental checksum is now self-consistent, so only the MAC
        // stands between the tampered image and a replay.
        disk.patch(INVENTORY_FIRST_LBA + 1, 0, &[0xA5]);
        disk.fixup_crc_no_mac();
        assert!(
            disk.crc_is_consistent(),
            "the attacker's re-fixed CRC must be self-consistent"
        );
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&disk, &target, layout()),
            RestoreOutcome::ColdBoot("header authentication failed")
        );
        assert_eq!(target.write_count(), 0, "no RAM replay");
        assert_eq!(disk.state_byte(), 0, "a tampered image is erased");
    }

    #[test]
    fn snapshot_device_without_a_monotonic_source_refuses() {
        // Capture: refused before any block I/O when there is no epoch source.
        let blind = FakeDisk::new().without_freshness();
        let source = sparse_ram();
        assert_eq!(
            capture_image(&blind, &source, layout(), &sparse_runs()),
            Err(SnapshotError::NoFreshnessSource)
        );
        assert_eq!(
            blind.writes(),
            0,
            "no write may precede the freshness check"
        );

        // Restore: an otherwise-valid image is refused, and the region is left
        // alone — a device we cannot judge is not proven bad.
        let good = FakeDisk::new();
        capture_authentic(&good);
        let blind2 = FakeDisk::new().without_freshness();
        copy_image(&good, &blind2);
        let target = FakeRam::default();
        assert_eq!(
            restore_image(&blind2, &target, layout()),
            RestoreOutcome::ColdBoot("device has no trusted monotonic epoch source")
        );
        assert_eq!(target.write_count(), 0);
        assert_eq!(
            blind2.state_byte(),
            SnapshotState::Committed as u8,
            "an unsupported device is not written to"
        );
    }

    #[test]
    fn snapshot_header_mac_binds_every_covered_field() {
        let key = SNAPSHOT_TRUST_KEY.expect("dev trust key");
        let base = valid_header();
        let tag = header_mac(&key, &base);
        let cases: [(&str, fn(&mut SnapshotHeader)); 16] = [
            ("magic", |h| h.magic ^= 1),
            ("version", |h| h.version ^= 1),
            ("state", |h| h.state ^= 1),
            ("flags", |h| h.flags ^= 1),
            ("kernel hash", |h| h.kernel_hash ^= 1),
            ("ram base", |h| h.ram_base ^= 1),
            ("ram end", |h| h.ram_end ^= 1),
            ("sector size", |h| h.sector_size ^= 1),
            ("run count", |h| h.run_count ^= 1),
            ("frame count", |h| h.frame_count ^= 1),
            ("inventory sectors", |h| h.inventory_sectors ^= 1),
            ("image sectors", |h| h.image_sectors ^= 1),
            ("payload lba", |h| h.payload_lba ^= 1),
            ("crc (payload digest)", |h| h.crc32 ^= 1),
            ("epoch", |h| h.epoch ^= 1),
            ("reserved tail", |h| h._reserved[0] ^= 1),
        ];
        for (name, mutate) in cases {
            let mut h = base;
            mutate(&mut h);
            assert!(
                !mac_eq(&tag, &header_mac(&key, &h)),
                "{name} must be covered by the MAC"
            );
        }
        // The MAC's own field is excluded by definition (the tag is computed
        // over the header with it zeroed).
        let mut excluded = base;
        excluded.auth = [0x11u8; AUTH_BYTES];
        assert!(mac_eq(&tag, &header_mac(&key, &excluded)));
        assert_eq!(excluded.signed_bytes()[AUTH_FIELD_OFFSET], 0);
    }
}
