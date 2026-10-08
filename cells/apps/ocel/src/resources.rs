// SPDX-License-Identifier: MIT
//! Same-origin classic script and stylesheet loading. No cross-origin requests.
extern crate alloc;
use alloc::{format, string::String, vec::Vec};
const MAX_RESOURCE_BYTES: usize = 256 * 1024;

pub fn resolve(base: &str, reference: &str) -> Result<String, String> {
    let reference = reference.trim();
    let base = base.split('#').next().unwrap_or(base);
    let (origin, base_path) = if let Some((scheme, rest)) = base.split_once("://") {
        if !matches!(scheme, "http" | "https" | "file") { return Err(String::from("Unsupported resource scheme")); }
        let split = rest.find('/').unwrap_or(rest.len());
        (format!("{}://{}", scheme, &rest[..split]), &rest[split..])
    } else { (String::new(), base) };
    let reference = if reference.contains("://") {
        let suffix = reference.strip_prefix(&origin).ok_or_else(|| String::from("Cross-origin resources are not supported"))?;
        if origin.is_empty() || !suffix.starts_with('/') { return Err(String::from("Cross-origin resources are not supported")); }
        suffix
    } else {
        if reference.starts_with("//") || reference.split('/').next().unwrap_or("").contains(':') { return Err(String::from("Unsupported resource URL")); }
        reference
    };
    let base_path = base_path.split('?').next().unwrap_or(base_path);
    let reference = reference.split('#').next().unwrap_or(reference);
    let joined = if reference.is_empty() { String::from(base_path) }
        else if reference.starts_with('/') { String::from(reference) }
        else if reference.starts_with('?') { format!("{}{}", base_path, reference) }
        else { format!("{}/{}", base_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(""), reference) };
    let (path, query) = joined.split_once('?').map(|(p,q)| (p,Some(q))).unwrap_or((&joined,None));
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') { match part { "" | "." => {}, ".." => { parts.pop(); }, _ => parts.push(part) } }
    let mut result = format!("{}/{}", origin, parts.join("/"));
    if let Some(query) = query { result.push('?'); result.push_str(query); }
    Ok(result)
}

pub fn load_text(base: &str, reference: &str) -> Result<String, String> {
    let url = resolve(base, reference)?;
    let content = if url.starts_with("https://") { crate::net::fetch_https(&url)? }
        else if url.starts_with("http://") { crate::net::fetch_http(&url)? }
        else {
            use ostd::fs::File;
            let mut file = File::open(url.strip_prefix("file://").unwrap_or(&url)).map_err(|_| format!("Cannot open {}", url))?;
            let mut bytes = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let count = file.read(&mut chunk).map_err(|_| format!("Cannot read {}", url))?;
                if count == 0 { break; }
                if bytes.len() + count > MAX_RESOURCE_BYTES { return Err(String::from("Resource exceeds 256 KiB")); }
                bytes.extend_from_slice(&chunk[..count]);
            }
            String::from_utf8(bytes).map_err(|_| String::from("Resource is not UTF-8"))?
        };
    if content.len() > MAX_RESOURCE_BYTES { return Err(String::from("Resource exceeds 256 KiB")); }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_resource_resolution_and_origin_boundary() {
        assert_eq!(resolve("https://example.com/a/b.html?q=1#x", "../js/app.js?v=2").unwrap(), "https://example.com/js/app.js?v=2");
        assert_eq!(resolve("file:///data/web/index.html", "./style.css").unwrap(), "file:///data/web/style.css");
        assert_eq!(resolve("https://example.com/a.html", "/css/site.css").unwrap(), "https://example.com/css/site.css");
        for url in ["https://example.com.evil/x", "https://evil/x", "//evil/x", "javascript:alert(1)", "data:text/plain,x"] {
            assert!(resolve("https://example.com/a.html", url).is_err());
        }
    }
}
