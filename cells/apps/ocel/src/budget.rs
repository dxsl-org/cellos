// SPDX-License-Identifier: MIT
//! Allocation limits shared by acquisition, parsers, and tab residency.
use alloc::string::String;

pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_RESOURCE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_DOM_NODES: usize = 16_384;
pub const MAX_DOM_DEPTH: usize = 64;
pub const MAX_CSS_RULES: usize = 1024;
pub const MAX_CSS_SELECTORS: usize = 4096;
pub const MAX_SELECTOR_BYTES: usize = 512;
pub const MAX_IMAGES: usize = 16;
pub const MAX_IMAGE_PIXELS: usize = 1024 * 1024;
pub const MAX_IMAGE_TOTAL_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TABS: usize = 4;
pub const MAX_RESIDENT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TABLE_ROWS: usize = 1024;
pub const MAX_TABLE_CELLS: usize = 4096;

pub fn validate_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_TEXT_BYTES { Err(String::from("Document exceeds text byte budget")) } else { Ok(()) }
}

pub fn validate_css(css: &str) -> Result<(), String> {
    validate_text(css)?;
    let mut rules=0usize;let mut selectors=0usize;let mut depth=0usize;
    let mut start=0usize;let mut quote=None;let mut escaped=false;let mut comment=false;
    let bytes=css.as_bytes();let mut i=0;
    while i<bytes.len() {
        let c=bytes[i];
        if comment {if c==b'*'&&bytes.get(i+1)==Some(&b'/'){comment=false;i+=2;}else{i+=1;}continue;}
        if escaped{escaped=false;i+=1;continue;}
        if c==b'\\'{escaped=true;i+=1;continue;}
        if let Some(q)=quote{if c==q{quote=None;}i+=1;continue;}
        if c==b'/'&&bytes.get(i+1)==Some(&b'*'){comment=true;i+=2;continue;}
        if c==b'\''||c==b'"'{quote=Some(c);i+=1;continue;}
        if c==b'{' {
            rules+=1;depth+=1;
            for selector in css[start..i].split(',') {
                selectors+=1;
                if selector.trim().len()>MAX_SELECTOR_BYTES{return Err(String::from("CSS selector exceeds byte budget"));}
            }
            if rules>MAX_CSS_RULES||selectors>MAX_CSS_SELECTORS{return Err(String::from("CSS exceeds rule/selector budget"));}
            if depth>MAX_DOM_DEPTH{return Err(String::from("CSS exceeds nesting depth budget"));}
            start=i+1;
        }else if c==b'}'{depth=depth.saturating_sub(1);start=i+1;}else if c==b';'&&depth==0{start=i+1;}
        i+=1;
    }
    Ok(())
}

/// Match the tokenizer's stack without allocating tokens or attacker-sized stacks.
pub fn validate_html(text: &str) -> Result<(), String> {
    validate_text(text)?;
    let mut stack=["";MAX_DOM_DEPTH];let mut depth=0usize;let mut nodes=1usize;
    let mut rest=text;let mut images=0usize;
    while let Some(open)=rest.find('<') {
        if open>0{nodes+=1;}
        rest=&rest[open+1..];
        if let Some(comment)=rest.strip_prefix("!--") {
            rest=comment.find("-->").map_or("",|end|&comment[end+3..]);continue;
        }
        let mut quote=None;let mut close=None;
        for (index,c) in rest.char_indices() {
            if quote==Some(c){quote=None;}else if quote.is_none()&&(c=='\''||c=='"'){quote=Some(c);}
            else if c=='>'&&quote.is_none(){close=Some(index);break;}
        }
        let end=close.unwrap_or(rest.len());let tag=rest[..end].trim();
        rest=if end<rest.len(){&rest[end+1..]}else{""};
        if let Some(name)=tag.strip_prefix('/') {
            if let Some(position)=stack[..depth].iter().rposition(|open|open.eq_ignore_ascii_case(name.trim())){depth=position;}
        }else{
            let name=tag.split_whitespace().next().unwrap_or("").trim_end_matches('/');
            let self_closing=tag.ends_with('/');
            if name.eq_ignore_ascii_case("script")&&!self_closing {
                let end=rawtext_end(rest,name);
                rest=end.map_or("",|(_,end)|&rest[end..]);continue;
            }
            nodes+=1;
            if name.eq_ignore_ascii_case("img"){images+=1;if images>MAX_IMAGES{return Err(String::from("Document exceeds image count budget"));}}
            let void=["hr","br","img","input","meta","link"].iter().any(|void|name.eq_ignore_ascii_case(void));
            if !self_closing&&!void {
                if depth==MAX_DOM_DEPTH{return Err(String::from("HTML exceeds DOM depth budget"));}
                stack[depth]=name;depth+=1;
            }
            if !self_closing&&["style","textarea","title","xmp","iframe","noembed","noframes"].iter().any(|raw|name.eq_ignore_ascii_case(raw)) {
                let end=rawtext_end(rest,name);
                let body=end.map_or(rest,|(start,_)|&rest[..start]);
                if name.eq_ignore_ascii_case("style"){validate_css(body)?;}nodes+=1;
                rest=end.map_or("",|(_,end)|&rest[end..]);depth=depth.saturating_sub(1);
            }
        }
        if nodes>MAX_DOM_NODES{return Err(String::from("HTML exceeds DOM node budget"));}
    }
    if nodes+usize::from(!rest.is_empty())>MAX_DOM_NODES{return Err(String::from("HTML exceeds DOM node budget"));}
    Ok(())
}

/// Header-only inspection: reserve the aggregate decoded bytes before decoding.
pub fn image_bytes(data: &[u8]) -> Result<usize, String> {
    use zune_core::{bytestream::ZCursor, options::DecoderOptions};
    let dimensions = if data.starts_with(b"BM") && data.len() >= 26 {
        let w = i32::from_le_bytes(data[18..22].try_into().unwrap());
        let h = i32::from_le_bytes(data[22..26].try_into().unwrap());
        if w <= 0 || h == i32::MIN { None } else { Some((w as usize, h.unsigned_abs() as usize)) }
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut decoder = zune_png::PngDecoder::new_with_options(ZCursor::new(data), DecoderOptions::new_safe());
        decoder.decode_headers().ok().and_then(|_| decoder.dimensions())
    } else if data.starts_with(b"\xff\xd8") {
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), DecoderOptions::new_safe());
        decoder.decode_headers().ok().and_then(|_| decoder.dimensions())
    } else { None };
    let (w,h) = dimensions.ok_or_else(|| String::from("Unsupported or invalid image"))?;
    let pixels = w.checked_mul(h).filter(|p| *p > 0 && *p <= MAX_IMAGE_PIXELS).ok_or_else(|| String::from("Image exceeds pixel budget"))?;
    Ok(pixels * 4)
}

/// Number of characters after an already-consumed '<' in a raw-text end tag.
/// HTML whitespace is permitted after the case-insensitive tag name.
pub fn rawtext_close_chars(mut chars:impl Iterator<Item=char>,name:&str)->Option<usize> {
    if chars.next()!=Some('/'){return None;}
    let mut count=1;
    for expected in name.chars(){let actual=chars.next()?;count+=1;if !actual.eq_ignore_ascii_case(&expected){return None;}}
    loop {let c=chars.next()?;count+=1;
        if c=='>'{return Some(count);}
        if !matches!(c,' '|'\t'|'\n'|'\r'|'\u{c}'){return None;}
    }
}

pub fn rawtext_end(text:&str,name:&str)->Option<(usize,usize)> {
    for (index,c) in text.char_indices(){if c=='<' {
        if let Some(count)=rawtext_close_chars(text[index+1..].chars(),name){return Some((index,index+1+count));}
    }}
    None
}

/// Count nested table allocation units before allocating Markdown nodes/cells.
pub fn validate_markdown(text:&str)->Result<(),String> {
    validate_text(text)?;
    let mut rows=0usize;let mut cells=0usize;let mut fenced=false;
    for line in text.lines() {
        let line=line.trim();
        if line.starts_with("```"){fenced=!fenced;continue;}
        if fenced{continue;}
        if line.starts_with('|')&&line.ends_with('|')&&line.len()>1 {
            rows+=1;cells=cells.saturating_add(line.trim_start_matches('|').trim_end_matches('|').split('|').count());
            if rows>MAX_TABLE_ROWS||cells>MAX_TABLE_CELLS{return Err(String::from("Markdown exceeds table row/cell budget"));}
        }
    }
    Ok(())
}
