//! Route classification is pure: all network and backend work belongs to the reactor.
use crate::net_ipc::{request_complete_len, RequestReadError};

/// Bounds for the operator-supplied `--api-token`; shorter tokens are guessable over the LAN.
pub(crate) const API_TOKEN_MIN: usize = 16;
pub(crate) const API_TOKEN_MAX: usize = 128;

pub(crate) enum Route<'a> {
    Index,
    Status,
    ApiStatus,
    ApiCells,
    ApiCluster,
    ApiGatewayStats,
    /// The single file the operator named on the command line.
    File(&'a str),
    /// A client-named file, relative to the web root (always starts with `/`).
    WebFile(&'a str),
    /// A client-named directory, relative to the web root (always starts with `/`).
    Files(&'a str),
    Infer {
        body: &'a [u8],
        max_tokens: u16,
    },
    Unauthorized,
    NotFound,
}

/// `api_token` is the operator's bearer token; without one, control endpoints do not exist.
pub(crate) fn classify<'a>(
    raw: &'a [u8],
    file_to_serve: Option<&'a str>,
    api_token: Option<&str>,
) -> Result<Route<'a>, RequestReadError> {
    let len = request_complete_len(raw)?.ok_or(RequestReadError::Incomplete)?;
    if raw.len() < len {
        return Err(RequestReadError::Incomplete);
    }
    if let Some(file) = file_to_serve {
        return Ok(Route::File(file));
    }
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);
    let header_end = match req.parse(&raw[..len]) {
        Ok(httparse::Status::Complete(n)) => n,
        _ => return Err(RequestReadError::BadRequest),
    };
    let path = req.path.ok_or(RequestReadError::BadRequest)?;
    let path_only = path.split('?').next().unwrap_or(path);
    Ok(match (req.method.unwrap_or("GET"), path_only) {
        ("GET", "/") => Route::Index,
        ("GET", "/status") => Route::Status,
        ("GET", path) if path.starts_with("/files/") => Route::WebFile(&path["/files".len()..]),
        ("GET", "/api/status") | ("GET", "/api/system") => Route::ApiStatus,
        ("GET", "/api/cells") => Route::ApiCells,
        ("GET", "/api/cluster") | ("GET", "/api/cluster/status") => Route::ApiCluster,
        ("GET", "/api/gateway") | ("GET", "/api/gateway/stats") => Route::ApiGatewayStats,
        ("GET", "/api/files") => match extract_query_param(path, "path").unwrap_or("/") {
            dir if dir.starts_with('/') => Route::Files(dir),
            _ => Route::NotFound,
        },
        ("POST", "/api/infer") => match api_token {
            None => Route::NotFound,
            Some(token) if !bearer_matches(req.headers, token) => Route::Unauthorized,
            Some(_) => Route::Infer {
                body: &raw[header_end..len],
                max_tokens: extract_query_param(path, "max_tokens")
                    .and_then(|value| value.parse::<u16>().ok())
                    .unwrap_or(24)
                    .clamp(1, 64),
            },
        },
        _ => Route::NotFound,
    })
}

pub(crate) fn valid_api_token(token: &str) -> bool {
    (API_TOKEN_MIN..=API_TOKEN_MAX).contains(&token.len())
        && token.bytes().all(|b| b.is_ascii_graphic())
}

/// Constant-time in the token bytes so response timing does not leak a matching prefix.
fn bearer_matches(headers: &[httparse::Header<'_>], token: &str) -> bool {
    let Some(value) = headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("authorization"))
        .map(|h| h.value)
    else {
        return false;
    };
    let Some(presented) = value
        .get(..7)
        .filter(|scheme| scheme.eq_ignore_ascii_case(b"Bearer "))
        .map(|_| &value[7..])
    else {
        return false;
    };
    if presented.len() != token.len() {
        return false;
    }
    presented
        .iter()
        .zip(token.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub fn extract_query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef-test";

    #[test]
    fn test_classify_cluster_and_gateway_routes() {
        let req = b"GET /api/cluster HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert!(matches!(classify(req, None, None), Ok(Route::ApiCluster)));

        let req_stats = b"GET /api/gateway/stats HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert!(matches!(
            classify(req_stats, None, None),
            Ok(Route::ApiGatewayStats)
        ));
    }

    #[test]
    fn unimplemented_control_endpoints_are_gone() {
        for req in [
            &b"POST /api/c2c/dispatch HTTP/1.1\r\nContent-Length: 9\r\n\r\nhello-c2c"[..],
            &b"POST /api/cells/net/restart HTTP/1.1\r\nContent-Length: 0\r\n\r\n"[..],
        ] {
            assert!(matches!(
                classify(req, None, Some(TOKEN)),
                Ok(Route::NotFound)
            ));
        }
    }

    #[test]
    fn client_paths_stay_relative_to_the_web_root() {
        let file = b"GET /files/mnt/sd/etc/cellos/cluster.key HTTP/1.1\r\n\r\n";
        assert!(matches!(
            classify(file, None, None),
            Ok(Route::WebFile("/mnt/sd/etc/cellos/cluster.key"))
        ));
        let dir = b"GET /api/files?path=/mnt/sd/etc/cellos HTTP/1.1\r\n\r\n";
        assert!(matches!(
            classify(dir, None, None),
            Ok(Route::Files("/mnt/sd/etc/cellos"))
        ));
        let relative = b"GET /api/files?path=etc HTTP/1.1\r\n\r\n";
        assert!(matches!(
            classify(relative, None, None),
            Ok(Route::NotFound)
        ));
    }

    #[test]
    fn infer_needs_an_operator_token_and_a_matching_bearer() {
        let bare = b"POST /api/infer HTTP/1.1\r\nContent-Length: 2\r\n\r\nhi";
        assert!(matches!(classify(bare, None, None), Ok(Route::NotFound)));
        assert!(matches!(
            classify(bare, None, Some(TOKEN)),
            Ok(Route::Unauthorized)
        ));

        let wrong = b"POST /api/infer HTTP/1.1\r\nAuthorization: Bearer 0123456789abcdef-tesT\r\nContent-Length: 2\r\n\r\nhi";
        assert!(matches!(
            classify(wrong, None, Some(TOKEN)),
            Ok(Route::Unauthorized)
        ));
        let prefix = b"POST /api/infer HTTP/1.1\r\nAuthorization: Bearer 0123456789abcdef\r\nContent-Length: 2\r\n\r\nhi";
        assert!(matches!(
            classify(prefix, None, Some(TOKEN)),
            Ok(Route::Unauthorized)
        ));

        let good = b"POST /api/infer?max_tokens=3 HTTP/1.1\r\nauthorization: bearer 0123456789abcdef-test\r\nContent-Length: 2\r\n\r\nhi";
        match classify(good, None, Some(TOKEN)) {
            Ok(Route::Infer { body, max_tokens }) => {
                assert_eq!(body, b"hi");
                assert_eq!(max_tokens, 3);
            }
            other => panic!("expected Infer, got {:?}", other.err()),
        }
    }

    #[test]
    fn api_tokens_must_be_long_visible_ascii() {
        assert!(valid_api_token(TOKEN));
        assert!(!valid_api_token("short"));
        assert!(!valid_api_token("0123456789abcdef 0123"));
        assert!(!valid_api_token(&"x".repeat(API_TOKEN_MAX + 1)));
    }
}
