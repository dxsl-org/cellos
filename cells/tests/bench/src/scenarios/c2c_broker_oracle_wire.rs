#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientMode {
    EchoSync = 1,
    EchoAsync = 2,
    HoldAsync = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientConfig {
    pub mode: ClientMode,
    pub request_count: u16,
    pub base_sequence: u64,
    pub hold_turns: u16,
    pub ack_posts: bool,
    pub wait_for_start: bool,
    pub wait_for_drain: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientSummary {
    pub attempted: u16,
    pub success: u16,
    pub busy: u16,
    pub indeterminate: u16,
    pub correlation: u16,
    pub latency_ns: u64,
    pub send_latency_ns: u64,
    pub reply_wait_ns: u64,
}

pub const CONFIG_BYTES: usize = 15;
pub const READY_BYTES: usize = 9;
pub const POSTED_BYTES: usize = 3;
pub const START_BYTES: usize = 1;
pub const DRAIN_BYTES: usize = 1;
pub const SUMMARY_BYTES: usize = 35;

const TAG_CONFIG: u8 = 0x41;
const TAG_READY: u8 = 0x42;
const TAG_POSTED: u8 = 0x43;
const TAG_START: u8 = 0x44;
const TAG_DRAIN: u8 = 0x45;
const TAG_SUMMARY: u8 = 0x46;

pub fn encode_config(config: ClientConfig, out: &mut [u8; CONFIG_BYTES]) {
    out[0] = TAG_CONFIG;
    out[1] = config.mode as u8;
    out[2..4].copy_from_slice(&config.request_count.to_le_bytes());
    out[4..12].copy_from_slice(&config.base_sequence.to_le_bytes());
    out[12..14].copy_from_slice(&config.hold_turns.to_le_bytes());
    out[14] = (config.ack_posts as u8)
        | ((config.wait_for_start as u8) << 1)
        | ((config.wait_for_drain as u8) << 2);
}

pub fn decode_config(buf: &[u8]) -> Option<ClientConfig> {
    if buf.first().copied()? != TAG_CONFIG || buf.len() < CONFIG_BYTES {
        return None;
    }
    let mode = match buf[1] {
        1 => ClientMode::EchoSync,
        2 => ClientMode::EchoAsync,
        3 => ClientMode::HoldAsync,
        _ => return None,
    };
    Some(ClientConfig {
        mode,
        request_count: u16::from_le_bytes(buf[2..4].try_into().ok()?),
        base_sequence: u64::from_le_bytes(buf[4..12].try_into().ok()?),
        hold_turns: u16::from_le_bytes(buf[12..14].try_into().ok()?),
        ack_posts: buf[14] & 0x01 != 0,
        wait_for_start: buf[14] & 0x02 != 0,
        wait_for_drain: buf[14] & 0x04 != 0,
    })
}

pub fn encode_ready(broker_tid: usize, out: &mut [u8; READY_BYTES]) {
    out[0] = TAG_READY;
    out[1..9].copy_from_slice(&(broker_tid as u64).to_le_bytes());
}

pub fn decode_ready(buf: &[u8]) -> Option<usize> {
    if buf.first().copied()? != TAG_READY || buf.len() < READY_BYTES {
        return None;
    }
    Some(u64::from_le_bytes(buf[1..9].try_into().ok()?) as usize)
}

pub fn encode_posted(posted: u16, out: &mut [u8; POSTED_BYTES]) {
    out[0] = TAG_POSTED;
    out[1..3].copy_from_slice(&posted.to_le_bytes());
}

pub fn decode_posted(buf: &[u8]) -> Option<u16> {
    if buf.first().copied()? != TAG_POSTED || buf.len() < POSTED_BYTES {
        return None;
    }
    Some(u16::from_le_bytes(buf[1..3].try_into().ok()?))
}

pub fn encode_drain(out: &mut [u8; DRAIN_BYTES]) {
    out[0] = TAG_DRAIN;
}

pub fn encode_start(out: &mut [u8; START_BYTES]) {
    out[0] = TAG_START;
}

pub fn is_start(buf: &[u8]) -> bool {
    buf.first().copied() == Some(TAG_START)
}

pub fn is_drain(buf: &[u8]) -> bool {
    buf.first().copied() == Some(TAG_DRAIN)
}

pub fn encode_summary(summary: ClientSummary, out: &mut [u8; SUMMARY_BYTES]) {
    out[0] = TAG_SUMMARY;
    out[1..3].copy_from_slice(&summary.attempted.to_le_bytes());
    out[3..5].copy_from_slice(&summary.success.to_le_bytes());
    out[5..7].copy_from_slice(&summary.busy.to_le_bytes());
    out[7..9].copy_from_slice(&summary.indeterminate.to_le_bytes());
    out[9..11].copy_from_slice(&summary.correlation.to_le_bytes());
    out[11..19].copy_from_slice(&summary.latency_ns.to_le_bytes());
    out[19..27].copy_from_slice(&summary.send_latency_ns.to_le_bytes());
    out[27..35].copy_from_slice(&summary.reply_wait_ns.to_le_bytes());
}

pub fn decode_summary(buf: &[u8]) -> Option<ClientSummary> {
    if buf.first().copied()? != TAG_SUMMARY || buf.len() < SUMMARY_BYTES {
        return None;
    }
    Some(ClientSummary {
        attempted: u16::from_le_bytes(buf[1..3].try_into().ok()?),
        success: u16::from_le_bytes(buf[3..5].try_into().ok()?),
        busy: u16::from_le_bytes(buf[5..7].try_into().ok()?),
        indeterminate: u16::from_le_bytes(buf[7..9].try_into().ok()?),
        correlation: u16::from_le_bytes(buf[9..11].try_into().ok()?),
        latency_ns: u64::from_le_bytes(buf[11..19].try_into().ok()?),
        send_latency_ns: u64::from_le_bytes(buf[19..27].try_into().ok()?),
        reply_wait_ns: u64::from_le_bytes(buf[27..35].try_into().ok()?),
    })
}
