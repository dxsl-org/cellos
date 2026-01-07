//! ELF Parsing Logic
use types::*;
use super::{ElfParser, ElfHeader};
use xmas_elf::ElfFile;

pub struct ElfLoader;

impl ElfLoader {
    /// Load loadable segments into memory.
    /// This is not part of ElfParser trait but required for process loading.
    pub fn load_segments(&self, data: &[u8], frame_allocator: &mut crate::memory::frame::FrameAllocator) -> ViResult<()> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
        
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
                let file_offset = ph.offset() as usize;
                let vaddr = ph.virtual_addr() as usize;
                let mem_size = ph.mem_size() as usize;
                let file_size = ph.file_size() as usize;
                
                let start_addr = vaddr;
                let end_addr = vaddr + mem_size;
                
                // Align start/end to page boundaries
                let start_page = start_addr & !(4096 - 1);
                let end_page = (end_addr + 4096 - 1) & !(4096 - 1);
                
                // Map pages
                let mut current_page = start_page;
                while current_page < end_page {
                    // Allocate frame
                    let buf_frame = frame_allocator.allocate_frame().ok_or(ViError::OutOfMemory)?;
                    
                    // Map it
                    // Permissions: R/W/X + USER (for U-mode access)
                    // TODO: Parse ph.flags for more fine-grained permissions
                    use crate::memory::paging::Flags;
                    let flags_bits = Flags::READ | Flags::WRITE | Flags::EXECUTE | Flags::USER | Flags::ACCESSED | Flags::DIRTY;
                    let flags = Flags::from_bits(flags_bits);
                    
                    crate::memory::paging::map_page(frame_allocator, current_page, buf_frame, flags)
                        .map_err(|_| ViError::OutOfMemory)?;
                    
                    // Copy Data
                    // We can write to `buf_frame` directly because of Identity Mapping
                    // BUT: We need to know which part of the file goes here.
                    
                    let page_offset = current_page - start_page; // Offset from allocation start
                    // Actually, ELF loads relative to vaddr.
                    // If vaddr starts at 0x10050, first page 0x10000.
                    // We need to calculate overlaps.
                    
                    // Zero the frame first (simplifies BSS and padding)
                    unsafe {
                        core::ptr::write_bytes(buf_frame as *mut u8, 0, 4096);
                    }
                    
                    // Calculate intersection with file data
                    let page_start_vaz = current_page;
                    let page_end_vaz = current_page + 4096;
                    
                    // Intersection of [page_start_vaz, page_end_vaz) AND [vaddr, vaddr + file_size)
                    let copy_start_v = core::cmp::max(page_start_vaz, vaddr);
                    let copy_end_v = core::cmp::min(page_end_vaz, vaddr + file_size);
                    
                    if copy_start_v < copy_end_v {
                        let len = copy_end_v - copy_start_v;
                        let dst_offset = copy_start_v - page_start_vaz; // offset in page
                        
                        let src_offset_in_file = file_offset + (copy_start_v - vaddr);
                        if src_offset_in_file + len <= data.len() {
                            let src = &data[src_offset_in_file .. src_offset_in_file + len];
                            unsafe {
                                let dst = (buf_frame as *mut u8).add(dst_offset);
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, len);
                            }
                        }
                    }
                    
                    current_page += 4096;
                }
                
                log::info!("ELF LOAD: Segment loaded at 0x{:X}-0x{:X}", start_addr, end_addr);
            }
        }
        Ok(())
    }
}

impl ElfParser for ElfLoader {
    fn parse_header(&self, data: &[u8]) -> ViResult<ElfHeader> {
        let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
        
        // Verify architecture (RISC-V 64)
         // Header check is implicit in successful new(), but specific machine check?
         // elf.header.pt2.machine() == xmas_elf::header::Machine::RISC_V
        
        Ok(ElfHeader {
            entry: elf.header.pt2.entry_point() as usize,
            shoff: elf.header.pt2.sh_offset() as usize,
        })
    }
    
    fn get_section<'a>(&self, data: &'a [u8], name: &str) -> ViResult<&'a [u8]> {
         let elf = ElfFile::new(data).map_err(|_| ViError::InvalidInput)?;
         match elf.find_section_by_name(name) {
             Some(section) => Ok(section.raw_data(&elf)),
             None => Err(ViError::NotFound),
         }
    }
}
