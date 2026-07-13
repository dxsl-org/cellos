//! Constants for [`super::manifest::CellManifest`] — magic/version, isolation
//! tiers, and capability flag bits.  Split out of `manifest.rs` to keep that file
//! under the 200-LOC law; re-exported from `manifest.rs` so callers see one
//! surface (`api::manifest::MANIFEST_FLAG_*` etc. keep working unchanged).

/// Magic value identifying a valid manifest (`0x5649_4345`, "VICE" as a u32).
pub const MANIFEST_MAGIC: u32 = 0x5649_4345;

/// Current manifest layout version.  Bump on any field addition or reorder.
pub const MANIFEST_VERSION: u8 = 2;

/// The v1 layout version (8-byte record).  Recognised by `from_bytes` for upcast.
pub const MANIFEST_VERSION_V1: u8 = 1;

// ─── Isolation tiers (x86 PKU protection domain request) ─────────────────────
// A tier is a FLOOR request, the inverse of a capability: a higher number means
// MORE isolation / LESS authority.  A cell may always raise its own tier
// (self-restriction); it may NOT lower it below the floor the loader derives from
// its capabilities (see `kernel/src/loader.rs`).  This is why declaring a tier is
// safe without a ceiling check for the raise direction, and gated for the lower.

/// Trusted-core domain (PKU key 0 — no fencing).  Only reachable if the cell's
/// caps already authorise it (the loader floors non-privileged cells above this).
pub const TIER_TRUSTED_CORE: u8 = 0;
/// Standard Rust cell (PKU key 1).  The default authority floor for a plain cell.
pub const TIER_STANDARD: u8 = 1;
/// Tier-1b C/FFI cell (PKU key 2).  Fences untrusted FFI code from the cell's Rust
/// data — the key the v1 `TODO(pku-ffi)` could not reach.
pub const TIER_TIER1B_FFI: u8 = 2;
/// Untrusted domain (maps to the most-isolated available key).
pub const TIER_UNTRUSTED: u8 = 3;
/// Sentinel meaning "no explicit tier requested — apply the caller's floor
/// policy (the legacy `is_trusted` heuristic)."  Set automatically on upcast from
/// a v1 manifest (which had no tier field), AND baked in by the tier-less
/// constructors (`CellManifest::new`/`with_parts`, used by `declare_manifest!`'s
/// back-compat macro forms) into a native v2 record — so it is a valid value in
/// BOTH v1-upcast and native-v2 manifests, never an error on its own.
pub const TIER_LEGACY: u8 = 0xFF;

// ─── Capability flags (u16 in v2; low 8 bits are bit-identical to v1) ─────────

/// Raw block-device access (BlkRead/BlkWrite/BlkFlush).  Grants `BlockIoCap`.
pub const MANIFEST_FLAG_BLOCK_IO: u16 = 1 << 0;
/// Network transmit/receive (NetTx/NetRx).  Grants `NetworkCap`.
pub const MANIFEST_FLAG_NETWORK: u16 = 1 << 1;
/// Cell spawning and hot-swap (SpawnFromPath/SpawnPinned/HotSwap).  Grants `SpawnCap`.
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

/// Bitmask of all defined flags.  `from_bytes` rejects manifests setting any bit
/// outside this mask (a stale/forward binary is treated as malformed → legacy path
/// grants, the fail-safe direction).
pub const MANIFEST_FLAGS_MASK: u16 =
    MANIFEST_FLAG_BLOCK_IO | MANIFEST_FLAG_NETWORK | MANIFEST_FLAG_SPAWN
    | MANIFEST_FLAG_GPIO | MANIFEST_FLAG_UART | MANIFEST_FLAG_HYPERVISOR
    | MANIFEST_FLAG_PART_DATA | MANIFEST_FLAG_PART_LFS
    | MANIFEST_FLAG_CAN | MANIFEST_FLAG_ADC;
