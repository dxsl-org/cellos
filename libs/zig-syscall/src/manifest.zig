// Cellos cell manifest — Zig intentionally emits the legacy v1 8-byte record.
//
// The Rust loader upcasts this to the 16-byte v2 ABI by zero-extending flags and
// setting the compatibility protection-class sentinel, so this file MUST keep
// the v1 layout until Zig cells opt into native v2 explicitly.

pub const MANIFEST_MAGIC: u32 = 0x5649_4345; // "VICE"

/// Capability flag bits for CellManifest.flags.
pub const Flags = struct {
    pub const BLOCK_IO:    u8 = 1 << 0; // raw FAT32 / littlefs block access
    pub const NETWORK:     u8 = 1 << 1; // TCP/UDP socket syscalls
    pub const SPAWN:       u8 = 1 << 2; // sys_spawn capability
    pub const GPIO:        u8 = 1 << 3; // GPIO MMIO via sys_request_mmio
    pub const UART:        u8 = 1 << 4; // UART MMIO via sys_request_mmio
    pub const HYPERVISOR:  u8 = 1 << 5; // H-extension CSR (RISC-V VMM only)
    pub const PART_DATA:   u8 = 1 << 6; // FAT32 partition P1 block access
    pub const PART_LFS:    u8 = 1 << 7; // littlefs partition P4 block access
};

/// 8-byte legacy cell manifest. The kernel loader upcasts it into the Rust v2
/// `CellManifest`; this layout itself must stay 8 bytes.
pub const CellManifest = extern struct {
    magic:   u32 = MANIFEST_MAGIC,
    version: u8  = 1,
    flags:   u8,
    _pad:    [2]u8 = .{ 0, 0 },

    comptime {
        if (@sizeOf(CellManifest) != 8)
            @compileError("CellManifest size mismatch — must be 8 bytes");
    }
};

/// Emit the __ViCell_manifest ELF section.
///
/// Call once at module scope inside a comptime block:
///   comptime {
///       manifest.declare(.{ .flags = 0 });
///   }
///
/// The comptime context forces the export var into the final ELF even if unused,
/// preventing LLD garbage collection.
pub fn declare(comptime m: CellManifest) void {
    // Comptime function that emits the export at compile time.
    // This forces the symbol into the final ELF.
    _ = struct {
        export var cell_manifest: CellManifest linksection("__ViCell_manifest") = m;
    };
}
