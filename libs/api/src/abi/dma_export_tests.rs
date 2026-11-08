// SPDX-License-Identifier: Apache-2.0
//! Unit tests for `ViDmaExportV1` boundary record.

#[cfg(test)]
mod tests {
    use crate::abi::dma_export::*;
    use core::mem::size_of;
    use types::ViError;

    #[test]
    fn test_vidmaexportv1_size_and_offsets() {
        assert_eq!(size_of::<ViDmaExportV1>(), DMA_EXPORT_RECORD_LEN);
        assert_eq!(size_of::<ViDmaExportV1>(), 48);

        let export = ViDmaExportV1::new(EXPORT_DEVICE_WRITE, 42, 7, 3, 0x40_0000_0000, 65536);

        let mut buf = [0u8; DMA_EXPORT_RECORD_LEN];
        export.encode(&mut buf);

        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), DMA_EXPORT_VERSION);
        assert_eq!(u16::from_le_bytes([buf[2], buf[3]]), EXPORT_DEVICE_WRITE);
        assert_eq!(u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]), 0);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 42);
        assert_eq!(u64::from_le_bytes(buf[16..24].try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(buf[24..32].try_into().unwrap()), 3);
        assert_eq!(
            u64::from_le_bytes(buf[32..40].try_into().unwrap()),
            0x40_0000_0000
        );
        assert_eq!(u64::from_le_bytes(buf[40..48].try_into().unwrap()), 65536);

        let decoded = ViDmaExportV1::decode(&buf).expect("decode valid record");
        assert_eq!(decoded, export);
    }

    #[test]
    fn test_vidmaexportv1_decode_rejections() {
        let export = ViDmaExportV1::new(0, 100, 1, 1, 0x1000, 4096);
        let mut buf = [0u8; DMA_EXPORT_RECORD_LEN];
        export.encode(&mut buf);

        // Wrong buffer size
        assert_eq!(
            ViDmaExportV1::decode(&buf[..47]),
            Err(ViError::InvalidInput)
        );
        assert_eq!(ViDmaExportV1::decode(&[]), Err(ViError::InvalidInput));

        // Wrong version
        let mut bad_ver = buf;
        bad_ver[0] = 2;
        assert_eq!(ViDmaExportV1::decode(&bad_ver), Err(ViError::InvalidInput));

        // Unknown flags bit
        let mut bad_flags = buf;
        bad_flags[2] = 0x80;
        assert_eq!(
            ViDmaExportV1::decode(&bad_flags),
            Err(ViError::InvalidInput)
        );

        // Non-zero reserved bytes
        let mut bad_reserved = buf;
        bad_reserved[4] = 1;
        assert_eq!(
            ViDmaExportV1::decode(&bad_reserved),
            Err(ViError::InvalidInput)
        );
    }
}
