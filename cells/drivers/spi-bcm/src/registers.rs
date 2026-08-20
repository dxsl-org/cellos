pub const CS: usize = 0x00;
pub const FIFO: usize = 0x04;
pub const CLK: usize = 0x08;

pub const CS_CLEAR: u32 = 0b11 << 4;
pub const CS_TA: u32 = 1 << 7;
pub const CS_DONE: u32 = 1 << 16;
pub const CS_RXD: u32 = 1 << 17;
pub const CS_TXD: u32 = 1 << 18;

pub const CLOCK_DIVIDER: u32 = 64;
pub const POLL_BUDGET: usize = 1024;
