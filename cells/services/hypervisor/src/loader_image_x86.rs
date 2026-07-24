//! x86 PVH guest loader: uncompressed `vmlinux` ELF → guest RAM.
//!
//! Parses the ELF64 header + program headers, discovers the PVH entry point
//! from the `XEN_ELFNOTE_PHYS32_ENTRY` note (name `"Xen"`, type 18) that Alpine
//! ships in the uncompressed `vmlinux` (not the `vmlinuz` bzImage), and streams
//! every `PT_LOAD` segment to its physical load address in guest RAM.
//!
//! VIFS1 file handles are stateless — each `read_cap` re-seeks the FAT chain
//! from the start, so there is no random access. The header/phdrs/notes are
//! read from a bounded prefix; `PT_LOAD` bytes are then routed to segments in a
//! single sequential pass over the file.

extern crate alloc;
use alloc::vec::Vec;
use types::{ViError, ViResult};

/// Prefix bytes read to locate the ELF header, program headers, and PVH note.
const PREFIX_BYTES: usize = 256 * 1024;
const PT_LOAD: u32 = 1;
const PT_NOTE: u32 = 4;
const XEN_PHYS32_ENTRY: u32 = 18;

/// One loadable segment: copy `[file_off, file_off+filesz)` → `paddr`, then
/// zero-fill up to `memsz` (bss).
#[derive(Clone, Copy)]
pub struct PtLoad {
    pub file_off: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
}

/// Largest PT_NOTE segment the loader will buffer while searching for the PVH
/// entry (the real `.notes` is a few hundred bytes; this bounds a bad header).
const MAX_NOTE_BYTES: u64 = 64 * 1024;

/// Parsed vmlinux layout.
pub struct ElfInfo {
    pub loads: Vec<PtLoad>,
    /// File offset + length of the PT_NOTE segment carrying the PVH note. In a
    /// real `vmlinux` this sits tens of MiB into the file (inside the first
    /// PT_LOAD), so the entry is resolved during [`load_segments`]'s streaming
    /// pass rather than from the bounded header prefix.
    note_off: u64,
    note_len: u64,
}

/// Parse the ELF header + program headers from `path`.
///
/// # Errors
/// [`ViError::NotFound`] if the file cannot be opened; [`ViError::InvalidInput`]
/// if it is not an x86-64 ELF or declares no loadable segments.
pub fn parse_headers(path: &str) -> ViResult<ElfInfo> {
    let buf = read_prefix(path, PREFIX_BYTES)?;
    if buf.len() < 64 || &buf[0..4] != b"\x7fELF" {
        return Err(ViError::InvalidInput);
    }
    // e_machine (0x12) must be x86-64 (62); ELF64 little-endian assumed.
    if rd16(&buf, 0x12)? != 62 {
        return Err(ViError::InvalidInput);
    }
    let e_phoff = rd64(&buf, 0x20)? as usize;
    let e_phnum = rd16(&buf, 0x38)? as usize;
    let e_phentsize = rd16(&buf, 0x36)? as usize;

    let mut loads = Vec::new();
    let mut note_off = 0u64;
    let mut note_len = 0u64;

    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 56 > buf.len() {
            break;
        }
        match rd32(&buf, ph)? {
            PT_LOAD => loads.push(PtLoad {
                file_off: rd64(&buf, ph + 8)?,
                paddr: rd64(&buf, ph + 24)?,
                filesz: rd64(&buf, ph + 32)?,
                memsz: rd64(&buf, ph + 40)?,
            }),
            PT_NOTE => {
                let len = rd64(&buf, ph + 32)?;
                if len <= MAX_NOTE_BYTES && (note_len == 0 || len < note_len) {
                    note_off = rd64(&buf, ph + 8)?;
                    note_len = len;
                }
            }
            _ => {}
        }
    }

    if loads.is_empty() {
        return Err(ViError::InvalidInput);
    }
    Ok(ElfInfo {
        loads,
        note_off,
        note_len,
    })
}

/// Stream every `PT_LOAD` segment into guest RAM via `write_fn(gpa, bytes)`,
/// zero each segment's bss tail, and return the PVH `PHYS32_ENTRY` captured from
/// the note segment during the same single sequential pass over the file.
///
/// # Errors
/// [`ViError::InvalidInput`] if no PHYS32_ENTRY note was found (⇒ the image is
/// not PVH-capable — supply an uncompressed `vmlinux`, not a `vmlinuz` bzImage).
pub fn load_segments<W>(path: &str, info: &ElfInfo, mut write_fn: W) -> ViResult<u64>
where
    W: FnMut(u64, &[u8]) -> ViResult<()>,
{
    let loads = &info.loads;
    let mut note_buf = alloc::vec![0u8; info.note_len as usize];
    let note_end = info.note_off + info.note_len;

    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| ViError::NotFound)?;
    let mut chunk = alloc::vec![0u8; 256 * 1024];
    let mut fpos: u64 = 0;
    loop {
        let n = match ostd::syscall::sys_read_cap(cap, &mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => {
                ostd::syscall::sys_close_cap(cap);
                return Err(ViError::IO);
            }
        };
        if let Err(e) = route_chunk(&chunk[..n], fpos, loads, &mut write_fn) {
            ostd::syscall::sys_close_cap(cap);
            return Err(e);
        }
        // Capture the note segment's bytes as they stream past.
        if info.note_len != 0 {
            let cend = fpos + n as u64;
            let lo = fpos.max(info.note_off);
            let hi = cend.min(note_end);
            if lo < hi {
                let dst = (lo - info.note_off) as usize;
                let src = (lo - fpos) as usize;
                note_buf[dst..dst + (hi - lo) as usize]
                    .copy_from_slice(&chunk[src..src + (hi - lo) as usize]);
            }
        }
        fpos += n as u64;
    }
    ostd::syscall::sys_close_cap(cap);

    // Zero-fill each segment's bss gap [paddr+filesz, paddr+memsz).
    let zeros = alloc::vec![0u8; 64 * 1024];
    for s in loads {
        let mut off = s.filesz;
        while off < s.memsz {
            let take = ((s.memsz - off) as usize).min(zeros.len());
            write_fn(s.paddr + off, &zeros[..take])?;
            off += take as u64;
        }
    }

    scan_notes(&note_buf, 0, note_buf.len()).ok_or(ViError::InvalidInput)
}

/// Route a file chunk `[fpos, fpos+len)` to the segments it overlaps.
fn route_chunk<W>(chunk: &[u8], fpos: u64, loads: &[PtLoad], write_fn: &mut W) -> ViResult<()>
where
    W: FnMut(u64, &[u8]) -> ViResult<()>,
{
    let cstart = fpos;
    let cend = fpos + chunk.len() as u64;
    for s in loads {
        let sstart = s.file_off;
        let send = s.file_off + s.filesz;
        let lo = cstart.max(sstart);
        let hi = cend.min(send);
        if lo < hi {
            let src = &chunk[(lo - cstart) as usize..(hi - cstart) as usize];
            write_fn(s.paddr + (lo - sstart), src)?;
        }
    }
    Ok(())
}

/// Scan an ELF64 note segment for the PVH `PHYS32_ENTRY` (name "Xen", type 18).
fn scan_notes(buf: &[u8], off: usize, len: usize) -> Option<u64> {
    let end = (off + len).min(buf.len());
    let mut p = off;
    while p + 12 <= end {
        let namesz = rd32(buf, p).ok()? as usize;
        let descsz = rd32(buf, p + 4).ok()? as usize;
        let ntype = rd32(buf, p + 8).ok()?;
        let name_off = p + 12;
        let desc_off = name_off + align4(namesz);
        if desc_off + descsz > end {
            break;
        }
        if ntype == XEN_PHYS32_ENTRY && namesz >= 3 && &buf[name_off..name_off + 3] == b"Xen" {
            return Some(if descsz >= 8 {
                rd64(buf, desc_off).ok()?
            } else {
                rd32(buf, desc_off).ok()? as u64
            });
        }
        p = desc_off + align4(descsz);
    }
    None
}

#[inline]
fn align4(n: usize) -> usize {
    (n + 3) & !3
}

/// Read up to `max` bytes from the start of a VIFS1 file.
fn read_prefix(path: &str, max: usize) -> ViResult<Vec<u8>> {
    let cap = ostd::syscall::sys_open_cap(path).map_err(|_| ViError::NotFound)?;
    let mut out = Vec::with_capacity(max);
    let mut chunk = alloc::vec![0u8; 64 * 1024];
    while out.len() < max {
        match ostd::syscall::sys_read_cap(cap, &mut chunk) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&chunk[..n]),
            Err(_) => {
                ostd::syscall::sys_close_cap(cap);
                return Err(ViError::IO);
            }
        }
    }
    ostd::syscall::sys_close_cap(cap);
    Ok(out)
}

#[inline]
fn rd16(b: &[u8], o: usize) -> ViResult<u16> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ViError::InvalidInput)
}

#[inline]
fn rd32(b: &[u8], o: usize) -> ViResult<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ViError::InvalidInput)
}

#[inline]
fn rd64(b: &[u8], o: usize) -> ViResult<u64> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
        .ok_or(ViError::InvalidInput)
}
