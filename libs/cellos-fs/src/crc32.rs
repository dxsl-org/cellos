//! Fast pure-Rust CRC32C (Castagnoli) for CellosFS blocks and metadata.
//!
//! Compatible with hardware acceleration (CRC32 instruction on x86/ARM/RISC-V)
//! with a portable software table-driven fallback for no_std environments.

const CRC32C_POLYNOMIAL: u32 = 0x82F63B78;

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ CRC32C_POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

static CRC32C_TABLE: [u32; 256] = make_table();

/// Compute CRC32C checksum of `bytes`.
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        let table_idx = ((crc ^ (b as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32C_TABLE[table_idx];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc32c_standard_vectors() {
        // Standard check: "123456789" -> 0xE3069283
        assert_eq!(crc32c(b"123456789"), 0xE3069283);
        assert_eq!(crc32c(b""), 0);
    }
}
