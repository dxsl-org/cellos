//! ARM64 Linux Image header parser + guest RAM placement.
//!
//! Parses the 64-byte ARM64 Image header, places vmlinuz at the correct GPA
//! (guest_ram_base + 2MB-aligned + text_offset), initramfs within 1GB of kernel,
//! and the DTB at an 8-byte-aligned GPA below the kernel load address.
//!
//! # ARM64 Image header layout (from Linux Documentation/arm64/booting.rst)
//! ```text
//! Offset  Size  Field
//!  0x00    4    code0 (executable instruction or MZ)
//!  0x04    4    code1
//!  0x08    8    text_offset (image load offset from start of RAM, LE)
//!  0x10    8    image_size  (effective image size, LE)
//!  0x18    8    flags
//!  0x20    8    res2
//!  0x28    8    res3
//!  0x30    8    res4
//!  0x38    4    magic ("ARM\x64" = 0x644D5241)
//!  0x3C    4    res5
//! ```

extern crate alloc;
use alloc::vec::Vec;
use types::{ViError, ViResult};

/// ARM64 Image header magic.
const ARM64_IMAGE_MAGIC: u32 = 0x644D5241;

/// Loaded guest image addresses.
pub struct LoadedGuest {
    /// GPA of the kernel entry point (= RAM base + text_offset).
    pub kernel_entry_gpa: u64,
    /// GPA of the initramfs blob.
    pub initrd_gpa: u64,
    /// Size of the initramfs in bytes.
    pub initrd_size: u64,
    /// GPA of the DTB (passed in x0 on entry).
    pub dtb_gpa: u64,
}

/// Parse the ARM64 Image header from `kernel_bytes`.
///
/// Returns `(text_offset, image_size)` or an error if the magic is wrong.
pub fn parse_image_header(kernel_bytes: &[u8]) -> ViResult<(u64, u64)> {
    if kernel_bytes.len() < 0x40 {
        return Err(ViError::InvalidInput);
    }
    let magic = u32::from_le_bytes(kernel_bytes[0x38..0x3C].try_into().unwrap());
    if magic != ARM64_IMAGE_MAGIC {
        return Err(ViError::InvalidInput);
    }
    let text_offset = u64::from_le_bytes(kernel_bytes[0x08..0x10].try_into().unwrap());
    let image_size  = u64::from_le_bytes(kernel_bytes[0x10..0x18].try_into().unwrap());
    Ok((text_offset, image_size))
}

/// Place vmlinuz, initramfs, and DTB into guest RAM by writing via the VMM.
///
/// # Layout
/// ```text
///   GUEST_RAM_BASE = 0x4000_0000
///   dtb_gpa        = GUEST_RAM_BASE + 0x0000 (DTB at start, ≤ 2MB)
///   kernel_gpa     = GUEST_RAM_BASE + 2MB-align(text_offset) [typically 0x4020_0000]
///   initrd_gpa     = kernel_gpa + round_up(image_size, 2MB) [within 1GB of kernel]
/// ```
///
/// All writes go through `write_fn(gpa, bytes)` which calls `sys_write_guest_memory`.
pub fn place_images<W>(
    kernel_bytes: &[u8],
    initrd_bytes: &[u8],
    dtb_bytes: &[u8],
    guest_ram_base: u64,
    mut write_fn: W,
) -> ViResult<LoadedGuest>
where
    W: FnMut(u64, &[u8]) -> ViResult<()>,
{
    let (text_offset, image_size) = parse_image_header(kernel_bytes)?;

    // Align text_offset up to 2MB boundary (ARM64 boot protocol requirement).
    const MB2: u64 = 2 * 1024 * 1024;
    let aligned_offset = (text_offset + MB2 - 1) & !(MB2 - 1);

    let kernel_gpa = guest_ram_base + aligned_offset;

    // DTB: 8-byte-aligned, placed at guest_ram_base (before the kernel).
    // Must be within 2MB of start of RAM per ARM64 boot spec.
    let dtb_gpa = guest_ram_base; // 0x4000_0000

    // initramfs: after kernel, 2MB-aligned, within 1GB window.
    let initrd_offset = aligned_offset
        + ((image_size + MB2 - 1) & !(MB2 - 1));
    let initrd_gpa = guest_ram_base + initrd_offset;

    // Write DTB first (small, fits before kernel).
    write_fn(dtb_gpa, dtb_bytes)?;

    // Write kernel.
    write_fn(kernel_gpa, kernel_bytes)?;

    // Write initramfs.
    write_fn(initrd_gpa, initrd_bytes)?;

    Ok(LoadedGuest {
        kernel_entry_gpa: kernel_gpa,
        initrd_gpa,
        initrd_size: initrd_bytes.len() as u64,
        dtb_gpa,
    })
}

/// Read a file from the ViCell VFS into a `Vec<u8>`.
pub fn read_file_from_vfs(path: &str) -> ViResult<Vec<u8>> {
    let mut f = ostd::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}
