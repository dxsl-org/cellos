//! Trap-emulated GICv2 distributor. No access to the physical GIC registers.

pub const GICD_BASE_IPA: u64 = 0x0800_0000;
pub const GICD_SIZE: u64 = 0x0001_0000;
pub const GICC_BASE_IPA: u64 = 0x0801_0000;
pub const GICC_SIZE: u64 = 0x0001_0000;

const MAX_IRQS: usize = 256;
const WORDS: usize = MAX_IRQS / 32;

#[cfg(feature = "board-rpi3")]
use core::sync::atomic::{AtomicU32, Ordering};

// The device backends call vmm::inject_irq while the run loop owns the GICD.
// Coalescing matches the GIC pending-bit semantics; no interrupt is lost when
// multiple different devices complete before the next vCPU entry.
#[cfg(feature = "board-rpi3")]
static DEVICE_PENDING: [AtomicU32; WORDS] = [const { AtomicU32::new(0) }; WORDS];

#[cfg(feature = "board-rpi3")]
pub fn queue_device_irq(intid: u32) {
    let intid = intid as usize;
    if intid < MAX_IRQS {
        DEVICE_PENDING[intid / 32].fetch_or(1 << (intid % 32), Ordering::Release);
    }
}

pub struct Gicd {
    ctlr: u32,
    enabled: [u32; WORDS],
    pending: [u32; WORDS],
    active: [u32; WORDS],
    level: [u32; WORDS],
    priority: [u8; MAX_IRQS],
    target: [u8; MAX_IRQS],
    config: [u32; MAX_IRQS / 16],
}

impl Gicd {
    pub const fn new() -> Self {
        Self {
            ctlr: 0,
            enabled: [0; WORDS],
            pending: [0; WORDS],
            active: [0; WORDS],
            level: [0; WORDS],
            priority: [0xa0; MAX_IRQS],
            target: [1; MAX_IRQS],
            config: [0; MAX_IRQS / 16],
        }
    }

    #[cfg(feature = "board-rpi3")]
    pub fn collect_device_irqs(&mut self) {
        for (word, pending) in DEVICE_PENDING.iter().enumerate() {
            self.pending[word] |= pending.swap(0, Ordering::AcqRel);
        }
    }

    pub fn set_level(&mut self, intid: u32, asserted: bool) {
        if let Some((word, bit)) = Self::bit(intid) {
            if asserted {
                self.level[word] |= bit;
                if self.active[word] & bit == 0 {
                    self.pending[word] |= bit;
                }
            } else {
                self.level[word] &= !bit;
                self.pending[word] &= !bit;
            }
        }
    }

    pub fn highest_pending(&self, pmr: u8, running_priority: u8) -> Option<u32> {
        if self.ctlr & 1 == 0 {
            return None;
        }
        let mut best = None;
        let mut best_priority = pmr.min(running_priority);
        for word in 0..WORDS {
            let mut candidates = self.pending[word] & self.enabled[word] & !self.active[word];
            while candidates != 0 {
                let bit = candidates.trailing_zeros();
                candidates &= candidates - 1;
                let irq = word * 32 + bit as usize;
                let priority = self.priority[irq];
                if priority < best_priority && (irq < 32 || self.target[irq] & 1 != 0) {
                    best_priority = priority;
                    best = Some(irq as u32);
                }
            }
        }
        best
    }

    pub fn priority(&self, intid: u32) -> u8 {
        self.priority.get(intid as usize).copied().unwrap_or(0xff)
    }

    pub fn acknowledge(&mut self, intid: u32) {
        if let Some((word, bit)) = Self::bit(intid) {
            self.pending[word] &= !bit;
            self.active[word] |= bit;
        }
    }

    pub fn eoi(&mut self, intid: u32) {
        if let Some((word, bit)) = Self::bit(intid) {
            self.active[word] &= !bit;
            if self.level[word] & bit != 0 {
                self.pending[word] |= bit;
            }
        }
    }

    fn bit(intid: u32) -> Option<(usize, u32)> {
        ((intid as usize) < MAX_IRQS).then(|| {
            let intid = intid as usize;
            (intid / 32, 1 << (intid % 32))
        })
    }

    pub fn write(&mut self, offset: u64, val: u64, size: u8) {
        if !matches!(size, 1 | 2 | 4) {
            return;
        }
        let val = val as u32;
        match offset {
            0 => self.ctlr = val & 1,
            0x100..=0x11f => Self::write_bits(&mut self.enabled, offset - 0x100, val, size, true),
            0x180..=0x19f => Self::write_bits(&mut self.enabled, offset - 0x180, val, size, false),
            0x200..=0x21f => Self::write_bits(&mut self.pending, offset - 0x200, val, size, true),
            0x280..=0x29f => Self::write_bits(&mut self.pending, offset - 0x280, val, size, false),
            0x300..=0x31f => Self::write_bits(&mut self.active, offset - 0x300, val, size, true),
            0x380..=0x39f => Self::write_bits(&mut self.active, offset - 0x380, val, size, false),
            0x400..=0x4ff => Self::write_bytes(&mut self.priority, offset - 0x400, val, size),
            0x800..=0x8ff => Self::write_bytes(&mut self.target, offset - 0x800, val, size),
            0xc00..=0xc3f => {
                let index = ((offset - 0xc00) / 4) as usize;
                let shift = ((offset - 0xc00) % 4 * 8) as u32;
                let mask = (((1u64 << (size as u32 * 8)) - 1) as u32) << shift;
                if let Some(config) = self.config.get_mut(index) {
                    *config = (*config & !mask) | ((val << shift) & mask);
                }
            }
            0xf00 => {
                // SGIR target filter: 0 = explicit CPU mask, 2 = self.
                if (val >> 24) & 3 == 2 || ((val >> 24) & 3 == 0 && (val >> 16) & 1 != 0) {
                    self.pending[0] |= 1 << (val & 15);
                }
            }
            _ => {}
        }
    }

    fn write_bits(words: &mut [u32], offset: u64, val: u32, size: u8, set: bool) {
        let index = (offset / 4) as usize;
        if let Some(word) = words.get_mut(index) {
            let shift = (offset % 4 * 8) as u32;
            let mask = val & ((1u64 << (size as u32 * 8)) - 1) as u32;
            if set {
                *word |= mask << shift;
            } else {
                *word &= !(mask << shift);
            }
        }
    }

    fn write_bytes(bytes: &mut [u8], offset: u64, val: u32, size: u8) {
        for i in 0..size as usize {
            if let Some(byte) = bytes.get_mut(offset as usize + i) {
                *byte = (val >> (8 * i)) as u8;
            }
        }
    }

    pub fn read(&self, offset: u64, size: u8) -> u64 {
        if !matches!(size, 1 | 2 | 4) {
            return 0;
        }
        let (value, shift) = match offset {
            0 => (self.ctlr, 0),
            4 => (7, 0), // 256 IRQs, one CPU
            8 => (0x0200_143b, 0),
            0x100..=0x11f | 0x180..=0x19f => Self::read_word(&self.enabled, offset & 0x7f),
            0x200..=0x21f | 0x280..=0x29f => Self::read_word(&self.pending, offset & 0x7f),
            0x300..=0x31f | 0x380..=0x39f => Self::read_word(&self.active, offset & 0x7f),
            0x400..=0x4ff => return Self::read_bytes(&self.priority, offset - 0x400, size),
            0x800..=0x8ff => return Self::read_bytes(&self.target, offset - 0x800, size),
            0xc00..=0xc3f => Self::read_word(&self.config, offset - 0xc00),
            _ => (0, 0),
        };
        ((value >> shift) as u64) & ((1u64 << (size as u32 * 8)) - 1)
    }

    fn read_word(words: &[u32], offset: u64) -> (u32, u32) {
        (
            words.get((offset / 4) as usize).copied().unwrap_or(0),
            (offset % 4 * 8) as u32,
        )
    }

    fn read_bytes(bytes: &[u8], offset: u64, size: u8) -> u64 {
        let mut value = 0;
        for i in 0..size as usize {
            value |= (bytes.get(offset as usize + i).copied().unwrap_or(0) as u64) << (i * 8);
        }
        value
    }

    pub fn owns_gicd(ipa: u64) -> bool {
        (GICD_BASE_IPA..GICD_BASE_IPA + GICD_SIZE).contains(&ipa)
    }

    pub fn owns_gicc(ipa: u64) -> bool {
        (GICC_BASE_IPA..GICC_BASE_IPA + GICC_SIZE).contains(&ipa)
    }
}
