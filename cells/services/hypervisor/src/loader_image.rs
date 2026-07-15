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
    let image_size = u64::from_le_bytes(kernel_bytes[0x10..0x18].try_into().unwrap());
    Ok((text_offset, image_size))
}

/// Compute the guest RAM layout from the ARM64 Image header fields.
///
/// # Layout
/// ```text
///   GUEST_RAM_BASE = 0x4000_0000
///   dtb_gpa        = GUEST_RAM_BASE + 0x0000 (DTB at start, ≤ 2MB)
///   kernel_gpa     = GUEST_RAM_BASE + 2MB-align(text_offset) [typically 0x4020_0000]
///   initrd_gpa     = kernel_gpa + round_up(image_size, 2MB) [within 1GB of kernel]
/// ```
///
/// `image_size` is the header's effective size (text+data+bss), so the initrd
/// lands past the kernel's runtime footprint, not just past the file bytes.
pub fn compute_layout(text_offset: u64, image_size: u64, guest_ram_base: u64) -> LoadedGuest {
    const MB2: u64 = 2 * 1024 * 1024;
    let aligned_offset = (text_offset + MB2 - 1) & !(MB2 - 1);
    let kernel_gpa = guest_ram_base + aligned_offset;
    let initrd_offset = aligned_offset + ((image_size + MB2 - 1) & !(MB2 - 1));
    LoadedGuest {
        kernel_entry_gpa: kernel_gpa,
        initrd_gpa: guest_ram_base + initrd_offset,
        initrd_size: 0, // filled in after the initrd stream completes
        dtb_gpa: guest_ram_base,
    }
}

/// Read the 64-byte ARM64 Image header of a VIFS1 file.
///
/// Returns `(text_offset, image_size)`.
///
/// # Errors
/// [`ViError::NotFound`] if the file cannot be opened; [`ViError::InvalidInput`]
/// if it is shorter than 64 bytes or the magic is wrong.
pub fn read_image_header(path: &str) -> ViResult<(u64, u64)> {
    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| ViError::NotFound)?;
    let mut hdr = [0u8; 0x40];
    let mut got = 0;
    while got < hdr.len() {
        match ostd::syscall::sys_read_cap(cap, &mut hdr[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(_) => {
                ostd::syscall::sys_close_cap(cap);
                return Err(ViError::IO);
            }
        }
    }
    ostd::syscall::sys_close_cap(cap);
    if got < hdr.len() {
        return Err(ViError::InvalidInput);
    }
    parse_image_header(&hdr)
}

/// Stream a VIFS1 file into guest RAM at `gpa`; returns total bytes written.
///
/// The raw Alpine Image (34+ MiB) and initramfs (8+ MiB) together exceed the
/// 8 MiB cell heap, so the images must never be buffered whole (the previous
/// read-to-`Vec` approach OOM-killed the cell). Peak memory is one 256 KiB
/// heap chunk regardless of file size. The chunk is deliberately large:
/// every ReadCap re-seeks the FAT16 cluster chain from the start (stateless
/// kernel file handles), so per-file wall time is quadratic in call count —
/// small chunks ground a 35 MiB stream for hours under TCG.
pub fn stream_file_to_guest<W>(path: &str, gpa: u64, mut write_fn: W) -> ViResult<u64>
where
    W: FnMut(u64, &[u8]) -> ViResult<()>,
{
    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| ViError::NotFound)?;
    let mut chunk = alloc::vec![0u8; 256 * 1024];
    let mut off = 0u64;
    loop {
        match ostd::syscall::sys_read_cap(cap, &mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if let Err(e) = write_fn(gpa + off, &chunk[..n]) {
                    ostd::syscall::sys_close_cap(cap);
                    return Err(e);
                }
                off += n as u64;
            }
            Err(_) => {
                ostd::syscall::sys_close_cap(cap);
                return Err(ViError::IO);
            }
        }
    }
    ostd::syscall::sys_close_cap(cap);
    Ok(off)
}
