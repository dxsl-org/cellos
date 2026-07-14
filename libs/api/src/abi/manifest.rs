//! Cell capability manifest embedded in the `__ViCell_manifest` ELF section.
//!
//! A fixed **16-byte** `#[repr(C)]` record (v2) declaring which privileged
//! capabilities a Cell requests, its isolation **tier**, and a reserved hook for
//! future per-cell cap arguments.  The kernel reads it at spawn time (see
//! `kernel/src/loader.rs::spawn_from_path`) to grant capability tokens, choose the
//! x86 PKU protection domain, and reject user Cells that over-declare privilege.
//!
//! Binary layout (16 bytes, little-endian):
//! ```text
//!   offset  0–3 : magic        u32  = MANIFEST_MAGIC (0x5649_4345)
//!   offset  4   : version      u8   = MANIFEST_VERSION (2)
//!   offset  5   : tier         u8   = TIER_* (isolation floor request; TIER_LEGACY
//!                                     on upcast from v1 → keep the is_trusted heuristic)
//!   offset  6–7 : flags        u16  = bitwise-OR of MANIFEST_FLAG_*
//!   offset  8–11: cap_args_off u32  = RESERVED (0) — future offset into a
//!                                     __ViCell_cap_args section (do not repurpose)
//!   offset 12–15: reserved     u32  = 0
//! ```
//!
//! ## v1 compatibility
//! v1 was an 8-byte record `{magic, version=1, flags:u8, _pad:[u8;2]}`.  A v2
//! kernel reads a v1 manifest via `from_bytes` (zero-extends `flags`, sets
//! `tier = TIER_LEGACY` so the loader keeps v1's `is_trusted`→PKU-key behaviour
//! byte-for-byte).  A v1 kernel reading a v2 manifest sees `version != 1` and
//! rejects it (fail-closed → legacy path grants).  `TIER_LEGACY` is ALSO a valid
//! tier value in a native v2 record (not just a v1-upcast artifact) — it is what
//! the tier-less constructors (`CellManifest::new`/`with_parts`) bake in by
//! default, meaning "no explicit tier requested." ABI-stable — see Law 1.

// Constants (magic/version, tiers, flag bits, mask) live in the sibling
// `manifest_flags` module (Law 5: `foo.rs` parallel to `foo/`, keeps this file
// under 200 LOC) and are re-exported here so `api::manifest::MANIFEST_FLAG_*` /
// `TIER_*` keep resolving unchanged for every existing caller.
pub use super::manifest_flags::*;

/// Fixed-layout capability manifest (v2).  ABI-stable — see Law 1.
///
/// Always 16 bytes due to `#[repr(C)]` and explicit reserved fields.  Version the
/// struct via `MANIFEST_VERSION` before adding fields.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CellManifest {
    /// Must equal `MANIFEST_MAGIC`; `from_bytes` rejects any other value.
    pub magic: u32,
    /// `MANIFEST_VERSION` (2) for a native v2 manifest; a v1 upcast keeps this at 2
    /// (the value in `tier` records that it came from v1).
    pub version: u8,
    /// Isolation tier request (`TIER_*`); `TIER_LEGACY` when upcast from v1.
    pub tier: u8,
    /// Bitwise-OR of `MANIFEST_FLAG_*` constants (u16 in v2).
    pub flags: u16,
    /// RESERVED — must be 0.  Future offset into a `__ViCell_cap_args` section for
    /// parameterized capabilities; kept here so filling it is an additive section
    /// parse, not a third ABI confirmation.
    pub cap_args_off: u32,
    /// RESERVED — must be 0.
    pub reserved: u32,
}

impl CellManifest {
    /// Construct a manifest from capability bits (tier defaults to `TIER_LEGACY`,
    /// preserving v1's `is_trusted`→PKU-key behaviour for cells that do not opt in).
    ///
    /// Evaluates at compile time; safe to use as a `static` initializer.
    pub const fn new(
        block_io: bool,
        network: bool,
        spawn: bool,
        gpio: bool,
        uart: bool,
        hypervisor: bool,
    ) -> Self {
        Self::with_all(
            block_io,
            network,
            spawn,
            gpio,
            uart,
            hypervisor,
            false,
            false,
            false,
            false,
            TIER_LEGACY,
        )
    }

    /// Construct a manifest including block-I/O partition range grants.
    #[allow(clippy::too_many_arguments)]
    pub const fn with_parts(
        block_io: bool,
        network: bool,
        spawn: bool,
        gpio: bool,
        uart: bool,
        hypervisor: bool,
        part_data: bool,
        part_lfs: bool,
    ) -> Self {
        Self::with_all(
            block_io,
            network,
            spawn,
            gpio,
            uart,
            hypervisor,
            part_data,
            part_lfs,
            false,
            false,
            TIER_LEGACY,
        )
    }

    /// Full constructor — all flags + tier.  The macro is the public face.
    #[allow(clippy::too_many_arguments)]
    pub const fn with_all(
        block_io: bool,
        network: bool,
        spawn: bool,
        gpio: bool,
        uart: bool,
        hypervisor: bool,
        part_data: bool,
        part_lfs: bool,
        can: bool,
        adc: bool,
        tier: u8,
    ) -> Self {
        Self {
            magic: MANIFEST_MAGIC,
            version: MANIFEST_VERSION,
            tier,
            flags: (block_io as u16 * MANIFEST_FLAG_BLOCK_IO)
                | (network as u16 * MANIFEST_FLAG_NETWORK)
                | (spawn as u16 * MANIFEST_FLAG_SPAWN)
                | (gpio as u16 * MANIFEST_FLAG_GPIO)
                | (uart as u16 * MANIFEST_FLAG_UART)
                | (hypervisor as u16 * MANIFEST_FLAG_HYPERVISOR)
                | (part_data as u16 * MANIFEST_FLAG_PART_DATA)
                | (part_lfs as u16 * MANIFEST_FLAG_PART_LFS)
                | (can as u16 * MANIFEST_FLAG_CAN)
                | (adc as u16 * MANIFEST_FLAG_ADC),
            cap_args_off: 0,
            reserved: 0,
        }
    }

    // `from_bytes` (the ELF-section parser) lives in the sibling `manifest_parse`
    // module — see that file for the v1-upcast / v2-parse logic.

    /// Returns `true` if the cell declared raw block-device access.
    pub fn has_block_io(&self) -> bool {
        self.flags & MANIFEST_FLAG_BLOCK_IO != 0
    }
    /// Returns `true` if the cell declared network transmit/receive.
    pub fn has_network(&self) -> bool {
        self.flags & MANIFEST_FLAG_NETWORK != 0
    }
    /// Returns `true` if the cell declared cell-spawning and hot-swap.
    pub fn has_spawn(&self) -> bool {
        self.flags & MANIFEST_FLAG_SPAWN != 0
    }
    /// Returns `true` if the cell declared GPIO pin-control access.
    pub fn has_gpio(&self) -> bool {
        self.flags & MANIFEST_FLAG_GPIO != 0
    }
    /// Returns `true` if the cell declared UART serial access.
    pub fn has_uart(&self) -> bool {
        self.flags & MANIFEST_FLAG_UART != 0
    }
    /// Returns `true` if the cell declared H-extension hypervisor CSR access.
    pub fn has_hypervisor(&self) -> bool {
        self.flags & MANIFEST_FLAG_HYPERVISOR != 0
    }
    /// Returns `true` if the cell's block I/O is granted the P1 (FAT32) range.
    pub fn has_part_data(&self) -> bool {
        self.flags & MANIFEST_FLAG_PART_DATA != 0
    }
    /// Returns `true` if the cell's block I/O is granted the P4 (littlefs) range.
    pub fn has_part_lfs(&self) -> bool {
        self.flags & MANIFEST_FLAG_PART_LFS != 0
    }
    /// Returns `true` if the cell declared CAN controller MMIO access (v2).
    pub fn has_can(&self) -> bool {
        self.flags & MANIFEST_FLAG_CAN != 0
    }
    /// Returns `true` if the cell declared ADC controller MMIO access (v2).
    pub fn has_adc(&self) -> bool {
        self.flags & MANIFEST_FLAG_ADC != 0
    }

    /// The declared isolation tier (`TIER_*`, or `TIER_LEGACY` if upcast from v1).
    pub fn tier(&self) -> u8 {
        self.tier
    }

    /// Returns `true` if any privileged capability bit is set.  Used by
    /// `spawn_from_path` to reject over-declaring user Cells (non-`/bin/` paths).
    pub fn declares_any_privilege(&self) -> bool {
        self.flags != 0
    }
}

// `declare_manifest!` (embeds a CellManifest into the current Cell's ELF binary)
// lives in the sibling `manifest_macro` module.
