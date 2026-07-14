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

use ostd::mmio::MmioRegion;
use ostd::syscall::{sys_register_pci_device, sys_register_pcie_bar};

// PCI type-0 config space offsets.
const CFG_VENDOR_ID: usize = 0x00;
const CFG_COMMAND: usize = 0x04;
const CFG_CLASS_PROG: usize = 0x09;
const CFG_SUBCLASS: usize = 0x0A;
const CFG_CLASS_CODE: usize = 0x0B;
const CFG_HEADER_TYPE: usize = 0x0E;
const CFG_BAR0: usize = 0x10;

// Command register bit — disable MMIO decode during BAR size probe.
const CMD_MEM_SPACE: u16 = 1 << 1;

// ── low-level config-space accessors (all return a fallback on bounds error) ─

fn r32(r: &MmioRegion, dev: u8, fun: u8, off: usize) -> u32 {
    r.read_u32(cfg_off(dev, fun, off)).unwrap_or(0xFFFF_FFFF)
}

fn r16(r: &MmioRegion, dev: u8, fun: u8, off: usize) -> u16 {
    r.read::<u16>(cfg_off(dev, fun, off)).unwrap_or(0xFFFF)
}

fn r8(r: &MmioRegion, dev: u8, fun: u8, off: usize) -> u8 {
    r.read::<u8>(cfg_off(dev, fun, off)).unwrap_or(0xFF)
}

fn w32(r: &MmioRegion, dev: u8, fun: u8, off: usize, v: u32) {
    let _ = r.write_u32(cfg_off(dev, fun, off), v);
}

fn w16(r: &MmioRegion, dev: u8, fun: u8, off: usize, v: u16) {
    let _ = r.write::<u16>(cfg_off(dev, fun, off), v);
}

/// ECAM formula for bus 0: `(dev << 15) | (fun << 12) | off`
#[inline(always)]
fn cfg_off(dev: u8, fun: u8, off: usize) -> usize {
    ((dev as usize) << 15) | ((fun as usize) << 12) | off
}

// ── BAR size probing ──────────────────────────────────────────────────────────

/// Probe size of a 32-bit MMIO BAR via the write-all-ones / read-back method.
fn probe32(r: &MmioRegion, dev: u8, fun: u8, bar_idx: usize) -> u32 {
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
    let mask = rb & 0xFFFF_FFF0;
    if mask == 0 {
        0
    } else {
        (!mask).wrapping_add(1)
    }
}

/// Probe size of a 64-bit MMIO BAR (low + high dword pair).
fn probe64(r: &MmioRegion, dev: u8, fun: u8, bar_idx: usize) -> u64 {
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
    let mask64 = ((rb_hi as u64) << 32) | ((rb_lo & 0xFFFF_FFF0) as u64);
    if mask64 == 0 {
        0
    } else {
        (!mask64).wrapping_add(1)
    }
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
            if r16(region, dev, fun, CFG_VENDOR_ID) == 0xFFFF {
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
