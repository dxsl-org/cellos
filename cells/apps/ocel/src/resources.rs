// SPDX-License-Identifier: MIT
//! Same-origin, bounded resources. Parsing only reads a prefetched bundle.
extern crate alloc;
use alloc::{format, string::String, sync::Arc, vec::Vec};
pub const MAX_RESOURCE_BYTES: usize = 256 * 1024;
pub const MAX_IMAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_REFERENCES: usize = 64;

#[derive(Default)]
pub struct ResourceBundle { entries: Vec<ResourceEntry>, bytes: usize }
struct ResourceEntry { url: String, image: bool, data: Result<Arc<Vec<u8>>, String> }
impl ResourceBundle {
    pub fn resident_bytes(&self) -> usize {
        self.bytes + self.entries.capacity() * core::mem::size_of::<ResourceEntry>()
            + self.entries.iter().map(|entry| entry.url.capacity() + match &entry.data { Ok(_) => 0, Err(error) => error.capacity() }).sum::<usize>()
    }
    pub fn estimated_decoded_bytes(&self) -> Result<usize, String> {
        let mut total = 0usize; let mut count = 0usize;
        for entry in self.entries.iter().filter(|entry| entry.image) {
            if let Ok(bytes) = &entry.data {
                match crate::budget::image_bytes(bytes) {
                    Ok(size) => { total = total.checked_add(size).ok_or("Image byte budget overflow")?; count += 1; }
                    Err(error) if error.contains("budget") => return Err(error),
                    Err(_) => continue,
                }
                if count > crate::budget::MAX_IMAGES || total > crate::budget::MAX_IMAGE_TOTAL_BYTES {
                    return Err("Document exceeds aggregate image budget".into());
                }
            }
        }
        Ok(total)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &str> { self.entries.iter().filter_map(|entry| entry.data.as_ref().err().map(String::as_str)) }
    pub(crate) fn insert(&mut self, url: String, result: Result<Vec<u8>, String>, image: bool) -> Result<(), String> {
        if self.entries.iter().any(|entry| entry.url == url) { return Ok(()); }
        if self.entries.len() >= MAX_REFERENCES { return Err("Too many document resources".into()); }
        let data = match result {
            Ok(bytes) => {
                if bytes.capacity() > crate::budget::MAX_RESOURCE_BYTES.saturating_sub(self.bytes) { return Err("Document resource budget exceeded".into()); }
                self.bytes += bytes.capacity() + core::mem::size_of::<Vec<u8>>() + 2 * core::mem::size_of::<usize>(); Ok(Arc::new(bytes))
            }
            Err(error) => Err(error),
        };
        self.entries.push(ResourceEntry { url, image, data }); Ok(())
    }
    pub(crate) fn remaining_bytes(&self) -> usize { crate::budget::MAX_RESOURCE_BYTES.saturating_sub(self.bytes) }
}
static ACTIVE: ostd::sync::Mutex<ResourceBundle> = ostd::sync::Mutex::new(ResourceBundle { entries: Vec::new(), bytes: 0 });
/// The caller temporarily activates one tab's bundle around parse/extraction.
pub fn replace(bundle: ResourceBundle) -> ResourceBundle { core::mem::replace(&mut *ACTIVE.lock(), bundle) }

pub fn resolve(base: &str, reference: &str) -> Result<String, String> {
    let reference = reference.trim();
    if reference.len() > 2048 || reference.bytes().any(|b| b < 32 || b == 127) { return Err("Invalid/overlong resource URL".into()); }
    let base = base.split('#').next().unwrap_or(base);
    let (origin, base_path) = if let Some((scheme, rest)) = base.split_once("://") {
        if !matches!(scheme, "http" | "https" | "gemini" | "file") { return Err("Unsupported resource scheme".into()); }
        let split = rest.find(|c| c == '/' || c == '?').unwrap_or(rest.len());
        (format!("{}://{}", scheme, &rest[..split]), &rest[split..])
    } else { (String::new(), base) };
    let reference = if reference.contains("://") {
        let suffix = reference.strip_prefix(&origin).ok_or("Cross-origin resources are not supported")?;
        if origin.is_empty() || !suffix.starts_with('/') { return Err("Cross-origin resources are not supported".into()); } suffix
    } else {
        if reference.starts_with("//") || reference.split('/').next().unwrap_or("").contains(':') { return Err("Unsupported resource URL".into()); } reference
    };
    let reference = reference.split('#').next().unwrap_or(reference);
    let base_resource_path = base_path;
    let base_path = base_path.split('?').next().unwrap_or(base_path);
    let joined = if reference.is_empty() { String::from(base_resource_path) }
        else if reference.starts_with('/') { String::from(reference) }
        else if reference.starts_with('?') { format!("{}{}", if base_path.is_empty() { "/" } else { base_path }, reference) }
        else { format!("{}/{}", base_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or(""), reference) };
    let (path, query) = joined.split_once('?').map(|(p,q)| (p,Some(q))).unwrap_or((&joined,None));
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') { match part { "" | "." => {}, ".." => { parts.pop(); }, _ => parts.push(part) } }
    let trailing = path.ends_with('/') && !parts.is_empty();
    let mut result = format!("{}/{}{}", origin, parts.join("/"), if trailing { "/" } else { "" });
    if let Some(query) = query { result.push('?'); result.push_str(query); }
    Ok(result)
}

fn cached(url: &str, limit: usize) -> Result<Arc<Vec<u8>>, String> {
    let active = ACTIVE.lock();
    let entry = active.entries.iter().find(|entry| entry.url == url).ok_or_else(|| format!("Resource was not prefetched: {}", url))?;
    let bytes = entry.data.as_ref().map_err(Clone::clone)?;
    if bytes.len() > limit { return Err("Resource exceeds type-specific byte budget".into()); }
    Ok(bytes.clone())
}
pub fn load_text(base: &str, reference: &str) -> Result<String, String> {
    let url = resolve(base, reference)?;
    let bytes = cached(&url, MAX_RESOURCE_BYTES)?;
    core::str::from_utf8(&bytes).map(String::from).map_err(|_| "Resource is not UTF-8".into())
}
pub fn load_binary(base: &str, reference: &str) -> Result<Arc<Vec<u8>>, String> { cached(&resolve(base, reference)?, MAX_IMAGE_BYTES) }

pub(crate) fn local_path(url: &str) -> Result<String, String> {
    let path = url.strip_prefix("file://").unwrap_or(url);
    if path.contains("://") { return Err("Unsupported local URL scheme".into()); }
    if url.starts_with("file://") && !path.starts_with('/') { return Err("File URL must use an absolute local path; remote authorities unsupported".into()); }
    let path = path.split(|c| c == '?' || c == '#').next().unwrap_or(path);
    if path.is_empty() || path.len() > 2048 { return Err("Invalid file path".into()); }
    let mut decoded = Vec::with_capacity(path.len()); let bytes = path.as_bytes(); let mut index = 0;
    while index < bytes.len() {
        let byte = if bytes[index] == b'%' {
            if index + 2 >= bytes.len() { return Err("Invalid file URL escape".into()); }
            let hex = core::str::from_utf8(&bytes[index + 1..index + 3]).map_err(|_| "Invalid file URL escape")?;
            index += 3; u8::from_str_radix(hex, 16).map_err(|_| "Invalid file URL escape")?
        } else { let byte = bytes[index]; index += 1; byte };
        if byte == 0 || byte < 32 || byte == 127 { return Err("Invalid file path character".into()); } decoded.push(byte);
    }
    String::from_utf8(decoded).map_err(|_| "File path is not UTF-8".into())
}
pub(crate) fn read_local(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let path = local_path(url)?;
    let mut file = ostd::fs::File::open(&path).map_err(|_| format!("Cannot open {}", path))?;
    let mut bytes = Vec::new(); let mut chunk = [0u8; 4096];
    loop {
        let count = file.read(&mut chunk).map_err(|_| format!("Cannot read {}", path))?;
        if count == 0 { return Ok(bytes); }
        if count > limit.saturating_sub(bytes.len()) { return Err(format!("File exceeds {} byte budget", limit)); }
        let needed = bytes.len() + count;
        if needed > bytes.capacity() {
            let capacity = needed.next_power_of_two().min(limit);
            bytes.reserve_exact(capacity - bytes.len());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

pub(crate) struct Reference { pub url: String, pub image: bool }
fn push_reference(refs: &mut Vec<Reference>, base: &str, reference: &str, image: bool) -> Result<(), String> {
    let Ok(url) = resolve(base, reference) else { return Ok(()); }; // Cross-origin references are deliberately not fetched.
    if let Some(index) = refs.iter().position(|item| item.url == url) {
        if image && !refs[index].image {
            if refs.iter().filter(|item| item.image).count() >= crate::budget::MAX_IMAGES { return Err("Document exceeds image resource budget".into()); }
            refs[index].image = true;
        }
        return Ok(());
    }
    if refs.len() >= MAX_REFERENCES { return Err("Document exceeds resource reference budget".into()); }
    if image && refs.iter().filter(|item| item.image).count() >= crate::budget::MAX_IMAGES { return Err("Document exceeds image resource budget".into()); }
    refs.push(Reference { url, image }); Ok(())
}
/// Discover only references the supported format actually activates.
pub(crate) fn references(base: &str, content: &str) -> Result<Vec<Reference>, String> {
    let mut refs = Vec::new();
    match crate::parser::DocFormat::detect_from_url_or_content(base, content) {
        crate::parser::DocFormat::Html => {
            for (reference, image) in crate::parser::html::resource_references_checked(content)? {
                push_reference(&mut refs, base, &reference, image)?;
            }
        }
        crate::parser::DocFormat::Markdown => {
            let mut fence: Option<char> = None;
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("```") || line.starts_with("~~~") {
                    let marker = line.chars().next().unwrap();
                    if fence == Some(marker) { fence = None; }
                    else if fence.is_none() { fence = Some(marker); }
                    continue;
                }
                if fence.is_some() || !line.starts_with("![") || !line.ends_with(')') { continue; }
                if let Some(end) = line.find("](") { push_reference(&mut refs, base, &line[end + 2..line.len() - 1], true)?; }
            }
        }
        _ => {},
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn resolution_preserves_queries_not_fragments_and_origin() {
        assert_eq!(resolve("https://example.com/a/b.html?q=1#x", "../js/app.js?v=2").unwrap(), "https://example.com/js/app.js?v=2");
        assert_eq!(resolve("file:///data/web/index.html", "./style.css").unwrap(), "file:///data/web/style.css");
        assert_eq!(resolve("https://example.com", "?q=1").unwrap(), "https://example.com/?q=1");
        for url in ["https://example.com.evil/x", "https://evil/x", "//evil/x", "javascript:alert(1)", "data:text/plain,x"] { assert!(resolve("https://example.com/a.html", url).is_err()); }
    }
    #[test] fn local_paths_strip_url_suffix_before_decoding() {
        assert_eq!(local_path("file:///data/a%20b.md?q=x#toc").unwrap(), "/data/a b.md");
        assert_eq!(local_path("/data/a%3Fb.md#toc").unwrap(), "/data/a?b.md");
        for path in ["file://remote/file", "file:///a%00b", "file:///a%ZZ"] { assert!(local_path(path).is_err()); }
    }
    #[test] fn static_resources_are_deduplicated_and_cross_origin_excluded() {
        let refs = references("https://host/a/index.html", "<!-- <img src='ignored'> --><img src='../pic.png'><script src=app.js></script><link rel='stylesheet' href='s.css'><img src='https://evil/pic'><img src='../pic.png'> ![p](other.jpg)").unwrap();
        assert_eq!(refs.len(), 3); assert_eq!(refs[0].url, "https://host/pic.png");
        assert_eq!(refs.iter().filter(|r| r.image).count(), 1);
        let refs = references("https://host/a/readme.md", "```\n![inert](bad.png)\n```\n~~~html\n<img src='bad.png'>\n~~~\n![active](good.png)\n").unwrap();
        assert_eq!(refs.len(), 1); assert_eq!(refs[0].url, "https://host/a/good.png");
    }
    #[test] fn cache_missing_and_error_are_not_truncated_success() {
        let mut bundle = ResourceBundle::default();
        bundle.insert("https://host/a".into(), Err("Truncated response".into()), false).unwrap();
        assert!(bundle.resident_bytes() > 0);
        assert_eq!(bundle.warnings().collect::<Vec<_>>(), alloc::vec!["Truncated response"]);
    }
    #[test]
    fn decoded_resource_budget_is_preflighted_and_shared_bytes_are_not_cloned() {
        let mut header = alloc::vec![0; 26]; header[..2].copy_from_slice(b"BM");
        header[18..22].copy_from_slice(&1024i32.to_le_bytes());
        header[22..26].copy_from_slice(&1024i32.to_le_bytes());
        let mut bundle = ResourceBundle::default();
        bundle.insert("file:///one.bmp".into(), Ok(header.clone()), true).unwrap();
        assert_eq!(bundle.estimated_decoded_bytes().unwrap(), 4 * 1024 * 1024);
        let bytes = bundle.entries[0].data.as_ref().unwrap();
        let shared = bytes.clone(); assert!(Arc::ptr_eq(bytes, &shared));
        bundle.insert("file:///two.bmp".into(), Ok(header), true).unwrap();
        assert!(bundle.estimated_decoded_bytes().is_err());
    }
}
