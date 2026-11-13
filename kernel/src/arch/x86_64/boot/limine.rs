// SPDX-License-Identifier: MPL-2.0
//! x86_64 Limine requests: MP, RSDP, SMBIOS and the executable file (command
//! line). The shared requests and the response readers they build on stay in
//! `crate::boot::limine`.

use crate::boot::limine::{get_hhdm_offset, get_memory_map, LIMINE_COMMON_MAGIC};

// x86 MP revision 0 ABI adapted from Limine v8.x-binary/limine.h.
// Copyright (C) 2022-2025 mintsuki and contributors, BSD-0-Clause:
// https://raw.githubusercontent.com/limine-bootloader/limine/v8.x-binary/limine.h
// Protocol: https://github.com/limine-bootloader/limine/blob/v8.x/PROTOCOL.md
// Bootloader-reclaimable memory is never admitted to our frame allocator; these
// records and Limine's page tables therefore remain live through AP handoff.
#[repr(C)]
pub struct LimineMpInfo {
    pub processor_id: u32,
    pub lapic_id: u32,
    pub reserved: u64,
    pub goto_address: core::sync::atomic::AtomicUsize,
    pub extra_argument: u64,
}

#[repr(C)]
pub struct LimineMpResponse {
    pub revision: u64,
    pub flags: u32,
    pub bsp_lapic_id: u32,
    pub cpu_count: u64,
    pub cpus: *const *mut LimineMpInfo,
}

#[repr(C)]
struct LimineMpRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineMpResponse,
    flags: u64,
}

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
    // Bit 0: enable x2APIC where the CPU supports it (plan 261103-2218
    // phase 03). The kernel latches the mode per CPU (`apic::init_mode`).
    flags: 1,
};

const _: [(); 32] = [(); core::mem::size_of::<LimineMpInfo>()];

const _: [(); 32] = [(); core::mem::size_of::<LimineMpResponse>()];

/// A response pointer must name a complete, aligned bootloader-owned object.
pub fn mp_record_valid<T>(pointer: *const T, count: usize) -> bool {
    let start = pointer as usize;
    if start == 0 || !start.is_multiple_of(core::mem::align_of::<T>()) {
        return false;
    }
    let Some(size) = core::mem::size_of::<T>().checked_mul(count) else {
        return false;
    };
    let Some(end) = start.checked_add(size) else {
        return false;
    };
    let Some(offset) = get_hhdm_offset() else {
        return false;
    };
    let Some(physical) = (start as u64).checked_sub(offset) else {
        return false;
    };
    let Some(physical_end) = (end as u64).checked_sub(offset) else {
        return false;
    };
    let Some(map) = get_memory_map() else {
        return false;
    };
    // Limine's memory-map response is already a mandatory trusted boot input.
    unsafe {
        (0..map.entry_count as usize).any(|index| {
            let entry = &**map.entries.add(index);
            entry.entry_type == 5
                && physical >= entry.base
                && entry
                    .base
                    .checked_add(entry.length)
                    .is_some_and(|limit| physical_end <= limit)
        })
    }
}

pub fn get_mp_response() -> Option<&'static LimineMpResponse> {
    let response = unsafe { core::ptr::read_volatile(&raw const MP_REQUEST.response) };
    mp_record_valid(response, 1).then(|| unsafe { &*response })
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
#[repr(C)]
pub struct LimineRsdpRequest {
    pub id: [u64; 4],
    pub revision: u64,
    pub response: *const LimineRsdpResponse,
}

unsafe impl Send for LimineRsdpRequest {}

unsafe impl Sync for LimineRsdpRequest {}

#[repr(C)]
pub struct LimineRsdpResponse {
    pub revision: u64,
    /// Physical address of the RSDP structure.
    pub address: u64,
}

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
#[repr(C)]
struct LimineSmbiosRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineSmbiosResponse,
}

#[repr(C)]
struct LimineSmbiosResponse {
    revision: u64,
    entry_32: u64,
    entry_64: u64,
}

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
#[repr(C)]
struct LimineExecutableFileRequest {
    id: [u64; 4],
    revision: u64,
    response: *const LimineExecutableFileResponse,
}

#[repr(C)]
struct LimineExecutableFileResponse {
    revision: u64,
    executable_file: *const LimineFilePrefix,
}

/// The leading fields of `struct limine_file` up to `cmdline`; the kernel
/// reads nothing after them.
#[repr(C)]
struct LimineFilePrefix {
    revision: u64,
    address: *const u8,
    size: u64,
    path: *const u8,
    cmdline: *const u8,
}

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
