//! Trap-emulated GICv2 Distributor (GICD) shadow register model.
//!
//! Security contract (M5, Red Team): this module ONLY maintains a shadow register
//! file. It NEVER issues direct MMIO writes to the physical GICD at 0x08000000.
//! Physical GIC access stays exclusively with the HAL GIC driver in the kernel.
//!
//! Linux probes GICD during `gic_init_bases` to read TYPER (IRQ count) and IIDR
//! (implementer). It then programs ISENABLER/IPRIORITYR/ITARGETSR for each IRQ.
//! We emulate just enough for a clean init: writes are accepted silently; reads
//! return safe/plausible values.

/// GICD base IPA.
pub const GICD_BASE_IPA: u64 = 0x0800_0000;
pub const GICD_SIZE: u64 = 0x0001_0000; // 64 KiB

/// GICC base IPA (virtual CPU interface — also emulated as pass-through silent).
pub const GICC_BASE_IPA: u64 = 0x0801_0000;
pub const GICC_SIZE: u64 = 0x0001_0000;

/// GICD register offsets.
mod reg {
    pub const GICD_CTLR: u64 = 0x000;
    pub const GICD_TYPER: u64 = 0x004; // read: (ITLinesNumber-1) | ...
    pub const GICD_IIDR: u64 = 0x008; // implementer ID
    pub const GICD_ISENABLER: u64 = 0x100; // [0..31]: enable-set per IRQ word
    pub const GICD_ICENABLER: u64 = 0x180; // clear-enable
    pub const GICD_ISPENDR: u64 = 0x200; // set-pending
    pub const GICD_ICPENDR: u64 = 0x280; // clear-pending
    pub const GICD_IPRIORITYR: u64 = 0x400; // priority byte per IRQ
    pub const GICD_ITARGETSR: u64 = 0x800; // target CPU mask byte per IRQ
    pub const GICD_ICFGR: u64 = 0xC00; // level/edge config
    pub const GICD_SGIR: u64 = 0xF00; // software generated IRQ
}

/// Maximum IRQ lines emulated (GICv2 supports up to 1020).
const MAX_IRQS: usize = 256;
const WORD_COUNT: usize = MAX_IRQS / 32; // 8 words for enable/pending

/// GICv2 Distributor shadow state.
pub struct Gicd {
    ctlr: u32,
    isenabler: [u32; WORD_COUNT],
    // ICENABLER writes clear bits directly in `isenabler` (matching real GICv2
    // semantics: ISENABLER/ICENABLER are two write-only views onto the same
    // underlying enable-bit state). This field is therefore write-only shadow
    // state at the current emulation depth — never read back.
    #[allow(dead_code)]
    icenabler: [u32; WORD_COUNT],
    ispendr: [u32; WORD_COUNT],
    ipriorityr: [u8; MAX_IRQS],
    itargetsr: [u8; MAX_IRQS],
    icfgr: [u32; MAX_IRQS / 16],
}

impl Gicd {
    pub const fn new() -> Self {
        Self {
            ctlr: 0,
            isenabler: [0u32; WORD_COUNT],
            icenabler: [0u32; WORD_COUNT],
            ispendr: [0u32; WORD_COUNT],
            ipriorityr: [0xa0u8; MAX_IRQS], // default: low priority
            itargetsr: [0x01u8; MAX_IRQS],  // default: CPU 0
            icfgr: [0u32; MAX_IRQS / 16],
        }
    }

    /// Handle a GICD MMIO write.
    pub fn write(&mut self, offset: u64, val: u64, size: u8) {
        let val32 = val as u32;
        match offset {
            reg::GICD_CTLR => {
                self.ctlr = val32;
            }
            o if o >= reg::GICD_ISENABLER && o < reg::GICD_ICENABLER => {
                let idx = ((o - reg::GICD_ISENABLER) / 4) as usize;
                if idx < WORD_COUNT {
                    self.isenabler[idx] |= val32;
                }
            }
            o if o >= reg::GICD_ICENABLER && o < reg::GICD_ISPENDR => {
                let idx = ((o - reg::GICD_ICENABLER) / 4) as usize;
                if idx < WORD_COUNT {
                    self.isenabler[idx] &= !val32;
                }
            }
            o if o >= reg::GICD_ISPENDR && o < reg::GICD_ICPENDR => {
                let idx = ((o - reg::GICD_ISPENDR) / 4) as usize;
                if idx < WORD_COUNT {
                    self.ispendr[idx] |= val32;
                }
            }
            o if o >= reg::GICD_ICPENDR && o < reg::GICD_IPRIORITYR => {
                let idx = ((o - reg::GICD_ICPENDR) / 4) as usize;
                if idx < WORD_COUNT {
                    self.ispendr[idx] &= !val32;
                }
            }
            o if o >= reg::GICD_IPRIORITYR && o < reg::GICD_ITARGETSR => {
                let base = (o - reg::GICD_IPRIORITYR) as usize;
                self.write_bytes(&mut self.ipriorityr.clone(), base, val, size);
                // Workaround: update in place
                let bytes = val.to_le_bytes();
                let count = size as usize;
                for i in 0..count {
                    if base + i < MAX_IRQS {
                        self.ipriorityr[base + i] = bytes[i];
                    }
                }
            }
            o if o >= reg::GICD_ITARGETSR && o < reg::GICD_ICFGR => {
                let base = (o - reg::GICD_ITARGETSR) as usize;
                let bytes = val.to_le_bytes();
                let count = size as usize;
                for i in 0..count {
                    if base + i < MAX_IRQS {
                        self.itargetsr[base + i] = bytes[i];
                    }
                }
            }
            o if o >= reg::GICD_ICFGR && o < reg::GICD_SGIR => {
                let idx = ((o - reg::GICD_ICFGR) / 4) as usize;
                if idx < self.icfgr.len() {
                    self.icfgr[idx] = val32;
                }
            }
            _ => {} // SGIR and unknown: accept silently
        }
    }

    /// Handle a GICD MMIO read. Returns a plausible value.
    pub fn read(&self, offset: u64, size: u8) -> u64 {
        match offset {
            reg::GICD_CTLR => self.ctlr as u64,
            // TYPER: ITLinesNumber=7 → (7+1)*32=256 IRQs; 1 CPU; SecurityExtn=0.
            reg::GICD_TYPER => 0x0000_0007u64,
            // IIDR: ARM implementer, GICv2 product.
            reg::GICD_IIDR => 0x0200_143Bu64,
            o if o >= reg::GICD_ISENABLER && o < reg::GICD_ICENABLER => {
                let idx = ((o - reg::GICD_ISENABLER) / 4) as usize;
                if idx < WORD_COUNT {
                    self.isenabler[idx] as u64
                } else {
                    0
                }
            }
            o if o >= reg::GICD_IPRIORITYR && o < reg::GICD_ITARGETSR => {
                self.read_bytes(&self.ipriorityr, (o - reg::GICD_IPRIORITYR) as usize, size)
            }
            o if o >= reg::GICD_ITARGETSR && o < reg::GICD_ICFGR => {
                self.read_bytes(&self.itargetsr, (o - reg::GICD_ITARGETSR) as usize, size)
            }
            _ => 0,
        }
    }

    /// True if `ipa` falls within the GICD MMIO window.
    pub fn owns_gicd(ipa: u64) -> bool {
        ipa >= GICD_BASE_IPA && ipa < GICD_BASE_IPA + GICD_SIZE
    }

    /// True if `ipa` falls within the GICC MMIO window.
    pub fn owns_gicc(ipa: u64) -> bool {
        ipa >= GICC_BASE_IPA && ipa < GICC_BASE_IPA + GICC_SIZE
    }

    fn write_bytes(&self, _buf: &mut [u8], _base: usize, _val: u64, _size: u8) {}

    fn read_bytes(&self, buf: &[u8], base: usize, size: u8) -> u64 {
        let mut out = 0u64;
        let count = (size as usize).min(8);
        for i in 0..count {
            if base + i < buf.len() {
                out |= (buf[base + i] as u64) << (i * 8);
            }
        }
        out
    }
}
