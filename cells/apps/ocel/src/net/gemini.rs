// SPDX-License-Identifier: MIT
//! Gemini Protocol (gemini://) client & Gemtext parser for Ocel.
//! Standard port: 1965, encrypted via TLS 1.3.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use api::syscall::service;
use ostd::syscall::{sys_lookup_service, sys_yield};
use crate::doc::{DocNode, StyledSpan};
use crate::draw::theme;

const MAX_GEMINI_PAYLOAD: usize = 1024 * 1024; // 1 MiB

pub struct GeminiResponse {
    pub status: u8,
    pub meta: String,
    pub body: String,
}

pub fn parse_gemini_url(url: &str) -> Option<(&str, u16, &str)> {
    let rest = url.strip_prefix("gemini://")?;
    if rest.is_empty() {
        return None;
    }
    let (host_port, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match host_port.rfind(':') {
        Some(i) => {
            let p = host_port[i + 1..].parse::<u16>().ok()?;
            (&host_port[..i], p)
        }
        None => (host_port, 1965),
    };
    if host.is_empty() {
        return None;
    }
    Some((host, port, path))
}

pub fn fetch_gemini(url: &str) -> Result<GeminiResponse, String> {
    let (host, port, _) = parse_gemini_url(url).ok_or_else(|| String::from("Invalid gemini:// URL"))?;

    let net_ep = sys_lookup_service(service::NET)
        .ok_or_else(|| String::from("Net service unavailable in CellOS"))?;

    // 1. Resolve host
    let mut req_buf = [0u8; api::ipc::IPC_BUF_SIZE];
    let len = api::ipc::encode(&api::ipc::NetRequest::Resolve { hostname: host }, &mut req_buf)
        .ok()
        .map(|b| b.len())
        .ok_or_else(|| format!("Cannot encode DNS resolve request for {}", host))?;

    ostd::syscall::sys_send(net_ep, &req_buf[..len]);
    let mut resp_buf = [0u8; api::ipc::IPC_BUF_SIZE];
    let addr = match ostd::ipc::recv_from(net_ep, &mut resp_buf) {
        Ok(_) => match api::ipc::decode::<api::ipc::NetResponse>(&resp_buf) {
            Ok(api::ipc::NetResponse::Addr(a)) => a,
            _ => return Err(format!("Cannot resolve Gemini host: {}", host)),
        },
        _ => return Err(format!("DNS query failed for {}", host)),
    };

    // 2. Open TLS 1.3 connection to port (default 1965)
    let cap_id = ostd::tls::tls_connect(net_ep, addr, port, host);
    if cap_id == 0 {
        return Err(format!("Gemini TLS connection failed to {}:{}", host, port));
    }

    // 3. Send request URL followed by CRLF
    let req_line = format!("{}\r\n", url);
    let req_bytes = req_line.as_bytes();
    let mut sent = 0usize;
    for _ in 0..100 {
        if sent >= req_bytes.len() {
            break;
        }
        let chunk = &req_bytes[sent..];
        let n = ostd::tls::tls_write(net_ep, cap_id, chunk);
        if n > 0 {
            sent += n;
        } else {
            sys_yield();
        }
    }

    // 4. Read response
    let mut response: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk_buf = [0u8; 1024];
    for _ in 0..1500 {
        let n = ostd::tls::tls_read(net_ep, cap_id, &mut chunk_buf);
        if n > 0 {
            response.extend_from_slice(&chunk_buf[..n]);
            if response.len() >= MAX_GEMINI_PAYLOAD {
                break;
            }
        } else {
            sys_yield();
        }
    }
    ostd::tls::tls_close(net_ep, cap_id);

    if response.is_empty() {
        return Err(String::from("Empty response from Gemini server"));
    }

    // 5. Parse Header: <STATUS> <META>\r\n
    let header_end = response.windows(2).position(|w| w == b"\r\n")
        .ok_or_else(|| String::from("Malformed Gemini header (missing CRLF)"))?;

    let header_str = core::str::from_utf8(&response[..header_end])
        .map_err(|_| String::from("Non-UTF8 Gemini header"))?;

    let mut parts = header_str.splitn(2, ' ');
    let status_code = parts.next().unwrap_or("").parse::<u8>().unwrap_or(0);
    let meta = parts.next().unwrap_or("").trim().into();
    let body_bytes = &response[header_end + 2..];
    let body = String::from_utf8_lossy(body_bytes).into();

    Ok(GeminiResponse {
        status: status_code,
        meta,
        body,
    })
}

/// Parse Gemtext (text/gemini) into native CellOS DocNodes.
pub fn parse_gemtext(input: &str, base_url: &str) -> Vec<DocNode> {
    let mut nodes = Vec::new();
    let mut in_preformatted = false;
    let mut pre_alt = String::new();
    let mut pre_lines = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();

        // 1. Toggle preformatted mode ```
        if trimmed.starts_with("```") {
            if in_preformatted {
                nodes.push(DocNode::CodeBlock {
                    lang: core::mem::take(&mut pre_alt),
                    lines: core::mem::take(&mut pre_lines),
                });
                in_preformatted = false;
            } else {
                in_preformatted = true;
                pre_alt = String::from(trimmed.trim_start_matches("```").trim());
            }
            continue;
        }

        if in_preformatted {
            pre_lines.push(String::from(line));
            continue;
        }

        // 2. Empty line
        if trimmed.is_empty() {
            continue;
        }

        // 3. Headings: #, ##, ###
        if let Some(rest) = trimmed.strip_prefix("### ") {
            nodes.push(DocNode::Heading { level: 3, text: rest.trim().into() });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            nodes.push(DocNode::Heading { level: 2, text: rest.trim().into() });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            nodes.push(DocNode::Heading { level: 1, text: rest.trim().into() });
            continue;
        }

        // 4. Links: => URL [Optional label]
        if let Some(rest) = trimmed.strip_prefix("=>") {
            let link_content = rest.trim();
            let mut parts = link_content.split_whitespace();
            if let Some(raw_url) = parts.next() {
                let label: String = parts.collect::<Vec<&str>>().join(" ");
                let display_label = if label.is_empty() { raw_url } else { &label };
                let target_url = if raw_url.contains("://") {
                    String::from(raw_url)
                } else {
                    crate::resources::resolve(base_url, raw_url).unwrap_or_else(|_| String::from(raw_url))
                };
                let mut span = StyledSpan::plain(&format!("🚀 {}", display_label));
                span.link = Some(target_url);
                span.color = Some(theme::ACCENT_CYAN);
                span.bold = true;
                nodes.push(DocNode::Paragraph { spans: alloc::vec![span] });
            }
            continue;
        }

        // 5. Unordered List: * Item
        if let Some(rest) = trimmed.strip_prefix("* ") {
            nodes.push(DocNode::ListItem {
                bullet: '•',
                indent: 0,
                spans: alloc::vec![StyledSpan::plain(rest.trim())],
            });
            continue;
        }

        // 6. Blockquote: > Quote
        if let Some(rest) = trimmed.strip_prefix('>') {
            nodes.push(DocNode::Blockquote {
                lines: alloc::vec![String::from(rest.trim())],
            });
            continue;
        }

        // 7. Standard Text Paragraph
        nodes.push(DocNode::Paragraph {
            spans: alloc::vec![StyledSpan::plain(trimmed)],
        });
    }

    if in_preformatted && !pre_lines.is_empty() {
        nodes.push(DocNode::CodeBlock {
            lang: pre_alt,
            lines: pre_lines,
        });
    }

    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gemini_url() {
        assert_eq!(
            parse_gemini_url("gemini://geminispace.info/search"),
            Some(("geminispace.info", 1965, "/search"))
        );
        assert_eq!(
            parse_gemini_url("gemini://example.com:1966/page"),
            Some(("example.com", 1966, "/page"))
        );
        assert_eq!(
            parse_gemini_url("gemini://rawtext.club"),
            Some(("rawtext.club", 1965, "/"))
        );
        assert_eq!(parse_gemini_url("http://example.com"), None);
    }

    #[test]
    fn test_parse_gemtext_elements() {
        let gemtext = "# Gemini Title\n## Subtitle\n* Bullet item 1\n* Bullet item 2\n> This is a quote from Geminispace\n=> /about About this capsule\n=> https://cellos.org CellOS Official Site\n```rust\nfn main() {}\n```\nFinal paragraph text.\n";
        let nodes = parse_gemtext(gemtext, "gemini://example.com/root");
        assert!(nodes.iter().any(|n| match n {
            DocNode::Heading { level, text } => *level == 1 && text == "Gemini Title",
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::ListItem { spans, .. } => spans.iter().any(|s| s.text == "Bullet item 1"),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::Blockquote { lines } => lines.iter().any(|l| l == "This is a quote from Geminispace"),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::Paragraph { spans } => spans.iter().any(|s| s.link == Some(String::from("https://cellos.org"))),
            _ => false,
        }));
        assert!(nodes.iter().any(|n| match n {
            DocNode::CodeBlock { lines, .. } => lines.iter().any(|l| l == "fn main() {}"),
            _ => false,
        }));
    }
}
