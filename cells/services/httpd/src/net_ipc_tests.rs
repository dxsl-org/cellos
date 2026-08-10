#[cfg(test)]
mod tests {
    use crate::net_ipc::{decode_net_send_progress, map_tcp_recv_response, tcp_send_all_with};
    use alloc::vec;
    use ostd::ipc::IpcError;

    #[test]
    fn wrong_sender_recv_maps_to_none() {
        assert_eq!(map_tcp_recv_response(Err(IpcError::WrongSender)), None);
    }

    #[test]
    fn recv_empty_data_stays_nonfatal() {
        assert_eq!(
            map_tcp_recv_response(Ok(api::ipc::NetResponse::Data(&[]))),
            Some(None)
        );
    }

    #[test]
    fn tcp_send_progress_accepts_ok_and_counted_data() {
        assert_eq!(
            decode_net_send_progress(Ok(api::ipc::NetResponse::Ok), 128),
            Some(128)
        );
        assert_eq!(
            decode_net_send_progress(Ok(api::ipc::NetResponse::Data(&128u32.to_le_bytes())), 256),
            Some(128)
        );
    }

    #[test]
    fn tcp_send_progress_rejects_wrong_sender_and_short_counts() {
        assert_eq!(
            decode_net_send_progress(Err(IpcError::WrongSender), 64),
            None
        );
        assert_eq!(
            decode_net_send_progress(Ok(api::ipc::NetResponse::Data(&[1, 2, 3])), 64),
            None
        );
    }

    #[test]
    fn tcp_send_retries_zero_progress_then_succeeds() {
        let mut retries = 0usize;
        let mut calls = 0usize;
        let ok = tcp_send_all_with(
            &vec![b'x'; 600],
            |_| {
                calls += 1;
                match calls {
                    1 | 2 => None,
                    _ => Some(480),
                }
            },
            || retries += 1,
        );

        assert!(ok);
        assert_eq!(retries, 2);
        assert_eq!(calls, 4);
    }

    #[test]
    fn tcp_send_fails_after_bounded_zero_progress_retries() {
        let mut retries = 0usize;
        let ok = tcp_send_all_with(&vec![b'x'; 32], |_| Some(0), || retries += 1);

        assert!(!ok);
        assert_eq!(retries, 3);
    }
}
