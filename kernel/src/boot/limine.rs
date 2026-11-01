//! Limine bootloader protocol structures and requests.
//!
//! This module defines the Limine protocol for communicating with the bootloader.
//! See: https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md

/// Limine protocol magic values
const LIMINE_COMMON_MAGIC: [u64; 2] = [0xc7b1dd30df4c8b88, 0x0a82e883a194f07b];

/// Limine base-revision tag (v8 protocol, revision 3).
/// Layout: [identifier_magic0, identifier_magic1, revision].
/// The bootloader writes back the revision it will honour.
#[used]
#[link_section = ".requests"]
static LIMINE_BASE_REVISION: [u64; 3] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3];

/// Limine request-section delimiter: start marker (required by rev 2+).
#[used]
#[link_section = ".requests_start_marker"]
static REQUESTS_START_MARKER: [u64; 4] = [
    0xf6b8f4b39de7d1ae,
    0xfab91a6940fcb9cf,
    0x785c6ed015d3e316,
    0x181e920a7852b9d9,
];

/// Limine request-section delimiter: end marker (required by rev 2+).
#[used]
#[link_section = ".requests_end_marker"]
static REQUESTS_END_MARKER: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];

/// Memory map request
#[repr(C)]
pub struct LimineMemoryMapRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineMemoryMapResponse,
}

unsafe impl Send for LimineMemoryMapRequest {}
unsafe impl Sync for LimineMemoryMapRequest {}

#[repr(C)]
pub struct LimineMemoryMapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: *const *const LimineMemoryMapEntry,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LimineMemoryMapEntry {
    pub base: u64,
    pub length: u64,
    pub entry_type: u64,
}

#[used]
#[link_section = ".requests"]
static mut MEMORY_MAP_REQUEST: LimineMemoryMapRequest = LimineMemoryMapRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x67cf3d9d378a806f,
        0xe304acdfc50c3c62,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Framebuffer request
#[repr(C)]
pub struct LimineFramebufferRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineFramebufferResponse,
}

unsafe impl Send for LimineFramebufferRequest {}
unsafe impl Sync for LimineFramebufferRequest {}

#[repr(C)]
pub struct LimineFramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: *const *const LimineFramebuffer,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct LimineFramebuffer {
    pub address: *mut u8,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: *const u8,
}

#[used]
#[link_section = ".requests"]
static mut FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// HHDM (Higher Half Direct Map) request
#[repr(C)]
pub struct LimineHhdmRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineHhdmResponse,
}

unsafe impl Send for LimineHhdmRequest {}
unsafe impl Sync for LimineHhdmRequest {}

#[repr(C)]
pub struct LimineHhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[used]
#[link_section = ".requests"]
static mut HHDM_REQUEST: LimineHhdmRequest = LimineHhdmRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x48dcf1cb8ad2b852,
        0x63984e959a98244b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Kernel address request
#[repr(C)]
pub struct LimineKernelAddressRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineKernelAddressResponse,
}

unsafe impl Send for LimineKernelAddressRequest {}
unsafe impl Sync for LimineKernelAddressRequest {}

#[repr(C)]
pub struct LimineKernelAddressResponse {
    pub revision: u64,
    pub physical_base: u64,
    pub virtual_base: u64,
}

#[used]
#[link_section = ".requests"]
static mut KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x71ba76863cc55f63,
        0xb2644a48c516a487,
    ],
    revision: 0,
    response: core::ptr::null(),
};

// x86 MP revision 0 ABI adapted from Limine v8.x-binary/limine.h.
// Copyright (C) 2022-2025 mintsuki and contributors, BSD-0-Clause:
// https://raw.githubusercontent.com/limine-bootloader/limine/v8.x-binary/limine.h
// Protocol: https://github.com/limine-bootloader/limine/blob/v8.x/PROTOCOL.md
// Bootloader-reclaimable memory is never admitted to our frame allocator; these
// records and Limine's page tables therefore remain live through AP handoff.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct LimineMpInfo {
    pub processor_id: u32,
    pub lapic_id: u32,
    pub reserved: u64,
    pub goto_address: core::sync::atomic::AtomicUsize,
    pub extra_argument: u64,
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct LimineMpResponse {
    pub revision: u64,
    pub flags: u32,
    pub bsp_lapic_id: u32,
    pub cpu_count: u64,
    pub cpus: *const *mut LimineMpInfo,
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineMpRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineMpResponse,
    flags: u64,
}

#[cfg(target_arch = "x86_64")]
#[used]
#[link_section = ".requests"]
static mut MP_REQUEST: LimineMpRequest = LimineMpRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x95a67b819a1b857e,
        0xa0b61b723b6a73e0,
    ],
    revision: 0,
    response: core::ptr::null(),
    flags: 0, // legacy physical xAPIC, never request x2APIC
};

#[cfg(target_arch = "x86_64")]
const _: [(); 32] = [(); core::mem::size_of::<LimineMpInfo>()];
#[cfg(target_arch = "x86_64")]
const _: [(); 32] = [(); core::mem::size_of::<LimineMpResponse>()];

/// A response pointer must name a complete, aligned bootloader-owned object.
#[cfg(target_arch = "x86_64")]
pub fn mp_record_valid<T>(pointer: *const T, count: usize) -> bool {
    let start = pointer as usize;
    if start == 0 || start % core::mem::align_of::<T>() != 0 {
        return false;
    }
    let Some(size) = core::mem::size_of::<T>().checked_mul(count) else { return false };
    let Some(end) = start.checked_add(size) else { return false };
    let Some(offset) = get_hhdm_offset() else { return false };
    let Some(physical) = (start as u64).checked_sub(offset) else { return false };
    let Some(physical_end) = (end as u64).checked_sub(offset) else { return false };
    let Some(map) = get_memory_map() else { return false };
    // Limine's memory-map response is already a mandatory trusted boot input.
    unsafe {
        (0..map.entry_count as usize).any(|index| {
            let entry = &**map.entries.add(index);
            entry.entry_type == 5
                && physical >= entry.base
                && entry.base.checked_add(entry.length).is_some_and(|limit| physical_end <= limit)
        })
    }
}

#[cfg(target_arch = "x86_64")]
pub fn get_mp_response() -> Option<&'static LimineMpResponse> {
    let response = unsafe { core::ptr::read_volatile(&raw const MP_REQUEST.response) };
    mp_record_valid(response, 1).then(|| unsafe { &*response })
}

/// Get memory map from Limine
pub fn get_memory_map() -> Option<&'static LimineMemoryMapResponse> {
    unsafe {
        let response = MEMORY_MAP_REQUEST.response;
        if response.is_null() {
            None
        } else {
            Some(&*response)
        }
    }
}

/// Get framebuffer from Limine
pub fn get_framebuffer() -> Option<&'static LimineFramebufferResponse> {
    unsafe {
        let response = FRAMEBUFFER_REQUEST.response;
        if response.is_null() {
            None
        } else {
            Some(&*response)
        }
    }
}

/// Get HHDM offset from Limine
pub fn get_hhdm_offset() -> Option<u64> {
    unsafe {
        let response = HHDM_REQUEST.response;
        if response.is_null() {
            None
        } else {
            Some((*response).offset)
        }
    }
}

/// Get kernel addresses from Limine
pub fn get_kernel_address() -> Option<&'static LimineKernelAddressResponse> {
    unsafe {
        let response = KERNEL_ADDRESS_REQUEST.response;
        if response.is_null() {
            None
        } else {
            Some(&*response)
        }
    }
}

/// DTB (Device Tree Blob) request — Limine v8 protocol.
/// The response provides the physical address of the DTB passed by firmware.
/// On RISC-V UEFI boots this is the only reliable way to get the DTB address
/// (the `a1` register contains the Limine boot info pointer, not the DTB).
///
/// Request GUID (after LIMINE_COMMON_MAGIC):
///   0x0b40dca86177520e, 0xc8809c1e7bbbde33
/// Verify against: https://github.com/limine-bootloader/limine/blob/v8.x-binary/limine.h
#[repr(C)]
pub struct LimineDtbRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineDtbResponse,
}

unsafe impl Send for LimineDtbRequest {}
unsafe impl Sync for LimineDtbRequest {}

#[repr(C)]
pub struct LimineDtbResponse {
    pub revision: u64,
    /// Physical address of the DTB (may be null if no DTB on this platform).
    pub dtb_ptr: *const u8,
}

#[used]
#[link_section = ".requests"]
static mut DTB_REQUEST: LimineDtbRequest = LimineDtbRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x0b40dca86177520e,
        0xc8809c1e7bbbde33,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Return the DTB physical address from the Limine DtbResponse, if present.
///
/// Returns `None` when:
/// - not booted via Limine (OpenSBI direct boot), or
/// - Limine did not populate a DTB response (non-RISC-V or no firmware DTB).
///
/// The kernel's `platform::init` calls this first and falls back to the `a1`
/// register value when it returns `None`.
pub fn get_dtb_ptr() -> Option<usize> {
    // SAFETY: DTB_REQUEST is a Limine protocol request static. The response
    // pointer is written by Limine before the kernel entry point is called,
    // so no data race exists at the point this function is invoked (single
    // hart, no other code running). Null check guards against absent response.
    unsafe {
        let response = DTB_REQUEST.response;
        if response.is_null() {
            return None;
        }
        let dtb = (*response).dtb_ptr;
        if dtb.is_null() {
            return None;
        }
        Some(dtb as usize)
    }
}

/// RSDP (Root System Description Pointer) request — x86_64 only.
///
/// Limine provides the RSDP physical address via this request.
/// On x86_64 UEFI boots, the RSDP lives in EFI memory; Limine locates it
/// from the UEFI System Table or by scanning the legacy BIOS ROM range.
///
/// Request GUID (after LIMINE_COMMON_MAGIC):
///   0xc5e77b6b397e7b43, 0x27637845accdcf3c
/// Verify against: https://github.com/limine-bootloader/limine/blob/v8.x-binary/limine.h
#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct LimineRsdpRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineRsdpResponse,
}

#[cfg(target_arch = "x86_64")]
unsafe impl Send for LimineRsdpRequest {}
#[cfg(target_arch = "x86_64")]
unsafe impl Sync for LimineRsdpRequest {}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct LimineRsdpResponse {
    pub revision: u64,
    /// Physical address of the RSDP structure.
    pub address: u64,
}

#[cfg(target_arch = "x86_64")]
#[used]
#[link_section = ".requests"]
static mut RSDP_REQUEST: LimineRsdpRequest = LimineRsdpRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0xc5e77b6b397e7b43,
        0x27637845accdcf3c,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Get the RSDP physical address from the Limine RSDP response.
///
/// Returns `None` when:
/// - not x86_64 (compile-time), or
/// - Limine did not populate an RSDP response (UEFI firmware has no ACPI),
/// - the response address is null/zero.
///
/// The kernel's x86_64 boot block calls this before `init_kernel_paging_x86`
/// to parse ACPI tables for MMIO base addresses.
#[cfg(target_arch = "x86_64")]
pub fn get_rsdp_ptr() -> Option<usize> {
    // SAFETY: RSDP_REQUEST is a Limine protocol request static. The response
    // pointer is written by Limine before the kernel entry point is called,
    // so no data race exists at the point this function is invoked (single
    // CPU, no other code running). Null check guards against absent response.
    unsafe {
        let response = RSDP_REQUEST.response;
        if response.is_null() {
            return None;
        }
        let addr = (*response).address as usize;
        if addr == 0 {
            None
        } else {
            Some(addr)
        }
    }
}

/// SMBIOS request — x86_64 only.
///
/// Under base revision 3 Limine reports both entry points as physical
/// addresses; zero means that entry point is absent, and no response means
/// firmware published neither. Layout and GUID match the vendored Limine 8.7.0
/// `limine.h` (`LIMINE_SMBIOS_REQUEST`, `struct limine_smbios_response`).
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineSmbiosRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineSmbiosResponse,
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineSmbiosResponse {
    revision: u64,
    entry_32: u64,
    entry_64: u64,
}

#[cfg(target_arch = "x86_64")]
#[used]
#[link_section = ".requests"]
static mut SMBIOS_REQUEST: LimineSmbiosRequest = LimineSmbiosRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0x9e9046f11e095391,
        0xaa4a520fefbde5ee,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// Physical addresses of the 32-bit and 64-bit SMBIOS entry points, in that
/// order; each is `None` when Limine reported it absent.
#[cfg(target_arch = "x86_64")]
pub fn get_smbios_entry_points() -> (Option<usize>, Option<usize>) {
    // SAFETY: Limine writes the response pointer before kernel entry and
    // nothing writes it afterwards.
    let response = unsafe { core::ptr::read_volatile(&raw const SMBIOS_REQUEST.response) };
    if !mp_record_valid(response, 1) {
        return (None, None);
    }
    // SAFETY: `mp_record_valid` checked alignment and that the whole response
    // lies in bootloader-reclaimable memory, which is never handed to the
    // frame allocator.
    let response = unsafe { &*response };
    let present = |address: u64| {
        usize::try_from(address)
            .ok()
            .filter(|address| *address != 0)
    };
    (present(response.entry_32), present(response.entry_64))
}

/// Executable-file request — x86_64 only; used for the kernel command line.
/// GUID and layout match the vendored Limine 8.7.0 `limine.h`
/// (`LIMINE_EXECUTABLE_FILE_REQUEST`, `struct limine_executable_file_response`).
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineExecutableFileRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineExecutableFileResponse,
}

#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineExecutableFileResponse {
    revision: u64,
    executable_file: *const LimineFilePrefix,
}

/// The leading fields of `struct limine_file` up to `cmdline`; the kernel
/// reads nothing after them.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
struct LimineFilePrefix {
    revision: u64,
    address: *const u8,
    size: u64,
    path: *const u8,
    cmdline: *const u8,
}

#[cfg(target_arch = "x86_64")]
#[used]
#[link_section = ".requests"]
static mut EXECUTABLE_FILE_REQUEST: LimineExecutableFileRequest = LimineExecutableFileRequest {
    id: [
        LIMINE_COMMON_MAGIC[0],
        LIMINE_COMMON_MAGIC[1],
        0xad97e90e83f1ed67,
        0x31eb5d1c5ff23b69,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// The kernel command line from `limine.conf` (`cmdline:`), without its NUL.
///
/// At most `limit` bytes are returned: a line that has no NUL within `limit`
/// bytes comes back exactly `limit` bytes long, so a caller passing one more
/// than its own maximum can tell "too long" from "fits". `None` means Limine
/// gave no executable-file response, or the string does not lie inside
/// bootloader-reclaimable memory.
#[cfg(target_arch = "x86_64")]
pub fn get_executable_cmdline(limit: usize) -> Option<&'static [u8]> {
    // SAFETY: Limine writes the response pointer before kernel entry and
    // nothing writes it afterwards.
    let response = unsafe { core::ptr::read_volatile(&raw const EXECUTABLE_FILE_REQUEST.response) };
    if !mp_record_valid(response, 1) {
        return None;
    }
    // SAFETY: validated as a complete bootloader-reclaimable object above.
    let file = unsafe { (*response).executable_file };
    if !mp_record_valid(file, 1) {
        return None;
    }
    // SAFETY: as above, for the file record.
    let cmdline = unsafe { (*file).cmdline };
    bootloader_bytes(cmdline, limit)
}

/// The NUL-terminated string at `pointer`, capped at `limit` bytes, provided
/// every byte read lies in one bootloader-reclaimable memory-map entry.
#[cfg(target_arch = "x86_64")]
fn bootloader_bytes(pointer: *const u8, limit: usize) -> Option<&'static [u8]> {
    let start = pointer as usize;
    let physical = (start as u64).checked_sub(get_hhdm_offset()?)?;
    let map = get_memory_map()?;
    // SAFETY: Limine's memory-map response is a mandatory trusted boot input.
    let readable = unsafe {
        (0..map.entry_count as usize).find_map(|index| {
            let entry = &**map.entries.add(index);
            let end = entry.base.checked_add(entry.length)?;
            (entry.entry_type == 5 && physical >= entry.base && physical < end)
                .then(|| end - physical)
        })
    }?;
    let window = usize::try_from(readable).ok()?.min(limit);
    // SAFETY: `[start, start + window)` lies inside one bootloader-reclaimable
    // entry, which the HHDM maps and the frame allocator never receives.
    let bytes = unsafe { core::slice::from_raw_parts(pointer, window) };
    match bytes.iter().position(|byte| *byte == 0) {
        Some(len) => Some(&bytes[..len]),
        // The string runs to the cap: report the capped prefix (too long).
        None if window == limit => Some(bytes),
        // The string runs off its memory-map entry: not a valid string.
        None => None,
    }
}

/// Limine memory map entry types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum LimineMemoryType {
    Usable = 0,
    Reserved = 1,
    AcpiReclaimable = 2,
    AcpiNvs = 3,
    BadMemory = 4,
    BootloaderReclaimable = 5,
    KernelAndModules = 6,
    Framebuffer = 7,
}

impl LimineMemoryMapEntry {
    pub fn memory_type(&self) -> LimineMemoryType {
        match self.entry_type {
            0 => LimineMemoryType::Usable,
            1 => LimineMemoryType::Reserved,
            2 => LimineMemoryType::AcpiReclaimable,
            3 => LimineMemoryType::AcpiNvs,
            4 => LimineMemoryType::BadMemory,
            5 => LimineMemoryType::BootloaderReclaimable,
            6 => LimineMemoryType::KernelAndModules,
            7 => LimineMemoryType::Framebuffer,
            _ => LimineMemoryType::Reserved,
        }
    }
}
