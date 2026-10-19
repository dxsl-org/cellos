// SPDX-License-Identifier: MIT
//! Per-document, bounded browser I/O. JavaScript never receives transport authority.
extern crate alloc;
use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use core::task::Poll;
use dom_arena::{BrowserReply, BrowserRequest, BrowserResourceKind};
use crate::{budget, net::http::{FetchJob, FetchResult, Url}, resources};

const MAX_PENDING: usize = 64;
const MAX_ACTIVE: usize = 4;
const MAX_REQUEST_BYTES: usize = 128 * 1024;
const MAX_BODY: usize = 500 * 1024;
const MAX_COOKIE_BYTES: usize = 16 * 1024;
const MAX_COOKIES: usize = 64;
const MAX_REDIRECTS: usize = 5;

fn now_ms() -> u64 { ostd::syscall::sys_get_time_ms().unwrap_or(u64::MAX) }
fn wall_secs() -> u64 { ostd::syscall::sys_get_wall_secs() }
fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|prefix| prefix.ends_with('.'))
}
fn path_matches(path: &str, cookie: &str) -> bool {
    path == cookie || path.strip_prefix(cookie).is_some_and(|tail| cookie.ends_with('/') || tail.starts_with('/'))
}

#[derive(Clone)]
struct Cookie { name: String, value: String, domain: String, path: String, host_only: bool, secure: bool, http_only: bool, expires: Option<u64> }
impl Cookie { fn bytes(&self) -> usize { self.name.capacity() + self.value.capacity() + self.domain.capacity() + self.path.capacity() } }
/// Bounded, in-memory cookies; transferable between successful navigations in one tab.
#[derive(Clone, Default)]
pub struct CookieJar { entries: Vec<Cookie> }
impl CookieJar {
    pub fn resident_bytes(&self) -> usize { self.entries.capacity() * core::mem::size_of::<Cookie>() + self.entries.iter().map(Cookie::bytes).sum::<usize>() }
    fn header(&self, url: &Url, script: bool, now: u64) -> String {
        let path = url.path.split('?').next().unwrap_or("/");
        let mut matches: Vec<&Cookie> = self.entries.iter().filter(|c| (!script || !c.http_only) && (!c.secure || url.scheme == "https")
            && (if c.host_only { url.host == c.domain } else { domain_matches(&url.host, &c.domain) })
            && path_matches(path, &c.path) && c.expires.is_none_or(|expiry| expiry > now)).collect();
        matches.sort_by(|a,b| b.path.len().cmp(&a.path.len()));
        let mut value = String::new();
        for cookie in matches { if !value.is_empty() { value.push_str("; "); } value.push_str(&cookie.name); value.push('='); value.push_str(&cookie.value); }
        value
    }
    fn store(&mut self, url: &Url, value: &str, script: bool, now: u64, wall: u64) -> Result<(), String> {
        if value.len() > 4096 || value.bytes().any(|b| b < 32 || b == 127) { return Err("Invalid/overlong cookie".into()); }
        let mut fields = value.split(';');
        let (name, value) = fields.next().unwrap_or("").split_once('=').ok_or("Cookie lacks value")?;
        let name = name.trim(); let value = value.trim();
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            || !value.bytes().all(|b| b == 0x21 || (0x23..=0x2b).contains(&b) || (0x2d..=0x3a).contains(&b) || (0x3c..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b)) {
            return Err("Invalid cookie name/value".into());
        }
        let request_path = url.path.split('?').next().unwrap_or("/");
        let default_path = request_path.rsplit_once('/').map(|(prefix,_)| if prefix.is_empty() { "/" } else { prefix }).unwrap_or("/");
        let mut cookie = Cookie { name: name.into(), value: value.into(), domain: url.host.clone(), path: default_path.into(), host_only: true, secure: false, http_only: false, expires: None };
        let mut max_age = None; let mut expires = None;
        for field in fields {
            let (key, val) = field.trim().split_once('=').unwrap_or((field.trim(), "")); let val = val.trim();
            if key.eq_ignore_ascii_case("domain") {
                let domain = val.trim_start_matches('.').to_ascii_lowercase();
                if domain.is_empty() || domain.ends_with('.') || !domain_matches(&url.host, &domain)
                    || resources::registrable_domain(&domain).is_none()
                    || !resources::is_same_site(&url.host,&domain) { return Err("Cookie Domain is not permitted".into()); }
                cookie.domain = domain; cookie.host_only = false;
            } else if key.eq_ignore_ascii_case("path") && val.starts_with('/') { cookie.path = val.into(); }
            else if key.eq_ignore_ascii_case("secure") { cookie.secure = true; }
            else if key.eq_ignore_ascii_case("httponly") { if script { return Err("Script cannot set HttpOnly cookie".into()); } cookie.http_only = true; }
            else if key.eq_ignore_ascii_case("max-age") { max_age = val.parse::<i64>().ok(); }
            else if key.eq_ignore_ascii_case("expires") { expires = cookie_date(val); }
        }
        if cookie.secure && url.scheme != "https" || name.starts_with("__Secure-") && !cookie.secure
            || name.starts_with("__Host-") && (!cookie.secure || !cookie.host_only || cookie.path != "/") { return Err("Cookie secure policy rejected".into()); }
        if let Some(age) = max_age { cookie.expires = Some(if age <= 0 { now } else { now.saturating_add((age as u64).saturating_mul(1000)) }); }
        else if let Some(expiry) = expires { if wall == 0 { return Err("Cookie expiry requires wall clock".into()); } cookie.expires = Some(now.saturating_add(expiry.saturating_sub(wall).saturating_mul(1000))); }
        self.entries.retain(|c| c.expires.is_none_or(|expiry| expiry > now));
        if self.entries.iter().any(|c| c.name == name && (script && c.http_only || url.scheme != "https" && c.secure)
            && domain_matches(&url.host, &c.domain) && path_matches(&cookie.path, &c.path)) { return Err("Protected cookie cannot be overwritten".into()); }
        let previous = self.entries.iter().position(|c| c.name == cookie.name && c.domain == cookie.domain && c.path == cookie.path);
        if cookie.expires.is_some_and(|expiry| expiry <= now) { if let Some(i) = previous { self.entries.remove(i); } return Ok(()); }
        let old = previous.map(|i| self.entries[i].bytes()).unwrap_or(0);
        let bytes = self.entries.iter().map(Cookie::bytes).sum::<usize>();
        if bytes.saturating_sub(old).saturating_add(cookie.bytes()) > MAX_COOKIE_BYTES || previous.is_none() && self.entries.len() >= MAX_COOKIES { return Err("Cookie jar budget exceeded".into()); }
        if let Some(i) = previous { self.entries[i] = cookie; } else { self.entries.push(cookie); }
        Ok(())
    }
}
// RFC 6265 date tokens, including legacy cookie separators and two-digit years.
fn cookie_date(value: &str) -> Option<u64> {
    let mut time = None; let mut day = None; let mut month = None; let mut year = None;
    for token in value.split(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | '-' | '/')) {
        if token.is_empty() { continue; }
        if time.is_none() && token.contains(':') {
            let parts: Vec<&str> = token.split(':').collect();
            if parts.len() == 3 { time = Some((parts[0].parse::<u64>().ok()?, parts[1].parse::<u64>().ok()?, parts[2].parse::<u64>().ok()?)); }
        } else if month.is_none() && token.len() >= 3 {
            month = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"].iter().position(|m| token.get(..3).is_some_and(|prefix| prefix.eq_ignore_ascii_case(m))).map(|m| m as u64 + 1);
            if month.is_some() { continue; }
            if token.bytes().all(|b| b.is_ascii_digit()) { if day.is_none() && token.len() <= 2 { day = token.parse::<u64>().ok(); } else { year = token.parse::<u64>().ok(); } }
        } else if token.bytes().all(|b| b.is_ascii_digit()) { if day.is_none() && token.len() <= 2 { day = token.parse::<u64>().ok(); } else { year = token.parse::<u64>().ok(); } }
    }
    let (h,m,s) = time?; let day = day?; let month = month?; let mut year = year?;
    if year < 70 { year += 2000; } else if year < 100 { year += 1900; }
    if year < 1601 || year > 9999 || day == 0 || h > 23 || m > 59 || s > 59 { return None; }
    fn leap(y:u64)->bool { y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) }
    let days_in = [31, if leap(year) {29} else {28},31,30,31,30,31,31,30,31,30,31];
    if day > days_in[(month - 1) as usize] { return None; }
    if year < 1970 { return Some(0); }
    let mut days = 0; for y in 1970..year { days += if leap(y) {366} else {365}; }
    days += days_in[..(month - 1) as usize].iter().sum::<u64>() + day - 1;
    Some(((days * 24 + h) * 60 + m) * 60 + s)
}

pub struct BrowserCompletion { pub request: BrowserRequest, pub reply: BrowserReply, pub bytes: Vec<u8> }
struct Pending { request: BrowserRequest, url: String, visited: Vec<String>, get: bool, job: Option<FetchJob> }
impl Pending {
    fn id(&self) -> u32 { match &self.request { BrowserRequest::Load{id,..} | BrowserRequest::Fetch{id,..} => *id, _ => 0 } }
    fn bytes(&self) -> usize { request_bytes(&self.request) + self.url.capacity() + self.visited.capacity() * core::mem::size_of::<String>() + self.visited.iter().map(String::capacity).sum::<usize>() + self.job.as_ref().map_or(0, FetchJob::resident_bytes) }
}
fn request_bytes(request: &BrowserRequest) -> usize {
    match request {
        BrowserRequest::Load {url,..} => url.capacity(),
        BrowserRequest::Fetch {url,method,headers,body,credentials,..} => url.capacity() + method.capacity() + body.capacity() + credentials.capacity() + headers.capacity() * core::mem::size_of::<(String,String)>() + headers.iter().map(|(n,v)| n.capacity()+v.capacity()).sum::<usize>(),
        BrowserRequest::SetCookie{value} => value.capacity(), BrowserRequest::Abort{..} => 0,
    }
}
fn failure(request: BrowserRequest, url: String, error: String) -> BrowserCompletion {
    let id = match &request { BrowserRequest::Load{id,..} | BrowserRequest::Fetch{id,..} => *id, _ => 0 };
    BrowserCompletion { request, reply: BrowserReply {id,url,status:0,headers:Vec::new(),body:String::new(),error:Some(error)}, bytes:Vec::new() }
}

pub struct BrowserSession {
    document_url: String, cookies: CookieJar, queue: VecDeque<Pending>, active: Vec<Pending>,
    memory_budget: usize, accepted: usize, cursor: usize,
}
impl BrowserSession {
    pub fn new(document_url: &str) -> Self { Self {document_url:document_url.into(),cookies:CookieJar::default(),queue:VecDeque::new(),active:Vec::with_capacity(MAX_ACTIVE),memory_budget:budget::MAX_RESOURCE_BYTES,accepted:0,cursor:0} }
    pub fn document_url(&self) -> &str { &self.document_url }
    pub fn cookies(&self) -> CookieJar { self.cookies.clone() }
    pub fn set_cookies(&mut self, cookies: CookieJar) { self.cookies = cookies; }
    pub fn accept_response_cookies(&mut self, url:&str, headers:&[(String,String)]) {
        if let Ok(url) = Url::parse(url) { for (name,value) in headers { if name.eq_ignore_ascii_case("set-cookie") { let _ = self.cookies.store(&url,value,false,now_ms(),wall_secs()); } } }
    }
    pub fn document_cookie(&self) -> String { Url::parse(&self.document_url).map(|url| self.cookies.header(&url,true,now_ms())).unwrap_or_default() }
    pub fn set_memory_budget(&mut self, bytes:usize) { self.memory_budget = bytes.min(budget::MAX_RESOURCE_BYTES); }
    pub fn resident_bytes(&self) -> usize {
        self.document_url.capacity() + self.cookies.resident_bytes() + self.queue.capacity() * core::mem::size_of::<Pending>()
            + self.active.capacity() * core::mem::size_of::<Pending>() + self.queue.iter().chain(self.active.iter()).map(Pending::bytes).sum::<usize>()
    }
    pub fn is_pending(&self) -> bool { !self.queue.is_empty() || !self.active.is_empty() }
    pub fn cancel_all(&mut self) { self.queue.clear(); self.active.clear(); }
    pub fn enqueue(&mut self, request: BrowserRequest) -> Result<(),String> {
        match &request {
            BrowserRequest::Abort{id} => {
                self.queue.retain(|p| p.id() != *id); self.active.retain(|p| p.id() != *id); return Ok(());
            }
            BrowserRequest::SetCookie{value} => {
                if self.memory_budget.saturating_sub(self.resident_bytes()) < 8192 { return Err("Cookie memory budget exceeded".into()); }
                let url = Url::parse(&self.document_url)?;
                return self.cookies.store(&url,value,true,now_ms(),wall_secs());
            }
            _ => {}
        }
        let (id,reference) = match &request { BrowserRequest::Load{id,url,..} | BrowserRequest::Fetch{id,url,..} => (*id,url.as_str()), _ => unreachable!() };
        if self.queue.iter().chain(self.active.iter()).any(|p| p.id() == id) { return Err("Duplicate pending browser request ID".into()); }
        if self.queue.len() + self.active.len() >= MAX_PENDING || self.accepted >= budget::MAX_REFERENCES { return Err("Browser request count budget exceeded".into()); }
        if request_bytes(&request) > MAX_REQUEST_BYTES { return Err("Browser request byte budget exceeded".into()); }
        let url = resources::resolve(&self.document_url, reference)?;
        validate_target(&self.document_url,&url)?;
        if url.starts_with("file://") && matches!(request,BrowserRequest::Fetch{..}) { return Err("Fetch requires an HTTP(S) document".into()); }
        if let BrowserRequest::Fetch{method,headers,body,credentials,..} = &request { validate_fetch(method,headers,body,credentials)?; }
        let capacity = if self.queue.len() == self.queue.capacity() { (self.queue.capacity().max(1) * 2).min(MAX_PENDING) } else { self.queue.capacity() };
        let growth = capacity.saturating_sub(self.queue.capacity()) * core::mem::size_of::<Pending>();
        let reserve = request_bytes(&request).saturating_add(url.capacity() * 2).saturating_add(core::mem::size_of::<String>()).saturating_add(growth);
        if reserve > self.memory_budget.saturating_sub(self.resident_bytes()) { return Err("Browser request exceeds remaining memory budget".into()); }
        if growth != 0 { self.queue.reserve_exact(capacity - self.queue.len()); }
        let visited = alloc::vec![url.clone()];
        self.queue.push_back(Pending {request,url,visited,get:false,job:None}); self.accepted += 1; Ok(())
    }
    /// At most four exact transport operations are advanced per call; one completion is returned.
    pub fn poll(&mut self) -> Option<BrowserCompletion> {
        while self.active.len() < MAX_ACTIVE { match self.queue.pop_front() { Some(pending) => self.active.push(pending), None => break } }
        let count = self.active.len();
        for _ in 0..count {
            if self.active.is_empty() { break; }
            let i = self.cursor % self.active.len(); self.cursor = i + 1;
            if self.active[i].job.is_none() {
                if self.active[i].url.starts_with("file://") {
                    let pending = self.active.remove(i);
                    let limit = response_limit(&pending.request);
                    let result = resources::read_local(&pending.url, limit.min(self.memory_budget.saturating_sub(self.resident_bytes()) / 2));
                    return Some(match result { Ok(body) => complete(&self.document_url,pending, FetchResult {url:String::new(),body,media_type:String::new(),gemini:false,status:200,headers:Vec::new()}), Err(error) => failure(pending.request,pending.url,error) });
                }
                let available = self.memory_budget.saturating_sub(self.resident_bytes());
                if available < 32 * 1024 {
                    let pending = self.active.remove(i); return Some(failure(pending.request,pending.url,"Browser transport memory budget exceeded".into()));
                }
                let result = self.start(&self.active[i], available);
                match result { Ok(job) => self.active[i].job = Some(job), Err(error) => { let pending = self.active.remove(i); return Some(failure(pending.request,pending.url,error)); } }
            }
            let other_bytes = self.resident_bytes().saturating_sub(self.active[i].job.as_ref().map_or(0,FetchJob::resident_bytes));
            let available = self.memory_budget.saturating_sub(other_bytes);
            let job = self.active[i].job.as_mut().unwrap(); job.set_memory_budget(available / 2);
            match job.poll() {
                Poll::Pending => {},
                Poll::Ready(result) => {
                    let mut pending = self.active.remove(i); pending.job = None;
                    match result {
                        Err(error) => return Some(failure(pending.request,pending.url,error)),
                        Ok(result) => {
                            let target = match Url::parse(&pending.url) { Ok(url) => url, Err(error) => return Some(failure(pending.request,pending.url,error)) };
                            let credentialed = credentials_allowed(&pending.request,&self.document_url,&target);
                            if credentialed { self.accept_response_cookies(&pending.url,&result.headers); }
                            if let BrowserRequest::Fetch{credentials,..} = &pending.request {
                                if let Err(error) = validate_cors(&self.document_url,&target,&result.headers,credentials == "include") { return Some(failure(pending.request,pending.url,error)); }
                            }
                            if matches!(result.status,301|302|303|307|308) {
                                match redirect_pending(&self.document_url,&mut pending,&result) {
                                    Ok(()) => { self.active.push(pending); },
                                    Err(error) => return Some(failure(pending.request,pending.url,error)),
                                }
                            } else { return Some(complete(&self.document_url,pending,result)); }
                        }
                    }
                }
            }
        }
        None
    }
    fn start(&self, pending:&Pending, available:usize) -> Result<FetchJob,String> {
        let target = Url::parse(&pending.url)?;
        let mut headers: Vec<(&str,&str)> = Vec::new(); let mut method = "GET"; let mut body:&[u8] = &[];
        let origin = if matches!(pending.request,BrowserRequest::Fetch{..}) { Url::parse(&self.document_url)?.origin() } else { String::new() };
        if let BrowserRequest::Fetch{method:m,headers:h,body:b,..} = &pending.request {
            if !pending.get { method = m; body = b.as_bytes(); }
            for (name,value) in h { if redirect_keeps_header(pending.get,name) { headers.push((name.as_str(),value.as_str())); } }
            if origin != target.origin() || method == "POST" { headers.push(("Origin",origin.as_str())); }
        }
        let cookie = if credentials_allowed(&pending.request,&self.document_url,&target) { self.cookies.header(&target,false,now_ms()) } else { String::new() };
        if !cookie.is_empty() { headers.push(("Cookie",cookie.as_str())); }
        let mut job = FetchJob::browser(&pending.url,response_limit(&pending.request).min(available / 2),method,&headers,body)?;
        job.set_memory_budget(available / 2); Ok(job)
    }
}
fn redirect_keeps_header(converted_to_get:bool,name:&str) -> bool {
    !converted_to_get || !["content-type","content-language"].iter().any(|header| name.eq_ignore_ascii_case(header))
}
impl Drop for BrowserSession { fn drop(&mut self) { self.cancel_all(); } }
fn validate_target(document:&str,target:&str) -> Result<(),String> {
    if document.starts_with("file://") { if target.starts_with("file://") { resources::local_path(target)?; return Ok(()); } return Err("Local document cannot load network resources".into()); }
    let base = Url::parse(document)?; let target = Url::parse(target)?;
    if !matches!(base.scheme.as_str(),"http"|"https") || !matches!(target.scheme.as_str(),"http"|"https") { return Err("Unsupported browser resource scheme".into()); }
    if base.scheme == "https" && target.scheme != "https" { return Err("Insecure mixed content blocked".into()); }
    if !resources::is_same_site(&base.host,&target.host) { return Err("Cross-site browser request blocked".into()); }
    Ok(())
}
fn validate_fetch(method:&str,headers:&[(String,String)],body:&str,credentials:&str) -> Result<(),String> {
    if !matches!(method,"GET"|"HEAD"|"POST") || method != "POST" && !body.is_empty() { return Err("Unsupported fetch method/body".into()); }
    if !matches!(credentials,"omit"|"same-origin"|"include") { return Err("Invalid fetch credentials mode".into()); }
    if body.len() > 64 * 1024 || headers.len() > 24 { return Err("Fetch request exceeds budget".into()); }
    let mut total = 0;
    for (name,value) in headers {
        total += name.len() + value.len();
        if total > 8192 || value.len() > 128 || value.bytes().any(|b| b < 32 || b == 127 || b"\"():<>?@[\\]{}".contains(&b)) { return Err("Unsafe/non-simple fetch header".into()); }
        match name.to_ascii_lowercase().as_str() {
            "accept" => {},
            "accept-language" | "content-language" if value.bytes().all(|b| b.is_ascii_alphanumeric() || b" *,-.;=".contains(&b)) => {},
            "content-type" if matches!(value.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str(),"application/x-www-form-urlencoded"|"multipart/form-data"|"text/plain") => {},
            _ => return Err("Fetch header requires unavailable CORS preflight or is forbidden".into()),
        }
    }
    Ok(())
}
fn credentials_allowed(request:&BrowserRequest,document:&str,target:&Url) -> bool {
    match request {
        BrowserRequest::Load{..} => true,
        BrowserRequest::Fetch{credentials,..} => credentials == "include" || credentials == "same-origin" && Url::parse(document).is_ok_and(|base| base.origin() == target.origin()),
        _ => false,
    }
}
fn one_header<'a>(headers:&'a [(String,String)],name:&str) -> Result<Option<&'a str>,String> {
    let mut found = None; for (key,value) in headers { if key.eq_ignore_ascii_case(name) { if found.is_some() { return Err("Duplicate CORS/redirect header".into()); } found = Some(value.as_str()); } } Ok(found)
}
fn validate_cors(document:&str,target:&Url,headers:&[(String,String)],credentials:bool) -> Result<(),String> {
    let origin = Url::parse(document)?.origin(); if origin == target.origin() { return Ok(()); }
    let allow = one_header(headers,"access-control-allow-origin")?.ok_or("Cross-origin response lacks CORS permission")?;
    if allow != origin && !(allow == "*" && !credentials) { return Err("Cross-origin response origin rejected".into()); }
    if credentials && one_header(headers,"access-control-allow-credentials")? != Some("true") { return Err("Cross-origin credentials rejected".into()); }
    Ok(())
}
fn exposed_headers(request:&BrowserRequest,document:&str,target:&Url,headers:Vec<(String,String)>) -> Vec<(String,String)> {
    let cross = matches!(request,BrowserRequest::Fetch{..}) && Url::parse(document).is_ok_and(|base| base.origin() != target.origin());
    let include = matches!(request,BrowserRequest::Fetch{credentials,..} if credentials == "include");
    let mut exposed = Vec::new();
    if cross { for (name,value) in &headers { if name.eq_ignore_ascii_case("access-control-expose-headers") { for token in value.split(',') { exposed.push(token.trim().to_ascii_lowercase()); } } } }
    headers.into_iter().filter(|(name,_)| !name.eq_ignore_ascii_case("set-cookie") && !name.eq_ignore_ascii_case("set-cookie2")
        && (!cross || ["cache-control","content-language","content-length","content-type","expires","last-modified","pragma"].contains(&name.as_str()) || exposed.iter().any(|item| item == name || item == "*" && !include))).collect()
}
fn redirect_pending(document:&str,pending:&mut Pending,result:&FetchResult) -> Result<(),String> {
    if pending.visited.len() > MAX_REDIRECTS { return Err("Too many browser redirects".into()); }
    let location = one_header(&result.headers,"location")?.ok_or("Redirect lacks Location")?;
    let next = Url::parse(&pending.url)?.redirect(location)?.full;
    validate_target(document,&next)?;
    if pending.visited.contains(&next) { return Err("Browser redirect loop".into()); }
    if let BrowserRequest::Fetch{method,..} = &pending.request { if result.status == 303 && method != "HEAD" || matches!(result.status,301|302) && method == "POST" { pending.get = true; } }
    pending.visited.push(next.clone()); pending.url = next; Ok(())
}
fn response_limit(request:&BrowserRequest) -> usize { match request { BrowserRequest::Load{kind:BrowserResourceKind::Image,..} => budget::MAX_ENCODED_IMAGE_BYTES, _ => MAX_BODY } }
fn lossy_length(mut bytes:&[u8]) -> usize {
    let mut size = 0usize;
    loop {
        match core::str::from_utf8(bytes) {
            Ok(text) => return size.saturating_add(text.len()),
            Err(error) => {
                size = size.saturating_add(error.valid_up_to()).saturating_add(3);
                match error.error_len() { Some(length) => bytes = &bytes[error.valid_up_to()+length..], None => return size }
            }
        }
    }
}
fn complete(document:&str,pending:Pending,mut result:FetchResult) -> BrowserCompletion {
    if result.url.is_empty() { result.url = pending.url.clone(); }
    let image = matches!(pending.request,BrowserRequest::Load{kind:BrowserResourceKind::Image,..});
    if matches!(pending.request,BrowserRequest::Load{..}) && !(200..300).contains(&result.status) { return failure(pending.request,result.url,format!("HTTP status {}",result.status)); }
    let strict = matches!(pending.request,BrowserRequest::Load{..});
    let headers = match Url::parse(&result.url) {
        Ok(target) => exposed_headers(&pending.request,document,&target,result.headers),
        Err(_) => Vec::new(),
    };
    let length = if image { 0 } else if strict { result.body.len() } else { lossy_length(&result.body) };
    let size = length + result.url.len() + headers.iter().map(|(n,v)|n.len()+v.len()+12).sum::<usize>() + 128;
    if length > MAX_BODY || size > dom_arena::OCEL_JS_MAX_TRANSFER { return failure(pending.request,result.url,"Browser reply exceeds wire budget".into()); }
    let body = if image { String::new() } else if strict {
        match String::from_utf8(core::mem::take(&mut result.body)) { Ok(body) => body, Err(_) => return failure(pending.request,result.url,"Browser resource is not UTF-8".into()) }
    } else {
        match String::from_utf8(core::mem::take(&mut result.body)) { Ok(body) => body, Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned() }
    };
    BrowserCompletion { reply:BrowserReply{id:pending.id(),url:result.url,status:result.status,headers,body,error:None},request:pending.request,bytes:if image {result.body} else {Vec::new()} }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn load(id:u32,url:&str) -> BrowserRequest { BrowserRequest::Load{id,node:None,url:url.into(),kind:BrowserResourceKind::Script} }
    fn fetch(id:u32) -> BrowserRequest { BrowserRequest::Fetch{id,url:"/api".into(),method:"GET".into(),headers:Vec::new(),body:String::new(),credentials:"same-origin".into()} }
    fn pending(request:BrowserRequest,url:&str) -> Pending { Pending {request,url:url.into(),visited:alloc::vec![url.into()],get:false,job:None} }
    fn response(url:&str,status:u16,headers:Vec<(String,String)>,body:Vec<u8>) -> FetchResult { FetchResult{url:url.into(),status,headers,body,media_type:String::new(),gemini:false} }

    #[test]
    fn request_policy_rejects_cross_site_mixed_content_and_header_injection() {
        assert!(validate_target("https://tuoitre.vn/","https://static-tuoitre.tuoitre.vn/a.js").is_ok());
        for target in ["https://evil.test/a","http://tuoitre.vn/a","file:///secret"] { assert!(validate_target("https://tuoitre.vn/",target).is_err()); }
        assert!(validate_target("file:///page.html","https://tuoitre.vn/a").is_err());
        assert!(validate_fetch("POST",&[("Content-Type".into(),"text/plain;charset=UTF-8".into())],"hello","include").is_ok());
        for header in [("Cookie","secret=yes"),("Origin","https://evil"),("Authorization","secret"),("X-Foo","bar"),("Accept","a\r\nInjected: b"),("Content-Type","application/json")] {
            assert!(validate_fetch("GET",&[(header.0.into(),header.1.into())],"","same-origin").is_err());
        }
        assert!(validate_fetch("POST",&[],&"x".repeat(64*1024+1),"omit").is_err());
    }
    #[test]
    fn cors_requires_exact_single_origin_and_credentials_grant() {
        let target = Url::parse("https://api.example.test/data").unwrap();
        assert!(validate_cors("https://example.test/",&target,&[],false).is_err());
        let wildcard = alloc::vec![("access-control-allow-origin".into(),"*".into())];
        assert!(validate_cors("https://example.test/",&target,&wildcard,false).is_ok());
        assert!(validate_cors("https://example.test/",&target,&wildcard,true).is_err());
        let mut exact = alloc::vec![("access-control-allow-origin".into(),"https://example.test".into())];
        assert!(validate_cors("https://example.test/",&target,&exact,true).is_err());
        exact.push(("access-control-allow-credentials".into(),"true".into()));
        assert!(validate_cors("https://example.test/",&target,&exact,true).is_ok());
        exact.push(("access-control-allow-origin".into(),"https://evil.test".into()));
        assert!(validate_cors("https://example.test/",&target,&exact,false).is_err());
    }
    #[test]
    fn cors_exposes_only_safe_headers_and_never_set_cookie() {
        let target = Url::parse("https://api.example.test/data").unwrap();
        let headers = alloc::vec![("content-type".into(),"text/plain".into()),("set-cookie".into(),"private=yes".into()),("x-private".into(),"secret".into()),("x-public".into(),"visible".into()),("access-control-expose-headers".into(),"X-Public, Set-Cookie".into())];
        let visible = exposed_headers(&fetch(1),"https://example.test/",&target,headers);
        assert_eq!(visible,alloc::vec![("content-type".into(),"text/plain".into()),("x-public".into(),"visible".into())]);
    }
    #[test]
    fn cookies_preserve_host_path_secure_http_only_and_domain_boundaries() {
        let url = Url::parse("https://www.example.com/news/item").unwrap();
        let mut jar = CookieJar::default();
        jar.store(&url,"host=a; Path=/news",false,1000,1).unwrap();
        jar.store(&url,"shared=b; Domain=example.com; Path=/; Secure",false,1000,1).unwrap();
        jar.store(&url,"secret=c; HttpOnly; Path=/",false,1000,1).unwrap();
        assert!(!jar.header(&url,true,1000).contains("secret"));
        assert!(jar.header(&url,false,1000).contains("secret=c"));
        assert_eq!(jar.header(&Url::parse("https://cdn.example.com/news").unwrap(),false,1000),"shared=b");
        assert!(!jar.header(&Url::parse("http://www.example.com/newspaper").unwrap(),false,1000).contains("host="));
        assert!(!jar.header(&Url::parse("http://www.example.com/news").unwrap(),false,1000).contains("shared="));
        for cookie in ["x=a; Domain=test","x=a; Domain=evil.test","x=a; Domain=example.test.evil","secret=overwrite; Path=/"] { assert!(jar.store(&url,cookie,true,1000,1).is_err()); }
        assert!(jar.store(&url,"x=a; HttpOnly",true,1000,1).is_err());
        assert!(jar.store(&Url::parse("http://www.example.com/").unwrap(),"shared=overwrite; Domain=example.com; Path=/",true,1000,1).is_err());
        assert!(jar.resident_bytes() <= MAX_COOKIE_BYTES + MAX_COOKIES * core::mem::size_of::<Cookie>());
    }
    #[test]
    fn cookies_expire_delete_and_transfer_without_leaking_to_other_hosts() {
        let url = Url::parse("https://example.test/").unwrap(); let mut jar = CookieJar::default();
        jar.store(&url,"x=one; Max-Age=1",true,1000,1).unwrap();
        assert_eq!(jar.header(&url,true,1999),"x=one"); assert_eq!(jar.header(&url,true,2000),"");
        jar.store(&url,"x=two",true,2000,1).unwrap();
        jar.store(&url,"x=; Max-Age=0",true,2001,1).unwrap();
        assert_eq!(jar.header(&url,true,2001),"");
        jar.store(&url,"keep=yes; HttpOnly",false,2001,1).unwrap();
        let transferred = jar.clone();
        assert!(transferred.header(&url,false,2001).contains("keep=yes"));
        assert_eq!(transferred.header(&Url::parse("https://evil.test/").unwrap(),false,2001),"");
        assert_eq!(cookie_date("Thu, 01 Jan 1970 00:00:00 GMT"),Some(0));
        assert_eq!(cookie_date("Wed, 01 Jan 2020 00:00:00 GMT"),Some(1_577_836_800));
        assert!(cookie_date("Wed, 31 Feb 2020 00:00:00 GMT").is_none());
    }
    #[test]
    fn queue_abort_reset_and_request_count_are_bounded_without_network() {
        let mut session = BrowserSession::new("https://example.test/");
        session.enqueue(load(1,"/one.js")).unwrap();
        assert!(session.enqueue(load(1,"/duplicate.js")).is_err());
        session.enqueue(BrowserRequest::Abort{id:1}).unwrap(); assert!(!session.is_pending());
        let mut session = BrowserSession::new("https://example.test/");
        for id in 2..=MAX_PENDING as u32+1 { session.enqueue(load(id,"/a.js")).unwrap(); }
        assert!(session.enqueue(load(1000,"/overflow.js")).is_err());
        session.cancel_all(); assert!(!session.is_pending());
        let mut local = BrowserSession::new("file:///page.html");
        assert!(local.enqueue(fetch(1)).is_err());
        assert!(local.enqueue(load(2,"https://example.test/a.js")).is_err());
        let mut constrained = BrowserSession::new("https://example.test/");
        constrained.set_memory_budget(0); assert!(constrained.enqueue(load(2000,"/budget.js")).is_err());
    }
    #[test]
    fn redirects_validate_each_hop_and_convert_post_without_replaying_body() {
        let mut request = fetch(1); if let BrowserRequest::Fetch{method,body,..} = &mut request { *method = "POST".into(); *body = "private".into(); }
        let mut p = pending(request,"https://example.com/api");
        let reply = response(&p.url,302,alloc::vec![("location".into(),"https://cdn.example.com/next".into())],Vec::new());
        redirect_pending("https://example.com/",&mut p,&reply).unwrap(); assert!(p.get);
        assert!(!redirect_keeps_header(p.get,"Content-Type")); assert!(!redirect_keeps_header(p.get,"CONTENT-LANGUAGE"));
        assert!(redirect_keeps_header(p.get,"Accept")); assert!(redirect_keeps_header(false,"Content-Type"));
        let escape = response(&p.url,302,alloc::vec![("location".into(),"https://evil.com/x".into())],Vec::new());
        assert!(redirect_pending("https://example.com/",&mut p,&escape).is_err());
        let insecure = response(&p.url,307,alloc::vec![("location".into(),"http://cdn.example.com/x".into())],Vec::new());
        assert!(redirect_pending("https://example.com/",&mut p,&insecure).is_err());
    }
    #[test]
    fn completion_preserves_real_status_binary_images_and_strict_script_utf8() {
        let result = complete("https://example.test/",pending(fetch(1),"https://example.test/api"),response("https://example.test/api",404,Vec::new(),b"missing".to_vec()));
        assert_eq!(result.reply.status,404); assert_eq!(result.reply.body,"missing"); assert!(result.reply.error.is_none());
        let result = complete("https://example.test/",pending(load(2,"/a.js"),"https://example.test/a.js"),response("https://example.test/a.js",200,Vec::new(),alloc::vec![0xff]));
        assert!(result.reply.error.is_some());
        let request = BrowserRequest::Load{id:3,node:None,url:"/a.png".into(),kind:BrowserResourceKind::Image};
        let result = complete("https://example.test/",pending(request,"https://example.test/a.png"),response("https://example.test/a.png",200,Vec::new(),alloc::vec![0xff,0x00]));
        assert_eq!(result.bytes,alloc::vec![0xff,0x00]); assert_eq!(result.reply.body,"");
        let result = complete("https://example.test/",pending(fetch(4),"https://example.test/api"),response("https://example.test/api",200,Vec::new(),alloc::vec![0xff]));
        assert_eq!(result.reply.body,"\u{fffd}");
        let oversized = alloc::vec![0xff;MAX_BODY/3+1];
        assert!(complete("https://example.test/",pending(fetch(5),"https://example.test/api"),response("https://example.test/api",200,Vec::new(),oversized)).reply.error.is_some());
    }
}
