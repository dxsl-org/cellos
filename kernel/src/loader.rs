//! kernel/src/loader.rs
//! Cell loader and linker interfaces (v2.2 - Modern Style).

use types::*; // Dùng VAddr, CellId, ViResult từ libs/types

// Khai báo các module con (Luật 5: foo.rs ngang hàng thư mục foo/)
pub mod elf; // Xử lý cấu trúc file ELF
pub use elf::ElfLoader;
pub mod reloc; // Xử lý vá địa chỉ (Relocation)

/// ELF parser trait - Đã sửa lỗi u64 -> usize để hỗ trợ RV32
pub trait ElfParser {
    /// Parse ELF header.
    fn parse_header(&self, data: &[u8]) -> ViResult<ElfHeader>;

    /// Get section by name.
    fn get_section<'a>(&self, data: &'a [u8], name: &str) -> ViResult<&'a [u8]>;
}

/// ELF header (v2.2 - Multi-Arch Ready)
pub struct ElfHeader {
    /// Entry point address. Dùng VAddr để tự nhảy theo kiến trúc (32/64 bit).
    pub entry: VAddr,
    /// Section header offset. Dùng usize vì offset phụ thuộc độ rộng bus.
    pub shoff: usize,
}

/// Symbol table entry - Sử dụng kiểu dữ liệu chuẩn từ libs/types
pub struct Symbol {
    pub name: &'static str,
    pub addr: VAddr,  // Đã đổi từ VirtAddr sang VAddr
    pub cell: CellId, // Định danh Cell sở hữu symbol này
}

/// Linker trait - Trái tim của quá trình nạp Cell
pub trait Linker {
    /// Load a Cell from object file. Trả về ViResult thay vì panic!
    fn load_cell(&mut self, data: &[u8]) -> ViResult<CellId>;

    /// Resolve a symbol by name.
    fn resolve_symbol(&self, name: &str) -> ViResult<VAddr>;

    /// Unload a Cell (Hỗ trợ Panic Recovery)
    fn unload_cell(&mut self, id: CellId) -> ViResult<()>;
}
