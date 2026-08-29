//! Emulated 8259A PIC pair and edge/level control registers.
//!
//! Linux programs the master/slave command and mask ports plus ELCR ports
//! 0x4D0/0x4D1. IRR/ISR and priority resolution are not modelled; the run loop
//! delivers one eligible line at a time through [`Pic8259::irq`].

/// One 8259 chip: its programmed vector base, interrupt mask, and ICW state.
struct Chip {
    vector_base: u8,
    mask: u8,
    icw_step: u8, // 0 = initialised (data port = OCW1); 1..=3 = expecting ICWn
    icw4_needed: bool,
}

impl Chip {
    const fn new() -> Self {
        Self {
            vector_base: 0,
            mask: 0xFF, // all masked until the guest programs OCW1
            icw_step: 0,
            icw4_needed: false,
        }
    }

    /// Command-port write: ICW1 (bit4 set) starts init; anything else is an
    /// OCW2/OCW3 (EOI / read-register-select) — no state to keep in the MVP.
    fn command(&mut self, val: u8) {
        if val & 0x10 != 0 {
            self.icw4_needed = val & 0x01 != 0;
            self.icw_step = 1;
        }
    }

    /// Data-port write: ICW2/ICW3/ICW4 while initialising, else OCW1 (mask).
    fn data(&mut self, val: u8) {
        match self.icw_step {
            1 => {
                self.vector_base = val; // ICW2 = vector offset
                self.icw_step = 2;
            }
            2 => {
                // ICW3 = cascade wiring — not needed for single-IRQ delivery.
                self.icw_step = if self.icw4_needed { 3 } else { 0 };
            }
            3 => {
                self.icw_step = 0; // ICW4 = mode (8086); accepted, unused
            }
            _ => self.mask = val, // OCW1 = interrupt mask register
        }
    }
}

/// The master + slave 8259 pair.
pub struct Pic8259 {
    master: Chip,
    slave: Chip,
    elcr: [u8; 2],
}

impl Pic8259 {
    pub const fn new() -> Self {
        Self {
            master: Chip::new(),
            slave: Chip::new(),
            elcr: [0; 2],
        }
    }

    /// True if `port` addresses either PIC or its edge/level control register.
    pub fn owns(port: u16) -> bool {
        matches!(port, 0x20 | 0x21 | 0xA0 | 0xA1 | 0x4D0 | 0x4D1)
    }

    /// Handle a guest `out` to a PIC port.
    pub fn write(&mut self, port: u16, val: u32) {
        let b = (val & 0xFF) as u8;
        match port {
            0x20 => self.master.command(b),
            0x21 => self.master.data(b),
            0xA0 => self.slave.command(b),
            0xA1 => self.slave.data(b),
            0x4D0 => self.elcr[0] = b,
            0x4D1 => self.elcr[1] = b,
            _ => {}
        }
    }

    /// Handle a guest `in` from a PIC port. Data ports return the mask (IMR);
    /// command ports return 0 (IRR/ISR unmodelled).
    pub fn read(&self, port: u16) -> u32 {
        let v: u8 = match port {
            0x21 => self.master.mask,
            0xA1 => self.slave.mask,
            0x4D0 => self.elcr[0],
            0x4D1 => self.elcr[1],
            _ => 0,
        };
        v as u32
    }

    /// The CPU vector master-PIC `line` (0–7) is remapped to, or `None` if the
    /// line is masked or the master PIC has not been initialised (vector_base 0
    /// would alias the CPU divide-error exception). Slave lines (8–15) are not
    /// deliverable in this model.
    pub fn irq(&self, line: u8) -> Option<u8> {
        if line >= 8 || self.master.vector_base == 0 || self.master.mask & (1 << line) != 0 {
            None
        } else {
            Some(self.master.vector_base + line)
        }
    }

    /// The PIT tick line (master IRQ0).
    pub fn irq0(&self) -> Option<u8> {
        self.irq(0)
    }
}
