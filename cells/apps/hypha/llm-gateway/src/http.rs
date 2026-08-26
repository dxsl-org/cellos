//! Minimal HTTP/1.0 helpers and strict JSON extraction for the LLM gateway.

use crate::json_validation::parse_unique;
use agent_proto::ToolCall;
use alloc::format;
use alloc::string::String;

/// Build an OpenAI-compatible chat-completion JSON body with a single user turn.
pub fn build_chat_body(model: &str, prompt: &str) -> String {
    format!(
        "{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"{}\"}}]}}",
        json_escape(model),
        json_escape(prompt),
    )
}

/// Build an HTTP/1.0 POST with `Connection: close` (so the server closes the
/// stream when the body is done — our read loop relies on that).
pub fn build_post(host: &str, path: &str, body: &str) -> String {
    format!(
        "POST {} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        host,
        body.len(),
        body,
    )
}

/// Escape a string for use inside a JSON string literal.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Return the body slice after the first `\r\n\r\n` header terminator.
pub fn http_body(resp: &[u8]) -> Option<&[u8]> {
    resp.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| &resp[i + 4..])
}

/// If `content` starts with `TOOL_CALL:` (ReAct-style), parse and return a
/// [`ToolCall`]. Returns `None` for ordinary text replies.
///
/// Expected format (LLM must emit ONLY this, nothing else):
/// `TOOL_CALL: {"name":"tool_name","args":{...}}`
pub fn extract_tool_call(content: &str) -> Option<ToolCall> {
    let s = content.trim_start_matches([' ', '\t', '\n', '\r']);
    let rest = s.strip_prefix("TOOL_CALL:")?;
    let value = parse_unique(rest.as_bytes())?;
    let object = value.as_object()?;
    let name = object.get("name")?.as_str()?;
    let args_json = match object.get("args") {
        Some(args) if args.is_object() => ostd::json::to_string(args).ok()?,
        Some(_) => return None,
        None => String::from("{}"),
    };
    Some(ToolCall {
        name: String::from(name),
        args_json,
    })
}

/// Extract `choices[0].message.content` from a complete provider response.
///
/// Malformed input, duplicate keys, trailing data, and non-string content are
/// rejected so ambiguous provider responses fail closed.
pub fn extract_content(body: &[u8]) -> Option<String> {
    let value = parse_unique(body)?;
    let content = value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()?;
    Some(String::from(content))
}

#[cfg(test)]
#[path = "http-tests.rs"]
mod tests;
