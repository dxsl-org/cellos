//! Constants for [`super::manifest::CellManifest`] — magic/version, protection
//! classes, legacy isolation-tier aliases, and capability flag bits. Split out
//! of `manifest.rs` to keep that file under the 200-LOC law; re-exported from
//! `manifest.rs` so callers see one surface
//! (`api::manifest::MANIFEST_FLAG_*` etc. keep working unchanged).

/// Magic value identifying a valid manifest (`0x5649_4345`, "VICE" as a u32).
pub const MANIFEST_MAGIC: u32 = 0x5649_4345;

/// Current manifest layout version.  Bump on any field addition or reorder.
pub const MANIFEST_VERSION: u8 = 2;

/// The v1 layout version (8-byte record).  Recognised by `from_bytes` for upcast.
pub const MANIFEST_VERSION_V1: u8 = 1;

// ─── Protection classes / legacy isolation-tier names ─────────────────────────
// The on-disk byte is a FLOOR request for the x86 PKU protection domain: a
// higher number means MORE isolation / LESS authority. A cell may always raise
// its own class (self-restriction); it may NOT lower it below the floor the
// loader derives from its capabilities (see `kernel/src/loader.rs`). This is why
// declaring a class is safe without a ceiling check for the raise direction, and
// gated for the lower.

/// Trusted-core domain (PKU key 0 — no fencing).  Only reachable if the cell's
/// caps already authorise it (the loader floors non-privileged cells above this).
pub const TIER_TRUSTED_CORE: u8 = 0;
/// Canonical alias for `TIER_TRUSTED_CORE`.
pub const PROTECTION_CLASS_TRUSTED_CORE: u8 = TIER_TRUSTED_CORE;
/// Standard Rust cell (PKU key 1).  The default authority floor for a plain cell.
pub const TIER_STANDARD: u8 = 1;
/// Canonical alias for `TIER_STANDARD`.
pub const PROTECTION_CLASS_STANDARD: u8 = TIER_STANDARD;
/// Tier-1b C/FFI cell (PKU key 2).  Fences untrusted FFI code from the cell's Rust
/// data — the key the v1 `TODO(pku-ffi)` could not reach.
pub const TIER_TIER1B_FFI: u8 = 2;
/// Canonical alias for `TIER_TIER1B_FFI`.
pub const PROTECTION_CLASS_FFI: u8 = TIER_TIER1B_FFI;
/// Untrusted domain (maps to the most-isolated available key).
pub const TIER_UNTRUSTED: u8 = 3;
/// Canonical alias for `TIER_UNTRUSTED`.
pub const PROTECTION_CLASS_UNTRUSTED: u8 = TIER_UNTRUSTED;
/// Sentinel meaning "no explicit tier requested — apply the caller's floor
/// policy (the legacy `is_trusted` heuristic)."  Set automatically on upcast from
/// a v1 manifest (which had no tier field), AND baked in by the tier-less
/// constructors (`CellManifest::new`/`with_parts`, used by `declare_manifest!`'s
/// back-compat macro forms) into a native v2 record — so it is a valid value in
/// BOTH v1-upcast and native-v2 manifests, never an error on its own.
pub const TIER_LEGACY: u8 = 0xFF;
/// Canonical alias for `TIER_LEGACY`.
pub const PROTECTION_CLASS_LEGACY: u8 = TIER_LEGACY;

// ─── Capability flags (u16 in v2; low 8 bits are bit-identical to v1) ─────────

/// Raw block-device access (BlkRead/BlkWrite/BlkFlush).  Grants `BlockIoCap`.
pub const MANIFEST_FLAG_BLOCK_IO: u16 = 1 << 0;
/// Network transmit/receive (NetTx/NetRx).  Grants `NetworkCap`.
pub const MANIFEST_FLAG_NETWORK: u16 = 1 << 1;
/// Cell spawning and lifecycle control entry points (SpawnFromPath/SpawnPinned). Grants `SpawnCap`.
pub const MANIFEST_FLAG_SPAWN: u16 = 1 << 2;
/// GPIO pin control (ViGpio driver cell).  MMIO range via `sys_request_mmio`.
pub const MANIFEST_FLAG_GPIO: u16 = 1 << 3;
/// UART serial access (ViUart driver cell).  MMIO range via `sys_request_mmio`.
pub const MANIFEST_FLAG_UART: u16 = 1 << 4;
/// RISC-V H-extension (hypervisor) CSR access for VMM cells.  Grants `HypervisorCap`
/// only when the CPU also reports H-ext at boot.
pub const MANIFEST_FLAG_HYPERVISOR: u16 = 1 << 5;
/// Block-I/O sector range grant: MBR partition P1 (FAT32, `api::disk`).
pub const MANIFEST_FLAG_PART_DATA: u16 = 1 << 6;
/// Block-I/O sector range grant: MBR partition P4 (littlefs, `api::disk`).
pub const MANIFEST_FLAG_PART_LFS: u16 = 1 << 7;
/// CAN bus controller MMIO (v2 — freed by the u16 widening).  Grants the CAN
/// device class via `sys_request_mmio`.
pub const MANIFEST_FLAG_CAN: u16 = 1 << 8;
/// ADC controller MMIO (v2).  Grants the ADC device class via `sys_request_mmio`.
pub const MANIFEST_FLAG_ADC: u16 = 1 << 9;
/// I2C controller MMIO (v2).  Grants the I2C device class via `sys_request_mmio`.
pub const MANIFEST_FLAG_I2C: u16 = 1 << 10;
/// SPI controller MMIO (v2).  Grants the SPI device class via `sys_request_mmio`.
pub const MANIFEST_FLAG_SPI: u16 = 1 << 11;

/// Bitmask of all defined flags.  `from_bytes` rejects manifests setting any bit
/// outside this mask (a stale/forward binary is treated as malformed → legacy path
/// grants, the fail-safe direction).
pub const MANIFEST_FLAGS_MASK: u16 = MANIFEST_FLAG_BLOCK_IO
    | MANIFEST_FLAG_NETWORK
    | MANIFEST_FLAG_SPAWN
    | MANIFEST_FLAG_GPIO
    | MANIFEST_FLAG_UART
    | MANIFEST_FLAG_HYPERVISOR
    | MANIFEST_FLAG_PART_DATA
    | MANIFEST_FLAG_PART_LFS
    | MANIFEST_FLAG_CAN
    | MANIFEST_FLAG_ADC
    | MANIFEST_FLAG_I2C
    | MANIFEST_FLAG_SPI;
