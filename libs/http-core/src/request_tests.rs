use super::*;

#[test]
fn post_with_body_exact_bytes() {
    let body = b"{\"key\":\"val\"}";
    let req = RequestBuilder::new(
        "POST",
        "/v1/completions",
        "api.example.com",
        &[("Content-Type", "application/json")],
        Some(body),
    )
    .build();

    let text = core::str::from_utf8(&req).unwrap();
    assert!(text.starts_with("POST /v1/completions HTTP/1.1\r\n"), "request line: {text:?}");
    assert!(text.contains("Host: api.example.com\r\n"), "host: {text:?}");
    assert!(text.contains("Content-Type: application/json\r\n"), "ct: {text:?}");
    let cl = alloc::format!("Content-Length: {}\r\n", body.len());
    assert!(text.contains(&cl), "cl: {text:?}");
    assert!(text.contains("Connection: close\r\n"), "conn: {text:?}");
    assert!(text.contains("\r\n\r\n"), "hdr end: {text:?}");
    assert!(req.ends_with(body), "body: {text:?}");
}

#[test]
fn get_no_body_no_content_length() {
    let req = RequestBuilder::new("GET", "/index.html", "example.com", &[], None).build();
    let text = core::str::from_utf8(&req).unwrap();
    assert!(text.starts_with("GET /index.html HTTP/1.1\r\n"));
    assert!(!text.contains("Content-Length"), "cl must be absent for GET: {text:?}");
    assert!(text.ends_with("\r\n\r\n"));
}

#[test]
fn empty_body_no_content_length() {
    let req = RequestBuilder::new("POST", "/", "host.invalid", &[], Some(b"")).build();
    let text = core::str::from_utf8(&req).unwrap();
    assert!(!text.contains("Content-Length"), "cl must be absent for empty body: {text:?}");
}

#[test]
fn host_header_ordering() {
    // Host must appear before any extra headers (RFC 9112 §6.3).
    let req = RequestBuilder::new(
        "POST",
        "/path",
        "myhost.com",
        &[("Authorization", "Bearer tok")],
        None,
    )
    .build();
    let text = core::str::from_utf8(&req).unwrap();
    let host_pos = text.find("Host:").unwrap();
    let auth_pos = text.find("Authorization:").unwrap();
    assert!(host_pos < auth_pos, "Host must precede extra headers");
}

#[test]
fn content_length_value_correct() {
    let body = b"hello world";
    let req = RequestBuilder::new("POST", "/x", "h", &[], Some(body)).build();
    let text = core::str::from_utf8(&req).unwrap();
    let expected = alloc::format!("Content-Length: {}\r\n", body.len());
    assert!(text.contains(&expected));
}
