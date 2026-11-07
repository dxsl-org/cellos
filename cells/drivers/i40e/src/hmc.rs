//! Host Memory Cache (HMC) queue context structures for Intel X710/XL710.
//!
//! Intel Ethernet Controller X710 Datasheet §8.3.3.

#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LanTxQueueContext {
    pub(crate) head: u16,
    pub(crate) new_context: bool,
    pub(crate) base_iova: u64,
    pub(crate) qlen: u16,
    pub(crate) rdylist: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LanRxQueueContext {
    pub(crate) head: u16,
    pub(crate) base_iova: u64,
    pub(crate) qlen: u16,
    pub(crate) dbuff_size: u16,
}

impl LanTxQueueContext {
    /// Encode LAN Tx context into five 32-bit words for HMC programming.
    pub(crate) fn encode(&self) -> [u32; 5] {
        let mut words = [0u32; 5];
        words[0] = (self.head as u32) | if self.new_context { 1 << 30 } else { 0 };
        // Base address aligned to 128 bytes (shifted right by 7)
        let base_shifted = self.base_iova >> 7;
        words[1] = base_shifted as u32;
        words[2] = ((base_shifted >> 32) as u32) | ((self.qlen as u32) << 18);
        words[3] = (self.rdylist as u32) << 16;
        words[4] = 0;
        words
    }
}

impl LanRxQueueContext {
    /// Encode LAN Rx context into eight 32-bit words for HMC programming.
    pub(crate) fn encode(&self) -> [u32; 8] {
        let mut words = [0u32; 8];
        words[0] = (self.head as u32) << 16;
        let base_shifted = self.base_iova >> 7;
        words[1] = base_shifted as u32;
        words[2] = ((base_shifted >> 32) as u32) | ((self.qlen as u32) << 18);
        // dbuff size in 128-byte units
        words[3] = (self.dbuff_size >> 7) as u32;
        words
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_context_encoding_sets_new_context_bit() {
        let ctx = LanTxQueueContext {
            head: 0,
            new_context: true,
            base_iova: 0x1000,
            qlen: 16,
            rdylist: 0,
        };
        let words = ctx.encode();
        assert_ne!(words[0] & (1 << 30), 0);
        assert_eq!(words[1], (0x1000 >> 7));
    }

    #[test]
    fn rx_context_encoding_sets_dbuff_and_qlen() {
        let ctx = LanRxQueueContext {
            head: 0,
            base_iova: 0x2000,
            qlen: 16,
            dbuff_size: 2048,
        };
        let words = ctx.encode();
        assert_eq!(words[1], (0x2000 >> 7));
        assert_eq!(words[3], 2048 >> 7);
    }
}
