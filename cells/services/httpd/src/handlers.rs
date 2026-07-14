// HTTP response handlers: HTML pages, VFS file serving, and JSON REST API.

extern crate alloc;
use alloc::{format, string::String, vec::Vec};

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

// ── Response helpers ──────────────────────────────────────────────────────────

pub fn send_response(cap: u32, net_ep: usize, status: u16, content_type: &str, body: &[u8]) {
    let status_text = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        status,
        status_text,
        content_type,
        body.len()
    );
    net_ipc::tcp_send_all(cap, net_ep, header.as_bytes());
    net_ipc::tcp_send_all(cap, net_ep, body);
}

fn send_json(cap: u32, net_ep: usize, status: u16, json: &str) {
    let body = json.as_bytes();
    let status_text = if status == 200 { "OK" } else { "Not Found" };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status, status_text, body.len()
    );
    net_ipc::tcp_send_all(cap, net_ep, header.as_bytes());
    net_ipc::tcp_send_all(cap, net_ep, body);
}

// ── HTML pages ────────────────────────────────────────────────────────────────

// Note: askama compile-time templates are the intended long-term approach.
// format! is used here for simplicity pending no_std askama validation.

pub fn index(cap: u32, net_ep: usize) {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ViCell Dashboard</title>
<style>
  body{{font-family:monospace;background:#1a1a2e;color:#e0e0e0;margin:0;padding:2rem}}
  h1{{color:#a0c4ff;margin-bottom:0.5rem}}
  .card{{background:#16213e;padding:1.2rem;border-radius:8px;margin:1rem 0}}
  a{{color:#a0c4ff;text-decoration:none}} a:hover{{text-decoration:underline}}
  .badge{{display:inline-block;background:#0f3460;padding:2px 8px;border-radius:4px;font-size:0.85em}}
</style></head>
<body>
<h1>ViCell Dashboard</h1>
<div class="card">
  <p>Status: <span class="badge">Running</span></p>
  <p>Architecture: <strong>{}</strong></p>
</div>
<div class="card">
  <p><a href="/status">&#9658; System Status</a></p>
  <p><a href="/api/status">&#9658; JSON API: /api/status</a></p>
  <p><a href="/api/cells">&#9658; JSON API: /api/cells</a></p>
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
    );
}

pub fn status_page(cap: u32, net_ep: usize) {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ViCell Status</title>
<style>
  body{{font-family:monospace;background:#1a1a2e;color:#e0e0e0;margin:0;padding:2rem}}
  h1{{color:#a0c4ff}} .card{{background:#16213e;padding:1.2rem;border-radius:8px;margin:1rem 0}}
  a{{color:#a0c4ff}}
  table{{border-collapse:collapse;width:100%}} td{{padding:4px 8px}}
  tr:nth-child(even){{background:#0f3460}}
</style></head>
<body>
<h1>System Status</h1>
<div class="card">
<table>
  <tr><td>Architecture</td><td><strong>{}</strong></td></tr>
  <tr><td>HTTP Server</td><td><strong>port 8080</strong></td></tr>
  <tr><td>Protocol</td><td><strong>HTTP/1.1</strong></td></tr>
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
    );
}

pub fn not_found(cap: u32, net_ep: usize) {
    send_response(cap, net_ep, 404, "text/plain", b"404 Not Found");
}

// ── Static file serving ───────────────────────────────────────────────────────

pub fn serve_file(cap: u32, net_ep: usize, vfs_ep: usize, path: &str) {
    let content_type = mime_from_ext(path);
    let data = net_ipc::vfs_read_file(path, vfs_ep);
    if data.is_empty() {
        not_found(cap, net_ep);
    } else {
        send_response(cap, net_ep, 200, content_type, &data);
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

pub fn api_status(cap: u32, net_ep: usize) {
    let json = format!(
        r#"{{"status":"running","arch":"{}","http_port":8080,"protocol":"HTTP/1.1"}}"#,
        ARCH
    );
    send_json(cap, net_ep, 200, &json);
}

pub fn api_cells(cap: u32, net_ep: usize) {
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
    send_json(cap, net_ep, 200, &json);
}

pub fn api_files(cap: u32, net_ep: usize, vfs_ep: usize, path: &str) {
    let raw = net_ipc::vfs_list_dir(path, vfs_ep);
    let listing = core::str::from_utf8(&raw).unwrap_or("");
    let entries: Vec<String> = listing
        .lines()
        .filter(|l| !l.is_empty())
        .map(|name| format!(r#"{{"name":"{}"}}"#, name))
        .collect();
    let list = entries.join(",");
    let json = format!(r#"{{"path":"{}","entries":[{}]}}"#, path, list);
    send_json(cap, net_ep, 200, &json);
}

pub fn api_restart(cap: u32, net_ep: usize, _cell_name: &str) {
    // Restart via init IPC is not yet implemented — return accepted.
    send_json(
        cap,
        net_ep,
        200,
        r#"{"ok":true,"note":"restart not yet wired to init"}"#,
    );
}
