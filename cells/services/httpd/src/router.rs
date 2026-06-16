// HTTP request router: parse method + path, dispatch to handler.

extern crate alloc;

use crate::{handlers, net_ipc};

/// Dispatch one HTTP/1.1 request received on `cap` to the appropriate handler.
pub fn handle_connection(cap: u32, net_ep: usize, vfs_ep: usize) {
    let raw = net_ipc::recv_request(cap, net_ep);
    if raw.is_empty() {
        return;
    }

    let mut header_buf = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut header_buf);

    let (method, path) = match req.parse(&raw) {
        Ok(_) => (
            req.method.unwrap_or("GET"),
            req.path.unwrap_or("/"),
        ),
        Err(_) => {
            handlers::send_response(cap, net_ep, 400, "text/plain", b"Bad Request");
            return;
        }
    };

    // Strip query string for routing (keep it for specific handlers that need it)
    let path_only = path.split('?').next().unwrap_or(path);

    match (method, path_only) {
        ("GET",  "/")        => handlers::index(cap, net_ep),
        ("GET",  "/status")  => handlers::status_page(cap, net_ep),

        // Static file serving from VFS: /files/<vfs_path>
        ("GET", p) if p.starts_with("/files/") => {
            handlers::serve_file(cap, net_ep, vfs_ep, &p["/files/".len()..]);
        }

        // JSON REST API
        ("GET",  "/api/status") => handlers::api_status(cap, net_ep),
        ("GET",  "/api/cells")  => handlers::api_cells(cap, net_ep),
        ("GET",  "/api/files")  => {
            let vfs_path = extract_query_param(path, "path").unwrap_or("/");
            handlers::api_files(cap, net_ep, vfs_ep, vfs_path);
        }

        // POST /api/cells/<name>/restart
        ("POST", p) if p.starts_with("/api/cells/") && p.ends_with("/restart") => {
            let inner = &p["/api/cells/".len()..];
            let name = inner.trim_end_matches("/restart");
            handlers::api_restart(cap, net_ep, name);
        }

        _ => handlers::not_found(cap, net_ep),
    }
}

/// Extract `?key=value` from a query string. Returns the value or None.
fn extract_query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
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
