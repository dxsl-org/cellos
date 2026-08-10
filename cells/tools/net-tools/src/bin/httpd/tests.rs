use super::{
    classify_file_read, classify_stat_preflight, decode_tcp_send_progress, parse_u16,
    write_content_length, FileReadOutcome, StatPreflight, MAX_FILE_READ_BYTES,
};
use alloc::{vec, vec::Vec};
use api::ipc::{NetResponse, VfsResponse};
use ostd::ipc::IpcError;
use ostd::ViError;

#[test]
fn parse_u16_accepts_valid_ports() {
    assert_eq!(parse_u16("0"), Some(0));
    assert_eq!(parse_u16("8080"), Some(8080));
    assert_eq!(parse_u16("65535"), Some(65535));
}

#[test]
fn parse_u16_rejects_invalid_ports() {
    assert_eq!(parse_u16(""), None);
    assert_eq!(parse_u16("65536"), None);
    assert_eq!(parse_u16("80a"), None);
}

#[test]
fn classify_file_read_keeps_success_bytes() {
    match classify_file_read(Ok(Vec::from(b"hello".as_slice()))) {
        FileReadOutcome::Ok(bytes) => assert_eq!(bytes, b"hello"),
        _ => panic!("expected successful file read"),
    }
}

#[test]
fn classify_file_read_keeps_empty_existing_files_as_success() {
    match classify_file_read(Ok(Vec::new())) {
        FileReadOutcome::Ok(bytes) => assert!(bytes.is_empty()),
        _ => panic!("expected empty file read to stay successful"),
    }
}

#[test]
fn classify_file_read_maps_missing_to_not_found() {
    assert!(matches!(
        classify_file_read(Err(ViError::NotFound)),
        FileReadOutcome::NotFound
    ));
}

#[test]
fn classify_stat_preflight_maps_err_io_to_not_found() {
    assert!(matches!(
        classify_stat_preflight(Ok(VfsResponse::Err(1))),
        StatPreflight::NotFound
    ));
}

#[test]
fn classify_stat_preflight_keeps_other_failures_internal() {
    assert!(matches!(
        classify_stat_preflight(Ok(VfsResponse::Stat {
            size: 0,
            is_dir: true
        })),
        StatPreflight::InternalError
    ));
    assert!(matches!(
        classify_stat_preflight(Ok(VfsResponse::Err(4))),
        StatPreflight::InternalError
    ));
    assert!(matches!(
        classify_stat_preflight(Err(IpcError::Recv)),
        StatPreflight::InternalError
    ));
}

#[test]
fn decode_tcp_send_progress_accepts_valid_ack() {
    let bytes = 12u32.to_le_bytes();
    assert_eq!(
        decode_tcp_send_progress(Some(NetResponse::Data(&bytes))),
        Some(12)
    );
}

#[test]
fn decode_tcp_send_progress_rejects_malformed_ack() {
    assert_eq!(decode_tcp_send_progress(Some(NetResponse::Data(&[]))), None);
    assert_eq!(
        decode_tcp_send_progress(Some(NetResponse::Data(&vec![1, 2, 3]))),
        None
    );
    assert_eq!(decode_tcp_send_progress(Some(NetResponse::CapId(7))), None);
    assert_eq!(decode_tcp_send_progress(None), None);
}

#[test]
fn classify_file_read_rejects_truncation_or_transport_failures() {
    assert!(matches!(
        classify_file_read(Err(ViError::OutOfMemory)),
        FileReadOutcome::InternalError
    ));
    assert!(matches!(
        classify_file_read(Err(ViError::IO)),
        FileReadOutcome::InternalError
    ));
    assert_eq!(MAX_FILE_READ_BYTES, 4096);
}

#[test]
fn write_content_length_formats_ascii_header_line() {
    let mut out = [0u8; 64];
    let len = write_content_length(1234, &mut out);
    assert_eq!(&out[..len], b"Content-Length: 1234\r\n");
}
