// HTTP response handlers: HTML pages, VFS file serving, and JSON REST API.

extern crate alloc;
use alloc::{format, string::String, vec::Vec};
#[cfg(target_os = "none")]
use ai_proto::MAX_PROMPT_BYTES;

#[cfg(target_arch = "riscv64")]
const ARCH: &str = "riscv64";
#[cfg(target_arch = "aarch64")]
const ARCH: &str = "aarch64";
#[cfg(target_arch = "x86_64")]
const ARCH: &str = "x86_64";
#[cfg(not(any(
    target_arch = "riscv64",
    target_arch = "aarch64",
    target_arch = "x86_64"
)))]
const ARCH: &str = "unknown";


// ── Response helpers ──────────────────────────────────────────────────────────

pub(crate) struct Response {
    pub header: alloc::vec::Vec<u8>,
    pub body: alloc::vec::Vec<u8>,
}

pub(crate) fn response(status: u16, content_type: &str, body: &[u8], json: bool) -> Response {
    let status_text = match status {
        200 => "OK", 204 => "No Content", 400 => "Bad Request",
        408 => "Request Timeout", 413 => "Payload Too Large", 404 => "Not Found",
        500 => "Internal Server Error", 503 => "Service Unavailable", _ => "OK",
    };
    let cors = if json { "Access-Control-Allow-Origin: *\r\n" } else { "" };
    Response {
        header: format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
            status, status_text, content_type, body.len(), cors
        ).into_bytes(),
        body: body.to_vec(),
    }
}

pub(crate) fn response_owned(status: u16, content_type: &str, body: Vec<u8>) -> Response {
    Response {
        header: format!(
            "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            status, content_type, body.len()
        ).into_bytes(),
        body,
    }
}

pub(crate) fn json(status: u16, text: &str) -> Response {
    response(status, "application/json", text.as_bytes(), true)
}
// ── HTML pages ────────────────────────────────────────────────────────────────

// Note: askama compile-time templates are the intended long-term approach.
// format! is used here for simplicity pending no_std askama validation.

pub(crate) fn index() -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Cellos Mini-Server Dashboard</title>
<style>
  body{{font-family:system-ui,-apple-system,sans-serif;background:#0d1117;color:#c9d1d9;margin:0;padding:2rem}}
  h1{{color:#58a6ff;margin-bottom:0.25rem}}
  p.sub{{color:#8b949e;margin-top:0;margin-bottom:1.5rem}}
  .card{{background:#161b22;border:1px solid #30363d;padding:1.2rem;border-radius:8px;margin:1rem 0}}
  a{{color:#58a6ff;text-decoration:none}} a:hover{{text-decoration:underline}}
  .badge{{display:inline-block;background:#238636;color:#ffffff;padding:2px 8px;border-radius:4px;font-size:0.85em;font-weight:bold}}
  ul{{list-style-type:none;padding-left:0}}
  li{{margin:0.5rem 0}}
</style></head>
<body>
<h1>Cellos Mini-Server Dashboard</h1>
<p class="sub">Stage G2 &bull; Cellular Single Address Space OS</p>
<div class="card">
  <p>Server Status: <span class="badge">ONLINE</span></p>
  <p>Hardware Architecture: <strong>{}</strong></p>
  <p>HTTP Endpoint: <strong>Port 8080 (HTTP/1.1)</strong></p>
</div>
<div class="card">
  <h3>Endpoints &amp; Navigation</h3>
  <ul>
    <li><a href="/status">&#9658; System Status</a></li>
    <li><a href="/api/system">&#9658; REST API: /api/system</a></li>
    <li><a href="/api/cells">&#9658; REST API: /api/cells (Active Services)</a></li>
    <li><a href="/api/files?path=/tmp">&#9658; REST API: /api/files?path=/tmp (VFS Browser)</a></li>
    <li><a href="/files/readme.txt">&#9658; Static File: /files/readme.txt</a></li>
  </ul>
</div>
</body></html>"#,
        ARCH
    );
    response(200, "text/html; charset=utf-8", html.as_bytes(), false)
}

pub(crate) fn status_page() -> Response {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>Cellos - System Status</title>
<style>
  body{{font-family:system-ui,-apple-system,sans-serif;background:#0d1117;color:#c9d1d9;margin:0;padding:2rem}}
  h1{{color:#58a6ff;margin-bottom:1rem}}
  .card{{background:#161b22;border:1px solid #30363d;padding:1.2rem;border-radius:8px;margin:1rem 0}}
  table{{border-collapse:collapse;width:100%}} td{{padding:8px 12px;border-bottom:1px solid #21262d}}
  a{{color:#58a6ff;text-decoration:none}} a:hover{{text-decoration:underline}}
</style></head>
<body>
<h1>System Status</h1>
<div class="card">
<table>
  <tr><td>Architecture</td><td><strong>{}</strong></td></tr>
  <tr><td>HTTP Server</td><td><strong>port 8080</strong></td></tr>
  <tr><td>Protocol</td><td><strong>HTTP/1.1</strong></td></tr>
  <tr><td>Isolation Model</td><td><strong>SAS / Language-Based Isolation (LBI)</strong></td></tr>
</table>
</div>
<p><a href="/">&#8592; Back to Dashboard</a></p>
</body></html>"#,
        ARCH
    );
    response(200, "text/html; charset=utf-8", html.as_bytes(), false)
}

pub(crate) fn not_found() -> Response {
    response(404, "text/plain", b"404 Not Found", false)
}

pub(crate) fn mime_from_ext(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

// ── JSON REST API ─────────────────────────────────────────────────────────────

pub(crate) fn api_status(inflight: usize, peak: usize, refused: usize) -> Response {
    let text = format!(
        r#"{{"status":"running","arch":"{}","http_port":8080,"protocol":"HTTP/1.1","accepted_inflight":{},"accepted_peak":{},"refused":{}}}"#,
        ARCH, inflight, peak, refused
    );
    json(200, &text)
}

pub(crate) fn api_cells() -> Response {
    // Well-known service IDs probed via service registry
    use ostd::service::{lookup, service};
    let mut entries = Vec::new();
    let known: &[(&str, u16)] = &[
        ("net", service::NET),
        ("vfs", service::VFS),
        ("input", service::INPUT),
        ("compositor", service::COMPOSITOR),
        ("config", service::CONFIG),
    ];
    for (name, id) in known {
        if let Some(tid) = lookup(*id) {
            entries.push(format!(
                r#"{{"name":"{}","tid":{},"state":"Running"}}"#,
                name, tid
            ));
        }
    }
    let list = entries.join(",");
    let text = format!(r#"{{"cells":[{}]}}"#, list);
    json(200, &text)
}

pub(crate) fn api_files(path: &str, raw: &[u8]) -> Response {
    let listing = core::str::from_utf8(raw).unwrap_or("");
    let entries: Vec<String> = listing.lines().filter(|l| !l.is_empty())
        .map(|name| format!(r#"{{"name":"{}"}}"#, name)).collect();
    let text = format!(r#"{{"path":"{}","entries":[{}]}}"#, path, entries.join(","));
    json(200, &text)
}

#[cfg(target_os = "none")]
pub(crate) fn infer_prompt(body: &[u8]) -> Result<&str, Response> {
    if body.is_empty() {
        return Err(json(400, r#"{"error":"body must contain the prompt"}"#));
    }
    if body.len() > MAX_PROMPT_BYTES {
        return Err(json(400, r#"{"error":"prompt is too large"}"#));
    }
    core::str::from_utf8(body).map_err(|_| json(400, r#"{"error":"prompt is not UTF-8"}"#))
}

#[cfg(target_os = "none")]
pub(crate) fn infer_success(model: &str, prompt_bytes: usize, tokens: usize, finish: ai_proto::FinishReason, text: &str) -> Response {
    let text = format!(
        r#"{{"model":"{}","prompt_bytes":{},"tokens":{},"finish":"{:?}","text":"{}"}}"#,
        json_escape(model), prompt_bytes, tokens, finish, json_escape(text)
    );
    json(200, &text)
}

#[cfg(target_os = "none")]
pub(crate) fn infer_failed(error: &str) -> Response {
    json(503, &format!(r#"{{"error":"inference unavailable","cause":"{}"}}"#, json_escape(error)))
}

#[cfg(target_os = "none")]
/// Escape a string for a JSON string literal.
fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

pub(crate) fn api_restart() -> Response {
    // Restart via init IPC is not yet implemented — preserve the existing accepted response.
    json(200, r#"{"ok":true,"note":"restart not yet wired to init"}"#)
}
