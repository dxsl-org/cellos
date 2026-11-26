use super::*;

fn query_req(code: u32, start: u32, count: u32, size: u32) -> Vec<u8> {
    [code, start, count, size]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect()
}

fn pcm_req(code: u32, stream: u32) -> Vec<u8> {
    [code, stream]
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect()
}

fn set_params(buffer: u32, period: u32, channels: u8, format: u8, rate: u8) -> Vec<u8> {
    let mut req = pcm_req(R_PCM_SET_PARAMS, 0);
    for word in [buffer, period, 0] {
        req.extend_from_slice(&word.to_le_bytes());
    }
    req.extend_from_slice(&[channels, format, rate, 0]);
    req
}

fn status(reply: &Reply) -> u32 {
    le32(&reply.bytes, 0).unwrap()
}

fn good_params() -> Vec<u8> {
    set_params(32_768, 8_192, 2, FMT_S16, RATE_48000)
}

#[test]
fn pcm_info_describes_the_one_s16_48k_stereo_output() {
    let mut pcm = Pcm::new();
    let reply = pcm.handle(&query_req(R_PCM_INFO, 0, 1, 32), 36);
    assert_eq!(status(&reply), S_OK);
    assert_eq!(reply.bytes.len(), 36);
    let info = &reply.bytes[4..];
    assert_eq!(&info[8..16], &(1u64 << 5).to_le_bytes());
    assert_eq!(&info[16..24], &(1u64 << 7).to_le_bytes());
    assert_eq!(&info[24..32], &[0, 2, 2, 0, 0, 0, 0, 0]);
}

#[test]
fn queries_pad_or_truncate_to_the_drivers_item_size() {
    let mut pcm = Pcm::new();
    let reply = pcm.handle(&query_req(R_PCM_INFO, 0, 1, 40), 44);
    assert_eq!(reply.bytes.len(), 44);
    assert_eq!(&reply.bytes[36..], &[0; 8]);
    let reply = pcm.handle(&query_req(R_CHMAP_INFO, 0, 1, 8), 12);
    assert_eq!(status(&reply), S_OK);
    assert_eq!(&reply.bytes[4..], &[0, 0, 0, 0, 0, 2, 3, 4]);
    // No jacks: an empty query is fine, any jack is out of range.
    assert_eq!(
        status(&pcm.handle(&query_req(R_JACK_INFO, 0, 0, 24), 4)),
        S_OK
    );
    assert_eq!(
        status(&pcm.handle(&query_req(R_JACK_INFO, 0, 1, 24), 28)),
        S_BAD_MSG
    );
}

#[test]
fn malformed_queries_are_bad_messages() {
    let mut pcm = Pcm::new();
    for (req, room) in [
        (query_req(R_PCM_INFO, 0, 2, 32), 68),
        (query_req(R_PCM_INFO, 1, 1, 32), 36),
        (query_req(R_PCM_INFO, u32::MAX, 2, 32), 68),
        (query_req(R_PCM_INFO, 0, 1, 0), 36),
        (query_req(R_PCM_INFO, 0, 1, 4096), 4100),
        // The response does not fit.
        (query_req(R_PCM_INFO, 0, 1, 32), 35),
        // Short and long requests.
        (query_req(R_PCM_INFO, 0, 1, 32)[..12].to_vec(), 36),
        ([query_req(R_PCM_INFO, 0, 1, 32), vec![0]].concat(), 36),
    ] {
        let reply = pcm.handle(&req, room);
        assert_eq!(status(&reply), S_BAD_MSG, "{req:?}");
        assert_eq!(reply.bytes.len(), 4);
    }
}

#[test]
fn replies_never_exceed_the_room_and_unknown_requests_are_not_supported() {
    let mut pcm = Pcm::new();
    assert!(pcm
        .handle(&query_req(R_PCM_INFO, 0, 1, 32), 3)
        .bytes
        .is_empty());
    assert_eq!(status(&pcm.handle(&[1, 0, 0], 4)), S_BAD_MSG);
    assert_eq!(status(&pcm.handle(&pcm_req(2, 0), 4)), S_NOT_SUPP);
    assert_eq!(status(&pcm.handle(&pcm_req(0x0300, 0), 4)), S_NOT_SUPP);
}

#[test]
fn only_the_fixed_format_is_accepted() {
    let mut pcm = Pcm::new();
    for req in [
        set_params(32_768, 8_192, 1, FMT_S16, RATE_48000),
        set_params(32_768, 8_192, 2, 6, RATE_48000),
        set_params(32_768, 8_192, 2, FMT_S16, 6),
    ] {
        assert_eq!(status(&pcm.handle(&req, 4)), S_NOT_SUPP);
    }
    let mut features = good_params();
    features[16] = 1 << 2;
    assert_eq!(status(&pcm.handle(&features, 4)), S_NOT_SUPP);
    assert_eq!(pcm.state(), State::Initial);
    assert_eq!(status(&pcm.handle(&good_params(), 4)), S_OK);
    assert_eq!(pcm.state(), State::ParamsSet);
}

#[test]
fn buffer_and_period_sizes_are_bounded() {
    let mut pcm = Pcm::new();
    for (buffer, period) in [
        (32_768, 0),
        (32_768, 6),
        (30_000, 8_192),
        (MAX_BUFFER_BYTES + 8_192, 8_192),
    ] {
        let req = set_params(buffer, period, 2, FMT_S16, RATE_48000);
        assert_eq!(status(&pcm.handle(&req, 4)), S_BAD_MSG, "{buffer}/{period}");
    }
    let mut other_stream = good_params();
    other_stream[4] = 1;
    assert_eq!(status(&pcm.handle(&other_stream, 4)), S_BAD_MSG);
    assert_eq!(status(&pcm.handle(&good_params()[..23], 4)), S_BAD_MSG);
}

#[test]
fn the_lifecycle_follows_the_spec() {
    let mut pcm = Pcm::new();
    let send = |pcm: &mut Pcm, code| {
        let reply = pcm.handle(&pcm_req(code, 0), 4);
        (status(&reply), reply.effect)
    };
    assert_eq!(send(&mut pcm, R_PCM_PREPARE).0, S_BAD_MSG);
    assert_eq!(status(&pcm.handle(&good_params(), 4)), S_OK);
    assert_eq!(send(&mut pcm, R_PCM_START).0, S_BAD_MSG);
    assert_eq!(send(&mut pcm, R_PCM_PREPARE), (S_OK, Effect::None));
    assert!(pcm.accepts_io() && !pcm.playing());
    assert_eq!(send(&mut pcm, R_PCM_START), (S_OK, Effect::Start));
    assert!(pcm.playing());
    assert_eq!(status(&pcm.handle(&good_params(), 4)), S_BAD_MSG);
    assert_eq!(send(&mut pcm, R_PCM_RELEASE).0, S_BAD_MSG);
    assert_eq!(send(&mut pcm, R_PCM_STOP), (S_OK, Effect::Stop));
    assert_eq!(send(&mut pcm, R_PCM_START), (S_OK, Effect::Start));
    assert_eq!(send(&mut pcm, R_PCM_STOP), (S_OK, Effect::Stop));
    assert_eq!(send(&mut pcm, R_PCM_RELEASE), (S_OK, Effect::Release));
    assert!(!pcm.accepts_io());
    assert_eq!(send(&mut pcm, R_PCM_PREPARE), (S_OK, Effect::None));
    // Another stream id, or a short header, changes nothing.
    assert_eq!(status(&pcm.handle(&pcm_req(R_PCM_START, 1), 4)), S_BAD_MSG);
    assert_eq!(
        status(&pcm.handle(&pcm_req(R_PCM_START, 0)[..6], 4)),
        S_BAD_MSG
    );
    assert_eq!(pcm.state(), State::Prepared);
}
