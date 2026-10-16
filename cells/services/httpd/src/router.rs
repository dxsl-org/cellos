//! Route classification is pure: all network and backend work belongs to the reactor.
use crate::net_ipc::{request_complete_len, RequestReadError};

pub(crate) enum Route<'a> {
    Index,
    Status,
    ApiStatus,
    ApiCells,
    ApiCluster,
    ApiGatewayStats,
    ApiC2cDispatch { body: &'a [u8] },
    File(&'a str),
    Files(&'a str),
    #[cfg(target_os = "none")]
    Infer { body: &'a [u8], max_tokens: u16 },
    Restart,
    NotFound,
}

pub(crate) fn classify<'a>(raw: &'a [u8], file_to_serve: Option<&'a str>) -> Result<Route<'a>, RequestReadError> {
    let len = request_complete_len(raw)?.ok_or(RequestReadError::Incomplete)?;
    if raw.len() < len { return Err(RequestReadError::Incomplete); }
    if let Some(file) = file_to_serve { return Ok(Route::File(file)); }
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);
    #[cfg_attr(not(target_os = "none"), allow(unused_variables))]
    let header_end = match req.parse(&raw[..len]) {
        Ok(httparse::Status::Complete(n)) => n,
        _ => return Err(RequestReadError::BadRequest),
    };
    let path = req.path.ok_or(RequestReadError::BadRequest)?;
    let path_only = path.split('?').next().unwrap_or(path);
    Ok(match (req.method.unwrap_or("GET"), path_only) {
        ("GET", "/") => Route::Index,
        ("GET", "/status") => Route::Status,
        ("GET", path) if path.starts_with("/files/") => Route::File(&path["/files".len()..]),
        ("GET", "/api/status") | ("GET", "/api/system") => Route::ApiStatus,
        ("GET", "/api/cells") => Route::ApiCells,
        ("GET", "/api/cluster") | ("GET", "/api/cluster/status") => Route::ApiCluster,
        ("GET", "/api/gateway") | ("GET", "/api/gateway/stats") => Route::ApiGatewayStats,
        ("POST", "/api/c2c/dispatch") => Route::ApiC2cDispatch {
            body: &raw[header_end..len],
        },
        ("GET", "/api/files") => Route::Files(extract_query_param(path, "path").unwrap_or("/")),
        #[cfg(target_os = "none")]
        ("POST", "/api/infer") => Route::Infer {
            body: &raw[header_end..len],
            max_tokens: extract_query_param(path, "max_tokens")
                .and_then(|value| value.parse::<u16>().ok()).unwrap_or(24).clamp(1, 64),
        },
        ("POST", path) if path.starts_with("/api/cells/") && path.ends_with("/restart") => Route::Restart,
        _ => Route::NotFound,
    })
}

pub fn extract_query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key { return Some(v); }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_cluster_and_gateway_routes() {
        let req = b"GET /api/cluster HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert!(matches!(classify(req, None), Ok(Route::ApiCluster)));

        let req_stats = b"GET /api/gateway/stats HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert!(matches!(classify(req_stats, None), Ok(Route::ApiGatewayStats)));
    }

    #[test]
    fn test_classify_c2c_dispatch_payload() {
        let req = b"POST /api/c2c/dispatch HTTP/1.1\r\nContent-Length: 9\r\n\r\nhello-c2c";
        match classify(req, None) {
            Ok(Route::ApiC2cDispatch { body }) => {
                assert_eq!(body, b"hello-c2c");
            }
            other => panic!("expected ApiC2cDispatch, got {:?}", other.err()),
        }
    }
}
