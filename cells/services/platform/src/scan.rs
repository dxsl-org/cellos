//! ECAM bus-0 scanner for the Platform Cell.
//!
//! Walks all 32 device slots on bus 0 and registers each discovered MMIO BAR
//! with the kernel via `sys_register_pcie_bar`. This lets Driver Cells
//! subsequently claim individual BARs through `sys_request_mmio`.
//!
//! BAR size probing follows PCI 3.0 §6.2.5.1: write 0xFFFFFFFF, read back,
//! mask lower 4 bits, compute `~mask + 1`. Memory decode is disabled in the
//! command register before each probe and restored after to prevent the device
//! from responding to MMIO accesses during the transient probe window.

use ostd::io::println;
use ostd::mmio::MmioRegion;
use ostd::syscall::{sys_register_pci_device, sys_register_pcie_bar};

// PCI type-0 config space offsets.
const CFG_VENDOR_ID: usize = 0x00;
const CFG_DEVICE_ID: usize = 0x02;
const CFG_COMMAND: usize = 0x04;
const CFG_CLASS_PROG: usize = 0x09;
const CFG_SUBCLASS: usize = 0x0A;
const CFG_CLASS_CODE: usize = 0x0B;
const CFG_HEADER_TYPE: usize = 0x0E;
const CFG_BAR0: usize = 0x10;

// Command register bit — disable MMIO decode during BAR size probe.
const CMD_MEM_SPACE: u16 = 1 << 1;

// ── low-level config-space accessors (all return a fallback on bounds error) ─

trait ConfigSpace {
    fn read_u32(&self, offset: usize) -> Option<u32>;
    fn read_u16(&self, offset: usize) -> Option<u16>;
    fn read_u8(&self, offset: usize) -> Option<u8>;
    fn write_u32(&self, offset: usize, value: u32) -> Option<()>;
    fn write_u16(&self, offset: usize, value: u16) -> Option<()>;
}

impl ConfigSpace for MmioRegion {
    fn read_u32(&self, offset: usize) -> Option<u32> {
        MmioRegion::read_u32(self, offset).ok()
    }

    fn read_u16(&self, offset: usize) -> Option<u16> {
        MmioRegion::read::<u16>(self, offset).ok()
    }

    fn read_u8(&self, offset: usize) -> Option<u8> {
        MmioRegion::read::<u8>(self, offset).ok()
    }

    fn write_u32(&self, offset: usize, value: u32) -> Option<()> {
        MmioRegion::write_u32(self, offset, value).ok()
    }

    fn write_u16(&self, offset: usize, value: u16) -> Option<()> {
        MmioRegion::write::<u16>(self, offset, value).ok()
    }
}

fn r32<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, off: usize) -> u32 {
    r.read_u32(cfg_off(dev, fun, off)).unwrap_or(0xFFFF_FFFF)
}

fn r16<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, off: usize) -> u16 {
    r.read_u16(cfg_off(dev, fun, off)).unwrap_or(0xFFFF)
}

fn r8<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, off: usize) -> u8 {
    r.read_u8(cfg_off(dev, fun, off)).unwrap_or(0xFF)
}

fn w32<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, off: usize, v: u32) {
    let _ = r.write_u32(cfg_off(dev, fun, off), v);
}

fn w16<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, off: usize, v: u16) {
    let _ = r.write_u16(cfg_off(dev, fun, off), v);
}

/// ECAM formula for bus 0: `(dev << 15) | (fun << 12) | off`
#[inline(always)]
fn cfg_off(dev: u8, fun: u8, off: usize) -> usize {
    ((dev as usize) << 15) | ((fun as usize) << 12) | off
}

// ── BAR size probing ──────────────────────────────────────────────────────────

fn bar_size32(mask_value: u32) -> u32 {
    let mask = mask_value & 0xFFFF_FFF0;
    if mask == 0 {
        0
    } else {
        (!mask).wrapping_add(1)
    }
}

fn bar_size64(mask_low: u32, mask_high: u32) -> u64 {
    let mask = ((mask_high as u64) << 32) | ((mask_low & 0xFFFF_FFF0) as u64);
    if mask == 0 {
        0
    } else {
        (!mask).wrapping_add(1)
    }
}

/// Probe size of a 32-bit MMIO BAR via the write-all-ones / read-back method.
fn probe32<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, bar_idx: usize) -> u32 {
    let off = CFG_BAR0 + bar_idx * 4;
    let orig_cmd = r16(r, dev, fun, CFG_COMMAND);
    let orig_bar = r32(r, dev, fun, off);
    // Disable memory decode before touching the BAR.
    w16(r, dev, fun, CFG_COMMAND, orig_cmd & !CMD_MEM_SPACE);
    w32(r, dev, fun, off, 0xFFFF_FFFF);
    let rb = r32(r, dev, fun, off);
    // Restore BAR and command register.
    w32(r, dev, fun, off, orig_bar);
    w16(r, dev, fun, CFG_COMMAND, orig_cmd);
    bar_size32(rb)
}

/// Probe size of a 64-bit MMIO BAR (low + high dword pair).
fn probe64<R: ConfigSpace + ?Sized>(r: &R, dev: u8, fun: u8, bar_idx: usize) -> u64 {
    let off_lo = CFG_BAR0 + bar_idx * 4;
    let off_hi = CFG_BAR0 + (bar_idx + 1) * 4;
    let orig_cmd = r16(r, dev, fun, CFG_COMMAND);
    let orig_lo = r32(r, dev, fun, off_lo);
    let orig_hi = r32(r, dev, fun, off_hi);
    w16(r, dev, fun, CFG_COMMAND, orig_cmd & !CMD_MEM_SPACE);
    w32(r, dev, fun, off_lo, 0xFFFF_FFFF);
    w32(r, dev, fun, off_hi, 0xFFFF_FFFF);
    let rb_lo = r32(r, dev, fun, off_lo);
    let rb_hi = r32(r, dev, fun, off_hi);
    w32(r, dev, fun, off_lo, orig_lo);
    w32(r, dev, fun, off_hi, orig_hi);
    w16(r, dev, fun, CFG_COMMAND, orig_cmd);
    bar_size64(rb_lo, rb_hi)
}

// ── Public scanner entry point ────────────────────────────────────────────────

/// Walk bus 0, decode all type-0 MMIO BARs, and register each non-zero BAR
/// with the kernel via `sys_register_pcie_bar`.
///
/// After this call returns, Driver Cells can claim individual BARs through
/// `sys_request_mmio` backed by `PcieDriverCap`.
pub fn scan_and_register(region: &MmioRegion) {
    for dev in 0u8..32 {
        if r16(region, dev, 0, CFG_VENDOR_ID) == 0xFFFF {
            continue; // slot empty
        }
        let hdr = r8(region, dev, 0, CFG_HEADER_TYPE);
        let max_f = if hdr & 0x80 != 0 { 8u8 } else { 1u8 };

        for fun in 0u8..max_f {
            let vendor_id = r16(region, dev, fun, CFG_VENDOR_ID);
            if vendor_id == 0xFFFF {
                continue;
            }
            // Skip PCI-to-PCI bridges (header type 1) — they have no BARs.
            if r8(region, dev, fun, CFG_HEADER_TYPE) & 0x7F != 0 {
                continue;
            }

            let bdf: u32 = ((dev as u32) << 3) | (fun as u32);
            let class = r8(region, dev, fun, CFG_CLASS_CODE);
            let subclass = r8(region, dev, fun, CFG_SUBCLASS);
            let prog_if = r8(region, dev, fun, CFG_CLASS_PROG);
            let device_id = r16(region, dev, fun, CFG_DEVICE_ID);
            if class == 0x02
                && subclass == 0x00
                && prog_if == 0x00
                && (vendor_id != 0x8086 || device_id != 0x100E)
            {
                println("[platform] unsupported Ethernet identity — registration skipped");
                continue;
            }
            let cls: u32 = ((class as u32) << 16) | ((subclass as u32) << 8) | (prog_if as u32);

            let mut bar0_base: usize = 0;
            let mut bar0_size: usize = 0;

            let mut i = 0usize;
            while i < 6 {
                let raw = r32(region, dev, fun, CFG_BAR0 + i * 4);
                if raw & 1 == 1 {
                    // I/O port BAR — skip.
                    i += 1;
                    continue;
                }
                let bar_type = (raw >> 1) & 0x3;
                if bar_type == 0x2 && i + 1 < 6 {
                    // 64-bit MMIO BAR: spans two slots.
                    let raw_hi = r32(region, dev, fun, CFG_BAR0 + (i + 1) * 4);
                    let base = ((raw as u64) & !0xF) | ((raw_hi as u64) << 32);
                    let size = probe64(region, dev, fun, i);
                    if base != 0 && size != 0 {
                        let _ = sys_register_pcie_bar(bdf, base as usize, size as usize);
                        if i == 0 {
                            bar0_base = base as usize;
                            bar0_size = size as usize;
                        }
                    }
                    i += 2;
                } else {
                    // 32-bit MMIO BAR.
                    let base = (raw & !0xF) as usize;
                    let size = probe32(region, dev, fun, i) as usize;
                    if base != 0 && size != 0 {
                        let _ = sys_register_pcie_bar(bdf, base, size);
                        if i == 0 {
                            bar0_base = base;
                            bar0_size = size;
                        }
                    }
                    i += 1;
                }
            }

            // Register class/BAR0 info so the kernel PCI_DEVICES list is populated
            // and sys_find_pcie_device queries work without a kernel ECAM scan.
            let _ = sys_register_pci_device(bdf, cls, bar0_base, bar0_size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::cell::{Cell, RefCell};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Read16(usize),
        Read32(usize),
        Write16(usize, u16),
        Write32(usize, u32),
    }

    struct FakeConfig {
        command: Cell<u16>,
        bars: RefCell<[u32; 6]>,
        masks: [u32; 6],
        events: RefCell<Vec<Event>>,
    }

    impl FakeConfig {
        fn new(command: u16, bars: [u32; 6], masks: [u32; 6]) -> Self {
            Self {
                command: Cell::new(command),
                bars: RefCell::new(bars),
                masks,
                events: RefCell::new(Vec::new()),
            }
        }

        fn bar_index(offset: usize) -> Option<usize> {
            let relative = offset.checked_sub(CFG_BAR0)?;
            if relative < 6 * 4 && relative % 4 == 0 {
                Some(relative / 4)
            } else {
                None
            }
        }
    }

    impl ConfigSpace for FakeConfig {
        fn read_u32(&self, offset: usize) -> Option<u32> {
            self.events.borrow_mut().push(Event::Read32(offset));
            let index = Self::bar_index(offset)?;
            let value = self.bars.borrow()[index];
            Some(if value == u32::MAX {
                self.masks[index]
            } else {
                value
            })
        }

        fn read_u16(&self, offset: usize) -> Option<u16> {
            self.events.borrow_mut().push(Event::Read16(offset));
            (offset == CFG_COMMAND).then(|| self.command.get())
        }

        fn read_u8(&self, _offset: usize) -> Option<u8> {
            None
        }

        fn write_u32(&self, offset: usize, value: u32) -> Option<()> {
            self.events.borrow_mut().push(Event::Write32(offset, value));
            let index = Self::bar_index(offset)?;
            self.bars.borrow_mut()[index] = value;
            Some(())
        }

        fn write_u16(&self, offset: usize, value: u16) -> Option<()> {
            self.events.borrow_mut().push(Event::Write16(offset, value));
            if offset != CFG_COMMAND {
                return None;
            }
            self.command.set(value);
            Some(())
        }
    }

    #[test]
    fn bar_mask_decoding_covers_absent_and_sized_bars() {
        assert_eq!(bar_size32(0), 0);
        assert_eq!(bar_size32(0xFFFF_F00F), 0x1000);
        assert_eq!(bar_size32(0xFFFF_C008), 0x4000);
        assert_eq!(bar_size32(0xFFFE_0000), 0x2_0000);

        assert_eq!(bar_size64(0, 0), 0);
        assert_eq!(bar_size64(0xFFE0_000C, 0xFFFF_FFFF), 0x20_0000);
    }

    #[test]
    fn probe32_disables_decode_and_restores_bar_and_command() {
        let original_bar = 0xFEBC_0008;
        let config = FakeConfig::new(
            0x0007,
            [original_bar, 0, 0, 0, 0, 0],
            [0xFFFE_0008, 0, 0, 0, 0, 0],
        );

        assert_eq!(probe32(&config, 0, 0, 0), 0x2_0000);
        assert_eq!(config.command.get(), 0x0007);
        assert_eq!(config.bars.borrow()[0], original_bar);
        assert_eq!(
            config.events.borrow().as_slice(),
            &[
                Event::Read16(CFG_COMMAND),
                Event::Read32(CFG_BAR0),
                Event::Write16(CFG_COMMAND, 0x0005),
                Event::Write32(CFG_BAR0, u32::MAX),
                Event::Read32(CFG_BAR0),
                Event::Write32(CFG_BAR0, original_bar),
                Event::Write16(CFG_COMMAND, 0x0007),
            ]
        );
    }

    #[test]
    fn probe64_disables_decode_and_restores_both_dwords() {
        let original_low = 0x1000_000C;
        let original_high = 0x0000_0001;
        let config = FakeConfig::new(
            0x0007,
            [original_low, original_high, 0, 0, 0, 0],
            [0xFFE0_000C, 0xFFFF_FFFF, 0, 0, 0, 0],
        );

        assert_eq!(probe64(&config, 0, 0, 0), 0x20_0000);
        assert_eq!(config.command.get(), 0x0007);
        assert_eq!(&config.bars.borrow()[..2], &[original_low, original_high]);
        assert_eq!(
            config.events.borrow().as_slice(),
            &[
                Event::Read16(CFG_COMMAND),
                Event::Read32(CFG_BAR0),
                Event::Read32(CFG_BAR0 + 4),
                Event::Write16(CFG_COMMAND, 0x0005),
                Event::Write32(CFG_BAR0, u32::MAX),
                Event::Write32(CFG_BAR0 + 4, u32::MAX),
                Event::Read32(CFG_BAR0),
                Event::Read32(CFG_BAR0 + 4),
                Event::Write32(CFG_BAR0, original_low),
                Event::Write32(CFG_BAR0 + 4, original_high),
                Event::Write16(CFG_COMMAND, 0x0007),
            ]
        );
    }
}
