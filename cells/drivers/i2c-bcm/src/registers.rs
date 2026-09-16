pub const C: usize = 0x00;
pub const S: usize = 0x04;
pub const DLEN: usize = 0x08;
pub const A: usize = 0x0C;
pub const FIFO: usize = 0x10;
pub const CLKT: usize = 0x1C;

pub const C_READ: u32 = 1 << 0;
pub const C_CLEAR: u32 = 0b11 << 4;
pub const C_ST: u32 = 1 << 7;
pub const C_I2CEN: u32 = 1 << 15;

pub const S_TXW: u32 = 1 << 2;
pub const S_DONE: u32 = 1 << 1;
pub const S_TXD: u32 = 1 << 4;
pub const S_RXD: u32 = 1 << 5;
pub const S_ERR: u32 = 1 << 8;
pub const S_CLKT: u32 = 1 << 9;
pub const S_CLEAR: u32 = S_DONE | S_ERR | S_CLKT;

pub const POLL_BUDGET: usize = 1024;
