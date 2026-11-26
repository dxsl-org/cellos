//! virtio-sound control and I/O message layouts (virtio 1.2 §5.14.6). Pure:
//! encoding and decoding only, host-tested.
//!
//! The cell drives one output stream in the one format `wire::audio` fixes
//! (48 kHz, 2 channels, S16LE), so only the PCM requests are encoded.

use wire::audio::{CHANNELS, FRAME_BYTES, RATE};

/// `VIRTIO_SND_R_PCM_*` request codes.
pub const R_PCM_INFO: u32 = 0x0100;
pub const R_PCM_SET_PARAMS: u32 = 0x0101;
pub const R_PCM_PREPARE: u32 = 0x0102;
pub const R_PCM_RELEASE: u32 = 0x0103;
pub const R_PCM_START: u32 = 0x0104;
pub const R_PCM_STOP: u32 = 0x0105;

/// `VIRTIO_SND_S_OK`: the only status the cell treats as success.
pub const S_OK: u32 = 0x8000;

/// `VIRTIO_SND_D_OUTPUT`.
const D_OUTPUT: u8 = 0;
/// `VIRTIO_SND_PCM_FMT_S16` and `VIRTIO_SND_PCM_RATE_48000`: bit numbers in
/// `virtio_snd_pcm_info.formats` / `.rates`, and the values SET_PARAMS takes.
const FMT_S16: u8 = 5;
const RATE_48000: u8 = 7;

/// `struct virtio_snd_pcm_info` length; also the `size` a query asks for.
pub const PCM_INFO_LEN: usize = 32;
/// `struct virtio_snd_hdr` (status) length.
pub const STATUS_LEN: usize = 4;
/// `struct virtio_snd_pcm_xfer` length (the device-readable tx header).
pub const XFER_LEN: usize = 4;
/// `struct virtio_snd_pcm_status` length (the device-writable tx trailer).
pub const PCM_STATUS_LEN: usize = 8;
/// `struct virtio_snd_event` length.
pub const EVENT_LEN: usize = 8;

/// One period: 10 ms of the fixed format (1920 bytes).
pub const PERIOD_BYTES: usize = (RATE / 100 * FRAME_BYTES) as usize;
/// Periods in the device buffer; also the tx buffers in flight at most.
pub const PERIODS: usize = 8;
/// `buffer_bytes`: 80 ms, enough to ride out a 10 ms receive tick plus jitter.
pub const BUFFER_BYTES: usize = PERIODS * PERIOD_BYTES;

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le64(bytes: &[u8], at: usize) -> u64 {
    u64::from(le32(bytes, at)) | (u64::from(le32(bytes, at + 4)) << 32)
}

/// `virtio_snd_query_info{hdr: PCM_INFO, start_id: stream, count: 1, size}`.
pub fn query_pcm_info(stream: u32) -> [u8; 16] {
    let mut out = [0; 16];
    out[0..4].copy_from_slice(&R_PCM_INFO.to_le_bytes());
    out[4..8].copy_from_slice(&stream.to_le_bytes());
    out[8..12].copy_from_slice(&1u32.to_le_bytes());
    out[12..16].copy_from_slice(&(PCM_INFO_LEN as u32).to_le_bytes());
    out
}

/// `virtio_snd_pcm_hdr{code, stream_id}`: PREPARE, RELEASE, START and STOP.
pub fn pcm_hdr(code: u32, stream: u32) -> [u8; 8] {
    let mut out = [0; 8];
    out[0..4].copy_from_slice(&code.to_le_bytes());
    out[4..8].copy_from_slice(&stream.to_le_bytes());
    out
}

/// `virtio_snd_pcm_set_params` for the fixed format, [`BUFFER_BYTES`] and
/// [`PERIOD_BYTES`], no PCM features.
pub fn set_params(stream: u32) -> [u8; 24] {
    let mut out = [0; 24];
    out[0..8].copy_from_slice(&pcm_hdr(R_PCM_SET_PARAMS, stream));
    out[8..12].copy_from_slice(&(BUFFER_BYTES as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(PERIOD_BYTES as u32).to_le_bytes());
    out[20] = CHANNELS as u8;
    out[21] = FMT_S16;
    out[22] = RATE_48000;
    out
}

/// `virtio_snd_pcm_xfer{stream_id}`.
pub fn xfer(stream: u32) -> [u8; XFER_LEN] {
    stream.to_le_bytes()
}

/// The `code`/`status` word every response and status trailer starts with.
pub fn status(bytes: &[u8]) -> u32 {
    le32(bytes, 0)
}

/// The fields of `virtio_snd_pcm_info` the stream choice reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmInfo {
    pub formats: u64,
    pub rates: u64,
    pub direction: u8,
    pub channels_min: u8,
    pub channels_max: u8,
}

impl PcmInfo {
    /// `None` when `bytes` is shorter than [`PCM_INFO_LEN`].
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..PCM_INFO_LEN)?;
        Some(Self {
            formats: le64(bytes, 8),
            rates: le64(bytes, 16),
            direction: bytes[24],
            channels_min: bytes[25],
            channels_max: bytes[26],
        })
    }

    /// An output stream that plays the fixed format.
    pub fn plays_fixed_format(&self) -> bool {
        let channels = CHANNELS as u8;
        self.direction == D_OUTPUT
            && self.formats & (1 << FMT_S16) != 0
            && self.rates & (1 << RATE_48000) != 0
            && (self.channels_min..=self.channels_max).contains(&channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(direction: u8, formats: u64, rates: u64, min: u8, max: u8) -> [u8; PCM_INFO_LEN] {
        let mut out = [0; PCM_INFO_LEN];
        out[8..16].copy_from_slice(&formats.to_le_bytes());
        out[16..24].copy_from_slice(&rates.to_le_bytes());
        out[24] = direction;
        out[25] = min;
        out[26] = max;
        out
    }

    #[test]
    fn requests_encode_little_endian() {
        assert_eq!(
            query_pcm_info(3),
            [0, 1, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 32, 0, 0, 0]
        );
        assert_eq!(pcm_hdr(R_PCM_START, 1), [4, 1, 0, 0, 1, 0, 0, 0]);
        let params = set_params(0);
        assert_eq!(&params[0..8], &[1, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(le32(&params, 8), 15_360);
        assert_eq!(le32(&params, 12), 1_920);
        assert_eq!(le32(&params, 16), 0);
        assert_eq!(&params[20..24], &[2, 5, 7, 0]);
        assert_eq!(xfer(2), [2, 0, 0, 0]);
    }

    #[test]
    fn period_is_ten_milliseconds_of_whole_frames() {
        assert_eq!(PERIOD_BYTES, 1920);
        assert_eq!(PERIOD_BYTES % FRAME_BYTES as usize, 0);
    }

    #[test]
    fn stream_choice_needs_output_s16_48k_stereo() {
        let s16 = 1 << FMT_S16;
        let r48 = 1 << RATE_48000;
        let good = PcmInfo::decode(&info(0, s16 | 1, r48 | 1, 1, 2)).unwrap();
        assert!(good.plays_fixed_format());
        for bytes in [
            info(1, s16, r48, 2, 2),
            info(0, 1 << 6, r48, 2, 2),
            info(0, s16, 1 << 6, 2, 2),
            info(0, s16, r48, 1, 1),
            info(0, s16, r48, 4, 8),
        ] {
            assert!(!PcmInfo::decode(&bytes).unwrap().plays_fixed_format());
        }
        assert_eq!(PcmInfo::decode(&[0; PCM_INFO_LEN - 1]), None);
    }
}
