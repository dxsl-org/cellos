// HTTP response handlers: HTML pages, VFS file serving, and JSON REST API.

extern crate alloc;
use ai_proto::MAX_PROMPT_BYTES;
use ai_sdk::AiClient;
use alloc::{format, string::String, vec::Vec};
use api::ipc::{VfsRequest, VfsResponse, IPC_BUF_SIZE};
use ostd::clients::VfsClient;
use ostd::ipc::service_call_typed;

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

use crate::net_ipc;
use crate::static_files::{
    classify_static_file_preflight_wire, classify_static_file_read, StaticFileResult,
    STATIC_FILE_MAX_BYTES,
};

// ── Response helpers ──────────────────────────────────────────────────────────

pub fn send_response(
    cap: u32,
    net_ep: usize,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> bool {
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        status_text,
        content_type,
        body.len()
    );
    let mut payload = header.into_bytes();
    payload.extend_from_slice(body);
    net_ipc::tcp_send_all(cap, net_ep, &payload)
}
fn send_json(cap: u32, net_ep: usize, status: u16, json: &str) -> bool {
    let body = json.as_bytes();
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status, status_text, body.len()
    );
    let payload = format!("{header}{json}");
    net_ipc::tcp_send_all(cap, net_ep, payload.as_bytes())
}
// ── HTML pages ────────────────────────────────────────────────────────────────

// Note: askama compile-time templates are the intended long-term approach.
// format! is used here for simplicity pending no_std askama validation.

pub fn index(cap: u32, net_ep: usize) -> bool {
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
    send_response(
        cap,
        net_ep,
        200,
        "text/html; charset=utf-8",
        html.as_bytes(),
    )
}

pub fn status_page(cap: u32, net_ep: usize) -> bool {
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
    send_response(
        cap,
        net_ep,
        200,
        "text/html; charset=utf-8",
        html.as_bytes(),
    )
}

pub fn not_found(cap: u32, net_ep: usize) -> bool {
    send_response(cap, net_ep, 404, "text/plain", b"404 Not Found")
}

// ── Static file serving ───────────────────────────────────────────────────────

pub fn serve_file(cap: u32, net_ep: usize, vfs_ep: usize, path: &str) -> bool {
    let mut vfs = VfsClient::new();
    let mut stat_req = [0u8; IPC_BUF_SIZE];
    let mut stat_resp = [0u8; IPC_BUF_SIZE];
    match classify_static_file_preflight_wire(service_call_typed::<_, VfsResponse>(
        vfs_ep,
        &VfsRequest::Stat(path),
        &mut stat_req,
        &mut stat_resp,
    )) {
        Ok(()) => {}
        Err(StaticFileResult::NotFound) => return not_found(cap, net_ep),
        Err(_) => {
            return send_response(cap, net_ep, 500, "text/plain", b"500 Internal Server Error");
        }
    }
    let content_type = mime_from_ext(path);
    match classify_static_file_read(vfs.read_file_bounded(path, STATIC_FILE_MAX_BYTES)) {
        StaticFileResult::Body(data) => send_response(cap, net_ep, 200, content_type, &data),
        StaticFileResult::NotFound => not_found(cap, net_ep),
        StaticFileResult::InternalError => {
            send_response(cap, net_ep, 500, "text/plain", b"500 Internal Server Error")
        }
    }
}

fn mime_from_ext(path: &str) -> &'static str {
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

pub fn api_status(cap: u32, net_ep: usize) -> bool {
    let json = format!(
        r#"{{"status":"running","arch":"{}","http_port":8080,"protocol":"HTTP/1.1"}}"#,
        ARCH
    );
    send_json(cap, net_ep, 200, &json)
}

pub fn api_cells(cap: u32, net_ep: usize) -> bool {
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
    let json = format!(r#"{{"cells":[{}]}}"#, list);
    send_json(cap, net_ep, 200, &json)
}

pub fn api_files(cap: u32, net_ep: usize, vfs_ep: usize, path: &str) -> bool {
    let raw = net_ipc::vfs_list_dir(path, vfs_ep);
    let listing = core::str::from_utf8(&raw).unwrap_or("");
    let entries: Vec<String> = listing
        .lines()
        .filter(|l| !l.is_empty())
        .map(|name| format!(r#"{{"name":"{}"}}"#, name))
        .collect();
    let list = entries.join(",");
    let json = format!(r#"{{"path":"{}","entries":[{}]}}"#, path, list);
    send_json(cap, net_ep, 200, &json)
}

/// `POST /api/infer` — run the local AI inference service and return the completion as JSON.
///
/// The request body is the prompt, verbatim: no JSON envelope and therefore no parser in the trusted
/// path. `?max_tokens=N` sets the cap (default [`INFER_DEFAULT_TOKENS`], clamped to
/// [`INFER_MAX_TOKENS`] so the reply fits one TCP payload).
/// The prompt is bounded by the AI wire limit and the generated text is capped at 64 tokens, so the
/// JSON reply remains a bounded one-shot response. Callers that need streaming use the service's
/// poll API directly (`ai_sdk::AiClient`) rather than this front door.
pub fn api_infer(cap: u32, net_ep: usize, request: &[u8], path: &str) -> bool {
    let prompt = match request_body(request) {
        Some(body) if !body.is_empty() => {
            if body.len() > MAX_PROMPT_BYTES {
                return send_json(cap, net_ep, 400, r#"{"error":"prompt is too large"}"#);
            }
            match core::str::from_utf8(body) {
                Ok(text) => text,
                Err(_) => {
                    return send_json(cap, net_ep, 400, r#"{"error":"prompt is not UTF-8"}"#);
                }
            }
        }
        _ => {
            return send_json(
                cap,
                net_ep,
                400,
                r#"{"error":"body must contain the prompt"}"#,
            );
        }
    };

    let max_tokens = crate::router::extract_query_param(path, "max_tokens")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(INFER_DEFAULT_TOKENS)
        .clamp(1, INFER_MAX_TOKENS);

    // The client resolves the service through the registry, so this works whether init or the shell
    // spawned the inference cell.
    let mut client = AiClient::new(ai_sdk::ostd_transport::OstdTransport::new());
    // The reply names the model that actually served it, so a caller can tell which deployment it
    // reached without a second endpoint.
    let model = client.describe().map(|info| info.model).unwrap_or_default();
    let params = ai_sdk::InferParams::greedy(prompt, max_tokens);
    let generation = match client.generate(&params, INFER_MAX_POLLS) {
        Ok(generation) => generation,
        Err(error) => {
            // Report the typed refusal instead of an empty 200: a caller must be able to tell
            // "no model" from "no service" from "generation failed".
            let json = format!(
                r#"{{"error":"inference unavailable","cause":"{:?}"}}"#,
                error
            );
            return send_json(cap, net_ep, 503, &json);
        }
    };

    let json = format!(
        r#"{{"model":"{}","prompt_bytes":{},"tokens":{},"finish":"{:?}","text":"{}"}}"#,
        json_escape(model.as_str()),
        prompt.len(),
        generation.ids.len(),
        generation.finish,
        json_escape(generation.text.as_str())
    );
    send_json(cap, net_ep, 200, &json)
}

/// Tokens generated when the request does not ask for a count.
const INFER_DEFAULT_TOKENS: u16 = 24;

/// Hard cap for this endpoint: the reply has to fit one inline TCP payload alongside its JSON.
const INFER_MAX_TOKENS: u16 = 64;

/// Poll round trips allowed per request; the service advances four model steps per poll.
const INFER_MAX_POLLS: usize = 96;

/// The request body: everything after the header terminator.
fn request_body(request: &[u8]) -> Option<&[u8]> {
    let separator = b"\r\n\r\n";
    let start = request
        .windows(separator.len())
        .position(|window| window == separator)?
        + separator.len();
    request.get(start..)
}

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

pub fn api_restart(cap: u32, net_ep: usize, _cell_name: &str) -> bool {
    // Restart via init IPC is not yet implemented — return accepted.
    send_json(
        cap,
        net_ep,
        200,
        r#"{"ok":true,"note":"restart not yet wired to init"}"#,
    )
}
