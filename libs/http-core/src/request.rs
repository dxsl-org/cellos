//! HTTP/1.1 request serialisation.
//!
//! Generalises `build_post` from `cells/apps/hypha/llm-gateway/src/http.rs`.
//! That function emitted HTTP/1.0; this builder emits HTTP/1.1, which enables
//! chunked responses from modern servers (needed by Phase 02 `BodyReader`).

use alloc::vec::Vec;

/// Builds a well-formed HTTP/1.1 request and serialises it into a `Vec<u8>`.
///
/// # Auto-emitted headers
///
/// - `Host: <host>` — always first after the request line.
/// - `Content-Length: <n>` — only when `body` is `Some` and non-empty.
/// - `Connection: close` — always last in the auto-headers.
///
/// # Caller-supplied headers
///
/// `extra_headers` are emitted between `Host` and `Connection: close`.
/// Callers MUST NOT repeat `Host`, `Content-Length`, or `Connection` in
/// `extra_headers`; doing so produces a technically valid but redundant request.
///
/// # Panics
///
/// Never — all formatting is done via `Vec<u8>` writes which cannot fail in
/// a healthy allocator.  Malformed method/path strings are passed through as-is
/// (no validation — callers own the correctness of those fields).
pub struct RequestBuilder<'a> {
    method: &'a str,
    path: &'a str,
    host: &'a str,
    extra_headers: &'a [(&'a str, &'a str)],
    body: Option<&'a [u8]>,
}

impl<'a> RequestBuilder<'a> {
    /// Create a new builder.
    ///
    /// - `method` — HTTP verb, e.g. `"POST"`.
    /// - `path` — Request-URI, e.g. `"/v1/chat/completions"`.
    /// - `host` — value for the `Host` header, e.g. `"api.openai.com"`.
    /// - `extra_headers` — ordered list of additional headers to emit.
    /// - `body` — optional request body bytes; triggers `Content-Length`.
    pub fn new(
        method: &'a str,
        path: &'a str,
        host: &'a str,
        extra_headers: &'a [(&'a str, &'a str)],
        body: Option<&'a [u8]>,
    ) -> Self {
        Self { method, path, host, extra_headers, body }
    }

    /// Serialise the request into a byte vector ready to write to a stream.
    ///
    /// Layout (RFC 9112):
    /// ```text
    /// <METHOD> <path> HTTP/1.1\r\n
    /// Host: <host>\r\n
    /// [<extra headers>\r\n]*
    /// [Content-Length: <n>\r\n]
    /// Connection: close\r\n
    /// \r\n
    /// [<body>]
    /// ```
    pub fn build(self) -> Vec<u8> {
        let mut out = Vec::new();

        // Request line
        push_str(&mut out, self.method);
        out.push(b' ');
        push_str(&mut out, self.path);
        out.extend_from_slice(b" HTTP/1.1\r\n");

        // Mandatory Host header
        push_header(&mut out, "Host", self.host);

        // Caller-supplied headers
        for (name, value) in self.extra_headers {
            push_header(&mut out, name, value);
        }

        // Content-Length — only when body is present and non-empty
        if let Some(body) = self.body {
            if !body.is_empty() {
                let len_str = usize_to_decimal(body.len());
                push_header(&mut out, "Content-Length", &len_str);
            }
        }

        // Connection: close — signals to the server that this is a one-shot req
        push_header(&mut out, "Connection", "close");

        // Header terminator
        out.extend_from_slice(b"\r\n");

        // Body
        if let Some(body) = self.body {
            out.extend_from_slice(body);
        }

        out
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn push_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
}

fn push_header(buf: &mut Vec<u8>, name: &str, value: &str) {
    push_str(buf, name);
    buf.extend_from_slice(b": ");
    push_str(buf, value);
    buf.extend_from_slice(b"\r\n");
}

/// Convert a `usize` to its decimal string representation without `std::fmt`.
///
/// Used in no_std context where `format!` works but we want to avoid allocating
/// an extra `String`; this writes directly to a small stack buffer.
fn usize_to_decimal(mut n: usize) -> alloc::string::String {
    if n == 0 {
        return alloc::string::String::from("0");
    }
    // 20 digits is enough for u64::MAX
    let mut buf = [0u8; 20];
    let mut i = 0;
    while n > 0 {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    alloc::string::String::from(core::str::from_utf8(&buf[..i]).unwrap())
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
