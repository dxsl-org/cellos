#![no_std]

use hal_core::{HalResult, uart::SerialPort};


/// NS16550A UART Driver
pub struct Ns16550a {
    base_addr: usize,
}

impl Ns16550a {
    /// Unsafe because the caller must ensure base_addr is valid MMIO
    pub unsafe fn new(base_addr: usize) -> Self {
        Self { base_addr }
    }

    fn read_reg(&self, offset: usize) -> u8 {
        unsafe {
            let ptr = (self.base_addr + offset) as *const u8;
            core::ptr::read_volatile(ptr)
        }
    }

    fn write_reg(&mut self, offset: usize, value: u8) {
        unsafe {
            let ptr = (self.base_addr + offset) as *mut u8;
            core::ptr::write_volatile(ptr, value);
        }
    }
}

impl SerialPort for Ns16550a {
    fn init(&mut self) -> HalResult<()> {
        // 1. Disable Interrupts
        self.write_reg(1, 0x00); 
        
        // 2. Enable FIFO
        self.write_reg(2, 0x01);
        
        // 3. Set standard 8N1 mode (8 bits, No parity, 1 stop bit)
        // LCR = 0x03
        self.write_reg(3, 0x03);

        Ok(())
    }

    fn send(&mut self, data: u8) -> HalResult<()> {
        // Wait for THR empty (LSR bit 5)
        while (self.read_reg(5) & 0x20) == 0 {
            core::hint::spin_loop();
        }
        self.write_reg(0, data);
        Ok(())
    }

    fn receive(&mut self) -> HalResult<u8> {
        // Check Data Ready (LSR bit 0)
        if (self.read_reg(5) & 0x01) != 0 {
            Ok(self.read_reg(0))
        } else {
            // Non-blocking return? Or blocking?
            // For now, let's block or return 0
            // Trait signature says -> HalResult<u8>, not Option. So blocking implies wait.
            // But we might want polling. Let's spin wait.
            while (self.read_reg(5) & 0x01) == 0 {
                core::hint::spin_loop();
            }
            Ok(self.read_reg(0))
        }
    }
}
