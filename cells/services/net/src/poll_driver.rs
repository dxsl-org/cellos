//! Net cell polling driver — timer-tick interval and kernel frame opcode.

/// Recommended poll interval in milliseconds.
pub const POLL_INTERVAL_MS: u64 = 100;

/// IPC opcodes sent by the kernel VirtIO net ISR (raw bytes, NOT postcard).
pub mod kernel_opcodes {
    /// Kernel pushes a raw Ethernet frame to be processed by smoltcp.
    /// Not actively used since pump_rx() / sys_net_rx polls the ring directly,
    /// but retained as a sentinel in case the kernel adds push notifications.
    // reason: reserved opcode value, not yet dispatched anywhere — see doc comment.
    #[allow(dead_code)]
    pub const RX_FRAME: u8 = 0x00;
}
