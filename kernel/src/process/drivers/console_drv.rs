use alloc::collections::VecDeque;
use crate::sync::Spinlock;

pub struct ConsoleDriver {
    pub buffer: VecDeque<u8>,
}

impl ConsoleDriver {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
        }
    }

    /// Polls SBI for a character and pushes it to buffer if available.
    /// Returns true if a character was received.
    pub fn poll(&mut self) -> bool {
        // Try legacy extension first (EID=1)
        let c = sbi_rt::legacy::console_getchar();
        // SBI Legacy returns -1 (usize::MAX) if no char
        if c != usize::MAX {
            let byte = c as u8;
            self.buffer.push_back(byte);
            
            // Optional: Echo back to screen? 
            // Shell usually handles echo, but for raw input validation we might want it.
            // Let's rely on Shell for echo.
            return true;
        }
        false
    }

    /// Read a byte from buffer (Non-blocking)
    pub fn read_byte(&mut self) -> Option<u8> {
        self.buffer.pop_front()
    }
}

pub static CONSOLE: Spinlock<ConsoleDriver> = Spinlock::new(ConsoleDriver { buffer: VecDeque::new() });

pub fn init() {
    // Nothing special to init for SBI Console so far
    // But we might want to clear buffer
    let mut cons = CONSOLE.lock();
    cons.buffer.clear();
    log::info!("Console: Input Driver Initialized.");
}
