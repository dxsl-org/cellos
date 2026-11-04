//! Range-checked config-space access for one owned function, and the standard
//! capability walk shared by the 434 record and the 435 programming path.

use crate::task::drivers::pcie_ecam::config_ptr;
use alloc::vec::Vec;

pub const CFG_COMMAND: usize = 0x04;
pub const CFG_STATUS: usize = 0x06;
pub const CFG_REVISION: usize = 0x08;
pub const CFG_HEADER_TYPE: usize = 0x0E;
pub const CFG_BAR0: usize = 0x10;
pub const CFG_SUBSYSTEM_VENDOR: usize = 0x2C;
pub const CFG_SUBSYSTEM_ID: usize = 0x2E;
pub const CFG_CAP_PTR: usize = 0x34;
pub const STATUS_CAP_LIST: u16 = 1 << 4;
pub const CMD_MEM_SPACE: u16 = 1 << 1;
pub const CMD_BUS_MASTER: u16 = 1 << 2;
pub const CMD_INTX_DISABLE: u16 = 1 << 10;
pub const CAP_ID_MSI: u8 = 0x05;
pub const CAP_ID_MSIX: u8 = 0x11;
/// Same bound as the scanner's walk: a malformed circular list cannot loop.
const CAP_WALK_BUDGET: usize = 48;

/// Config space of one function. Every access is range-checked against the
/// admitted ECAM window and fails with `None` outside it.
#[derive(Clone, Copy)]
pub struct Config {
    bdf: u32,
}

impl Config {
    pub fn new(bdf: u32) -> Self {
        Self { bdf }
    }

    pub fn read8(&self, off: usize) -> Option<u8> {
        let ptr = config_ptr(self.bdf, off)?;
        // SAFETY: `config_ptr` returns an address inside the identity-mapped ECAM window.
        Some(unsafe { core::ptr::read_volatile(ptr) })
    }

    pub fn read16(&self, off: usize) -> Option<u16> {
        let ptr = config_ptr(self.bdf, off & !1)?;
        // SAFETY: as `read8`; the offset is 2-byte aligned.
        Some(unsafe { core::ptr::read_volatile(ptr as *const u16) })
    }

    pub fn read32(&self, off: usize) -> Option<u32> {
        let ptr = config_ptr(self.bdf, off & !3)?;
        // SAFETY: as `read8`; the offset is 4-byte aligned.
        Some(unsafe { core::ptr::read_volatile(ptr as *const u32) })
    }

    pub fn write16(&self, off: usize, value: u16) -> Option<()> {
        let ptr = config_ptr(self.bdf, off & !1)?;
        // SAFETY: as `read16`; the caller owns the function's configuration.
        unsafe { core::ptr::write_volatile(ptr as *mut u16, value) };
        Some(())
    }

    pub fn write32(&self, off: usize, value: u32) -> Option<()> {
        let ptr = config_ptr(self.bdf, off & !3)?;
        // SAFETY: as `read32`; the caller owns the function's configuration.
        unsafe { core::ptr::write_volatile(ptr as *mut u32, value) };
        Some(())
    }

    /// Standard capabilities `(id, offset)` in walk order; empty when the
    /// function has no capability list or does not answer.
    pub fn capabilities(&self) -> Vec<(u8, u8)> {
        let mut caps = Vec::new();
        if self
            .read16(CFG_STATUS)
            .is_none_or(|s| s == 0xFFFF || s & STATUS_CAP_LIST == 0)
        {
            return caps;
        }
        let mut at = self.read8(CFG_CAP_PTR).unwrap_or(0) & 0xFC;
        while at >= 0x40 && caps.len() < CAP_WALK_BUDGET {
            let (Some(id), Some(next)) = (self.read8(at as usize), self.read8(at as usize + 1))
            else {
                break;
            };
            caps.push((id, at));
            at = next & 0xFC;
        }
        caps
    }

    pub fn find_capability(&self, id: u8) -> Option<u8> {
        self.capabilities()
            .into_iter()
            .find_map(|(cap, at)| (cap == id).then_some(at))
    }
}
